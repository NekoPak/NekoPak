//! Account-scoped, snapshot-checked sync. No failed scan becomes an empty tree.
use super::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha1::Sha1;
use std::{collections::BTreeSet, io::Read};

const MAX_FILES: usize = 100_000;
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
struct LocalNode {
    kind: EntryKind,
    size: u64,
    identity: String,
    sha256: Option<String>,
    gcid: Option<String>,
}
#[derive(Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
struct Snapshot {
    local: BTreeMap<String, LocalNode>,
    remote: BTreeMap<String, RemoteEntry>,
}
#[derive(Clone, Serialize, Deserialize)]
struct Baseline {
    revision: String,
    snapshot: Snapshot,
}
#[derive(Clone, Serialize, Deserialize)]
enum StepKind {
    MkdirLocal,
    MkdirRemote,
    Upload,
    Download,
    DeleteLocal,
    DeleteRemote,
}
#[derive(Clone, Serialize, Deserialize)]
struct Step {
    kind: StepKind,
    path: String,
    backup: bool,
}
#[derive(Clone, Serialize, Deserialize)]
struct PlanRecord {
    config: SyncConfig,
    scope: String,
    snapshot: Snapshot,
    plan: SyncPlan,
    steps: Vec<Step>,
}
#[derive(Clone, Serialize, Deserialize)]
struct Run {
    job_id: String,
    scope: String,
    plan_id: String,
    config_id: String,
    state: String,
    index: usize,
    task_id: Option<String>,
    error: Option<ClientError>,
}
#[derive(Serialize, Deserialize, Default)]
struct Schedule {
    next_attempt: i64,
    failures: u32,
}
fn invalid() -> ClientError {
    ClientError::new(
        ErrorCode::IntegrityFailed,
        "同步记录或路径不完整，已停止并保留现场。",
    )
}
fn conflict() -> ClientError {
    ClientError::new(
        ErrorCode::Conflict,
        "同步源、目标或基线已变化，请重新扫描确认。",
    )
}
fn encode<T: Serialize>(v: &T) -> Result<String, ClientError> {
    serde_json::to_string(v).map_err(|_| invalid())
}
fn decode<T: serde::de::DeserializeOwned>(s: &str) -> Result<T, ClientError> {
    serde_json::from_str(s).map_err(|_| invalid())
}
fn valid_relative(path: &str) -> Result<(), ClientError> {
    if path.is_empty()
        || path.len() > 16384
        || path
            .split('/')
            .any(|p| p.is_empty() || p == "." || p == ".." || downloads::safe_name(p) != p)
        || path.contains('\\')
    {
        return Err(invalid());
    }
    Ok(())
}
fn nested(a: &Path, b: &Path) -> bool {
    let a = a
        .to_string_lossy()
        .replace('\\', "/")
        .trim_end_matches('/')
        .to_lowercase();
    let b = b
        .to_string_lossy()
        .replace('\\', "/")
        .trim_end_matches('/')
        .to_lowercase();
    a == b || a.starts_with(&(b.clone() + "/")) || b.starts_with(&(a + "/"))
}
fn glob_match(pattern: &str, text: &str) -> bool {
    let p = pattern.as_bytes();
    let t = text.as_bytes();
    let (mut a, mut b, mut star, mut retry) = (0, 0, None, 0);
    while b < t.len() {
        if a < p.len() && (p[a] == b'?' || p[a] == t[b]) {
            a += 1;
            b += 1;
        } else if a < p.len() && p[a] == b'*' {
            star = Some(a);
            a += 1;
            retry = b;
        } else if let Some(s) = star {
            a = s + 1;
            retry += 1;
            b = retry;
        } else {
            return false;
        }
    }
    while a < p.len() && p[a] == b'*' {
        a += 1;
    }
    a == p.len()
}
fn excluded(config: &SyncConfig, path: &str) -> bool {
    path.split('/')
        .any(|p| p == ".nekopak-backup" || p.ends_with(".part") || p.ends_with(".nekopak-part"))
        || config
            .exclude_globs
            .iter()
            .any(|p| glob_match(p, path) || path.split('/').any(|n| glob_match(p, n)))
}

impl AppService {
    pub async fn sync_configs_list(
        &self,
        ctx: &RequestContext,
    ) -> Result<Vec<SyncConfig>, ClientError> {
        let scope = ctx.run(self.provider.account_scope()).await?;
        self.records()?
            .record_list(&scope, "sync-config")?
            .into_iter()
            .map(|(_, s)| decode(&s))
            .collect()
    }
    pub async fn sync_config_save(
        &self,
        config: &SyncConfig,
        ctx: &RequestContext,
    ) -> Result<SyncConfig, ClientError> {
        let _guard = self.sync_gate.lock().await;
        let scope = ctx.run(self.provider.account_scope()).await?;
        let mut config = config.clone();
        let mut reset_baseline = false;
        validate_destination(&config.local_path)?;
        let _locks = downloads::lock_parents(Path::new(&config.local_path))?;
        if nested(Path::new(&config.local_path), &self.data_root) {
            return Err(ClientError::new(
                ErrorCode::Conflict,
                "同步根不能包含应用数据/缓存目录或位于其中。",
            ));
        }
        if !Path::new(&config.local_path).is_dir()
            || !(60..=86400).contains(&config.interval_seconds)
            || config.name.trim().is_empty()
            || config.name.len() > 256
            || config.exclude_globs.len() > 64
            || config.exclude_globs.iter().any(|p| {
                p.len() > 256
                    || p.chars().any(char::is_control)
                    || p.contains(['\\', '[', ']', '{', '}'])
            })
        {
            return Err(ClientError::new(
                ErrorCode::Conflict,
                "同步名称、目录或排除规则无效；排除规则支持*和?。",
            ));
        }
        if config.remote_id == "root" {
            ctx.run(self.provider.list("root", None, 1)).await?;
        } else {
            let root = ctx.run(self.provider.stat(&config.remote_id)).await?;
            if root.kind != EntryKind::Folder {
                return Err(ClientError::new(ErrorCode::Conflict, "请选择远端文件夹。"));
            }
        }
        if config.config_id.is_empty() {
            config.config_id = uuid::Uuid::new_v4().to_string();
            config.first_run = true;
            config.last_applied_at = None;
            config.allow_deletes = false;
        } else {
            let old: SyncConfig = decode(
                &self
                    .records()?
                    .record_get(&scope, "sync-config", &config.config_id)?
                    .ok_or_else(invalid)?,
            )?;
            config.first_run = old.first_run;
            config.last_applied_at = old.last_applied_at;
            if old.local_path != config.local_path
                || old.remote_id != config.remote_id
                || old.mode != config.mode
            {
                config.first_run = true;
                config.allow_deletes = false;
                reset_baseline = true;
            }
        }
        for other in self.sync_configs_list(ctx).await? {
            if other.config_id != config.config_id
                && (nested(Path::new(&other.local_path), Path::new(&config.local_path))
                    || self
                        .remote_ancestor(&other.remote_id, &config.remote_id, &scope, ctx)
                        .await?
                    || self
                        .remote_ancestor(&config.remote_id, &other.remote_id, &scope, ctx)
                        .await?)
            {
                return Err(ClientError::new(
                    ErrorCode::Conflict,
                    "同步根目录不能相互嵌套或重复。",
                ));
            }
        }
        if self
            .records()?
            .record_list(&scope, "sync-run")?
            .iter()
            .any(|(_, s)| {
                decode::<Run>(s)
                    .is_ok_and(|r| r.config_id == config.config_id && r.state == "running")
            })
        {
            return Err(ClientError::new(
                ErrorCode::Conflict,
                "同步正在执行，不能修改配置。",
            ));
        }
        if config.first_run {
            config.allow_deletes = false;
        }
        self.check_scope(&scope, ctx).await?;
        self.records()?
            .record_put(&scope, "sync-config", &config.config_id, &encode(&config)?)?;
        if reset_baseline {
            self.records()?
                .record_remove(&scope, "sync-baseline", &config.config_id)?;
        }
        self.records()?
            .record_remove(&scope, "sync-schedule", &config.config_id)?;
        Ok(config)
    }
    async fn remote_ancestor(
        &self,
        ancestor: &str,
        child: &str,
        scope: &str,
        ctx: &RequestContext,
    ) -> Result<bool, ClientError> {
        let mut id = child.to_owned();
        let mut seen = BTreeSet::new();
        for _ in 0..128 {
            self.check_scope(scope, ctx).await?;
            if id == ancestor {
                return Ok(true);
            }
            if id == "root" {
                return Ok(false);
            }
            if !seen.insert(id.clone()) {
                return Err(invalid());
            }
            let entry = ctx.run(self.provider.stat(&id)).await?;
            id = entry.parent_id.ok_or_else(invalid)?;
        }
        Err(invalid())
    }
    async fn sync_snapshot(
        &self,
        config: &SyncConfig,
        scope: &str,
        ctx: &RequestContext,
    ) -> Result<Snapshot, ClientError> {
        let mut remote = BTreeMap::new();
        let mut folded = BTreeSet::new();
        for item in self.walk(&config.remote_id, scope, ctx, None).await? {
            let path = item.relative.join("/");
            if excluded(config, &path) {
                continue;
            }
            valid_relative(&path)?;
            if !folded.insert(path.to_lowercase()) || remote.insert(path, item.entry).is_some() {
                return Err(ClientError::new(
                    ErrorCode::Conflict,
                    "远端存在大小写重名或重复路径，不能安全同步到本地。",
                ));
            }
        }
        let local = local_snapshot(config, ctx).await?;
        self.check_scope(scope, ctx).await?;
        Ok(Snapshot { local, remote })
    }
    pub async fn sync_scan(&self, id: &str, ctx: &RequestContext) -> Result<ScanRef, ClientError> {
        let _guard = self.sync_gate.lock().await;
        let scope = ctx.run(self.provider.account_scope()).await?;
        let config: SyncConfig = decode(
            &self
                .records()?
                .record_get(&scope, "sync-config", id)?
                .ok_or_else(invalid)?,
        )?;
        if self
            .records()?
            .record_list(&scope, "sync-run")?
            .iter()
            .any(|(_, s)| decode::<Run>(s).is_ok_and(|r| r.config_id == id && r.state == "running"))
        {
            return Err(ClientError::new(
                ErrorCode::Conflict,
                "同步正在执行，请等待完成后再扫描。",
            ));
        }
        let snapshot = match self.sync_snapshot(&config, &scope, ctx).await {
            Ok(s) => s,
            Err(e) => {
                self.sync_log(
                    &scope,
                    id,
                    "error",
                    "扫描失败，未生成删除计划。",
                    Some(e.clone()),
                )?;
                return Err(e);
            }
        };
        let baseline = if config.first_run {
            None
        } else {
            self.records()?
                .record_get(&scope, "sync-baseline", id)?
                .map(|v| decode::<Baseline>(&v))
                .transpose()?
        };
        let (plan, steps) = make_plan(&config, &snapshot, baseline.as_ref())?;
        let record = PlanRecord {
            config: config.clone(),
            scope: scope.clone(),
            snapshot,
            plan: plan.clone(),
            steps,
        };
        self.records()?
            .record_put(&scope, "sync-plan", &plan.plan_id, &encode(&record)?)?;
        self.records()?
            .record_put(&scope, "sync-latest", id, &encode(&plan.plan_id)?)?;
        self.sync_log(&scope, id, "plan", "完整扫描完成，操作计划等待确认。", None)?;
        Ok(ScanRef {
            scan_id: plan.plan_id,
        })
    }
    pub async fn sync_plan(&self, id: &str, ctx: &RequestContext) -> Result<SyncPlan, ClientError> {
        let scope = ctx.run(self.provider.account_scope()).await?;
        let latest: String = decode(
            &self
                .records()?
                .record_get(&scope, "sync-latest", id)?
                .ok_or_else(|| ClientError::new(ErrorCode::NotFound, "请先扫描同步目录。"))?,
        )?;
        let record: PlanRecord = decode(
            &self
                .records()?
                .record_get(&scope, "sync-plan", &latest)?
                .ok_or_else(invalid)?,
        )?;
        Ok(record.plan)
    }
    pub async fn sync_apply(
        &self,
        id: &str,
        confirm_deletes: bool,
        ctx: &RequestContext,
    ) -> Result<TaskRef, ClientError> {
        let _guard = self.sync_gate.lock().await;
        let scope = ctx.run(self.provider.account_scope()).await?;
        let record: PlanRecord = decode(
            &self
                .records()?
                .record_get(&scope, "sync-plan", id)?
                .ok_or_else(invalid)?,
        )?;
        if record.plan.has_unresolved_conflicts {
            return Err(ClientError::new(
                ErrorCode::Conflict,
                "有未解决的双端变化，已保留两侧文件；处理后重新扫描。",
            ));
        }
        if record.plan.has_deletes
            && (!confirm_deletes || !record.config.allow_deletes || record.config.first_run)
        {
            return Err(ClientError::new(
                ErrorCode::Conflict,
                "删除需要已建立基线、启用删除并明确确认。",
            ));
        }
        let key = format!("sync-apply:{id}");
        let fp = fingerprint(&[&encode(&record.plan)?]);
        if let Some(result) = self.records()?.begin_value(&scope, &key, &fp)? {
            return serde_json::from_value(result).map_err(|_| invalid());
        }
        let result = self.create_sync_run(&record, &scope, ctx).await;
        self.records()?.finish_value(
            &scope,
            &key,
            &result.as_ref().map(|v| json!(v)).map_err(Clone::clone),
        )?;
        result
    }
    async fn create_sync_run(
        &self,
        record: &PlanRecord,
        scope: &str,
        ctx: &RequestContext,
    ) -> Result<TaskRef, ClientError> {
        let config: SyncConfig = decode(
            &self
                .records()?
                .record_get(scope, "sync-config", &record.config.config_id)?
                .ok_or_else(invalid)?,
        )?;
        if config != record.config {
            return Err(conflict());
        }
        let baseline = if config.first_run {
            None
        } else {
            self.records()?
                .record_get(scope, "sync-baseline", &config.config_id)?
                .map(|v| decode::<Baseline>(&v))
                .transpose()?
        };
        if baseline
            .as_ref()
            .map(|b| b.revision.as_str())
            .unwrap_or("0")
            != record.plan.base_revision
        {
            return Err(conflict());
        }
        if self.sync_snapshot(&config, scope, ctx).await? != record.snapshot {
            return Err(conflict());
        }
        if self
            .records()?
            .record_list(scope, "sync-run")?
            .iter()
            .any(|(_, s)| {
                decode::<Run>(s)
                    .is_ok_and(|r| r.config_id == config.config_id && r.state == "running")
            })
        {
            return Err(conflict());
        }
        let job_id = uuid::Uuid::new_v4().to_string();
        let run = Run {
            job_id: job_id.clone(),
            scope: scope.into(),
            plan_id: record.plan.plan_id.clone(),
            config_id: config.config_id,
            state: "running".into(),
            index: 0,
            task_id: None,
            error: None,
        };
        self.records()?
            .record_put(scope, "sync-run", &job_id, &encode(&run)?)?;
        Ok(TaskRef { task_id: job_id })
    }
    pub async fn sync_run_status(
        &self,
        id: &str,
        ctx: &RequestContext,
    ) -> Result<Value, ClientError> {
        let scope = ctx.run(self.provider.account_scope()).await?;
        let run: Run = decode(
            &self
                .records()?
                .record_get(&scope, "sync-run", id)?
                .ok_or_else(invalid)?,
        )?;
        Ok(
            json!({"job_id":run.job_id,"config_id":run.config_id,"state":run.state,"error":run.error}),
        )
    }
    pub async fn sync_history(
        &self,
        id: &str,
        cursor: Option<&str>,
        ctx: &RequestContext,
    ) -> Result<Page<SyncEvent>, ClientError> {
        let scope = ctx.run(self.provider.account_scope()).await?;
        let mut items = self
            .records()?
            .record_list(&scope, "sync-history")?
            .into_iter()
            .map(|(_, v)| decode::<SyncEvent>(&v))
            .collect::<Result<Vec<_>, _>>()?;
        items.retain(|e| e.config_id == id);
        items.sort_by(|a, b| b.at.cmp(&a.at).then(b.id.cmp(&a.id)));
        let start = cursor
            .map(|c| c.parse::<usize>().map_err(|_| invalid()))
            .transpose()?
            .unwrap_or(0);
        if start > items.len() {
            return Err(invalid());
        }
        let next_cursor = (start + 100 < items.len()).then(|| (start + 100).to_string());
        Ok(Page {
            items: items.into_iter().skip(start).take(100).collect(),
            next_cursor,
        })
    }
    fn sync_log(
        &self,
        scope: &str,
        id: &str,
        kind: &str,
        summary: &str,
        error: Option<ClientError>,
    ) -> Result<(), ClientError> {
        let event = SyncEvent {
            id: uuid::Uuid::new_v4().to_string(),
            config_id: id.into(),
            at: now(),
            kind: kind.into(),
            summary: summary.into(),
            error,
        };
        self.records()?
            .record_put(scope, "sync-history", &event.id, &encode(&event)?)
    }
    pub async fn sync_reconcile(&self, ctx: &RequestContext) -> Result<Vec<Value>, ClientError> {
        let Ok(_guard) = self.sync_gate.try_lock() else {
            return Ok(vec![]);
        };
        let scope = ctx.run(self.provider.account_scope()).await?;
        let mut events = Vec::new();
        for (_, value) in self.records()?.record_list(&scope, "sync-run")? {
            let mut run: Run = decode(&value)?;
            if run.state != "running" {
                continue;
            }
            let record: PlanRecord = decode(
                &self
                    .records()?
                    .record_get(&scope, "sync-plan", &run.plan_id)?
                    .ok_or_else(invalid)?,
            )?;
            let result = self.sync_step(&record, &mut run, ctx).await;
            if let Err(e) = result {
                run.state = "failed".into();
                run.error = Some(e.clone());
                self.sync_log(
                    &scope,
                    &run.config_id,
                    "error",
                    "同步停止，旧基线与备份已保留。",
                    Some(e),
                )?;
            }
            self.records()?
                .record_put(&scope, "sync-run", &run.job_id, &encode(&run)?)?;
            events.push(json!({"job_id":run.job_id,"config_id":run.config_id,"state":run.state,"error":run.error}));
        }
        Ok(events)
    }
    /// A short host timer calls this. Only explicitly opted-in configurations
    /// can submit, and any deletion/conflict always waits for a user decision.
    pub async fn sync_automatic(&self, ctx: &RequestContext) -> Result<Vec<Value>, ClientError> {
        let Ok(_automatic) = self.sync_auto_gate.try_lock() else {
            return Ok(vec![]);
        };
        let scope = ctx.run(self.provider.account_scope()).await?;
        let configs = self.sync_configs_list(ctx).await?;
        let mut events = Vec::new();
        for config in configs.into_iter().filter(|c| c.auto_enabled) {
            self.check_scope(&scope, ctx).await?;
            let tick = time::OffsetDateTime::now_utc().unix_timestamp();
            let mut schedule = self
                .records()?
                .record_get(&scope, "sync-schedule", &config.config_id)?
                .map(|v| decode::<Schedule>(&v))
                .transpose()?
                .unwrap_or_default();
            if schedule.next_attempt > tick {
                continue;
            }
            if self
                .records()?
                .record_list(&scope, "sync-run")?
                .iter()
                .any(|(_, s)| {
                    decode::<Run>(s)
                        .is_ok_and(|r| r.config_id == config.config_id && r.state == "running")
                })
            {
                continue;
            }
            // Persist attempt spacing before work. A process crash or duplicate
            // timer invocation cannot turn a failed scan into a tight retry loop.
            schedule.next_attempt = tick + config.interval_seconds as i64;
            self.records()?.record_put(
                &scope,
                "sync-schedule",
                &config.config_id,
                &encode(&schedule)?,
            )?;
            let result=async{
                self.sync_scan(&config.config_id,ctx).await?;
                let plan=self.sync_plan(&config.config_id,ctx).await?;
                let current:SyncConfig=decode(&self.records()?.record_get(&scope,"sync-config",&config.config_id)?.ok_or_else(invalid)?)?;
                if current!=config||!current.auto_enabled{return Err(conflict())}
                if plan.has_deletes||plan.has_unresolved_conflicts{
                    self.sync_log(&scope,&config.config_id,"plan","自动同步已生成计划；冲突或删除等待人工确认。",None)?;
                    events.push(json!({"config_id":config.config_id,"plan_id":plan.plan_id,"state":"needs_review"}));
                    return Ok(());
                }
                if plan.actions.is_empty(){return Ok(());}
                // Config may have been disabled/changed while scanning; apply
                // also compares its exact saved config and full snapshots.
                let job=self.sync_apply(&plan.plan_id,false,ctx).await?;
                events.push(json!({"config_id":config.config_id,"job_id":job.task_id,"state":"running"}));Ok::<(),ClientError>(())
            }.await;
            match result {
                Ok(()) => schedule.failures = 0,
                Err(error) => {
                    schedule.failures = schedule.failures.saturating_add(1);
                    let backoff = config
                        .interval_seconds
                        .saturating_mul(1u64 << schedule.failures.min(6))
                        .min(3600);
                    schedule.next_attempt = tick + backoff as i64;
                    events
                        .push(json!({"config_id":config.config_id,"state":"failed","error":error}));
                }
            }
            self.records()?.record_put(
                &scope,
                "sync-schedule",
                &config.config_id,
                &encode(&schedule)?,
            )?;
        }
        Ok(events)
    }
    async fn sync_step(
        &self,
        record: &PlanRecord,
        run: &mut Run,
        ctx: &RequestContext,
    ) -> Result<(), ClientError> {
        self.check_scope(&run.scope, ctx).await?;
        if let Some(id) = &run.task_id {
            let task = self.task(id)?;
            match task.state {
                TaskState::Completed => {
                    run.index += 1;
                    run.task_id = None;
                }
                TaskState::Failed | TaskState::Cancelled => {
                    return Err(task.error.unwrap_or_else(conflict))
                }
                _ => return Ok(()),
            }
        }
        if run.index == record.steps.len() {
            let snapshot = self.sync_snapshot(&record.config, &run.scope, ctx).await?;
            validate_completed(record, &snapshot)?;
            let baseline = Baseline {
                revision: run.job_id.clone(),
                snapshot,
            };
            self.records()?.record_put(
                &run.scope,
                "sync-baseline",
                &run.config_id,
                &encode(&baseline)?,
            )?;
            let mut config = record.config.clone();
            config.first_run = false;
            config.last_applied_at = Some(now());
            self.records()?.record_put(
                &run.scope,
                "sync-config",
                &run.config_id,
                &encode(&config)?,
            )?;
            run.state = "completed".into();
            self.sync_log(
                &run.scope,
                &run.config_id,
                "apply",
                "同步任务均完成，已保存新基线；旧副本保留在备份目录。",
                None,
            )?;
            return Ok(());
        }
        let step = &record.steps[run.index];
        let path = Path::new(&record.config.local_path).join(&step.path);
        valid_relative(&step.path)?;
        let key = format!("sync:{}:{}", run.job_id, run.index);
        // A child task may have committed before the aggregate run did. Its
        // receipt is replayed before rechecking/moving an already-backed-up path.
        if let Some(payload) = self.records()?.record_get(&run.scope, "sync-step", &key)? {
            let task: Option<String> = decode(&payload)?;
            if let Some(id) = task {
                run.task_id = Some(id);
            } else {
                run.index += 1;
            }
            return Ok(());
        }
        match step.kind {
            StepKind::MkdirLocal => {
                ensure_local_parent(Path::new(&record.config.local_path), &path)?;
                self.records()?
                    .record_put(&run.scope, "sync-step", &key, "null")?;
                run.index += 1;
            }
            StepKind::MkdirRemote => {
                self.sync_remote_directory(&record.config, &step.path, &key, &run.scope, ctx)
                    .await?;
                self.records()?
                    .record_put(&run.scope, "sync-step", &key, "null")?;
                run.index += 1;
            }
            StepKind::Upload => {
                let expected = record.snapshot.local.get(&step.path).ok_or_else(invalid)?;
                if read_local_node(&path, ctx).await? != *expected {
                    return Err(conflict());
                }
                if step.backup {
                    self.backup_remote(record, run, &step.path, ctx).await?;
                }
                let parent = step.path.rsplit_once('/').map(|(p, _)| p).unwrap_or("");
                let parent = self
                    .sync_remote_directory(&record.config, parent, &key, &run.scope, ctx)
                    .await?;
                let task = self
                    .upload_start_at(
                        path.to_str().ok_or_else(invalid)?,
                        &parent,
                        &key,
                        &run.scope,
                        ctx,
                    )
                    .await?;
                self.records()?.record_put(
                    &run.scope,
                    "sync-step",
                    &key,
                    &encode(&Some(task.task_id.clone()))?,
                )?;
                run.task_id = Some(task.task_id);
            }
            StepKind::Download => {
                let entry = record.snapshot.remote.get(&step.path).ok_or_else(invalid)?;
                let current = ctx.run(self.provider.stat(&entry.id)).await?;
                if current != *entry {
                    return Err(conflict());
                }
                if step.backup {
                    backup_local(record, run, &step.path, ctx).await?;
                }
                let parent = path.parent().ok_or_else(invalid)?;
                ensure_local_parent(Path::new(&record.config.local_path), parent)?;
                let task = self
                    .transfer_start_at(
                        &entry.id,
                        parent.to_str().ok_or_else(invalid)?,
                        Some(&key),
                        &run.scope,
                        ctx,
                    )
                    .await?;
                self.records()?.record_put(
                    &run.scope,
                    "sync-step",
                    &key,
                    &encode(&Some(task.task_id.clone()))?,
                )?;
                run.task_id = Some(task.task_id);
            }
            StepKind::DeleteLocal => {
                backup_local(record, run, &step.path, ctx).await?;
                self.records()?
                    .record_put(&run.scope, "sync-step", &key, "null")?;
                run.index += 1;
            }
            StepKind::DeleteRemote => {
                let entry = record.snapshot.remote.get(&step.path).ok_or_else(invalid)?;
                if ctx.run(self.provider.stat(&entry.id)).await? != *entry {
                    return Err(conflict());
                }
                self.files_mutate_at(
                    &FileMutation {
                        op: MutateOp::Trash,
                        ids: vec![entry.id.clone()],
                        target: None,
                    },
                    &key,
                    &run.scope,
                    ctx,
                )
                .await?;
                self.records()?
                    .record_put(&run.scope, "sync-step", &key, "null")?;
                run.index += 1;
            }
        }
        Ok(())
    }
    async fn sync_remote_directory(
        &self,
        config: &SyncConfig,
        relative: &str,
        key: &str,
        scope: &str,
        ctx: &RequestContext,
    ) -> Result<String, ClientError> {
        let mut parent = config.remote_id.clone();
        if relative.is_empty() {
            return Ok(parent);
        }
        for (depth, name) in relative.split('/').enumerate() {
            let mut cursor = None;
            let mut found = None;
            let mut seen = BTreeSet::new();
            loop {
                self.check_scope(scope, ctx).await?;
                let page = ctx
                    .run(self.provider.list(&parent, cursor.as_deref(), 500))
                    .await?;
                for entry in page.items {
                    if entry.name.eq_ignore_ascii_case(name) {
                        if found.is_some() || entry.name != name || entry.kind != EntryKind::Folder
                        {
                            return Err(conflict());
                        }
                        found = Some(entry.id)
                    }
                }
                let Some(next) = page.next_cursor else { break };
                if !seen.insert(next.clone()) {
                    return Err(invalid());
                }
                cursor = Some(next);
            }
            parent = if let Some(found) = found {
                found
            } else {
                let result = self
                    .files_mutate_at(
                        &FileMutation {
                            op: MutateOp::Mkdir,
                            ids: vec![],
                            target: Some(MutateTarget {
                                name: Some(name.into()),
                                parent_id: Some(parent),
                            }),
                        },
                        &format!("{key}:mkdir:{depth}"),
                        scope,
                        ctx,
                    )
                    .await?;
                result.created.first().ok_or_else(invalid)?.id.clone()
            };
        }
        Ok(parent)
    }
    async fn backup_remote(
        &self,
        record: &PlanRecord,
        run: &Run,
        path: &str,
        ctx: &RequestContext,
    ) -> Result<(), ClientError> {
        let entry = record.snapshot.remote.get(path).ok_or_else(invalid)?;
        let key = format!("sync:{}:{}:backup", run.job_id, run.index);
        let mut current = ctx.run(self.provider.stat(&entry.id)).await?;
        let relative = path
            .rsplit_once('/')
            .map(|(p, _)| format!(".nekopak-backup/{}/{p}", run.job_id))
            .unwrap_or_else(|| format!(".nekopak-backup/{}", run.job_id));
        let parent = self
            .sync_remote_directory(&record.config, &relative, &key, &run.scope, ctx)
            .await?;
        if current.parent_id.as_deref() == Some(&parent) {
            current.parent_id = entry.parent_id.clone();
            if current == *entry {
                return Ok(());
            }
        }
        if current != *entry {
            return Err(conflict());
        }
        self.files_mutate_at(
            &FileMutation {
                op: MutateOp::Move,
                ids: vec![entry.id.clone()],
                target: Some(MutateTarget {
                    parent_id: Some(parent),
                    name: None,
                }),
            },
            &key,
            &run.scope,
            ctx,
        )
        .await?;
        Ok(())
    }
}

async fn local_snapshot(
    config: &SyncConfig,
    ctx: &RequestContext,
) -> Result<BTreeMap<String, LocalNode>, ClientError> {
    let root = Path::new(&config.local_path);
    let _locks = downloads::lock_parents(root)?;
    let mut pending = vec![(root.to_owned(), String::new())];
    let mut entries = BTreeMap::new();
    let mut folded = BTreeSet::new();
    while let Some((directory, relative)) = pending.pop() {
        if relative.split('/').count() > 128 {
            return Err(invalid());
        }
        let _locks = downloads::lock_parents(&directory)?;
        for item in std::fs::read_dir(&directory).map_err(|_| invalid())? {
            let item = item.map_err(|_| invalid())?;
            let name = item.file_name().into_string().map_err(|_| invalid())?;
            let rel = if relative.is_empty() {
                name
            } else {
                format!("{relative}/{name}")
            };
            if excluded(config, &rel) {
                continue;
            }
            valid_relative(&rel)?;
            let metadata = item.file_type().map_err(|_| invalid())?;
            if metadata.is_symlink() {
                return Err(ClientError::new(
                    ErrorCode::Conflict,
                    "同步目录包含符号链接；未把链接替换的文件视为已删除，请调整排除规则。",
                ));
            }
            if !folded.insert(rel.to_lowercase()) || entries.len() >= MAX_FILES {
                return Err(invalid());
            }
            let node = read_local_node(&item.path(), ctx).await?;
            if node.kind == EntryKind::Folder {
                pending.push((item.path(), rel.clone()));
            }
            entries.insert(rel, node);
        }
    }
    Ok(entries)
}
async fn read_local_node(path: &Path, ctx: &RequestContext) -> Result<LocalNode, ClientError> {
    let metadata = std::fs::symlink_metadata(path).map_err(|_| conflict())?;
    if metadata.is_dir() {
        let _locks = downloads::lock_parents(path)?;
        return Ok(LocalNode {
            kind: EntryKind::Folder,
            size: 0,
            identity: String::new(),
            sha256: None,
            gcid: None,
        });
    }
    let mut file = local::open_readonly(path)?;
    let identity = local::read_identity(&file)?;
    let size = file.metadata().map_err(|_| invalid())?.len();
    let block = if size <= 128 * 1024 * 1024 {
        262144
    } else if size <= 256 * 1024 * 1024 {
        524288
    } else if size <= 512 * 1024 * 1024 {
        1048576
    } else {
        2097152
    };
    let mut buffer = vec![0; block];
    let mut remaining = size;
    let mut sha = Sha256::new();
    let mut gcid = Sha1::new();
    while remaining > 0 {
        ctx.run(async { Ok(()) }).await?;
        let take = (remaining as usize).min(block);
        file.read_exact(&mut buffer[..take])
            .map_err(|_| invalid())?;
        sha.update(&buffer[..take]);
        gcid.update(Sha1::digest(&buffer[..take]));
        remaining -= take as u64;
        tokio::task::yield_now().await;
    }
    if local::read_identity(&file)? != identity {
        return Err(conflict());
    }
    Ok(LocalNode {
        kind: EntryKind::File,
        size,
        identity,
        sha256: Some(format!("{:x}", sha.finalize())),
        gcid: Some(format!("{:X}", gcid.finalize())),
    })
}
fn equivalent(local: &LocalNode, remote: &RemoteEntry) -> bool {
    if local.kind != remote.kind {
        return false;
    }
    if local.kind == EntryKind::Folder {
        return true;
    }
    if local.size != remote.size_bytes.0 {
        return false;
    }
    remote.hash.as_ref().is_some_and(|h| match h.algorithm {
        HashAlgorithm::Gcid => local
            .gcid
            .as_ref()
            .is_some_and(|s| s.eq_ignore_ascii_case(&h.value)),
        HashAlgorithm::Sha256 => local
            .sha256
            .as_ref()
            .is_some_and(|s| s.eq_ignore_ascii_case(&h.value)),
        _ => false,
    })
}
fn make_plan(
    config: &SyncConfig,
    snapshot: &Snapshot,
    baseline: Option<&Baseline>,
) -> Result<(SyncPlan, Vec<Step>), ClientError> {
    let mut plan = SyncPlan {
        plan_id: uuid::Uuid::new_v4().to_string(),
        base_revision: baseline
            .map(|b| b.revision.clone())
            .unwrap_or_else(|| "0".into()),
        source: if config.mode == SyncMode::DownloadOnly {
            config.remote_path.clone()
        } else {
            config.local_path.clone()
        },
        destination: if config.mode == SyncMode::DownloadOnly {
            config.local_path.clone()
        } else {
            config.remote_path.clone()
        },
        mode: config.mode,
        actions: vec![],
        warnings: vec!["更新文件会保留目标旧副本到 .nekopak-backup；不会永久删除。".into()],
        has_unresolved_conflicts: false,
        has_deletes: false,
    };
    let mut steps = Vec::new();
    let paths: BTreeSet<_> = snapshot
        .local
        .keys()
        .chain(snapshot.remote.keys())
        .cloned()
        .collect();
    for path in paths {
        let l = snapshot.local.get(&path);
        let r = snapshot.remote.get(&path);
        if l.zip(r).is_some_and(|(l, r)| equivalent(l, r)) {
            continue;
        }
        let old_l = baseline.and_then(|b| b.snapshot.local.get(&path));
        let old_r = baseline.and_then(|b| b.snapshot.remote.get(&path));
        let lc = l != old_l;
        let rc = r != old_r;
        let mut backup = false;
        let action = match (l, r) {
            (Some(l), None) => {
                if old_r.is_some()
                    && old_l.is_some()
                    && !lc
                    && config.allow_deletes
                    && baseline.is_some()
                    && config.mode != SyncMode::UploadOnly
                {
                    if l.kind == EntryKind::File {
                        Some(StepKind::DeleteLocal)
                    } else {
                        None
                    }
                } else if config.mode != SyncMode::DownloadOnly {
                    Some(if l.kind == EntryKind::Folder {
                        StepKind::MkdirRemote
                    } else {
                        StepKind::Upload
                    })
                } else {
                    None
                }
            }
            (None, Some(r)) => {
                if old_l.is_some()
                    && old_r.is_some()
                    && !rc
                    && config.allow_deletes
                    && baseline.is_some()
                    && config.mode != SyncMode::DownloadOnly
                {
                    if r.kind == EntryKind::File {
                        Some(StepKind::DeleteRemote)
                    } else {
                        None
                    }
                } else if config.mode != SyncMode::UploadOnly {
                    Some(if r.kind == EntryKind::Folder {
                        StepKind::MkdirLocal
                    } else {
                        StepKind::Download
                    })
                } else {
                    None
                }
            }
            (Some(l), Some(r)) => {
                if !lc && !rc {
                    None
                } else if baseline.is_some()
                    && l.kind == EntryKind::File
                    && r.kind == EntryKind::File
                    && lc
                    && !rc
                    && config.mode != SyncMode::DownloadOnly
                {
                    backup = true;
                    Some(StepKind::Upload)
                } else if baseline.is_some()
                    && l.kind == EntryKind::File
                    && r.kind == EntryKind::File
                    && rc
                    && !lc
                    && config.mode != SyncMode::UploadOnly
                {
                    backup = true;
                    Some(StepKind::Download)
                } else {
                    plan.has_unresolved_conflicts = true;
                    plan.actions.push(SyncAction {
                        id: fingerprint(&[&path, "conflict"]),
                        op: SyncOp::KeepBoth,
                        path: path.clone(),
                        reason: "双方不同或内容身份未知，保留两端并等待人工解决。".into(),
                        destructive: false,
                    });
                    None
                }
            }
            _ => None,
        };
        if let Some(kind) = action {
            let op = match kind {
                StepKind::MkdirLocal | StepKind::MkdirRemote => SyncOp::Mkdir,
                StepKind::Upload => SyncOp::Upload,
                StepKind::Download => SyncOp::Download,
                StepKind::DeleteLocal | StepKind::DeleteRemote => SyncOp::Delete,
            };
            let destructive = op == SyncOp::Delete;
            plan.has_deletes |= destructive;
            plan.actions.push(SyncAction {
                id: fingerprint(&[&path, &format!("{op:?}")]),
                op,
                path: path.clone(),
                reason: if backup {
                    "目标旧副本先保留到备份目录，再同步已变化文件。"
                } else if destructive {
                    "基线确认源端已删除；目标移至备份或回收站。"
                } else {
                    "源端存在，目标尚不存在。"
                }
                .into(),
                destructive,
            });
            steps.push(Step { kind, path, backup });
        }
    }
    // Parents before children; deletion descendants first to preserve checks.
    steps.sort_by_key(|s| {
        let del = matches!(s.kind, StepKind::DeleteLocal | StepKind::DeleteRemote);
        (
            del,
            if del {
                usize::MAX - s.path.split('/').count()
            } else {
                s.path.split('/').count()
            },
            s.path.clone(),
        )
    });
    Ok((plan, steps))
}
fn ensure_local_parent(base: &Path, path: &Path) -> Result<(), ClientError> {
    let relative = path.strip_prefix(base).map_err(|_| invalid())?;
    let mut current = base.to_owned();
    for part in relative.components() {
        let _locks = downloads::lock_parents(&current)?;
        current.push(part);
        if !current.exists() {
            std::fs::create_dir(&current).map_err(|_| invalid())?
        }
        let _locks = downloads::lock_parents(&current)?;
    }
    Ok(())
}
async fn backup_local(
    record: &PlanRecord,
    run: &Run,
    relative: &str,
    ctx: &RequestContext,
) -> Result<(), ClientError> {
    let base = Path::new(&record.config.local_path);
    let source = base.join(relative);
    let target = base
        .join(".nekopak-backup")
        .join(&run.job_id)
        .join(relative);
    let expected = record.snapshot.local.get(relative).ok_or_else(invalid)?;
    if target.exists() {
        if source.exists() || read_local_node(&target, ctx).await?.sha256 != expected.sha256 {
            return Err(conflict());
        }
        return Ok(());
    }
    if read_local_node(&source, ctx).await? != *expected {
        return Err(conflict());
    }
    ensure_local_parent(base, target.parent().ok_or_else(invalid)?)?;
    let _source_lock = downloads::lock_parents(source.parent().ok_or_else(invalid)?)?;
    let _target_lock = downloads::lock_parents(target.parent().ok_or_else(invalid)?)?;
    let file = downloads::open_part(&source, false)?;
    if local::read_identity(&file)? != expected.identity {
        return Err(conflict());
    }
    downloads::publish(&file, &source, &target)?;
    Ok(())
}

fn validate_completed(record: &PlanRecord, current: &Snapshot) -> Result<(), ClientError> {
    let touched: BTreeSet<_> = record.steps.iter().map(|s| s.path.as_str()).collect();
    for (path, node) in &current.local {
        if !touched.contains(path.as_str())
            && node.kind == EntryKind::File
            && record.snapshot.local.get(path) != Some(node)
        {
            return Err(conflict());
        }
    }
    for (path, node) in &current.remote {
        if !touched.contains(path.as_str())
            && node.kind == EntryKind::File
            && record.snapshot.remote.get(path) != Some(node)
        {
            return Err(conflict());
        }
    }
    for (path, node) in &record.snapshot.local {
        if !touched.contains(path.as_str())
            && node.kind == EntryKind::File
            && current.local.get(path) != Some(node)
        {
            return Err(conflict());
        }
    }
    for (path, node) in &record.snapshot.remote {
        if !touched.contains(path.as_str())
            && node.kind == EntryKind::File
            && current.remote.get(path) != Some(node)
        {
            return Err(conflict());
        }
    }
    for step in &record.steps {
        match step.kind {
            StepKind::Upload => {
                let local = current.local.get(&step.path).ok_or_else(conflict)?;
                if record.snapshot.local.get(&step.path) != Some(local)
                    || !current
                        .remote
                        .get(&step.path)
                        .is_some_and(|r| equivalent(local, r))
                {
                    return Err(conflict());
                }
            }
            StepKind::Download => {
                let remote = current.remote.get(&step.path).ok_or_else(conflict)?;
                if record.snapshot.remote.get(&step.path) != Some(remote)
                    || !current
                        .local
                        .get(&step.path)
                        .is_some_and(|l| equivalent(l, remote))
                {
                    return Err(conflict());
                }
            }
            StepKind::DeleteLocal => {
                if current.local.contains_key(&step.path) {
                    return Err(conflict());
                }
            }
            StepKind::DeleteRemote => {
                if current.remote.contains_key(&step.path) {
                    return Err(conflict());
                }
            }
            StepKind::MkdirLocal => {
                if current
                    .local
                    .get(&step.path)
                    .is_none_or(|n| n.kind != EntryKind::Folder)
                {
                    return Err(conflict());
                }
            }
            StepKind::MkdirRemote => {
                if current
                    .remote
                    .get(&step.path)
                    .is_none_or(|n| n.kind != EntryKind::Folder)
                {
                    return Err(conflict());
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn config() -> SyncConfig {
        SyncConfig {
            config_id: "sync-one".into(),
            name: "Sync".into(),
            mode: SyncMode::Bidirectional,
            local_path: "C:/sample".into(),
            remote_id: "root".into(),
            remote_path: "/".into(),
            exclude_globs: vec!["*.tmp".into()],
            allow_deletes: false,
            first_run: true,
            auto_enabled: false,
            interval_seconds: 300,
            last_applied_at: None,
        }
    }
    fn local(hash: &str) -> LocalNode {
        LocalNode {
            kind: EntryKind::File,
            size: 10,
            identity: hash.into(),
            sha256: Some(hash.into()),
            gcid: None,
        }
    }
    fn remote(hash: &str) -> RemoteEntry {
        RemoteEntry {
            id: "file-one".into(),
            parent_id: Some("root".into()),
            name: "sample.bin".into(),
            kind: EntryKind::File,
            size_bytes: DecimalU64(10),
            modified_at: None,
            etag: Some(hash.into()),
            hash: Some(ContentHash {
                algorithm: HashAlgorithm::Sha256,
                value: hash.into(),
            }),
            mime_type: None,
            starred: None,
        }
    }
    #[test]
    fn three_way_changes_conflict_and_default_deletions_are_preserved() {
        let baseline = Baseline {
            revision: "one".into(),
            snapshot: Snapshot {
                local: BTreeMap::from([("sample.bin".into(), local("old"))]),
                remote: BTreeMap::from([("sample.bin".into(), remote("old"))]),
            },
        };
        let changed = Snapshot {
            local: BTreeMap::from([("sample.bin".into(), local("local-change"))]),
            remote: BTreeMap::from([("sample.bin".into(), remote("remote-change"))]),
        };
        let (plan, steps) = make_plan(&config(), &changed, Some(&baseline)).unwrap();
        assert!(plan.has_unresolved_conflicts);
        assert!(steps.is_empty());
        let deleted = Snapshot {
            local: BTreeMap::new(),
            remote: baseline.snapshot.remote.clone(),
        };
        let (plan, steps) = make_plan(&config(), &deleted, Some(&baseline)).unwrap();
        assert!(!plan.has_deletes);
        assert!(matches!(steps[0].kind, StepKind::Download));
    }
    #[test]
    fn completed_run_cannot_absorb_unrelated_changes_into_baseline() {
        let config = config();
        let initial = Snapshot {
            local: BTreeMap::from([("sample.bin".into(), local("old"))]),
            remote: BTreeMap::from([("sample.bin".into(), remote("old"))]),
        };
        let (plan, steps) = make_plan(&config, &initial, None).unwrap();
        let record = PlanRecord {
            config,
            scope: "one".into(),
            snapshot: initial.clone(),
            plan,
            steps,
        };
        let mut current = initial;
        current
            .local
            .insert("sample.bin".into(), local("late-change"));
        assert!(validate_completed(&record, &current).is_err());
    }
    #[test]
    fn patterns_match_paths_and_reserved_backup_stays_out() {
        let config = config();
        assert!(excluded(&config, "nested/cache.tmp"));
        assert!(excluded(&config, ".nekopak-backup/one/file.bin"));
        assert!(!excluded(&config, "nested/file.bin"));
        assert!(glob_match("a?c*", "abcde"));
    }
}
