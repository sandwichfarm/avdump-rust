//! MediaInfo binding (the pure-Rust `mediainfo` crate, compiled in) and the provider built on it.

use crate::info::meta::{container_types as ct, keys, MetaInfoContainer, MetaProvider};
use crate::info::value::*;
use crate::misc::xml::XElement;
use mediainfo::{InfoKind as MiInfoKind, StreamKind as MiStreamKind};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(usize)]
pub enum StreamKind {
    General = 0,
    Video = 1,
    Audio = 2,
    Text = 3,
    Other = 4,
    Image = 5,
    Menu = 6,
}

impl StreamKind {
    pub const ALL: [StreamKind; 7] = [Self::General, Self::Video, Self::Audio, Self::Text, Self::Other, Self::Image, Self::Menu];
    pub fn name(&self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Video => "Video",
            Self::Audio => "Audio",
            Self::Text => "Text",
            Self::Other => "Other",
            Self::Image => "Image",
            Self::Menu => "Menu",
        }
    }
    fn mi(self) -> MiStreamKind {
        MiStreamKind::from_usize(self as usize).unwrap_or(MiStreamKind::General)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(usize)]
pub enum InfoKind {
    Name = 0,
    Text = 1,
    Measure = 2,
    Options = 3,
    NameText = 4,
    MeasureText = 5,
    Info = 6,
    HowTo = 7,
}

impl InfoKind {
    fn mi(self) -> MiInfoKind {
        MiInfoKind::from_usize(self as usize).unwrap_or(MiInfoKind::Text)
    }
}

/// The MediaInfo implementation is compiled in (pure Rust); it is always available.
pub fn is_available() -> bool {
    true
}

/// Handle to one MediaInfo instance (the pure-Rust `mediainfo` crate).
pub struct MediaInfo {
    inner: mediainfo::MediaInfo,
}

impl MediaInfo {
    pub fn new() -> Option<Self> {
        Some(Self { inner: mediainfo::MediaInfo::new() })
    }

    pub fn open(&mut self, path: &Path) -> bool {
        self.inner.open(path)
    }

    pub fn close(&mut self) {
        self.inner.close()
    }

    pub fn inform(&self) -> String {
        self.inner.inform()
    }

    pub fn option(&mut self, option: &str, value: &str) -> String {
        self.inner.option(option, value)
    }

    pub fn get(&self, parameter: &str, kind: StreamKind, index: usize) -> String {
        self.get_kind(parameter, kind, index, InfoKind::Text)
    }

    pub fn get_kind(&self, parameter: &str, kind: StreamKind, index: usize, info: InfoKind) -> String {
        self.inner.get(kind.mi(), index, parameter, info.mi())
    }

    pub fn get_i(&self, parameter: usize, kind: StreamKind, index: usize, info: InfoKind) -> String {
        self.inner.get_i(kind.mi(), index, parameter, info.mi())
    }

    pub fn count(&self, kind: StreamKind, index: Option<usize>) -> usize {
        self.inner.count_get(kind.mi(), index)
    }
}

/// Version string reported by the library (`Info_Version`).
pub fn version() -> Option<String> {
    let mut mi = MediaInfo::new()?;
    Some(mi.option("Info_Version", ""))
}

// ----------------------------------------------------------------------------- provider

pub struct MediaInfoLibProvider;

fn remove_non_numerics(s: &str) -> String {
    s.chars().filter(|c| matches!(c, '-' | ',' | '.' | '0'..='9')).collect()
}
fn split_take_first(s: &str) -> String {
    s.split(['\\', '/', '|']).next().unwrap_or("").to_string()
}
fn skip_int_default(s: &str) -> Option<String> {
    if s.trim_matches(|c| c == '0' || c == '.' || c == ',').is_empty() { None } else { Some(s.to_string()) }
}
fn parse_f64(s: &str) -> Option<f64> {
    s.trim().parse().ok()
}
fn parse_i64(s: &str) -> Option<i64> {
    s.trim().parse().ok()
}
fn clean(s: &str) -> String {
    s.trim().chars().filter(|c| crate::misc::xml::is_xml_char(*c)).collect()
}
fn non_empty(s: String) -> Option<String> {
    if s.trim().is_empty() { None } else { Some(s) }
}

impl MediaInfoLibProvider {
    pub const NAME: &'static str = "MediaInfoLibProvider";

    pub fn create(path: &Path) -> Option<MetaProvider> {
        let mut mi = MediaInfo::new()?;
        let mut p = MetaProvider::new(Self::NAME, ct::MEDIA_PROVIDER);
        if !path.is_file() {
            return Some(p);
        }
        if !mi.open(path) {
            // The original raises here; we report an empty provider instead so hashing continues.
            return Some(p);
        }
        Self::populate(&mut p, &mi);
        mi.close();
        Some(p)
    }

    fn populate(p: &mut MetaProvider, mi: &MediaInfo) {
        let d = keys::DIMENSIONLESS;
        let g = |k: &str| mi.get(k, StreamKind::General, 0);

        p.add_opt("FileSize", keys::BYTES, non_empty(remove_non_numerics(&split_take_first(&g("FileSize")))).and_then(|s| parse_i64(&s)).map(Value::I64));
        p.add_opt("Duration", keys::SECONDS, non_empty(remove_non_numerics(&split_take_first(&g("Duration")))).and_then(|s| parse_f64(&s)).map(|v| Value::F64(v / 1000.0)));
        p.add_opt("FileExtension", d, non_empty(split_take_first(&g("FileExtension"))).map(|s| Value::Str(s.to_uppercase())));
        p.add_opt("WritingApp", d, non_empty(g("Encoded_Application")).map(Value::Str));
        p.add_opt("MuxingApp", d, non_empty(g("Encoded_Library")).map(Value::Str));

        let (mut has_audio, mut has_video, mut has_subtitle) = (false, false, false);
        for kind in [StreamKind::Video, StreamKind::Audio, StreamKind::Text] {
            let count = mi.count(kind, None);
            for index in 0..count {
                let sg = |k: &str| clean(&mi.get(k, kind, index));
                let mut id: Option<u64> = None;
                if !sg("UniqueID").is_empty() {
                    id = sg("UniqueID").parse().ok();
                }
                if id.is_none() && !sg("ID").is_empty() {
                    id = sg("ID").replace('-', "000").parse().ok();
                }

                let mut stream = match kind {
                    StreamKind::Video => {
                        has_video = true;
                        let mut s = MetaInfoContainer::new(id.unwrap_or(p.root.count_nodes(ct::CHAPTERS) as u64), ct::VIDEO_STREAM);
                        p.add_to(&mut s, "StatedSampleRate", keys::HZ, skip_int_default(&sg("FrameRate")).and_then(|v| parse_f64(&v)).map(Value::F64));
                        p.add_to(&mut s, "SampleCount", d, skip_int_default(&sg("FrameCount")).and_then(|v| parse_i64(&v)).map(Value::I64));
                        p.add_to(&mut s, "PixelAspectRatio", d, parse_f64(&sg("PixelAspectRatio")).map(Value::F64));
                        if let (Some(w), Some(h)) = (parse_i64(&sg("Width")), parse_i64(&sg("Height"))) {
                            p.add_to(&mut s, "PixelDimensions", d, Some(Value::Dimensions(Dimensions { width: w as i32, height: h as i32 })));
                        }
                        p.add_to(&mut s, "DisplayAspectRatio", d, parse_f64(&sg("DisplayAspectRatio")).map(Value::F64));
                        p.add_to(&mut s, "ColorBitDepth", d, parse_i64(&sg("BitDepth")).map(|v| Value::I32(v as i32)));
                        let vfr = sg("FrameRate_Mode").eq_ignore_ascii_case("VFR");
                        p.add_to(&mut s, "AverageSampleRate", keys::HZ, if vfr { parse_f64(&sg("FrameRate")).map(Value::F64) } else { None });
                        p.add_to(&mut s, "MaxSampleRate", keys::HZ, skip_int_default(&sg("FrameRate_Maximum")).and_then(|v| parse_f64(&v)).map(Value::F64));
                        p.add_to(&mut s, "MinSampleRate", keys::HZ, skip_int_default(&sg("FrameRate_Minimum")).and_then(|v| parse_f64(&v)).map(Value::F64));
                        p.add_to(&mut s, "IsInterlaced", d, Some(Value::Bool(sg("ScanType").eq_ignore_ascii_case("Interlaced"))));
                        p.add_to(&mut s, "HasVariableFrameRate", d, Some(Value::Bool(vfr)));
                        let cs = sg("ChromaSubsampling");
                        let parts: Vec<Option<i32>> = cs.split(':').map(|x| x.trim().parse().ok()).collect();
                        if parts.len() == 3 && parts.iter().all(|x| x.is_some()) {
                            p.add_to(&mut s, "ChromaSubsampling", d, Some(Value::ChromaSubsampling(ChromaSubsampling { y: parts[0].unwrap(), cb: parts[1].unwrap(), cr: parts[2].unwrap() })));
                        }
                        s
                    }
                    StreamKind::Audio => {
                        has_audio = true;
                        let mut s = MetaInfoContainer::new(id.unwrap_or(p.root.count_nodes(ct::AUDIO_STREAM) as u64), ct::AUDIO_STREAM);
                        p.add_to(&mut s, "StatedSampleRate", keys::HZ, skip_int_default(&sg("SamplingRate")).and_then(|v| parse_f64(&v)).map(Value::F64));
                        p.add_to(&mut s, "SampleCount", d, parse_i64(&sg("SamplingCount")).map(Value::I64));
                        p.add_to(&mut s, "ChannelCount", d, parse_i64(&sg("Channel(s)")).map(|v| Value::I32(v as i32)));
                        s
                    }
                    _ => {
                        has_subtitle = true;
                        MetaInfoContainer::new(id.unwrap_or(p.root.count_nodes(ct::SUBTITLE_STREAM) as u64), ct::SUBTITLE_STREAM)
                    }
                };

                if matches!(kind, StreamKind::Video | StreamKind::Audio) {
                    p.add_to(&mut stream, "Bitrate", keys::BITS_PER_SECOND, skip_int_default(&sg("BitRate")).and_then(|v| parse_f64(&v)).map(Value::F64));
                    p.add_to(&mut stream, "StatedBitrateMode", d, non_empty(sg("BitRate_Mode")).map(Value::Str));
                }
                p.add_to(&mut stream, "Size", keys::BYTES, parse_i64(&sg("StreamSize")).map(Value::I64));
                p.add_to(&mut stream, "Title", d, non_empty(sg("Title")).map(Value::Str));
                p.add_to(&mut stream, "IsForced", d, Some(Value::Bool(sg("Forced").eq_ignore_ascii_case("yes"))));
                p.add_to(&mut stream, "IsDefault", d, Some(Value::Bool(sg("Default").eq_ignore_ascii_case("yes"))));
                p.add_to(&mut stream, "Id", d, skip_int_default(&sg("UniqueID")).and_then(|v| v.parse::<u64>().ok()).map(Value::U64));
                p.add_to(&mut stream, "Language", d, non_empty(sg("Language")).map(Value::Str));
                p.add_to(&mut stream, "Duration", keys::SECONDS, non_empty(split_take_first(&sg("Duration"))).and_then(|v| parse_f64(&v)).map(|v| Value::TimeSpan(v / 1000.0)));
                p.add_to(&mut stream, "ContainerCodecIdWithCodecPrivate", d, non_empty(sg("CodecID")).map(Value::Str));
                p.add_to(&mut stream, "CodecId", d, non_empty(sg("Format")).map(Value::Str));
                p.add_to(&mut stream, "CodecAdditionalFeatures", d, non_empty(sg("Format_AdditionalFeatures")).map(Value::Str));
                p.add_to(&mut stream, "CodecCommercialId", d, non_empty(sg("Format_Commercial")).map(Value::Str));
                p.add_to(&mut stream, "CodecProfile", d, non_empty(sg("Format_Profile")).map(Value::Str));
                p.add_to(&mut stream, "CodecVersion", d, non_empty(sg("Format_Version")).map(Value::Str));
                p.add_to(&mut stream, "CodecName", d, non_empty(sg("Format-Info")).map(Value::Str));
                p.add_to(&mut stream, "EncoderSettings", d, non_empty(sg("Encoded_Library_Settings")).map(Value::Str));
                p.add_to(&mut stream, "EncoderName", d, non_empty(sg("Encoded_Library")).map(Value::Str));
                p.add_node(stream);
            }
        }

        Self::add_suggested_file_extension(p, mi, has_audio, has_video, has_subtitle);

        let menu_count = mi.count(StreamKind::Menu, None);
        for i in 0..menu_count {
            Self::populate_chapters(p, mi, i);
        }
    }

    fn populate_chapters(p: &mut MetaProvider, mi: &MediaInfo, index: usize) {
        let d = keys::DIMENSIONLESS;
        let menu = StreamKind::Menu;
        let mut chapters = MetaInfoContainer::new(index as u64, ct::CHAPTERS);
        let format = mi.get("Format", menu, index);
        let language_chapters = mi.get("Language", menu, index);
        p.add_to(&mut chapters, "Format", d, Some(Value::Str(format!("{} -- {}", format, if format.is_empty() { "nero" } else { "mov" }).trim().to_string())));

        let begin = mi.get("Chapters_Pos_Begin", menu, index).trim().parse::<usize>();
        let end = mi.get("Chapters_Pos_End", menu, index).trim().parse::<usize>();
        if let (Ok(mut start), Ok(mut end)) = (begin, end) {
            // MIL offset bug workaround.
            let mut tries = 20;
            let is_timestamp = |i: usize| mi.get_i(i, menu, index, InfoKind::Name).split('-').all(|x| x.contains(':'));
            while tries > 0 && !is_timestamp(start) {
                start += 1;
                end += 1;
                tries -= 1;
            }
            if tries == 0 {
                return;
            }
            while start < end {
                let mut chapter = MetaInfoContainer::new(start as u64, ct::CHAPTER);
                let stamps = mi.get_i(start, menu, index, InfoKind::Name);
                let first = stamps.split('-').next().unwrap_or("").trim().to_string();
                if let Some(ns) = parse_timestamp_ns(&first) {
                    p.add_to(&mut chapter, "TimeStart", "byte", Some(Value::F64(ns as f64 / 1000.0)));
                }
                let mut title = mi.get_i(start, menu, index, InfoKind::Text);
                let mut languages = Vec::new();
                match title.find(':') {
                    Some(pos) if pos < 5 => {
                        let language = title[..pos].to_string();
                        if !language.is_empty() {
                            languages.push(language.clone());
                        }
                        title = title[language.len() + 1..].to_string();
                    }
                    _ => {
                        if !language_chapters.is_empty() {
                            languages.push(language_chapters.clone());
                        }
                    }
                }
                p.add_to(&mut chapter, "Titles", d, Some(Value::ChapterTitles(vec![ChapterTitle { title, languages, countries: Vec::new() }])));
                chapters.add_node(chapter);
                start += 1;
            }
        }
        p.add_node(chapters);
    }

    fn add_suggested_file_extension(p: &mut MetaProvider, mi: &MediaInfo, has_audio: bool, has_video: bool, has_subtitle: bool) {
        let d = keys::DIMENSIONLESS;
        let g = |k: &str| mi.get(k, StreamKind::General, 0);
        let file_ext = g("FileExtension").to_lowercase();
        let mut mil_info: Vec<String> = g("Format/Extensions").to_lowercase().split(' ').filter(|s| !s.is_empty()).map(|s| s.to_string()).collect();
        let commercial = g("Format_Commercial").to_lowercase();
        let has = |v: &[String], s: &str| v.iter().any(|x| x == s);
        let suggest = |p: &mut MetaProvider, e: &str| {
            p.add("SuggestedFileExtension", d, Value::Strings(vec![e.to_string()]));
        };

        if has(&mil_info, "asf") && has(&mil_info, "wmv") && has(&mil_info, "wma") {
            if !has_video && has_audio && !has_subtitle {
                suggest(p, "wma");
            } else {
                suggest(p, "wmv");
            }
        } else if has(&mil_info, "ts") && has(&mil_info, "m2t") {
            if file_ext.eq_ignore_ascii_case("ts") {
                suggest(p, "ts");
            }
        } else if has(&mil_info, "mpeg") && has(&mil_info, "mpg") {
            if !has_video && !has_audio && has_subtitle {
                suggest(p, "sub");
            } else {
                suggest(p, if file_ext.eq_ignore_ascii_case("mpeg") { "mpeg" } else { "mpg" });
            }
        } else if has(&mil_info, "mp1") && has(&mil_info, "mp2") && has(&mil_info, "mp3") {
            match mi.get("Format_Profile", StreamKind::Audio, 0).as_str() {
                "Layer 1" => suggest(p, "mp1"),
                "Layer 2" => suggest(p, "mp2"),
                "Layer 3" => suggest(p, "mp3"),
                _ => suggest(p, &mil_info[0].clone()),
            }
        } else if has(&mil_info, "mp4") && has(&mil_info, "m4a") && has(&mil_info, "m4v") {
            if !has_video && has_audio && !has_subtitle {
                suggest(p, "m4a");
            } else if has_video && !has_audio && !has_subtitle {
                suggest(p, "m4v");
            } else {
                suggest(p, "mp4");
            }
        } else if has(&mil_info, "dts") || has(&mil_info, "thd") {
            if commercial.contains("dts") {
                if commercial.contains("dts-hd") {
                    suggest(p, "dtshd");
                } else {
                    suggest(p, "dts");
                }
            } else {
                mil_info = g("Audio_Codec_List").to_lowercase().split(' ').map(|s| s.to_string()).collect();
                if has(&mil_info, "truehd") {
                    suggest(p, "thd");
                } else {
                    suggest(p, "dts");
                }
            }
        } else if has(&mil_info, "mlp") || mil_info.is_empty() || mil_info[0].is_empty() {
            if commercial.contains("truehd") {
                suggest(p, "thd");
            } else if commercial.contains("mlp") {
                suggest(p, "mlp");
            }
        } else if has(&mil_info, "mkv") {
            if has_video {
                suggest(p, "mkv");
            } else if has_audio {
                suggest(p, "mka");
            } else if has_subtitle {
                suggest(p, "mks");
            }
        }

        if has(&mil_info, "ts") {
            suggest(p, "ts");
        } else if has(&mil_info, "m2ts") || has(&mil_info, "m2t") {
            suggest(p, "m2ts");
        } else if has(&mil_info, "wav") {
            suggest(p, "wav");
        } else if has(&mil_info, "m4v") {
            suggest(p, "m4v");
        } else if has(&mil_info, "avc") {
            suggest(p, "avc");
        }

        if p.select("SuggestedFileExtension").is_none() {
            if has(&mil_info, "rm") || has(&mil_info, "rmvb") {
                suggest(p, "rm");
            } else if has(&mil_info, "asf") || has(&mil_info, "wmv") {
                suggest(p, "wmv");
            } else if has(&mil_info, "mov") || has(&mil_info, "qt") {
                suggest(p, "mov");
            } else if has(&mil_info, "aac") || has(&mil_info, "aacp") || has(&mil_info, "adts") {
                suggest(p, "aac");
            }
        }
        if p.select("SuggestedFileExtension").is_none() && !mil_info.is_empty() {
            p.add("SuggestedFileExtension", d, Value::Strings(mil_info));
        }
    }
}

/// `hh:mm:ss.mmm` → nanoseconds.
fn parse_timestamp_ns(s: &str) -> Option<u64> {
    let parts: Vec<u64> = s.split([':', '.']).map(|x| x.trim().parse().ok()).collect::<Option<Vec<_>>>()?;
    if parts.len() != 4 {
        return None;
    }
    Some((((parts[0] * 60 + parts[1]) * 60 + parts[2]) * 1000 + parts[3]) * 1_000_000)
}

/// Raw MediaInfo dump as XML (`MediaInfoXml` report).
pub fn xml_report(path: &Path) -> XElement {
    let mut root = XElement::new("File");
    let mut mi = match MediaInfo::new() {
        Some(mi) => mi,
        None => {
            root.add(XElement::with_text("Error", "MediaInfo library not available"));
            return root;
        }
    };
    mi.open(path);
    for kind in StreamKind::ALL {
        let stream_count = mi.count(kind, None);
        for i in 0..stream_count {
            let entry_count = mi.count(kind, Some(i));
            let mut sub = XElement::new(kind.name());
            for j in 0..entry_count {
                let name = mi.get_i(j, kind, i, InfoKind::Name).replace('/', "-").replace(['(', ')'], "").replace(' ', "_");
                if name == "Chapters_Pos_End" || name == "Chapters_Pos_Begin" || name.contains("-String") {
                    continue;
                }
                let name = if name == "Bits-Pixel*Frame" { "BitsPerPixel".to_string() } else { name };
                let text = mi.get_i(j, kind, i, InfoKind::Text);
                let measure = mi.get_i(j, kind, i, InfoKind::Measure).trim().to_string();
                if !name.contains([')', ':']) && !text.is_empty() {
                    sub.add(XElement::with_text(crate::misc::xml::safe_name(&name), text).attr("Unit", measure));
                }
            }
            if kind == StreamKind::Menu {
                let begin = mi.get("Chapters_Pos_Begin", kind, i).trim().parse::<usize>();
                let end = mi.get("Chapters_Pos_End", kind, i).trim().parse::<usize>();
                if let (Ok(mut start), Ok(end)) = (begin, end) {
                    let mut chapters = XElement::new("Chapters");
                    while start < end {
                        chapters.add(XElement::with_text("Chapter", mi.get_i(start, kind, i, InfoKind::Text)).attr("TimeStamp", mi.get_i(start, kind, i, InfoKind::Name)));
                        start += 1;
                    }
                    sub.add(chapters);
                }
            }
            root.add(sub);
        }
    }
    mi.close();
    root
}
