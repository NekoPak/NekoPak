//! Minimal decoder-configuration parsing. No video/audio decoding or encoding.
use super::*;
pub(super) struct VideoConfig {
    pub entry: Vec<u8>,
    pub width: u32,
    pub height: u32,
}
fn boxed(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let mut out = ((payload.len() + 8) as u32).to_be_bytes().to_vec();
    out.extend_from_slice(kind);
    out.extend_from_slice(payload);
    out
}
struct Bits {
    bytes: Vec<u8>,
    at: usize,
}
impl Bits {
    fn rbsp(bytes: &[u8]) -> Self {
        let mut out = Vec::new();
        let mut zero = 0;
        for &b in bytes {
            if zero >= 2 && b == 3 {
                zero = 0;
                continue;
            }
            out.push(b);
            zero = if b == 0 { zero + 1 } else { 0 };
        }
        Self { bytes: out, at: 0 }
    }
    fn bits(&mut self, n: usize) -> Result<u64, ClientError> {
        if n > 64
            || self
                .at
                .checked_add(n)
                .is_none_or(|end| end > self.bytes.len() * 8)
        {
            return Err(invalid());
        }
        let mut v = 0;
        for _ in 0..n {
            v = (v << 1) | ((self.bytes[self.at / 8] >> (7 - self.at % 8)) & 1) as u64;
            self.at += 1;
        }
        Ok(v)
    }
    fn skip(&mut self, n: usize) -> Result<(), ClientError> {
        for _ in 0..n {
            self.bits(1)?;
        }
        Ok(())
    }
    fn ue(&mut self) -> Result<u32, ClientError> {
        let mut n = 0;
        while self.bits(1)? == 0 {
            n += 1;
            if n > 30 {
                return Err(invalid());
            }
        }
        Ok(((1u64 << n) - 1 + self.bits(n)?) as u32)
    }
    fn se(&mut self) -> Result<i32, ClientError> {
        let n = self.ue()?;
        Ok(if n & 1 == 1 {
            n.div_ceil(2) as i32
        } else {
            -((n / 2) as i32)
        })
    }
}
pub(super) fn avc(sps: &[u8], pps: &[u8]) -> Result<VideoConfig, ClientError> {
    if sps.len() < 4 || sps.len() > 65535 || pps.len() > 65535 {
        return Err(invalid());
    }
    let mut b = Bits::rbsp(&sps[1..]);
    let profile = b.bits(8)? as u8;
    b.skip(16)?;
    b.ue()?;
    let mut chroma = 1;
    let mut separate = false;
    if matches!(
        profile,
        100 | 110 | 122 | 244 | 44 | 83 | 86 | 118 | 128 | 138 | 139 | 134 | 135
    ) {
        chroma = b.ue()?;
        if chroma > 3 {
            return Err(invalid());
        }
        if chroma == 3 {
            separate = b.bits(1)? != 0;
        }
        b.ue()?;
        b.ue()?;
        b.skip(1)?;
        if b.bits(1)? != 0 {
            for i in 0..if chroma == 3 { 12 } else { 8 } {
                if b.bits(1)? != 0 {
                    let mut last = 8;
                    let mut next = 8;
                    for _ in 0..if i < 6 { 16 } else { 64 } {
                        if next != 0 {
                            next = (last + b.se()? + 256) % 256;
                        }
                        if next != 0 {
                            last = next;
                        }
                    }
                }
            }
        }
    }
    b.ue()?;
    match b.ue()? {
        0 => {
            b.ue()?;
        }
        1 => {
            b.skip(1)?;
            b.se()?;
            b.se()?;
            let n = b.ue()?;
            if n > 256 {
                return Err(invalid());
            }
            for _ in 0..n {
                b.se()?;
            }
        }
        2 => {}
        _ => return Err(invalid()),
    };
    b.ue()?;
    b.skip(1)?;
    let wm = b.ue()?.checked_add(1).ok_or_else(invalid)?;
    let hm = b.ue()?.checked_add(1).ok_or_else(invalid)?;
    let progressive = b.bits(1)? != 0;
    if !progressive {
        b.skip(1)?;
    }
    b.skip(1)?;
    let mut crop = [0; 4];
    if b.bits(1)? != 0 {
        for n in &mut crop {
            *n = b.ue()?;
        }
    }
    let chroma = if separate { 0 } else { chroma };
    let (sx, sy) = match chroma {
        0 => (1, 1),
        1 => (2, 2),
        2 => (2, 1),
        3 => (1, 1),
        _ => return Err(invalid()),
    };
    let factor = if progressive { 1 } else { 2 };
    let width = wm
        .checked_mul(16)
        .and_then(|v| v.checked_sub((crop[0] + crop[1]) * sx))
        .ok_or_else(invalid)?;
    let height = hm
        .checked_mul(16 * factor)
        .and_then(|v| v.checked_sub((crop[2] + crop[3]) * sy * factor))
        .ok_or_else(invalid)?;
    let mut config = vec![1, sps[1], sps[2], sps[3], 255, 225];
    config.extend_from_slice(&(sps.len() as u16).to_be_bytes());
    config.extend(sps);
    config.push(1);
    config.extend_from_slice(&(pps.len() as u16).to_be_bytes());
    config.extend(pps);
    Ok(VideoConfig {
        entry: video_entry(b"avc1", width, height, boxed(b"avcC", &config))?,
        width,
        height,
    })
}
pub(super) fn hevc(vps: &[u8], sps: &[u8], pps: &[u8]) -> Result<VideoConfig, ClientError> {
    if [vps, sps, pps]
        .iter()
        .any(|b| b.len() < 3 || b.len() > 65535)
    {
        return Err(invalid());
    }
    let mut b = Bits::rbsp(&sps[2..]);
    b.skip(4)?;
    let layers = b.bits(3)? as usize;
    let nested = b.bits(1)? as u8;
    let profile = b.bits(8)? as u8;
    let compatibility = b.bits(32)? as u32;
    let constraint = b.bits(48)?;
    let level = b.bits(8)? as u8;
    let mut sub = Vec::new();
    for _ in 0..layers {
        sub.push((b.bits(1)? != 0, b.bits(1)? != 0));
    }
    if layers > 0 {
        b.skip((8 - layers) * 2)?;
    }
    for (p, l) in sub {
        if p {
            b.skip(88)?;
        }
        if l {
            b.skip(8)?;
        }
    }
    b.ue()?;
    let chroma = b.ue()?;
    if chroma > 3 {
        return Err(invalid());
    }
    let separate = chroma == 3 && b.bits(1)? != 0;
    let width = b.ue()?;
    let height = b.ue()?;
    let mut crop = [0u32; 4];
    if b.bits(1)? != 0 {
        for n in &mut crop {
            *n = b.ue()?;
        }
    }
    let depth_l = b.ue()?;
    let depth_c = b.ue()?;
    if depth_l > 7 || depth_c > 7 {
        return Err(invalid());
    }
    let chroma = if separate { 0 } else { chroma };
    let (sx, sy) = match chroma {
        1 => (2, 2),
        2 => (2, 1),
        _ => (1, 1),
    };
    let width = width
        .checked_sub((crop[0] + crop[1]) * sx)
        .ok_or_else(invalid)?;
    let height = height
        .checked_sub((crop[2] + crop[3]) * sy)
        .ok_or_else(invalid)?;
    let mut c = vec![1, profile];
    c.extend_from_slice(&compatibility.to_be_bytes());
    c.extend_from_slice(&constraint.to_be_bytes()[2..]);
    c.push(level);
    c.extend_from_slice(&[
        0xf0,
        0,
        0xfc,
        0xfc | chroma as u8,
        0xf8 | depth_l as u8,
        0xf8 | depth_c as u8,
        0,
        0,
        ((layers as u8 + 1) << 3) | (nested << 2) | 3,
        3,
    ]);
    for (ty, nal) in [(32, vps), (33, sps), (34, pps)] {
        c.push(0x80 | ty);
        c.extend_from_slice(&1u16.to_be_bytes());
        c.extend_from_slice(&(nal.len() as u16).to_be_bytes());
        c.extend(nal);
    }
    Ok(VideoConfig {
        entry: video_entry(b"hvc1", width, height, boxed(b"hvcC", &c))?,
        width,
        height,
    })
}
fn video_entry(
    kind: &[u8; 4],
    width: u32,
    height: u32,
    config: Vec<u8>,
) -> Result<Vec<u8>, ClientError> {
    if width == 0 || height == 0 || width > 65535 || height > 65535 {
        return Err(invalid());
    }
    let mut entry = vec![0; 78];
    entry[6..8].copy_from_slice(&1u16.to_be_bytes());
    entry[24..26].copy_from_slice(&(width as u16).to_be_bytes());
    entry[26..28].copy_from_slice(&(height as u16).to_be_bytes());
    entry[28..32].copy_from_slice(&0x00480000u32.to_be_bytes());
    entry[32..36].copy_from_slice(&0x00480000u32.to_be_bytes());
    entry[40..42].copy_from_slice(&1u16.to_be_bytes());
    entry[74..76].copy_from_slice(&24u16.to_be_bytes());
    entry[76..78].copy_from_slice(&65535u16.to_be_bytes());
    entry.extend(config);
    Ok(boxed(kind, &entry))
}

pub(super) fn video_private(
    codec: &str,
    width: u32,
    height: u32,
    config: &[u8],
) -> Result<Vec<u8>, ClientError> {
    let (kind, box_kind) = match codec {
        "V_MPEG4/ISO/AVC" => (b"avc1", b"avcC"),
        "V_MPEGH/ISO/HEVC" => (b"hvc1", b"hvcC"),
        _ => {
            return Err(unsupported(
                "This Matroska video codec cannot be remuxed into MP4.",
            ))
        }
    };
    if config.len() < 7 || config.len() > 65535 || config[0] != 1 {
        return Err(invalid());
    }
    video_entry(kind, width, height, boxed(box_kind, config))
}

pub(super) fn aac_private(config: &[u8]) -> Result<(Vec<u8>, u32), ClientError> {
    if config.len() < 2 {
        return Err(invalid());
    }
    let bits = u16::from_be_bytes([config[0], config[1]]);
    let object = (bits >> 11) as u8;
    if !(1..=4).contains(&object) {
        return Err(unsupported(
            "This AAC profile needs a complete AudioSpecificConfig parser.",
        ));
    }
    aac(
        object - 1,
        ((bits >> 7) & 15) as u8,
        ((bits >> 3) & 15) as u8,
    )
}
fn descriptor(tag: u8, data: &[u8]) -> Result<Vec<u8>, ClientError> {
    if data.len() > 127 {
        return Err(invalid());
    }
    let mut d = vec![tag, data.len() as u8];
    d.extend(data);
    Ok(d)
}
pub(super) fn aac(
    profile: u8,
    sample_index: u8,
    channels: u8,
) -> Result<(Vec<u8>, u32), ClientError> {
    const RATES: [u32; 13] = [
        96000, 88200, 64000, 48000, 44100, 32000, 24000, 22050, 16000, 12000, 11025, 8000, 7350,
    ];
    let rate = *RATES.get(sample_index as usize).ok_or_else(invalid)?;
    if channels == 0 || channels > 7 || profile > 3 || rate > 65535 {
        return Err(unsupported("AAC requires a supported AudioSpecificConfig."));
    }
    let asc =
        (((profile as u16 + 1) << 11) | ((sample_index as u16) << 7) | ((channels as u16) << 3))
            .to_be_bytes();
    let dec = descriptor(5, &asc)?;
    let mut decoder = vec![0x40, 0x15, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    decoder.extend(dec);
    let mut es = vec![0, 1, 0];
    es.extend(descriptor(4, &decoder)?);
    es.extend(descriptor(6, &[2])?);
    let mut esds = vec![0; 4];
    esds.extend(descriptor(3, &es)?);
    let mut entry = vec![0; 28];
    entry[6..8].copy_from_slice(&1u16.to_be_bytes());
    entry[16..18].copy_from_slice(&(channels as u16).to_be_bytes());
    entry[18..20].copy_from_slice(&16u16.to_be_bytes());
    entry[24..28].copy_from_slice(&(rate << 16).to_be_bytes());
    entry.extend(boxed(b"esds", &esds));
    Ok((boxed(b"mp4a", &entry), rate))
}
