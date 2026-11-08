//! Explicit per-user startup registration. No default write and no elevation.
use nekopak_domain::{ClientError, ErrorCode};
use serde_json::{json, Value};
fn unavailable() -> ClientError {
    ClientError::new(ErrorCode::PermissionDenied, "无法访问当前用户的启动项。")
}
#[cfg(windows)]
fn read_write(path: &str, name: &str, write: Option<bool>) -> Result<bool, ClientError> {
    use windows_sys::Win32::{
        Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS},
        System::Registry::*,
    };
    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(Some(0)).collect()
    }
    let path = wide(path);
    let name = wide(name);
    let mut key = std::ptr::null_mut();
    let rights = if write.is_some() {
        KEY_QUERY_VALUE | KEY_SET_VALUE
    } else {
        KEY_QUERY_VALUE
    };
    let result = unsafe {
        if write == Some(true) {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                path.as_ptr(),
                0,
                std::ptr::null(),
                0,
                rights,
                std::ptr::null(),
                &mut key,
                std::ptr::null_mut(),
            )
        } else {
            RegOpenKeyExW(HKEY_CURRENT_USER, path.as_ptr(), 0, rights, &mut key)
        }
    };
    if result == ERROR_FILE_NOT_FOUND {
        return Ok(false);
    }
    if result != ERROR_SUCCESS {
        return Err(unavailable());
    }
    struct Key(HKEY);
    impl Drop for Key {
        fn drop(&mut self) {
            unsafe {
                RegCloseKey(self.0);
            }
        }
    }
    let _owned = Key(key);
    let exe = std::env::current_exe().map_err(|_| unavailable())?;
    let command = format!("\"{}\"", exe.to_string_lossy());
    if let Some(enabled) = write {
        let result = if enabled {
            let value = wide(&command);
            unsafe {
                RegSetValueExW(
                    key,
                    name.as_ptr(),
                    0,
                    REG_SZ,
                    value.as_ptr().cast(),
                    (value.len() * 2) as u32,
                )
            }
        } else {
            unsafe { RegDeleteValueW(key, name.as_ptr()) }
        };
        if result != ERROR_SUCCESS && result != ERROR_FILE_NOT_FOUND {
            return Err(unavailable());
        }
    }
    let mut kind = 0;
    let mut size = 0;
    let result = unsafe {
        RegQueryValueExW(
            key,
            name.as_ptr(),
            std::ptr::null(),
            &mut kind,
            std::ptr::null_mut(),
            &mut size,
        )
    };
    if result == ERROR_FILE_NOT_FOUND {
        return Ok(false);
    }
    if result != ERROR_SUCCESS || kind != REG_SZ || size > 65536 || size % 2 != 0 {
        return Err(unavailable());
    }
    let mut value = vec![0u16; size as usize / 2];
    if unsafe {
        RegQueryValueExW(
            key,
            name.as_ptr(),
            std::ptr::null(),
            &mut kind,
            value.as_mut_ptr().cast(),
            &mut size,
        )
    } != ERROR_SUCCESS
    {
        return Err(unavailable());
    }
    while value.last() == Some(&0) {
        value.pop();
    }
    Ok(String::from_utf16(&value).map_err(|_| unavailable())? == command)
}
const RUN: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
#[tauri::command]
pub fn autostart_get() -> Result<Value, ClientError> {
    #[cfg(windows)]
    {
        Ok(json!({"enabled":read_write(RUN,"NekoPak",None)?,"available":true}))
    }
    #[cfg(not(windows))]
    {
        Ok(json!({"enabled":false,"available":false}))
    }
}
#[tauri::command]
pub fn autostart_set(enabled: bool) -> Result<Value, ClientError> {
    #[cfg(windows)]
    {
        Ok(json!({"enabled":read_write(RUN,"NekoPak",Some(enabled))?,"available":true}))
    }
    #[cfg(not(windows))]
    {
        let _ = enabled;
        Err(ClientError::new(
            ErrorCode::Unsupported,
            "当前平台启动项尚未接入。",
        ))
    }
}
#[cfg(all(test, windows))]
mod tests {
    use super::*;
    #[test]
    fn synthetic_registry_roundtrip_never_touches_run_key() {
        let path = format!("Software\\NekoPakTests\\startup-{}", std::process::id());
        assert!(!read_write(&path, "test", None).unwrap());
        assert!(read_write(&path, "test", Some(true)).unwrap());
        assert!(read_write(&path, "test", None).unwrap());
        assert!(!read_write(&path, "test", Some(false)).unwrap());
        use windows_sys::Win32::System::Registry::*;
        let path: Vec<u16> = path.encode_utf16().chain(Some(0)).collect();
        assert_eq!(
            unsafe { RegDeleteKeyW(HKEY_CURRENT_USER, path.as_ptr()) },
            0
        );
    }
}
