use nekopak_core::AppService;
use nekopak_provider_mock::Scenario;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

async fn request(url: &str, range: &str, origin: Option<&str>) -> Vec<u8> {
    let url = url::Url::parse(url).unwrap();
    let mut socket = tokio::net::TcpStream::connect(("127.0.0.1", url.port().unwrap()))
        .await
        .unwrap();
    let request = format!(
        "GET {} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nRange: {range}\r\n{}Connection: close\r\n\r\n",
        url.path(),
        url.port().unwrap(),
        origin
            .map(|o| format!("Origin: {o}\r\n"))
            .unwrap_or_default()
    );
    socket.write_all(request.as_bytes()).await.unwrap();
    let mut out = Vec::new();
    socket.read_to_end(&mut out).await.unwrap();
    out
}
#[tokio::test]
async fn readonly_local_mapping_and_streams_preserve_files_and_reject_external_origins() {
    let temp = tempfile::tempdir().unwrap();
    let local = temp.path().join("local");
    std::fs::create_dir(&local).unwrap();
    std::fs::write(local.join("video.mp4"), b"0123456789").unwrap();
    let app = Arc::new(AppService::mock(
        &temp.path().join("tasks.db"),
        Scenario::Free,
    ));
    let source = app.local_sources_add(local.to_str().unwrap()).unwrap();
    let page = app.local_list(&source.root_id, None, None).unwrap();
    assert_eq!(page.items.len(), 1);
    let id = &page.items[0].id;
    assert_eq!(app.local_path(id).unwrap().len(), 2);
    let stream = app.media_session_open(id, None).await.unwrap();
    let url = stream["url"].as_str().unwrap();
    let out = request(url, "bytes=2-5", Some("http://tauri.localhost")).await;
    assert!(out.starts_with(b"HTTP/1.1 206"));
    assert!(out.ends_with(b"2345"));
    let out = request(url, "bytes=0-2", Some("https://attacker.example")).await;
    assert!(out.starts_with(b"HTTP/1.1 403"));
    let out = request(url, "bytes=99-", None).await;
    assert!(out.starts_with(b"HTTP/1.1 416"));
    let snapshot = std::fs::read(local.join("video.mp4")).unwrap();
    app.local_sources_remove(&source.source_id).unwrap();
    assert!(app.local_list(&source.root_id, None, None).is_err());
    assert_eq!(std::fs::read(local.join("video.mp4")).unwrap(), snapshot);
}
#[test]
fn local_ids_reject_traversal_and_corrupt_mappings_are_not_replaced() {
    use base64::Engine;
    let temp = tempfile::tempdir().unwrap();
    let app = AppService::mock(&temp.path().join("tasks.db"), Scenario::Free);
    let source = app
        .local_sources_add(temp.path().to_str().unwrap())
        .unwrap();
    let traversal = format!(
        "local:{}:{}",
        source.source_id,
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode("../outside")
    );
    assert!(app.local_stat(&traversal).is_err());
}
