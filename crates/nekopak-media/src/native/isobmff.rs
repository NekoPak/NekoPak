use super::*;

#[derive(Clone, Copy)]
struct Atom<'a> {
    kind: [u8; 4],
    data: &'a [u8],
}

#[derive(Clone)]
struct Sample {
    offset: u64,
    size: u32,
    dts: u64,
    duration: u32,
    composition: i64,
    sync: bool,
}

struct Track {
    info: TrackInfo,
    scale: u32,
    media_start: i64,
    empty_ms: u64,
    tkhd: Vec<u8>,
    mdhd: Vec<u8>,
    hdlr: Vec<u8>,
    media_header: Vec<u8>,
    stsd: Vec<u8>,
    samples: Vec<Sample>,
}

pub(super) struct Index {
    identity: String,
    length: u64,
    mvhd: Vec<u8>,
    movie_scale: u32,
    duration_ms: u64,
    tracks: Vec<Track>,
    warnings: Vec<String>,
}

fn u32_at(data: &[u8], offset: usize) -> Result<u32, ClientError> {
    Ok(u32::from_be_bytes(
        data.get(offset..offset + 4)
            .ok_or_else(invalid)?
            .try_into()
            .map_err(|_| invalid())?,
    ))
}
fn u64_at(data: &[u8], offset: usize) -> Result<u64, ClientError> {
    Ok(u64::from_be_bytes(
        data.get(offset..offset + 8)
            .ok_or_else(invalid)?
            .try_into()
            .map_err(|_| invalid())?,
    ))
}
fn atoms(data: &[u8]) -> Result<Vec<Atom<'_>>, ClientError> {
    let mut at = 0usize;
    let mut out = Vec::new();
    while at < data.len() {
        if out.len() == 4096 {
            return Err(invalid());
        }
        let size = u32_at(data, at)?;
        let kind = data
            .get(at + 4..at + 8)
            .ok_or_else(invalid)?
            .try_into()
            .map_err(|_| invalid())?;
        let (size, header) = match size {
            0 => ((data.len() - at) as u64, 8),
            1 => (u64_at(data, at + 8)?, 16),
            n => (n as u64, 8),
        };
        let end = at
            .checked_add(usize::try_from(size).map_err(|_| invalid())?)
            .ok_or_else(invalid)?;
        if size < header as u64 || end > data.len() {
            return Err(invalid());
        }
        out.push(Atom {
            kind,
            data: &data[at + header..end],
        });
        at = end;
    }
    Ok(out)
}
fn one<'a>(items: &'a [Atom<'a>], kind: &[u8; 4]) -> Result<Atom<'a>, ClientError> {
    let mut found = items.iter().filter(|item| &item.kind == kind);
    let item = *found.next().ok_or_else(invalid)?;
    if found.next().is_some() {
        return Err(invalid());
    }
    Ok(item)
}
fn optional<'a>(items: &'a [Atom<'a>], kind: &[u8; 4]) -> Result<Option<Atom<'a>>, ClientError> {
    let found: Vec<_> = items
        .iter()
        .filter(|item| &item.kind == kind)
        .copied()
        .collect();
    if found.len() > 1 {
        return Err(invalid());
    }
    Ok(found.first().copied())
}
fn boxed(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(payload.len() + 8);
    out.extend_from_slice(&((payload.len() + 8) as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(payload);
    out
}
fn full(kind: &[u8; 4], version: u8, payload: &[u8]) -> Vec<u8> {
    let mut bytes = vec![version, 0, 0, 0];
    bytes.extend_from_slice(payload);
    boxed(kind, &bytes)
}
fn concat_box(kind: &[u8; 4], parts: &[Vec<u8>]) -> Vec<u8> {
    boxed(kind, &parts.concat())
}
fn scale_duration(data: &[u8]) -> Result<(u32, u64), ClientError> {
    let (scale, duration) = match data.first() {
        Some(0) => (u32_at(data, 12)?, u32_at(data, 16)? as u64),
        Some(1) => (u32_at(data, 20)?, u64_at(data, 24)?),
        _ => return Err(invalid()),
    };
    if scale == 0 || scale > 1_000_000_000 || duration == u64::MAX || duration == u32::MAX as u64 {
        return Err(invalid());
    }
    Ok((scale, duration))
}
fn milliseconds(value: u64, scale: u32) -> Result<u64, ClientError> {
    u64::try_from(value as u128 * 1000 / scale as u128).map_err(|_| invalid())
}

pub(super) async fn probe(source: &dyn RangeSource, limits: Limits) -> Result<Index, ClientError> {
    let identity = source.identity().to_owned();
    let length = source.len();
    let mut at = 0u64;
    let mut moov = None;
    let mut data_ranges = Vec::new();
    let mut ftyp = false;
    for _ in 0..4096 {
        if at == length {
            break;
        }
        let header = read(source, at, 8, limits).await?;
        let raw_size = u32_at(&header, 0)?;
        let kind = &header[4..8];
        let (size, header_size) = match raw_size {
            0 => (length - at, 8),
            1 => (u64_at(&read(source, at + 8, 8, limits).await?, 0)?, 16),
            n => (n as u64, 8),
        };
        let end = at.checked_add(size).ok_or_else(invalid)?;
        if size < header_size || end > length {
            return Err(invalid());
        }
        match kind {
            b"ftyp" => {
                if ftyp || !(16..=4096).contains(&size) {
                    return Err(invalid());
                }
                ftyp = true;
            }
            b"moov" => {
                if moov.is_some() || size - header_size > limits.max_metadata_bytes as u64 {
                    return Err(invalid());
                }
                moov = Some(
                    read(
                        source,
                        at + header_size,
                        (size - header_size) as usize,
                        limits,
                    )
                    .await?,
                );
            }
            b"mdat" => data_ranges.push((at + header_size, end)),
            b"moof" => {
                return Err(unsupported(
                    "Fragmented MP4 needs a fragment index before safe clipping.",
                ))
            }
            _ => {}
        }
        at = end;
    }
    if at != length
        || !ftyp
        || data_ranges.is_empty()
        || identity != source.identity()
        || length != source.len()
    {
        return Err(invalid());
    }
    let metadata = moov
        .ok_or_else(|| unsupported("An indexed MP4 container is required for native clipping."))?;
    let children = atoms(&metadata)?;
    if optional(&children, b"mvex")?.is_some() {
        return Err(unsupported(
            "Fragmented MP4 needs a fragment index before safe clipping.",
        ));
    }
    let mvhd = one(&children, b"mvhd")?.data.to_vec();
    let (movie_scale, movie_duration) = scale_duration(&mvhd)?;
    let duration_ms = milliseconds(movie_duration, movie_scale)?;
    if duration_ms == 0 || duration_ms > i64::MAX as u64 {
        return Err(invalid());
    }
    let mut tracks = Vec::new();
    let mut warnings = Vec::new();
    let mut count = 0;
    for child in children.iter().filter(|a| &a.kind == b"trak") {
        if tracks.len() >= limits.max_tracks {
            return Err(invalid());
        }
        let track = parse_track(child.data, movie_scale, &data_ranges, limits)?;
        count += track.samples.len();
        if count > limits.max_samples {
            return Err(invalid());
        }
        if matches!(track.info.kind, TrackKind::Subtitle | TrackKind::Other) {
            warnings.push(format!(
                "Track {} ({}) is not included in MP4 stream-copy export.",
                track.info.id, track.info.codec
            ));
        }
        if tracks.iter().any(|t: &Track| t.info.id == track.info.id) {
            return Err(invalid());
        }
        tracks.push(track);
    }
    if tracks
        .iter()
        .filter(|t| t.info.kind == TrackKind::Video)
        .count()
        != 1
    {
        return Err(unsupported(
            "Native clipping requires exactly one video track; all audio tracks are retained.",
        ));
    }
    Ok(Index {
        identity,
        length,
        mvhd,
        movie_scale,
        duration_ms,
        tracks,
        warnings,
    })
}

fn parse_track(
    data: &[u8],
    movie_scale: u32,
    data_ranges: &[(u64, u64)],
    limits: Limits,
) -> Result<Track, ClientError> {
    let children = atoms(data)?;
    let tkhd = one(&children, b"tkhd")?.data.to_vec();
    let id = u32_at(&tkhd, if tkhd.first() == Some(&1) { 20 } else { 12 })?;
    if id == 0 || tkhd.len() < 84 || !matches!(tkhd.first(), Some(0 | 1)) {
        return Err(invalid());
    }
    let mdia = atoms(one(&children, b"mdia")?.data)?;
    let mdhd = one(&mdia, b"mdhd")?.data.to_vec();
    let (scale, duration) = scale_duration(&mdhd)?;
    let hdlr = one(&mdia, b"hdlr")?.data.to_vec();
    let kind = match hdlr.get(8..12).ok_or_else(invalid)? {
        b"vide" => TrackKind::Video,
        b"soun" => TrackKind::Audio,
        b"subt" | b"sbtl" | b"text" => TrackKind::Subtitle,
        _ => TrackKind::Other,
    };
    let language_at = if mdhd[0] == 1 { 32 } else { 20 };
    let raw_language = u16::from_be_bytes(
        mdhd.get(language_at..language_at + 2)
            .ok_or_else(invalid)?
            .try_into()
            .map_err(|_| invalid())?,
    );
    let language: String = [10, 5, 0]
        .iter()
        .map(|shift| ((raw_language >> shift & 31) as u8 + 0x60) as char)
        .collect();
    let minf = atoms(one(&mdia, b"minf")?.data)?;
    let media_header = minf
        .iter()
        .find(|a| matches!(&a.kind, b"vmhd" | b"smhd" | b"nmhd" | b"sthd"))
        .map(|a| boxed(&a.kind, a.data))
        .unwrap_or_else(|| full(b"nmhd", 0, &[]));
    let stbl = atoms(one(&minf, b"stbl")?.data)?;
    let stsd_data = one(&stbl, b"stsd")?.data;
    if stsd_data.first() != Some(&0) || u32_at(stsd_data, 4)? != 1 {
        return Err(unsupported(
            "Changing sample descriptions require a dedicated remux path.",
        ));
    }
    let descriptions = atoms(stsd_data.get(8..).ok_or_else(invalid)?)?;
    if descriptions.len() != 1 {
        return Err(invalid());
    }
    let description = descriptions[0];
    if description.data.get(6..8) != Some(&[0, 1]) {
        return Err(unsupported(
            "External MP4 media references are not allowed.",
        ));
    }
    let codec = String::from_utf8_lossy(&description.kind).to_string();
    if &description.kind == b"encv" || &description.kind == b"enca" {
        return Err(unsupported(
            "Encrypted media cannot be exported by this engine.",
        ));
    }
    if kind == TrackKind::Video
        && !matches!(
            &description.kind,
            b"avc1" | b"avc3" | b"hvc1" | b"hev1" | b"vp09" | b"av01"
        )
    {
        return Err(unsupported(
            "This video codec has not been verified for native MP4 stream-copy.",
        ));
    }
    if kind == TrackKind::Audio
        && !matches!(
            &description.kind,
            b"mp4a" | b"ac-3" | b"ec-3" | b"Opus" | b"alac" | b"fLaC"
        )
    {
        return Err(unsupported(
            "An audio track cannot be safely retained in MP4; no audio was silently removed.",
        ));
    }
    let stsz = one(&stbl, b"stsz")?.data;
    let fixed = u32_at(stsz, 4)?;
    let n = u32_at(stsz, 8)? as usize;
    if n == 0 || n > limits.max_samples || stsz.len() != 12 + if fixed == 0 { n * 4 } else { 0 } {
        return Err(invalid());
    }
    let mut samples = Vec::with_capacity(n);
    for i in 0..n {
        let size = if fixed != 0 {
            fixed
        } else {
            u32_at(stsz, 12 + i * 4)?
        };
        if size == 0 {
            return Err(invalid());
        }
        samples.push(Sample {
            offset: 0,
            size,
            dts: 0,
            duration: 0,
            composition: 0,
            sync: kind != TrackKind::Video,
        });
    }
    let stts = one(&stbl, b"stts")?.data;
    let runs = u32_at(stts, 4)? as usize;
    if stts.len() != 8 + runs * 8 || runs > n {
        return Err(invalid());
    }
    let mut sample_at = 0;
    let mut dts = 0u64;
    for i in 0..runs {
        let count = u32_at(stts, 8 + i * 8)? as usize;
        let delta = u32_at(stts, 12 + i * 8)?;
        if count == 0 || delta == 0 || sample_at + count > n {
            return Err(invalid());
        }
        for sample in &mut samples[sample_at..sample_at + count] {
            sample.dts = dts;
            sample.duration = delta;
            dts = dts.checked_add(delta as u64).ok_or_else(invalid)?;
        }
        sample_at += count;
    }
    if sample_at != n || dts == 0 {
        return Err(invalid());
    }
    if let Some(ctts) = optional(&stbl, b"ctts")? {
        let data = ctts.data;
        let runs = u32_at(data, 4)? as usize;
        if !matches!(data.first(), Some(0 | 1)) || data.len() != 8 + runs * 8 || runs > n {
            return Err(invalid());
        }
        let mut sample_at = 0;
        for i in 0..runs {
            let count = u32_at(data, 8 + i * 8)? as usize;
            let raw = u32_at(data, 12 + i * 8)?;
            let offset = if data[0] == 1 {
                raw as i32 as i64
            } else {
                raw as i64
            };
            if count == 0 || sample_at + count > n {
                return Err(invalid());
            }
            for sample in &mut samples[sample_at..sample_at + count] {
                sample.composition = offset;
            }
            sample_at += count;
        }
        if sample_at != n {
            return Err(invalid());
        }
    }
    if kind == TrackKind::Video {
        if let Some(stss) = optional(&stbl, b"stss")? {
            let count = u32_at(stss.data, 4)? as usize;
            if stss.data.len() != 8 + count * 4 || count > n || count == 0 {
                return Err(invalid());
            }
            let mut previous = 0;
            for i in 0..count {
                let number = u32_at(stss.data, 8 + i * 4)? as usize;
                if number <= previous || number > n {
                    return Err(invalid());
                }
                samples[number - 1].sync = true;
                previous = number;
            }
        } else {
            for sample in &mut samples {
                sample.sync = true;
            }
        }
    }
    let stco = optional(&stbl, b"stco")?;
    let co64 = optional(&stbl, b"co64")?;
    let (offsets, width) = match (stco, co64) {
        (Some(atom), None) => (atom.data, 4),
        (None, Some(atom)) => (atom.data, 8),
        _ => return Err(invalid()),
    };
    let chunks = u32_at(offsets, 4)? as usize;
    if chunks == 0 || chunks > n || offsets.len() != 8 + chunks * width {
        return Err(invalid());
    }
    let stsc = one(&stbl, b"stsc")?.data;
    let count = u32_at(stsc, 4)? as usize;
    if count == 0 || count > chunks || stsc.len() != 8 + count * 12 {
        return Err(invalid());
    }
    let mut mappings = Vec::with_capacity(count);
    let mut previous = 0;
    for i in 0..count {
        let first = u32_at(stsc, 8 + i * 12)? as usize;
        let per = u32_at(stsc, 12 + i * 12)? as usize;
        let description = u32_at(stsc, 16 + i * 12)?;
        if first <= previous
            || first > chunks
            || per == 0
            || description != 1
            || (i == 0 && first != 1)
        {
            return Err(invalid());
        }
        mappings.push((first, per));
        previous = first;
    }
    let mut map_at = 0;
    let mut sample_at = 0;
    for chunk in 1..=chunks {
        while map_at + 1 < mappings.len() && mappings[map_at + 1].0 <= chunk {
            map_at += 1;
        }
        let per = mappings[map_at].1;
        let mut offset = if width == 4 {
            u32_at(offsets, 8 + (chunk - 1) * 4)? as u64
        } else {
            u64_at(offsets, 8 + (chunk - 1) * 8)?
        };
        if sample_at + per > n {
            return Err(invalid());
        }
        for sample in &mut samples[sample_at..sample_at + per] {
            let end = offset.checked_add(sample.size as u64).ok_or_else(invalid)?;
            if !data_ranges
                .iter()
                .any(|&(start, stop)| offset >= start && end <= stop)
            {
                return Err(invalid());
            }
            sample.offset = offset;
            offset = end;
        }
        sample_at += per;
    }
    if sample_at != n {
        return Err(invalid());
    }
    let (media_start, empty_ms) = parse_edits(&children, movie_scale)?;
    let info = TrackInfo {
        id,
        kind: kind.clone(),
        codec,
        language,
        duration_ms: milliseconds(duration, scale)?,
        width: (kind == TrackKind::Video)
            .then(|| u32_at(&tkhd, tkhd.len() - 8).map(|n| n >> 16))
            .transpose()?,
        height: (kind == TrackKind::Video)
            .then(|| u32_at(&tkhd, tkhd.len() - 4).map(|n| n >> 16))
            .transpose()?,
    };
    Ok(Track {
        info,
        scale,
        media_start,
        empty_ms,
        tkhd,
        mdhd,
        hdlr,
        media_header,
        stsd: boxed(b"stsd", stsd_data),
        samples,
    })
}

fn parse_edits(children: &[Atom<'_>], movie_scale: u32) -> Result<(i64, u64), ClientError> {
    let Some(edts) = optional(children, b"edts")? else {
        return Ok((0, 0));
    };
    let edits = atoms(edts.data)?;
    let elst = one(&edits, b"elst")?.data;
    let count = u32_at(elst, 4)? as usize;
    let width = match elst.first() {
        Some(0) => 12,
        Some(1) => 20,
        _ => return Err(invalid()),
    };
    if count == 0 || count > 2 || elst.len() != 8 + count * width {
        return Err(unsupported(
            "Complex MP4 edit lists need a dedicated timeline mapping.",
        ));
    }
    let mut empty = 0;
    let mut media = None;
    for i in 0..count {
        let at = 8 + i * width;
        let (duration, start, rate) = if width == 12 {
            (
                u32_at(elst, at)? as u64,
                u32_at(elst, at + 4)? as i32 as i64,
                u32_at(elst, at + 8)?,
            )
        } else {
            (
                u64_at(elst, at)?,
                u64_at(elst, at + 8)? as i64,
                u32_at(elst, at + 16)?,
            )
        };
        if rate != 0x10000 {
            return Err(unsupported(
                "Variable-rate MP4 edits cannot be stream-copied by this engine.",
            ));
        }
        if start == -1 && i == 0 && count == 2 {
            empty = milliseconds(duration, movie_scale)?;
        } else if start >= 0 && media.is_none() {
            media = Some(start);
        } else {
            return Err(invalid());
        }
    }
    Ok((media.ok_or_else(invalid)?, empty))
}

impl Track {
    fn presentation_ms(&self, sample: &Sample) -> i128 {
        (sample.dts as i128 + sample.composition as i128 - self.media_start as i128) * 1000
            / self.scale as i128
            + self.empty_ms as i128
    }
}
impl Index {
    pub(super) fn info(&self) -> MediaInfo {
        MediaInfo {
            container: "mp4".into(),
            duration_ms: self.duration_ms,
            tracks: self.tracks.iter().map(|t| t.info.clone()).collect(),
            keyframes: "indexed".into(),
            warnings: self.warnings.clone(),
        }
    }
    pub(super) fn plan(&self, range: crate::ClipRange) -> Result<ClipPlan, ClientError> {
        let video = self
            .tracks
            .iter()
            .find(|t| t.info.kind == TrackKind::Video)
            .ok_or_else(invalid)?;
        let start = video
            .samples
            .iter()
            .enumerate()
            .filter(|(_, s)| s.sync && video.presentation_ms(s) <= range.start_ms as i128)
            .max_by_key(|(_, s)| video.presentation_ms(s))
            .map(|(i, _)| i)
            .ok_or_else(invalid)?;
        let aligned = video.presentation_ms(&video.samples[start]).max(0) as u64;
        let duration = range
            .end_ms
            .checked_sub(aligned)
            .filter(|&n| n > 0)
            .ok_or_else(invalid)?;
        let mut selected = Vec::new();
        for track in &self.tracks {
            if !matches!(track.info.kind, TrackKind::Video | TrackKind::Audio) {
                continue;
            }
            let first = if track.info.kind == TrackKind::Video {
                start
            } else {
                track
                    .samples
                    .iter()
                    .position(|s| {
                        track.presentation_ms(s)
                            + milliseconds(s.duration as u64, track.scale).unwrap_or(0) as i128
                            > aligned as i128
                    })
                    .ok_or_else(invalid)?
            };
            let last = track
                .samples
                .iter()
                .enumerate()
                .skip(first)
                .filter(|(_, s)| track.presentation_ms(s) < range.end_ms as i128)
                .map(|(i, _)| i)
                .next_back()
                .ok_or_else(invalid)?;
            if last < first {
                return Err(invalid());
            }
            let samples = track.samples[first..=last].to_vec();
            let media_start = (aligned as i128 - track.empty_ms as i128) * track.scale as i128
                / 1000
                + track.media_start as i128
                - samples[0].dts as i128;
            if media_start > i64::MAX as i128 {
                return Err(unsupported(
                    "A delayed track cannot be exported without preserving its empty edit.",
                ));
            }
            selected.push(Selection {
                track,
                samples,
                media_start: media_start.max(0) as u64,
                empty_ms: if media_start < 0 {
                    u64::try_from((-media_start) * 1000 / track.scale as i128)
                        .map_err(|_| invalid())?
                } else {
                    0
                },
                offsets: vec![0; last - first + 1],
            });
        }
        let header0 = self.build_header(&selected, duration, 0)?;
        let mut order = Vec::new();
        let mut total = 0u64;
        for (t, selection) in selected.iter().enumerate() {
            for (i, s) in selection.samples.iter().enumerate() {
                order.push((s.offset, t, i));
                total = total.checked_add(s.size as u64).ok_or_else(invalid)?;
            }
        }
        order.sort_unstable_by_key(|&(offset, _, _)| offset);
        let mut cursor = header0.len() as u64;
        let mut spans: Vec<ReadSpan> = Vec::new();
        for (offset, t, i) in order {
            let sample = &selected[t].samples[i];
            let size = sample.size as u64;
            selected[t].offsets[i] = cursor;
            cursor = cursor.checked_add(size).ok_or_else(invalid)?;
            if let Some(last) = spans
                .last_mut()
                .filter(|last| last.offset + last.length == offset)
            {
                last.length += size;
            } else {
                spans.push(ReadSpan {
                    offset,
                    length: size,
                    prefix: Vec::new(),
                });
            }
        }
        let header = self.build_header(&selected, duration, total)?;
        if header.len() != header0.len() {
            return Err(invalid());
        }
        let mut warnings = self.warnings.clone();
        warnings.push(
            "Stream copy starts at the previous video keyframe; it is not frame-exact.".into(),
        );
        Ok(ClipPlan {
            source_identity: self.identity.clone(),
            source_bytes: self.length,
            start_ms: range.start_ms,
            end_ms: range.end_ms,
            aligned_start_ms: aligned,
            output_duration_ms: duration,
            output_format: "mp4".into(),
            output_bytes: cursor,
            warnings,
            header,
            spans,
        })
    }
    fn build_header(
        &self,
        selected: &[Selection<'_>],
        duration_ms: u64,
        payload: u64,
    ) -> Result<Vec<u8>, ClientError> {
        let movie_duration = u64::try_from(duration_ms as u128 * self.movie_scale as u128 / 1000)
            .map_err(|_| invalid())?;
        let mvhd = boxed(
            b"mvhd",
            &replace_duration(&self.mvhd, movie_duration, false)?,
        );
        let mut moov = vec![mvhd];
        for selection in selected {
            moov.push(build_track(selection, movie_duration, self.movie_scale)?);
        }
        let mut header = boxed(b"ftyp", b"isom\0\0\x02\0isomiso2mp41");
        header.extend(concat_box(b"moov", &moov));
        header.extend_from_slice(&1u32.to_be_bytes());
        header.extend_from_slice(b"mdat");
        header.extend_from_slice(&payload.checked_add(16).ok_or_else(invalid)?.to_be_bytes());
        Ok(header)
    }
}

struct Selection<'a> {
    track: &'a Track,
    samples: Vec<Sample>,
    media_start: u64,
    empty_ms: u64,
    offsets: Vec<u64>,
}
fn replace_duration(data: &[u8], duration: u64, tkhd: bool) -> Result<Vec<u8>, ClientError> {
    let mut bytes = data.to_vec();
    let at = match (data.first(), tkhd) {
        (Some(0), false) => 16,
        (Some(1), false) => 24,
        (Some(0), true) => 20,
        (Some(1), true) => 28,
        _ => return Err(invalid()),
    };
    if data[0] == 0 {
        let duration = u32::try_from(duration)
            .map_err(|_| unsupported("The clip exceeds the version-0 MP4 timeline capacity."))?;
        bytes
            .get_mut(at..at + 4)
            .ok_or_else(invalid)?
            .copy_from_slice(&duration.to_be_bytes());
    } else {
        bytes
            .get_mut(at..at + 8)
            .ok_or_else(invalid)?
            .copy_from_slice(&duration.to_be_bytes());
    }
    Ok(bytes)
}
fn table(kind: &[u8; 4], version: u8, values: &[u8], count: u32) -> Vec<u8> {
    let mut payload = count.to_be_bytes().to_vec();
    payload.extend_from_slice(values);
    full(kind, version, &payload)
}
fn build_track(
    selection: &Selection<'_>,
    movie_duration: u64,
    movie_scale: u32,
) -> Result<Vec<u8>, ClientError> {
    let track = selection.track;
    let samples = &selection.samples;
    let first = samples.first().ok_or_else(invalid)?;
    let last = samples.last().ok_or_else(invalid)?;
    let media_duration = last
        .dts
        .checked_add(last.duration as u64)
        .and_then(|n| n.checked_sub(first.dts))
        .ok_or_else(invalid)?;
    let mut stts = Vec::new();
    let mut runs: Vec<(u32, u32)> = Vec::new();
    for s in samples {
        if let Some((count, delta)) = runs.last_mut().filter(|(_, delta)| *delta == s.duration) {
            *count += 1;
            let _ = delta;
        } else {
            runs.push((1, s.duration));
        }
    }
    for (count, delta) in &runs {
        stts.extend_from_slice(&count.to_be_bytes());
        stts.extend_from_slice(&delta.to_be_bytes());
    }
    let mut tables = vec![
        track.stsd.clone(),
        table(b"stts", 0, &stts, runs.len() as u32),
    ];
    if samples.iter().any(|s| s.composition != 0) {
        let signed = samples.iter().any(|s| s.composition < 0);
        let mut runs: Vec<(u32, i64)> = Vec::new();
        let mut bytes = Vec::new();
        for s in samples {
            if let Some((count, _)) = runs
                .last_mut()
                .filter(|(_, offset)| *offset == s.composition)
            {
                *count += 1;
            } else {
                runs.push((1, s.composition));
            }
        }
        for (count, offset) in &runs {
            bytes.extend_from_slice(&count.to_be_bytes());
            let word = if signed {
                i32::try_from(*offset).map_err(|_| invalid())? as u32
            } else {
                u32::try_from(*offset).map_err(|_| invalid())?
            };
            bytes.extend_from_slice(&word.to_be_bytes());
        }
        tables.push(table(b"ctts", u8::from(signed), &bytes, runs.len() as u32));
    }
    let mut stsc = Vec::new();
    for n in [1u32, 1, 1] {
        stsc.extend_from_slice(&n.to_be_bytes());
    }
    tables.push(table(b"stsc", 0, &stsc, 1));
    let mut sizes = vec![0; 4];
    sizes.extend_from_slice(&(samples.len() as u32).to_be_bytes());
    for s in samples {
        sizes.extend_from_slice(&s.size.to_be_bytes());
    }
    tables.push(full(b"stsz", 0, &sizes));
    let mut offsets = Vec::new();
    for n in &selection.offsets {
        offsets.extend_from_slice(&n.to_be_bytes());
    }
    tables.push(table(b"co64", 0, &offsets, samples.len() as u32));
    if track.info.kind == TrackKind::Video {
        let mut sync = Vec::new();
        let mut count = 0;
        for (i, s) in samples.iter().enumerate() {
            if s.sync {
                count += 1;
                sync.extend_from_slice(&((i + 1) as u32).to_be_bytes());
            }
        }
        tables.push(table(b"stss", 0, &sync, count));
    }
    let url = boxed(b"url ", &[0, 0, 0, 1]);
    let dinf = boxed(b"dinf", &table(b"dref", 0, &url, 1));
    let minf = concat_box(
        b"minf",
        &[
            track.media_header.clone(),
            dinf,
            concat_box(b"stbl", &tables),
        ],
    );
    let mdia = concat_box(
        b"mdia",
        &[
            boxed(
                b"mdhd",
                &replace_duration(&track.mdhd, media_duration, false)?,
            ),
            boxed(b"hdlr", &track.hdlr),
            minf,
        ],
    );
    let empty = u64::try_from(selection.empty_ms as u128 * movie_scale as u128 / 1000)
        .map_err(|_| invalid())?;
    if empty >= movie_duration {
        return Err(invalid());
    }
    let mut edit = Vec::new();
    if empty > 0 {
        edit.extend_from_slice(&empty.to_be_bytes());
        edit.extend_from_slice(&(-1i64).to_be_bytes());
        edit.extend_from_slice(&0x10000u32.to_be_bytes());
    }
    edit.extend_from_slice(&(movie_duration - empty).to_be_bytes());
    edit.extend_from_slice(&selection.media_start.to_be_bytes());
    edit.extend_from_slice(&0x10000u32.to_be_bytes());
    Ok(concat_box(
        b"trak",
        &[
            boxed(
                b"tkhd",
                &replace_duration(&track.tkhd, movie_duration, true)?,
            ),
            boxed(
                b"edts",
                &table(b"elst", 1, &edit, if empty > 0 { 2 } else { 1 }),
            ),
            mdia,
        ],
    ))
}

pub(super) fn validate_header(header: &[u8], output_bytes: u64) -> Result<(), ClientError> {
    if header.len() < 16 {
        return Err(invalid());
    }
    let prefix = &header[..header.len() - 16];
    let items = atoms(prefix)?;
    if items.len() != 2 || items[0].kind != *b"ftyp" || items[1].kind != *b"moov" {
        return Err(invalid());
    }
    let mdat = &header[header.len() - 16..];
    if u32_at(mdat, 0)? != 1
        || &mdat[4..8] != b"mdat"
        || u64_at(mdat, 8)? != output_bytes - prefix.len() as u64
    {
        return Err(invalid());
    }
    Ok(())
}

/// Generic indexed sample mux used by native TS/Matroska demuxers. The track
/// descriptions contain codec config only; payload spans are copied during render.
pub(super) fn remux(
    source: &dyn RangeSource,
    range: crate::ClipRange,
    aligned_ms: u64,
    input: Vec<MuxTrack>,
    warnings: Vec<String>,
) -> Result<ClipPlan, ClientError> {
    let duration_ms = range
        .end_ms
        .checked_sub(aligned_ms)
        .filter(|n| *n > 0)
        .ok_or_else(invalid)?;
    let mut tracks = Vec::new();
    let mut records = Vec::new();
    for t in input {
        if t.samples.is_empty() {
            return Err(invalid());
        }
        let first = t.samples[0].dts;
        let mut samples = Vec::new();
        let mut spans = Vec::new();
        for s in t.samples {
            let size = s
                .spans
                .iter()
                .try_fold(0u64, |n, p| {
                    n.checked_add(p.length)?.checked_add(p.prefix.len() as u64)
                })
                .ok_or_else(invalid)?;
            let dts = u64::try_from(s.dts - first).map_err(|_| invalid())?;
            samples.push(Sample {
                offset: 0,
                size: u32::try_from(size).map_err(|_| invalid())?,
                dts,
                duration: s.duration,
                composition: s.pts.checked_sub(s.dts).ok_or_else(invalid)?,
                sync: s.sync,
            });
            spans.push(s.spans);
        }
        let end = samples
            .last()
            .and_then(|s| s.dts.checked_add(s.duration as u64))
            .ok_or_else(invalid)?;
        let mut tkhd = vec![0; 96];
        tkhd[0] = 1;
        tkhd[3] = 7;
        tkhd[20..24].copy_from_slice(&t.info.id.to_be_bytes());
        if t.info.kind == TrackKind::Audio {
            tkhd[48..50].copy_from_slice(&0x100u16.to_be_bytes());
        }
        for (at, n) in [(52, 0x10000u32), (68, 0x10000), (84, 0x40000000)] {
            tkhd[at..at + 4].copy_from_slice(&n.to_be_bytes());
        }
        tkhd[88..92].copy_from_slice(&(t.info.width.unwrap_or(0) << 16).to_be_bytes());
        tkhd[92..96].copy_from_slice(&(t.info.height.unwrap_or(0) << 16).to_be_bytes());
        let mut mdhd = vec![0; 36];
        mdhd[0] = 1;
        mdhd[20..24].copy_from_slice(&t.scale.to_be_bytes());
        mdhd[24..32].copy_from_slice(&end.to_be_bytes());
        let lang = t.info.language.as_bytes();
        let code = if lang.len() == 3 && lang.iter().all(u8::is_ascii_lowercase) {
            ((lang[0] as u16 - 0x60) << 10)
                | ((lang[1] as u16 - 0x60) << 5)
                | (lang[2] as u16 - 0x60)
        } else {
            0x55c4
        };
        mdhd[32..34].copy_from_slice(&code.to_be_bytes());
        let mut hdlr = vec![0; 25];
        hdlr[8..12].copy_from_slice(if t.info.kind == TrackKind::Video {
            b"vide"
        } else {
            b"soun"
        });
        let media_header = if t.info.kind == TrackKind::Video {
            boxed(b"vmhd", &[0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0])
        } else {
            full(b"smhd", 0, &[0; 4])
        };
        let media_start = i128::from(aligned_ms) * t.scale as i128 / 1000 - first as i128;
        if media_start > i64::MAX as i128 {
            return Err(unsupported(
                "A delayed track needs an explicit empty edit before MP4 remuxing.",
            ));
        }
        let stsd = table(b"stsd", 0, &t.sample_entry, 1);
        tracks.push(Track {
            info: t.info,
            scale: t.scale,
            media_start: media_start.max(0) as i64,
            empty_ms: if media_start < 0 {
                u64::try_from((-media_start) * 1000 / t.scale as i128).map_err(|_| invalid())?
            } else {
                0
            },
            tkhd,
            mdhd,
            hdlr,
            media_header,
            stsd,
            samples,
        });
        records.push(spans);
    }
    let mut mvhd = vec![0; 112];
    mvhd[0] = 1;
    mvhd[20..24].copy_from_slice(&1000u32.to_be_bytes());
    mvhd[32..36].copy_from_slice(&0x10000u32.to_be_bytes());
    mvhd[36..38].copy_from_slice(&0x100u16.to_be_bytes());
    for (at, n) in [(48, 0x10000u32), (64, 0x10000), (80, 0x40000000)] {
        mvhd[at..at + 4].copy_from_slice(&n.to_be_bytes());
    }
    mvhd[108..112].copy_from_slice(&((tracks.len() + 1) as u32).to_be_bytes());
    let index = Index {
        identity: source.identity().into(),
        length: source.len(),
        mvhd,
        movie_scale: 1000,
        duration_ms,
        tracks,
        warnings: warnings.clone(),
    };
    let mut selected: Vec<_> = index
        .tracks
        .iter()
        .map(|t| Selection {
            track: t,
            samples: t.samples.clone(),
            media_start: t.media_start as u64,
            empty_ms: t.empty_ms,
            offsets: vec![0; t.samples.len()],
        })
        .collect();
    let preliminary = index.build_header(&selected, duration_ms, 0)?;
    let mut cursor = preliminary.len() as u64;
    let mut spans = Vec::new();
    let mut order = Vec::new();
    for (t, selection) in selected.iter().enumerate() {
        for (i, s) in selection.samples.iter().enumerate() {
            order.push((
                s.dts as u128 * 1_000_000_000 / selection.track.scale as u128,
                t,
                i,
            ));
        }
    }
    order.sort_unstable();
    for (_, t, i) in order {
        selected[t].offsets[i] = cursor;
        cursor = cursor
            .checked_add(selected[t].samples[i].size as u64)
            .ok_or_else(invalid)?;
        spans.extend(records[t][i].clone());
    }
    let header = index.build_header(&selected, duration_ms, cursor - preliminary.len() as u64)?;
    if header.len() != preliminary.len() {
        return Err(invalid());
    }
    Ok(ClipPlan {
        source_identity: source.identity().into(),
        source_bytes: source.len(),
        start_ms: range.start_ms,
        end_ms: range.end_ms,
        aligned_start_ms: aligned_ms,
        output_duration_ms: duration_ms,
        output_format: "mp4".into(),
        output_bytes: cursor,
        warnings,
        header,
        spans,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Bytes(Vec<u8>);
    impl RangeSource for Bytes {
        fn len(&self) -> u64 {
            self.0.len() as u64
        }
        fn identity(&self) -> &str {
            "test-media-v1"
        }
        fn read_exact(&self, offset: u64, length: usize) -> RangeFuture<'_> {
            Box::pin(async move {
                self.0
                    .get(offset as usize..offset as usize + length)
                    .map(|b| b.to_vec())
                    .ok_or_else(invalid)
            })
        }
    }
    #[tokio::test]
    async fn duplicate_oversized_truncated_and_fragmented_movie_boxes_fail_closed() {
        let ftyp = boxed(b"ftyp", b"isom\0\0\0\0");
        let mdat = boxed(b"mdat", &[1, 2, 3]);
        let duplicate = [
            ftyp.clone(),
            boxed(b"moov", &[]),
            boxed(b"moov", &[]),
            mdat.clone(),
        ]
        .concat();
        assert_eq!(
            probe(&Bytes(duplicate), Limits::default())
                .await
                .err()
                .unwrap()
                .code,
            ErrorCode::IntegrityFailed
        );
        let huge = [ftyp.clone(), boxed(b"moov", &[0; 128]), mdat.clone()].concat();
        assert!(probe(
            &Bytes(huge),
            Limits {
                max_metadata_bytes: 64,
                ..Limits::default()
            }
        )
        .await
        .is_err());
        let fragmented = [ftyp.clone(), boxed(b"moof", &[0; 8]), mdat.clone()].concat();
        assert_eq!(
            probe(&Bytes(fragmented), Limits::default())
                .await
                .err()
                .unwrap()
                .code,
            ErrorCode::Unsupported
        );
        for suffix in [
            vec![0, 0, 0, 4, b'm', b'o', b'o', b'v'],
            vec![
                0, 0, 0, 1, b'm', b'o', b'o', b'v', 255, 255, 255, 255, 255, 255, 255, 255,
            ],
        ] {
            assert!(
                probe(&Bytes([ftyp.clone(), suffix].concat()), Limits::default())
                    .await
                    .is_err()
            );
        }
        // A zero-size mdat legally extends to EOF; it cannot hide a second moov.
        assert!(probe(
            &Bytes([ftyp, vec![0, 0, 0, 0, b'm', b'd', b'a', b't'], vec![0; 32]].concat()),
            Limits::default()
        )
        .await
        .is_err());
    }
    #[test]
    fn malicious_sample_counts_do_not_allocate_before_validating_bounds() {
        let data = vec![0; 32];
        assert!(parse_track(&data, 1000, &[(0, 100)], Limits::default()).is_err());
        assert!(atoms(&[
            0, 0, 0, 1, b'f', b'r', b'e', b'e', 255, 255, 255, 255, 255, 255, 255, 255
        ])
        .is_err());
    }
}
