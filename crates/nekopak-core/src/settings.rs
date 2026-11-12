use super::*;
use serde_json::{json, Value};
impl AppService {
    pub async fn ui_prefs_get(&self, ctx: &RequestContext) -> Result<Value, ClientError> {
        let auth = ctx.run(self.provider.auth_status()).await?;
        let account = auth
            .account_id
            .ok_or_else(|| ClientError::new(ErrorCode::AuthExpired, "请先选择账号。"))?;
        let scope = ctx.run(self.provider.account_scope_for(&account)).await?;
        let prefs = self
            .records()?
            .record_get(&scope, "ui-prefs", "app")?
            .map(|s| {
                serde_json::from_str::<Value>(&s).map_err(|_| {
                    ClientError::new(ErrorCode::IntegrityFailed, "账号偏好记录损坏，未覆盖。")
                })
            })
            .transpose()?
            .unwrap_or_else(|| json!({"pins":[]}));
        Ok(json!({"account_id":account,"prefs":prefs}))
    }
    pub async fn ui_prefs_update(
        &self,
        value: &Value,
        account: &str,
        ctx: &RequestContext,
    ) -> Result<Value, ClientError> {
        let scope = ctx.run(self.provider.account_scope_for(account)).await?;
        let object = value
            .as_object()
            .filter(|o| {
                o.keys().all(|k| {
                    matches!(
                        k.as_str(),
                        "pins" | "view" | "sort" | "density" | "gridSize" | "foldersFirst"
                    )
                })
            })
            .ok_or_else(|| ClientError::new(ErrorCode::Conflict, "账号偏好格式无效。"))?;
        if value.to_string().len() > 65536 {
            return Err(ClientError::new(ErrorCode::Conflict, "账号偏好过大。"));
        }
        if let Some(pins) = object.get("pins") {
            let pins = pins
                .as_array()
                .filter(|p| p.len() <= 100)
                .ok_or_else(|| ClientError::new(ErrorCode::Conflict, "固定目录数量过多。"))?;
            for pin in pins {
                for key in ["id", "name"] {
                    if pin[key].as_str().is_none_or(|s| {
                        s.is_empty() || s.len() > 4096 || s.chars().any(char::is_control)
                    }) {
                        return Err(ClientError::new(ErrorCode::Conflict, "固定目录无效。"));
                    }
                }
            }
        }
        self.records()?
            .record_put(&scope, "ui-prefs", "app", &value.to_string())?;
        Ok(value.clone())
    }
    pub fn settings_get(&self) -> Result<Value, ClientError> {
        self.store()?;
        let store = self
            .intents
            .as_ref()
            .ok_or_else(|| ClientError::new(ErrorCode::Internal, "设置存储不可用。"))?;
        let settings = if let Some(payload) = store.settings_get()? {
            let mut settings: Value = serde_json::from_str(&payload).map_err(|_| {
                ClientError::new(ErrorCode::IntegrityFailed, "设置损坏，未覆盖原文件。")
            })?;
            if settings.get("ask_download_directory").is_none() {
                settings["ask_download_directory"] = false.into();
            }
            if settings.get("close_to_tray").is_none() {
                settings["close_to_tray"] = false.into();
            }
            if settings.get("language").is_none() {
                settings["language"] = "system".into();
            }
            if settings.get("background_transfers").is_none() {
                settings["background_transfers"] = true.into();
            }
            if settings.get("media").is_none() {
                settings["media"] = json!({"player_path":""});
            }
            if settings.get("updates").is_none() {
                settings["updates"] = json!({"auto_check":false});
            }
            if settings.get("download").is_none() {
                settings["download"] =
                    json!({"default_quality":"original","connections_per_task":1});
            }
            if settings["experimental_network"]
                .get("compatibility_mode")
                .is_none()
            {
                settings["experimental_network"]["compatibility_mode"] = false.into();
            }
            if settings["experimental_network"].get("proxy_url").is_none() {
                settings["experimental_network"]["proxy_url"] = "".into();
            }
            if settings["experimental_network"]
                .get("block_access_domains")
                .is_none()
            {
                settings["experimental_network"]["block_access_domains"] = false.into();
            }
            if settings["experimental_network"]
                .get("server_domain")
                .is_none()
            {
                settings["experimental_network"]["server_domain"] = "auto".into();
            }
            validate(&settings)?;
            settings
        } else {
            #[cfg(target_os = "android")]
            let base = self.data_root.clone();
            #[cfg(not(target_os = "android"))]
            // Windows publishes USERPROFILE; other desktops use HOME. Without
            // either the path stays empty and the UI asks for a folder.
            let base = std::env::var_os("USERPROFILE")
                .or_else(|| std::env::var_os("HOME"))
                .map(std::path::PathBuf::from)
                .unwrap_or_default();
            #[cfg(target_os = "android")]
            std::fs::create_dir_all(base.join("Downloads")).map_err(|_| {
                ClientError::new(ErrorCode::PermissionDenied, "无法创建应用下载目录。")
            })?;
            json!({"schema_version":1,"theme":"system","language":"system","ask_download_directory":false,"close_to_tray":false,"background_transfers":true,"download_dir":base.join("Downloads").to_string_lossy(),"max_concurrent_transfers":1,"bandwidth_limit_kib_s":0,
                "cache":{"dir":"","max_bytes":"536870912"},"experimental_network":{"server_domain":"auto","enabled":false,"proxy_mode":"system","proxy_url":"","compatibility_mode":false,"block_access_domains":false,"user_proxy_configured":false,"custom_dns_mode":false,"connection_diagnostics":false},"telemetry":false,
                "download":{"default_quality":"original","connections_per_task":1},"media":{"player_path":""},"updates":{"auto_check":false}})
        };
        self.apply_network(&settings)?;
        Ok(settings)
    }
    pub fn settings_update(&self, patch: &Value) -> Result<Value, ClientError> {
        let mut settings = self.settings_get()?;
        let previous = settings.clone();
        let object = patch
            .as_object()
            .ok_or_else(|| ClientError::new(ErrorCode::Conflict, "设置格式无效。"))?;
        for (key, value) in object {
            match key.as_str() {
                "updates" => {
                    let options = value
                        .as_object()
                        .filter(|o| {
                            o.len() == 1 && o.get("auto_check").is_some_and(Value::is_boolean)
                        })
                        .ok_or_else(|| ClientError::new(ErrorCode::Conflict, "更新选项无效。"))?;
                    settings["updates"] = serde_json::to_value(options)
                        .map_err(|_| ClientError::new(ErrorCode::Internal, "无法保存更新设置。"))?;
                }
                "theme"
                | "download_dir"
                | "bandwidth_limit_kib_s"
                | "ask_download_directory"
                | "close_to_tray"
                | "background_transfers"
                | "language" => settings[key] = value.clone(),
                "experimental_network" => {
                    for (key, value) in value.as_object().ok_or_else(|| {
                        ClientError::new(ErrorCode::Conflict, "网络设置格式无效。")
                    })? {
                        if !matches!(
                            key.as_str(),
                            "enabled"
                                | "proxy_mode"
                                | "proxy_url"
                                | "compatibility_mode"
                                | "block_access_domains"
                                | "server_domain"
                        ) {
                            return Err(ClientError::new(
                                ErrorCode::Unsupported,
                                "该网络设置尚未实现。",
                            ));
                        }
                        settings["experimental_network"][key] = value.clone();
                    }
                }
                "download" => {
                    for (key, value) in value
                        .as_object()
                        .ok_or_else(|| ClientError::new(ErrorCode::Conflict, "下载设置无效。"))?
                    {
                        if key != "default_quality" {
                            return Err(ClientError::new(
                                ErrorCode::Unsupported,
                                "当前连接数由下载引擎管理。",
                            ));
                        }
                        settings["download"][key] = value.clone();
                    }
                }
                "media" => {
                    for (key, value) in value
                        .as_object()
                        .ok_or_else(|| ClientError::new(ErrorCode::Conflict, "播放器设置无效。"))?
                    {
                        if key != "player_path" {
                            return Err(ClientError::new(
                                ErrorCode::Unsupported,
                                "不支持该播放器设置。",
                            ));
                        }
                        settings["media"][key] = value.clone();
                    }
                }
                _ => {
                    return Err(ClientError::new(
                        ErrorCode::Unsupported,
                        "该设置尚未实现，没有保存无效选项。",
                    ))
                }
            }
        }
        validate(&settings)?;
        #[cfg(target_os = "android")]
        if settings["close_to_tray"] == true
            || settings["ask_download_directory"] == true
            || settings["media"]["player_path"]
                .as_str()
                .is_some_and(|s| !s.is_empty())
            || settings["download_dir"].as_str() != self.data_root.join("Downloads").to_str()
        {
            return Err(ClientError::new(
                ErrorCode::Unsupported,
                "Android 使用应用内下载目录，完成后可导出；桌面设置不可用。",
            ));
        }
        settings["experimental_network"]["user_proxy_configured"] = settings
            ["experimental_network"]["proxy_url"]
            .as_str()
            .is_some_and(|s| !s.is_empty())
            .into();
        if object.contains_key("download_dir") {
            let path = settings["download_dir"]
                .as_str()
                .ok_or_else(|| ClientError::new(ErrorCode::Conflict, "下载目录无效。"))?;
            validate_destination(path)?;
            if !Path::new(path).is_dir() {
                return Err(ClientError::new(
                    ErrorCode::Conflict,
                    "请选择已存在的本地下载目录。",
                ));
            }
        }
        self.intents
            .as_ref()
            .ok_or_else(|| ClientError::new(ErrorCode::Internal, "设置存储不可用。"))?
            .settings_put(&settings.to_string())?;
        if let Err(error) = self.apply_network(&settings) {
            self.intents
                .as_ref()
                .ok_or_else(|| ClientError::new(ErrorCode::Internal, "设置存储不可用。"))?
                .settings_put(&previous.to_string())?;
            return Err(error);
        }
        Ok(settings)
    }
    fn apply_network(&self, settings: &Value) -> Result<(), ClientError> {
        let n = &settings["experimental_network"];
        let mode = if n["enabled"] == true {
            n["proxy_mode"].as_str().unwrap_or("system")
        } else {
            "system"
        };
        self.provider.configure_network(
            mode,
            n["proxy_url"].as_str().filter(|p| !p.is_empty()),
            n["enabled"] == true && n["compatibility_mode"] == true,
        )?;
        self.provider
            .configure_server_domain(n["server_domain"].as_str().unwrap_or("auto"))?;
        self.provider
            .set_domain_blocking(n["block_access_domains"] == true)
    }
}
fn validate(settings: &Value) -> Result<(), ClientError> {
    if settings["updates"]["auto_check"].as_bool().is_none() {
        return Err(ClientError::new(
            ErrorCode::Conflict,
            "更新设置损坏，未覆盖。",
        ));
    }
    if !matches!(
        settings["experimental_network"]["server_domain"].as_str(),
        Some("auto" | "mypikpak.com" | "mypikpak.net" | "pikpak.me" | "pikpakdrive.com")
    ) {
        return Err(ClientError::new(
            ErrorCode::Conflict,
            "请选择受支持的服务器域名。",
        ));
    }
    if !matches!(
        settings["download"]["default_quality"].as_str(),
        Some("original" | "2160p" | "1080p" | "720p" | "480p")
    ) {
        return Err(ClientError::new(ErrorCode::Conflict, "默认画质无效。"));
    }
    if let Some(path) = settings["media"]["player_path"]
        .as_str()
        .filter(|s| !s.is_empty())
    {
        validate_destination(path)?;
        // Only Windows identifies programs by extension.
        if !Path::new(path).is_file()
            || (cfg!(windows) && !path.to_ascii_lowercase().ends_with(".exe"))
        {
            return Err(ClientError::new(
                ErrorCode::Conflict,
                "请选择已安装的播放器 EXE。",
            ));
        }
        let _ = crate::local::open_readonly(Path::new(path))?;
    }
    if settings["schema_version"] != 1
        || settings["experimental_network"]["block_access_domains"]
            .as_bool()
            .is_none()
        || settings["ask_download_directory"].as_bool().is_none()
        || settings["close_to_tray"].as_bool().is_none()
        || settings["background_transfers"].as_bool().is_none()
        || !matches!(
            settings["language"].as_str(),
            Some("system" | "zh-CN" | "zh-TW" | "en" | "ja-JP" | "ko-KR")
        )
        || !matches!(
            settings["theme"].as_str(),
            Some("system" | "light" | "dark")
        )
        || !matches!(
            settings["experimental_network"]["proxy_mode"].as_str(),
            Some("system" | "none" | "user_provided")
        )
        || settings["experimental_network"]["enabled"]
            .as_bool()
            .is_none()
        || settings["bandwidth_limit_kib_s"]
            .as_u64()
            .is_none_or(|n| n > 1048576)
        || settings["download_dir"].as_str().is_none()
        || settings["experimental_network"]["compatibility_mode"]
            .as_bool()
            .is_none()
    {
        return Err(ClientError::new(
            ErrorCode::Conflict,
            "设置包含无效值，未应用。",
        ));
    }
    let proxy = settings["experimental_network"]["proxy_url"]
        .as_str()
        .unwrap_or("");
    if !proxy.is_empty() {
        validate_proxy(proxy)?;
    }
    if settings["experimental_network"]["proxy_mode"] == "user_provided" && proxy.is_empty() {
        return Err(ClientError::new(
            ErrorCode::Conflict,
            "请输入 HTTP 或 SOCKS5 代理地址。",
        ));
    }
    Ok(())
}
fn validate_proxy(proxy: &str) -> Result<(), ClientError> {
    let fail = || {
        ClientError::new(ErrorCode::Conflict,"代理地址需为 http(s)://主机:端口 或 socks5(h)://主机:端口，不含用户名、密码、路径或查询参数。" )
    };
    let url = url::Url::parse(proxy).map_err(|_| fail())?;
    if proxy.len() > 1024
        || !matches!(url.scheme(), "http" | "https" | "socks5" | "socks5h")
        || url.host_str().is_none()
        || url.port_or_known_default().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || !matches!(url.path(), "" | "/")
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(fail());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn access_blocking_is_independent_default_off_and_restores_after_restart() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("block.db");
        let app = AppService::mock(&path, Scenario::Free);
        assert_eq!(
            app.settings_get().unwrap()["experimental_network"]["block_access_domains"],
            false
        );
        let saved = app
            .settings_update(&json!({"experimental_network":{"block_access_domains":true}}))
            .unwrap();
        assert_eq!(saved["experimental_network"]["enabled"], false);
        drop(app);
        let app = AppService::mock(&path, Scenario::Free);
        assert_eq!(
            app.settings_get().unwrap()["experimental_network"]["block_access_domains"],
            true
        );
        assert!(app
            .settings_update(&json!({"experimental_network":{"block_access_domains":"true"}}))
            .is_err());
        app.settings_update(&json!({"experimental_network":{"block_access_domains":false}}))
            .unwrap();
        assert_eq!(
            app.settings_get().unwrap()["experimental_network"]["block_access_domains"],
            false
        );
    }
    #[test]
    fn language_tray_and_network_switches_persist_and_invalid_proxy_does_not_save() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.db");
        let app = AppService::mock(&path, Scenario::Free);
        let settings=app.settings_update(&json!({"language":"en","close_to_tray":true,"ask_download_directory":true,"experimental_network":{"enabled":true,"proxy_mode":"user_provided","proxy_url":"socks5h://127.0.0.1:1080","compatibility_mode":true}})).unwrap();
        assert_eq!(settings["language"], "en");
        for proxy in [
            "http://user:secret@127.0.0.1:8888",
            "http://127.0.0.1:8888/path",
            "file:///etc/passwd",
        ] {
            assert!(app
                .settings_update(&json!({"experimental_network":{"proxy_url":proxy}}))
                .is_err());
        }
        drop(app);
        let app = AppService::mock(&path, Scenario::Free);
        assert_eq!(
            app.settings_get().unwrap()["experimental_network"]["proxy_url"],
            "socks5h://127.0.0.1:1080"
        );
        assert_eq!(app.settings_get().unwrap()["close_to_tray"], true);
        app.settings_update(&json!({"experimental_network":{"enabled":false}}))
            .unwrap();
        assert_eq!(
            app.settings_get().unwrap()["experimental_network"]["enabled"],
            false
        );
    }
}
