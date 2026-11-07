//! Bounded ISO-BMFF container sanity check, not a full decoder/integrity certification.
use crate::{integrity, ClientError};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
};
pub fn validate(file: &mut File) -> Result<u64, ClientError> {
    let length = file.metadata().map_err(|_| integrity())?.len();
    let mut offset = 0u64;
    let mut ftyp = false;
    let mut moov = false;
    let mut mdat = false;
    let mut duration = None;
    for _ in 0..4096 {
        if offset == length {
            break;
        }
        let (kind, start, end) = header(file, offset, length)?;
        match &kind {
            b"ftyp" => ftyp = true,
            b"moov" => {
                if mdat || end - start > 16 * 1024 * 1024 {
                    return Err(integrity());
                }
                moov = true;
                duration = movie_duration(file, start, end)?;
            }
            b"mdat" => {
                if end <= start {
                    return Err(integrity());
                }
                mdat = true;
            }
            _ => {}
        }
        offset = end;
    }
    if offset != length || !ftyp || !moov || !mdat {
        return Err(integrity());
    }
    duration.filter(|value| *value > 0).ok_or_else(integrity)
}
fn header(file: &mut File, offset: u64, bound: u64) -> Result<([u8; 4], u64, u64), ClientError> {
    if bound.saturating_sub(offset) < 8 {
        return Err(integrity());
    }
    file.seek(SeekFrom::Start(offset))
        .map_err(|_| integrity())?;
    let mut bytes = [0; 8];
    file.read_exact(&mut bytes).map_err(|_| integrity())?;
    let size = u32::from_be_bytes(bytes[..4].try_into().unwrap()) as u64;
    let kind = bytes[4..].try_into().unwrap();
    let (size, header) = match size {
        0 => (bound - offset, 8),
        1 => {
            let mut large = [0; 8];
            file.read_exact(&mut large).map_err(|_| integrity())?;
            (u64::from_be_bytes(large), 16)
        }
        _ => (size, 8),
    };
    let end = offset.checked_add(size).ok_or_else(integrity)?;
    if size < header || end > bound {
        return Err(integrity());
    }
    Ok((kind, offset + header, end))
}
fn movie_duration(
    file: &mut File,
    mut offset: u64,
    bound: u64,
) -> Result<Option<u64>, ClientError> {
    for _ in 0..4096 {
        if offset == bound {
            return Ok(None);
        }
        let (kind, start, end) = header(file, offset, bound)?;
        if &kind == b"mvhd" {
            if end - start < 20 {
                return Err(integrity());
            }
            file.seek(SeekFrom::Start(start)).map_err(|_| integrity())?;
            let mut fields = [0u8; 32];
            let count = (end - start).min(32) as usize;
            file.read_exact(&mut fields[..count])
                .map_err(|_| integrity())?;
            let (scale, duration) = match fields[0] {
                0 => (
                    u32::from_be_bytes(fields[12..16].try_into().unwrap()) as u64,
                    u32::from_be_bytes(fields[16..20].try_into().unwrap()) as u64,
                ),
                1 if count >= 32 => (
                    u32::from_be_bytes(fields[20..24].try_into().unwrap()) as u64,
                    u64::from_be_bytes(fields[24..32].try_into().unwrap()),
                ),
                _ => return Err(integrity()),
            };
            if scale == 0 {
                return Err(integrity());
            }
            return Ok(Some(
                u64::try_from(duration as u128 * 1000 / scale as u128).map_err(|_| integrity())?,
            ));
        }
        offset = end;
    }
    Err(integrity())
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    #[test]
    fn rejects_truncated_and_invalid_boxes() {
        for data in [
            b"not an mp4".as_slice(),
            b"\0\0\0\x04ftyp".as_slice(),
            b"\xff\xff\xff\xffmdat".as_slice(),
        ] {
            let mut file = tempfile::tempfile().unwrap();
            file.write_all(data).unwrap();
            assert!(validate(&mut file).is_err());
        }
    }
}
