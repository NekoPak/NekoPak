//! Bounded .torrent import. Hash the original info bytes; never re-encode them.
//! BEP3 wire semantics, implemented independently. No torrent peer networking.
use super::*;
use serde::Serialize;
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

pub const MAX_TORRENT_BYTES: usize = 10 * 1024 * 1024;

#[derive(Clone, Serialize)]
pub struct TorrentFile {
    pub path: String,
    pub size_bytes: DecimalU64,
}
#[derive(Clone, Serialize)]
pub struct TorrentPreview {
    pub import_id: String,
    pub title: String,
    pub info_hash: String,
    pub total_bytes: DecimalU64,
    pub file_count: usize,
    pub files: Vec<TorrentFile>,
    pub private: bool,
    pub expires_at: String,
}
pub(crate) struct PendingTorrent {
    scope: String,
    magnet: String,
    created: Instant,
}

fn invalid() -> ClientError {
    ClientError::new(
        ErrorCode::Conflict,
        "种子文件无效、损坏或超出限制，未提交云下载。",
    )
}
enum Value<'a> {
    Bytes(&'a [u8]),
    Integer(u64),
    List(Vec<Node<'a>>),
    Dict(BTreeMap<&'a [u8], Node<'a>>),
}
struct Node<'a> {
    value: Value<'a>,
    start: usize,
    end: usize,
}
impl<'a> Node<'a> {
    fn bytes(&self) -> Result<&'a [u8], ClientError> {
        if let Value::Bytes(value) = self.value {
            Ok(value)
        } else {
            Err(invalid())
        }
    }
    fn number(&self) -> Result<u64, ClientError> {
        if let Value::Integer(value) = self.value {
            Ok(value)
        } else {
            Err(invalid())
        }
    }
    fn list(&self) -> Result<&[Node<'a>], ClientError> {
        if let Value::List(value) = &self.value {
            Ok(value)
        } else {
            Err(invalid())
        }
    }
    fn dict(&self) -> Result<&BTreeMap<&'a [u8], Node<'a>>, ClientError> {
        if let Value::Dict(value) = &self.value {
            Ok(value)
        } else {
            Err(invalid())
        }
    }
}
struct Parser<'a> {
    bytes: &'a [u8],
    at: usize,
    nodes: usize,
}
impl<'a> Parser<'a> {
    fn number_until(&mut self, end: u8) -> Result<u64, ClientError> {
        let start = self.at;
        while self.bytes.get(self.at).is_some_and(u8::is_ascii_digit) {
            self.at += 1;
        }
        if self.at == start
            || self.bytes.get(self.at) != Some(&end)
            || (self.at - start > 1 && self.bytes[start] == b'0')
        {
            return Err(invalid());
        }
        let value = std::str::from_utf8(&self.bytes[start..self.at])
            .map_err(|_| invalid())?
            .parse()
            .map_err(|_| invalid())?;
        self.at += 1;
        Ok(value)
    }
    fn node(&mut self, depth: usize) -> Result<Node<'a>, ClientError> {
        self.nodes += 1;
        if depth > 64 || self.nodes > 100_000 {
            return Err(invalid());
        }
        let start = self.at;
        let value = match self.bytes.get(self.at).copied().ok_or_else(invalid)? {
            b'i' => {
                self.at += 1;
                Value::Integer(self.number_until(b'e')?)
            }
            b'0'..=b'9' => {
                let length = usize::try_from(self.number_until(b':')?).map_err(|_| invalid())?;
                let end = self
                    .at
                    .checked_add(length)
                    .filter(|end| *end <= self.bytes.len())
                    .ok_or_else(invalid)?;
                let value = &self.bytes[self.at..end];
                self.at = end;
                Value::Bytes(value)
            }
            b'l' => {
                self.at += 1;
                let mut values = Vec::new();
                while self.bytes.get(self.at) != Some(&b'e') {
                    values.push(self.node(depth + 1)?);
                }
                self.at += 1;
                Value::List(values)
            }
            b'd' => {
                self.at += 1;
                let mut values = BTreeMap::new();
                while self.bytes.get(self.at) != Some(&b'e') {
                    let key = self.node(depth + 1)?.bytes()?;
                    if values.insert(key, self.node(depth + 1)?).is_some() {
                        return Err(invalid());
                    }
                }
                self.at += 1;
                Value::Dict(values)
            }
            _ => return Err(invalid()),
        };
        Ok(Node {
            value,
            start,
            end: self.at,
        })
    }
}
fn text(bytes: &[u8]) -> Result<String, ClientError> {
    let text = std::str::from_utf8(bytes).map_err(|_| {
        ClientError::new(
            ErrorCode::Unsupported,
            "种子文件名不是UTF-8，当前无法安全解析。",
        )
    })?;
    if text.is_empty() || text.len() > 4096 || text.chars().any(char::is_control) {
        return Err(invalid());
    }
    Ok(text.into())
}
fn component(bytes: &[u8]) -> Result<String, ClientError> {
    let text = text(bytes)?;
    if text == "." || text == ".." || text.contains(['/', '\\']) {
        return Err(invalid());
    }
    Ok(text)
}
fn get<'a, 'b>(
    map: &'b BTreeMap<&'a [u8], Node<'a>>,
    key: &[u8],
) -> Result<&'b Node<'a>, ClientError> {
    map.get(key).ok_or_else(invalid)
}

fn parse(bytes: &[u8]) -> Result<(TorrentPreview, String), ClientError> {
    if bytes.is_empty() || bytes.len() > MAX_TORRENT_BYTES {
        return Err(invalid());
    }
    let mut parser = Parser {
        bytes,
        at: 0,
        nodes: 0,
    };
    let root = parser.node(0)?;
    if parser.at != bytes.len() {
        return Err(invalid());
    }
    let top = root.dict()?;
    let info = get(top, b"info")?;
    let fields = info.dict()?;
    if !fields.contains_key(b"pieces".as_slice()) {
        return Err(ClientError::new(
            ErrorCode::Unsupported,
            "当前支持v1或含v1信息的混合种子，尚不支持纯BitTorrent v2种子。",
        ));
    }
    let title = component(
        fields
            .get(b"name.utf-8".as_slice())
            .or_else(|| fields.get(b"name".as_slice()))
            .ok_or_else(invalid)?
            .bytes()?,
    )?;
    let mut files = Vec::new();
    let mut total = 0u64;
    let mut count = 0usize;
    let mut paths = std::collections::BTreeSet::new();
    if let Some(entries) = fields.get(b"files".as_slice()) {
        if fields.contains_key(b"length".as_slice()) || entries.list()?.is_empty() {
            return Err(invalid());
        }
        for entry in entries.list()? {
            count += 1;
            if count > 10_000 {
                return Err(invalid());
            }
            let values = entry.dict()?;
            let size = get(values, b"length")?.number()?;
            let components = values
                .get(b"path.utf-8".as_slice())
                .or_else(|| values.get(b"path".as_slice()))
                .ok_or_else(invalid)?
                .list()?;
            if components.is_empty() || components.len() > 64 {
                return Err(invalid());
            }
            let path = components
                .iter()
                .map(|node| component(node.bytes()?))
                .collect::<Result<Vec<_>, _>>()?
                .join("/");
            if path.len() > 4096 || !paths.insert(path.clone()) {
                return Err(invalid());
            }
            total = total.checked_add(size).ok_or_else(invalid)?;
            if files.len() < 200 {
                files.push(TorrentFile {
                    path,
                    size_bytes: DecimalU64(size),
                });
            }
        }
    } else {
        total = get(fields, b"length")?.number()?;
        count = 1;
        files.push(TorrentFile {
            path: title.clone(),
            size_bytes: DecimalU64(total),
        });
    }
    let piece_length = get(fields, b"piece length")?.number()?;
    let pieces = get(fields, b"pieces")?.bytes()?;
    if piece_length == 0
        || pieces.len() % 20 != 0
        || total.div_ceil(piece_length) != pieces.len() as u64 / 20
    {
        return Err(invalid());
    }
    let info_hash = format!("{:x}", sha1::Sha1::digest(&bytes[info.start..info.end]));
    let private = fields
        .get(b"private".as_slice())
        .map(Node::number)
        .transpose()?
        .unwrap_or(0)
        != 0;
    let mut trackers = std::collections::BTreeSet::new();
    if let Some(announce) = top.get(b"announce".as_slice()) {
        trackers.insert(text(announce.bytes()?)?);
    }
    if let Some(tiers) = top.get(b"announce-list".as_slice()) {
        for tier in tiers.list()? {
            for tracker in tier.list()? {
                trackers.insert(text(tracker.bytes()?)?);
                if trackers.len() > 64 {
                    return Err(invalid());
                }
            }
        }
    }
    let mut magnet = url::Url::parse("magnet:?").map_err(|_| invalid())?;
    {
        let mut query = magnet.query_pairs_mut();
        query.append_pair("xt", &format!("urn:btih:{info_hash}"));
        query.append_pair("dn", &title);
        query.append_pair("xl", &total.to_string());
        for tracker in trackers {
            let url = url::Url::parse(&tracker).map_err(|_| invalid())?;
            if !matches!(url.scheme(), "http" | "https" | "udp")
                || url.host_str().is_none()
                || !url.username().is_empty()
                || url.password().is_some()
            {
                return Err(invalid());
            }
            query.append_pair("tr", &tracker);
        }
    }
    validate_source(magnet.as_str())?;
    let expires_at = (time::OffsetDateTime::now_utc() + time::Duration::minutes(5))
        .format(&time::format_description::well_known::Rfc3339)
        .map_err(|_| invalid())?;
    Ok((
        TorrentPreview {
            import_id: uuid::Uuid::new_v4().to_string(),
            title,
            info_hash,
            total_bytes: DecimalU64(total),
            file_count: count,
            files,
            private,
            expires_at,
        },
        magnet.into(),
    ))
}

impl AppService {
    pub async fn torrent_import(
        &self,
        bytes: &[u8],
        expected_scope: &str,
        ctx: &RequestContext,
    ) -> Result<TorrentPreview, ClientError> {
        self.check_scope(expected_scope, ctx).await?;
        let (preview, magnet) = parse(bytes)?;
        self.check_scope(expected_scope, ctx).await?;
        let mut imports = self.torrent_imports.lock().map_err(|_| invalid())?;
        imports.retain(|_, entry| entry.created.elapsed() < Duration::from_secs(300));
        if imports.len() >= 8 {
            return Err(ClientError::new(
                ErrorCode::Conflict,
                "尚有未关闭的种子预览，请关闭后重试。",
            ));
        }
        imports.insert(
            preview.import_id.clone(),
            PendingTorrent {
                scope: expected_scope.into(),
                magnet,
                created: Instant::now(),
            },
        );
        Ok(preview)
    }
    pub async fn torrent_create(
        &self,
        id: &str,
        parent: &str,
        key: &str,
        ctx: &RequestContext,
    ) -> Result<TaskRef, ClientError> {
        let (scope, magnet) = {
            let imports = self.torrent_imports.lock().map_err(|_| invalid())?;
            let entry = imports
                .get(id)
                .filter(|entry| entry.created.elapsed() < Duration::from_secs(300))
                .ok_or_else(|| {
                    ClientError::new(ErrorCode::Conflict, "种子预览已过期，请重新选择文件。")
                })?;
            (entry.scope.clone(), entry.magnet.clone())
        };
        self.check_scope(&scope, ctx).await?;
        self.create_native_cloud_at(&magnet, parent, key, &scope, ctx)
            .await
    }
    pub fn torrent_release(&self, id: &str) -> Result<(), ClientError> {
        self.torrent_imports
            .lock()
            .map_err(|_| invalid())?
            .remove(id);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn source() -> Vec<u8> {
        let mut b =
            b"d4:infod6:lengthi3e4:name8:test.bin12:piece lengthi16384e6:pieces20:".to_vec();
        b.extend_from_slice(&[9; 20]);
        b.extend_from_slice(b"ee");
        b
    }
    #[test]
    fn torrent_uses_exact_info_bytes_and_rejects_damaged_or_ambiguous_input() {
        let bytes = source();
        let (preview, magnet) = parse(&bytes).unwrap();
        assert_eq!(
            preview.info_hash,
            format!("{:x}", sha1::Sha1::digest(&bytes[7..bytes.len() - 1]))
        );
        assert_eq!(preview.total_bytes.0, 3);
        assert_eq!(preview.file_count, 1);
        assert!(magnet.contains(&preview.info_hash));
        for wrong in [
            b"not torrent".to_vec(),
            bytes[..bytes.len() - 1].to_vec(),
            [bytes.clone(), b"extra".to_vec()].concat(),
            b"d4:infoi0e4:infoi1ee".to_vec(),
        ] {
            assert!(parse(&wrong).is_err());
        }
    }
    #[test]
    fn torrent_bounds_piece_counts_and_paths() {
        let bytes = source();
        let changed = String::from_utf8_lossy(&bytes).replace("i3e", "i999999e");
        assert!(parse(changed.as_bytes()).is_err());
        let changed = String::from_utf8_lossy(&bytes).replace("8:test.bin", "2:..");
        assert!(parse(changed.as_bytes()).is_err());
        assert!(parse(&vec![b'd'; MAX_TORRENT_BYTES + 1]).is_err());
        assert!(parse(&[b'l'; 70]).is_err());
    }

    #[test]
    fn multi_file_torrent_preserves_trackers_without_exposing_them_in_the_preview() {
        let tracker = "https://tracker.example/announce?passkey=PRIVATE";
        let mut bytes=format!("d8:announce{}:{tracker}4:infod5:filesld6:lengthi3e4:pathl5:a.bineed6:lengthi2e4:pathl3:sub5:b.bineee4:name6:bundle12:piece lengthi16384e6:pieces20:",tracker.len()).into_bytes();
        bytes.extend_from_slice(&[9; 20]);
        bytes.extend_from_slice(b"7:privatei1eee");
        let (preview, magnet) = parse(&bytes).unwrap();
        assert_eq!(preview.file_count, 2);
        assert_eq!(preview.total_bytes.0, 5);
        assert_eq!(preview.files[1].path, "sub/b.bin");
        assert!(preview.private);
        assert!(!serde_json::to_string(&preview).unwrap().contains("PRIVATE"));
        assert!(url::Url::parse(&magnet)
            .unwrap()
            .query_pairs()
            .any(|(key, value)| key == "tr" && value == tracker));
    }

    struct CloudFixture {
        scope: std::sync::Mutex<String>,
        creates: std::sync::atomic::AtomicUsize,
    }
    #[async_trait::async_trait]
    impl ProviderPort for CloudFixture {
        fn id(&self) -> BackendId {
            BackendId::RustCompat
        }
        async fn discover(&self) -> Result<CapabilityReport, ClientError> {
            unreachable!()
        }
        async fn list(
            &self,
            _: &str,
            _: Option<&str>,
            _: usize,
        ) -> Result<Page<RemoteEntry>, ClientError> {
            unreachable!()
        }
        async fn stat(&self, _: &str) -> Result<RemoteEntry, ClientError> {
            unreachable!()
        }
        async fn quota(&self) -> Result<QuotaReport, ClientError> {
            unreachable!()
        }
        async fn account_scope(&self) -> Result<String, ClientError> {
            Ok(self.scope.lock().unwrap().clone())
        }
        async fn cloud_create(
            &self,
            source: &str,
            _: &str,
            scope: &str,
        ) -> Result<CloudTaskSnapshot, ClientError> {
            assert!(source.starts_with("magnet:?"));
            assert_eq!(scope, self.account_scope().await?);
            self.creates
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(CloudTaskSnapshot {
                id: "cloud-from-torrent".into(),
                name: "test.bin".into(),
                state: TaskState::Queued,
                progress: None,
                file_id: None,
                total_bytes: Some(DecimalU64(3)),
                created_at: None,
                error: None,
            })
        }
    }
    #[tokio::test]
    async fn torrent_confirmation_reuses_cloud_intents_and_rejects_other_accounts_or_released_imports(
    ) {
        let directory = tempfile::tempdir().unwrap();
        let provider = Arc::new(CloudFixture {
            scope: std::sync::Mutex::new("a".into()),
            creates: Default::default(),
        });
        let app = AppService::with_provider(&directory.path().join("tasks.db"), provider.clone());
        let ctx = RequestContext::default();
        let preview = app.torrent_import(&source(), "a", &ctx).await.unwrap();
        assert_eq!(
            provider.creates.load(std::sync::atomic::Ordering::SeqCst),
            0
        );
        *provider.scope.lock().unwrap() = "b".into();
        assert_eq!(
            app.torrent_create(&preview.import_id, "root", "one", &ctx)
                .await
                .unwrap_err()
                .code,
            ErrorCode::AuthExpired
        );
        *provider.scope.lock().unwrap() = "a".into();
        let first = app
            .torrent_create(&preview.import_id, "root", "one", &ctx)
            .await
            .unwrap();
        assert_eq!(
            app.torrent_create(&preview.import_id, "root", "one", &ctx)
                .await
                .unwrap()
                .task_id,
            first.task_id
        );
        assert_eq!(
            provider.creates.load(std::sync::atomic::Ordering::SeqCst),
            1
        );
        app.torrent_release(&preview.import_id).unwrap();
        assert_eq!(
            app.torrent_create(&preview.import_id, "root", "one", &ctx)
                .await
                .unwrap_err()
                .code,
            ErrorCode::Conflict
        );
        let preview = app.torrent_import(&source(), "a", &ctx).await.unwrap();
        app.torrent_imports
            .lock()
            .unwrap()
            .get_mut(&preview.import_id)
            .unwrap()
            .created = Instant::now() - Duration::from_secs(301);
        assert_eq!(
            app.torrent_create(&preview.import_id, "root", "two", &ctx)
                .await
                .unwrap_err()
                .code,
            ErrorCode::Conflict
        );
        assert_eq!(
            provider.creates.load(std::sync::atomic::Ordering::SeqCst),
            1
        );
    }
}
