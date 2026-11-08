//! Independent cloud-task protocol implementation. Source facts: NATIVE_EXTENDED_PROTOCOLS.md.
use super::*;

pub(super) fn validate_source(source: &str) -> Result<(), ClientError> {
    if source.len() > 16_384 || source.chars().any(char::is_control) {
        return Err(error(ErrorCode::Conflict, "资源链接无效。"));
    }
    let url = url::Url::parse(source)
        .map_err(|_| error(ErrorCode::Conflict, "请输入磁力链或 HTTP(S) 资源链接。"))?;
    if url.scheme() == "magnet" {
        if !url
            .query_pairs()
            .any(|(key, value)| key == "xt" && value.starts_with("urn:btih:") && value.len() > 9)
        {
            return Err(error(ErrorCode::Conflict, "磁力链缺少资源标识。"));
        }
    } else if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err(error(
            ErrorCode::Conflict,
            "资源链接不能包含账号密码或不支持的协议。",
        ));
    }
    Ok(())
}

pub(super) fn submitted_error(mut failure: ClientError) -> ClientError {
    if failure.code == ErrorCode::VerificationRequired {
        failure
            .details
            .get_or_insert_with(Default::default)
            .insert("safe_to_resubmit".into(), true.into());
    } else if matches!(
        failure.code,
        ErrorCode::TransientNetwork
            | ErrorCode::Internal
            | ErrorCode::IntegrityFailed
            | ErrorCode::Cancelled
    ) {
        failure.retryable = false;
        failure
            .details
            .get_or_insert_with(Default::default)
            .insert("outcome_unknown".into(), true.into());
    }
    failure
}

pub(super) fn task_snapshot(value: &Value) -> Result<CloudTaskSnapshot, ClientError> {
    let task = value.get("task").unwrap_or(value);
    let id = wire::text(task, "id", 256)?;
    wire::validate_id(&id)?;
    let state = match task.get("phase").and_then(Value::as_str) {
        Some("PHASE_TYPE_PENDING") => TaskState::Queued,
        Some("PHASE_TYPE_RUNNING") => TaskState::Running,
        Some("PHASE_TYPE_COMPLETE") => TaskState::Completed,
        Some("PHASE_TYPE_ERROR") => TaskState::Failed,
        Some("PHASE_TYPE_PAUSED") => TaskState::Paused,
        Some("PHASE_TYPE_CANCELLED") => TaskState::Cancelled,
        _ => return Err(wire::invalid()),
    };
    let progress = task
        .get("progress")
        .filter(|v| !v.is_null())
        .map(|v| {
            let value = wire::number(v)?.0;
            if value > 100 {
                return Err(wire::invalid());
            }
            Ok(value as u32)
        })
        .transpose()?;
    let file_id = task
        .get("file_id")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(|id| {
            wire::validate_id(id)?;
            Ok(id.to_owned())
        })
        .transpose()?;
    let total_bytes = task
        .get("file_size")
        .filter(|v| !v.is_null() && v.as_str() != Some(""))
        .map(wire::number)
        .transpose()?;
    let created_at = task
        .get("created_time")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(|s| {
            OffsetDateTime::parse(s, &Rfc3339).map_err(|_| wire::invalid())?;
            Ok(s.to_owned())
        })
        .transpose()?;
    // The remote task name/message may echo the signed source URL. Never relay it.
    let name = task
        .get("name")
        .and_then(Value::as_str)
        .filter(|s| {
            !s.is_empty()
                && s.len() <= 1024
                && !s.chars().any(char::is_control)
                && !s.contains("://")
                && !s.contains("magnet:")
        })
        .unwrap_or("云下载")
        .to_owned();
    Ok(CloudTaskSnapshot {
        id,
        name,
        state,
        progress,
        file_id,
        total_bytes,
        created_at,
        error: (state == TaskState::Failed).then(|| {
            error(
                ErrorCode::Conflict,
                "云下载失败，请检查资源状态及账号额度后重试。",
            )
        }),
    })
}

impl NativeProvider {
    pub(super) async fn checked_scope(&self, scope: &str) -> Result<u64, ClientError> {
        let snapshot = self.snapshot().await?;
        let subject = self.drive_subject(&snapshot).await?;
        if format!(
            "{:x}",
            Sha256::digest(format!("{}\0{}", self.profile()?.id, subject))
        ) != scope
        {
            return Err(expired());
        }
        Ok(snapshot.epoch)
    }

    pub async fn native_cloud_create(
        &self,
        source: &str,
        parent: &str,
        scope: &str,
    ) -> Result<CloudTaskSnapshot, ClientError> {
        validate_source(source)?;
        wire::validate_id(parent)?;
        let epoch = self.checked_scope(scope).await?;
        let body = json!({"kind":"drive#file","upload_type":"UPLOAD_TYPE_URL","url":{"url":source},"parent_id":if parent=="root" {""} else {parent},"folder_type":""});
        let (value, _) = self
            .drive_request_at(
                Method::POST,
                &format!("{}/drive/v1/files", self.http.drive_origin),
                Some(&body),
                Some(epoch),
            )
            .await
            .map_err(submitted_error)?;
        task_snapshot(&value).map_err(submitted_error)
    }

    pub async fn native_cloud_task(
        &self,
        id: &str,
        scope: &str,
    ) -> Result<CloudTaskSnapshot, ClientError> {
        wire::validate_id(id)?;
        let epoch = self.checked_scope(scope).await?;
        let (value, _) = self
            .drive_request_at(
                Method::GET,
                &format!(
                    "{}/drive/v1/tasks/{id}?type=offline&checkPhase=true",
                    self.http.drive_origin
                ),
                None,
                Some(epoch),
            )
            .await?;
        let task = task_snapshot(&value)?;
        if task.id != id {
            return Err(wire::invalid());
        }
        Ok(task)
    }

    pub async fn native_cloud_list(
        &self,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<Page<CloudTaskSnapshot>, ClientError> {
        let mut url = url::Url::parse(&format!("{}/drive/v1/tasks", self.http.drive_origin))
            .map_err(|_| wire::invalid())?;
        url.query_pairs_mut()
            .append_pair("type", "offline")
            .append_pair("thumbnail_size", "SIZE_SMALL")
            .append_pair("with", "reference_resource");
        let (value, next_cursor) = self.page_request("cloud_tasks", url, cursor, limit).await?;
        let rows = value
            .get("tasks")
            .and_then(Value::as_array)
            .ok_or_else(wire::invalid)?;
        if rows.len() > limit {
            return Err(wire::invalid());
        }
        let items = rows
            .iter()
            .map(task_snapshot)
            .collect::<Result<Vec<_>, _>>()?;
        if items.iter().map(|t| &t.id).collect::<BTreeSet<_>>().len() != items.len() {
            return Err(wire::invalid());
        }
        Ok(Page { items, next_cursor })
    }

    pub async fn native_cloud_control(
        &self,
        id: &str,
        action: TransferAction,
        scope: &str,
    ) -> Result<(), ClientError> {
        wire::validate_id(id)?;
        let epoch = self.checked_scope(scope).await?;
        let (method, path, body) = match action {
            TransferAction::Retry => (
                Method::POST,
                "/drive/v1/task".to_owned(),
                Some(json!({"id":id,"create_type":"RETRY","type":"offline"})),
            ),
            TransferAction::Cancel => (
                Method::DELETE,
                format!("/drive/v1/tasks?task_ids={id}&delete_files=false"),
                None,
            ),
            _ => {
                return Err(error(
                    ErrorCode::Unsupported,
                    "云端任务暂不支持暂停或继续；可取消或重试失败任务。",
                ))
            }
        };
        self.drive_request_at(
            method,
            &format!("{}{path}", self.http.drive_origin),
            body.as_ref(),
            Some(epoch),
        )
        .await
        .map_err(submitted_error)?;
        // Mutating HTTP success is acceptance only. Confirm via the remote task.
        if action == TransferAction::Cancel {
            match self.native_cloud_task(id, scope).await {
                Err(failure) if failure.code == ErrorCode::NotFound => Ok(()),
                Ok(task) if task.state == TaskState::Cancelled => Ok(()),
                Err(failure) => Err(failure),
                _ => Err(error(
                    ErrorCode::Conflict,
                    "任务取消尚未确认，请刷新核对，不会重复提交。",
                )),
            }
        } else {
            let task = self.native_cloud_task(id, scope).await?;
            if matches!(
                task.state,
                TaskState::Queued | TaskState::Running | TaskState::Completed
            ) {
                Ok(())
            } else {
                Err(error(
                    ErrorCode::Conflict,
                    "云端重试尚未开始，请刷新核对，不会重新创建资源。",
                ))
            }
        }
    }

    pub async fn native_cloud_preview(
        &self,
        source: &str,
    ) -> Result<CloudResourcePreview, ClientError> {
        validate_source(source)?;
        let body = json!({"urls":source,"page_size":500,"thumbnail_type":"FROM_HASH"});
        let (value, _) = self
            .drive_request_at(
                Method::POST,
                &format!("{}/drive/v1/resource/list", self.http.drive_origin),
                Some(&body),
                None,
            )
            .await?;
        resource_preview(&value)
    }
}

fn resource_preview(value: &Value) -> Result<CloudResourcePreview, ClientError> {
    let root = value.get("list").unwrap_or(value);
    let resources = root
        .get("resources")
        .and_then(Value::as_array)
        .ok_or_else(wire::invalid)?;
    if resources.len() != 1 {
        return Err(wire::invalid());
    }
    let item = &resources[0];
    let title = wire::text(item, "name", 1024)?;
    if title.contains("://") || title.contains("magnet:") {
        return Err(wire::invalid());
    }
    let total_bytes = item
        .get("file_size")
        .or_else(|| item.get("size"))
        .filter(|v| !v.is_null() && v.as_str() != Some(""))
        .map(wire::number)
        .transpose()?;
    // The public parse endpoint does not establish a verified subset-creation contract.
    // Never offer fabricated index selections; callers may save the whole resource.
    Ok(CloudResourcePreview {
        title,
        total_bytes,
        files: vec![],
        selection_supported: false,
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::{collections::HashMap, sync::Mutex as StdMutex};
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };
    pub(crate) struct Request {
        pub method: String,
        pub path: String,
        pub body: Value,
        pub headers: HashMap<String, String>,
    }
    pub(crate) struct Server {
        pub origin: String,
        pub requests: Arc<StdMutex<Vec<Request>>>,
        task: tokio::task::JoinHandle<()>,
    }
    impl Drop for Server {
        fn drop(&mut self) {
            self.task.abort();
        }
    }
    impl Server {
        pub(crate) async fn new(
            handler: impl Fn(&Request) -> (u16, Value) + Send + Sync + 'static,
        ) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let origin = format!("http://{}", listener.local_addr().unwrap());
            let requests = Arc::new(StdMutex::new(Vec::new()));
            let seen = requests.clone();
            let handler = Arc::new(handler);
            let task = tokio::spawn(async move {
                let mut children = tokio::task::JoinSet::new();
                loop {
                    let (mut stream, _) = listener.accept().await.unwrap();
                    let seen = seen.clone();
                    let handler = handler.clone();
                    children.spawn(async move {
                let mut bytes=Vec::new();let end=loop {let mut b=[0;4096];let n=stream.read(&mut b).await.unwrap();if n==0{return}bytes.extend_from_slice(&b[..n]);assert!(bytes.len()<131072);if let Some(i)=bytes.windows(4).position(|p|p==b"\r\n\r\n"){break i+4}};
                let text=std::str::from_utf8(&bytes[..end]).unwrap();let mut lines=text.lines();let mut first=lines.next().unwrap().split_whitespace();let method=first.next().unwrap().to_owned();let path=first.next().unwrap().to_owned();let headers:HashMap<String,String>=lines.filter_map(|l|l.split_once(':')).map(|(k,v)|(k.to_ascii_lowercase(),v.trim().to_owned())).collect();let length=headers.get("content-length").map(|s|s.parse::<usize>().unwrap()).unwrap_or(0);while bytes.len()<end+length {let mut b=[0;4096];let n=stream.read(&mut b).await.unwrap();if n==0{return}bytes.extend_from_slice(&b[..n]);}
                let request=Request{method,path,headers,body:if length==0{Value::Null}else{serde_json::from_slice(&bytes[end..end+length]).unwrap()}};
                let(status,value)=match request.path.split('?').next().unwrap(){"/v1/shield/captcha/init"=>(200,json!({"captcha_token":"synthetic-captcha"})),"/v1/auth/signin"=>(200,json!({"access_token":"synthetic-access","refresh_token":"synthetic-refresh","token_type":"Bearer","expires_in":3600,"sub":"synthetic-user"})),_=>handler(&request)};seen.lock().unwrap().push(request);let body=value.to_string();let response=format!("HTTP/1.1 {status} Test\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len());let _=stream.write_all(response.as_bytes()).await;
            });
                }
            });
            Self {
                origin,
                requests,
                task,
            }
        }
        pub(crate) async fn provider(&self) -> NativeProvider {
            let provider = NativeProvider::synthetic_loopback(&self.origin).unwrap();
            provider
                .sign_in("synthetic@example.test", "not-an-account")
                .await
                .unwrap();
            provider
        }
    }
    #[tokio::test]
    async fn cloud_create_poll_and_retry_use_remote_id_and_never_recreate_source() {
        let server=Server::new(|r|match r.path.split('?').next().unwrap(){
            "/drive/v1/files"=>{assert_eq!(r.body["upload_type"],"UPLOAD_TYPE_URL");assert_eq!(r.body["parent_id"],"");(200,json!({"task":{"id":"task-1","phase":"PHASE_TYPE_PENDING","name":"video"}}))},
            "/drive/v1/tasks/task-1"=>(200,json!({"id":"task-1","phase":"PHASE_TYPE_COMPLETE","progress":100,"name":"video","file_id":"file-1"})),
            "/drive/v1/task"=>{assert_eq!(r.body,json!({"id":"task-1","create_type":"RETRY","type":"offline"}));(200,json!({}))},_ => (404,json!({"error":"file_not_found"}))}).await;
        let p = server.provider().await;
        let scope = p.account_scope().await.unwrap();
        let t = p
            .native_cloud_create("https://example.test/video", "root", &scope)
            .await
            .unwrap();
        assert_eq!(t.state, TaskState::Queued);
        assert_eq!(t.total_bytes, None);
        p.native_cloud_control(&t.id, TransferAction::Retry, &scope)
            .await
            .unwrap();
        let t = p.native_cloud_task(&t.id, &scope).await.unwrap();
        assert_eq!(t.state, TaskState::Completed);
        assert_eq!(t.total_bytes, None);
        assert_eq!(
            server
                .requests
                .lock()
                .unwrap()
                .iter()
                .filter(|r| r.path == "/drive/v1/files")
                .count(),
            1
        );
        assert!(server
            .requests
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r.path.starts_with("/drive/"))
            .all(|r| r.headers.get("authorization").map(String::as_str)
                == Some("Bearer synthetic-access")));
    }
    #[test]
    fn invalid_or_private_task_responses_do_not_leak_source() {
        assert!(task_snapshot(&json!({"id":"one","phase":"made-up"})).is_err());
        let task=task_snapshot(&json!({"id":"one","phase":"PHASE_TYPE_ERROR","name":"https://secret.test/?token=secret","message":"password=secret"})).unwrap();
        assert!(!serde_json::to_string(&task).unwrap().contains("secret"));
        assert!(validate_source("https://user:password@example.test/a").is_err());
        assert!(
            task_snapshot(&json!({"id":"one","phase":"PHASE_TYPE_RUNNING","progress":101}))
                .is_err()
        );
        let preview=resource_preview(&json!({"list":{"resources":[{"name":"sample","file_size":"42","dir":{"resources":[{"name":"child"}]}}]}})).unwrap();
        assert_eq!(preview.total_bytes, Some(DecimalU64(42)));
        assert!(!preview.selection_supported);
    }
}
