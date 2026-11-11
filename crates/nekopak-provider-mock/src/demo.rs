//! Richer fixtures for the interactive offline demo: a small tree that can be
//! searched and reorganised in memory, plus a handful of pictures drawn at
//! run time so previews work without any network or bundled binary.
use crate::FIXTURE_TIME;
use nekopak_domain::*;

/// The fixture video: listed and sized, but without decodable content.
pub const SAMPLE_VIDEO: &str = "file-1";
/// Folder that holds the generated pictures.
pub const PHOTOS: &str = "demo-photos";

struct Picture {
    id: &'static str,
    name: &'static str,
    width: u32,
    height: u32,
    top: [u8; 3],
    bottom: [u8; 3],
    glow: [u8; 3],
    /// Glow centre and radius as fractions of the picture.
    at: (f32, f32, f32),
}

const PICTURES: [Picture; 6] = [
    Picture {
        id: "demo-photo-1",
        name: "晨雾.png",
        width: 480,
        height: 320,
        top: [180, 205, 232],
        bottom: [238, 222, 214],
        glow: [255, 244, 214],
        at: (0.68, 0.34, 0.42),
    },
    Picture {
        id: "demo-photo-2",
        name: "海岸.png",
        width: 480,
        height: 320,
        top: [34, 86, 142],
        bottom: [120, 196, 204],
        glow: [236, 250, 244],
        at: (0.3, 0.78, 0.5),
    },
    Picture {
        id: "demo-photo-3",
        name: "黄昏.png",
        width: 480,
        height: 320,
        top: [58, 44, 96],
        bottom: [240, 142, 92],
        glow: [255, 214, 140],
        at: (0.5, 0.72, 0.36),
    },
    Picture {
        id: "demo-photo-4",
        name: "森林 portrait.png",
        width: 320,
        height: 480,
        top: [28, 70, 62],
        bottom: [150, 190, 120],
        glow: [224, 240, 170],
        at: (0.4, 0.3, 0.5),
    },
    Picture {
        id: "demo-photo-5",
        name: "夜空.png",
        width: 480,
        height: 320,
        top: [12, 18, 44],
        bottom: [50, 60, 120],
        glow: [190, 200, 255],
        at: (0.74, 0.26, 0.22),
    },
    Picture {
        id: "demo-photo-6",
        name: "樱花.png",
        width: 400,
        height: 400,
        top: [250, 220, 230],
        bottom: [232, 160, 186],
        glow: [255, 250, 252],
        at: (0.32, 0.36, 0.46),
    },
];

fn entry(id: &str, parent: &str, name: &str, kind: EntryKind, size: u64) -> RemoteEntry {
    RemoteEntry {
        id: id.into(),
        parent_id: Some(parent.into()),
        name: name.into(),
        kind,
        size_bytes: DecimalU64(size),
        modified_at: Some(FIXTURE_TIME.into()),
        etag: Some("mock-v1".into()),
        hash: None,
        mime_type: None,
        starred: None,
    }
}

/// Entries added on top of the contract fixtures for the interactive demo.
pub fn extra_entries() -> Vec<RemoteEntry> {
    let mut out = vec![entry(PHOTOS, "root", "照片", EntryKind::Folder, 0)];
    for picture in &PICTURES {
        let mut item = entry(
            picture.id,
            PHOTOS,
            picture.name,
            EntryKind::File,
            picture_len(picture) as u64,
        );
        item.mime_type = Some("image/png".into());
        out.push(item);
    }
    out.push(entry(
        "demo-course",
        "documents",
        "课程",
        EntryKind::Folder,
        0,
    ));
    out.push(entry(
        "demo-notes",
        "demo-course",
        "第一章 笔记.txt",
        EntryKind::File,
        18_432,
    ));
    out.push(entry(
        "demo-archive",
        "documents",
        "资料合集.zip",
        EntryKind::File,
        734_003_200,
    ));
    out.push(entry(
        "demo-song",
        "documents",
        "片头曲.mp3",
        EntryKind::File,
        8_912_896,
    ));
    out[2].starred = Some(true);
    out
}

fn picture(id: &str) -> Option<&'static Picture> {
    PICTURES.iter().find(|picture| picture.id == id)
}

/// Stored (uncompressed) deflate keeps the encoder tiny and its size exact.
fn picture_len(picture: &Picture) -> usize {
    let raw = (picture.width as usize * 3 + 1) * picture.height as usize;
    let blocks = raw.div_ceil(65_535);
    8 + 25 + (12 + 2 + raw + blocks * 5 + 4) + 12
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xedb8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], body: &[u8]) {
    out.extend_from_slice(&(body.len() as u32).to_be_bytes());
    let start = out.len();
    out.extend_from_slice(kind);
    out.extend_from_slice(body);
    let crc = crc32(&out[start..]);
    out.extend_from_slice(&crc.to_be_bytes());
}

/// PNG bytes of a generated picture; `None` for every other entry.
pub fn picture_bytes(id: &str) -> Option<Vec<u8>> {
    let picture = picture(id)?;
    let (width, height) = (picture.width as usize, picture.height as usize);
    let mut raw = Vec::with_capacity((width * 3 + 1) * height);
    let (cx, cy, radius) = picture.at;
    let aspect = width as f32 / height as f32;
    for y in 0..height {
        raw.push(0);
        let v = y as f32 / (height - 1) as f32;
        for x in 0..width {
            let u = x as f32 / (width - 1) as f32;
            let distance = (((u - cx) * aspect).powi(2) + (v - cy).powi(2)).sqrt();
            let glow = (1.0 - distance / radius).clamp(0.0, 1.0).powi(2);
            // A second, fainter band keeps the picture from looking flat.
            let band = (0.5 + 0.5 * ((u * 2.4 + v * 3.1) * std::f32::consts::PI).sin()) * 0.06;
            for channel in 0..3 {
                let base = picture.top[channel] as f32
                    + (picture.bottom[channel] as f32 - picture.top[channel] as f32) * v;
                let value = base
                    + (picture.glow[channel] as f32 - base) * glow * 0.85
                    + (255.0 - base) * band;
                raw.push(value.clamp(0.0, 255.0) as u8);
            }
        }
    }
    let mut data = Vec::with_capacity(raw.len() + raw.len() / 65_535 * 5 + 16);
    data.extend_from_slice(&[0x78, 0x01]);
    let mut blocks = raw.chunks(65_535).peekable();
    while let Some(block) = blocks.next() {
        data.push(u8::from(blocks.peek().is_none()));
        data.extend_from_slice(&(block.len() as u16).to_le_bytes());
        data.extend_from_slice(&(!(block.len() as u16)).to_le_bytes());
        data.extend_from_slice(block);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for byte in &raw {
        a = (a + u32::from(*byte)) % 65_521;
        b = (b + a) % 65_521;
    }
    data.extend_from_slice(&((b << 16) | a).to_be_bytes());
    let mut out = Vec::with_capacity(picture_len(picture));
    out.extend_from_slice(b"\x89PNG\r\n\x1a\n");
    let mut header = Vec::with_capacity(13);
    header.extend_from_slice(&picture.width.to_be_bytes());
    header.extend_from_slice(&picture.height.to_be_bytes());
    header.extend_from_slice(&[8, 2, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &header);
    chunk(&mut out, b"IDAT", &data);
    chunk(&mut out, b"IEND", &[]);
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_pictures_match_their_listed_size() {
        for picture in &PICTURES {
            let bytes = picture_bytes(picture.id).unwrap();
            assert_eq!(bytes.len(), picture_len(picture), "{}", picture.name);
            assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
        }
        assert!(picture_bytes("file-1").is_none());
    }

    #[test]
    fn crc_matches_the_png_reference_value() {
        assert_eq!(crc32(b"IEND"), 0xae42_6082);
    }
}
