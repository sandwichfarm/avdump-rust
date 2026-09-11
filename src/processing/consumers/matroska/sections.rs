//! Matroska section model and parsing (EBML header, segment info, tracks, chapters, tags, ...).

use super::ebml::EbmlReader;
use super::ids::*;
use crate::misc::xml::XElement;
use crate::processing::ProcessingError;

type R<T> = Result<T, ProcessingError>;

fn hex(b: &[u8]) -> String {
    crate::hashes::to_hex_upper(b)
}

fn bin_elem(name: &str, b: Option<&[u8]>) -> Option<XElement> {
    let b = b?;
    let mut e = XElement::new(name).attr("Size", b.len().to_string());
    if !b.is_empty() {
        let shown = &b[..b.len().min(1024)];
        e.set_text(format!("{}{}", hex(shown), if b.len() > 1024 { "..." } else { "" }));
    }
    Some(e)
}

fn opt_elem<T: ToString>(name: &str, v: Option<T>) -> XElement {
    let mut e = XElement::new(name);
    if let Some(v) = v {
        e.set_text(v.to_string());
    }
    e
}

fn elem<T: ToString>(name: &str, v: T) -> XElement {
    XElement::with_text(name, v.to_string())
}

fn f64_str(v: f64) -> String {
    crate::info::value::format_f64(v)
}

/// Read children of the current master element, dispatching each to `f`. Unhandled children are
/// skipped automatically.
fn read_children(reader: &mut EbmlReader<'_, '_>, mut f: impl FnMut(&mut EbmlReader<'_, '_>, u32) -> R<bool>) -> R<()> {
    reader.enter();
    let result = (|| -> R<()> {
        while let Some(h) = reader.next()? {
            let _ = f(reader, h.id);
        }
        Ok(())
    })();
    reader.leave()?;
    result
}

fn section_size(reader: &EbmlReader<'_, '_>) -> Option<u64> {
    reader.current().and_then(|h| h.size)
}

// ---------------------------------------------------------------- EBML header

#[derive(Debug, Clone, Default)]
pub struct EbmlHeaderSection {
    pub section_size: Option<u64>,
    ebml_version: Option<u64>,
    ebml_read_version: Option<u64>,
    ebml_max_id_length: Option<u64>,
    ebml_max_size_length: Option<u64>,
    doc_type: Option<String>,
    doc_type_version: Option<u64>,
    doc_type_read_version: Option<u64>,
}

impl EbmlHeaderSection {
    pub fn ebml_version(&self) -> u64 {
        self.ebml_version.unwrap_or(1)
    }
    pub fn ebml_read_version(&self) -> u64 {
        self.ebml_read_version.unwrap_or(1)
    }
    pub fn ebml_max_id_length(&self) -> u64 {
        self.ebml_max_id_length.unwrap_or(4)
    }
    pub fn ebml_max_size_length(&self) -> u64 {
        self.ebml_max_size_length.unwrap_or(8)
    }
    pub fn doc_type_read_version(&self) -> u64 {
        self.doc_type_read_version.unwrap_or(1)
    }
    pub fn doc_type_version(&self) -> u64 {
        self.doc_type_version.unwrap_or(1)
    }
    pub fn doc_type(&self) -> &str {
        self.doc_type.as_deref().unwrap_or("matroska")
    }

    pub fn read(reader: &mut EbmlReader<'_, '_>) -> R<Self> {
        let mut s = Self { section_size: section_size(reader), ..Default::default() };
        read_children(reader, |r, id| {
            match id {
                DOC_TYPE => s.doc_type = Some(r.read_string()?),
                DOC_TYPE_READ_VERSION => s.doc_type_read_version = Some(r.read_uint()?),
                DOC_TYPE_VERSION => s.doc_type_version = Some(r.read_uint()?),
                EBML_MAX_ID_LENGTH => s.ebml_max_id_length = Some(r.read_uint()?),
                EBML_MAX_SIZE_LENGTH => s.ebml_max_size_length = Some(r.read_uint()?),
                EBML_READ_VERSION => s.ebml_read_version = Some(r.read_uint()?),
                EBML_VERSION => s.ebml_version = Some(r.read_uint()?),
                _ => return Ok(false),
            }
            Ok(true)
        })?;
        Ok(s)
    }

    pub fn to_xml(&self) -> XElement {
        let mut e = XElement::new("EbmlHeader");
        e.add(elem("EbmlVersion", self.ebml_version()));
        e.add(elem("EbmlReadVersion", self.ebml_read_version()));
        e.add(elem("EbmlMaxIdLength", self.ebml_max_id_length()));
        e.add(elem("EbmlMaxSizeLength", self.ebml_max_size_length()));
        e.add(elem("DocTypeReadVersion", self.doc_type_read_version()));
        e.add(elem("DocTypeVersion", self.doc_type_version()));
        e.add(elem("DocType", self.doc_type()));
        e
    }
}

// ---------------------------------------------------------------- Segment info

#[derive(Debug, Clone, Default)]
pub struct ChapterTranslateSection {
    pub edition_uids: Vec<u64>,
    pub codec: Option<u64>,
    pub id: Option<Vec<u8>>,
}

impl ChapterTranslateSection {
    fn read(reader: &mut EbmlReader<'_, '_>) -> R<Self> {
        let mut s = Self::default();
        read_children(reader, |r, id| {
            match id {
                CHAPTER_TRANSLATE_EDITION_UID => s.edition_uids.push(r.read_uint()?),
                CHAPTER_TRANSLATE_CODEC => s.codec = Some(r.read_uint()?),
                CHAPTER_TRANSLATE_ID => s.id = Some(r.read_binary()?),
                _ => return Ok(false),
            }
            Ok(true)
        })?;
        Ok(s)
    }
}

#[derive(Debug, Clone, Default)]
pub struct SegmentInfoSection {
    pub section_size: Option<u64>,
    timecode_scale: Option<u64>,
    pub segment_family: Vec<Vec<u8>>,
    pub segment_uid: Option<Vec<u8>>,
    pub previous_uid: Option<Vec<u8>>,
    pub next_uid: Option<Vec<u8>>,
    pub segment_filename: Option<String>,
    pub previous_filename: Option<String>,
    pub next_filename: Option<String>,
    pub duration: Option<f64>,
    pub title: Option<String>,
    pub muxing_app: Option<String>,
    pub writing_app: Option<String>,
    /// Unix timestamp (seconds) of DateUTC.
    pub production_date: Option<f64>,
    pub chapter_translate: Vec<ChapterTranslateSection>,
}

impl SegmentInfoSection {
    pub fn timecode_scale(&self) -> u64 {
        self.timecode_scale.unwrap_or(1_000_000)
    }

    pub fn read(reader: &mut EbmlReader<'_, '_>) -> R<Self> {
        let mut s = Self { section_size: section_size(reader), ..Default::default() };
        read_children(reader, |r, id| {
            match id {
                SEGMENT_UID => s.segment_uid = Some(r.read_binary()?),
                SEGMENT_FILENAME => s.segment_filename = Some(r.read_string()?),
                PREV_UID => s.previous_uid = Some(r.read_binary()?),
                PREV_FILENAME => s.previous_filename = Some(r.read_string()?),
                NEXT_UID => s.next_uid = Some(r.read_binary()?),
                NEXT_FILENAME => s.next_filename = Some(r.read_string()?),
                SEGMENT_FAMILY => s.segment_family.push(r.read_binary()?),
                CHAPTER_TRANSLATE => s.chapter_translate.push(ChapterTranslateSection::read(r)?),
                TIMECODE_SCALE => s.timecode_scale = Some(r.read_uint()?),
                DURATION => s.duration = Some(r.read_float()?),
                TITLE => s.title = Some(r.read_string()?),
                MUXING_APP => s.muxing_app = Some(r.read_string()?),
                WRITING_APP => s.writing_app = Some(r.read_string()?),
                DATE_UTC => s.production_date = Some(r.read_date()?),
                _ => return Ok(false),
            }
            Ok(true)
        })?;
        Ok(s)
    }

    pub fn to_xml(&self) -> XElement {
        let mut e = XElement::new("SegmentInfo");
        e.add(bin_elem("SegmentUId", self.segment_uid.as_deref()).unwrap_or_else(|| XElement::new("SegmentUId")));
        e.add(bin_elem("PreviousUId", self.previous_uid.as_deref()).unwrap_or_else(|| XElement::new("PreviousUId")));
        e.add(bin_elem("NextUId", self.next_uid.as_deref()).unwrap_or_else(|| XElement::new("NextUId")));
        e.add(opt_elem("SegmentFilename", self.segment_filename.as_ref()));
        e.add(opt_elem("PreviousFilename", self.previous_filename.as_ref()));
        e.add(opt_elem("NextFilename", self.next_filename.as_ref()));
        e.add(opt_elem("Duration", self.duration.map(f64_str)));
        e.add(opt_elem("Title", self.title.as_ref()));
        e.add(opt_elem("MuxingApp", self.muxing_app.as_ref()));
        e.add(opt_elem("WritingApp", self.writing_app.as_ref()));
        e.add(opt_elem("ProductionDate", self.production_date.map(crate::info::value::format_datetime)));
        e
    }
}

// ---------------------------------------------------------------- Tracks

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackType {
    Invalid = 0,
    Video = 1,
    Audio = 2,
    Complex = 3,
    Logo = 0x10,
    Subtitle = 0x11,
    Button = 0x12,
    Control = 0x20,
}

impl TrackType {
    fn from_u64(v: u64) -> Self {
        match v {
            1 => Self::Video,
            2 => Self::Audio,
            3 => Self::Complex,
            0x10 => Self::Logo,
            0x11 => Self::Subtitle,
            0x12 => Self::Button,
            0x20 => Self::Control,
            _ => Self::Invalid,
        }
    }
}

impl std::fmt::Display for TrackType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

pub mod track_flags {
    pub const NONE: u32 = 0;
    pub const ENABLED: u32 = 1;
    pub const DEFAULT: u32 = 2;
    pub const FORCED: u32 = 4;
    pub const LACING: u32 = 8;
    pub fn to_string(flags: u32) -> String {
        let names: Vec<&str> = [(ENABLED, "Enabled"), (DEFAULT, "Default"), (FORCED, "Forced"), (LACING, "Lacing")]
            .iter()
            .filter(|(b, _)| flags & b != 0)
            .map(|(_, n)| *n)
            .collect();
        if names.is_empty() { "None".to_string() } else { names.join(", ") }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayUnit {
    Pixels = 0,
    Centimeters = 1,
    Inches = 2,
    AspectRatio = 3,
    Unknown = 4,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArType {
    FreeResizing = 0,
    KeepAr = 1,
    Fixed = 2,
    Unknown = 3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StereoMode {
    Mono = 0,
    LeftRight = 1,
    BottomTop = 2,
    TopBottom = 3,
    CheckBoardRight = 4,
    CheckboardLeft = 5,
    RowInterleavedRight = 6,
    RowInterleavedLeft = 7,
    ColumnInterleavedRight = 8,
    ColumnInterleavedLeft = 9,
    AnaGlyphCyanRed = 10,
    RightLeft = 11,
    AnaGlyphGreenMagenta = 12,
    AlternatingFramesRight = 13,
    AlternatingFramesLeft = 14,
    Other = 255,
}

impl StereoMode {
    fn from_u64(v: u64) -> Self {
        match v {
            0 => Self::Mono,
            1 => Self::LeftRight,
            2 => Self::BottomTop,
            3 => Self::TopBottom,
            4 => Self::CheckBoardRight,
            5 => Self::CheckboardLeft,
            6 => Self::RowInterleavedRight,
            7 => Self::RowInterleavedLeft,
            8 => Self::ColumnInterleavedRight,
            9 => Self::ColumnInterleavedLeft,
            10 => Self::AnaGlyphCyanRed,
            11 => Self::RightLeft,
            12 => Self::AnaGlyphGreenMagenta,
            13 => Self::AlternatingFramesRight,
            14 => Self::AlternatingFramesLeft,
            _ => Self::Other,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct VideoSection {
    pub frame_rate: Option<f64>,
    pub gamma: Option<f64>,
    pub color_space: Option<Vec<u8>>,
    pub pixel_width: u64,
    pub pixel_height: u64,
    aspect_ratio_type: Option<u64>,
    pub old_stereo_mode: Option<u64>,
    alpha_mode: Option<u64>,
    stereo_mode: Option<u64>,
    interlaced: Option<bool>,
    pixel_crop_bottom: Option<u64>,
    pixel_crop_top: Option<u64>,
    pixel_crop_left: Option<u64>,
    pixel_crop_right: Option<u64>,
    display_width: Option<u64>,
    display_height: Option<u64>,
    display_unit: Option<u64>,
}

impl VideoSection {
    pub fn aspect_ratio_type(&self) -> ArType {
        match self.aspect_ratio_type.unwrap_or(0) {
            0 => ArType::FreeResizing,
            1 => ArType::KeepAr,
            2 => ArType::Fixed,
            _ => ArType::Unknown,
        }
    }
    pub fn alpha_mode(&self) -> u64 {
        self.alpha_mode.unwrap_or(0)
    }
    pub fn stereo_mode(&self) -> StereoMode {
        StereoMode::from_u64(self.stereo_mode.unwrap_or(0))
    }
    pub fn interlaced(&self) -> bool {
        self.interlaced.unwrap_or(false)
    }
    pub fn pixel_crop_bottom(&self) -> u64 {
        self.pixel_crop_bottom.unwrap_or(0)
    }
    pub fn pixel_crop_top(&self) -> u64 {
        self.pixel_crop_top.unwrap_or(0)
    }
    pub fn pixel_crop_left(&self) -> u64 {
        self.pixel_crop_left.unwrap_or(0)
    }
    pub fn pixel_crop_right(&self) -> u64 {
        self.pixel_crop_right.unwrap_or(0)
    }
    pub fn display_width(&self) -> u64 {
        self.display_width.unwrap_or(self.pixel_width)
    }
    pub fn display_height(&self) -> u64 {
        self.display_height.unwrap_or(self.pixel_height)
    }
    pub fn display_unit(&self) -> DisplayUnit {
        match self.display_unit.unwrap_or(0) {
            0 => DisplayUnit::Pixels,
            1 => DisplayUnit::Centimeters,
            2 => DisplayUnit::Inches,
            3 => DisplayUnit::AspectRatio,
            _ => DisplayUnit::Unknown,
        }
    }

    fn read(reader: &mut EbmlReader<'_, '_>) -> R<Self> {
        let mut s = Self::default();
        read_children(reader, |r, id| {
            match id {
                PIXEL_WIDTH => s.pixel_width = r.read_uint()?,
                PIXEL_HEIGHT => s.pixel_height = r.read_uint()?,
                PIXEL_CROP_BOTTOM => s.pixel_crop_bottom = Some(r.read_uint()?),
                PIXEL_CROP_TOP => s.pixel_crop_top = Some(r.read_uint()?),
                PIXEL_CROP_LEFT => s.pixel_crop_left = Some(r.read_uint()?),
                PIXEL_CROP_RIGHT => s.pixel_crop_right = Some(r.read_uint()?),
                DISPLAY_WIDTH => s.display_width = Some(r.read_uint()?),
                DISPLAY_HEIGHT => s.display_height = Some(r.read_uint()?),
                DISPLAY_UNIT => s.display_unit = Some(r.read_uint()?),
                ASPECT_RATIO_TYPE => s.aspect_ratio_type = Some(r.read_uint()?),
                OLD_STEREO_MODE => s.old_stereo_mode = Some(r.read_uint()?),
                STEREO_MODE => s.stereo_mode = Some(r.read_uint()?),
                ALPHA_MODE => s.alpha_mode = Some(r.read_uint()?),
                FRAME_RATE => s.frame_rate = Some(r.read_float()?),
                FLAG_INTERLACED => s.interlaced = Some(r.read_uint()? == 1),
                COLOUR_SPACE => s.color_space = Some(r.read_binary()?),
                GAMMA_VALUE => s.gamma = Some(r.read_float()?),
                _ => return Ok(false),
            }
            Ok(true)
        })?;
        Ok(s)
    }

    pub fn to_xml(&self) -> XElement {
        let mut e = XElement::new("Video");
        e.add(opt_elem("FrameRate", self.frame_rate.map(f64_str)));
        e.add(bin_elem("ColorSpace", self.color_space.as_deref()).unwrap_or_else(|| XElement::new("ColorSpace")));
        e.add(elem("PixelWidth", self.pixel_width));
        e.add(elem("PixelHeight", self.pixel_height));
        e.add(elem("AspectRatioType", format!("{:?}", self.aspect_ratio_type())));
        e.add(elem("StereoMode", format!("{:?}", self.stereo_mode())));
        e.add(elem("AlphaMode", self.alpha_mode()));
        e.add(opt_elem(
            "OldStereoMode",
            self.old_stereo_mode.map(|v| match v {
                0 => "Mono".to_string(),
                1 => "RightEye".to_string(),
                2 => "LeftEye".to_string(),
                3 => "Both".to_string(),
                o => o.to_string(),
            }),
        ));
        e.add(elem("Interlaced", cs_bool(self.interlaced())));
        e.add(elem("PixelCropBottom", self.pixel_crop_bottom()));
        e.add(elem("PixelCropTop", self.pixel_crop_top()));
        e.add(elem("PixelCropLeft", self.pixel_crop_left()));
        e.add(elem("PixelCropRight", self.pixel_crop_right()));
        e.add(elem("DisplayWidth", self.display_width()));
        e.add(elem("DisplayHeight", self.display_height()));
        e.add(elem("DisplayUnit", format!("{:?}", self.display_unit())));
        e
    }
}

fn cs_bool(b: bool) -> &'static str {
    if b { "True" } else { "False" }
}

#[derive(Debug, Clone, Default)]
pub struct AudioSection {
    sampling_frequency: Option<f64>,
    output_sampling_frequency: Option<f64>,
    channel_count: Option<u64>,
    pub bit_depth: Option<u64>,
}

impl AudioSection {
    pub fn sampling_frequency(&self) -> f64 {
        self.sampling_frequency.unwrap_or(8000.0)
    }
    pub fn output_sampling_frequency(&self) -> f64 {
        self.output_sampling_frequency.unwrap_or_else(|| self.sampling_frequency())
    }
    pub fn channel_count(&self) -> u64 {
        self.channel_count.unwrap_or(1)
    }

    fn read(reader: &mut EbmlReader<'_, '_>) -> R<Self> {
        let mut s = Self::default();
        read_children(reader, |r, id| {
            match id {
                SAMPLING_FREQUENCY => s.sampling_frequency = Some(r.read_float()?),
                OUTPUT_SAMPLING_FREQUENCY => s.output_sampling_frequency = Some(r.read_float()?),
                CHANNELS => s.channel_count = Some(r.read_uint()?),
                BIT_DEPTH => s.bit_depth = Some(r.read_uint()?),
                _ => return Ok(false),
            }
            Ok(true)
        })?;
        Ok(s)
    }

    pub fn to_xml(&self) -> XElement {
        let mut e = XElement::new("Audio");
        e.add(elem("SamplingFrequency", f64_str(self.sampling_frequency())));
        e.add(elem("OutputSamplingFrequency", f64_str(self.output_sampling_frequency())));
        e.add(elem("ChannelCount", self.channel_count()));
        e.add(opt_elem("BitDepth", self.bit_depth));
        e
    }
}

#[derive(Debug, Clone, Default)]
pub struct ContentCompressionSection {
    content_comp_algo: Option<u64>,
}

impl ContentCompressionSection {
    pub fn content_comp_algo(&self) -> &'static str {
        match self.content_comp_algo.unwrap_or(0) {
            0 => "zlib",
            1 => "bzlib",
            2 => "lzo1x",
            3 => "HeaderScripting",
            _ => "Unknown",
        }
    }
    fn read(reader: &mut EbmlReader<'_, '_>) -> R<Self> {
        let mut s = Self::default();
        read_children(reader, |r, id| {
            match id {
                CONTENT_COMP_ALGO => s.content_comp_algo = Some(r.read_uint()?),
                _ => return Ok(false),
            }
            Ok(true)
        })?;
        Ok(s)
    }
}

#[derive(Debug, Clone, Default)]
pub struct ContentEncodingSection {
    content_encoding_order: Option<u64>,
    content_encoding_scope: Option<u64>,
    content_encoding_type: Option<u64>,
    pub content_compression: Option<ContentCompressionSection>,
}

impl ContentEncodingSection {
    fn read(reader: &mut EbmlReader<'_, '_>) -> R<Self> {
        let mut s = Self::default();
        read_children(reader, |r, id| {
            match id {
                CONTENT_ENCODING_ORDER => s.content_encoding_order = Some(r.read_uint()?),
                CONTENT_ENCODING_SCOPE => s.content_encoding_scope = Some(r.read_uint()?),
                CONTENT_ENCODING_TYPE => s.content_encoding_type = Some(r.read_uint()?),
                CONTENT_COMPRESSION => s.content_compression = Some(ContentCompressionSection::read(r)?),
                _ => return Ok(false),
            }
            Ok(true)
        })?;
        Ok(s)
    }

    fn to_xml(&self) -> XElement {
        let mut e = XElement::new("ContentEncoding");
        e.add(elem("ContentEncodingOrder", self.content_encoding_order.unwrap_or(0)));
        let scope = self.content_encoding_scope.unwrap_or(1);
        let scope_names: Vec<&str> = [(1, "AllFrames"), (2, "CodecPrivate"), (4, "ContentCompression")]
            .iter()
            .filter(|(b, _)| scope & b != 0)
            .map(|(_, n)| *n)
            .collect();
        e.add(elem("ContentEncodingScope", if scope_names.is_empty() { scope.to_string() } else { scope_names.join(", ") }));
        e.add(elem("ContentEncodingType", if self.content_encoding_type.unwrap_or(0) == 1 { "Encryption" } else { "Compression" }));
        let mut cc = XElement::new("ContentCompression");
        if let Some(c) = &self.content_compression {
            cc.add(elem("ContentCompAlgo", c.content_comp_algo()));
        }
        e.add(cc);
        e
    }
}

#[derive(Debug, Clone, Default)]
pub struct ContentEncodingsSection {
    pub encodings: Vec<ContentEncodingSection>,
}

impl ContentEncodingsSection {
    fn read(reader: &mut EbmlReader<'_, '_>) -> R<Self> {
        let mut s = Self::default();
        read_children(reader, |r, id| {
            match id {
                CONTENT_ENCODING => s.encodings.push(ContentEncodingSection::read(r)?),
                _ => return Ok(false),
            }
            Ok(true)
        })?;
        Ok(s)
    }
    fn to_xml(&self) -> XElement {
        let mut e = XElement::new("ContentEncodings");
        for enc in &self.encodings {
            e.add(enc.to_xml());
        }
        e
    }
}

#[derive(Debug, Clone, Default)]
pub struct TrackEntrySection {
    pub section_size: Option<u64>,
    enabled: Option<bool>,
    default: Option<bool>,
    forced: Option<bool>,
    lacing: Option<bool>,
    language: Option<String>,
    min_cache: Option<u64>,
    max_block_addition_id: Option<u64>,
    track_type: Option<u64>,
    pub track_number: Option<u64>,
    pub track_uid: Option<u64>,
    pub track_overlay: Vec<u64>,
    pub max_cache: Option<u64>,
    pub default_duration: Option<u64>,
    pub default_decoded_field_duration: Option<u64>,
    pub track_timecode_scale: Option<f64>,
    pub name: Option<String>,
    pub codec_id: Option<String>,
    pub codec_private: Option<Vec<u8>>,
    pub codec_name: Option<String>,
    pub attachment_link: Option<String>,
    pub video: Option<VideoSection>,
    pub audio: Option<AudioSection>,
    pub content_encodings: Option<ContentEncodingsSection>,
}

impl TrackEntrySection {
    pub fn track_type(&self) -> TrackType {
        TrackType::from_u64(self.track_type.unwrap_or(0))
    }
    pub fn track_flags(&self) -> u32 {
        let mut f = track_flags::NONE;
        if self.enabled.unwrap_or(true) {
            f |= track_flags::ENABLED;
        }
        if self.forced.unwrap_or(false) {
            f |= track_flags::FORCED;
        }
        if self.default.unwrap_or(true) {
            f |= track_flags::DEFAULT;
        }
        if self.lacing.unwrap_or(false) {
            f |= track_flags::LACING;
        }
        f
    }
    pub fn min_cache(&self) -> u64 {
        self.min_cache.unwrap_or(0)
    }
    pub fn max_block_addition_id(&self) -> u64 {
        self.max_block_addition_id.unwrap_or(0)
    }
    pub fn language(&self) -> &str {
        self.language.as_deref().unwrap_or("eng")
    }

    fn read(reader: &mut EbmlReader<'_, '_>) -> R<Self> {
        let mut s = Self { section_size: section_size(reader), ..Default::default() };
        read_children(reader, |r, id| {
            match id {
                TRACK_NUMBER => s.track_number = Some(r.read_uint()?),
                TRACK_UID => s.track_uid = Some(r.read_uint()?),
                TRACK_OVERLAY => s.track_overlay.push(r.read_uint()?),
                TRACK_TYPE => s.track_type = Some(r.read_uint()?),
                MIN_CACHE => s.min_cache = Some(r.read_uint()?),
                MAX_CACHE => s.max_cache = Some(r.read_uint()?),
                MAX_BLOCK_ADDITION_ID => s.max_block_addition_id = Some(r.read_uint()?),
                DEFAULT_DURATION => s.default_duration = Some(r.read_uint()?),
                DEFAULT_DECODED_FIELD_DURATION => s.default_decoded_field_duration = Some(r.read_uint()?),
                TRACK_TIMECODE_SCALE => s.track_timecode_scale = Some(r.read_float()?),
                NAME => s.name = Some(r.read_string()?),
                LANGUAGE => s.language = Some(r.read_string()?),
                CODEC_ID => s.codec_id = Some(r.read_string()?),
                CODEC_NAME => s.codec_name = Some(r.read_string()?),
                CODEC_PRIVATE => s.codec_private = Some(r.read_binary()?),
                ATTACHMENT_LINK => s.attachment_link = Some(r.read_string()?),
                FLAG_ENABLED => s.enabled = Some(r.read_uint()? == 1),
                FLAG_DEFAULT => s.default = Some(r.read_uint()? == 1),
                FLAG_FORCED => s.forced = Some(r.read_uint()? == 1),
                FLAG_LACING => s.lacing = Some(r.read_uint()? == 1),
                VIDEO => s.video = Some(VideoSection::read(r)?),
                AUDIO => s.audio = Some(AudioSection::read(r)?),
                CONTENT_ENCODINGS => s.content_encodings = Some(ContentEncodingsSection::read(r)?),
                _ => return Ok(false),
            }
            Ok(true)
        })?;
        Ok(s)
    }

    pub fn to_xml(&self) -> XElement {
        let mut e = XElement::new("Track");
        e.add(opt_elem("TrackNumber", self.track_number));
        e.add(opt_elem("TrackUId", self.track_uid));
        for o in &self.track_overlay {
            e.add(elem("TrackOverlay", o));
        }
        e.add(elem("TrackType", self.track_type()));
        e.add(elem("TrackFlags", track_flags::to_string(self.track_flags())));
        e.add(elem("MinCache", self.min_cache()));
        e.add(opt_elem("MaxCache", self.max_cache));
        e.add(elem("MaxBlockAdditionID", self.max_block_addition_id()));
        e.add(opt_elem("DefaultDuration", self.default_duration));
        e.add(opt_elem("DefaultDecodedFieldDuration", self.default_decoded_field_duration));
        e.add(opt_elem("TrackTimecodeScale", self.track_timecode_scale.map(f64_str)));
        e.add(opt_elem("Name", self.name.as_ref()));
        e.add(elem("Language", self.language()));
        e.add(opt_elem("CodecId", self.codec_id.as_ref()));
        e.add(bin_elem("CodecPrivate", self.codec_private.as_deref()).unwrap_or_else(|| XElement::new("CodecPrivate")));
        e.add(opt_elem("CodecName", self.codec_name.as_ref()));
        e.add(opt_elem("AttachmentLink", self.attachment_link.as_ref()));
        e.add(self.video.as_ref().map(|v| v.to_xml()).unwrap_or_else(|| XElement::new("Video")));
        e.add(self.audio.as_ref().map(|a| a.to_xml()).unwrap_or_else(|| XElement::new("Audio")));
        e.add(self.content_encodings.as_ref().map(|c| c.to_xml()).unwrap_or_else(|| XElement::new("ContentEncodings")));
        e
    }
}

#[derive(Debug, Clone, Default)]
pub struct TracksSection {
    pub section_size: Option<u64>,
    pub items: Vec<TrackEntrySection>,
}

impl TracksSection {
    pub fn read(reader: &mut EbmlReader<'_, '_>) -> R<Self> {
        let mut s = Self { section_size: section_size(reader), ..Default::default() };
        read_children(reader, |r, id| {
            match id {
                TRACK_ENTRY => s.items.push(TrackEntrySection::read(r)?),
                _ => return Ok(false),
            }
            Ok(true)
        })?;
        Ok(s)
    }

    pub fn to_xml(&self) -> XElement {
        let mut e = XElement::new("Tracks");
        for t in &self.items {
            e.add(t.to_xml());
        }
        e
    }
}

// ---------------------------------------------------------------- Attachments

#[derive(Debug, Clone, Default)]
pub struct AttachedFileSection {
    pub section_size: Option<u64>,
    pub file_description: Option<String>,
    pub file_name: Option<String>,
    pub file_mime_type: Option<String>,
    pub file_uid: u64,
    pub file_data_size: u64,
}

impl AttachedFileSection {
    fn read(reader: &mut EbmlReader<'_, '_>) -> R<Self> {
        let mut s = Self { section_size: section_size(reader), ..Default::default() };
        read_children(reader, |r, id| {
            match id {
                FILE_DESCRIPTION => s.file_description = Some(r.read_string()?),
                FILE_NAME => s.file_name = Some(r.read_string()?),
                FILE_MIME_TYPE => s.file_mime_type = Some(r.read_string()?),
                FILE_UID => s.file_uid = r.read_uint()?,
                FILE_DATA => s.file_data_size = r.current().and_then(|h| h.size).unwrap_or(0),
                _ => return Ok(false),
            }
            Ok(true)
        })?;
        Ok(s)
    }

    fn to_xml(&self) -> XElement {
        let mut e = XElement::new("AttachedFile");
        e.add(opt_elem("FileDescription", self.file_description.as_ref()));
        e.add(opt_elem("FileName", self.file_name.as_ref()));
        e.add(opt_elem("FileMimeType", self.file_mime_type.as_ref()));
        e.add(elem("FileUId", self.file_uid));
        e.add(elem("FileDataSize", self.file_data_size));
        e
    }
}

#[derive(Debug, Clone, Default)]
pub struct AttachmentsSection {
    pub section_size: Option<u64>,
    pub items: Vec<AttachedFileSection>,
}

impl AttachmentsSection {
    pub fn read(reader: &mut EbmlReader<'_, '_>) -> R<Self> {
        let mut s = Self { section_size: section_size(reader), ..Default::default() };
        read_children(reader, |r, id| {
            match id {
                ATTACHED_FILE => s.items.push(AttachedFileSection::read(r)?),
                _ => return Ok(false),
            }
            Ok(true)
        })?;
        Ok(s)
    }
    pub fn to_xml(&self) -> XElement {
        let mut e = XElement::new("Attachments");
        for a in &self.items {
            e.add(a.to_xml());
        }
        e
    }
}

// ---------------------------------------------------------------- Chapters

#[derive(Debug, Clone, Default)]
pub struct ChapterTrackSection {
    pub chapter_track_numbers: Vec<u64>,
}

impl ChapterTrackSection {
    fn read(reader: &mut EbmlReader<'_, '_>) -> R<Self> {
        let mut s = Self::default();
        read_children(reader, |r, id| {
            match id {
                CHAPTER_TRACK_NUMBER => s.chapter_track_numbers.push(r.read_uint()?),
                _ => return Ok(false),
            }
            Ok(true)
        })?;
        Ok(s)
    }
}

#[derive(Debug, Clone, Default)]
pub struct ChapterDisplaySection {
    pub chapter_string: Option<String>,
    pub chapter_languages: Vec<String>,
    pub chapter_countries: Vec<String>,
}

impl ChapterDisplaySection {
    fn read(reader: &mut EbmlReader<'_, '_>) -> R<Self> {
        let mut s = Self::default();
        read_children(reader, |r, id| {
            match id {
                CHAP_STRING => s.chapter_string = Some(r.read_string()?),
                CHAP_LANGUAGE => s.chapter_languages.push(r.read_string()?),
                CHAP_COUNTRY => s.chapter_countries.push(r.read_string()?),
                _ => return Ok(false),
            }
            Ok(true)
        })?;
        Ok(s)
    }
    fn to_xml(&self) -> XElement {
        let mut e = XElement::new("ChapterDisplay");
        for l in &self.chapter_languages {
            e.add(elem("ChapterLanguage", l));
        }
        for c in &self.chapter_countries {
            e.add(elem("ChapterCountry", c));
        }
        e.add(opt_elem("ChapterString", self.chapter_string.as_ref()));
        e
    }
}

#[derive(Debug, Clone, Default)]
pub struct ChapterProcessCommandSection {
    pub chapter_process_time: Option<u64>,
}

#[derive(Debug, Clone, Default)]
pub struct ChapterProcessSection {
    chapter_process_codec_id: Option<u64>,
    pub chapter_process_commands: Vec<ChapterProcessCommandSection>,
}

impl ChapterProcessSection {
    fn read(reader: &mut EbmlReader<'_, '_>) -> R<Self> {
        let mut s = Self::default();
        read_children(reader, |r, id| {
            match id {
                CHAP_PROCESS_CODEC_ID => s.chapter_process_codec_id = Some(r.read_uint()?),
                CHAP_PROCESS_COMMAND => {
                    let mut cmd = ChapterProcessCommandSection::default();
                    read_children(r, |r2, id2| {
                        match id2 {
                            CHAP_PROCESS_TIME => cmd.chapter_process_time = Some(r2.read_uint()?),
                            _ => return Ok(false),
                        }
                        Ok(true)
                    })?;
                    s.chapter_process_commands.push(cmd);
                }
                _ => return Ok(false),
            }
            Ok(true)
        })?;
        Ok(s)
    }
    fn to_xml(&self) -> XElement {
        let mut e = XElement::new("ChapterProcess");
        e.add(elem("ChapterProcessCodecId", self.chapter_process_codec_id.unwrap_or(0)));
        for c in &self.chapter_process_commands {
            let mut ce = XElement::new("ChapterProcessCommand");
            ce.add(opt_elem(
                "ChapterProcessTime",
                c.chapter_process_time.map(|t| match t {
                    0 => "During".to_string(),
                    1 => "Before".to_string(),
                    2 => "After".to_string(),
                    o => o.to_string(),
                }),
            ));
            e.add(ce);
        }
        e
    }
}

pub mod chapter_flags {
    pub const NONE: u32 = 0;
    pub const HIDDEN: u32 = 1;
    pub const ENABLED: u32 = 2;
    pub fn to_string(flags: u32) -> String {
        let names: Vec<&str> = [(HIDDEN, "Hidden"), (ENABLED, "Enabled")].iter().filter(|(b, _)| flags & b != 0).map(|(_, n)| *n).collect();
        if names.is_empty() { "None".to_string() } else { names.join(", ") }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ChapterAtomSection {
    enabled: Option<bool>,
    hidden: Option<bool>,
    pub chapter_uid: Option<u64>,
    pub chapter_string_uid: Option<String>,
    pub chapter_time_start: Option<u64>,
    pub chapter_time_end: Option<u64>,
    pub chapter_segment_uid: Option<Vec<u8>>,
    pub chapter_segment_edition_uid: Option<u64>,
    pub chapter_physical_equiv: Option<u64>,
    pub chapter_track: Option<ChapterTrackSection>,
    pub chapter_atoms: Vec<ChapterAtomSection>,
    pub chapter_displays: Vec<ChapterDisplaySection>,
    pub chapter_processes: Vec<ChapterProcessSection>,
}

impl ChapterAtomSection {
    pub fn chapter_flags(&self) -> u32 {
        let mut f = chapter_flags::NONE;
        if self.hidden.unwrap_or(false) {
            f |= chapter_flags::HIDDEN;
        }
        if self.enabled.unwrap_or(true) {
            f |= chapter_flags::ENABLED;
        }
        f
    }

    fn read(reader: &mut EbmlReader<'_, '_>) -> R<Self> {
        let mut s = Self::default();
        read_children(reader, |r, id| {
            match id {
                CHAPTER_TRACK => s.chapter_track = Some(ChapterTrackSection::read(r)?),
                CHAPTER_ATOM => s.chapter_atoms.push(ChapterAtomSection::read(r)?),
                CHAPTER_DISPLAY => s.chapter_displays.push(ChapterDisplaySection::read(r)?),
                CHAP_PROCESS => s.chapter_processes.push(ChapterProcessSection::read(r)?),
                CHAPTER_UID => s.chapter_uid = Some(r.read_uint()?),
                CHAPTER_STRING_UID => s.chapter_string_uid = Some(r.read_string()?),
                CHAPTER_TIME_START => s.chapter_time_start = Some(r.read_uint()?),
                CHAPTER_TIME_END => s.chapter_time_end = Some(r.read_uint()?),
                CHAPTER_FLAG_ENABLED => s.enabled = Some(r.read_uint()? == 1),
                CHAPTER_FLAG_HIDDEN => s.hidden = Some(r.read_uint()? == 1),
                CHAPTER_SEGMENT_UID => s.chapter_segment_uid = Some(r.read_binary()?),
                CHAPTER_SEGMENT_EDITION_UID => s.chapter_segment_edition_uid = Some(r.read_uint()?),
                CHAPTER_PHYSICAL_EQUIV => s.chapter_physical_equiv = Some(r.read_uint()?),
                _ => return Ok(false),
            }
            Ok(true)
        })?;
        Ok(s)
    }

    fn to_xml(&self) -> XElement {
        let mut e = XElement::new("ChapterAtom");
        let mut ct = XElement::new("ChapterTrack");
        if let Some(t) = &self.chapter_track {
            for n in &t.chapter_track_numbers {
                ct.add(elem("ChapterTrackNumber", n));
            }
        }
        e.add(ct);
        for a in &self.chapter_atoms {
            e.add(a.to_xml());
        }
        for d in &self.chapter_displays {
            e.add(d.to_xml());
        }
        for p in &self.chapter_processes {
            e.add(p.to_xml());
        }
        e.add(opt_elem("ChapterUId", self.chapter_uid));
        e.add(opt_elem("ChapterTimeStart", self.chapter_time_start));
        e.add(opt_elem("ChapterTimeEnd", self.chapter_time_end));
        e.add(elem("ChapterFlags", chapter_flags::to_string(self.chapter_flags())));
        e.add(bin_elem("ChapterSegmentUId", self.chapter_segment_uid.as_deref()).unwrap_or_else(|| XElement::new("ChapterSegmentUId")));
        e.add(opt_elem("ChapterSegmentEditionUId", self.chapter_segment_edition_uid));
        e
    }
}

pub mod edition_flags {
    pub const HIDDEN: u32 = 1;
    pub const DEFAULT: u32 = 2;
    pub const ORDERED: u32 = 4;
    pub fn to_string(flags: u32) -> String {
        let names: Vec<&str> = [(HIDDEN, "Hidden"), (DEFAULT, "Default"), (ORDERED, "Ordered")].iter().filter(|(b, _)| flags & b != 0).map(|(_, n)| *n).collect();
        if names.is_empty() { "None".to_string() } else { names.join(", ") }
    }
}

#[derive(Debug, Clone, Default)]
pub struct EditionEntrySection {
    hidden: Option<bool>,
    ordered: Option<bool>,
    default: Option<bool>,
    pub edition_uid: Option<u64>,
    pub chapter_atoms: Vec<ChapterAtomSection>,
}

impl EditionEntrySection {
    pub fn edition_flags(&self) -> u32 {
        let mut f = 0;
        if self.hidden.unwrap_or(false) {
            f |= edition_flags::HIDDEN;
        }
        if self.ordered.unwrap_or(false) {
            f |= edition_flags::ORDERED;
        }
        if self.default.unwrap_or(false) {
            f |= edition_flags::DEFAULT;
        }
        f
    }

    fn read(reader: &mut EbmlReader<'_, '_>) -> R<Self> {
        let mut s = Self::default();
        read_children(reader, |r, id| {
            match id {
                CHAPTER_ATOM => s.chapter_atoms.push(ChapterAtomSection::read(r)?),
                EDITION_UID => s.edition_uid = Some(r.read_uint()?),
                EDITION_FLAG_HIDDEN => s.hidden = Some(r.read_uint()? == 1),
                EDITION_FLAG_DEFAULT => s.default = Some(r.read_uint()? == 1),
                EDITION_FLAG_ORDERED => s.ordered = Some(r.read_uint()? == 1),
                _ => return Ok(false),
            }
            Ok(true)
        })?;
        Ok(s)
    }

    fn to_xml(&self) -> XElement {
        let mut e = XElement::new("EditionEntry");
        e.add(opt_elem("EditionUId", self.edition_uid));
        e.add(elem("EditionFlags", edition_flags::to_string(self.edition_flags())));
        for a in &self.chapter_atoms {
            e.add(a.to_xml());
        }
        e
    }
}

#[derive(Debug, Clone, Default)]
pub struct ChaptersSection {
    pub section_size: Option<u64>,
    pub items: Vec<EditionEntrySection>,
}

impl ChaptersSection {
    pub fn read(reader: &mut EbmlReader<'_, '_>) -> R<Self> {
        let mut s = Self { section_size: section_size(reader), ..Default::default() };
        read_children(reader, |r, id| {
            match id {
                EDITION_ENTRY => s.items.push(EditionEntrySection::read(r)?),
                _ => return Ok(false),
            }
            Ok(true)
        })?;
        Ok(s)
    }
    pub fn to_xml(&self) -> XElement {
        let mut e = XElement::new("Chapters");
        for i in &self.items {
            e.add(i.to_xml());
        }
        e
    }
}

// ---------------------------------------------------------------- Tags

#[derive(Debug, Clone, Default)]
pub struct SimpleTagSection {
    tag_language: Option<String>,
    tag_default: Option<bool>,
    tag_binary: Option<Vec<u8>>,
    pub simple_tags: Vec<SimpleTagSection>,
    pub tag_name: Option<String>,
    pub tag_string: Option<String>,
}

impl SimpleTagSection {
    pub fn tag_language(&self) -> &str {
        self.tag_language.as_deref().unwrap_or("und")
    }
    pub fn tag_default(&self) -> bool {
        self.tag_default.unwrap_or(true)
    }
    pub fn tag_binary(&self) -> &[u8] {
        self.tag_binary.as_deref().unwrap_or(&[])
    }

    fn read(reader: &mut EbmlReader<'_, '_>) -> R<Self> {
        let mut s = Self::default();
        read_children(reader, |r, id| {
            match id {
                TAG_NAME => s.tag_name = Some(r.read_string()?),
                TAG_LANGUAGE => s.tag_language = Some(r.read_string()?),
                TAG_STRING => s.tag_string = Some(r.read_string()?),
                TAG_DEFAULT => s.tag_default = Some(r.read_uint()? == 1),
                TAG_BINARY => s.tag_binary = Some(r.read_binary()?),
                SIMPLE_TAG => s.simple_tags.push(SimpleTagSection::read(r)?),
                _ => return Ok(false),
            }
            Ok(true)
        })?;
        Ok(s)
    }

    fn to_xml(&self) -> XElement {
        let mut e = XElement::new("SimpleTag");
        e.add(opt_elem("TagName", self.tag_name.as_ref()));
        e.add(elem("TagLanguage", self.tag_language()));
        e.add(opt_elem("TagString", self.tag_string.as_ref()));
        e.add(elem("TagDefault", cs_bool(self.tag_default())));
        e.add(bin_elem("TagBinary", Some(self.tag_binary())).unwrap());
        for t in &self.simple_tags {
            e.add(t.to_xml());
        }
        e
    }
}

#[derive(Debug, Clone, Default)]
pub struct TargetsSection {
    target_type_value: Option<u64>,
    pub target_type: Option<String>,
    track_uids: Vec<u64>,
    edition_uids: Vec<u64>,
    chapter_uids: Vec<u64>,
    attachment_uids: Vec<u64>,
}

impl TargetsSection {
    pub fn target_type_value(&self) -> u64 {
        self.target_type_value.unwrap_or(50)
    }
    fn or_zero(v: &[u64]) -> Vec<u64> {
        if v.is_empty() { vec![0] } else { v.to_vec() }
    }
    pub fn track_uids(&self) -> Vec<u64> {
        Self::or_zero(&self.track_uids)
    }
    pub fn edition_uids(&self) -> Vec<u64> {
        Self::or_zero(&self.edition_uids)
    }
    pub fn chapter_uids(&self) -> Vec<u64> {
        Self::or_zero(&self.chapter_uids)
    }
    pub fn attachment_uids(&self) -> Vec<u64> {
        Self::or_zero(&self.attachment_uids)
    }

    fn read(reader: &mut EbmlReader<'_, '_>) -> R<Self> {
        let mut s = Self::default();
        read_children(reader, |r, id| {
            match id {
                TARGET_TYPE_VALUE => s.target_type_value = Some(r.read_uint()?),
                TARGET_TYPE => s.target_type = Some(r.read_string()?),
                TAG_TRACK_UID => s.track_uids.push(r.read_uint()?),
                TAG_EDITION_UID => s.edition_uids.push(r.read_uint()?),
                TAG_CHAPTER_UID => s.chapter_uids.push(r.read_uint()?),
                TAG_ATTACHMENT_UID => s.attachment_uids.push(r.read_uint()?),
                _ => return Ok(false),
            }
            Ok(true)
        })?;
        Ok(s)
    }

    fn to_xml(&self) -> XElement {
        let mut e = XElement::new("Targets");
        e.add(elem("TargetTypeValue", self.target_type_value()));
        e.add(opt_elem("TargetType", self.target_type.as_ref()));
        for v in self.track_uids() {
            e.add(elem("TrackUId", v));
        }
        for v in self.edition_uids() {
            e.add(elem("EditionUId", v));
        }
        for v in self.chapter_uids() {
            e.add(elem("ChapterUId", v));
        }
        for v in self.attachment_uids() {
            e.add(elem("AttachmentUId", v));
        }
        e
    }
}

#[derive(Debug, Clone, Default)]
pub struct TagSection {
    pub targets: TargetsSection,
    pub simple_tags: Vec<SimpleTagSection>,
}

impl TagSection {
    fn read(reader: &mut EbmlReader<'_, '_>) -> R<Self> {
        let mut s = Self::default();
        read_children(reader, |r, id| {
            match id {
                TARGETS => s.targets = TargetsSection::read(r)?,
                SIMPLE_TAG => s.simple_tags.push(SimpleTagSection::read(r)?),
                _ => return Ok(false),
            }
            Ok(true)
        })?;
        Ok(s)
    }
    fn to_xml(&self) -> XElement {
        let mut e = XElement::new("Tag");
        e.add(self.targets.to_xml());
        for t in &self.simple_tags {
            e.add(t.to_xml());
        }
        e
    }
}

#[derive(Debug, Clone, Default)]
pub struct TagsSection {
    pub section_size: Option<u64>,
    pub items: Vec<TagSection>,
}

impl TagsSection {
    pub fn read(reader: &mut EbmlReader<'_, '_>) -> R<Self> {
        let mut s = Self { section_size: section_size(reader), ..Default::default() };
        read_children(reader, |r, id| {
            match id {
                TAG => s.items.push(TagSection::read(r)?),
                _ => return Ok(false),
            }
            Ok(true)
        })?;
        Ok(s)
    }
    pub fn to_xml(&self) -> XElement {
        let mut e = XElement::new("Tags");
        for t in &self.items {
            e.add(t.to_xml());
        }
        e
    }
}

// ---------------------------------------------------------------- Cues / SeekHead

#[derive(Debug, Clone, Default)]
pub struct CueReferenceSection {
    pub cue_cluster_position: u64,
    pub cue_ref_cluster: u64,
    cue_ref_number: Option<u64>,
    cue_ref_codec_state: Option<u64>,
}

#[derive(Debug, Clone, Default)]
pub struct CueTrackPositionsSection {
    pub cue_references: Vec<CueReferenceSection>,
    cue_block_number: Option<u64>,
    cue_codec_state: Option<u64>,
    pub cue_track: u64,
    pub cue_cluster_position: u64,
    pub cue_relative_position: Option<u64>,
    pub cue_duration: Option<u64>,
}

impl CueTrackPositionsSection {
    pub fn cue_block_number(&self) -> u64 {
        match self.cue_block_number {
            Some(0) | None => 1,
            Some(v) => v,
        }
    }
    pub fn cue_codec_state(&self) -> u64 {
        self.cue_codec_state.unwrap_or(0)
    }
    fn read(reader: &mut EbmlReader<'_, '_>) -> R<Self> {
        let mut s = Self::default();
        read_children(reader, |r, id| {
            match id {
                CUE_REFERENCE => {
                    let mut cr = CueReferenceSection::default();
                    read_children(r, |r2, id2| {
                        match id2 {
                            CUE_CLUSTER_POSITION => cr.cue_cluster_position = r2.read_uint()?,
                            CUE_REF_CLUSTER => cr.cue_ref_cluster = r2.read_uint()?,
                            CUE_REF_NUMBER => cr.cue_ref_number = Some(r2.read_uint()?),
                            CUE_REF_CODEC_STATE => cr.cue_ref_codec_state = Some(r2.read_uint()?),
                            _ => return Ok(false),
                        }
                        Ok(true)
                    })?;
                    s.cue_references.push(cr);
                }
                CUE_TRACK => s.cue_track = r.read_uint()?,
                CUE_CLUSTER_POSITION => s.cue_cluster_position = r.read_uint()?,
                CUE_RELATIVE_POSITION => s.cue_relative_position = Some(r.read_uint()?),
                CUE_DURATION => s.cue_duration = Some(r.read_uint()?),
                CUE_BLOCK_NUMBER => s.cue_block_number = Some(r.read_uint()?),
                CUE_CODEC_STATE => s.cue_codec_state = Some(r.read_uint()?),
                _ => return Ok(false),
            }
            Ok(true)
        })?;
        Ok(s)
    }
}

#[derive(Debug, Clone, Default)]
pub struct CuePointSection {
    pub cue_track_positions: Vec<CueTrackPositionsSection>,
    pub cue_time: u64,
}

#[derive(Debug, Clone, Default)]
pub struct CuesSection {
    pub section_size: Option<u64>,
    pub cue_points: Vec<CuePointSection>,
}

impl CuesSection {
    pub fn read(reader: &mut EbmlReader<'_, '_>) -> R<Self> {
        let mut s = Self { section_size: section_size(reader), ..Default::default() };
        read_children(reader, |r, id| {
            match id {
                CUE_POINT => {
                    let mut cp = CuePointSection::default();
                    read_children(r, |r2, id2| {
                        match id2 {
                            CUE_TRACK_POSITIONS => cp.cue_track_positions.push(CueTrackPositionsSection::read(r2)?),
                            CUE_TIME => cp.cue_time = r2.read_uint()?,
                            _ => return Ok(false),
                        }
                        Ok(true)
                    })?;
                    s.cue_points.push(cp);
                }
                _ => return Ok(false),
            }
            Ok(true)
        })?;
        Ok(s)
    }
}

#[derive(Debug, Clone, Default)]
pub struct SeekSection {
    pub seek_id: Option<Vec<u8>>,
    pub seek_position: u64,
}

#[derive(Debug, Clone, Default)]
pub struct SeekHeadSection {
    pub section_size: Option<u64>,
    pub seeks: Vec<SeekSection>,
}

impl SeekHeadSection {
    pub fn read(reader: &mut EbmlReader<'_, '_>) -> R<Self> {
        let mut s = Self { section_size: section_size(reader), ..Default::default() };
        read_children(reader, |r, id| {
            match id {
                SEEK => {
                    let mut seek = SeekSection::default();
                    read_children(r, |r2, id2| {
                        match id2 {
                            SEEK_ID => seek.seek_id = Some(r2.read_binary()?),
                            SEEK_POSITION => seek.seek_position = r2.read_uint()?,
                            _ => return Ok(false),
                        }
                        Ok(true)
                    })?;
                    s.seeks.push(seek);
                }
                _ => return Ok(false),
            }
            Ok(true)
        })?;
        Ok(s)
    }
}
