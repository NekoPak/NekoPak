use nekopak_domain::{ClientError, ErrorCode};
use nekopak_mount::*;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};

fn entry(path: &str, directory: bool) -> MountEntry {
    MountEntry {
        path: path.into(),
        remote_id: path.into(),
        identity: format!("account:version:{path}"),
        size: if directory { 0 } else { 16_384 },
        directory,
        modified_filetime: 0,
    }
}
#[derive(Default)]
struct Source {
    reads: Mutex<Vec<(u64, usize)>>,
    short: AtomicBool,
    cancel_during: AtomicBool,
}
impl ReadSource for Source {
    fn read_exact(
        &self,
        entry: &MountEntry,
        offset: u64,
        length: usize,
        cancel: &CancellationToken,
    ) -> Result<Vec<u8>> {
        cancel.check()?;
        self.reads.lock().unwrap().push((offset, length));
        if entry.identity != format!("account:version:{}", entry.path) {
            return Err(ClientError::new(ErrorCode::IntegrityFailed, "version"));
        }
        if self.cancel_during.load(Ordering::Acquire) {
            cancel.cancel();
        }
        let length = if self.short.load(Ordering::Acquire) {
            length.saturating_sub(1)
        } else {
            length
        };
        Ok((offset..offset + length as u64)
            .map(|n| (n % 251) as u8)
            .collect())
    }
}
#[test]
fn paths_reject_escape_devices_ads_and_collisions() {
    for path in [
        "",
        "../a",
        "/x",
        "a//b",
        "a\\b",
        "C:/a",
        "a:stream",
        "a/..",
        "CON.txt",
        "Lpt1",
        "COM¹.txt",
        "NUL",
        "a.",
        "a ",
        "a?b",
        ".nekopak-cloud-root.json",
    ] {
        assert!(validate_relative_path(path).is_err(), "{path}");
    }
    for path in ["汉字/视频 🎬.mkv", "a b", ".hidden", "COM10.txt"] {
        validate_relative_path(path).unwrap();
    }
    assert!(
        MountSnapshot::new(vec![entry("movie.mkv", false), entry("MOVIE.mkv", false)]).is_err()
    );
    assert!(MountSnapshot::new(vec![entry("missing/file", false)]).is_err());
    assert!(MountSnapshot::new(vec![entry("file", false), entry("file/child", false)]).is_err());
}
#[test]
fn tree_children_and_identity_are_deterministic() {
    let entries = vec![
        entry("视频", true),
        entry("视频/b.mkv", false),
        entry("视频/A.mkv", false),
        entry("root.txt", false),
    ];
    let a = MountSnapshot::new(entries.clone()).unwrap();
    let b = MountSnapshot::new(entries.into_iter().rev().collect()).unwrap();
    assert_eq!(a.fingerprint(), b.fingerprint());
    assert_eq!(a.children("视频").len(), 2);
    assert_eq!(a.children("").len(), 2);
    assert_eq!(a.get("ROOT.TXT").unwrap().path, "root.txt");
    assert_eq!(a.total_bytes(), 49_152);
}
#[test]
fn mount_verification_performs_nonsequential_reads_and_rejects_short_data() {
    let snapshot = Arc::new(MountSnapshot::new(vec![entry("movie", false)]).unwrap());
    let source = Arc::new(Source::default());
    ReadSession::verified(snapshot.clone(), source.clone()).unwrap();
    assert_eq!(
        *source.reads.lock().unwrap(),
        vec![(12_288, 4096), (0, 4096), (6144, 4096)]
    );
    source.short.store(true, Ordering::Release);
    assert!(ReadSession::verified(snapshot, source).is_err());
    assert!(ReadSession::verified(
        Arc::new(MountSnapshot::new(vec![]).unwrap()),
        Arc::new(Source::default())
    )
    .is_err());
}
#[test]
fn cancelled_failed_short_or_out_of_bounds_read_never_succeeds() {
    let snapshot = Arc::new(MountSnapshot::new(vec![entry("movie", false)]).unwrap());
    let source = Arc::new(Source::default());
    let session = ReadSession::verified(snapshot.clone(), source.clone()).unwrap();
    let e = snapshot.get("movie").unwrap();
    let request = session.begin(42).unwrap();
    assert_eq!(
        request.read(e, 4096, 17).unwrap(),
        (4096..4113).map(|n| (n % 251) as u8).collect::<Vec<_>>()
    );
    assert!(request.read(e, u64::MAX, 1).is_err());
    assert!(request.read(e, 16_000, 500).is_err());
    assert!(session.begin(42).is_err());
    session.cancel(42);
    assert_eq!(
        request.read(e, 0, 1).unwrap_err().code,
        ErrorCode::Cancelled
    );
    drop(request);
    session.cancel(100);
    assert_eq!(session.begin(100).err().unwrap().code, ErrorCode::Cancelled);
    let request = session.begin(43).unwrap();
    source.cancel_during.store(true, Ordering::Release);
    assert_eq!(
        request.read(e, 0, 1).unwrap_err().code,
        ErrorCode::Cancelled
    );
    drop(request);
    session.stop();
    assert!(session.begin(44).is_err());
}
#[test]
fn bounded_concurrency_and_request_lifetime() {
    let session = ReadSession::verified(
        Arc::new(MountSnapshot::new(vec![entry("movie", false)]).unwrap()),
        Arc::new(Source::default()),
    )
    .unwrap();
    let mut requests = Vec::new();
    for n in 0..16 {
        requests.push(session.begin(n).unwrap());
    }
    assert!(session.begin(100).is_err());
    requests.pop();
    assert!(session.begin(100).is_ok());
    session.stop();
    for request in requests {
        assert_eq!(request.check().unwrap_err().code, ErrorCode::Cancelled);
    }
}
#[test]
fn hydration_rounds_ranges_without_overflow_or_reading_past_eof() {
    assert_eq!(hydration_range(4101, 12, 20_000).unwrap(), 4096..8192);
    assert_eq!(
        hydration_range(16_385, 8000, 20_000).unwrap(),
        16_384..20_000
    );
    assert!(hydration_range(0, 0, 100).is_err());
    assert!(hydration_range(100, 1, 100).is_err());
    assert!(hydration_range(2, u64::MAX, 100).is_err());
}
