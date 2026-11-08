//! Independently written WinFsp 2.x ABI adapter; no upstream implementation copied.
//! Public ABI researched at 4425d4807aea4626affb151e38c1cc774ccd5e75,
//! inc/winfsp/{winfsp.h,fsctl.h}. Runtime licensing: GPLv3 + FLOSS DLL exception.
use crate::{
    model::{error, invalid, path_key},
    windows_util::*,
    *,
};
use nekopak_domain::ErrorCode;
use std::{
    ffi::c_void,
    fs::File,
    mem::{offset_of, size_of},
    path::PathBuf,
    ptr,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};
use windows::{
    core::{PCSTR, PWSTR},
    Win32::{
        Foundation::*,
        Security::{Authorization::*, *},
        Storage::FileSystem::*,
        System::{LibraryLoader::*, Registry::*, Threading::*},
    },
};
type Status = i32;
const SUCCESS: Status = 0;
const DENIED: Status = 0xC0000022u32 as i32;
const NOT_FOUND: Status = 0xC0000034u32 as i32;
const INVALID: Status = 0xC000000Du32 as i32;
const EOF: Status = 0xC0000011u32 as i32;
const CANCELLED: Status = 0xC0000120u32 as i32;
const IO_ERROR: Status = 0xC0000185u32 as i32;
const BUFFER_SMALL: Status = 0x80000005u32 as i32;

// Only the stable public header is dereferenced. The DLL allocates its full object.
#[repr(C)]
struct Fs {
    version: u16,
    user_context: *mut c_void,
}
#[repr(C)]
struct Interface {
    slots: [usize; 64],
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct FileInfo {
    attributes: u32,
    reparse: u32,
    allocation: u64,
    size: u64,
    created: u64,
    accessed: u64,
    modified: u64,
    changed: u64,
    index: u64,
    links: u32,
    ea: u32,
}
#[repr(C)]
struct VolumeInfo {
    total: u64,
    free: u64,
    label_length: u16,
    label: [u16; 32],
}
#[repr(C)]
struct VolumeParams {
    version: u16,
    sector: u16,
    sectors_per_allocation: u16,
    max_component: u16,
    created: u64,
    serial: u32,
    transact_timeout: u32,
    irp_timeout: u32,
    irp_capacity: u32,
    file_info_timeout: u32,
    flags: u32,
    prefix: [u16; 192],
    name: [u16; 16],
    extra_flags: u32,
    volume_timeout: u32,
    dir_timeout: u32,
    security_timeout: u32,
    stream_timeout: u32,
    ea_timeout: u32,
    control: u32,
    reserved32: [u32; 1],
    reserved64: [u64; 2],
}
#[repr(C)]
struct DirInfo {
    size: u16,
    info: FileInfo,
    padding: [u8; 24],
    name: [u16; 255],
}
const _: () = assert!(
    size_of::<FileInfo>() == 72
        && size_of::<VolumeInfo>() == 88
        && size_of::<VolumeParams>() == 504
        && offset_of!(DirInfo, name) == 104
        && size_of::<Interface>() == 64 * size_of::<usize>()
);
type Create =
    unsafe extern "C" fn(*mut u16, *const VolumeParams, *const Interface, *mut *mut Fs) -> Status;
type Delete = unsafe extern "C" fn(*mut Fs);
type SetMount = unsafe extern "C" fn(*mut Fs, *mut u16) -> Status;
type Start = unsafe extern "C" fn(*mut Fs, u32) -> Status;
type Stop = unsafe extern "C" fn(*mut Fs);
type AddDir = unsafe extern "C" fn(*const DirInfo, *mut c_void, u32, *mut u32) -> u8;
type Version = unsafe extern "C" fn(*mut u32) -> Status;
type DispatcherResult = unsafe extern "C" fn(*mut Fs, *mut Status);
struct Library(HMODULE);
unsafe impl Send for Library {}
unsafe impl Sync for Library {}
impl Drop for Library {
    fn drop(&mut self) {
        unsafe {
            let _ = FreeLibrary(self.0);
        }
    }
}
struct Api {
    create: Create,
    delete: Delete,
    set_mount: SetMount,
    remove_mount: Stop,
    start: Start,
    stop: Stop,
    add_dir: AddDir,
    dispatcher_result: DispatcherResult,
    version: u32,
    _library: Option<Library>,
    _locks: Vec<File>,
}
impl Api {
    fn load() -> Result<Arc<Self>> {
        #[cfg(not(target_arch = "x86_64"))]
        return Err(error(
            ErrorCode::Unsupported,
            "WinFsp 当前仅支持 Windows x64。",
        ));
        let key = wide("SOFTWARE\\WinFsp");
        let value = wide("InstallDir");
        let mut path = [0u16; 32768];
        let mut bytes = (path.len() * 2) as u32;
        let code = unsafe {
            RegGetValueW(
                HKEY_LOCAL_MACHINE,
                pw(&key),
                pw(&value),
                RRF_RT_REG_SZ | RRF_SUBKEY_WOW6432KEY,
                None,
                Some(path.as_mut_ptr().cast()),
                Some(&mut bytes),
            )
        };
        if code != ERROR_SUCCESS {
            return Err(error(
                ErrorCode::Unsupported,
                "未找到用户安装的 WinFsp 运行时。",
            ));
        }
        let n = path.iter().position(|c| *c == 0).ok_or_else(invalid)?;
        let directory = PathBuf::from(String::from_utf16(&path[..n]).map_err(|_| invalid())?);
        let bin = directory.join("bin");
        let mut locks = lock_ancestors(&bin)?;
        let dll = bin.join("winfsp-x64.dll");
        let file = open_metadata(&dll, false)?;
        if is_reparse(&file)? {
            return Err(invalid());
        }
        locks.push(file);
        // Explicit administrator-installed path and safe dependency search; never PATH/CWD.
        let library = Library(
            unsafe {
                LoadLibraryExW(
                    pw(&wide(&dll)),
                    None,
                    LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32,
                )
            }
            .map_err(platform_error)?,
        );
        macro_rules! symbol {
            ($name:literal,$ty:ty) => {{
                let address =
                    unsafe { GetProcAddress(library.0, PCSTR(concat!($name, "\0").as_ptr())) }
                        .ok_or_else(|| {
                            error(ErrorCode::Unsupported, "WinFsp 运行时缺少所需接口。")
                        })?;
                unsafe { std::mem::transmute::<unsafe extern "system" fn() -> isize, $ty>(address) }
            }};
        }
        let version_fn = symbol!("FspVersion", Version);
        let mut version = 0;
        nt(unsafe { version_fn(&mut version) })?;
        Ok(Arc::new(Self {
            create: symbol!("FspFileSystemCreate", Create),
            delete: symbol!("FspFileSystemDelete", Delete),
            set_mount: symbol!("FspFileSystemSetMountPoint", SetMount),
            remove_mount: symbol!("FspFileSystemRemoveMountPoint", Stop),
            start: symbol!("FspFileSystemStartDispatcher", Start),
            stop: symbol!("FspFileSystemStopDispatcher", Stop),
            add_dir: symbol!("FspFileSystemAddDirInfo", AddDir),
            dispatcher_result: symbol!("FspFileSystemGetDispatcherResultF", DispatcherResult),
            version,
            _library: Some(library),
            _locks: locks,
        }))
    }
}
fn nt(status: Status) -> Result<()> {
    if status < 0 {
        Err(error(
            ErrorCode::PermissionDenied,
            "WinFsp 无法启动文件系统；请检查驱动、权限或盘符占用。",
        ))
    } else {
        Ok(())
    }
}
struct Context {
    session: Arc<ReadSession>,
    api: Arc<Api>,
    security: Vec<u8>,
    next: AtomicU64,
}
struct OpenFile {
    entry: MountEntry,
    index: u64,
}
pub struct WinFspMount {
    fs: *mut Fs,
    context: Box<Context>,
    _interface: Box<Interface>,
    drive: String,
}
unsafe impl Send for WinFspMount {}
impl WinFspMount {
    pub fn start(
        drive: &str,
        snapshot: Arc<MountSnapshot>,
        source: Arc<dyn ReadSource>,
    ) -> Result<Self> {
        let drive = validate_drive(drive)?;
        let index = drive.as_bytes()[0] - b'A';
        if unsafe { GetLogicalDrives() } & (1u32 << index) != 0 {
            return Err(error(ErrorCode::Conflict, "所选盘符已被占用。"));
        }
        let session = ReadSession::verified(snapshot, source)?;
        let api = Api::load()?;
        let mut context = Box::new(Context {
            session,
            api: api.clone(),
            security: user_security()?,
            next: AtomicU64::new(1),
        });
        let mut interface = Box::new(Interface { slots: [0; 64] });
        interface.slots[0] = get_volume as *const () as usize;
        interface.slots[2] = get_security_by_name as *const () as usize;
        interface.slots[4] = open as *const () as usize;
        interface.slots[7] = close as *const () as usize;
        interface.slots[8] = read as *const () as usize;
        interface.slots[10] = flush as *const () as usize;
        interface.slots[11] = get_info as *const () as usize;
        interface.slots[16] = get_security as *const () as usize;
        interface.slots[18] = read_directory as *const () as usize;
        interface.slots[32] = dispatcher_stopped as *const () as usize;
        let mut params: VolumeParams = unsafe { std::mem::zeroed() };
        params.version = size_of::<VolumeParams>() as u16;
        params.sector = 512;
        params.sectors_per_allocation = 8;
        params.max_component = 255;
        params.serial = 0x4E50414B;
        params.irp_timeout = 60_000;
        params.irp_capacity = 100;
        params.file_info_timeout = 1000;
        // Case-preserved, Unicode, ACLs, read-only, no reparse points/streams/writes.
        // FileContext identifies an individual open descriptor (bit 16), not a
        // shared file node. This matches the separately allocated OpenFile.
        params.flags =
            (1 << 1) | (1 << 2) | (1 << 3) | (1 << 9) | (1 << 10) | (1 << 12) | (1 << 16);
        for (dst, src) in params.name.iter_mut().zip("NekoPak".encode_utf16()) {
            *dst = src;
        }
        let mut device = wide("WinFsp.Disk");
        let mut fs = ptr::null_mut();
        nt(unsafe { (api.create)(device.as_mut_ptr(), &params, &*interface, &mut fs) })?;
        if fs.is_null() {
            return Err(invalid());
        }
        unsafe {
            (*fs).user_context = (&mut *context as *mut Context).cast();
        }
        let mut mount = Self {
            fs,
            context,
            _interface: interface,
            drive,
        };
        let mut target = wide(&mount.drive);
        if let Err(e) = nt(unsafe { (api.set_mount)(fs, target.as_mut_ptr()) }) {
            mount.stop()?;
            return Err(e);
        }
        if let Err(e) = nt(unsafe { (api.start)(fs, 4) }) {
            mount.stop()?;
            return Err(e);
        }
        let mut dispatcher = 0;
        unsafe { (api.dispatcher_result)(fs, &mut dispatcher) };
        nt(dispatcher)?;
        if !mount.is_connected() {
            return Err(error(ErrorCode::TransientNetwork, "WinFsp 调度器已停止。"));
        }
        Ok(mount)
    }
    pub fn drive(&self) -> &str {
        &self.drive
    }
    pub fn is_connected(&self) -> bool {
        !self.fs.is_null() && !self.context.session.is_stopped()
    }
    pub fn stop(&mut self) -> Result<()> {
        self.context.session.stop();
        if !self.fs.is_null() {
            unsafe {
                (self.context.api.stop)(self.fs);
                (self.context.api.remove_mount)(self.fs);
                (self.context.api.delete)(self.fs);
            }
            self.fs = ptr::null_mut();
        }
        Ok(())
    }
}
impl Drop for WinFspMount {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}
fn validate_drive(drive: &str) -> Result<String> {
    let bytes = drive.as_bytes();
    if bytes.len() != 2 || !bytes[0].is_ascii_alphabetic() || bytes[1] != b':' {
        return Err(error(
            ErrorCode::Conflict,
            "请输入 A: 至 Z: 格式的空闲盘符。",
        ));
    }
    Ok(drive.to_ascii_uppercase())
}
unsafe fn context<'a>(fs: *mut Fs) -> Option<&'a Context> {
    fs.as_ref()?.user_context.cast::<Context>().as_ref()
}
unsafe fn from_wide(value: *const u16) -> std::result::Result<String, Status> {
    if value.is_null() {
        return Err(INVALID);
    }
    let mut n = 0;
    while n < 32_768 && *value.add(n) != 0 {
        n += 1;
    }
    if n == 32_768 {
        return Err(INVALID);
    }
    String::from_utf16(std::slice::from_raw_parts(value, n)).map_err(|_| INVALID)
}
fn guard(f: impl FnOnce() -> Status) -> Status {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).unwrap_or(IO_ERROR)
}
unsafe fn lookup(ctx: &Context, name: *const u16) -> std::result::Result<MountEntry, Status> {
    let name = from_wide(name)?;
    let relative = name.strip_prefix('\\').ok_or(INVALID)?.replace('\\', "/");
    if relative.is_empty() {
        return Ok(MountEntry {
            path: String::new(),
            remote_id: String::new(),
            identity: String::new(),
            size: 0,
            directory: true,
            modified_filetime: 0,
        });
    }
    validate_relative_path(&relative).map_err(|_| INVALID)?;
    ctx.session
        .snapshot
        .get(&relative)
        .cloned()
        .ok_or(NOT_FOUND)
}
fn info(entry: &MountEntry, index: u64) -> FileInfo {
    FileInfo {
        attributes: if entry.directory {
            FILE_ATTRIBUTE_DIRECTORY.0
        } else {
            FILE_ATTRIBUTE_READONLY.0
        },
        allocation: (entry.size.saturating_add(4095) / 4096) * 4096,
        size: entry.size,
        created: entry.modified_filetime,
        accessed: entry.modified_filetime,
        modified: entry.modified_filetime,
        changed: entry.modified_filetime,
        index,
        ..Default::default()
    }
}
fn index(entry: &MountEntry) -> u64 {
    let mut n = 14695981039346656037u64;
    for b in entry.remote_id.as_bytes() {
        n = (n ^ *b as u64).wrapping_mul(1099511628211);
    }
    n
}
unsafe extern "C" fn get_volume(fs: *mut Fs, out: *mut VolumeInfo) -> Status {
    guard(|| {
        let Some(ctx) = context(fs) else {
            return INVALID;
        };
        let Some(out) = out.as_mut() else {
            return INVALID;
        };
        *out = VolumeInfo {
            total: ctx.session.snapshot.total_bytes(),
            free: 0,
            label_length: 14,
            label: [0; 32],
        };
        for (dst, src) in out.label.iter_mut().zip("NekoPak".encode_utf16()) {
            *dst = src;
        }
        SUCCESS
    })
}
unsafe extern "C" fn get_security_by_name(
    fs: *mut Fs,
    name: *mut u16,
    attributes: *mut u32,
    buffer: *mut c_void,
    size: *mut usize,
) -> Status {
    guard(|| {
        let Some(ctx) = context(fs) else {
            return INVALID;
        };
        let entry = match lookup(ctx, name) {
            Ok(e) => e,
            Err(e) => return e,
        };
        if let Some(attributes) = attributes.as_mut() {
            *attributes = info(&entry, index(&entry)).attributes;
        }
        copy_security(&ctx.security, buffer, size)
    })
}
unsafe extern "C" fn open(
    fs: *mut Fs,
    name: *mut u16,
    options: u32,
    access: u32,
    out: *mut *mut c_void,
    file_info: *mut FileInfo,
) -> Status {
    guard(|| {
        let Some(ctx) = context(fs) else {
            return INVALID;
        };
        if out.is_null() || file_info.is_null() {
            return INVALID;
        }
        // No writable handle, metadata mutation, delete-on-close or ownership changes.
        if access & (0x2 | 0x4 | 0x10 | 0x100 | 0x10000 | 0x40000 | 0x80000) != 0
            || options & 0x1000 != 0
        {
            return DENIED;
        }
        let entry = match lookup(ctx, name) {
            Ok(e) => e,
            Err(e) => return e,
        };
        if options & 1 != 0 && !entry.directory {
            return 0xC0000103u32 as i32;
        }
        if options & 0x40 != 0 && entry.directory {
            return 0xC00000BAu32 as i32;
        }
        let index = index(&entry);
        *file_info = info(&entry, index);
        *out = Box::into_raw(Box::new(OpenFile { entry, index })).cast();
        SUCCESS
    })
}
unsafe extern "C" fn close(_: *mut Fs, file: *mut c_void) {
    if !file.is_null() {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            drop(Box::from_raw(file.cast::<OpenFile>()))
        }));
    }
}
unsafe extern "C" fn get_info(_: *mut Fs, file: *mut c_void, out: *mut FileInfo) -> Status {
    guard(|| {
        let Some(file) = file.cast::<OpenFile>().as_ref() else {
            return INVALID;
        };
        let Some(out) = out.as_mut() else {
            return INVALID;
        };
        *out = info(&file.entry, file.index);
        SUCCESS
    })
}
unsafe extern "C" fn flush(fs: *mut Fs, file: *mut c_void, out: *mut FileInfo) -> Status {
    if file.is_null() {
        SUCCESS
    } else {
        get_info(fs, file, out)
    }
}
unsafe extern "C" fn read(
    fs: *mut Fs,
    file: *mut c_void,
    buffer: *mut c_void,
    offset: u64,
    length: u32,
    transferred: *mut u32,
) -> Status {
    guard(|| {
        let Some(ctx) = context(fs) else {
            return INVALID;
        };
        let Some(file) = file.cast::<OpenFile>().as_ref() else {
            return INVALID;
        };
        if buffer.is_null() || transferred.is_null() || file.entry.directory {
            return INVALID;
        }
        *transferred = 0;
        if length == 0 {
            return SUCCESS;
        }
        if offset >= file.entry.size {
            return EOF;
        }
        let request = match ctx.session.begin(ctx.next.fetch_add(1, Ordering::Relaxed)) {
            Ok(r) => r,
            Err(e) => return status(&e),
        };
        let total = (length as u64).min(file.entry.size - offset);
        let mut done = 0;
        while done < total {
            let take = (total - done).min(1024 * 1024) as usize;
            let bytes = match request.read(&file.entry, offset + done, take) {
                Ok(b) => b,
                Err(e) => return status(&e),
            };
            ptr::copy_nonoverlapping(bytes.as_ptr(), buffer.cast::<u8>().add(done as usize), take);
            done += take as u64;
        }
        *transferred = done as u32;
        SUCCESS
    })
}
unsafe extern "C" fn get_security(
    fs: *mut Fs,
    _: *mut c_void,
    buffer: *mut c_void,
    size: *mut usize,
) -> Status {
    guard(|| {
        let Some(ctx) = context(fs) else {
            return INVALID;
        };
        copy_security(&ctx.security, buffer, size)
    })
}
unsafe fn copy_security(security: &[u8], buffer: *mut c_void, size: *mut usize) -> Status {
    let Some(size) = size.as_mut() else {
        return SUCCESS;
    };
    let capacity = *size;
    *size = security.len();
    if buffer.is_null() {
        return SUCCESS;
    }
    if capacity < security.len() {
        return BUFFER_SMALL;
    }
    ptr::copy_nonoverlapping(security.as_ptr(), buffer.cast(), security.len());
    SUCCESS
}
unsafe extern "C" fn read_directory(
    fs: *mut Fs,
    file: *mut c_void,
    _: *mut u16,
    marker: *mut u16,
    buffer: *mut c_void,
    length: u32,
    transferred: *mut u32,
) -> Status {
    guard(|| {
        let Some(ctx) = context(fs) else {
            return INVALID;
        };
        let Some(file) = file.cast::<OpenFile>().as_ref() else {
            return INVALID;
        };
        if !file.entry.directory || buffer.is_null() || transferred.is_null() {
            return INVALID;
        }
        *transferred = 0;
        let marker = if marker.is_null() {
            String::new()
        } else {
            match from_wide(marker) {
                Ok(s) => path_key(&s),
                Err(e) => return e,
            }
        };
        let mut rows: Vec<(String, MountEntry)> = ctx
            .session
            .snapshot
            .children(&file.entry.path)
            .into_iter()
            .map(|e| {
                (
                    e.path.rsplit('/').next().unwrap_or("").to_owned(),
                    e.clone(),
                )
            })
            .collect();
        if !file.entry.path.is_empty() {
            rows.push((".".into(), file.entry.clone()));
            let parent = file.entry.path.rsplit_once('/').map(|p| p.0).unwrap_or("");
            let entry = ctx
                .session
                .snapshot
                .get(parent)
                .cloned()
                .unwrap_or(MountEntry {
                    path: String::new(),
                    remote_id: String::new(),
                    identity: String::new(),
                    size: 0,
                    directory: true,
                    modified_filetime: 0,
                });
            rows.push(("..".into(), entry));
        }
        rows.sort_by_key(|row| path_key(&row.0));
        for (name, entry) in rows {
            if path_key(&name) <= marker {
                continue;
            }
            let name: Vec<u16> = name.encode_utf16().collect();
            let mut row = DirInfo {
                size: (104 + name.len() * 2) as u16,
                info: info(&entry, index(&entry)),
                padding: [0; 24],
                name: [0; 255],
            };
            row.name[..name.len()].copy_from_slice(&name);
            if (ctx.api.add_dir)(&row, buffer, length, transferred) == 0 {
                return SUCCESS;
            }
        }
        (ctx.api.add_dir)(ptr::null(), buffer, length, transferred);
        SUCCESS
    })
}
unsafe extern "C" fn dispatcher_stopped(fs: *mut Fs, _: u8) {
    if let Some(ctx) = context(fs) {
        ctx.session.stop();
    }
}
fn status(error: &nekopak_domain::ClientError) -> Status {
    match error.code {
        ErrorCode::Cancelled => CANCELLED,
        ErrorCode::NotFound => NOT_FOUND,
        ErrorCode::PermissionDenied | ErrorCode::AuthExpired => DENIED,
        _ => IO_ERROR,
    }
}
fn user_security() -> Result<Vec<u8>> {
    // The virtual volume is readable only by this account and SYSTEM.
    unsafe {
        let mut token = HANDLE::default();
        OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).map_err(platform_error)?;
        let mut buffer = vec![0u64; 512];
        let mut returned = 0;
        let result = GetTokenInformation(
            token,
            TokenUser,
            Some(buffer.as_mut_ptr().cast()),
            (buffer.len() * 8) as u32,
            &mut returned,
        );
        let _ = CloseHandle(token);
        result.map_err(platform_error)?;
        let user = &*buffer.as_ptr().cast::<TOKEN_USER>();
        let mut sid = PWSTR::null();
        ConvertSidToStringSidW(user.User.Sid, &mut sid).map_err(platform_error)?;
        let sid_text = sid.to_string().map_err(|_| invalid());
        let _ = LocalFree(Some(HLOCAL(sid.0.cast())));
        let sid_text = sid_text?;
        let text = wide(format!(
            "O:{sid_text}G:{sid_text}D:P(A;;FRFX;;;{sid_text})(A;;FRFX;;;SY)"
        ));
        let mut security = PSECURITY_DESCRIPTOR::default();
        let mut length = 0;
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            pw(&text),
            1,
            &mut security,
            Some(&mut length),
        )
        .map_err(platform_error)?;
        let bytes = std::slice::from_raw_parts(security.0.cast::<u8>(), length as usize).to_vec();
        let _ = LocalFree(Some(HLOCAL(security.0)));
        Ok(bytes)
    }
}
pub(crate) fn detect() -> PlatformCapability {
    match Api::load() {
        Ok(api) => PlatformCapability {
            available: true,
            version: Some(format!("0x{:08x}", api.version)),
            reason: Some(
                "Installed DLL verified; driver availability is checked by mount_start.".into(),
            ),
        },
        Err(_) => PlatformCapability::unavailable(
            "User-installed WinFsp x64 with the required DLL interfaces is unavailable.",
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;
    struct Source {
        panic: AtomicBool,
    }
    impl ReadSource for Source {
        fn read_exact(
            &self,
            _: &MountEntry,
            offset: u64,
            length: usize,
            cancel: &CancellationToken,
        ) -> Result<Vec<u8>> {
            cancel.check()?;
            assert!(!self.panic.load(Ordering::Acquire), "injected source panic");
            Ok((offset..offset + length as u64)
                .map(|n| (n % 251) as u8)
                .collect())
        }
    }
    unsafe extern "C" fn fake_create(
        _: *mut u16,
        _: *const VolumeParams,
        _: *const Interface,
        _: *mut *mut Fs,
    ) -> Status {
        SUCCESS
    }
    unsafe extern "C" fn fake_void(_: *mut Fs) {}
    unsafe extern "C" fn fake_mount(_: *mut Fs, _: *mut u16) -> Status {
        SUCCESS
    }
    unsafe extern "C" fn fake_start(_: *mut Fs, _: u32) -> Status {
        SUCCESS
    }
    unsafe extern "C" fn fake_result(_: *mut Fs, status: *mut Status) {
        *status = SUCCESS;
    }
    unsafe extern "C" fn fake_add_dir(
        row: *const DirInfo,
        out: *mut c_void,
        length: u32,
        transferred: *mut u32,
    ) -> u8 {
        if row.is_null() {
            return 1;
        }
        let bytes = (*row).size as usize;
        let padded = bytes.div_ceil(8) * 8;
        if *transferred as usize + padded > length as usize {
            return 0;
        }
        ptr::copy_nonoverlapping(
            row.cast::<u8>(),
            out.cast::<u8>().add(*transferred as usize),
            bytes,
        );
        *transferred += padded as u32;
        1
    }
    fn setup() -> (Box<Context>, Fs, Arc<Source>) {
        let api = Arc::new(Api {
            create: fake_create,
            delete: fake_void,
            set_mount: fake_mount,
            remove_mount: fake_void,
            start: fake_start,
            stop: fake_void,
            add_dir: fake_add_dir,
            dispatcher_result: fake_result,
            version: 0,
            _library: None,
            _locks: Vec::new(),
        });
        let entries = vec![
            MountEntry {
                path: "电影.bin".into(),
                remote_id: "one".into(),
                identity: "v1".into(),
                size: 16_384,
                directory: false,
                modified_filetime: 42,
            },
            MountEntry {
                path: "folder".into(),
                remote_id: "two".into(),
                identity: "v2".into(),
                size: 0,
                directory: true,
                modified_filetime: 43,
            },
        ];
        let source = Arc::new(Source {
            panic: AtomicBool::new(false),
        });
        let session = ReadSession::verified(
            Arc::new(MountSnapshot::new(entries).unwrap()),
            source.clone(),
        )
        .unwrap();
        let mut ctx = Box::new(Context {
            session,
            api,
            security: user_security().unwrap(),
            next: AtomicU64::new(1),
        });
        let fs = Fs {
            version: 0,
            user_context: (&mut *ctx as *mut Context).cast(),
        };
        (ctx, fs, source)
    }
    #[test]
    fn real_callback_abi_enforces_readonly_bounds_eof_and_cancel() {
        let (ctx, mut fs, source) = setup();
        let mut name = wide("\\电影.bin");
        let mut file = ptr::null_mut();
        let mut file_info = FileInfo::default();
        unsafe {
            assert_eq!(
                open(&mut fs, name.as_mut_ptr(), 0, 2, &mut file, &mut file_info),
                DENIED
            );
            assert_eq!(
                open(&mut fs, name.as_mut_ptr(), 0, 1, &mut file, &mut file_info),
                SUCCESS
            );
            assert_eq!(file_info.size, 16_384);
            assert_ne!(file_info.attributes & FILE_ATTRIBUTE_READONLY.0, 0);
            let mut output = vec![0u8; 2000];
            let mut transferred = 0;
            assert_eq!(
                read(
                    &mut fs,
                    file,
                    output.as_mut_ptr().cast(),
                    16_000,
                    2000,
                    &mut transferred
                ),
                SUCCESS
            );
            assert_eq!(transferred, 384);
            assert_eq!(
                &output[..384],
                &(16_000..16_384)
                    .map(|n| (n % 251) as u8)
                    .collect::<Vec<_>>()
            );
            assert_eq!(
                read(
                    &mut fs,
                    file,
                    output.as_mut_ptr().cast(),
                    16_384,
                    1,
                    &mut transferred
                ),
                EOF
            );
            source.panic.store(true, Ordering::Release);
            assert_eq!(
                read(
                    &mut fs,
                    file,
                    output.as_mut_ptr().cast(),
                    0,
                    1,
                    &mut transferred
                ),
                IO_ERROR
            );
            source.panic.store(false, Ordering::Release);
            ctx.session.stop();
            assert_eq!(
                read(
                    &mut fs,
                    file,
                    output.as_mut_ptr().cast(),
                    0,
                    1,
                    &mut transferred
                ),
                CANCELLED
            );
            close(&mut fs, file);
        }
    }
    #[test]
    fn directory_resume_security_and_stopped_dispatcher() {
        let (ctx, mut fs, _) = setup();
        let mut root = wide("\\");
        let mut file = ptr::null_mut();
        let mut fi = FileInfo::default();
        unsafe {
            assert_eq!(
                open(&mut fs, root.as_mut_ptr(), 1, 1, &mut file, &mut fi),
                SUCCESS
            );
            let mut out = vec![0u64; 256];
            let mut count = 0;
            assert_eq!(
                read_directory(
                    &mut fs,
                    file,
                    ptr::null_mut(),
                    ptr::null_mut(),
                    out.as_mut_ptr().cast(),
                    2048,
                    &mut count
                ),
                SUCCESS
            );
            assert!(count > 208);
            let first = &*out.as_ptr().cast::<DirInfo>();
            assert_eq!(
                String::from_utf16(&first.name[..((first.size as usize - 104) / 2)]).unwrap(),
                "folder"
            );
            let mut marker = wide("folder");
            assert_eq!(
                read_directory(
                    &mut fs,
                    file,
                    ptr::null_mut(),
                    marker.as_mut_ptr(),
                    out.as_mut_ptr().cast(),
                    2048,
                    &mut count
                ),
                SUCCESS
            );
            let first = &*out.as_ptr().cast::<DirInfo>();
            assert_eq!(
                String::from_utf16(&first.name[..((first.size as usize - 104) / 2)]).unwrap(),
                "电影.bin"
            );
            let mut size = 0;
            assert_eq!(
                get_security(&mut fs, file, ptr::null_mut(), &mut size),
                SUCCESS
            );
            assert!(size > 40);
            let mut short = vec![0u8; 1];
            size = 1;
            assert_eq!(
                get_security(&mut fs, file, short.as_mut_ptr().cast(), &mut size),
                BUFFER_SMALL
            );
            dispatcher_stopped(&mut fs, 0);
            assert!(ctx.session.is_stopped());
            close(&mut fs, file);
        }
    }
    #[test]
    fn drive_and_abi_guards() {
        assert_eq!(validate_drive("p:").unwrap(), "P:");
        for name in ["P", "P:\\", "C:/path", "\\\\server", "1:", "P::"] {
            assert!(validate_drive(name).is_err());
        }
        assert_eq!(size_of::<VolumeParams>(), 504);
        assert_eq!(offset_of!(Fs, user_context), 8);
        assert_eq!(offset_of!(DirInfo, name), 104);
    }
    #[test]
    fn discover_is_read_only_and_reports_actual_prerequisites() {
        let report = native_capabilities();
        println!("{}", serde_json::to_string(&report).unwrap());
        if report.winfsp.available {
            assert!(report.winfsp.version.is_some());
        } else {
            assert!(report.winfsp.reason.is_some());
        }
    }
}
