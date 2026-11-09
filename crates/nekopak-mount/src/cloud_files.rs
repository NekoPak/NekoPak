//! Native CfAPI adapter, independently implemented from the Microsoft API contract.
use crate::{
    model::{error, invalid},
    windows_util::*,
    *,
};
use nekopak_domain::ErrorCode;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    ffi::c_void,
    fs::{File, OpenOptions},
    io::{Read, Write},
    mem::{offset_of, size_of},
    os::windows::io::AsRawHandle,
    path::{Path, PathBuf},
    sync::Arc,
};
use windows::{
    core::{GUID, HRESULT},
    Win32::{
        Foundation::{HANDLE, NTSTATUS},
        Storage::{CloudFilters::*, FileSystem::*},
    },
};

const MARKER: &str = ".nekopak-cloud-root.json";
const PROVIDER: GUID = GUID::from_u128(0x1cb65287_ba32_47a2_a09c_0e4f85ba8324);
const FAILURE: NTSTATUS = NTSTATUS(0xC000CF12u32 as i32); // STATUS_CLOUD_FILE_UNSUCCESSFUL
const DENIED: NTSTATUS = NTSTATUS(0xC000CF18u32 as i32); // STATUS_CLOUD_FILE_ACCESS_DENIED
#[derive(Serialize, Deserialize)]
struct RootRecord {
    schema: u32,
    snapshot: String,
    volume: u32,
    file_id: u64,
    #[serde(default)]
    registration_identity: Option<String>,
    #[serde(default)]
    entries: Vec<MountEntry>,
}
struct Context {
    session: Arc<ReadSession>,
    identities: BTreeMap<Vec<u8>, MountEntry>,
}

pub struct CloudFilesMount {
    root: PathBuf,
    connection: Option<CF_CONNECTION_KEY>,
    context: Option<Box<Context>>,
    // Held until disconnect; the directories cannot be renamed/replaced meanwhile.
    _locks: Vec<File>,
}
// CF connection keys are opaque integer tokens; Context contains no native pointer.
unsafe impl Send for CloudFilesMount {}
impl CloudFilesMount {
    /// Explicit start only. A new root must not exist. Existing roots must have
    /// our matching marker and OS registration identity. Refresh may add entries;
    /// changed/deleted entries are preserved and rejected until explicitly resolved.
    pub fn start(
        root: &Path,
        snapshot: Arc<MountSnapshot>,
        source: Arc<dyn ReadSource>,
    ) -> Result<Self> {
        validate_relative_path(
            root.file_name()
                .and_then(|n| n.to_str())
                .ok_or_else(invalid)?,
        )?;
        let session = ReadSession::verified(snapshot, source)?;
        let mut locks = lock_ancestors(root.parent().ok_or_else(invalid)?)?;
        for parent in &locks {
            if root_identity(parent)?.is_some() {
                return Err(error(ErrorCode::Conflict, "同步根不能嵌套在已有同步根内。"));
            }
        }
        let created = match std::fs::create_dir(root) {
            Ok(()) => true,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => false,
            Err(e) => return Err(io(e)),
        };
        let root_file = open_metadata(root, true)?;
        reject_foreign_reparse(&root_file)?;
        let (volume, id) = file_id(&root_file)?;
        let mut record = RootRecord {
            schema: 1,
            snapshot: session.snapshot.fingerprint().into(),
            volume,
            file_id: id,
            registration_identity: None,
            entries: session.snapshot.entries().cloned().collect(),
        };
        let marker = root.join(MARKER);
        if created {
            let mut f = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&marker)
                .map_err(io)?;
            f.write_all(&serde_json::to_vec(&record).map_err(|_| invalid())?)
                .map_err(io)?;
            f.sync_all().map_err(io)?;
        } else {
            // Never register/update an arbitrary existing directory, even if empty.
            let mut f = open_metadata(&marker, false)?;
            if is_reparse(&f)? || f.metadata().map_err(io)?.len() > 32 * 1024 * 1024 {
                return Err(invalid());
            }
            let mut bytes = Vec::new();
            f.read_to_end(&mut bytes).map_err(io)?;
            let prior: RootRecord = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
            if prior.schema != 1 || prior.volume != volume || prior.file_id != id {
                return Err(error(
                    ErrorCode::Conflict,
                    "该目录不是可恢复的 NekoPak 同步根。",
                ));
            }
            validate_extension(&prior, &session.snapshot)?;
            record.registration_identity = Some(
                prior
                    .registration_identity
                    .unwrap_or_else(|| format!("NekoPak:1:{}", prior.snapshot)),
            );
        }
        let root_w = wide(root);
        let name = wide("NekoPak");
        let version = wide(env!("CARGO_PKG_VERSION"));
        let identity = record
            .registration_identity
            .clone()
            .unwrap_or_else(|| format!("NekoPak:1:{}", session.snapshot.fingerprint()))
            .into_bytes();
        let registered = root_identity(&root_file)?;
        if let Some(registered) = registered {
            if registered != identity {
                return Err(error(
                    ErrorCode::Conflict,
                    "该目录已由另一个同步提供程序管理。",
                ));
            }
        } else {
            // A stale marker after a failed first start is safe only when no other
            // files have appeared. No UPDATE flag is ever used for registration.
            if std::fs::read_dir(root)
                .map_err(io)?
                .any(|e| e.map(|e| e.file_name() != MARKER).unwrap_or(true))
            {
                return Err(invalid());
            }
            let registration = CF_SYNC_REGISTRATION {
                StructSize: size_of::<CF_SYNC_REGISTRATION>() as u32,
                ProviderName: pw(&name),
                ProviderVersion: pw(&version),
                ProviderId: PROVIDER,
                SyncRootIdentity: identity.as_ptr().cast(),
                SyncRootIdentityLength: identity.len() as u32,
                ..Default::default()
            };
            let policies = CF_SYNC_POLICIES {
                StructSize: size_of::<CF_SYNC_POLICIES>() as u32,
                Hydration: CF_HYDRATION_POLICY {
                    Primary: CF_HYDRATION_POLICY_PARTIAL,
                    Modifier: CF_HYDRATION_POLICY_MODIFIER_NONE,
                },
                Population: CF_POPULATION_POLICY {
                    Primary: CF_POPULATION_POLICY_ALWAYS_FULL,
                    Modifier: CF_POPULATION_POLICY_MODIFIER_NONE,
                },
                InSync: CF_INSYNC_POLICY_TRACK_ALL,
                HardLink: CF_HARDLINK_POLICY_NONE,
                ..Default::default()
            };
            unsafe {
                CfRegisterSyncRoot(pw(&root_w), &registration, &policies, CF_REGISTER_FLAG_NONE)
            }
            .map_err(platform_error)?;
        }
        locks.push(root_file);
        let identities = session
            .snapshot
            .entries()
            .map(|e| (entry_identity(e), e.clone()))
            .collect();
        let context = Box::new(Context {
            session,
            identities,
        });
        let callbacks = [
            CF_CALLBACK_REGISTRATION {
                Type: CF_CALLBACK_TYPE_FETCH_DATA,
                Callback: Some(fetch),
            },
            CF_CALLBACK_REGISTRATION {
                Type: CF_CALLBACK_TYPE_CANCEL_FETCH_DATA,
                Callback: Some(cancel),
            },
            CF_CALLBACK_REGISTRATION {
                Type: CF_CALLBACK_TYPE_NOTIFY_DELETE,
                Callback: Some(deny_delete),
            },
            CF_CALLBACK_REGISTRATION {
                Type: CF_CALLBACK_TYPE_NOTIFY_RENAME,
                Callback: Some(deny_rename),
            },
            CF_CALLBACK_REGISTRATION {
                Type: CF_CALLBACK_TYPE_NONE,
                Callback: None,
            },
        ];
        let connection = unsafe {
            CfConnectSyncRoot(
                pw(&root_w),
                callbacks.as_ptr(),
                Some((&*context as *const Context).cast()),
                CF_CONNECT_FLAG_BLOCK_SELF_IMPLICIT_HYDRATION,
            )
        }
        .map_err(platform_error)?;
        let mut mounted = Self {
            root: root.to_owned(),
            connection: Some(connection),
            context: Some(context),
            _locks: locks,
        };
        // If population fails, Drop disconnects but preserves the registered root
        // and any completed placeholders for an explicit restart, without deletion.
        mounted.populate()?;
        // Only after every placeholder is verified/created do we publish the new
        // inventory. The prior marker survives any interrupted write/rename.
        write_record(&marker, &record)?;
        Ok(mounted)
    }
    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn is_connected(&self) -> bool {
        self.connection.is_some()
    }
    pub fn stop(&mut self) -> Result<()> {
        if let Some(context) = &self.context {
            context.session.stop();
        }
        if let Some(connection) = self.connection {
            unsafe { CfDisconnectSyncRoot(connection) }.map_err(platform_error)?;
            self.connection = None;
        }
        self.context = None;
        self._locks.clear();
        Ok(())
    }
    fn populate(&mut self) -> Result<()> {
        let context = self.context.as_ref().ok_or_else(invalid)?;
        let mut entries: Vec<_> = context.session.snapshot.entries().collect();
        entries.sort_by_key(|e| (e.path.matches('/').count(), !e.directory, e.path.clone()));
        for entry in entries {
            let path = self.root.join(entry.path.replace('/', "\\"));
            let identity = entry_identity(entry);
            if path.try_exists().map_err(io)? {
                let f = open_metadata(&path, entry.directory)?;
                reject_foreign_reparse(&f)?;
                validate_placeholder(&f, &identity)?;
                if !entry.directory && f.metadata().map_err(io)?.len() != entry.size {
                    return Err(invalid());
                }
                if entry.directory {
                    self._locks.push(f);
                }
                continue;
            }
            let parent = wide(path.parent().ok_or_else(invalid)?);
            let name = wide(path.file_name().ok_or_else(invalid)?);
            let attributes = if entry.directory {
                FILE_ATTRIBUTE_DIRECTORY.0
            } else {
                FILE_ATTRIBUTE_READONLY.0
            };
            let mut placeholders = [CF_PLACEHOLDER_CREATE_INFO {
                RelativeFileName: pw(&name),
                FsMetadata: CF_FS_METADATA {
                    BasicInfo: FILE_BASIC_INFO {
                        FileAttributes: attributes,
                        LastWriteTime: entry.modified_filetime as i64,
                        ChangeTime: entry.modified_filetime as i64,
                        ..Default::default()
                    },
                    FileSize: entry.size as i64,
                },
                FileIdentity: identity.as_ptr().cast(),
                FileIdentityLength: identity.len() as u32,
                Flags: CF_PLACEHOLDER_CREATE_FLAG_MARK_IN_SYNC
                    | CF_PLACEHOLDER_CREATE_FLAG_DISABLE_ON_DEMAND_POPULATION,
                ..Default::default()
            }];
            let mut processed = 0;
            unsafe {
                CfCreatePlaceholders(
                    pw(&parent),
                    &mut placeholders,
                    CF_CREATE_FLAG_STOP_ON_ERROR,
                    Some(&mut processed),
                )
            }
            .map_err(platform_error)?;
            placeholders[0].Result.ok().map_err(platform_error)?;
            if processed != 1 {
                return Err(invalid());
            }
            if entry.directory {
                let f = open_metadata(&path, true)?;
                validate_placeholder(&f, &identity)?;
                self._locks.push(f);
            }
        }
        Ok(())
    }
}
fn validate_extension(prior: &RootRecord, next: &MountSnapshot) -> Result<()> {
    if prior.snapshot == next.fingerprint() {
        return Ok(());
    }
    if prior.entries.is_empty() {
        return Err(error(
            ErrorCode::Conflict,
            "旧同步根缺少可校验目录清单，请保留原目录并选择新的专用目录。",
        ));
    }
    let old = MountSnapshot::new(prior.entries.clone())?;
    if old.fingerprint() != prior.snapshot {
        return Err(invalid());
    }
    if old
        .entries()
        .any(|entry| next.get(&entry.path) != Some(entry))
    {
        return Err(error(
            ErrorCode::Conflict,
            "远端存在修改、移动或删除；已保留本地占位文件，请选择新的同步根以免覆盖内容。",
        ));
    }
    Ok(())
}
fn write_record(marker: &Path, record: &RootRecord) -> Result<()> {
    let temporary = marker.with_file_name(format!(".nekopak-{}.tmp", uuid::Uuid::new_v4()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(io)?;
    file.write_all(&serde_json::to_vec(record).map_err(|_| invalid())?)
        .map_err(io)?;
    file.sync_all().map_err(io)?;
    drop(file);
    let result = unsafe {
        MoveFileExW(
            pw(&wide(&temporary)),
            pw(&wide(marker)),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    }
    .map_err(platform_error);
    if result.is_ok() {
        unsafe { SetFileAttributesW(pw(&wide(marker)), FILE_ATTRIBUTE_HIDDEN) }
            .map_err(platform_error)?;
    }
    result
}
fn reject_foreign_reparse(file: &File) -> Result<()> {
    let mut tag = FILE_ATTRIBUTE_TAG_INFO::default();
    unsafe {
        GetFileInformationByHandleEx(
            HANDLE(file.as_raw_handle()),
            FileAttributeTagInfo,
            (&mut tag as *mut FILE_ATTRIBUTE_TAG_INFO).cast(),
            size_of::<FILE_ATTRIBUTE_TAG_INFO>() as u32,
        )
    }
    .map_err(platform_error)?;
    // CF tags have the CLOUD_MASK bits; none are name-surrogate reparses.
    if tag.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0
        && tag.ReparseTag & !0xF000 != 0x9000001A
    {
        return Err(invalid());
    }
    Ok(())
}
impl Drop for CloudFilesMount {
    fn drop(&mut self) {
        if self.stop().is_err() {
            // Failure cannot allow Windows to dereference a freed callback context.
            // The process owns the remaining connection; retain context until exit.
            if let Some(context) = self.context.take() {
                Box::leak(context);
            }
        }
    }
}
fn entry_identity(entry: &MountEntry) -> Vec<u8> {
    let mut hash = Sha256::new();
    for text in [&entry.path, &entry.remote_id, &entry.identity] {
        hash.update(text.len().to_le_bytes());
        hash.update(text.as_bytes());
    }
    hash.update(entry.size.to_le_bytes());
    hash.finalize().to_vec()
}
fn root_identity(file: &File) -> Result<Option<Vec<u8>>> {
    let mut buffer = vec![0u64; 1024];
    let mut written = 0;
    let result = unsafe {
        CfGetSyncRootInfoByHandle(
            HANDLE(file.as_raw_handle()),
            CF_SYNC_ROOT_INFO_STANDARD,
            buffer.as_mut_ptr().cast(),
            (buffer.len() * 8) as u32,
            Some(&mut written),
        )
    };
    if let Err(e) = result {
        // Only ERROR_NOT_A_CLOUD_FILE means no root; access/driver errors must fail.
        if e.code() == HRESULT::from_win32(376) || e.code() == HRESULT::from_win32(390) {
            return Ok(None);
        }
        return Err(platform_error(e));
    }
    let info = unsafe { &*buffer.as_ptr().cast::<CF_SYNC_ROOT_STANDARD_INFO>() };
    let offset = offset_of!(CF_SYNC_ROOT_STANDARD_INFO, SyncRootIdentity);
    let n = info.SyncRootIdentityLength as usize;
    if n > 4096 || offset + n > written as usize {
        return Err(invalid());
    }
    Ok(Some(
        unsafe { std::slice::from_raw_parts(buffer.as_ptr().cast::<u8>().add(offset), n) }.to_vec(),
    ))
}
fn validate_placeholder(file: &File, expected: &[u8]) -> Result<()> {
    let mut buffer = vec![0u64; 1024];
    let mut written = 0;
    unsafe {
        CfGetPlaceholderInfo(
            HANDLE(file.as_raw_handle()),
            CF_PLACEHOLDER_INFO_BASIC,
            buffer.as_mut_ptr().cast(),
            (buffer.len() * 8) as u32,
            Some(&mut written),
        )
    }
    .map_err(platform_error)?;
    let info = unsafe { &*buffer.as_ptr().cast::<CF_PLACEHOLDER_BASIC_INFO>() };
    let offset = offset_of!(CF_PLACEHOLDER_BASIC_INFO, FileIdentity);
    let n = info.FileIdentityLength as usize;
    if info.InSyncState != CF_IN_SYNC_STATE_IN_SYNC
        || n != expected.len()
        || offset + n > written as usize
    {
        return Err(invalid());
    }
    if unsafe { std::slice::from_raw_parts(buffer.as_ptr().cast::<u8>().add(offset), n) }
        != expected
    {
        return Err(invalid());
    }
    Ok(())
}
unsafe extern "system" fn fetch(
    info: *const CF_CALLBACK_INFO,
    parameters: *const CF_CALLBACK_PARAMETERS,
) {
    if info.is_null() || parameters.is_null() {
        return;
    }
    let info = &*info;
    let params = (*parameters).Anonymous.FetchData;
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<()> {
        if info.CallbackContext.is_null()
            || info.FileIdentity.is_null()
            || info.FileIdentityLength != 32
            || params.RequiredFileOffset < 0
            || params.RequiredLength <= 0
        {
            return Err(invalid());
        }
        let ctx = &*info.CallbackContext.cast::<Context>();
        let identity = std::slice::from_raw_parts(
            info.FileIdentity.cast::<u8>(),
            info.FileIdentityLength as usize,
        );
        let entry = ctx.identities.get(identity).ok_or_else(invalid)?;
        if info.FileSize < 0 || info.FileSize as u64 != entry.size {
            return Err(invalid());
        }
        let range = hydration_range(
            params.RequiredFileOffset as u64,
            params.RequiredLength as u64,
            entry.size,
        )?;
        let request = ctx.session.begin(info.RequestKey as u64)?;
        let mut at = range.start;
        while at < range.end {
            let take = (range.end - at).min(1024 * 1024) as usize;
            let bytes = request.read(entry, at, take)?;
            transfer(info, at, take as u64, Some(&bytes), NTSTATUS(0))?;
            at += take as u64;
        }
        Ok(())
    }));
    if !matches!(result, Ok(Ok(())))
        && params.RequiredFileOffset >= 0
        && params.RequiredLength > 0
        && info.FileSize > 0
    {
        if let Ok(range) = hydration_range(
            params.RequiredFileOffset as u64,
            params.RequiredLength as u64,
            info.FileSize as u64,
        ) {
            let _ = transfer(info, range.start, range.end - range.start, None, FAILURE);
        }
    }
}
unsafe extern "system" fn cancel(info: *const CF_CALLBACK_INFO, _: *const CF_CALLBACK_PARAMETERS) {
    if let Some(info) = info.as_ref() {
        if !info.CallbackContext.is_null() {
            (&*info.CallbackContext.cast::<Context>())
                .session
                .cancel(info.RequestKey as u64);
        }
    }
}
fn operation(info: &CF_CALLBACK_INFO, kind: CF_OPERATION_TYPE) -> CF_OPERATION_INFO {
    CF_OPERATION_INFO {
        StructSize: size_of::<CF_OPERATION_INFO>() as u32,
        Type: kind,
        ConnectionKey: info.ConnectionKey,
        TransferKey: info.TransferKey,
        RequestKey: info.RequestKey,
        ..Default::default()
    }
}
fn transfer(
    info: &CF_CALLBACK_INFO,
    offset: u64,
    length: u64,
    bytes: Option<&[u8]>,
    status: NTSTATUS,
) -> Result<()> {
    let operation = operation(info, CF_OPERATION_TYPE_TRANSFER_DATA);
    let mut params = CF_OPERATION_PARAMETERS {
        ParamSize: (offset_of!(CF_OPERATION_PARAMETERS, Anonymous)
            + size_of::<CF_OPERATION_PARAMETERS_0_0>()) as u32,
        Anonymous: CF_OPERATION_PARAMETERS_0 {
            TransferData: CF_OPERATION_PARAMETERS_0_0 {
                Flags: CF_OPERATION_TRANSFER_DATA_FLAG_NONE,
                CompletionStatus: status,
                Buffer: bytes.map_or(std::ptr::null(), |b| b.as_ptr().cast::<c_void>()),
                Offset: offset as i64,
                Length: length as i64,
            },
        },
    };
    unsafe { CfExecute(&operation, &mut params) }.map_err(platform_error)
}
unsafe extern "system" fn deny_delete(
    info: *const CF_CALLBACK_INFO,
    _: *const CF_CALLBACK_PARAMETERS,
) {
    if let Some(info) = info.as_ref() {
        let mut params = CF_OPERATION_PARAMETERS {
            ParamSize: (offset_of!(CF_OPERATION_PARAMETERS, Anonymous)
                + size_of::<CF_OPERATION_PARAMETERS_0_7>()) as u32,
            Anonymous: CF_OPERATION_PARAMETERS_0 {
                AckDelete: CF_OPERATION_PARAMETERS_0_7 {
                    Flags: CF_OPERATION_ACK_DELETE_FLAG_NONE,
                    CompletionStatus: DENIED,
                },
            },
        };
        let _ = CfExecute(&operation(info, CF_OPERATION_TYPE_ACK_DELETE), &mut params);
    }
}
unsafe extern "system" fn deny_rename(
    info: *const CF_CALLBACK_INFO,
    _: *const CF_CALLBACK_PARAMETERS,
) {
    if let Some(info) = info.as_ref() {
        let mut params = CF_OPERATION_PARAMETERS {
            ParamSize: (offset_of!(CF_OPERATION_PARAMETERS, Anonymous)
                + size_of::<CF_OPERATION_PARAMETERS_0_6>()) as u32,
            Anonymous: CF_OPERATION_PARAMETERS_0 {
                AckRename: CF_OPERATION_PARAMETERS_0_6 {
                    Flags: CF_OPERATION_ACK_RENAME_FLAG_NONE,
                    CompletionStatus: DENIED,
                },
            },
        };
        let _ = CfExecute(&operation(info, CF_OPERATION_TYPE_ACK_RENAME), &mut params);
    }
}
pub(crate) fn detect() -> PlatformCapability {
    match unsafe { CfGetPlatformInfo() } {
        Ok(info) => PlatformCapability {
            available: true,
            version: Some(format!(
                "{}.{}.{}",
                info.BuildNumber, info.RevisionNumber, info.IntegrationNumber
            )),
            reason: None,
        },
        Err(_) => PlatformCapability::unavailable("Windows Cloud Files platform is unavailable."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Source;
    impl ReadSource for Source {
        fn read_exact(
            &self,
            _: &MountEntry,
            offset: u64,
            length: usize,
            cancel: &CancellationToken,
        ) -> Result<Vec<u8>> {
            cancel.check()?;
            Ok((offset..offset + length as u64)
                .map(|n| (n % 251) as u8)
                .collect())
        }
    }
    #[test]
    #[ignore = "requires a running Windows Cloud Files filter; run explicitly with native tests"]
    fn ordinary_directory_is_not_a_sync_root() {
        let temp = tempfile::tempdir().unwrap();
        let handle = open_metadata(temp.path(), true).unwrap();
        assert!(root_identity(&handle).unwrap().is_none());
    }
    #[test]
    fn callback_panics_and_invalid_identities_do_not_escape_ffi() {
        let entries = vec![
            MountEntry {
                path: "test.bin".into(),
                remote_id: "test".into(),
                identity: "account:version".into(),
                size: 32_768,
                directory: false,
                modified_filetime: 0,
            },
            MountEntry {
                path: "folder".into(),
                remote_id: "folder".into(),
                identity: "account:folder".into(),
                size: 0,
                directory: true,
                modified_filetime: 0,
            },
        ];
        let snapshot = Arc::new(MountSnapshot::new(entries.clone()).unwrap());
        let session = ReadSession::verified(snapshot, Arc::new(Source)).unwrap();
        let mut ctx = Context {
            session: session.clone(),
            identities: BTreeMap::new(),
        };
        let info = CF_CALLBACK_INFO {
            CallbackContext: (&mut ctx as *mut Context).cast(),
            RequestKey: 42,
            ..Default::default()
        };
        unsafe {
            fetch(&info, &CF_CALLBACK_PARAMETERS::default());
            cancel(&info, ptr::null());
            fetch(ptr::null(), ptr::null());
        }
        assert!(session.begin(42).is_err());
        let request = session.begin(42).unwrap();
        unsafe {
            cancel(&info, ptr::null());
        }
        assert!(request.check().is_err());
    }
    use std::ptr;
    #[test]
    #[ignore = "Explicit native smoke: registers only a newly created isolated temporary root."]
    fn native_register_populate_hydrate_reconnect() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("native-smoke");
        let entries = vec![
            MountEntry {
                path: "test.bin".into(),
                remote_id: "test".into(),
                identity: "account:version".into(),
                size: 32_768,
                directory: false,
                modified_filetime: 0,
            },
            MountEntry {
                path: "folder".into(),
                remote_id: "folder".into(),
                identity: "account:folder".into(),
                size: 0,
                directory: true,
                modified_filetime: 0,
            },
        ];
        let snapshot = Arc::new(MountSnapshot::new(entries.clone()).unwrap());
        let outcome = std::panic::catch_unwind(|| {
            let mut mount =
                CloudFilesMount::start(&root, snapshot.clone(), Arc::new(Source)).unwrap();
            assert!(mount.is_connected());
            let path = root.join("test.bin");
            let handle =
                unsafe { CfOpenFileWithOplock(pw(&wide(&path)), CF_OPEN_FILE_FLAG_NONE) }.unwrap();
            let hydrated =
                unsafe { CfHydratePlaceholder(handle, 8192, 4096, CF_HYDRATE_FLAG_NONE, None) };
            unsafe { CfCloseHandle(handle) };
            hydrated.unwrap();
            // Another process models Explorer/player reads. Provider self reads
            // are deliberately blocked from implicitly hydrating extra ranges.
            let consumer = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "cloud_files::tests::native_reader_helper",
                    "--ignored",
                    "--nocapture",
                ])
                .env("NEKOPAK_MOUNT_SMOKE_PATH", &path)
                .output()
                .unwrap();
            assert!(
                consumer.status.success(),
                "{}",
                String::from_utf8_lossy(&consumer.stderr)
            );
            mount.stop().unwrap();
            assert!(!mount.is_connected());
            drop(mount);
            let mut mount = CloudFilesMount::start(&root, snapshot, Arc::new(Source)).unwrap();
            mount.stop().unwrap();
            drop(mount);
            let mut added = entries.clone();
            added.push(MountEntry {
                path: "folder/added.bin".into(),
                remote_id: "added".into(),
                identity: "account:added".into(),
                size: 16_384,
                directory: false,
                modified_filetime: 0,
            });
            let mut mount = CloudFilesMount::start(
                &root,
                Arc::new(MountSnapshot::new(added.clone()).unwrap()),
                Arc::new(Source),
            )
            .unwrap();
            assert!(root.join("folder/added.bin").exists());
            mount.stop().unwrap();
            drop(mount);
            added[0].identity = "account:changed".into();
            assert!(CloudFilesMount::start(
                &root,
                Arc::new(MountSnapshot::new(added).unwrap()),
                Arc::new(Source)
            )
            .is_err());
            assert!(root.join("test.bin").exists());
        });
        // Only this test's newly created, canonical path may be unregistered.
        if root.exists() {
            let resolved = root.canonicalize().unwrap();
            let parent = temp.path().canonicalize().unwrap();
            assert!(resolved.starts_with(parent));
            let handle = open_metadata(&root, true).unwrap();
            let registered = root_identity(&handle).unwrap().is_some();
            drop(handle);
            if registered {
                unsafe { CfUnregisterSyncRoot(pw(&wide(&root))) }.unwrap();
            }
        }
        // Assert cleanup rather than relying on TempDir's best-effort Drop.
        temp.close().unwrap();
        if let Err(panic) = outcome {
            std::panic::resume_unwind(panic);
        }
    }
    #[test]
    #[ignore = "Child consumer for the explicit temporary-root native smoke."]
    fn native_reader_helper() {
        let Some(path) = std::env::var_os("NEKOPAK_MOUNT_SMOKE_PATH") else {
            return;
        };
        let mut f = File::open(path).unwrap();
        use std::io::{Seek, SeekFrom};
        for offset in [24_576, 8192, 0] {
            f.seek(SeekFrom::Start(offset)).unwrap();
            let mut actual = vec![0; 4096];
            f.read_exact(&mut actual).unwrap();
            assert_eq!(
                actual,
                (offset..offset + 4096)
                    .map(|n| (n % 251) as u8)
                    .collect::<Vec<_>>()
            );
        }
    }
}
