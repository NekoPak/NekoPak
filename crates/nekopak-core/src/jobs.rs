//! Account-scoped cloud extraction jobs, with durable intent before submission.
use super::*;
use serde_json::{json, Value};
fn invalid() -> ClientError {
    ClientError::new(ErrorCode::IntegrityFailed, "解压任务记录无效，未重复提交。")
}
impl AppService {
    pub async fn archive_list(
        &self,
        id: &str,
        path: &str,
        password: Option<&str>,
        ctx: &RequestContext,
    ) -> Result<ArchiveListing, ClientError> {
        ctx.run(self.provider.archive_list(id, path, password))
            .await
    }
    pub async fn archive_unlock(
        &self,
        id: &str,
        password: &str,
        ctx: &RequestContext,
    ) -> Result<Value, ClientError> {
        Ok(json!({"password_ref":ctx.run(self.provider.archive_unlock(id,password)).await?}))
    }
    pub async fn archive_extract(
        &self,
        id: &str,
        paths: &Value,
        parent: &str,
        password: Option<&str>,
        key: &str,
        ctx: &RequestContext,
    ) -> Result<Value, ClientError> {
        let requested: Vec<String> = if paths == "all" {
            vec![]
        } else {
            serde_json::from_value(paths.clone())
                .map_err(|_| ClientError::new(ErrorCode::Conflict, "解压选择无效。"))?
        };
        if paths != "all" && requested.is_empty() {
            return Err(ClientError::new(
                ErrorCode::Conflict,
                "请至少选择一个解压项目。",
            ));
        }
        let scope = ctx.run(self.provider.account_scope()).await?;
        let key = scoped_key("extract", key)?;
        let fp = fingerprint(&[id, &paths.to_string(), parent]);
        let job = if let Some(value) = self.records()?.begin_value(&scope, &key, &fp)? {
            serde_json::from_value::<ArchiveJob>(value).map_err(|_| invalid())?
        } else {
            let result = ctx
                .run(
                    self.provider
                        .archive_extract(id, &requested, parent, password, &scope),
                )
                .await
                .map_err(cloud::unknown_write);
            self.records()?.finish_value(
                &scope,
                &key,
                &result
                    .as_ref()
                    .map(|v| serde_json::to_value(v).expect("job serializes"))
                    .map_err(Clone::clone),
            )?;
            result?
        };
        if self
            .records()?
            .record_get(&scope, "jobs", &job.job_id)?
            .is_some()
        {
            return Ok(json!({"job_id":job.job_id}));
        }
        let state = job_state(job.state);
        let payload = json!({"job_id":job.job_id,"kind":"archive_extract","state":state,"title":"云端解压","progress":job.progress,"target_parent_id":parent,"created_at":now(),"updated_at":now(),"revision":"1"});
        self.records()?
            .record_put(&scope, "jobs", &job.job_id, &payload.to_string())?;
        Ok(json!({"job_id":job.job_id}))
    }
    pub async fn jobs_list(
        &self,
        cursor: Option<&str>,
        limit: Option<u32>,
        ctx: &RequestContext,
    ) -> Result<Page<Value>, ClientError> {
        let scope = ctx.run(self.provider.account_scope()).await?;
        let limit = page_limit(limit)?;
        let after = cursor
            .map(|s| s.parse::<usize>())
            .transpose()
            .map_err(|_| invalid())?
            .unwrap_or(0);
        let records = self.records()?.record_list(&scope, "jobs")?;
        if after > records.len() {
            return Err(invalid());
        }
        let more = after + limit < records.len();
        let mut items = Vec::new();
        for (id, payload) in records.into_iter().skip(after).take(limit) {
            let mut value: Value = serde_json::from_str(&payload).map_err(|_| invalid())?;
            if matches!(value["state"].as_str(), Some("queued" | "running")) {
                match ctx.run(self.provider.archive_job(&id, &scope)).await {
                    Ok(job) => {
                        value["state"] = job_state(job.state).into();
                        value["progress"] = json!(job.progress);
                        value["updated_at"] = now().into();
                        value["revision"] = value["revision"]
                            .as_str()
                            .and_then(|s| s.parse::<u64>().ok())
                            .unwrap_or(0)
                            .saturating_add(1)
                            .to_string()
                            .into();
                        self.records()?
                            .record_put(&scope, "jobs", &id, &value.to_string())?;
                    }
                    Err(error) => {
                        value["error"] = json!(error);
                    }
                }
            }
            items.push(value);
        }
        self.check_scope(&scope, ctx).await?;
        Ok(Page {
            items,
            next_cursor: more.then(|| (after + limit).to_string()),
        })
    }
}
fn job_state(state: TaskState) -> &'static str {
    match state {
        TaskState::Queued => "queued",
        TaskState::Completed => "completed",
        TaskState::Failed => "failed",
        TaskState::Cancelled => "cancelled",
        _ => "running",
    }
}
