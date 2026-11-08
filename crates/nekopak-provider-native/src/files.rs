//! Native file operations. Implementations are independent of source-reference clients.
use super::*;
use std::time::Duration;

impl NativeProvider {
    pub(super) async fn page_request(
        &self,
        scope: &str,
        mut url: url::Url,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<(Value, Option<String>), ClientError> {
        page_limit(Some(u32::try_from(limit).map_err(|_| wire::invalid())?))?;
        let previous = if let Some(cursor) = cursor {
            let session = self.session.lock().await;
            Some(
                session
                    .cursors
                    .iter()
                    .find(|c| {
                        c.id == cursor
                            && c.parent == scope
                            && c.limit == limit
                            && c.epoch == session.epoch
                    })
                    .cloned()
                    .ok_or_else(|| error(ErrorCode::Conflict, "分页已失效，请刷新列表。"))?,
            )
        } else {
            None
        };
        url.query_pairs_mut()
            .append_pair("limit", &limit.to_string());
        if let Some(previous) = &previous {
            url.query_pairs_mut()
                .append_pair("page_token", &previous.token);
        }
        let (value, epoch) = self.get(url.as_str()).await?;
        let next = wire::next_token(&value)?;
        let mut session = self.session.lock().await;
        if session.epoch != epoch || previous.as_ref().is_some_and(|p| p.epoch != epoch) {
            return Err(expired());
        }
        let next_cursor = if let Some(token) = next {
            let mut seen = previous.map(|p| p.seen).unwrap_or_default();
            if !seen.insert(token.clone())
                || seen.len() > 256
                || seen.iter().map(String::len).sum::<usize>() > 32768
            {
                return Err(error(
                    ErrorCode::IntegrityFailed,
                    "服务返回重复或异常游标，未接受不完整列表。",
                ));
            }
            let id = uuid::Uuid::new_v4().to_string();
            if session.cursors.len() >= 128 {
                session.cursors.pop_front();
            }
            session.cursors.push_back(Cursor {
                id: id.clone(),
                parent: scope.into(),
                limit,
                token,
                seen,
                epoch,
            });
            Some(id)
        } else {
            None
        };
        Ok((value, next_cursor))
    }

    pub(super) async fn native_query(
        &self,
        query: &FileQuery,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<Page<RemoteEntry>, ClientError> {
        let recent = *query == FileQuery::Recent;
        let trash = *query == FileQuery::Trash;
        let scope = serde_json::to_string(query).map_err(|_| wire::invalid())?;
        if let FileQuery::Search(q) = query {
            if q.is_empty() || q.len() > 512 || q.chars().any(char::is_control) {
                return Err(error(ErrorCode::Conflict, "请输入有效的文件名称。"));
            }
        }
        let mut url = url::Url::parse(&format!(
            "{}{}",
            self.http.drive_origin,
            if recent {
                "/drive/v1/events"
            } else {
                "/drive/v1/files"
            }
        ))
        .map_err(|_| wire::invalid())?;
        if !recent {
            let mut filters = json!({"trashed":{"eq":trash},"phase":{"eq":"PHASE_TYPE_COMPLETE"}});
            if *query == FileQuery::Starred {
                filters["system_tag"] = json!({"in":"STAR"});
            }
            url.query_pairs_mut()
                .append_pair("parent_id", "*")
                .append_pair("filters", &filters.to_string())
                .append_pair("with_audit", "true");
        }
        let (value, next_cursor) = self.page_request(&scope, url, cursor, limit).await?;
        let entries = value
            .get(if recent { "events" } else { "files" })
            .and_then(Value::as_array)
            .ok_or_else(wire::invalid)?;
        if entries.len() > limit {
            return Err(wire::invalid());
        }
        let mut items = Vec::new();
        let mut seen = BTreeSet::new();
        for value in entries {
            let mut metadata = if recent {
                let Some(reference) = value.get("reference_resource").filter(|v| v.is_object())
                else {
                    continue;
                };
                if value
                    .get("file_id")
                    .and_then(Value::as_str)
                    .is_some_and(|id| reference.get("id").and_then(Value::as_str) != Some(id))
                {
                    return Err(wire::invalid());
                }
                let mut reference = reference.clone();
                if let Some(time) = value
                    .get("created_time")
                    .or_else(|| value.get("updated_time"))
                {
                    reference["modified_time"] = time.clone();
                }
                reference
            } else {
                value.clone()
            };
            // Starred listing is authoritative even when a response omits the tags projection.
            if *query == FileQuery::Starred {
                metadata["starred"] = true.into();
            }
            let item = wire::entry_in(&metadata, trash)?;
            if !seen.insert(item.id.clone()) {
                if recent {
                    continue;
                } else {
                    return Err(wire::invalid());
                }
            }
            if let FileQuery::Search(q) = query {
                if !item.name.to_lowercase().contains(&q.to_lowercase()) {
                    continue;
                }
            }
            items.push(item);
        }
        Ok(Page { items, next_cursor })
    }

    pub(super) async fn native_mutate(
        &self,
        request: &FileMutation,
        account_scope: &str,
    ) -> Result<OperationResult, ClientError> {
        let snapshot = self.snapshot().await?;
        let subject = self.drive_subject(&snapshot).await?;
        if format!(
            "{:x}",
            Sha256::digest(format!("{}\0{}", self.profile()?.id, subject))
        ) != account_scope
        {
            return Err(expired());
        }
        let operation = request.op;
        if request.ids.len() > 500
            || request.ids.iter().collect::<BTreeSet<_>>().len() != request.ids.len()
        {
            return Err(error(
                ErrorCode::Conflict,
                "每次操作最多选择 500 个不同项目。",
            ));
        }
        for id in &request.ids {
            wire::validate_id(id)?;
            if id == "root" {
                return Err(error(ErrorCode::Conflict, "不能修改网盘根目录。"));
            }
        }
        let target = request.target.clone().unwrap_or_default();
        let name = target.name.as_deref().unwrap_or("");
        let parent = target.parent_id.as_deref().unwrap_or("root");
        wire::validate_id(parent)?;
        if matches!(operation, MutateOp::Mkdir | MutateOp::Rename)
            && (name.trim().is_empty()
                || name.len() > 1024
                || name.contains(['/', '\\'])
                || name.chars().any(char::is_control))
        {
            return Err(error(ErrorCode::Conflict, "名称为空或含不支持的字符。"));
        }
        if operation != MutateOp::Mkdir && request.ids.is_empty() {
            return Err(error(ErrorCode::Conflict, "请选择要操作的项目。"));
        }
        if operation == MutateOp::Mkdir && !request.ids.is_empty() {
            return Err(error(ErrorCode::Conflict, "新建文件夹不接受已有文件标识。"));
        }
        if operation == MutateOp::Rename && request.ids.len() != 1 {
            return Err(error(ErrorCode::Conflict, "每次重命名一个项目。"));
        }
        if matches!(operation, MutateOp::Move | MutateOp::Copy)
            && request.ids.iter().any(|id| id == parent)
        {
            return Err(error(ErrorCode::Conflict, "不能移动或复制到自身。"));
        }
        if operation == MutateOp::DeleteForever {
            // The UI confirms the destructive intent. Independently restrict
            // this protocol to items already in the remote recycle bin.
            for id in &request.ids {
                let (value, _) = self
                    .drive_request_at(
                        Method::GET,
                        &format!("{}/drive/v1/files/{id}", self.http.drive_origin),
                        None,
                        Some(snapshot.epoch),
                    )
                    .await?;
                if value.get("trashed").and_then(Value::as_bool) != Some(true)
                    || wire::entry_in(&value, true)?.id != *id
                {
                    return Err(error(
                        ErrorCode::Conflict,
                        "永久删除只允许已经移入回收站的项目。",
                    ));
                }
            }
        }
        let remote_parent = if parent == "root" { "" } else { parent };
        let (method, path, body) = match operation {
            MutateOp::Mkdir => (
                Method::POST,
                "/drive/v1/files".into(),
                json!({"kind":"drive#folder","name":name,"parent_id":remote_parent}),
            ),
            MutateOp::Rename => (
                Method::PATCH,
                format!("/drive/v1/files/{}", request.ids[0]),
                json!({"name":name}),
            ),
            MutateOp::Move | MutateOp::Copy => (
                Method::POST,
                format!(
                    "/drive/v1/files:{}",
                    if operation == MutateOp::Move {
                        "batchMove"
                    } else {
                        "batchCopy"
                    }
                ),
                json!({"ids":request.ids,"to":{"parent_id":remote_parent}}),
            ),
            MutateOp::Trash | MutateOp::Restore | MutateOp::Star | MutateOp::Unstar => (
                Method::POST,
                format!(
                    "/drive/v1/files:{}",
                    match operation {
                        MutateOp::Trash => "batchTrash",
                        MutateOp::Restore => "batchUntrash",
                        MutateOp::Star => "star",
                        _ => "unstar",
                    }
                ),
                json!({"ids":request.ids}),
            ),
            MutateOp::DeleteForever => (
                Method::POST,
                "/drive/v1/files:batchDelete".into(),
                json!({"ids":request.ids}),
            ),
        };
        let (value, epoch) = self
            .drive_request_at(
                method,
                &format!("{}{path}", self.http.drive_origin),
                Some(&body),
                Some(snapshot.epoch),
            )
            .await
            .map_err(|mut error| {
                // A challenge before submission or an explicit verification rejection means no mutation was accepted.
                // A later challenge while polling/stat'ing the accepted mutation must never clear its durable intent.
                if error.code == ErrorCode::VerificationRequired {
                    error
                        .details
                        .get_or_insert_with(Default::default)
                        .insert("safe_to_resubmit".into(), true.into());
                }
                error
            })?;
        let task = value.get("task").filter(|t| !t.is_null());
        let phase = task.and_then(|v| v.get("phase")).and_then(Value::as_str);
        if phase == Some("PHASE_TYPE_ERROR") {
            return Err(error(
                ErrorCode::Conflict,
                "云端操作失败，请刷新列表核对结果。",
            ));
        }
        let declared_id = value
            .get("task_id")
            .filter(|v| !v.is_null() && v.as_str() != Some(""));
        let task_id = declared_id
            .and_then(Value::as_str)
            .or_else(|| task.and_then(|v| v.get("id")).and_then(Value::as_str));
        if (declared_id.is_some() || task.is_some())
            && task_id.is_none()
            && phase != Some("PHASE_TYPE_COMPLETE")
        {
            return Err(wire::invalid());
        }
        if let Some(id) = task_id {
            wire::validate_id(id)?;
            let mut complete = task.and_then(|v| v.get("phase")).and_then(Value::as_str)
                == Some("PHASE_TYPE_COMPLETE");
            for _ in 0..10 {
                if complete {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(500)).await;
                let (task, current) = self
                    .drive_request_at(
                        Method::GET,
                        &format!("{}/drive/v1/tasks/{id}", self.http.drive_origin),
                        None,
                        Some(epoch),
                    )
                    .await?;
                if current != epoch {
                    return Err(expired());
                }
                match task.get("phase").and_then(Value::as_str) {
                    Some("PHASE_TYPE_COMPLETE") => complete = true,
                    Some("PHASE_TYPE_ERROR") => {
                        return Err(error(
                            ErrorCode::Conflict,
                            "云端操作失败，请刷新列表核对结果。",
                        ))
                    }
                    _ => {}
                }
            }
            if !complete {
                return Err(error(
                    ErrorCode::Conflict,
                    "云端操作尚未确认完成，请稍后刷新核对。请求不会重复提交。",
                ));
            }
        }
        let mut created = Vec::new();
        if operation == MutateOp::Mkdir {
            let item = wire::entry(value.get("file").unwrap_or(&value))?;
            if item.name != name
                || item.parent_id.as_deref() != Some(parent)
                || item.kind != EntryKind::Folder
            {
                return Err(wire::invalid());
            }
            created.push(item);
        }
        if operation == MutateOp::Rename {
            let (check, current) = self
                .drive_request_at(
                    Method::GET,
                    &format!(
                        "{}/drive/v1/files/{}",
                        self.http.drive_origin, request.ids[0]
                    ),
                    None,
                    Some(epoch),
                )
                .await?;
            if current != epoch || wire::entry(&check)?.name != name {
                return Err(error(
                    ErrorCode::Conflict,
                    "尚未确认新的文件名，请刷新列表核对。",
                ));
            }
        }
        if operation == MutateOp::DeleteForever {
            for id in &request.ids {
                match self
                    .drive_request_at(
                        Method::GET,
                        &format!("{}/drive/v1/files/{id}", self.http.drive_origin),
                        None,
                        Some(epoch),
                    )
                    .await
                {
                    Err(failure) if failure.code == ErrorCode::NotFound => {}
                    Err(failure) => return Err(failure),
                    Ok(_) => {
                        return Err(error(
                            ErrorCode::Conflict,
                            "永久删除尚未确认完成，请刷新回收站核对。",
                        ))
                    }
                }
            }
        }
        let mut session = self.session.lock().await;
        if session.epoch != epoch {
            return Err(expired());
        }
        session.cursors.clear();
        Ok(OperationResult {
            op: operation,
            succeeded: if operation == MutateOp::Mkdir {
                created.iter().map(|e| e.id.clone()).collect()
            } else {
                request.ids.clone()
            },
            failed: vec![],
            created,
            replayed: false,
        })
    }

    pub(super) async fn native_shares(
        &self,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<Page<ShareInfo>, ClientError> {
        let mut url = url::Url::parse(&format!("{}/drive/v1/share/list", self.http.drive_origin))
            .map_err(|_| wire::invalid())?;
        url.query_pairs_mut()
            .append_pair("thumbnail_size", "SIZE_SMALL");
        let (value, next_cursor) = self.page_request("shares", url, cursor, limit).await?;
        let rows = value
            .get("data")
            .and_then(Value::as_array)
            .ok_or_else(wire::invalid)?;
        if rows.len() > limit {
            return Err(wire::invalid());
        }
        let mut items = Vec::new();
        for row in rows {
            let url = wire::text(row, "share_url", 8192)?;
            let parsed = url::Url::parse(&url).map_err(|_| wire::invalid())?;
            if parsed.scheme() != "https"
                || parsed
                    .host_str()
                    .is_none_or(|h| h != "mypikpak.com" && !h.ends_with(".mypikpak.com"))
                || !parsed.username().is_empty()
                || parsed.password().is_some()
            {
                return Err(wire::invalid());
            }
            let number = |key| -> Result<Option<u32>, ClientError> {
                row.get(key)
                    .filter(|v| !v.is_null())
                    .map(|v| u32::try_from(wire::number(v)?.0).map_err(|_| wire::invalid()))
                    .transpose()
            };
            let created = wire::text(row, "create_time", 128)?;
            OffsetDateTime::parse(&created, &Rfc3339).map_err(|_| wire::invalid())?;
            let expires_at = row
                .get("expiration_at")
                .and_then(Value::as_str)
                .filter(|v| *v != "-1" && !v.is_empty())
                .map(parse_expiry)
                .transpose()?;
            let state = match row.get("share_status").and_then(Value::as_str) {
                Some("OK") => "active",
                Some("EXPIRED") => "expired",
                Some("DELETED" | "CANCELLED") => "cancelled",
                _ => "unavailable",
            };
            items.push(ShareInfo {
                share_id: wire::text(row, "share_id", 256)?,
                title: wire::text(row, "title", 4096)?,
                item_count: number("file_num")?.ok_or_else(wire::invalid)?,
                url,
                code: row
                    .get("pass_code")
                    .and_then(Value::as_str)
                    .filter(|v| !v.is_empty() && v.len() <= 64 && !v.chars().any(char::is_control))
                    .map(str::to_owned),
                created_at: created,
                expires_known: expires_at.is_some()
                    || row.get("expiration_at").and_then(Value::as_str) == Some("-1"),
                expires_at,
                state: state.into(),
                view_count: number("view_count")?,
                save_count: number("restore_count")?,
            });
        }
        Ok(Page { items, next_cursor })
    }

    pub(super) async fn native_quota(&self) -> Result<QuotaReport, ClientError> {
        let (about, epoch) = self
            .get(&format!("{}/drive/v1/about", self.http.drive_origin))
            .await?;
        let quota = about.get("quota").ok_or_else(wire::invalid)?;
        let limit = quota.get("limit").map(wire::number).transpose()?;
        let usage = quota.get("usage").map(wire::number).transpose()?;
        let mut dimensions = vec![QuotaDimension {
            dimension: "storage".into(),
            period: "total".into(),
            used_bytes: usage,
            limit_bytes: limit,
            remaining_bytes: limit
                .zip(usage)
                .map(|(l, u)| DecimalU64(l.0.saturating_sub(u.0))),
            resets_at: None,
        }];
        let mut limitations = vec![];
        match self
            .get(&format!(
                "{}/vip/v1/quantity/list?type=transfer",
                self.http.drive_origin
            ))
            .await
        {
            Ok((value, current)) => {
                if current != epoch {
                    return Err(expired());
                }
                for (key, dimension, period) in [
                    ("download", "download", "month"),
                    ("upload", "upload", "month"),
                    ("offline", "cloud_download", "month"),
                    ("download_daily", "download_daily", "day"),
                ] {
                    let band = &value["base"][key];
                    let mut limit = band.get("total_assets").map(wire::number).transpose()?;
                    let used = band.get("assets").map(wire::number).transpose()?;
                    if key == "download_daily" && limit == Some(DecimalU64(0)) {
                        limit = None;
                    }
                    dimensions.push(QuotaDimension {
                        dimension: dimension.into(),
                        period: period.into(),
                        used_bytes: used,
                        limit_bytes: limit,
                        remaining_bytes: limit
                            .zip(used)
                            .map(|(l, u)| DecimalU64(l.0.saturating_sub(u.0))),
                        resets_at: None,
                    });
                }
            }
            Err(e)
                if matches!(
                    e.code,
                    ErrorCode::VerificationRequired | ErrorCode::AuthExpired
                ) =>
            {
                return Err(e)
            }
            Err(_) => limitations.push("传输额度暂未取得；已取得的存储容量仍可查看。".into()),
        }
        Ok(QuotaReport {
            backend: self.id(),
            account_tier: AccountTier::Unknown,
            source: "account".into(),
            dimensions,
            checked_at: timestamp(now()),
            limitations,
        })
    }
}

fn parse_expiry(value: &str) -> Result<String, ClientError> {
    if let Ok(parsed) = OffsetDateTime::parse(value, &Rfc3339) {
        return parsed.format(&Rfc3339).map_err(|_| wire::invalid());
    }
    let number = value.parse::<i64>().map_err(|_| wire::invalid())?;
    let seconds = if value.len() == 13 {
        number / 1000
    } else {
        number
    };
    OffsetDateTime::from_unix_timestamp(seconds)
        .map_err(|_| wire::invalid())?
        .format(&Rfc3339)
        .map_err(|_| wire::invalid())
}

#[cfg(test)]
mod mutation_tests {
    use super::*;
    use crate::cloud::tests::Server;
    use std::sync::atomic::{AtomicBool, Ordering};
    #[tokio::test]
    async fn permanent_delete_only_accepts_trash_and_confirms_absence() {
        let deleted = Arc::new(AtomicBool::new(false));
        let state = deleted.clone();
        let server=Server::new(move|r|match(r.method.as_str(),r.path.split('?').next().unwrap()){
            ("GET","/drive/v1/files/trash-1")=>if state.load(Ordering::SeqCst){(404,json!({"error":"file_not_found"}))}else{(200,json!({"id":"trash-1","kind":"drive#file","name":"gone.txt","size":"1","parent_id":"","trashed":true,"phase":"PHASE_TYPE_COMPLETE"}))},
            ("GET","/drive/v1/files/live-1")=>(200,json!({"id":"live-1","kind":"drive#file","name":"keep.txt","size":"1","parent_id":"","trashed":false,"phase":"PHASE_TYPE_COMPLETE"})),
            ("POST","/drive/v1/files:batchDelete")=>{state.store(true,Ordering::SeqCst);(200,json!({"task":{"id":"delete-1","phase":"PHASE_TYPE_COMPLETE"}}))},_ =>(404,json!({"error":"file_not_found"}))}).await;
        let p = server.provider().await;
        let scope = p.account_scope().await.unwrap();
        let request = |id: &str| FileMutation {
            op: MutateOp::DeleteForever,
            ids: vec![id.into()],
            target: None,
        };
        assert!(p.native_mutate(&request("live-1"), &scope).await.is_err());
        assert!(!deleted.load(Ordering::SeqCst));
        assert_eq!(
            p.native_mutate(&request("trash-1"), &scope)
                .await
                .unwrap()
                .succeeded,
            ["trash-1"]
        );
        assert!(deleted.load(Ordering::SeqCst));
    }
}
