//! Temporary loopback receiver for the user's completed human verification.
//! No password, access token or refresh token is accepted here; no automatic solving.
use crate::{error, valid_token, wire};
use nekopak_domain::{ClientError, ErrorCode};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    task::JoinHandle,
};
use url::Url;
use zeroize::{Zeroize, Zeroizing};

pub(crate) struct VerifiedCaptcha {
    pub token: Zeroizing<String>,
    pub expires_at: tokio::time::Instant,
}

pub(crate) struct HumanCaptcha {
    pub redirect_uri: String,
    pub state: String,
    result: Arc<Mutex<Option<VerifiedCaptcha>>>,
    task: JoinHandle<()>,
}
impl Drop for HumanCaptcha {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl HumanCaptcha {
    pub async fn listen() -> Result<Self, ClientError> {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .map_err(|_| {
                error(
                    ErrorCode::PermissionDenied,
                    "无法建立本机安全验证回调，请检查本机网络权限后重试。",
                )
            })?;
        let address = listener.local_addr().map_err(|_| wire::invalid())?;
        let path = format!("/captcha/{}", uuid::Uuid::new_v4().simple());
        let redirect_uri = format!("http://{address}{path}");
        let state = uuid::Uuid::new_v4().simple().to_string();
        let result = Arc::new(Mutex::new(None));
        let output = result.clone();
        let expected_state = state.clone();
        let authority = address.to_string();
        let task = tokio::spawn(async move {
            let _ = tokio::time::timeout(Duration::from_secs(300), async {
                loop {
                    let Ok((mut stream, peer)) = listener.accept().await else {
                        break;
                    };
                    if !peer.ip().is_loopback() {
                        continue;
                    }
                    let received = tokio::time::timeout(
                        Duration::from_secs(3),
                        receive(&mut stream, &path, &authority, &expected_state),
                    )
                    .await;
                    match received {
                        Ok(Ok(verified)) => {
                            if let Ok(mut slot) = output.lock() {
                                *slot = Some(verified);
                            }
                            respond(&mut stream, true).await;
                            break;
                        }
                        _ => respond(&mut stream, false).await,
                    }
                }
            })
            .await;
        });
        Ok(Self {
            redirect_uri,
            state,
            result,
            task,
        })
    }

    pub fn take_verified(&self) -> Result<Option<VerifiedCaptcha>, ClientError> {
        let mut slot = self.result.lock().map_err(|_| wire::invalid())?;
        let token = slot.take();
        if token
            .as_ref()
            .is_some_and(|v| v.expires_at <= tokio::time::Instant::now())
        {
            return Err(error(
                ErrorCode::Conflict,
                "安全验证结果已过期，请重新登录。",
            ));
        }
        Ok(token)
    }

    pub fn challenge_url(&self, base: &str) -> Result<String, ClientError> {
        let mut url = Url::parse(base).map_err(|_| wire::invalid())?;
        let retained = url
            .query_pairs()
            .filter(|(k, _)| k != "redirect_uri" && k != "state")
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect::<Vec<_>>();
        url.set_query(None);
        url.query_pairs_mut()
            .extend_pairs(retained)
            .append_pair("redirect_uri", &self.redirect_uri)
            .append_pair("state", &self.state);
        Ok(url.into())
    }
}

async fn receive(
    stream: &mut TcpStream,
    path: &str,
    authority: &str,
    expected_state: &str,
) -> Result<VerifiedCaptcha, ClientError> {
    let mut bytes = Zeroizing::new(Vec::new());
    let end = loop {
        let mut buffer = Zeroizing::new([0_u8; 1024]);
        let size = stream
            .read(&mut *buffer)
            .await
            .map_err(|_| wire::invalid())?;
        if size == 0 || bytes.len() + size > 16384 {
            return Err(wire::invalid());
        }
        bytes.extend_from_slice(&buffer[..size]);
        if let Some(at) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
            break at;
        }
    };
    let headers = std::str::from_utf8(&bytes[..end]).map_err(|_| wire::invalid())?;
    let mut lines = headers.split("\r\n");
    let mut first = lines.next().ok_or_else(wire::invalid)?.split(' ');
    if first.next() != Some("GET") {
        return Err(wire::invalid());
    }
    let target = first.next().ok_or_else(wire::invalid)?;
    if !target.starts_with('/') || target.starts_with("//") || first.next() != Some("HTTP/1.1") {
        return Err(wire::invalid());
    }
    let hosts: Vec<_> = lines
        .filter_map(|line| line.split_once(':'))
        .filter(|(name, _)| name.eq_ignore_ascii_case("host"))
        .collect();
    if hosts.len() != 1 || hosts[0].1.trim() != authority {
        return Err(wire::invalid());
    }
    let mut url =
        Url::parse(&format!("http://{authority}{target}")).map_err(|_| wire::invalid())?;
    if url.path() != path || url.fragment().is_some() {
        return Err(wire::invalid());
    }
    let mut state = None;
    let mut token = None;
    let mut expiry = None;
    for (name, value) in url.query_pairs() {
        match name.as_ref() {
            "state" if state.is_none() => state = Some(value.into_owned()),
            "captcha_token" if token.is_none() => token = Some(Zeroizing::new(value.into_owned())),
            "expires_in" if expiry.is_none() => {
                expiry = Some(value.parse::<u32>().map_err(|_| wire::invalid())?)
            }
            _ => return Err(wire::invalid()),
        }
    }
    // Remove the parsed URL's query allocation promptly; it is never logged or persisted.
    url.set_query(None);
    if state.as_deref() != Some(expected_state) {
        return Err(wire::invalid());
    }
    if let Some(state) = state.as_mut() {
        state.zeroize();
    }
    let token = token
        .filter(|value| valid_token(value))
        .ok_or_else(wire::invalid)?;
    let seconds = expiry
        .filter(|seconds| *seconds > 0)
        .ok_or_else(wire::invalid)?
        .min(300);
    Ok(VerifiedCaptcha {
        token,
        expires_at: tokio::time::Instant::now() + Duration::from_secs(seconds as u64),
    })
}

async fn respond(stream: &mut TcpStream, success: bool) {
    let body = if success {
        "<!doctype html><meta charset=utf-8><title>NekoPak 安全验证</title><p>验证结果已收到。请返回 NekoPak 继续操作。</p>"
    } else {
        "<!doctype html><meta charset=utf-8><title>NekoPak 安全验证</title><p>这不是有效的验证回调，请返回验证页面重试。</p>"
    };
    let response = format!("HTTP/1.1 {}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nReferrer-Policy: no-referrer\r\nContent-Security-Policy: default-src 'none'\r\nConnection: close\r\n\r\n{}", if success { "200 OK" } else { "400 Bad Request" }, body.len(), body);
    let _ = tokio::time::timeout(
        Duration::from_secs(2),
        stream.write_all(response.as_bytes()),
    )
    .await;
}
