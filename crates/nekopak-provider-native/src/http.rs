use crate::{error, profile, wire};
use nekopak_domain::{ClientError, ErrorCode};
use reqwest::{header, Client, Method};
use serde::Serialize;
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        RwLock,
    },
    time::Duration,
};
use tokio::{sync::Mutex, time::Instant};
use zeroize::Zeroizing;

pub(crate) const AUTH_ORIGIN: &str = "https://user.mypikpak.com";
pub(crate) const DRIVE_ORIGIN: &str = "https://api-drive.mypikpak.com";
const MAX_RESPONSE: usize = 4 * 1024 * 1024;

pub(crate) struct AuthHeaders<'a> {
    pub device_id: &'a str,
    pub bearer: Option<&'a str>,
    pub captcha_token: Option<&'a str>,
}
impl<'a> AuthHeaders<'a> {
    pub fn device(device_id: &'a str) -> Self {
        Self {
            device_id,
            bearer: None,
            captcha_token: None,
        }
    }
}

pub(crate) struct HttpApi {
    client: RwLock<Client>,
    client_id: String,
    auth_headers: header::HeaderMap,
    network: RwLock<(String, Option<String>, bool)>,
    block_access_domains: AtomicBool,
    pub auth_origin: String,
    pub drive_origin: String,
    next_request: Mutex<Instant>,
    interval: Duration,
}

impl HttpApi {
    pub fn production(client_id: &str) -> Result<Self, ClientError> {
        Self::build(
            AUTH_ORIGIN,
            DRIVE_ORIGIN,
            client_id,
            true,
            Duration::from_millis(200),
            "system",
            None,
            false,
        )
    }
    #[allow(clippy::too_many_arguments)]
    fn build(
        auth: &str,
        drive: &str,
        client_id: &str,
        https: bool,
        interval: Duration,
        proxy_mode: &str,
        proxy_url: Option<&str>,
        compatibility: bool,
    ) -> Result<Self, ClientError> {
        let mut headers = header::HeaderMap::new();
        headers.insert(
            "x-client-id",
            header::HeaderValue::from_str(client_id).map_err(|_| wire::invalid())?,
        );
        headers.insert(
            "x-client-version",
            header::HeaderValue::from_static(profile::LOGIN_CLIENT_VERSION),
        );
        headers.insert(
            "x-sdk-version",
            header::HeaderValue::from_static(profile::SDK_VERSION),
        );
        headers.insert(
            "x-protocol-version",
            header::HeaderValue::from_static(profile::PROTOCOL_VERSION),
        );
        headers.insert(
            header::ACCEPT_LANGUAGE,
            header::HeaderValue::from_static("zh-CN"),
        );
        let builder = Client::builder()
            .https_only(https)
            // Compatibility profile observed to work in public handshake probes.
            // Network resets were also observed; this is not a service guarantee.
            // Certificate/hostname checks remain enabled; HTTP/1.1 needs no ALPN.
            // Reproduction: docs/evidence/login-tls-compat.json.
            .http1_only()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(8))
            .timeout(Duration::from_secs(12))
            .user_agent(concat!("NekoPak/", env!("CARGO_PKG_VERSION")));
        #[cfg(not(windows))]
        let builder = if compatibility {
            builder
                .min_tls_version(reqwest::tls::Version::TLS_1_2)
                .max_tls_version(reqwest::tls::Version::TLS_1_2)
        } else {
            builder.min_tls_version(reqwest::tls::Version::TLS_1_2)
        };
        #[cfg(windows)]
        let builder = {
            // A preconfigured connector prevents reqwest from re-adding ALPN.
            // Native TLS defaults retain system trust, SNI and hostname validation.
            let mut tls = native_tls::TlsConnector::builder();
            tls.min_protocol_version(Some(native_tls::Protocol::Tlsv12));
            if compatibility {
                tls.max_protocol_version(Some(native_tls::Protocol::Tlsv12));
            }
            builder.use_preconfigured_tls(
                tls.build()
                    .map_err(|_| error(ErrorCode::Internal, "无法初始化系统安全连接。"))?,
            )
        };
        let client = apply_proxy(builder, proxy_mode, proxy_url)?
            .build()
            .map_err(|_| error(ErrorCode::Internal, "无法初始化安全网络连接。"))?;
        Ok(Self {
            client: RwLock::new(client),
            client_id: client_id.into(),
            auth_headers: headers,
            network: RwLock::new((
                proxy_mode.into(),
                proxy_url.map(str::to_owned),
                compatibility,
            )),
            block_access_domains: AtomicBool::new(false),
            auth_origin: auth.into(),
            drive_origin: drive.into(),
            next_request: Mutex::new(Instant::now()),
            interval,
        })
    }
    #[cfg(any(test, debug_assertions))]
    pub fn loopback(origin: &str) -> Self {
        let url = url::Url::parse(origin).unwrap();
        assert_eq!(url.host_str(), Some("127.0.0.1"));
        Self::build(
            origin,
            origin,
            profile::WEB_CLIENT_ID,
            false,
            Duration::ZERO,
            "none",
            None,
            false,
        )
        .unwrap()
    }
    async fn throttle(&self) {
        let mut next = self.next_request.lock().await;
        tokio::time::sleep_until(*next).await;
        *next = Instant::now() + self.interval;
    }
    pub fn set_network(
        &self,
        mode: &str,
        proxy: Option<&str>,
        compatibility: bool,
    ) -> Result<(), ClientError> {
        let desired = (mode.to_owned(), proxy.map(str::to_owned), compatibility);
        if *self.network.read().map_err(|_| wire::invalid())? == desired {
            return Ok(());
        }
        let fresh = Self::build(
            &self.auth_origin,
            &self.drive_origin,
            &self.client_id,
            self.auth_origin.starts_with("https://"),
            self.interval,
            mode,
            proxy,
            compatibility,
        )?;
        *self.client.write().map_err(|_| wire::invalid())? =
            fresh.client.into_inner().map_err(|_| wire::invalid())?;
        *self.network.write().map_err(|_| wire::invalid())? = desired;
        Ok(())
    }
    pub fn cdn_proxy(
        &self,
        builder: reqwest::ClientBuilder,
    ) -> Result<reqwest::ClientBuilder, ClientError> {
        let network = self.network.read().map_err(|_| wire::invalid())?;
        apply_proxy(builder, &network.0, network.1.as_deref())
    }
    pub fn set_domain_blocking(&self, enabled: bool) {
        self.block_access_domains.store(enabled, Ordering::Release);
    }
    pub fn check_url(&self, url: &str) -> Result<(), ClientError> {
        let parsed = url::Url::parse(url).map_err(|_| wire::invalid())?;
        let host = parsed.host_str().unwrap_or("").trim_end_matches('.');
        if self.block_access_domains.load(Ordering::Acquire)
            && [
                "access.mypikpak.com",
                "access.mypikpak.net",
                "access.pikpak.me",
                "access.pikpakdrive.com",
            ]
            .iter()
            .any(|blocked| host.eq_ignore_ascii_case(blocked))
        {
            return Err(error(
                ErrorCode::PermissionDenied,
                "此连接已被 NekoPak 的 access 域名屏蔽规则阻止。可在网络设置中关闭规则。",
            ));
        }
        Ok(())
    }
    pub async fn request<T: Serialize + ?Sized>(
        &self,
        method: Method,
        url: &str,
        auth: AuthHeaders<'_>,
        body: Option<&T>,
        signing_in: bool,
    ) -> Result<Zeroizing<Vec<u8>>, ClientError> {
        self.check_url(url)?;
        self.throttle().await;
        let client = self.client.read().map_err(|_| wire::invalid())?.clone();
        let mut request = client
            .request(method, url)
            .header("x-device-id", auth.device_id);
        if url.starts_with(&format!("{}/v1/", self.auth_origin)) {
            request = request
                .headers(self.auth_headers.clone())
                // Locally generated Web device identifier, not a borrowed device signature.
                .header(
                    "x-device-sign",
                    format!("wdi10.{}{}", auth.device_id, "x".repeat(32)),
                );
        }
        if let Some(token) = auth.bearer {
            let mut header = header::HeaderValue::from_str(&format!("Bearer {token}"))
                .map_err(|_| wire::invalid())?;
            header.set_sensitive(true);
            request = request.header(header::AUTHORIZATION, header);
        }
        if let Some(token) = auth.captcha_token {
            let mut header = header::HeaderValue::from_str(token).map_err(|_| wire::invalid())?;
            header.set_sensitive(true);
            request = request.header("x-captcha-token", header);
        }
        if let Some(body) = body {
            request = request.json(body);
        }
        // Never include reqwest's error text: it can contain URLs and signed queries.
        // Retry only connector failures: no HTTP request bytes can have been sent.
        // Received HTTP failures or interrupted request/response bodies are never replayed.
        let template = request.build().map_err(|_| wire::invalid())?;
        let deadline = Instant::now() + Duration::from_secs(12);
        let mut attempts = 0_u32;
        let mut response = loop {
            attempts += 1;
            let request = template.try_clone().ok_or_else(wire::invalid)?;
            match tokio::time::timeout_at(deadline, client.execute(request)).await {
                Ok(Ok(response)) => break response,
                Ok(Err(failure))
                    if failure.is_connect() && attempts < 4 && Instant::now() < deadline =>
                {
                    let delay = Duration::from_millis(200 * (1 << (attempts - 1)));
                    if tokio::time::timeout_at(deadline, tokio::time::sleep(delay))
                        .await
                        .is_err()
                    {
                        return Err(network_detail(url, attempts));
                    }
                    self.throttle().await;
                }
                _ => return Err(network_detail(url, attempts)),
            }
        };
        let status = response.status().as_u16();
        let retry_after = response
            .headers()
            .get(header::RETRY_AFTER)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u32>().ok());
        if response
            .content_length()
            .is_some_and(|size| size > MAX_RESPONSE as u64)
        {
            return Err(wire::invalid());
        }
        let mut bytes = Zeroizing::new(Vec::new());
        while let Some(chunk) = response.chunk().await.map_err(|_| network())? {
            if bytes.len() + chunk.len() > MAX_RESPONSE {
                return Err(wire::invalid());
            }
            bytes.extend_from_slice(&chunk);
        }
        wire::check_response(status, &bytes, retry_after, signing_in)?;
        Ok(bytes)
    }
}

fn apply_proxy(
    builder: reqwest::ClientBuilder,
    mode: &str,
    proxy: Option<&str>,
) -> Result<reqwest::ClientBuilder, ClientError> {
    match mode {
        "system" => Ok(builder),
        "none" => Ok(builder.no_proxy()),
        "user_provided" => Ok(builder.no_proxy().proxy(
            reqwest::Proxy::all(proxy.ok_or_else(wire::invalid)?)
                .map_err(|_| error(ErrorCode::Conflict, "代理地址无效。"))?,
        )),
        _ => Err(wire::invalid()),
    }
}

fn network() -> ClientError {
    error(
        ErrorCode::TransientNetwork,
        "无法连接 PikPak，请检查网络后重试。",
    )
}

fn network_detail(url: &str, attempts: u32) -> ClientError {
    let mut failure = network();
    let stage = if url.ends_with("/captcha/init") {
        "captcha_init"
    } else if url.ends_with("/auth/signin") {
        "sign_in"
    } else if url.ends_with("/auth/token") {
        "token_refresh"
    } else {
        "metadata"
    };
    failure.details = Some(
        [
            ("stage".into(), stage.into()),
            ("connect_attempts".into(), attempts.into()),
        ]
        .into(),
    );
    failure
}

#[cfg(test)]
mod block_tests {
    use super::*;
    #[tokio::test]
    async fn access_rules_block_before_network_and_toggle_without_affecting_similar_hosts() {
        let api = HttpApi::loopback("http://127.0.0.1:9");
        let domains = [
            "access.mypikpak.com",
            "access.mypikpak.net",
            "access.pikpak.me",
            "access.pikpakdrive.com",
        ];
        for host in domains {
            assert!(api.check_url(&format!("https://{host}/area")).is_ok());
        }
        api.set_domain_blocking(true);
        for host in domains {
            for url in [
                format!("https://{host}/area"),
                format!("https://{}./area", host.to_uppercase()),
            ] {
                let error = api
                    .request::<Value>(
                        Method::GET,
                        &url,
                        AuthHeaders::device("synthetic-device"),
                        None,
                        false,
                    )
                    .await
                    .unwrap_err();
                assert_eq!(error.code, ErrorCode::PermissionDenied);
            }
        }
        for host in [
            "api-drive.mypikpak.com",
            "user.mypikpak.com",
            "access.mypikpak.com.example",
            "other.access.mypikpak.com",
        ] {
            assert!(api.check_url(&format!("https://{host}/")).is_ok());
        }
        api.set_domain_blocking(false);
        assert!(api.check_url("https://access.mypikpak.com/").is_ok());
    }
    use serde_json::Value;
}
