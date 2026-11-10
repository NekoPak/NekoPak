//! Independent fixed-domain routing and unauthenticated gateway measurements.
//! Protocol evidence: docs/NETWORK_RECOVERY_RESEARCH.md. CDN links are untouched.
use super::*;
use std::sync::atomic::Ordering;
use std::time::Duration;

pub(crate) const DOMAINS: [&str; 4] = [
    "mypikpak.com",
    "mypikpak.net",
    "pikpak.me",
    "pikpakdrive.com",
];
#[derive(Clone)]
pub(crate) struct DomainRouting {
    pub selected: String,
    pub active: String,
}
impl Default for DomainRouting {
    fn default() -> Self {
        Self {
            selected: "auto".into(),
            active: DOMAINS[0].into(),
        }
    }
}

impl HttpApi {
    pub fn set_server_domain(&self, selected: &str) -> Result<(), ClientError> {
        if selected != "auto" && !DOMAINS.contains(&selected) {
            return Err(error(ErrorCode::Conflict, "服务器域名不在支持列表中。"));
        }
        let mut routing = self.routing.write().map_err(|_| wire::invalid())?;
        if routing.selected != selected {
            routing.selected = selected.into();
            if selected != "auto" {
                routing.active = selected.into();
            }
            self.network_generation.fetch_add(1, Ordering::AcqRel);
        }
        Ok(())
    }
    pub fn route_api_url(&self, value: &str) -> Result<String, ClientError> {
        let mut url = url::Url::parse(value).map_err(|_| wire::invalid())?;
        let canonical = match url.host_str() {
            Some("user.mypikpak.com") => Some("user"),
            Some("api-drive.mypikpak.com") => Some("api-drive"),
            _ => None,
        };
        if let Some(prefix) = canonical {
            if url.scheme() != "https"
                || url.port_or_known_default() != Some(443)
                || !url.username().is_empty()
                || url.password().is_some()
            {
                return Err(wire::invalid());
            }
            let routing = self.routing.read().map_err(|_| wire::invalid())?;
            url.set_host(Some(&format!("{prefix}.{}", routing.active)))
                .map_err(|_| wire::invalid())?;
        }
        Ok(url.into())
    }
    pub fn is_auth_url(&self, value: &str) -> Result<bool, ClientError> {
        let url = url::Url::parse(value).map_err(|_| wire::invalid())?;
        if url.path().starts_with("/v1/") {
            if value.starts_with(&format!("{}/v1/", self.auth_origin)) {
                return Ok(true);
            }
            return Ok(DOMAINS
                .iter()
                .any(|root| url.host_str() == Some(format!("user.{root}").as_str())));
        }
        Ok(false)
    }
}

pub(super) async fn gateway_check(
    client: &reqwest::Client,
    endpoint: &str,
) -> Result<u64, &'static str> {
    let started = tokio::time::Instant::now();
    let result = tokio::time::timeout(Duration::from_secs(8), async {
        let mut response = client
            .get(endpoint)
            .send()
            .await
            .map_err(|_| "网络连接失败")?;
        if response.status().as_u16() != 401 {
            return Err("服务未返回预期的未登录响应");
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| "响应读取失败")? {
            if bytes.len() + chunk.len() > 4096 {
                return Err("服务响应超出探测限制");
            }
            bytes.extend_from_slice(&chunk);
        }
        let value: Value = serde_json::from_slice(&bytes).map_err(|_| "服务响应不是有效 JSON")?;
        if value["error_code"].as_i64() != Some(16) {
            return Err("服务身份校验未通过");
        }
        Ok(())
    })
    .await;
    match result {
        Ok(Ok(())) => Ok(started.elapsed().as_millis().min(u64::MAX as u128) as u64),
        Ok(Err(reason)) => Err(reason),
        Err(_) => Err("连接超时"),
    }
}

async fn probe(client: Result<reqwest::Client, ClientError>, domain: &'static str) -> Value {
    let mut result = json!({"domain":domain,"usable":false});
    let Ok(client) = client else {
        result["error"] = "无法初始化安全探测连接".into();
        return result;
    };
    let drive = format!("https://api-drive.{domain}/drive/v1/about");
    let user = format!("https://user.{domain}/v1/user/me");
    for (url, field) in [
        (&drive, "latency_ms"),
        (&drive, "warm_latency_ms"),
        (&user, "auth_latency_ms"),
    ] {
        match gateway_check(&client, url).await {
            Ok(elapsed) => result[field] = elapsed.into(),
            Err(reason) => {
                result["error"] = reason.into();
                return result;
            }
        }
    }
    result["usable"] = true.into();
    result
}

impl NativeProvider {
    pub(super) async fn probe_server_domains(&self) -> Result<Value, ClientError> {
        let selected = self
            .http
            .routing
            .read()
            .map_err(|_| wire::invalid())?
            .selected
            .clone();
        let generation = self.http.network_generation.load(Ordering::Acquire);
        let (a, b, c, d) = tokio::join!(
            probe(self.http.fresh_probe_client(), DOMAINS[0]),
            probe(self.http.fresh_probe_client(), DOMAINS[1]),
            probe(self.http.fresh_probe_client(), DOMAINS[2]),
            probe(self.http.fresh_probe_client(), DOMAINS[3])
        );
        let results = vec![a, b, c, d];
        let fastest = results
            .iter()
            .filter(|r| r["usable"] == true)
            .min_by_key(|r| r["latency_ms"].as_u64().unwrap_or(u64::MAX))
            .and_then(|r| r["domain"].as_str());
        let mut routing = self.http.routing.write().map_err(|_| wire::invalid())?;
        if selected == "auto"
            && routing.selected == selected
            && self.http.network_generation.load(Ordering::Acquire) == generation
        {
            if let Some(domain) = fastest {
                if routing.active != domain {
                    routing.active = domain.into();
                    self.http.network_generation.fetch_add(1, Ordering::AcqRel);
                }
            }
        }
        Ok(
            json!({"selected_domain":routing.selected,"active_domain":routing.active,"checked_at":timestamp(now()),"results":results}),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn domain_routing_changes_only_allowed_api_hosts_and_never_a_signed_cdn_link() {
        let http = HttpApi::production(profile::WEB_CLIENT_ID).unwrap();
        for domain in DOMAINS {
            http.set_server_domain(domain).unwrap();
            assert_eq!(
                http.route_api_url("https://api-drive.mypikpak.com/drive/v1/files?a=1")
                    .unwrap(),
                format!("https://api-drive.{domain}/drive/v1/files?a=1")
            );
            assert!(http
                .is_auth_url(&format!("https://user.{domain}/v1/auth/signin"))
                .unwrap());
            for untouched in [
                "https://dl-001.mypikpak.com/secret?token=synthetic",
                "https://user.mypikpak.com.example/v1/auth/signin",
                "https://mypikpak.com/share",
            ] {
                assert_eq!(http.route_api_url(untouched).unwrap(), untouched);
            }
        }
        for invalid in [
            "evil.test",
            "https://mypikpak.net",
            "mypikpak.com:443",
            "*.mypikpak.com",
            "",
        ] {
            assert!(http.set_server_domain(invalid).is_err());
        }
        assert!(!http
            .is_auth_url("https://user.mypikpak.com.example/v1/anything")
            .unwrap());
    }
}
