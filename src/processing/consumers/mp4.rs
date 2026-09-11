//! `MP4` consumer: walks the ISO base media file format box tree.

use super::data_source::ReaderDataSource;
use super::BlockConsumer;
use crate::processing::{BlockStreamReader, CancelToken, ProcessingError};
use std::any::Any;

/// Boxes whose payload is kept for the info provider.
const BOXES_WITH_DATA: &[&[u8; 4]] = &[
    b"mvhd", b"tkhd", b"mdhd", b"hdlr", b"vmhd", b"smhd", b"hmhd", b"dref", b"stsd", b"stts", b"ctts", b"stss", b"stsc", b"stsz",
    b"stco", b"co64", b"url ", b"urn ", b"cprt", b"ftyp", b"mehd", b"tfhd", b"mfro", b"nmhd", b"padb", b"frma", b"pitm", b"pdin",
    b"sgpd", b"schi", b"schm", b"stsh", b"subs", b"trun", b"stz2",
];

/// Container boxes (children follow immediately after the header).
const CONTAINER_BOXES: &[&[u8; 4]] = &[
    b"moov", b"trak", b"edts", b"mdia", b"minf", b"dinf", b"stbl", b"mvex", b"moof", b"traf", b"mfra", b"udta", b"tref", b"ipro", b"sinf",
    b"schi", b"skip", b"strk", b"strd", b"rinf",
];

/// Containers that carry a 4-byte version/flags prefix before their children.
const FULL_CONTAINER_BOXES: &[&[u8; 4]] = &[b"meta"];

const MAX_DATA_LENGTH: u64 = 16 << 20;

#[derive(Debug, Clone, Default)]
pub struct Mp4Node {
    pub box_type: [u8; 4],
    pub size: u64,
    pub data: Vec<u8>,
    pub children: Vec<Mp4Node>,
}

impl Mp4Node {
    pub fn type_str(&self) -> String {
        String::from_utf8_lossy(&self.box_type).into_owned()
    }

    pub fn is_type(&self, t: &[u8; 4]) -> bool {
        &self.box_type == t
    }

    /// Breadth-first search for all descendants of the given type.
    pub fn descendants<'a>(&'a self, t: &'a [u8; 4]) -> Vec<&'a Mp4Node> {
        let mut out = Vec::new();
        let mut queue: std::collections::VecDeque<&Mp4Node> = self.children.iter().collect();
        while let Some(cur) = queue.pop_front() {
            if cur.is_type(t) {
                out.push(cur);
            }
            queue.extend(cur.children.iter());
        }
        out
    }

    pub fn first_descendant<'a>(&'a self, t: &'a [u8; 4]) -> Option<&'a Mp4Node> {
        self.descendants(t).into_iter().next()
    }
}

struct Mp4Reader<'r, 'a> {
    src: &'r mut ReaderDataSource<'a>,
}

impl<'r, 'a> Mp4Reader<'r, 'a> {
    /// Read boxes until `end` (or end of stream) into `parent`.
    fn read_children(&mut self, parent: &mut Mp4Node, end: Option<u64>, ct: &CancelToken, depth: usize) -> Result<(), ProcessingError> {
        loop {
            ct.check()?;
            let pos = self.src.position();
            if let Some(e) = end {
                if pos + 8 > e {
                    break;
                }
            }
            if self.src.is_end_of_stream() {
                break;
            }
            let (mut size, box_type, mut header_len) = {
                let block = self.src.get(16)?;
                if block.len() < 8 {
                    break;
                }
                let size = u32::from_be_bytes([block[0], block[1], block[2], block[3]]) as u64;
                let mut bt = [0u8; 4];
                bt.copy_from_slice(&block[4..8]);
                let mut header_len = 8usize;
                let size = if size == 1 {
                    if block.len() < 16 {
                        break;
                    }
                    header_len = 16;
                    u64::from_be_bytes(block[8..16].try_into().unwrap())
                } else {
                    size
                };
                (size, bt, header_len)
            };
            if &box_type == b"uuid" {
                header_len += 16;
            }
            if size == 0 {
                // Box extends to the end of the file.
                size = end.unwrap_or(self.src.length()).saturating_sub(pos);
            }
            if size < header_len as u64 {
                break; // corrupt
            }
            if let Some(e) = end {
                if pos + size > e {
                    break;
                }
            }
            self.src.advance(header_len);
            let data_len = size - header_len as u64;
            let box_end = pos + size;
            let mut child = Mp4Node { box_type, size: data_len, ..Default::default() };

            if depth < 16 && CONTAINER_BOXES.contains(&&box_type) {
                self.read_children(&mut child, Some(box_end), ct, depth + 1)?;
            } else if depth < 16 && FULL_CONTAINER_BOXES.contains(&&box_type) {
                self.src.skip(4.min(data_len))?;
                self.read_children(&mut child, Some(box_end), ct, depth + 1)?;
            } else if BOXES_WITH_DATA.contains(&&box_type) && data_len <= MAX_DATA_LENGTH && data_len as usize <= self.src.max_request() {
                child.data = self.src.read_exact(data_len as usize)?;
            }
            parent.children.push(child);
            if !self.src.seek_forward(box_end)? {
                break;
            }
        }
        Ok(())
    }
}

pub struct Mp4Parser {
    name: String,
    root: Option<Mp4Node>,
}

impl Mp4Parser {
    pub fn new(name: &str) -> Self {
        Self { name: name.to_string(), root: None }
    }
    pub fn root_box(&self) -> Option<&Mp4Node> {
        self.root.as_ref()
    }
}

impl BlockConsumer for Mp4Parser {
    fn name(&self) -> &str {
        &self.name
    }

    fn do_work(&mut self, reader: &mut BlockStreamReader, ct: &CancelToken) -> Result<(), ProcessingError> {
        let length = reader.length();
        {
            let head = reader.get_block(12)?;
            // Any ISO-BMFF file starts with a box header; require a plausible first box type.
            let plausible = head.len() >= 8 && head[4..8].iter().all(|b| b.is_ascii_graphic() || *b == b' ');
            if !plausible {
                reader.set_active(false);
                return Ok(());
            }
        }
        let mut src = ReaderDataSource::new(reader);
        let mut root = Mp4Node { box_type: *b"root", size: length, ..Default::default() };
        let mut r = Mp4Reader { src: &mut src };
        match r.read_children(&mut root, None, ct, 0) {
            Ok(()) => {}
            Err(e) if e.is_cancelled() => return Err(e),
            Err(_) => {}
        }
        if root.first_descendant(b"ftyp").is_some() || root.first_descendant(b"moov").is_some() {
            self.root = Some(root);
        }
        Ok(())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// `ftyp`
pub struct FileTypeBox {
    pub major_brand: String,
    pub minor_version: u32,
    pub compatible_brands: Vec<String>,
}

impl FileTypeBox {
    pub fn parse(d: &[u8]) -> Option<Self> {
        if d.len() < 8 {
            return None;
        }
        let mut brands = Vec::new();
        let mut i = 8;
        while i + 4 <= d.len() {
            brands.push(String::from_utf8_lossy(&d[i..i + 4]).into_owned());
            i += 4;
        }
        Some(Self { major_brand: String::from_utf8_lossy(&d[0..4]).into_owned(), minor_version: be32(d, 4), compatible_brands: brands })
    }
}

fn be32(d: &[u8], o: usize) -> u32 {
    u32::from_be_bytes([d[o], d[o + 1], d[o + 2], d[o + 3]])
}
fn be64(d: &[u8], o: usize) -> u64 {
    u64::from_be_bytes(d[o..o + 8].try_into().unwrap())
}
fn be16(d: &[u8], o: usize) -> u16 {
    u16::from_be_bytes([d[o], d[o + 1]])
}

/// Seconds between 1904-01-01 and 1970-01-01.
const MP4_EPOCH_OFFSET: f64 = 2_082_844_800.0;

fn mp4_time(v: u64) -> Option<f64> {
    if v == 0 { None } else { Some(v as f64 - MP4_EPOCH_OFFSET) }
}

/// `mvhd`
pub struct MovieHeaderBox {
    pub creation_date: Option<f64>,
    pub modification_date: Option<f64>,
    pub timescale: u32,
    pub duration: u64,
}

impl MovieHeaderBox {
    pub fn parse(d: &[u8]) -> Option<Self> {
        if d.len() < 4 {
            return None;
        }
        let version = d[0];
        if version == 1 {
            if d.len() < 32 {
                return None;
            }
            Some(Self { creation_date: mp4_time(be64(d, 4)), modification_date: mp4_time(be64(d, 12)), timescale: be32(d, 20), duration: be64(d, 24) })
        } else {
            if d.len() < 20 {
                return None;
            }
            Some(Self {
                creation_date: mp4_time(be32(d, 4) as u64),
                modification_date: mp4_time(be32(d, 8) as u64),
                timescale: be32(d, 12),
                duration: be32(d, 16) as u64,
            })
        }
    }
}

/// `tkhd`
pub struct TrackHeaderBox {
    pub creation_date: Option<f64>,
    pub modification_date: Option<f64>,
    pub track_id: u32,
    pub duration: u64,
    /// 16.16 fixed point converted to integer pixels.
    pub width: u32,
    pub height: u32,
}

impl TrackHeaderBox {
    pub fn parse(d: &[u8]) -> Option<Self> {
        if d.len() < 4 {
            return None;
        }
        let version = d[0];
        let (creation, modification, track_id, duration, rest) = if version == 1 {
            if d.len() < 36 {
                return None;
            }
            (mp4_time(be64(d, 4)), mp4_time(be64(d, 12)), be32(d, 20), be64(d, 28), 36)
        } else {
            if d.len() < 24 {
                return None;
            }
            (mp4_time(be32(d, 4) as u64), mp4_time(be32(d, 8) as u64), be32(d, 12), be32(d, 20) as u64, 24)
        };
        // reserved(8) layer(2) alt_group(2) volume(2) reserved(2) matrix(36) width(4) height(4)
        let w_off = rest + 8 + 2 + 2 + 2 + 2 + 36;
        if d.len() < w_off + 8 {
            return None;
        }
        Some(Self { creation_date: creation, modification_date: modification, track_id, duration, width: be32(d, w_off) >> 16, height: be32(d, w_off + 4) >> 16 })
    }
}

/// `mdhd`
pub struct MediaHeaderBox {
    pub creation_date: Option<f64>,
    pub modification_date: Option<f64>,
    pub timescale: u32,
    pub duration: u64,
    pub language: String,
}

impl MediaHeaderBox {
    pub fn parse(d: &[u8]) -> Option<Self> {
        if d.len() < 4 {
            return None;
        }
        let version = d[0];
        let (creation, modification, timescale, duration, rest) = if version == 1 {
            if d.len() < 32 {
                return None;
            }
            (mp4_time(be64(d, 4)), mp4_time(be64(d, 12)), be32(d, 20), be64(d, 24), 32)
        } else {
            if d.len() < 20 {
                return None;
            }
            (mp4_time(be32(d, 4) as u64), mp4_time(be32(d, 8) as u64), be32(d, 12), be32(d, 16) as u64, 20)
        };
        let language = if d.len() >= rest + 2 {
            let packed = be16(d, rest);
            let c = |shift: u16| (((packed >> shift) & 0x1F) + 0x60) as u8 as char;
            format!("{}{}{}", c(10), c(5), c(0))
        } else {
            String::new()
        };
        Some(Self { creation_date: creation, modification_date: modification, timescale, duration, language })
    }
}

/// `hdlr`
pub struct HandlerBox {
    pub handler_type: String,
}

impl HandlerBox {
    pub fn parse(d: &[u8]) -> Option<Self> {
        if d.len() < 12 {
            return None;
        }
        Some(Self { handler_type: String::from_utf8_lossy(&d[8..12]).into_owned() })
    }
}

/// One `stsd` visual sample entry.
pub struct VisualSampleEntry {
    pub format: String,
    pub width: u16,
    pub height: u16,
    pub horizontal_resolution: u32,
    pub vertical_resolution: u32,
    pub frame_count: u16,
}

/// `stsd`
pub struct SampleDescriptionBox {
    pub entries: Vec<(String, Vec<u8>)>,
}

impl SampleDescriptionBox {
    pub fn parse(d: &[u8]) -> Option<Self> {
        if d.len() < 8 {
            return None;
        }
        let count = be32(d, 4) as usize;
        let mut entries = Vec::new();
        let mut off = 8usize;
        for _ in 0..count {
            if off + 8 > d.len() {
                break;
            }
            let size = be32(d, off) as usize;
            if size < 8 || off + size > d.len() {
                break;
            }
            let format = String::from_utf8_lossy(&d[off + 4..off + 8]).into_owned();
            entries.push((format, d[off + 8..off + size].to_vec()));
            off += size;
        }
        Some(Self { entries })
    }

    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    pub fn video_entry(&self, index: usize) -> Option<VisualSampleEntry> {
        let (format, body) = self.entries.get(index)?;
        // 6 reserved + 2 data_reference_index + 2 pre_defined + 2 reserved + 12 pre_defined = 24
        if body.len() < 24 + 2 + 2 + 4 + 4 + 4 + 2 {
            return None;
        }
        Some(VisualSampleEntry {
            format: format.clone(),
            width: be16(body, 24),
            height: be16(body, 26),
            horizontal_resolution: be32(body, 28) >> 16,
            vertical_resolution: be32(body, 32) >> 16,
            frame_count: be16(body, 40),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ftyp() {
        let f = FileTypeBox::parse(b"isom\0\0\x02\0isomiso2mp41").unwrap();
        assert_eq!(f.major_brand, "isom");
        assert_eq!(f.minor_version, 512);
        assert_eq!(f.compatible_brands, vec!["isom", "iso2", "mp41"]);
    }

    #[test]
    fn parses_mvhd_v0() {
        let mut d = vec![0u8; 100];
        d[12..16].copy_from_slice(&1000u32.to_be_bytes());
        d[16..20].copy_from_slice(&5000u32.to_be_bytes());
        let m = MovieHeaderBox::parse(&d).unwrap();
        assert_eq!(m.timescale, 1000);
        assert_eq!(m.duration, 5000);
        assert!(m.creation_date.is_none());
    }
}
