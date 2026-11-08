//! Bounded EBML/Cues reader and native Matroska stream-copy writer. Sample payloads
//! stay in authenticated range spans; only metadata/block headers are inspected.
use super::*;
const SEGMENT: u64 = 0x18538067;
const INFO: u64 = 0x1549a966;
const TRACKS: u64 = 0x1654ae6b;
const CUES: u64 = 0x1c53bb6b;
const CLUSTER: u64 = 0x1f43b675;

#[derive(Clone, Copy)]
struct Element {
    id: u64,
    start: u64,
    data: u64,
    end: u64,
}
#[derive(Clone, Copy)]
struct Cue {
    time: u64,
    position: u64,
}
pub(super) struct Index {
    identity: String,
    length: u64,
    limits: Limits,
    segment_end: u64,
    ebml: Vec<u8>,
    tracks: Vec<u8>,
    scale_ns: u64,
    video: u64,
    info: MediaInfo,
    cues: Vec<Cue>,
}
fn vint(bytes: &[u8], id: bool) -> Result<(u64, usize, bool), ClientError> {
    let first = *bytes.first().ok_or_else(invalid)?;
    if first == 0 {
        return Err(invalid());
    }
    let width = first.leading_zeros() as usize + 1;
    if width > if id { 4 } else { 8 } || bytes.len() < width {
        return Err(invalid());
    }
    let mut value = if id {
        first as u64
    } else {
        (first & (0xffu16 >> width) as u8) as u64
    };
    for byte in &bytes[1..width] {
        value = (value << 8) | *byte as u64;
    }
    Ok((value, width, !id && value == ((1u64 << (width * 7)) - 1)))
}
fn element(bytes: &[u8], offset: u64, bound: u64) -> Result<Element, ClientError> {
    let (id, ni, _) = vint(bytes, true)?;
    let (size, ns, unknown) = vint(&bytes[ni..], false)?;
    let data = offset.checked_add((ni + ns) as u64).ok_or_else(invalid)?;
    let end = if unknown {
        bound
    } else {
        data.checked_add(size).ok_or_else(invalid)?
    };
    if data > bound || end > bound || end < data {
        return Err(invalid());
    }
    if unknown && id != SEGMENT {
        return Err(unsupported(
            "Unknown-size Matroska sub-elements require a streaming parser.",
        ));
    }
    Ok(Element {
        id,
        start: offset,
        data,
        end,
    })
}
async fn remote_element(
    source: &dyn RangeSource,
    offset: u64,
    bound: u64,
    limits: Limits,
) -> Result<Element, ClientError> {
    if offset >= bound {
        return Err(invalid());
    }
    let header = read(source, offset, (bound - offset).min(12) as usize, limits).await?;
    element(&header, offset, bound)
}
fn elements(bytes: &[u8]) -> Result<Vec<Element>, ClientError> {
    let mut out = Vec::new();
    let mut at = 0;
    while at < bytes.len() as u64 {
        if out.len() > 2_000_000 {
            return Err(invalid());
        }
        let child = element(&bytes[at as usize..], at, bytes.len() as u64)?;
        out.push(child);
        at = child.end;
    }
    Ok(out)
}
fn payload(bytes: &[u8], element: Element) -> &[u8] {
    &bytes[element.data as usize..element.end as usize]
}
fn uint(bytes: &[u8]) -> Result<u64, ClientError> {
    if bytes.is_empty() || bytes.len() > 8 {
        return Err(invalid());
    }
    Ok(bytes.iter().fold(0, |value, b| (value << 8) | *b as u64))
}
fn child<'a>(bytes: &'a [u8], list: &[Element], id: u64) -> Result<Option<&'a [u8]>, ClientError> {
    let mut found = list.iter().filter(|item| item.id == id);
    let first = found.next().map(|e| payload(bytes, *e));
    if found.next().is_some() {
        return Err(invalid());
    }
    Ok(first)
}
fn required<'a>(bytes: &'a [u8], list: &[Element], id: u64) -> Result<&'a [u8], ClientError> {
    child(bytes, list, id)?.ok_or_else(invalid)
}
fn number(bytes: &[u8], list: &[Element], id: u64, default: u64) -> Result<u64, ClientError> {
    child(bytes, list, id)?
        .map(uint)
        .transpose()
        .map(|v| v.unwrap_or(default))
}
fn text(bytes: &[u8]) -> Result<String, ClientError> {
    if bytes.len() > 256 {
        return Err(invalid());
    }
    String::from_utf8(bytes.to_vec()).map_err(|_| invalid())
}
fn size(value: u64) -> Result<Vec<u8>, ClientError> {
    for width in 1..=8 {
        if value < (1u64 << (7 * width)) - 1 {
            let mut out = value.to_be_bytes()[8 - width..].to_vec();
            out[0] |= 1 << (8 - width);
            return Ok(out);
        }
    }
    Err(invalid())
}
fn id_bytes(id: u64) -> Vec<u8> {
    let bytes = id.to_be_bytes();
    bytes[bytes.iter().position(|&n| n != 0).unwrap_or(7)..].to_vec()
}
fn ebml(id: u64, data: &[u8]) -> Result<Vec<u8>, ClientError> {
    let mut out = id_bytes(id);
    out.extend(size(data.len() as u64)?);
    out.extend_from_slice(data);
    Ok(out)
}
fn unsigned(id: u64, value: u64) -> Result<Vec<u8>, ClientError> {
    let bytes = value.to_be_bytes();
    ebml(
        id,
        &bytes[bytes.iter().position(|&n| n != 0).unwrap_or(7)..],
    )
}
fn to_ms(value: u64, scale: u64) -> Result<u64, ClientError> {
    u64::try_from(value as u128 * scale as u128 / 1_000_000).map_err(|_| invalid())
}

pub(super) async fn probe(source: &dyn RangeSource, limits: Limits) -> Result<Index, ClientError> {
    let length = source.len();
    let identity = source.identity().to_owned();
    let first = remote_element(source, 0, length, limits).await?;
    if first.id != 0x1a45dfa3 || first.end > 4096 {
        return Err(invalid());
    }
    let ebml_header = read(source, 0, first.end as usize, limits).await?;
    let ebml_fields = elements(&ebml_header[first.data as usize..])?;
    let header_data = &ebml_header[first.data as usize..];
    let doc = required(header_data, &ebml_fields, 0x4282)?;
    if !matches!(doc, b"matroska" | b"webm") {
        return Err(unsupported("This EBML document is not Matroska/WebM."));
    }
    let segment = remote_element(source, first.end, length, limits).await?;
    if segment.id != SEGMENT {
        return Err(invalid());
    }
    let mut at = segment.data;
    let mut info = None;
    let mut tracks = None;
    let mut cues = None;
    let mut pending = Vec::new();
    let mut inspected = 0usize;
    let mut visited = std::collections::HashSet::new();
    for _ in 0..50_000 {
        if info.is_some() && tracks.is_some() && cues.is_some() {
            break;
        }
        let next = if let Some(position) = pending.pop() {
            position
        } else {
            if at >= segment.end {
                break;
            }
            at
        };
        let e = remote_element(source, next, segment.end, limits).await?;
        if next == at {
            at = e.end;
        }
        if !visited.insert(next) {
            continue;
        }
        if matches!(e.id, INFO | TRACKS | CUES | 0x114d9b74) {
            let count = usize::try_from(e.end - e.data).map_err(|_| invalid())?;
            inspected = inspected.checked_add(count).ok_or_else(invalid)?;
            if inspected > limits.max_metadata_bytes {
                return Err(invalid());
            }
            let bytes = read(source, e.data, count, limits).await?;
            match e.id {
                INFO => {
                    if info.is_some() {
                        return Err(invalid());
                    }
                    info = Some(bytes);
                }
                TRACKS => {
                    if tracks.is_some() {
                        return Err(invalid());
                    }
                    tracks = Some(bytes);
                }
                CUES => {
                    if cues.is_some() {
                        return Err(invalid());
                    }
                    cues = Some(bytes);
                }
                _ => {
                    for seek in elements(&bytes)?.into_iter().filter(|s| s.id == 0x4dbb) {
                        let p = payload(&bytes, seek);
                        let fields = elements(p)?;
                        let target = uint(required(p, &fields, 0x53ab)?)?;
                        if matches!(target, INFO | TRACKS | CUES) {
                            let position = segment
                                .data
                                .checked_add(uint(required(p, &fields, 0x53ac)?)?)
                                .ok_or_else(invalid)?;
                            if position < segment.data || position >= segment.end {
                                return Err(invalid());
                            }
                            pending.push(position);
                        }
                    }
                }
            }
        }
    }
    let info_data = info.ok_or_else(invalid)?;
    let fields = elements(&info_data)?;
    let scale_ns = number(&info_data, &fields, 0x2ad7b1, 1_000_000)?;
    if scale_ns == 0 || scale_ns > 1_000_000_000 {
        return Err(invalid());
    }
    let duration = required(&info_data, &fields, 0x4489)?;
    let duration = match duration.len() {
        4 => f32::from_be_bytes(duration.try_into().map_err(|_| invalid())?) as f64,
        8 => f64::from_be_bytes(duration.try_into().map_err(|_| invalid())?),
        _ => return Err(invalid()),
    };
    let duration_ms = duration * scale_ns as f64 / 1_000_000.;
    if !duration_ms.is_finite() || duration_ms <= 0. || duration_ms > i64::MAX as f64 {
        return Err(invalid());
    }
    let duration_ms = duration_ms.ceil() as u64;
    let tracks = tracks.ok_or_else(invalid)?;
    let mut track_infos = Vec::new();
    let mut video = None;
    let mut track_ids = std::collections::HashSet::new();
    for entry in elements(&tracks)?.into_iter().filter(|e| e.id == 0xae) {
        if track_infos.len() >= limits.max_tracks {
            return Err(invalid());
        }
        let data = payload(&tracks, entry);
        let fields = elements(data)?;
        let id = uint(required(data, &fields, 0xd7)?)?;
        if id == 0 || id > u32::MAX as u64 || !track_ids.insert(id) {
            return Err(invalid());
        }
        let kind = match number(data, &fields, 0x83, 0)? {
            1 => TrackKind::Video,
            2 => TrackKind::Audio,
            17 => TrackKind::Subtitle,
            _ => TrackKind::Other,
        };
        if kind == TrackKind::Video && video.replace(id).is_some() {
            return Err(unsupported(
                "Native clipping requires exactly one Matroska video track.",
            ));
        }
        if child(data, &fields, 0x6d80)?.is_some() {
            return Err(unsupported(
                "Compressed/encrypted Matroska tracks need a dedicated decoding policy.",
            ));
        }
        let codec = text(required(data, &fields, 0x86)?)?;
        let language = child(data, &fields, 0x22b59c)?
            .map(text)
            .transpose()?
            .unwrap_or_else(|| "eng".into());
        let (width, height) = if let Some(v) = child(data, &fields, 0xe0)? {
            let f = elements(v)?;
            (
                Some(u32::try_from(number(v, &f, 0xb0, 0)?).map_err(|_| invalid())?),
                Some(u32::try_from(number(v, &f, 0xba, 0)?).map_err(|_| invalid())?),
            )
        } else {
            (None, None)
        };
        track_infos.push(TrackInfo {
            id: id as u32,
            kind,
            codec,
            language,
            duration_ms,
            width,
            height,
        });
    }
    let video = video.ok_or_else(|| unsupported("Matroska clipping requires a video track."))?;
    let cue_data = cues.ok_or_else(|| {
        unsupported("Matroska has no Cues index; a full scan would be needed and was not started.")
    })?;
    let mut cues = Vec::new();
    for point in elements(&cue_data)?.into_iter().filter(|e| e.id == 0xbb) {
        if cues.len() >= limits.max_samples {
            return Err(invalid());
        }
        let data = payload(&cue_data, point);
        let fields = elements(data)?;
        let time = uint(required(data, &fields, 0xb3)?)?;
        for position in fields.iter().filter(|e| e.id == 0xb7) {
            let p = payload(data, *position);
            let f = elements(p)?;
            if uint(required(p, &f, 0xf7)?)? != video {
                continue;
            }
            let position = segment
                .data
                .checked_add(uint(required(p, &f, 0xf1)?)?)
                .ok_or_else(invalid)?;
            if position >= segment.end
                || position < segment.data
                || cues
                    .last()
                    .is_some_and(|last: &Cue| last.time >= time || last.position > position)
            {
                return Err(invalid());
            }
            cues.push(Cue { time, position });
        }
    }
    if cues.is_empty() {
        return Err(invalid());
    }
    let info=MediaInfo{container:"mkv".into(),duration_ms,tracks:track_infos,keyframes:"indexed".into(),warnings:vec!["Matroska export retains track metadata and selected blocks; chapters and attachments are not copied.".into()]};
    if source.identity() != identity || source.len() != length {
        return Err(invalid());
    }
    Ok(Index {
        identity,
        length,
        limits,
        segment_end: segment.end,
        ebml: ebml_header,
        tracks,
        scale_ns,
        video,
        info,
        cues,
    })
}

struct Block {
    element: Element,
    track: u64,
    time: i64,
    key: bool,
    time_offset: u64,
}
impl Index {
    pub(super) async fn mp4_plan(
        &self,
        source: &dyn RangeSource,
        range: crate::ClipRange,
    ) -> Result<ClipPlan, ClientError> {
        let original = self.plan(source, range).await?;
        let mut tracks = std::collections::BTreeMap::new();
        let mut defaults = std::collections::HashMap::new();
        for entry in elements(&self.tracks)?.into_iter().filter(|e| e.id == 0xae) {
            let bytes = payload(&self.tracks, entry);
            let fields = elements(bytes)?;
            let id = uint(required(bytes, &fields, 0xd7)?)?;
            let info = self
                .info
                .tracks
                .iter()
                .find(|t| t.id == id as u32)
                .ok_or_else(invalid)?
                .clone();
            if !matches!(info.kind, TrackKind::Audio | TrackKind::Video) {
                return Err(unsupported("Matroska subtitle/data tracks require a separate MP4 mapping; no track was silently removed."));
            }
            let config = required(bytes, &fields, 0x63a2)?;
            let (sample_entry, scale) = if info.kind == TrackKind::Video {
                (
                    codecs::video_private(
                        &info.codec,
                        info.width.ok_or_else(invalid)?,
                        info.height.ok_or_else(invalid)?,
                        config,
                    )?,
                    90_000,
                )
            } else {
                if info.codec != "A_AAC" {
                    return Err(unsupported("Matroska to MP4 currently retains AAC audio; other audio is not silently discarded."));
                }
                codecs::aac_private(config)?
            };
            let default = number(bytes, &fields, 0x23e383, 0)?;
            defaults.insert(id, default);
            tracks.insert(
                id,
                MuxTrack {
                    info,
                    scale,
                    sample_entry,
                    samples: Vec::new(),
                },
            );
        }
        for span in &original.spans {
            let cluster = element(&span.prefix, 0, (span.prefix.len() as u64) + span.length)?;
            if cluster.id != CLUSTER {
                return Err(invalid());
            }
            let tc = element(
                &span.prefix[cluster.data as usize..],
                cluster.data,
                cluster.end,
            )?;
            if tc.id != 0xe7 {
                return Err(invalid());
            }
            let time = uint(payload(&span.prefix, tc))?;
            let outer = element(&span.prefix[tc.end as usize..], tc.end, cluster.end)?;
            let mut block = outer;
            if outer.id == 0xa0 {
                let mut at = outer.data;
                let mut found = None;
                for _ in 0..64 {
                    if at >= span.prefix.len() as u64 {
                        break;
                    }
                    let e = element(&span.prefix[at as usize..], at, outer.end)?;
                    if e.id == 0xa1 {
                        found = Some(e);
                        break;
                    }
                    at = e.end;
                }
                block = found.ok_or_else(invalid)?;
            }
            if !matches!(block.id, 0xa3 | 0xa1) {
                return Err(invalid());
            }
            let (id, n, _) = vint(
                span.prefix.get(block.data as usize..).ok_or_else(invalid)?,
                false,
            )?;
            // prefix ends immediately after the rewritten relative timestamp.
            if block.data as usize + n + 2 != span.prefix.len() {
                return Err(invalid());
            }
            let flags = *read(source, span.offset, 1, self.limits)
                .await?
                .first()
                .ok_or_else(invalid)?;
            let track = tracks.get_mut(&id).ok_or_else(invalid)?;
            let block_header = (block.data as usize + n + 2) as u64;
            let payload_len = block
                .end
                .checked_sub(block_header + 1)
                .ok_or_else(invalid)?;
            let data_offset = span.offset + 1;
            let lace = (flags >> 1) & 3;
            let mut sizes = Vec::new();
            let mut lace_header = 0usize;
            if lace == 0 {
                sizes.push(payload_len);
            } else {
                let lead = read(
                    source,
                    data_offset,
                    payload_len.min(65536) as usize,
                    self.limits,
                )
                .await?;
                let count = *lead.first().ok_or_else(invalid)? as usize + 1;
                lace_header = 1;
                match lace {
                    1 => {
                        for _ in 0..count - 1 {
                            let mut size = 0u64;
                            loop {
                                let b = *lead.get(lace_header).ok_or_else(invalid)?;
                                lace_header += 1;
                                size += b as u64;
                                if b != 255 {
                                    break;
                                }
                            }
                            sizes.push(size);
                        }
                    }
                    2 => {
                        let bytes = payload_len - 1;
                        if !bytes.is_multiple_of(count as u64) {
                            return Err(invalid());
                        }
                        sizes = vec![bytes / count as u64; count];
                    }
                    3 => {
                        let (first, n, _) = vint(&lead[lace_header..], false)?;
                        lace_header += n;
                        sizes.push(first);
                        for _ in 1..count - 1 {
                            let (v, n, _) = vint(&lead[lace_header..], false)?;
                            lace_header += n;
                            let bias = (1i128 << (n * 7 - 1)) - 1;
                            let size =
                                *sizes.last().ok_or_else(invalid)? as i128 + v as i128 - bias;
                            sizes.push(u64::try_from(size).map_err(|_| invalid())?);
                        }
                    }
                    _ => return Err(invalid()),
                }
                if lace != 2 {
                    let used = sizes
                        .iter()
                        .try_fold(lace_header as u64, |n, s| n.checked_add(*s))
                        .ok_or_else(invalid)?;
                    sizes.push(payload_len.checked_sub(used).ok_or_else(invalid)?);
                }
            }
            if track.info.kind == TrackKind::Video && sizes.len() != 1 {
                return Err(unsupported(
                    "Laced video blocks need an explicit frame-time index.",
                ));
            }
            let pts = ((time as u128 * self.scale_ns as u128 / 1_000_000
                + original.aligned_start_ms as u128)
                * track.scale as u128
                / 1000) as i64;
            let duration = if track.info.kind == TrackKind::Audio {
                1024
            } else {
                u32::try_from(
                    *defaults.get(&id).ok_or_else(invalid)? as u128 * track.scale as u128
                        / 1_000_000_000,
                )
                .map_err(|_| invalid())?
            };
            let mut offset = data_offset + lace_header as u64;
            for (i, size) in sizes.into_iter().enumerate() {
                if size == 0 {
                    return Err(invalid());
                }
                let time = pts + i as i64 * duration as i64;
                track.samples.push(MuxSample {
                    pts: time,
                    dts: time,
                    duration,
                    sync: track.info.kind == TrackKind::Audio
                        || flags & 0x80 != 0
                        || outer.id == 0xa0,
                    spans: vec![ReadSpan {
                        offset,
                        length: size,
                        prefix: Vec::new(),
                    }],
                });
                offset = offset.checked_add(size).ok_or_else(invalid)?;
            }
        }
        for track in tracks.values_mut() {
            if track.samples.is_empty() {
                return Err(invalid());
            }
            if track.info.kind == TrackKind::Video {
                let duration = track.samples[0].duration;
                if duration == 0 {
                    return Err(unsupported("Matroska video has no verified default frame duration for MP4 decode timestamps."));
                }
                let first = track.samples[0].pts;
                // Minimal nonnegative composition offsets: preserve presentation
                // order while deriving monotonic decode timestamps from DefaultDuration.
                let delay = track
                    .samples
                    .iter()
                    .enumerate()
                    .map(|(i, s)| first + i as i64 * duration as i64 - s.pts)
                    .max()
                    .unwrap_or(0)
                    .max(0);
                for (i, s) in track.samples.iter_mut().enumerate() {
                    s.dts = first - delay + i as i64 * duration as i64;
                }
            }
        }
        isobmff::remux(source,range,original.aligned_start_ms,tracks.into_values().collect(),vec!["Matroska video/AAC streams are copied to MP4; attachments and chapters are not copied.".into()])
    }
    pub(super) fn info(&self) -> MediaInfo {
        self.info.clone()
    }
    pub(super) async fn plan(
        &self,
        source: &dyn RangeSource,
        range: crate::ClipRange,
    ) -> Result<ClipPlan, ClientError> {
        if source.identity() != self.identity || source.len() != self.length {
            return Err(invalid());
        }
        let cue = self
            .cues
            .iter()
            .rfind(|c| to_ms(c.time, self.scale_ns).unwrap_or(u64::MAX) <= range.start_ms)
            .ok_or_else(invalid)?;
        let end_units = u64::try_from(range.end_ms as u128 * 1_000_000 / self.scale_ns as u128)
            .map_err(|_| invalid())?;
        let start_units = u64::try_from(range.start_ms as u128 * 1_000_000 / self.scale_ns as u128)
            .map_err(|_| invalid())?;
        let mut at = cue.position;
        let mut groups = Vec::new();
        let mut blocks_count = 0;
        let mut metadata_bytes = 0usize;
        for _ in 0..50_000 {
            if at >= self.segment_end {
                break;
            }
            let cluster = remote_element(source, at, self.segment_end, self.limits).await?;
            at = cluster.end;
            if cluster.id != CLUSTER {
                continue;
            }
            let mut child_at = cluster.data;
            let mut timestamp = None;
            let mut blocks = Vec::new();
            for _ in 0..self.limits.max_samples {
                if child_at == cluster.end {
                    break;
                }
                let e = remote_element(source, child_at, cluster.end, self.limits).await?;
                child_at = e.end;
                metadata_bytes += 12;
                if metadata_bytes > self.limits.max_metadata_bytes {
                    return Err(invalid());
                }
                match e.id {
                    0xe7 => {
                        if timestamp.is_some() || e.end - e.data > 8 {
                            return Err(invalid());
                        }
                        timestamp = Some(uint(
                            &read(source, e.data, (e.end - e.data) as usize, self.limits).await?,
                        )?);
                    }
                    0xa3 => {
                        let prefix = read(
                            source,
                            e.data,
                            (e.end - e.data).min(16) as usize,
                            self.limits,
                        )
                        .await?;
                        let (track, n, _) = vint(&prefix, false)?;
                        let time = i16::from_be_bytes(
                            prefix
                                .get(n..n + 2)
                                .ok_or_else(invalid)?
                                .try_into()
                                .map_err(|_| invalid())?,
                        ) as i64;
                        let flags = *prefix.get(n + 2).ok_or_else(invalid)?;
                        blocks.push(Block {
                            element: e,
                            track,
                            time,
                            key: flags & 0x80 != 0,
                            time_offset: e.data + n as u64,
                        });
                    }
                    0xa0 => {
                        let mut group_at = e.data;
                        let mut block = None;
                        let mut reference = false;
                        for _ in 0..64 {
                            if group_at == e.end {
                                break;
                            }
                            let inner =
                                remote_element(source, group_at, e.end, self.limits).await?;
                            group_at = inner.end;
                            if inner.id == 0xfb {
                                reference = true;
                            }
                            if inner.id == 0xa1 {
                                if block.is_some() {
                                    return Err(invalid());
                                }
                                let prefix = read(
                                    source,
                                    inner.data,
                                    (inner.end - inner.data).min(16) as usize,
                                    self.limits,
                                )
                                .await?;
                                let (track, n, _) = vint(&prefix, false)?;
                                let time = i16::from_be_bytes(
                                    prefix
                                        .get(n..n + 2)
                                        .ok_or_else(invalid)?
                                        .try_into()
                                        .map_err(|_| invalid())?,
                                ) as i64;
                                block = Some((track, time, inner.data + n as u64));
                            }
                        }
                        if group_at != e.end {
                            return Err(invalid());
                        }
                        let (track, time, time_offset) = block.ok_or_else(invalid)?;
                        blocks.push(Block {
                            element: e,
                            track,
                            time,
                            key: !reference,
                            time_offset,
                        });
                    }
                    _ => {}
                }
                blocks_count += 1;
                if blocks_count > self.limits.max_samples {
                    return Err(invalid());
                }
            }
            if child_at != cluster.end {
                return Err(invalid());
            }
            let timestamp = timestamp.ok_or_else(invalid)?;
            let done = timestamp > end_units.saturating_add(2000 * 1_000_000 / self.scale_ns);
            groups.push((timestamp, blocks));
            if done {
                break;
            }
        }
        let aligned = groups
            .iter()
            .flat_map(|(time, blocks)| {
                blocks
                    .iter()
                    .filter(|b| b.track == self.video && b.key)
                    .filter_map(move |b| u64::try_from(*time as i128 + b.time as i128).ok())
            })
            .filter(|time| *time <= start_units)
            .max()
            .ok_or_else(invalid)?;
        let mut spans: Vec<ReadSpan> = Vec::new();
        let mut first_video = false;
        for (time, blocks) in groups {
            let selected: Vec<_> = blocks
                .into_iter()
                .filter(|b| {
                    let pts = time as i128 + b.time as i128;
                    pts >= aligned as i128 && pts < end_units as i128
                })
                .collect();
            if selected.is_empty() {
                continue;
            }
            // A new cluster per block avoids signed-relative-time overflow and
            // preserves source decode order, including B frames.
            for b in selected {
                if b.track == self.video && !first_video {
                    if !b.key {
                        return Err(invalid());
                    }
                    first_video = true;
                }
                let timestamp = u64::try_from(time as i128 + b.time as i128 - aligned as i128)
                    .map_err(|_| invalid())?;
                let tc = unsigned(0xe7, timestamp)?;
                let block_len = b.element.end - b.element.start;
                let mut prefix = id_bytes(CLUSTER);
                prefix.extend(size(tc.len() as u64 + block_len)?);
                prefix.extend(tc);
                let replace_len =
                    usize::try_from(b.time_offset + 2 - b.element.start).map_err(|_| invalid())?;
                if replace_len > 60 * 1024 {
                    return Err(invalid());
                }
                let mut before = read(source, b.element.start, replace_len, self.limits).await?;
                before[replace_len - 2..].fill(0);
                prefix.extend(before);
                let offset = b.time_offset + 2;
                spans.push(ReadSpan {
                    offset,
                    length: b.element.end - offset,
                    prefix,
                });
            }
        }
        if !first_video || spans.is_empty() {
            return Err(invalid());
        }
        let aligned_start_ms = to_ms(aligned, self.scale_ns)?;
        let output_duration_ms = range
            .end_ms
            .checked_sub(aligned_start_ms)
            .ok_or_else(invalid)?;
        let mut info = unsigned(0x2ad7b1, self.scale_ns)?;
        info.extend(ebml(
            0x4489,
            &((output_duration_ms as f64 * 1_000_000. / self.scale_ns as f64).to_be_bytes()),
        )?);
        info.extend(ebml(0x4d80, b"NekoPak")?);
        info.extend(ebml(0x5741, b"NekoPak")?);
        let mut header = self.ebml.clone();
        header.extend(id_bytes(SEGMENT));
        header.extend_from_slice(&[1, 255, 255, 255, 255, 255, 255, 255]);
        header.extend(ebml(INFO, &info)?);
        header.extend(ebml(TRACKS, &self.tracks)?);
        let output_bytes = spans
            .iter()
            .try_fold(header.len() as u64, |n, s| {
                n.checked_add(s.length)?.checked_add(s.prefix.len() as u64)
            })
            .ok_or_else(invalid)?;
        Ok(ClipPlan {
            source_identity: self.identity.clone(),
            source_bytes: self.length,
            start_ms: range.start_ms,
            end_ms: range.end_ms,
            aligned_start_ms,
            output_duration_ms,
            output_format: "mkv".into(),
            output_bytes,
            warnings: self.info.warnings.clone(),
            header,
            spans,
        })
    }
}
pub(super) fn validate_header(header: &[u8]) -> Result<(), ClientError> {
    let first = element(header, 0, header.len() as u64)?;
    if first.id != 0x1a45dfa3 {
        return Err(invalid());
    }
    let segment = element(
        &header[first.end as usize..],
        first.end,
        header.len() as u64,
    )?;
    if segment.id != SEGMENT {
        return Err(invalid());
    }
    let children = elements(&header[segment.data as usize..])?;
    if children.len() != 2 || children[0].id != INFO || children[1].id != TRACKS {
        return Err(invalid());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ebml_vints_have_checked_widths_and_unknown_sizes() {
        assert_eq!(
            vint(&[1, 255, 255, 255, 255, 255, 255, 255], false).unwrap(),
            ((1u64 << 56) - 1, 8, true)
        );
        for bytes in [vec![], vec![0], vec![1], vec![0x1a, 0x45]] {
            assert!(vint(&bytes, false).is_err());
        }
        assert!(element(&[0xa3, 0xff], 0, 2).is_err());
        for n in [0, 1, 126, 127, 16382, 16383, 1_000_000, u32::MAX as u64] {
            let e = size(n).unwrap();
            assert_eq!(vint(&e, false).unwrap().0, n);
        }
    }
}
