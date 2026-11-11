//! Application context shared by every controller on the UI thread.
use crate::{
    files::FilesState, generated::icons, host::Host, i18n, nav::Nav, overlay::Overlays,
    prefs::Prefs, AppWindow, Icons,
};
use nekopak_core::{updates::UpdateService, AppService};
use nekopak_domain::*;
use nekopak_provider::RequestContext;
use slint::ComponentHandle;
use std::{cell::RefCell, future::Future, rc::Rc, sync::Arc, time::Duration};

pub type Ctx = Rc<App>;

/// Session-wide facts read from the core. Replaced wholesale on re-sync.
#[derive(Default)]
pub struct Core {
    pub status: Option<AppStatus>,
    pub caps: Vec<CapabilityReport>,
    pub auth: Option<AuthState>,
    pub auth_error: Option<ClientError>,
    pub quota: Option<QuotaReport>,
    pub settings: serde_json::Value,
    /// Bumped whenever the remote tree may have changed.
    pub epoch: u64,
    /// Bumped whenever the account changed; stale results are dropped.
    pub session: u64,
}

pub struct App {
    pub ui: AppWindow,
    pub svc: Arc<AppService>,
    pub updater: Option<Arc<UpdateService>>,
    pub mock: bool,
    pub demo: Option<Arc<nekopak_provider_mock::MockProvider>>,
    runtime: tokio::runtime::Handle,
    pub core: RefCell<Core>,
    pub prefs: RefCell<Prefs>,
    pub nav: RefCell<Nav>,
    pub files: RefCell<FilesState>,
    pub overlays: RefCell<Overlays>,
    pub undo: RefCell<Option<(String, Vec<FileMutation>)>>,
    pub transfers: RefCell<crate::transfers::TransfersState>,
    pub panel: RefCell<crate::panel::PanelState>,
    /// Identifier of the duplicates scan shown on the page.
    pub scan: RefCell<Option<String>>,
    /// Last clipboard link that was offered, so it is not offered twice.
    pub last_clipboard_link: RefCell<String>,
    pub palette: RefCell<crate::palette::PaletteState>,
    pub viewer: RefCell<crate::preview::ViewerState>,
    pub gallery: RefCell<crate::gallery::GalleryState>,
}

/// Default deadline for one backend request, matching the web host.
pub fn request() -> RequestContext {
    RequestContext::new(Duration::from_secs(45))
}

pub fn new_key() -> String {
    uuid::Uuid::new_v4().to_string()
}

impl App {
    pub fn new(ui: AppWindow, host: Host, runtime: tokio::runtime::Handle) -> Ctx {
        let prefs = Prefs::load(&host.directory);
        Rc::new(Self {
            ui,
            svc: host.service,
            updater: host.updater,
            mock: host.mock,
            demo: host.demo,
            runtime,
            core: RefCell::new(Core {
                settings: serde_json::Value::Null,
                ..Core::default()
            }),
            prefs: RefCell::new(prefs),
            nav: RefCell::new(Nav::default()),
            files: RefCell::new(FilesState::default()),
            overlays: RefCell::new(Overlays::default()),
            undo: RefCell::new(None),
            transfers: RefCell::new(Default::default()),
            panel: RefCell::new(Default::default()),
            scan: RefCell::new(None),
            last_clipboard_link: RefCell::default(),
            palette: RefCell::default(),
            viewer: RefCell::default(),
            gallery: RefCell::default(),
        })
    }

    /// Runs backend work on the Tokio runtime and resumes on the UI thread.
    pub async fn call<T, F, Fut>(&self, work: F) -> T
    where
        T: Send + 'static,
        F: FnOnce(Arc<AppService>) -> Fut + Send + 'static,
        Fut: Future<Output = T> + Send + 'static,
    {
        let service = self.svc.clone();
        match self.runtime.spawn(async move { work(service).await }).await {
            Ok(value) => value,
            Err(error) => std::panic::resume_unwind(error.into_panic()),
        }
    }

    /// Runs blocking work (dialogs, file system) off the UI thread.
    pub async fn blocking<T, F>(&self, work: F) -> T
    where
        T: Send + 'static,
        F: FnOnce() -> T + Send + 'static,
    {
        match self.runtime.spawn_blocking(work).await {
            Ok(value) => value,
            Err(error) => std::panic::resume_unwind(error.into_panic()),
        }
    }

    /// Starts a UI-thread task. Tasks end when the event loop stops.
    pub fn spawn(self: &Rc<Self>, task: impl Future<Output = ()> + 'static) {
        if let Err(error) = slint::spawn_local(task) {
            eprintln!("NekoPak could not schedule UI work: {error}");
        }
    }

    pub fn icon(&self, name: &str) -> slint::Image {
        icons::icon(&self.ui.global::<Icons>(), name)
    }

    pub fn session(&self) -> u64 {
        self.core.borrow().session
    }

    pub fn feature(&self, name: &str) -> bool {
        self.core
            .borrow()
            .status
            .as_ref()
            .and_then(|status| status.features.get(name).copied())
            .unwrap_or(false)
    }

    pub fn signed_in(&self) -> bool {
        self.mock
            || self
                .core
                .borrow()
                .auth
                .as_ref()
                .is_some_and(|auth| auth.authenticated)
    }

    pub fn auth_expired(&self) -> bool {
        self.core
            .borrow()
            .auth
            .as_ref()
            .is_some_and(|auth| auth.state == AuthStatus::Expired)
    }

    fn active_backend<'a>(&self, core: &'a Core) -> Option<&'a CapabilityReport> {
        let wanted = crate::host::backend_id(self.mock);
        core.caps.iter().find(|report| report.backend == wanted)
    }

    pub fn quota_exhausted(&self, dimensions: &[&str]) -> bool {
        self.core.borrow().quota.as_ref().is_some_and(|quota| {
            quota.dimensions.iter().any(|dimension| {
                dimensions.contains(&dimension.dimension.as_str())
                    && dimension.remaining_bytes == Some(DecimalU64(0))
            })
        })
    }

    /// Whether `operation` may be offered right now, with a human reason when not.
    pub fn gate(&self, operation: Operation) -> Result<(), String> {
        let core = self.core.borrow();
        if core.status.is_none() {
            return Err(i18n::tr("正在启动"));
        }
        if !self.mock && !core.auth.as_ref().is_some_and(|auth| auth.authenticated) {
            return Err(
                if core
                    .auth
                    .as_ref()
                    .is_some_and(|auth| auth.state == AuthStatus::Expired)
                {
                    i18n::tr("登录已过期，请重新授权")
                } else {
                    i18n::tr("需要先登录")
                },
            );
        }
        let Some(backend) = self
            .active_backend(&core)
            .filter(|report| report.status == BackendStatus::Ready)
        else {
            return Err(i18n::tr("暂时无法连接 PikPak"));
        };
        if backend.operations.get(&operation) != Some(&true) {
            return Err(i18n::tx("暂不支持{0}", &[&operation_label(operation)]));
        }
        drop(core);
        let exhausted = match operation {
            Operation::CloudTask => self.quota_exhausted(&["cloud_download"]),
            Operation::Download
            | Operation::ClipDownload
            | Operation::QualityDownload
            | Operation::FolderDownload => self.quota_exhausted(&["download", "download_daily"]),
            _ => false,
        };
        if exhausted {
            return Err(i18n::tr("配额已用尽，等待重置后才能继续"));
        }
        Ok(())
    }

    pub fn can(&self, operation: Operation) -> bool {
        self.gate(operation).is_ok()
    }
}

pub fn operation_label(operation: Operation) -> String {
    i18n::tr(match operation {
        Operation::List => "列目录",
        Operation::Stat => "读取元数据",
        Operation::Search => "搜索",
        Operation::Mkdir => "新建文件夹",
        Operation::Rename => "重命名",
        Operation::Move => "移动",
        Operation::Copy => "复制",
        Operation::Trash => "移至回收站",
        Operation::Upload => "上传",
        Operation::Download => "下载",
        Operation::CloudTask => "云下载",
        Operation::StreamUrl => "播放链接",
        Operation::ReadRange => "随机读取",
        Operation::Quota => "配额查询",
        Operation::SegmentDownload => "视频指定片段下载",
        Operation::Restore => "从回收站还原",
        Operation::DeleteForever => "永久删除",
        Operation::MediaProbe => "读取视频信息",
        Operation::ClipDownload => "下载片段",
        Operation::QualityDownload => "按画质下载",
        Operation::FolderDownload => "下载文件夹",
        Operation::Star => "星标",
        Operation::Duplicates => "查找重复",
        Operation::SpaceSummary => "空间分布",
        Operation::Archive => "压缩包浏览与解压",
        Operation::LinkPreview => "链接预览",
        Operation::ExternalPlayer => "外部播放器",
        Operation::Share => "分享",
    })
}

/// User-facing text for a failure. Core messages are reviewed static copy;
/// codes that need guidance get a consistent headline.
pub fn error_text(error: &ClientError) -> String {
    let headline = match error.code {
        ErrorCode::AuthExpired => Some("登录已过期，请重新登录"),
        ErrorCode::QuotaExceeded => Some("配额已用尽，等待重置后才能继续"),
        ErrorCode::Cancelled => Some("操作已取消"),
        _ => None,
    };
    match headline {
        Some(text) if error.message.is_empty() => i18n::tr(text),
        _ => i18n::message(&error.message),
    }
}

pub fn error_title(error: &ClientError) -> String {
    i18n::tr(match error.code {
        ErrorCode::AuthExpired => "登录已过期",
        ErrorCode::AccountRestricted => "账号受限",
        ErrorCode::PermissionDenied => "没有权限",
        ErrorCode::VerificationRequired => "需要安全验证",
        ErrorCode::QuotaExceeded => "配额已用尽",
        ErrorCode::NotFound => "找不到项目",
        ErrorCode::Unsupported => "暂不支持",
        ErrorCode::TransientNetwork => "网络连接失败",
        ErrorCode::DiskFull => "磁盘空间不足",
        ErrorCode::IntegrityFailed => "校验失败",
        ErrorCode::Conflict => "操作冲突",
        ErrorCode::Cancelled => "已取消",
        ErrorCode::Internal => "出现问题",
    })
}
