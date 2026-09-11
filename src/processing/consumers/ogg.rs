//! `OGG` consumer: walks Ogg pages and collects per-bitstream information.

use super::data_source::ReaderDataSource;
use super::BlockConsumer;
use crate::processing::{BlockStreamReader, CancelToken, ProcessingError};
use std::any::Any;
use std::collections::HashMap;

pub const PAGE_FLAG_SPAN_BEFORE: u32 = 1;
pub const PAGE_FLAG_HEADER: u32 = 2;
pub const PAGE_FLAG_FOOTER: u32 = 4;
pub const PAGE_FLAG_SPAN_AFTER: u32 = 1 << 31;

/// One Ogg page (header fields plus the page payload).
#[derive(Debug, Clone, Default)]
pub struct OggPage {
    pub flags: u32,
    pub version: u8,
    pub granule_position: i64,
    pub stream_id: u32,
    pub page_index: u32,
    pub checksum: [u8; 4],
    pub segment_count: u8,
    pub packet_offsets: Vec<usize>,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BitStreamKind {
    Video,
    Audio,
    Subtitle,
    Unknown,
}

/// A logical bitstream inside the Ogg container.
#[derive(Debug, Clone)]
pub struct OggBitStream {
    pub id: u32,
    pub size: i64,
    pub last_granule_position: i64,
    pub kind: BitStreamKind,
    pub codec_name: String,
    pub codec_version: Option<String>,
    pub is_officially_supported: bool,
    /// Four-character code of OGM streams.
    pub actual_codec_name: Option<String>,
    // audio
    pub sample_rate: f64,
    pub channel_count: i32,
    // video
    pub frame_rate: f64,
    pub width: i32,
    pub height: i32,
    comment_parser: Option<VorbisCommentParser>,
}

impl OggBitStream {
    fn base(id: u32, kind: BitStreamKind, codec_name: &str, official: bool, comments: bool) -> Self {
        Self {
            id,
            size: 0,
            last_granule_position: 0,
            kind,
            codec_name: codec_name.to_string(),
            codec_version: None,
            is_officially_supported: official,
            actual_codec_name: None,
            sample_rate: 0.0,
            channel_count: 0,
            frame_rate: 0.0,
            width: 0,
            height: 0,
            comment_parser: if comments { Some(VorbisCommentParser::default()) } else { None },
        }
    }

    pub fn sample_count(&self) -> i64 {
        self.last_granule_position
    }
    pub fn frame_count(&self) -> i64 {
        self.last_granule_position
    }
    /// Duration in seconds (audio: samples / rate, video: frames / fps).
    pub fn duration(&self) -> f64 {
        match self.kind {
            BitStreamKind::Audio => self.sample_count() as f64 / self.sample_rate,
            BitStreamKind::Video => self.frame_count() as f64 / self.frame_rate,
            _ => 0.0,
        }
    }
    pub fn has_comments(&self) -> bool {
        self.comment_parser.is_some()
    }
    pub fn comments(&self) -> Option<Comments> {
        self.comment_parser.as_ref().and_then(|p| p.retrieve_comments())
    }

    fn process_begin_page(page: &OggPage) -> Self {
        let d = &page.data;
        let tag = |from: usize, to: usize| -> String { String::from_utf8_lossy(&d[from.min(d.len())..to.min(d.len())]).into_owned() };
        let mut bs = if d.len() >= 29 && tag(1, 6) == "video" {
            let mut s = Self::base(page.stream_id, BitStreamKind::Video, "OGMVideo", false, true);
            s.actual_codec_name = Some(tag(9, 13));
            let time_unit = i64::from_le_bytes(d[17..25].try_into().unwrap());
            s.frame_rate = 10_000_000f64 / time_unit as f64;
            s.width = i32::from_le_bytes(d[45..49].try_into().unwrap());
            s.height = i32::from_le_bytes(d[49..53].try_into().unwrap());
            s
        } else if d.len() >= 46 && tag(1, 6) == "audio" {
            let mut s = Self::base(page.stream_id, BitStreamKind::Audio, "OGMAudio", false, true);
            s.actual_codec_name = Some(tag(9, 13));
            s.sample_rate = i64::from_le_bytes(d[25..33].try_into().unwrap()) as f64;
            s.channel_count = if d.len() >= 47 { i16::from_le_bytes([d[45], d[46]]) as i32 } else { 0 };
            s
        } else if d.len() >= 0x39 && tag(1, 5) == "text" {
            let mut s = Self::base(page.stream_id, BitStreamKind::Subtitle, "OGMText", false, true);
            s.actual_codec_name = Some(tag(9, 13));
            s
        } else if d.len() >= 42 && tag(1, 7) == "theora" {
            let mut s = Self::base(page.stream_id, BitStreamKind::Video, "Theora", true, false);
            s.codec_version = Some(format!("{}.{}.{}", d[7], d[8], d[9]));
            s.width = ((d[14] as i32) << 16) | ((d[15] as i32) << 8) | d[16] as i32;
            s.height = ((d[17] as i32) << 16) | ((d[18] as i32) << 8) | d[19] as i32;
            let frn = u32::from_be_bytes([d[22], d[23], d[24], d[25]]) as f64;
            let frd = u32::from_be_bytes([d[26], d[27], d[28], d[29]]) as f64;
            s.frame_rate = frn / frd;
            s
        } else if d.len() >= 30 && tag(1, 7) == "vorbis" {
            let mut s = Self::base(page.stream_id, BitStreamKind::Audio, "Vorbis", true, true);
            let version = u32::from_le_bytes([d[7], d[8], d[9], d[10]]);
            s.channel_count = d[11] as i32;
            s.sample_rate = u32::from_le_bytes([d[12], d[13], d[14], d[15]]) as f64;
            s.codec_version = Some(version.to_string());
            s
        } else if d.len() >= 79 && tag(1, 5) == "FLAC" {
            let mut s = Self::base(page.stream_id, BitStreamKind::Audio, "Flac", true, false);
            s.sample_rate = (((d[33] as u32) << 12) | ((d[34] as u32) << 4) | ((d[35] as u32 & 0xF0) >> 4)) as f64;
            s.channel_count = (((d[35] & 0x0E) >> 1) + 1) as i32;
            s
        } else {
            Self::base(page.stream_id, BitStreamKind::Unknown, "Unknown", false, false)
        };
        bs.id = page.stream_id;
        bs
    }

    fn process_page(&mut self, page: &OggPage) {
        let gp = page.granule_position;
        if gp > self.last_granule_position && gp < self.last_granule_position + 10_000_000 {
            self.last_granule_position = gp;
        }
        self.size += page.data.len() as i64;
        if let Some(p) = &mut self.comment_parser {
            p.parse_page(page);
        }
    }
}

#[derive(Debug, Clone, Default)]
struct VorbisCommentParser {
    fully_read: bool,
    contains_comments: bool,
    data: Vec<u8>,
}

impl VorbisCommentParser {
    const HEADER: &'static [u8] = b"\x03vorbis";

    fn parse_page(&mut self, page: &OggPage) {
        if self.fully_read {
            return;
        }
        if !self.contains_comments && page.data.windows(Self::HEADER.len()).any(|w| w == Self::HEADER) {
            self.contains_comments = true;
        }
        if !self.contains_comments {
            return;
        }
        self.data.extend_from_slice(&page.data);
        if page.flags & PAGE_FLAG_SPAN_AFTER == 0 {
            self.fully_read = true;
        }
    }

    fn retrieve_comments(&self) -> Option<Comments> {
        if self.contains_comments {
            Comments::parse(&self.data)
        } else {
            Some(Comments::default())
        }
    }
}

/// Vorbis comment block.
#[derive(Debug, Clone, Default)]
pub struct Comments {
    pub vendor: String,
    /// Key (lower case) → values, in first-seen order.
    pub items: Vec<(String, Vec<String>)>,
}

impl Comments {
    pub fn get(&self, key: &str) -> Option<&Vec<String>> {
        self.items.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    pub fn parse(b: &[u8]) -> Option<Self> {
        let start = b.windows(7).position(|w| w == VorbisCommentParser::HEADER)?;
        let mut offset = start + 7;
        let read_u32 = |o: usize| -> Option<u32> { b.get(o..o + 4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]])) };
        let mut c = Comments::default();
        let len = read_u32(offset)? as usize;
        offset += 4;
        c.vendor = String::from_utf8_lossy(b.get(offset..offset + len)?).into_owned();
        offset += len;
        let count = read_u32(offset)? as usize;
        offset += 4;
        for _ in 0..count {
            let len = read_u32(offset)? as usize;
            offset += 4;
            let comment = String::from_utf8_lossy(b.get(offset..offset + len)?).into_owned();
            offset += len;
            let (key, value) = match comment.find('=') {
                Some(p) => (comment[..p].to_lowercase(), comment[p + 1..].to_string()),
                None => ("undefined".to_string(), comment),
            };
            match c.items.iter_mut().find(|(k, _)| *k == key) {
                Some((_, values)) => values.push(value),
                None => c.items.push((key, vec![value])),
            }
        }
        Some(c)
    }
}

/// Aggregated information about an Ogg file.
#[derive(Debug, Clone, Default)]
pub struct OggFile {
    pub file_size: i64,
    pub overhead: i64,
    bitstreams: Vec<OggBitStream>,
    index: HashMap<u32, usize>,
}

impl OggFile {
    pub fn bitstreams(&self) -> &[OggBitStream] {
        &self.bitstreams
    }

    pub fn process_page(&mut self, page: &OggPage) {
        self.overhead += 27 + page.segment_count as i64;
        if let Some(&i) = self.index.get(&page.stream_id) {
            self.bitstreams[i].process_page(page);
        } else if page.flags & PAGE_FLAG_HEADER != 0 {
            let bs = OggBitStream::process_begin_page(page);
            self.index.insert(bs.id, self.bitstreams.len());
            self.bitstreams.push(bs);
        } else {
            self.overhead += page.data.len() as i64;
        }
    }
}

const OGGS: &[u8] = b"OggS";
const MAX_PAGE_LEN: usize = 27 + 255 + 255 * 255;

/// Locate the next "OggS" sync marker. Skips at most `max_skippable_bytes` of garbage.
fn seek_past_sync_bytes(src: &mut ReaderDataSource<'_>, advance: bool, max_skippable_bytes: usize) -> Result<bool, ProcessingError> {
    let mut skipped = 0usize;
    loop {
        let (found, block_len) = {
            let block = src.get(src.reader().suggested_read_length())?;
            (block.windows(4).position(|w| w == OGGS), block.len())
        };
        if let Some(offset) = found {
            if skipped + offset > max_skippable_bytes {
                return Ok(false);
            }
            if advance {
                src.advance(offset + 4);
            }
            return Ok(true);
        }
        if block_len < 4 {
            return Ok(false);
        }
        let step = block_len - 3;
        skipped += step;
        if skipped > max_skippable_bytes {
            return Ok(false);
        }
        if !src.skip(step as u64)? {
            return Ok(false);
        }
    }
}

fn read_ogg_page(src: &mut ReaderDataSource<'_>, page: &mut OggPage) -> Result<bool, ProcessingError> {
    if !seek_past_sync_bytes(src, true, 1 << 20)? {
        return Ok(false);
    }
    let (header_len, data_len) = {
        let block = src.get(MAX_PAGE_LEN)?;
        if block.len() < 23 {
            return Ok(false);
        }
        page.version = block[0];
        page.flags = block[1] as u32;
        page.granule_position = i64::from_le_bytes(block[2..10].try_into().unwrap());
        page.stream_id = u32::from_le_bytes(block[10..14].try_into().unwrap());
        page.page_index = u32::from_le_bytes(block[14..18].try_into().unwrap());
        page.checksum.copy_from_slice(&block[18..22]);
        let segment_count = block[22] as usize;
        page.segment_count = block[22];
        if block.len() < 23 + segment_count {
            return Ok(false);
        }
        let mut data_len = 0usize;
        page.packet_offsets.clear();
        for i in 0..segment_count {
            let seg = block[23 + i] as usize;
            data_len += seg;
            if seg != 255 {
                page.packet_offsets.push(data_len);
            }
        }
        if segment_count > 0 && block[23 + segment_count - 1] == 255 {
            page.flags |= PAGE_FLAG_SPAN_AFTER;
        }
        let header_len = 23 + segment_count;
        let avail = data_len.min(block.len().saturating_sub(header_len));
        page.data.clear();
        page.data.extend_from_slice(&block[header_len..header_len + avail]);
        (header_len, avail)
    };
    src.advance(header_len + data_len);
    Ok(true)
}

pub struct OggParser {
    name: String,
    info: Option<OggFile>,
}

impl OggParser {
    pub fn new(name: &str) -> Self {
        Self { name: name.to_string(), info: None }
    }
    pub fn info(&self) -> Option<&OggFile> {
        self.info.as_ref()
    }
}

impl BlockConsumer for OggParser {
    fn name(&self) -> &str {
        &self.name
    }

    fn do_work(&mut self, reader: &mut BlockStreamReader, ct: &CancelToken) -> Result<(), ProcessingError> {
        let mut info = OggFile { file_size: reader.length() as i64, ..Default::default() };
        let mut src = ReaderDataSource::new(reader);
        if !seek_past_sync_bytes(&mut src, false, 0)? {
            src.reader().set_active(false);
            return Ok(());
        }
        let mut page = OggPage::default();
        while read_ogg_page(&mut src, &mut page)? {
            ct.check()?;
            info.process_page(&page);
        }
        self.info = Some(info);
        Ok(())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_vorbis_comments() {
        let mut b = b"\x03vorbis".to_vec();
        let vendor = b"test";
        b.extend_from_slice(&(vendor.len() as u32).to_le_bytes());
        b.extend_from_slice(vendor);
        b.extend_from_slice(&2u32.to_le_bytes());
        for c in [&b"TITLE=Hello"[..], &b"title=World"[..]] {
            b.extend_from_slice(&(c.len() as u32).to_le_bytes());
            b.extend_from_slice(c);
        }
        let c = Comments::parse(&b).unwrap();
        assert_eq!(c.vendor, "test");
        assert_eq!(c.get("title").unwrap(), &vec!["Hello".to_string(), "World".to_string()]);
    }
}
