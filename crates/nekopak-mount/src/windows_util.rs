//! Windows handles and platform errors shared by the optional native adapters.
use crate::{
    model::{error, invalid},
    Result,
};
use nekopak_domain::ErrorCode;
use std::{
    fs::{File, OpenOptions},
    os::windows::{
        ffi::OsStrExt,
        fs::{MetadataExt, OpenOptionsExt},
        io::AsRawHandle,
    },
    path::{Component, Path, PathBuf},
};
use windows::{
    core::PCWSTR,
    Win32::{Foundation::HANDLE, Storage::FileSystem::*},
};

pub(crate) fn wide(value: impl AsRef<std::ffi::OsStr>) -> Vec<u16> {
    value.as_ref().encode_wide().chain(Some(0)).collect()
}
pub(crate) fn platform_error(_: windows::core::Error) -> nekopak_domain::ClientError {
    error(
        ErrorCode::PermissionDenied,
        "Windows 文件系统操作失败；请检查目录、权限与可选运行时。",
    )
}
pub(crate) fn io(_: std::io::Error) -> nekopak_domain::ClientError {
    error(ErrorCode::PermissionDenied, "无法安全访问挂载目录。")
}
pub(crate) fn open_metadata(path: &Path, directory: bool) -> Result<File> {
    let file = OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ.0 | FILE_SHARE_WRITE.0)
        .custom_flags(
            FILE_FLAG_OPEN_REPARSE_POINT.0
                | if directory {
                    FILE_FLAG_BACKUP_SEMANTICS.0
                } else {
                    0
                },
        )
        .open(path)
        .map_err(io)?;
    let metadata = file.metadata().map_err(io)?;
    if metadata.is_dir() != directory {
        return Err(invalid());
    }
    Ok(file)
}
pub(crate) fn is_reparse(file: &File) -> Result<bool> {
    Ok(file.metadata().map_err(io)?.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0)
}
pub(crate) fn file_id(file: &File) -> Result<(u32, u64)> {
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    unsafe { GetFileInformationByHandle(HANDLE(file.as_raw_handle()), &mut info) }
        .map_err(platform_error)?;
    Ok((
        info.dwVolumeSerialNumber,
        ((info.nFileIndexHigh as u64) << 32) | info.nFileIndexLow as u64,
    ))
}
/// Hold every ordinary ancestor without FILE_SHARE_DELETE so a junction cannot
/// replace an already-checked directory while a later path API resolves it.
pub(crate) fn lock_ancestors(path: &Path) -> Result<Vec<File>> {
    let text = path.to_str().ok_or_else(invalid)?;
    if !path.is_absolute() || text.starts_with("\\\\") || text.contains('\0') {
        return Err(invalid());
    }
    let mut current = PathBuf::new();
    let mut held = Vec::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) if matches!(prefix.kind(), std::path::Prefix::Disk(_)) => {
                current.push(prefix.as_os_str())
            }
            Component::RootDir => current.push(component.as_os_str()),
            Component::Normal(name) => current.push(name),
            _ => return Err(invalid()),
        }
        if current.has_root() {
            let file = open_metadata(&current, true)?;
            if is_reparse(&file)? {
                return Err(error(
                    ErrorCode::Conflict,
                    "挂载目录不能经过符号链接、联接点或其他同步根。",
                ));
            }
            held.push(file);
        }
    }
    Ok(held)
}
pub(crate) fn pw(value: &[u16]) -> PCWSTR {
    PCWSTR(value.as_ptr())
}
