//! Keeps local transfers alive while a mobile activity is not visible.
//!
//! The policy is platform-neutral and unit-tested with a fake backend. Android
//! supplies a `dataSync` foreground service; without a running service the
//! previous behavior applies unchanged: the core is gated and local transfers
//! are durably paused until the user resumes them.
use nekopak_core::{AppService, LocalTransferActivity};
use nekopak_domain::ClientError;
use std::{
    sync::Mutex,
    time::{Duration, Instant},
};

/// Minimum spacing between progress-only notification refreshes.
const REFRESH: Duration = Duration::from_secs(1);
/// The host refreshes at least this often so the service can bound its wake lock.
const HEARTBEAT: Duration = Duration::from_secs(30);

pub trait Backend: Send + Sync {
    /// Returns false when the platform refuses to start (for example a
    /// background-start restriction). The caller then falls back to pausing.
    fn start(&self, activity: LocalTransferActivity) -> bool;
    fn update(&self, activity: LocalTransferActivity);
    fn stop(&self);
}

/// Why the platform ended the keep-alive without being asked by this module.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ended {
    /// The user chose "pause all" on the ongoing notification.
    PauseRequested,
    /// The platform's time budget for this service type was exhausted.
    TimedOut,
    /// The task was swiped away or the service was destroyed by the system.
    Removed,
}

impl Ended {
    pub fn from_code(code: i32) -> Self {
        match code {
            1 => Self::PauseRequested,
            2 => Self::TimedOut,
            _ => Self::Removed,
        }
    }
}

struct State {
    visible: bool,
    running: bool,
    shown: Option<(LocalTransferActivity, Instant)>,
}

pub struct KeepAlive {
    backend: Box<dyn Backend>,
    state: Mutex<State>,
}

/// The caller must durably pause local transfers when this is true. Gating has
/// already happened synchronously, so no new block can start in between.
#[must_use]
pub struct PauseRequired(pub bool);

impl KeepAlive {
    pub fn new(backend: Box<dyn Backend>) -> Self {
        Self {
            backend,
            state: Mutex::new(State {
                visible: true,
                running: false,
                shown: None,
            }),
        }
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        // The state holds plain flags; a panic elsewhere must not disable pausing.
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn wanted(service: &AppService) -> Result<Option<LocalTransferActivity>, ClientError> {
        let activity = service.local_transfer_activity()?;
        Ok((activity.active > 0 && service.background_transfers_enabled()).then_some(activity))
    }

    /// Periodic and event-driven reconciliation of the service with the queue.
    pub fn reconcile(&self, service: &AppService) -> Result<PauseRequired, ClientError> {
        let mut state = self.state();
        match Self::wanted(service)? {
            Some(activity) if state.running => {
                let now = Instant::now();
                let due = match state.shown {
                    Some((shown, at)) => {
                        shown.active != activity.active
                            || (shown != activity && now.duration_since(at) >= REFRESH)
                            || now.duration_since(at) >= HEARTBEAT
                    }
                    None => true,
                };
                if due {
                    self.backend.update(activity);
                    state.shown = Some((activity, now));
                }
            }
            // A service may only be started while the activity is visible.
            Some(activity) if state.visible => {
                if self.backend.start(activity) {
                    state.running = true;
                    state.shown = Some((activity, Instant::now()));
                }
            }
            Some(_) => {}
            None if state.running => {
                self.backend.stop();
                state.running = false;
                state.shown = None;
                if !state.visible {
                    // Nothing keeps the process alive any more: restore the
                    // ordinary background gate. This also covers the option
                    // being switched off while transfers are active.
                    service.set_foreground(false);
                    return Ok(PauseRequired(true));
                }
            }
            None => {}
        }
        Ok(PauseRequired(false))
    }

    /// The activity is leaving the screen. Called synchronously by the host.
    pub fn suspended(&self, service: &AppService) -> PauseRequired {
        let mut state = self.state();
        state.visible = false;
        if !state.running {
            // Last permitted moment to start: the activity is still stopping.
            if let Ok(Some(activity)) = Self::wanted(service) {
                if self.backend.start(activity) {
                    state.running = true;
                    state.shown = Some((activity, Instant::now()));
                }
            }
        }
        if state.running {
            return PauseRequired(false);
        }
        service.set_foreground(false);
        PauseRequired(true)
    }

    pub fn resumed(&self, service: &AppService) {
        self.state().visible = true;
        service.set_foreground(true);
    }

    /// The platform ended the service on its own.
    pub fn ended(&self, service: &AppService, reason: Ended) -> PauseRequired {
        let mut state = self.state();
        state.running = false;
        state.shown = None;
        if !state.visible {
            service.set_foreground(false);
        }
        // While visible, a timeout or removal needs no pause: the next
        // reconciliation simply starts a fresh service.
        PauseRequired(reason == Ended::PauseRequested || !state.visible)
    }

    #[cfg(test)]
    fn running(&self) -> bool {
        self.state().running
    }
}

#[cfg(target_os = "android")]
pub mod android {
    use super::*;
    use jni::{
        objects::{JClass, JObject, JValue},
        JNIEnv, JavaVM,
    };
    use std::sync::OnceLock;

    static ENDED: OnceLock<Box<dyn Fn(Ended) + Send + Sync>> = OnceLock::new();

    /// Registers the single handler for service-initiated stops.
    pub fn on_ended(handler: impl Fn(Ended) + Send + Sync + 'static) {
        let _ = ENDED.set(Box::new(handler));
    }

    /// Called by `TransferService` on the Android main thread.
    #[no_mangle]
    pub extern "system" fn Java_io_nekopak_desktop_TransferService_nativeEnded(
        _env: JNIEnv,
        _class: JClass,
        reason: jni::sys::jint,
    ) {
        // Never unwind into the JVM.
        let _ = std::panic::catch_unwind(|| {
            if let Some(handler) = ENDED.get() {
                handler(Ended::from_code(reason));
            }
        });
    }

    pub struct Service;

    impl Service {
        fn call(method: &str, activity: Option<LocalTransferActivity>) -> Option<bool> {
            let context = ndk_context::android_context();
            // SAFETY: the Android runtime owns this VM throughout the native app lifetime.
            let vm = unsafe { JavaVM::from_raw(context.vm().cast()) }.ok()?;
            let mut env = vm.attach_current_thread().ok()?;
            let result = env.with_local_frame(16, |env| -> jni::errors::Result<bool> {
                // Android owns this global Context. The local copy is scoped to this JNI frame.
                let raw = unsafe { JObject::from_raw(context.context().cast()) };
                let context = env.new_local_ref(&raw)?;
                // find_class on a Rust worker uses the boot classloader.
                let loader = env
                    .call_method(&context, "getClassLoader", "()Ljava/lang/ClassLoader;", &[])?
                    .l()?;
                let name = env.new_string("io.nekopak.desktop.TransferService")?;
                let class = JClass::from(
                    env.call_method(
                        loader,
                        "loadClass",
                        "(Ljava/lang/String;)Ljava/lang/Class;",
                        &[JValue::Object(&name)],
                    )?
                    .l()?,
                );
                match activity {
                    Some(activity) => env
                        .call_static_method(
                            class,
                            method,
                            "(Landroid/content/Context;II)Z",
                            &[
                                JValue::Object(&context),
                                JValue::Int(activity.active.min(i32::MAX as u32) as i32),
                                JValue::Int(activity.percent.map_or(-1, i32::from)),
                            ],
                        )?
                        .z(),
                    None => env
                        .call_static_method(
                            class,
                            method,
                            "(Landroid/content/Context;)Z",
                            &[JValue::Object(&context)],
                        )?
                        .z(),
                }
            });
            if env.exception_check().unwrap_or(true) {
                let _ = env.exception_clear();
                return None;
            }
            result.ok()
        }
    }

    impl Backend for Service {
        fn start(&self, activity: LocalTransferActivity) -> bool {
            Self::call("start", Some(activity)) == Some(true)
        }
        fn update(&self, activity: LocalTransferActivity) {
            let _ = Self::call("update", Some(activity));
        }
        fn stop(&self) {
            let _ = Self::call("stop", None);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nekopak_domain::{TaskState, TransferAction};
    use nekopak_provider::RequestContext;
    use nekopak_provider_mock::Scenario;
    use std::sync::{
        atomic::{AtomicBool, AtomicU32, Ordering},
        Arc,
    };

    #[derive(Default)]
    struct Fake {
        refuse: AtomicBool,
        starts: AtomicU32,
        updates: AtomicU32,
        stops: AtomicU32,
    }
    impl Backend for Arc<Fake> {
        fn start(&self, _: LocalTransferActivity) -> bool {
            self.starts.fetch_add(1, Ordering::SeqCst);
            !self.refuse.load(Ordering::SeqCst)
        }
        fn update(&self, _: LocalTransferActivity) {
            self.updates.fetch_add(1, Ordering::SeqCst);
        }
        fn stop(&self) {
            self.stops.fetch_add(1, Ordering::SeqCst);
        }
    }

    struct Fixture {
        _dir: tempfile::TempDir,
        app: AppService,
        fake: Arc<Fake>,
        keep: KeepAlive,
    }

    fn fixture() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let app = AppService::mock(&dir.path().join("tasks.db"), Scenario::Free);
        let fake = Arc::new(Fake::default());
        let keep = KeepAlive::new(Box::new(fake.clone()));
        Fixture {
            _dir: dir,
            app,
            fake,
            keep,
        }
    }

    async fn download(app: &AppService) -> String {
        download_file(app, "file-1").await
    }

    async fn download_file(app: &AppService, file: &str) -> String {
        app.transfer_start(
            file,
            "C:/NekoPak-Mock/downloads",
            Some(file),
            &RequestContext::default(),
        )
        .await
        .unwrap()
        .task_id
    }

    /// Mirrors the host: a required pause is applied right after gating.
    fn apply(app: &AppService, pause: PauseRequired) -> bool {
        if pause.0 {
            app.pause_local_transfers().unwrap();
        }
        pause.0
    }

    #[test]
    fn platform_codes_map_to_reasons_and_unknown_codes_are_conservative() {
        assert_eq!(Ended::from_code(1), Ended::PauseRequested);
        assert_eq!(Ended::from_code(2), Ended::TimedOut);
        assert_eq!(Ended::from_code(3), Ended::Removed);
        assert_eq!(Ended::from_code(-7), Ended::Removed);
    }

    #[tokio::test]
    async fn running_service_keeps_bytes_moving_while_hidden_then_gates_when_idle() {
        let f = fixture();
        assert!(!apply(&f.app, f.keep.reconcile(&f.app).unwrap()));
        assert_eq!(f.fake.starts.load(Ordering::SeqCst), 0);
        let id = download(&f.app).await;
        assert!(!apply(&f.app, f.keep.reconcile(&f.app).unwrap()));
        assert!(f.keep.running());
        for _ in 0..3 {
            f.app.tick().await.unwrap();
        }
        let before = f.app.task(&id).unwrap().transferred_bytes.0;

        assert!(!apply(&f.app, f.keep.suspended(&f.app)));
        assert!(f.app.is_foreground());
        f.app.tick().await.unwrap();
        assert!(f.app.task(&id).unwrap().transferred_bytes.0 > before);
        for _ in 0..16 {
            f.app.tick().await.unwrap();
        }
        assert_eq!(f.app.task(&id).unwrap().state, TaskState::Completed);

        // Finished while hidden: the service stops and the core is gated again.
        assert!(apply(&f.app, f.keep.reconcile(&f.app).unwrap()));
        assert!(!f.keep.running());
        assert!(!f.app.is_foreground());
        assert_eq!(f.fake.stops.load(Ordering::SeqCst), 1);
        // A hidden activity may not start another service.
        let queued = download_file(&f.app, "file-2").await;
        assert_ne!(queued, id);
        assert!(!apply(&f.app, f.keep.reconcile(&f.app).unwrap()));
        assert_eq!(f.fake.starts.load(Ordering::SeqCst), 1);
        f.app.tick().await.unwrap();
        assert_eq!(f.app.task(&queued).unwrap().transferred_bytes.0, 0);

        f.keep.resumed(&f.app);
        assert!(f.app.is_foreground());
    }

    #[tokio::test]
    async fn refused_or_disabled_service_falls_back_to_durable_pause() {
        let f = fixture();
        f.fake.refuse.store(true, Ordering::SeqCst);
        let id = download(&f.app).await;
        assert!(!apply(&f.app, f.keep.reconcile(&f.app).unwrap()));
        assert!(!f.keep.running());
        assert!(apply(&f.app, f.keep.suspended(&f.app)));
        assert!(!f.app.is_foreground());
        assert_eq!(f.app.task(&id).unwrap().state, TaskState::Paused);

        let f = fixture();
        f.app
            .settings_update(&serde_json::json!({"background_transfers":false}))
            .unwrap();
        let id = download(&f.app).await;
        assert!(!apply(&f.app, f.keep.reconcile(&f.app).unwrap()));
        assert!(apply(&f.app, f.keep.suspended(&f.app)));
        assert_eq!(f.fake.starts.load(Ordering::SeqCst), 0);
        assert_eq!(f.app.task(&id).unwrap().state, TaskState::Paused);
    }

    #[tokio::test]
    async fn suspension_starts_a_service_for_a_task_created_since_the_last_reconcile() {
        let f = fixture();
        let id = download(&f.app).await;
        assert!(!apply(&f.app, f.keep.suspended(&f.app)));
        assert!(f.keep.running() && f.app.is_foreground());
        for _ in 0..3 {
            f.app.tick().await.unwrap();
        }
        assert!(f.app.task(&id).unwrap().transferred_bytes.0 > 0);
    }

    #[tokio::test]
    async fn platform_stop_pauses_when_hidden_and_restarts_when_visible() {
        let f = fixture();
        let id = download(&f.app).await;
        let _ = f.keep.reconcile(&f.app).unwrap();
        // Visible timeout: nothing is paused and a new service starts.
        assert!(!apply(&f.app, f.keep.ended(&f.app, Ended::TimedOut)));
        assert!(f.app.is_foreground());
        let _ = f.keep.reconcile(&f.app).unwrap();
        assert_eq!(f.fake.starts.load(Ordering::SeqCst), 2);

        let _ = f.keep.suspended(&f.app);
        assert!(apply(&f.app, f.keep.ended(&f.app, Ended::TimedOut)));
        assert!(!f.app.is_foreground() && !f.keep.running());
        assert_eq!(f.app.task(&id).unwrap().state, TaskState::Paused);
        // The paused task needs an explicit resume and no service is restarted.
        f.keep.resumed(&f.app);
        assert!(!apply(&f.app, f.keep.reconcile(&f.app).unwrap()));
        assert_eq!(f.fake.starts.load(Ordering::SeqCst), 2);
        f.app.transfer_control(&id, TransferAction::Resume).unwrap();
        let _ = f.keep.reconcile(&f.app).unwrap();
        assert_eq!(f.fake.starts.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn notification_pause_pauses_even_while_visible() {
        let f = fixture();
        let id = download(&f.app).await;
        let _ = f.keep.reconcile(&f.app).unwrap();
        assert!(apply(&f.app, f.keep.ended(&f.app, Ended::PauseRequested)));
        assert!(f.app.is_foreground());
        assert_eq!(f.app.task(&id).unwrap().state, TaskState::Paused);
        assert!(!apply(&f.app, f.keep.reconcile(&f.app).unwrap()));
        assert_eq!(f.fake.starts.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn disabling_the_option_while_hidden_stops_and_pauses() {
        let f = fixture();
        let id = download(&f.app).await;
        let _ = f.keep.reconcile(&f.app).unwrap();
        let _ = f.keep.suspended(&f.app);
        f.app
            .settings_update(&serde_json::json!({"background_transfers":false}))
            .unwrap();
        assert!(apply(&f.app, f.keep.reconcile(&f.app).unwrap()));
        assert_eq!(f.app.task(&id).unwrap().state, TaskState::Paused);
        assert_eq!(f.fake.stops.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn progress_refreshes_are_rate_limited() {
        let f = fixture();
        download(&f.app).await;
        for _ in 0..5 {
            let _ = f.keep.reconcile(&f.app).unwrap();
            f.app.tick().await.unwrap();
        }
        assert_eq!(f.fake.starts.load(Ordering::SeqCst), 1);
        assert_eq!(f.fake.updates.load(Ordering::SeqCst), 0);
        // A changed task count is shown immediately.
        download_file(&f.app, "file-2").await;
        let _ = f.keep.reconcile(&f.app).unwrap();
        assert_eq!(f.fake.updates.load(Ordering::SeqCst), 1);
    }
}
