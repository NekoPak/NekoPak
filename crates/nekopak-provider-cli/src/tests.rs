use super::*;
use process::Output;
use serde_json::{json, Value};
use std::{collections::VecDeque, sync::Mutex};

struct ScriptedRunner {
    replies: Mutex<VecDeque<(Vec<String>, Output)>>,
}
#[async_trait]
impl CliRunner for ScriptedRunner {
    async fn run(&self, arguments: &[String]) -> Result<Output, ClientError> {
        let (expected, reply) = self
            .replies
            .lock()
            .unwrap()
            .pop_front()
            .expect("unexpected extra command or retry");
        assert_eq!(arguments, expected);
        Ok(reply)
    }
}
fn output(value: Value) -> Output {
    Output {
        success: true,
        stdout: serde_json::to_vec(&value).unwrap(),
        stderr: Vec::new(),
    }
}
fn configured(replies: Vec<(Vec<String>, Output)>) -> (OfficialCliProvider, Arc<ScriptedRunner>) {
    let runner = Arc::new(ScriptedRunner {
        replies: Mutex::new(replies.into()),
    });
    let provider = OfficialCliProvider {
        runner: Some(runner.clone()),
        preview_enabled: true,
        probe_cache: tokio::sync::Mutex::new(Some(CliProbeReport {
            backend: BackendId::OfficialCli,
            status: BackendStatus::Unavailable,
            version: Some(REVIEWED_VERSION.into()),
            binary_sha256: Some(WINDOWS_X64_SHA256.into()),
            verified_commands: vec![],
            live_account_verified: false,
            limitations: vec![],
            checked_at: now(),
        })),
    };
    (provider, runner)
}

fn raw_entry() -> Value {
    json!({"id":"file_1", "parent_id":"", "kind":"drive#file", "name":"中文 📁 movie.bin", "size":"18446744073709551615", "modified_time":"2026-11-07T08:30:00+08:00", "mime_type":"application/octet-stream", "web_content_link":"https://example.invalid/?token=SECRET", "params":{"url":"https://secret.invalid/"}, "medias":[{"link":{"url":"secret"}}], "hash":"untyped-value"})
}

#[test]
fn raw_mapping_preserves_bytes_normalizes_utc_and_removes_urls() {
    let item = wire::entry(&raw_entry()).unwrap();
    assert_eq!(item.size_bytes.0, u64::MAX);
    assert_eq!(item.parent_id.as_deref(), Some("root"));
    assert_eq!(item.modified_at.as_deref(), Some("2026-11-07T00:30:00Z"));
    let json = serde_json::to_string(&item).unwrap();
    assert!(!json.contains("SECRET") && !json.contains("secret.invalid") && item.hash.is_none());
}

#[test]
fn display_sizes_partial_pages_and_malformed_times_are_rejected() {
    for size in [
        json!("1.2 GB"),
        json!("-"),
        json!(9007199254740992u64),
        json!("18446744073709551616"),
    ] {
        let mut entry = raw_entry();
        entry["size"] = size;
        assert!(wire::entry(&entry).is_err());
    }
    let mut entry = raw_entry();
    entry["modified_time"] = json!("2026-11-07 12:00");
    assert!(wire::entry(&entry).is_err());
    assert!(wire::list(json!({}), "root", None).is_err());
    assert!(wire::list(json!({"files":[raw_entry(),raw_entry()]}), "root", None).is_err());
    assert!(wire::list(
        json!({"files":[raw_entry()],"next_page_token":"same"}),
        "root",
        Some("same")
    )
    .is_err());
    assert!(wire::list(json!({"files":[],"next_page_token":"next"}), "root", None).is_err());
}

#[test]
fn every_error_class_is_redacted_including_unrecognized_diagnostics() {
    for (upstream, expected) in [
        ("UNAUTHENTICATED", ErrorCode::AuthExpired),
        ("ACCESS_DENIED", ErrorCode::PermissionDenied),
        ("CONNECTED_APPS_QUOTA_EXCEEDED", ErrorCode::QuotaExceeded),
        ("FILE_NOT_FOUND", ErrorCode::NotFound),
        ("NETWORK_ERROR", ErrorCode::TransientNetwork),
        ("SERVICE_UNAVAILABLE", ErrorCode::Unsupported),
        ("ALIEN_SECRET", ErrorCode::Internal),
    ] {
        let bytes = format!("2026 warning private IP/token\n{}",json!({"error":{"code":upstream,"message":"SECRET","reason":"https://example.com/?token=SECRET","request_id":"PRIVATE"}})).into_bytes();
        let error = wire::parse_output(Output {
            success: false,
            stdout: Vec::new(),
            stderr: bytes,
        })
        .unwrap_err();
        assert_eq!(error.code, expected);
        let serialized = serde_json::to_string(&error).unwrap();
        assert!(
            !serialized.contains("SECRET")
                && !serialized.contains("PRIVATE")
                && !serialized.contains("http")
        );
    }
}

#[tokio::test]
async fn arguments_are_fixed_and_cursor_cannot_become_option() {
    let expected = args(&[
        "ls",
        "--raw",
        "-F",
        "json",
        "--limit=10",
        "--token=--verbose; & malicious",
    ]);
    let (provider, runner) = configured(vec![(expected, output(json!({"files":[raw_entry()]})))]);
    assert!(provider
        .list("root", Some("--verbose; & malicious"), 10)
        .await
        .is_ok());
    assert!(runner.replies.lock().unwrap().is_empty());
    assert!(provider.stat("--help").await.is_err());
    assert!(provider.list("root", None, 1).await.is_err());
}

#[tokio::test]
async fn unauthorized_discovery_never_claims_live_capability() {
    let (provider, _) = configured(vec![(
        args(&["auth", "status", "-F", "json"]),
        output(json!({"logged_in":false})),
    )]);
    let report = provider.discover().await.unwrap();
    assert_eq!(report.status, BackendStatus::NeedsLogin);
    assert!(!report.supports(Operation::List));
    assert!(provider.quota().await.is_err());
    assert!(provider.read_range("file_1", 0, 1).await.is_err());
}

#[tokio::test]
async fn authorized_preview_only_advertises_read_metadata() {
    let (provider, _) = configured(vec![(
        args(&["auth", "status", "-F", "json"]),
        output(
            json!({"logged_in":true,"email":"private@example.com","access_token":"SECRET","vip":{"type":"premium"}}),
        ),
    )]);
    let report = provider.discover().await.unwrap();
    assert!(report.supports(Operation::List) && report.supports(Operation::Stat));
    assert!(!report.supports(Operation::Download) && !report.supports(Operation::ReadRange));
    assert_eq!(report.account_tier, Some(AccountTier::Unknown));
}

#[tokio::test]
async fn version_drift_fails_before_any_account_request() {
    let (provider, _) = configured(vec![(
        args(&["version"]),
        Output {
            success: true,
            stdout: b"pikpak version v0.7.0\n".to_vec(),
            stderr: vec![],
        },
    )]);
    *provider.probe_cache.lock().await = None;
    assert_eq!(
        provider.probe().await.unwrap_err().code,
        ErrorCode::Unsupported
    );
}

#[tokio::test]
async fn no_configuration_executes_nothing_and_opt_in_is_required() {
    let provider = OfficialCliProvider::not_configured();
    assert_eq!(
        provider.discover().await.unwrap().status,
        BackendStatus::NotInstalled
    );
    assert!(provider.auth_status().await.is_err());
    let (mut provider, runner) = configured(vec![]);
    provider.preview_enabled = false;
    assert!(provider.list("root", None, 10).await.is_err());
    assert!(runner.replies.lock().unwrap().is_empty());
}

#[test]
fn auth_error_observed_from_real_binary_maps_to_contract() {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../docs/evidence/official-cli-v0.6.0-probe.json"
    ))
    .unwrap();
    let observed = fixture["commands"]
        .as_array()
        .unwrap()
        .iter()
        .find(|record| record["args"] == json!(["ls", "--raw", "-F", "json", "-l", "10"]))
        .unwrap();
    let result = wire::parse_output(Output {
        success: false,
        stdout: vec![],
        stderr: observed["stderr"].as_str().unwrap().as_bytes().to_vec(),
    });
    assert_eq!(result.unwrap_err().code, ErrorCode::AuthExpired);
}
