use async_trait::async_trait;
use nekopak_core::AppService;
use nekopak_domain::*;
use nekopak_provider::{ProviderPort, RequestContext};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex,
};
struct Upload {
    bytes: Mutex<Vec<u8>>,
    prepared: AtomicUsize,
    complete: Mutex<bool>,
    forgot: AtomicUsize,
}
fn result() -> RemoteEntry {
    RemoteEntry {
        id: "remote".into(),
        parent_id: Some("root".into()),
        name: "source.bin".into(),
        kind: EntryKind::File,
        size_bytes: DecimalU64(6),
        modified_at: None,
        etag: None,
        hash: None,
        mime_type: None,
        starred: None,
    }
}
#[async_trait]
impl ProviderPort for Upload {
    fn id(&self) -> BackendId {
        BackendId::RustCompat
    }
    async fn account_scope(&self) -> Result<String, ClientError> {
        Ok("one".into())
    }
    async fn discover(&self) -> Result<CapabilityReport, ClientError> {
        unreachable!()
    }
    async fn list(
        &self,
        _: &str,
        _: Option<&str>,
        _: usize,
    ) -> Result<Page<RemoteEntry>, ClientError> {
        unreachable!()
    }
    async fn stat(&self, _: &str) -> Result<RemoteEntry, ClientError> {
        Ok(result())
    }
    async fn quota(&self) -> Result<QuotaReport, ClientError> {
        unreachable!()
    }
    async fn upload_prepare(
        &self,
        _: &str,
        _: &str,
        _: &str,
    ) -> Result<UploadPrepared, ClientError> {
        self.prepared.fetch_add(1, Ordering::SeqCst);
        self.upload_open("opaque", "one").await
    }
    async fn upload_open(&self, _: &str, _: &str) -> Result<UploadPrepared, ClientError> {
        Ok(UploadPrepared {
            upload_ref: "opaque".into(),
            remote_id: "remote".into(),
            total_bytes: DecimalU64(6),
            gcid: "A".repeat(40),
            completed: *self.complete.lock().unwrap(),
            part_bytes: 3,
        })
    }
    async fn upload_part(
        &self,
        _: &str,
        part: u32,
        bytes: Vec<u8>,
        _: &str,
    ) -> Result<String, ClientError> {
        let mut out = self.bytes.lock().unwrap();
        let start = (part as usize - 1) * 3;
        let length = out.len().max(start + bytes.len());
        out.resize(length, 0);
        out[start..start + bytes.len()].copy_from_slice(&bytes);
        Ok(format!("{part:032x}"))
    }
    async fn upload_finish(
        &self,
        _: &str,
        etags: &[String],
        _: &str,
    ) -> Result<RemoteEntry, ClientError> {
        assert_eq!(etags.len(), 2);
        *self.complete.lock().unwrap() = true;
        Ok(result())
    }
    async fn upload_forget(&self, _: &str, _: &str) -> Result<(), ClientError> {
        self.forgot.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}
#[tokio::test]
async fn upload_parts_resume_without_new_remote_file_and_forget_secrets_after_completion() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source.bin");
    std::fs::write(&source, b"abcdef").unwrap();
    let db = dir.path().join("tasks.db");
    let p = Arc::new(Upload {
        bytes: Mutex::new(vec![]),
        prepared: AtomicUsize::new(0),
        complete: Mutex::new(false),
        forgot: AtomicUsize::new(0),
    });
    let app = AppService::with_provider(&db, p.clone());
    let task = app
        .upload_start(
            source.to_str().unwrap(),
            "root",
            "one",
            &RequestContext::default(),
        )
        .await
        .unwrap();
    app.run_upload().await.unwrap();
    app.run_upload().await.unwrap();
    assert_eq!(app.task(&task.task_id).unwrap().transferred_bytes.0, 3);
    drop(app);
    let app = AppService::with_provider(&db, p.clone());
    assert_eq!(app.task(&task.task_id).unwrap().state, TaskState::Paused);
    app.transfer_control(&task.task_id, TransferAction::Resume)
        .unwrap();
    for _ in 0..5 {
        app.run_upload().await.unwrap();
    }
    assert_eq!(app.task(&task.task_id).unwrap().state, TaskState::Completed);
    assert_eq!(p.prepared.load(Ordering::SeqCst), 1);
    assert_eq!(*p.bytes.lock().unwrap(), b"abcdef");
    assert_eq!(p.forgot.load(Ordering::SeqCst), 1);
}
