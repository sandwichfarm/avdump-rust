//! `MKV` consumer: parses the Matroska structure while the file streams past.

pub mod cluster;
pub mod ebml;
pub mod ids;
pub mod sections;

use super::data_source::ReaderDataSource;
use super::BlockConsumer;
use crate::misc::xml::XElement;
use crate::processing::{BlockStreamReader, CancelToken, ProcessingError};
use cluster::ClusterSection;
use ebml::EbmlReader;
use sections::*;
use std::any::Any;

/// Parsed Matroska file.
#[derive(Debug, Clone, Default)]
pub struct MatroskaFile {
    pub section_size: u64,
    pub ebml_header: EbmlHeaderSection,
    pub segment: Option<SegmentSection>,
    last_file_position: u64,
}

#[derive(Debug, Clone, Default)]
pub struct SegmentSection {
    pub section_size: Option<u64>,
    pub segment_info: Option<SegmentInfoSection>,
    pub attachments: Option<AttachmentsSection>,
    pub chapters: Option<ChaptersSection>,
    pub cluster: ClusterSection,
    pub tracks: Option<TracksSection>,
    pub tags: Vec<TagsSection>,
    pub cues: Option<CuesSection>,
    pub seek_head: Option<SeekHeadSection>,
}

impl SegmentSection {
    fn process_element(&mut self, reader: &mut EbmlReader<'_, '_>, id: u32) -> Result<bool, ProcessingError> {
        match id {
            ids::CLUSTER => self.cluster.read(reader)?,
            ids::TRACKS if self.tracks.is_none() => {
                let tracks = TracksSection::read(reader)?;
                self.cluster.add_tracks(&tracks.items);
                self.tracks = Some(tracks);
            }
            ids::INFO => {
                let info = SegmentInfoSection::read(reader)?;
                self.cluster.timecode_scale = info.timecode_scale();
                self.segment_info = Some(info);
            }
            ids::CHAPTERS => self.chapters = Some(ChaptersSection::read(reader)?),
            ids::ATTACHMENTS => self.attachments = Some(AttachmentsSection::read(reader)?),
            ids::TAGS => self.tags.push(TagsSection::read(reader)?),
            ids::SEEK_HEAD => self.seek_head = Some(SeekHeadSection::read(reader)?),
            ids::CUES => self.cues = Some(CuesSection::read(reader)?),
            _ => return Ok(false),
        }
        Ok(true)
    }

    /// Read all children of the current `Segment` element.
    fn read_into(&mut self, reader: &mut EbmlReader<'_, '_>, ct: &CancelToken) -> Result<(), ProcessingError> {
        self.section_size = reader.current().and_then(|h| h.size);
        reader.enter();
        let result = (|| -> Result<(), ProcessingError> {
            while let Some(h) = reader.next()? {
                ct.check()?;
                let _ = self.process_element(reader, h.id);
            }
            Ok(())
        })();
        reader.leave()?;
        result
    }

    /// Continue reading top-level elements when the `Segment` element itself was not found
    /// (the current element is the first known child).
    fn continue_read_into(&mut self, reader: &mut EbmlReader<'_, '_>, ct: &CancelToken) -> Result<(), ProcessingError> {
        let mut cur = reader.current().map(|h| h.id);
        while let Some(id) = cur {
            ct.check()?;
            let _ = self.process_element(reader, id);
            cur = reader.next()?.map(|h| h.id);
        }
        Ok(())
    }

    pub fn to_xml(&self) -> XElement {
        let mut e = XElement::new("Segment");
        e.add(self.segment_info.as_ref().map(|s| s.to_xml()).unwrap_or_else(|| XElement::new("SegmentInfo")));
        e.add(self.tracks.as_ref().map(|s| s.to_xml()).unwrap_or_else(|| XElement::new("Tracks")));
        e.add(self.chapters.as_ref().map(|s| s.to_xml()).unwrap_or_else(|| XElement::new("Chapters")));
        for t in &self.tags {
            e.add(t.to_xml());
        }
        e.add(self.attachments.as_ref().map(|s| s.to_xml()).unwrap_or_else(|| XElement::new("Attachments")));
        e
    }
}

impl MatroskaFile {
    pub fn new(file_size: u64) -> Self {
        Self { section_size: file_size, ..Default::default() }
    }

    pub fn has_meta_data(&self) -> bool {
        let segment = match &self.segment {
            Some(s) => s,
            None => return false,
        };
        let mut valid = segment.segment_info.is_some() && segment.tracks.is_some();
        if let Some(seg_size) = segment.section_size {
            if seg_size == 0 {
                return false;
            }
            let ratio = |a: u64| a as f64 / seg_size as f64;
            valid = valid && self.section_size.saturating_sub(seg_size) < (1 << 20) && ratio(self.section_size) < 1.01;
            valid = valid && self.last_file_position.saturating_sub(seg_size) < (1 << 20) && ratio(self.last_file_position) < 1.01;
        }
        valid
    }

    fn parse(&mut self, reader: &mut EbmlReader<'_, '_>, ct: &CancelToken) -> Result<(), ProcessingError> {
        reader.strict = true;
        match reader.next()? {
            Some(h) if h.id == ids::EBML => self.ebml_header = EbmlHeaderSection::read(reader)?,
            _ => return Ok(()),
        }
        reader.strict = false;

        let mut cur = reader.next()?;
        while let Some(h) = cur {
            if h.id == ids::SEGMENT || h.id == ids::INFO {
                break;
            }
            if reader.position() > 4 * 1024 * 1024 {
                break;
            }
            cur = reader.next()?;
        }
        ct.check()?;

        let mut segment = SegmentSection::default();
        let result = match cur {
            Some(h) if h.id == ids::SEGMENT => segment.read_into(reader, ct),
            Some(h) if h.id == ids::INFO => segment.continue_read_into(reader, ct),
            _ => return Ok(()),
        };
        self.segment = Some(segment);
        self.last_file_position = reader.position();
        result
    }

    pub fn to_xml(&self) -> XElement {
        let mut e = XElement::new("File");
        e.add(self.ebml_header.to_xml());
        e.add(self.segment.as_ref().map(|s| s.to_xml()).unwrap_or_else(|| XElement::new("Segment")));
        e
    }
}

pub struct MatroskaParser {
    name: String,
    info: Option<MatroskaFile>,
}

impl MatroskaParser {
    pub fn new(name: &str) -> Self {
        Self { name: name.to_string(), info: None }
    }
    pub fn info(&self) -> Option<&MatroskaFile> {
        self.info.as_ref()
    }
}

impl BlockConsumer for MatroskaParser {
    fn name(&self) -> &str {
        &self.name
    }

    fn do_work(&mut self, reader: &mut BlockStreamReader, ct: &CancelToken) -> Result<(), ProcessingError> {
        let length = reader.length();
        if length < 4 {
            reader.set_active(false);
            return Ok(());
        }
        {
            let head = reader.get_block(4)?;
            if head.len() < 4 || u32::from_be_bytes([head[0], head[1], head[2], head[3]]) != 0x1A45DFA3 {
                reader.set_active(false);
                return Ok(());
            }
        }
        let mut src = ReaderDataSource::new(reader);
        let mut ebml = EbmlReader::new(&mut src, ids::unknown_size_terminator);
        let mut file = MatroskaFile::new(length);
        match file.parse(&mut ebml, ct) {
            Ok(()) => {}
            Err(e) if e.is_cancelled() => return Err(e),
            Err(_) => { /* not a (complete) matroska file: keep what we have */ }
        }
        if file.segment.is_some() {
            self.info = Some(file);
        }
        Ok(())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
