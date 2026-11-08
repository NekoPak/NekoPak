//! Native MPEG-TS stream-copy using PAT/PMT and real PES presentation timestamps.
//! Sparse timestamp search is followed by a bounded backwards keyframe scan. It
//! never treats file-size/time ratios as the cut itself or promises frame accuracy.
use super::*;
const PACKET: usize = 188;
const WINDOW: usize = PACKET * 64;
const CLOCK: u64 = 90_000;
const WRAP: u64 = 1 << 33;

#[derive(Clone, Copy)]
struct Stamp {
    offset: u64,
    pts: u64,
    key: bool,
}
pub(super) struct Index {
    identity: String,
    length: u64,
    limits: Limits,
    video: u16,
    hevc: bool,
    first_pts: u64,
    first_offset: u64,
    header: Vec<u8>,
    info: MediaInfo,
}
struct Packet<'a> {
    pid: u16,
    start: bool,
    random_access: bool,
    payload: &'a [u8],
}
fn packet(bytes: &[u8]) -> Result<Packet<'_>, ClientError> {
    if bytes.len() != PACKET || bytes[0] != 0x47 || bytes[1] & 0x80 != 0 || bytes[3] & 0xc0 != 0 {
        return Err(invalid());
    }
    let pid = ((bytes[1] as u16 & 31) << 8) | bytes[2] as u16;
    let control = bytes[3] >> 4 & 3;
    if control == 0 {
        return Err(invalid());
    }
    let mut at = 4;
    let mut random_access = false;
    if control & 2 != 0 {
        let size = bytes[4] as usize;
        if size > 183 {
            return Err(invalid());
        }
        if size > 0 {
            random_access = bytes[5] & 0x40 != 0;
        }
        at += size + 1;
    }
    if at > PACKET {
        return Err(invalid());
    }
    Ok(Packet {
        pid,
        start: bytes[1] & 0x40 != 0,
        random_access,
        payload: if control & 1 != 0 { &bytes[at..] } else { &[] },
    })
}
fn psi<'a>(p: &Packet<'a>, id: u8) -> Result<&'a [u8], ClientError> {
    if !p.start {
        return Err(invalid());
    }
    let pointer = *p.payload.first().ok_or_else(invalid)? as usize;
    let body = p.payload.get(pointer + 1..).ok_or_else(invalid)?;
    if body.len() < 3 || body[0] != id {
        return Err(invalid());
    }
    let length = (((body[1] & 15) as usize) << 8) | body[2] as usize;
    if length < 4 || length + 3 > body.len() {
        return Err(unsupported(
            "Multi-packet TS program tables need section reassembly.",
        ));
    }
    let body = &body[..length + 3];
    // MPEG-2 section CRC, no reflection/final xor.
    let mut crc = 0xffffffffu32;
    for byte in body {
        crc ^= (*byte as u32) << 24;
        for _ in 0..8 {
            crc = if crc & 0x80000000 != 0 {
                (crc << 1) ^ 0x04c11db7
            } else {
                crc << 1
            };
        }
    }
    if crc != 0 {
        return Err(invalid());
    }
    Ok(body)
}
fn pat(body: &[u8]) -> Result<u16, ClientError> {
    if body.len() < 16 || body[5] & 1 == 0 || body[6] != 0 || body[7] != 0 {
        return Err(invalid());
    }
    let mut pmt = None;
    for entry in body[8..body.len() - 4].as_chunks::<4>().0 {
        if entry[0] == 0 && entry[1] == 0 {
            continue;
        }
        if pmt.is_some() {
            return Err(unsupported(
                "Multiple TS programs need an explicit program selection.",
            ));
        }
        pmt = Some(((entry[2] as u16 & 31) << 8) | entry[3] as u16);
    }
    pmt.ok_or_else(invalid)
}
fn pmt(body: &[u8]) -> Result<(u16, bool, Vec<TrackInfo>), ClientError> {
    if body.len() < 16 || body[5] & 1 == 0 || body[6] != 0 || body[7] != 0 {
        return Err(invalid());
    }
    let info_len = (((body[10] & 15) as usize) << 8) | body[11] as usize;
    let mut at = 12 + info_len;
    let mut video = None;
    let mut tracks = Vec::new();
    while at < body.len() - 4 {
        let bytes = body.get(at..at + 5).ok_or_else(invalid)?;
        let ty = bytes[0];
        let pid = ((bytes[1] as u16 & 31) << 8) | bytes[2] as u16;
        let len = (((bytes[3] & 15) as usize) << 8) | bytes[4] as usize;
        let descriptors = body.get(at + 5..at + 5 + len).ok_or_else(invalid)?;
        let (kind, codec) = match ty {
            0x1b => (TrackKind::Video, "avc1"),
            0x24 => (TrackKind::Video, "hvc1"),
            0x0f => (TrackKind::Audio, "mp4a"),
            0x03 | 0x04 => (TrackKind::Audio, "mp3"),
            0x81 => (TrackKind::Audio, "ac-3"),
            0x87 => (TrackKind::Audio, "ec-3"),
            _ => {
                return Err(unsupported(
                    "A TS elementary stream needs a verified codec mapping before clipping.",
                ))
            }
        };
        if kind == TrackKind::Video && video.replace((pid, ty == 0x24)).is_some() {
            return Err(unsupported(
                "TS clipping currently requires exactly one video stream.",
            ));
        }
        let mut language = "und".to_owned();
        let mut i = 0;
        while i < descriptors.len() {
            let n = *descriptors.get(i + 1).ok_or_else(invalid)? as usize;
            let data = descriptors.get(i + 2..i + 2 + n).ok_or_else(invalid)?;
            if descriptors[i] == 0x0a && n >= 4 {
                language = String::from_utf8_lossy(&data[..3]).to_string();
            }
            i += 2 + n;
        }
        tracks.push(TrackInfo {
            id: pid as u32,
            kind,
            codec: codec.into(),
            language,
            duration_ms: 0,
            width: None,
            height: None,
        });
        at += 5 + len;
    }
    if at != body.len() - 4 {
        return Err(invalid());
    }
    let (video, hevc) = video.ok_or_else(invalid)?;
    Ok((video, hevc, tracks))
}
fn timestamp(bytes: &[u8]) -> Result<u64, ClientError> {
    if bytes.len() < 5 || bytes[0] & 1 != 1 || bytes[2] & 1 != 1 || bytes[4] & 1 != 1 {
        return Err(invalid());
    }
    Ok(((bytes[0] as u64 & 14) << 29)
        | ((bytes[1] as u64) << 22)
        | ((bytes[2] as u64 & 254) << 14)
        | ((bytes[3] as u64) << 7)
        | ((bytes[4] as u64) >> 1))
}
fn stamp(
    p: &Packet<'_>,
    offset: u64,
    video: u16,
    hevc: bool,
) -> Result<Option<Stamp>, ClientError> {
    if p.pid != video || !p.start || p.payload.is_empty() {
        return Ok(None);
    }
    let b = p.payload;
    if b.len() < 14 || b[..3] != [0, 0, 1] || b[6] & 0xc0 != 0x80 || b[7] & 0x80 == 0 {
        return Err(invalid());
    }
    let pts = timestamp(&b[9..14])?;
    let data = b.get(9 + b[8] as usize..).ok_or_else(invalid)?;
    let mut idr = false;
    for i in 0..data.len().saturating_sub(4) {
        let at = if data[i..].starts_with(&[0, 0, 1]) {
            i + 3
        } else if data[i..].starts_with(&[0, 0, 0, 1]) {
            i + 4
        } else {
            continue;
        };
        if let Some(&nal) = data.get(at) {
            if if hevc {
                matches!((nal >> 1) & 63, 19..=21)
            } else {
                nal & 31 == 5
            } {
                idr = true;
            }
        }
    }
    // The muxer's random-access flag is retained even when parameter sets push
    // the IDR NAL into a subsequent packet. Decode-oracle tests cover both codecs.
    Ok(Some(Stamp {
        offset,
        pts,
        key: idr || p.random_access,
    }))
}
fn relative(pts: u64, base: u64) -> u64 {
    if pts >= base {
        pts - base
    } else {
        pts + WRAP - base
    }
}
async fn scan(
    source: &dyn RangeSource,
    offset: u64,
    length: usize,
    video: u16,
    hevc: bool,
    limits: Limits,
) -> Result<Vec<Stamp>, ClientError> {
    let offset = offset / PACKET as u64 * PACKET as u64;
    let length = length.min((source.len() - offset) as usize) / PACKET * PACKET;
    let bytes = read(source, offset, length, limits).await?;
    let mut stamps = Vec::new();
    for (i, bytes) in bytes.as_chunks::<PACKET>().0.iter().enumerate() {
        let p = packet(bytes)?;
        if let Some(s) = stamp(&p, offset + (i * PACKET) as u64, video, hevc)? {
            stamps.push(s);
        }
    }
    Ok(stamps)
}
pub(super) async fn probe(source: &dyn RangeSource, limits: Limits) -> Result<Index, ClientError> {
    if !source.len().is_multiple_of(PACKET as u64) || source.len() < PACKET as u64 * 4 {
        return Err(invalid());
    }
    let bytes = read(source, 0, WINDOW.min(source.len() as usize), limits).await?;
    let mut program = None;
    let mut mapping = None;
    let mut header = Vec::new();
    for b in bytes.as_chunks::<PACKET>().0 {
        let p = packet(b)?;
        if p.pid == 0 && p.start && program.is_none() {
            program = Some(pat(psi(&p, 0)?)?);
            header.extend_from_slice(b);
        } else if Some(p.pid) == program && p.start && mapping.is_none() {
            mapping = Some(pmt(psi(&p, 2)?)?);
            header.extend_from_slice(b);
        }
    }
    let (video, hevc, mut tracks) = mapping.ok_or_else(|| {
        unsupported("A complete PAT/PMT program map was not found in the bounded TS header.")
    })?;
    let head = scan(source, 0, WINDOW, video, hevc, limits).await?;
    let first = head.iter().min_by_key(|s| s.pts).ok_or_else(invalid)?;
    let offset = source.len().saturating_sub(WINDOW as u64) / PACKET as u64 * PACKET as u64;
    let tail = scan(source, offset, WINDOW, video, hevc, limits).await?;
    let last = tail
        .iter()
        .max_by_key(|s| relative(s.pts, first.pts))
        .ok_or_else(invalid)?;
    let duration = relative(last.pts, first.pts);
    if duration == 0 || duration > CLOCK * 60 * 60 * 24 * 7 {
        return Err(invalid());
    }
    let mut times: Vec<_> = head.iter().map(|s| relative(s.pts, first.pts)).collect();
    times.sort_unstable();
    times.dedup();
    let mut deltas: Vec<_> = times
        .windows(2)
        .map(|s| s[1] - s[0])
        .filter(|&n| n > 0 && n < CLOCK)
        .collect();
    deltas.sort_unstable();
    let frame = deltas.get(deltas.len() / 2).copied().unwrap_or(CLOCK / 25);
    let duration_ms = (duration + frame) * 1000 / CLOCK;
    for t in &mut tracks {
        t.duration_ms = duration_ms;
    }
    let info=MediaInfo{container:"ts".into(),duration_ms,tracks,keyframes:"estimated".into(),warnings:vec!["TS timestamps are preserved. The exported file begins at the preceding random-access video frame; chapters are not present.".into()]};
    Ok(Index {
        identity: source.identity().into(),
        length: source.len(),
        limits,
        video,
        hevc,
        first_pts: first.pts,
        first_offset: first.offset,
        header,
        info,
    })
}
impl Index {
    pub(super) async fn mp4_plan(
        &self,
        source: &dyn RangeSource,
        range: crate::ClipRange,
    ) -> Result<ClipPlan, ClientError> {
        let source_plan = self.plan(source, range).await?;
        let span = source_plan.spans.first().ok_or_else(invalid)?;
        let base = span.offset;
        let end = span.offset + span.length;
        let mut demux = Demux::new(self, source_plan.aligned_start_ms, range.end_ms);
        let mut at = base;
        let chunk = PACKET * (1024 * 1024 / PACKET);
        while at < end {
            let length = (end - at).min(chunk as u64) as usize;
            let bytes = read(source, at, length, self.limits).await?;
            for (i, raw) in bytes.as_chunks::<PACKET>().0.iter().enumerate() {
                demux.packet(raw, at + (i * PACKET) as u64)?;
            }
            at += length as u64;
            tokio::task::yield_now().await;
        }
        let tracks = demux.finish()?;
        isobmff::remux(source,range,source_plan.aligned_start_ms,tracks,vec!["Native stream copy retains every AAC track and language; output starts at the previous video keyframe.".into()])
    }
    pub(super) fn info(&self) -> MediaInfo {
        self.info.clone()
    }
    async fn locate(
        &self,
        source: &dyn RangeSource,
        target: u64,
    ) -> Result<(u64, u64), ClientError> {
        let (mut low, mut high) = (self.first_offset, self.length);
        let mut low_time = 0;
        for _ in 0..32 {
            if high - low <= WINDOW as u64 * 2 {
                break;
            }
            let mid = ((low + (high - low) / 2) / PACKET as u64) * PACKET as u64;
            let stamps = scan(source, mid, WINDOW, self.video, self.hevc, self.limits).await?;
            let Some(first) = stamps.first() else {
                return Err(unsupported(
                    "TS seek window contains no video timestamps; enlarge the indexed source.",
                ));
            };
            let time = relative(first.pts, self.first_pts);
            if time <= target {
                low = first.offset;
                low_time = time;
            } else {
                high = mid;
            }
        }
        Ok((low, low_time))
    }
    pub(super) async fn plan(
        &self,
        source: &dyn RangeSource,
        range: crate::ClipRange,
    ) -> Result<ClipPlan, ClientError> {
        if source.identity() != self.identity || source.len() != self.length {
            return Err(invalid());
        }
        let target = range.start_ms * CLOCK / 1000;
        let end_time = range.end_ms * CLOCK / 1000;
        let (low, _) = self.locate(source, target).await?;
        let mut search = low;
        let mut scanned = 0usize;
        let mut selected = None;
        // Up to 8MiB backwards, bounded independently of file size or a bad GOP.
        while scanned < 8 * 1024 * 1024 {
            let offset = search.saturating_sub(WINDOW as u64);
            let length = ((search - offset) as usize + WINDOW).min((self.length - offset) as usize);
            let stamps = scan(source, offset, length, self.video, self.hevc, self.limits).await?;
            if let Some(s) = stamps
                .into_iter()
                .filter(|s| s.key && relative(s.pts, self.first_pts) <= target)
                .max_by_key(|s| relative(s.pts, self.first_pts))
            {
                selected = Some(s);
                break;
            }
            if offset == 0 {
                break;
            }
            scanned += length;
            search = offset;
        }
        let start=selected.ok_or_else(||unsupported("No preceding TS random-access video frame was found within the bounded seek window."))?;
        let (end_low, _) = self.locate(source, end_time).await?;
        let mut at = end_low.max(start.offset);
        let mut end = None;
        let mut inspected = 0usize;
        while at < self.length && inspected < 8 * 1024 * 1024 {
            let stamps = scan(source, at, WINDOW, self.video, self.hevc, self.limits).await?;
            if let Some(s) = stamps
                .into_iter()
                .filter(|s| relative(s.pts, self.first_pts) >= end_time)
                .min_by_key(|s| s.offset)
            {
                end = Some(s.offset);
                break;
            }
            at = at.saturating_add(WINDOW as u64);
            inspected += WINDOW;
        }
        let end = if range.end_ms >= self.info.duration_ms {
            self.length
        } else {
            end.ok_or_else(|| {
                unsupported("TS end timestamp was not found within the bounded seek window.")
            })?
        };
        if end <= start.offset {
            return Err(invalid());
        }
        let aligned_start_ms = relative(start.pts, self.first_pts) * 1000 / CLOCK;
        let header = self.header.clone();
        let output_bytes = header.len() as u64 + end - start.offset;
        Ok(ClipPlan {
            source_identity: self.identity.clone(),
            source_bytes: self.length,
            start_ms: range.start_ms,
            end_ms: range.end_ms,
            aligned_start_ms,
            output_duration_ms: range.end_ms - aligned_start_ms,
            output_format: "ts".into(),
            output_bytes,
            warnings: self.info.warnings.clone(),
            header,
            spans: vec![ReadSpan {
                offset: start.offset,
                length: end - start.offset,
                prefix: Vec::new(),
            }],
        })
    }
}
pub(super) fn validate_header(header: &[u8]) -> Result<(), ClientError> {
    if header.len() != PACKET * 2 {
        return Err(invalid());
    }
    let first = packet(&header[..PACKET])?;
    let program = pat(psi(&first, 0)?)?;
    let second = packet(&header[PACKET..])?;
    if second.pid != program {
        return Err(invalid());
    }
    pmt(psi(&second, 2)?)?;
    Ok(())
}

struct Pes {
    bytes: Vec<u8>,
    parts: Vec<(usize, u64, usize)>,
}
impl Pes {
    fn ranges(
        &self,
        start: usize,
        end: usize,
        prefix: Vec<u8>,
    ) -> Result<Vec<ReadSpan>, ClientError> {
        if start >= end || end > self.bytes.len() {
            return Err(invalid());
        }
        let mut spans = Vec::new();
        let mut total = 0;
        for &(at, offset, length) in &self.parts {
            let lo = at.max(start);
            let hi = (at + length).min(end);
            if lo < hi {
                spans.push(ReadSpan {
                    offset: offset + (lo - at) as u64,
                    length: (hi - lo) as u64,
                    prefix: Vec::new(),
                });
                total += hi - lo;
            }
        }
        if total != end - start {
            return Err(invalid());
        }
        spans.first_mut().ok_or_else(invalid)?.prefix = prefix;
        Ok(spans)
    }
}
struct Demux<'a> {
    index: &'a Index,
    start_ms: u64,
    end_ms: u64,
    pending: std::collections::HashMap<u16, Pes>,
    tracks: std::collections::BTreeMap<u16, MuxTrack>,
    sps: Option<Vec<u8>>,
    pps: Option<Vec<u8>>,
    vps: Option<Vec<u8>>,
    aac: std::collections::HashMap<u16, (u8, u8, u8)>,
    count: usize,
}
impl<'a> Demux<'a> {
    fn new(index: &'a Index, start_ms: u64, end_ms: u64) -> Self {
        Self {
            index,
            start_ms,
            end_ms,
            pending: Default::default(),
            tracks: Default::default(),
            sps: None,
            pps: None,
            vps: None,
            aac: Default::default(),
            count: 0,
        }
    }
    fn packet(&mut self, raw: &[u8; PACKET], offset: u64) -> Result<(), ClientError> {
        let p = packet(raw)?;
        let Some(info) = self.index.info.tracks.iter().find(|t| t.id == p.pid as u32) else {
            return Ok(());
        };
        if info.kind == TrackKind::Audio && info.codec != "mp4a" {
            return Err(unsupported("Native TS to MP4 currently retains AAC audio only; other audio is not silently dropped."));
        }
        if p.payload.is_empty() {
            return Ok(());
        }
        if p.start {
            if let Some(pes) = self.pending.remove(&p.pid) {
                self.consume(p.pid, pes)?;
            }
            self.pending.insert(
                p.pid,
                Pes {
                    bytes: Vec::new(),
                    parts: Vec::new(),
                },
            );
        }
        let Some(pes) = self.pending.get_mut(&p.pid) else {
            return Ok(());
        };
        if pes.bytes.len() + p.payload.len() > 16 * 1024 * 1024 {
            return Err(invalid());
        }
        pes.parts.push((
            pes.bytes.len(),
            offset + (PACKET - p.payload.len()) as u64,
            p.payload.len(),
        ));
        pes.bytes.extend_from_slice(p.payload);
        Ok(())
    }
    fn consume(&mut self, pid: u16, pes: Pes) -> Result<(), ClientError> {
        let b = &pes.bytes;
        if b.len() < 14 || b[..3] != [0, 0, 1] || b[7] & 0x80 == 0 {
            return Err(invalid());
        }
        let size = u16::from_be_bytes([b[4], b[5]]) as usize;
        let end = if size == 0 { b.len() } else { size + 6 };
        if end > b.len() {
            return Err(invalid());
        }
        let at = 9 + b[8] as usize;
        if at >= end {
            return Err(invalid());
        }
        let pts = timestamp(&b[9..14])?;
        let dts = if b[7] & 0xc0 == 0xc0 {
            timestamp(b.get(14..19).ok_or_else(invalid)?)?
        } else {
            pts
        };
        let normalized = |n: u64| -> i64 {
            let diff = n as i64 - self.index.first_pts as i64;
            if diff < -(WRAP as i64 / 2) {
                diff + WRAP as i64
            } else if diff > WRAP as i64 / 2 {
                diff - WRAP as i64
            } else {
                diff
            }
        };
        let pts = normalized(pts);
        let dts = normalized(dts);
        if pid == self.index.video {
            self.video(pid, pes, at, end, pts, dts)
        } else {
            self.audio(pid, pes, at, end, pts)
        }
    }
    fn video(
        &mut self,
        pid: u16,
        pes: Pes,
        at: usize,
        end: usize,
        pts: i64,
        dts: i64,
    ) -> Result<(), ClientError> {
        let data = &pes.bytes;
        let mut starts = Vec::new();
        let mut i = at;
        while i + 3 < end {
            if data[i..end].starts_with(&[0, 0, 0, 1]) {
                starts.push((i, i + 4));
                i += 4;
            } else if data[i..end].starts_with(&[0, 0, 1]) {
                starts.push((i, i + 3));
                i += 3;
            } else {
                i += 1;
            }
        }
        if starts.is_empty() {
            return Err(invalid());
        }
        let mut spans = Vec::new();
        let mut key = false;
        for (j, &(_, start)) in starts.iter().enumerate() {
            let mut stop = starts.get(j + 1).map(|p| p.0).unwrap_or(end);
            while stop > start && data[stop - 1] == 0 {
                stop -= 1;
            }
            if start >= stop {
                return Err(invalid());
            }
            let nal = &data[start..stop];
            let ty = if self.index.hevc {
                (nal[0] >> 1) & 63
            } else {
                nal[0] & 31
            };
            let target = if self.index.hevc {
                match ty {
                    32 => Some(&mut self.vps),
                    33 => Some(&mut self.sps),
                    34 => Some(&mut self.pps),
                    _ => None,
                }
            } else {
                match ty {
                    7 => Some(&mut self.sps),
                    8 => Some(&mut self.pps),
                    _ => None,
                }
            };
            if let Some(target) = target {
                if nal.len() > 65535 {
                    return Err(invalid());
                }
                if target
                    .as_ref()
                    .is_some_and(|previous| previous.as_slice() != nal)
                {
                    return Err(unsupported(
                        "Changing decoder configuration requires a new sample description.",
                    ));
                }
                *target = Some(nal.to_vec());
                continue;
            }
            if if self.index.hevc { ty == 35 } else { ty == 9 } {
                continue;
            }
            key |= if self.index.hevc {
                matches!(ty, 19..=21)
            } else {
                ty == 5
            };
            spans.extend(pes.ranges(
                start,
                stop,
                ((stop - start) as u32).to_be_bytes().to_vec(),
            )?);
        }
        if spans.is_empty() {
            return Err(invalid());
        }
        let info = self
            .index
            .info
            .tracks
            .iter()
            .find(|t| t.id == pid as u32)
            .ok_or_else(invalid)?
            .clone();
        let track = self.tracks.entry(pid).or_insert_with(|| MuxTrack {
            info,
            scale: 90_000,
            sample_entry: Vec::new(),
            samples: Vec::new(),
        });
        if let Some(last) = track.samples.last_mut() {
            last.duration = u32::try_from(dts - last.dts).map_err(|_| invalid())?;
            if last.duration == 0 {
                return Err(invalid());
            }
        }
        track.samples.push(MuxSample {
            pts,
            dts,
            duration: 0,
            sync: key,
            spans,
        });
        self.count += 1;
        if self.count > self.index.limits.max_samples {
            return Err(invalid());
        }
        Ok(())
    }
    fn audio(
        &mut self,
        pid: u16,
        pes: Pes,
        mut at: usize,
        end: usize,
        pts: i64,
    ) -> Result<(), ClientError> {
        let mut frame = 0i64;
        while at < end {
            let b = pes.bytes.get(at..at + 7).ok_or_else(invalid)?;
            if b[0] != 255 || b[1] & 0xf6 != 0xf0 {
                return Err(invalid());
            }
            let profile = b[2] >> 6;
            let rate_index = b[2] >> 2 & 15;
            let channels = ((b[2] & 1) << 2) | (b[3] >> 6);
            let length =
                (((b[3] & 3) as usize) << 11) | ((b[4] as usize) << 3) | (b[5] as usize >> 5);
            let header = if b[1] & 1 != 0 { 7 } else { 9 };
            if b[6] & 3 != 0 || length <= header || at + length > end {
                return Err(unsupported(
                    "AAC ADTS multi-block or split frames need additional frame assembly.",
                ));
            }
            let config = (profile, rate_index, channels);
            if self.aac.get(&pid).is_some_and(|v| *v != config) {
                return Err(unsupported(
                    "Changing AAC configuration cannot be silently remuxed.",
                ));
            }
            self.aac.insert(pid, config);
            let (entry, rate) = codecs::aac(profile, rate_index, channels)?;
            let time = pts as i128 * rate as i128 / CLOCK as i128 + frame as i128 * 1024;
            let start = self.start_ms as i128 * rate as i128 / 1000;
            let stop = self.end_ms as i128 * rate as i128 / 1000;
            if time + 1024 > start && time < stop {
                let info = self
                    .index
                    .info
                    .tracks
                    .iter()
                    .find(|t| t.id == pid as u32)
                    .ok_or_else(invalid)?
                    .clone();
                let track = self.tracks.entry(pid).or_insert_with(|| MuxTrack {
                    info,
                    scale: rate,
                    sample_entry: entry,
                    samples: Vec::new(),
                });
                let time = i64::try_from(time).map_err(|_| invalid())?;
                let spans = pes.ranges(at + header, at + length, Vec::new())?;
                track.samples.push(MuxSample {
                    pts: time,
                    dts: time,
                    duration: 1024,
                    sync: true,
                    spans,
                });
                self.count += 1;
                if self.count > self.index.limits.max_samples {
                    return Err(invalid());
                }
            }
            at += length;
            frame += 1;
        }
        Ok(())
    }
    fn finish(mut self) -> Result<Vec<MuxTrack>, ClientError> {
        let pending = std::mem::take(&mut self.pending);
        for (pid, pes) in pending {
            self.consume(pid, pes)?;
        }
        let config = if self.index.hevc {
            codecs::hevc(
                self.vps.as_deref().ok_or_else(invalid)?,
                self.sps.as_deref().ok_or_else(invalid)?,
                self.pps.as_deref().ok_or_else(invalid)?,
            )?
        } else {
            codecs::avc(
                self.sps.as_deref().ok_or_else(invalid)?,
                self.pps.as_deref().ok_or_else(invalid)?,
            )?
        };
        let video = self.tracks.get_mut(&self.index.video).ok_or_else(invalid)?;
        let n = video.samples.len();
        if n < 2 || !video.samples[0].sync {
            return Err(invalid());
        }
        video.samples[n - 1].duration = video.samples[n - 2].duration;
        video.sample_entry = config.entry;
        video.info.width = Some(config.width);
        video.info.height = Some(config.height);
        if self.tracks.len() != self.index.info.tracks.len() {
            return Err(unsupported("An audio stream has no complete frames in the selected range; no track was silently removed."));
        }
        Ok(self.tracks.into_values().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct NoKey {
        bytes: Vec<u8>,
        requested: std::sync::atomic::AtomicU64,
    }
    impl RangeSource for NoKey {
        fn len(&self) -> u64 {
            self.bytes.len() as u64
        }
        fn identity(&self) -> &str {
            "synthetic-long-gop"
        }
        fn read_exact(&self, offset: u64, length: usize) -> RangeFuture<'_> {
            Box::pin(async move {
                self.requested.fetch_add(length as u64, Ordering::Relaxed);
                self.bytes
                    .get(offset as usize..offset as usize + length)
                    .map(|b| b.to_vec())
                    .ok_or_else(invalid)
            })
        }
    }
    fn pts(n: u64) -> [u8; 5] {
        [
            (0x20 | ((n >> 29) as u8 & 14) | 1),
            (n >> 22) as u8,
            ((n >> 14) as u8 & 254) | 1,
            (n >> 7) as u8,
            ((n << 1) as u8 & 254) | 1,
        ]
    }
    #[tokio::test]
    async fn long_gop_seek_without_any_random_access_fails_with_bounded_reads() {
        let mut bytes = Vec::new();
        for i in 0..600u64 {
            let mut p = [255; PACKET];
            p[..4].copy_from_slice(&[0x47, 0x41, 0, 0x10]);
            p[4..13].copy_from_slice(&[0, 0, 1, 0xe0, 0, 0, 0x80, 0x80, 5]);
            p[13..18].copy_from_slice(&pts(i * 3600));
            p[18..22].copy_from_slice(&[0, 0, 1, 0x41]);
            bytes.extend_from_slice(&p);
        }
        let source = NoKey {
            bytes,
            requested: Default::default(),
        };
        let index = Index {
            identity: source.identity().into(),
            length: source.len(),
            limits: Limits::default(),
            video: 256,
            hevc: false,
            first_pts: 0,
            first_offset: 0,
            header: vec![],
            info: MediaInfo {
                container: "ts".into(),
                duration_ms: 24000,
                tracks: vec![],
                keyframes: "estimated".into(),
                warnings: vec![],
            },
        };
        assert_eq!(
            index
                .plan(
                    &source,
                    crate::ClipRange {
                        start_ms: 5000,
                        end_ms: 8500
                    }
                )
                .await
                .unwrap_err()
                .code,
            ErrorCode::Unsupported
        );
        assert!(source.requested.load(Ordering::Relaxed) < 2 * source.len());
    }
    #[test]
    fn corrupt_transport_headers_and_program_crc_are_rejected() {
        for mut p in [[0; PACKET], [255; PACKET]] {
            assert!(packet(&p).is_err());
            p[0] = 0x47;
            p[1] = 0;
            p[3] = 0x20;
            p[4] = 255;
            assert!(packet(&p).is_err());
        }
        assert!(timestamp(&[0; 5]).is_err());
        for n in [0, 1, 90_000, WRAP - 1] {
            assert_eq!(timestamp(&pts(n)).unwrap(), n);
        }
        assert!(validate_header(&[0; PACKET * 2]).is_err());
    }
}
