//! File format detection by magic bytes and content heuristics (`FormatInfoProvider`).

use crate::info::meta::{container_types as ct, keys, MetaInfoContainer, MetaProvider};
use crate::info::value::Value;
use regex::Regex;
use std::path::Path;
use std::sync::OnceLock;

const MAX_SCAN: usize = 10 << 20;

/// Sample of a file: the first bytes plus the total length.
pub struct FileSample {
    pub data: Vec<u8>,
    pub length: u64,
}

impl FileSample {
    pub fn read(path: &Path) -> Option<Self> {
        use std::io::Read;
        let mut f = std::fs::File::open(path).ok()?;
        let length = f.metadata().ok()?.len();
        let want = (length as usize).min(MAX_SCAN);
        let mut data = Vec::with_capacity(want);
        f.by_ref().take(want as u64).read_to_end(&mut data).ok()?;
        Some(Self { data, length })
    }

    /// Text decoded like `StreamReader` with BOM detection (UTF-8 default).
    pub fn text(&self) -> String {
        decode_text(&self.data)
    }
}

fn decode_text(data: &[u8]) -> String {
    if data.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(&data[3..]).into_owned();
    }
    if data.starts_with(&[0xFF, 0xFE]) {
        let units: Vec<u16> = data[2..].chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        return String::from_utf16_lossy(&units);
    }
    if data.starts_with(&[0xFE, 0xFF]) {
        let units: Vec<u16> = data[2..].chunks_exact(2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect();
        return String::from_utf16_lossy(&units);
    }
    String::from_utf8_lossy(data).into_owned()
}

/// Lines (non-empty) of `text`, stopping at the first line longer than `max_line_length`.
fn read_lines(text: &str, max_line_length: usize) -> Vec<&str> {
    let mut out = Vec::new();
    for line in text.split(['\r', '\n']) {
        if line.chars().count() > max_line_length {
            break;
        }
        if !line.is_empty() {
            out.push(line);
        }
    }
    out
}

fn regex(cache: &'static OnceLock<Regex>, pattern: &str) -> &'static Regex {
    cache.get_or_init(|| Regex::new(pattern).expect("valid regex"))
}

/// One detectable file type.
struct FileType {
    magic: Vec<Option<Vec<u8>>>,
    magic_pos: Vec<usize>,
    offset: i64,
    identifier: Option<String>,
    container_type: &'static str,
    needs_more_magic_bytes: bool,
    is_candidate: bool,
    possible_extensions: Vec<&'static str>,
    kind: Kind,
    idx: Option<Idx>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Srt,
    AssSsa,
    Idx,
    Sami,
    SevenZip,
    Zip,
    Rar,
    Ra,
    Flac,
    Lrc,
    Avi,
    Sub,
    Tmp,
    Pjs,
    Webvtt,
    Js,
    Rt,
    Smil,
    Tts,
    Xss,
    ZeroG,
    Sup,
    FanSubber,
    Sasami2k,
}

impl FileType {
    fn new(kind: Kind, magics: Vec<Vec<u8>>, identifier: Option<&str>, container_type: &'static str, exts: Vec<&'static str>) -> Self {
        let needs = magics.iter().any(|m| !m.is_empty());
        Self {
            magic_pos: vec![0; magics.len()],
            magic: magics.into_iter().map(Some).collect(),
            offset: 0,
            identifier: identifier.map(|s| s.to_string()),
            container_type,
            needs_more_magic_bytes: needs,
            is_candidate: true,
            possible_extensions: exts,
            kind,
            idx: None,
        }
    }

    fn check_magic_byte(&mut self, b: u8) {
        if !self.needs_more_magic_bytes {
            return;
        }
        if self.offset > 0 {
            self.offset -= 1;
            return;
        }
        let mut needs_more = false;
        let mut is_candidate = false;
        for i in 0..self.magic.len() {
            match &self.magic[i] {
                None => is_candidate = true,
                Some(m) => {
                    if m.len() > self.magic_pos[i] {
                        let matched = m[self.magic_pos[i]] == b;
                        self.magic_pos[i] += 1;
                        is_candidate |= matched;
                        if m.len() == self.magic_pos[i] && is_candidate {
                            self.magic[i] = None;
                            continue;
                        }
                    }
                    if let Some(m) = &self.magic[i] {
                        needs_more |= is_candidate && m.len() != self.magic_pos[i];
                    }
                }
            }
        }
        self.needs_more_magic_bytes &= needs_more;
        self.is_candidate &= is_candidate;
    }

    fn elaborate_check(&mut self, sample: &FileSample) {
        if !self.is_candidate {
            return;
        }
        let d = &sample.data;
        match self.kind {
            Kind::Srt => {
                if sample.length > 10 * 1024 * 1024 {
                    self.is_candidate = false;
                    return;
                }
                static RE: OnceLock<Regex> = OnceLock::new();
                let re = regex(&RE, r"^(\d{1,4} ?: ?\d{1,4} ?: ?\d{1,4} ?[,:.] ?\d{1,4}) ?--> ?(\d{1,4} ?: ?\d{1,4} ?: ?\d{1,4} ?[,:.] ?\d{1,4})");
                let text = sample.text();
                let mut count = 0;
                for line in read_lines(&text, 1024) {
                    if count == 0 && line.to_lowercase().contains("webvtt") {
                        break;
                    }
                    if re.is_match(line) {
                        count += 1;
                    }
                    if count > 20 {
                        break;
                    }
                }
                if count == 0 {
                    self.is_candidate = false;
                }
            }
            Kind::AssSsa => {
                // Skip Matroska files.
                if d.len() >= 4 && d[0] == 0x1A && d[1] == 0x45 && d[2] == 0xDF && d[3] == 0xA3 {
                    self.is_candidate = false;
                    return;
                }
                let text = decode_text(&d[..d.len().min(4096)]);
                let str: String = text.chars().take(2048).collect::<String>().to_lowercase();
                let find = |hay: &str, needle: &str, from: usize, len: usize| -> Option<usize> {
                    let end = (from + len).min(hay.len());
                    if from > hay.len() || from > end {
                        return None;
                    }
                    hay.get(from..end).and_then(|s| s.find(needle)).map(|p| from + p)
                };
                let mut pos: Option<usize> = None;
                if let Some(p) = str.find(" styles]") {
                    pos = find(&str, "v4", p.saturating_sub(4), 10).map(|x| x + 2);
                }
                if pos.is_none() {
                    if let Some(p) = str.find(" styles]") {
                        pos = find(&str, "v3", p.saturating_sub(4), 10).map(|x| x + 2);
                    }
                }
                if pos.is_none() {
                    match str.find("scripttype:") {
                        None => {
                            self.is_candidate = false;
                            return;
                        }
                        Some(p) => match find(&str, "v4.00", p, 20) {
                            None => {
                                // try v3
                                match find(&str, "v3.00", p, 20) {
                                    None => {
                                        self.is_candidate = false;
                                        return;
                                    }
                                    Some(q) => pos = Some(q + 5),
                                }
                            }
                            Some(q) => pos = Some(q + 5),
                        },
                    }
                }
                let pos = pos.unwrap();
                if (pos + 2) as u64 <= sample.length {
                    let c = str.as_bytes().get(pos).copied().unwrap_or(b' ');
                    let ext = if c == b'+' { "ass" } else { "ssa" };
                    self.possible_extensions = vec![ext];
                    if let Some(id) = &mut self.identifier {
                        id.push_str(ext);
                    }
                } else {
                    self.is_candidate = false;
                }
            }
            Kind::Idx => {
                if sample.length > 10 * 1024 * 1024 {
                    self.is_candidate = false;
                    return;
                }
                let text = sample.text();
                let first = read_lines(&text, 1024).first().copied().unwrap_or("");
                self.is_candidate = first.contains("VobSub index file, v");
                if self.is_candidate {
                    self.idx = Idx::parse(&text.replace('\0', "#"));
                }
            }
            Kind::Lrc => {
                static RE: OnceLock<Regex> = OnceLock::new();
                let re = regex(&RE, r"\[\d\d:\d\d\.\d\d\].*");
                self.ratio_check(&sample.text(), |l| re.is_match(l), 0.8);
            }
            Kind::Tmp => {
                static RE: OnceLock<Regex> = OnceLock::new();
                let re = regex(&RE, r"^\d\d:\d\d:\d\d(\.\d)?:.*");
                self.ratio_check(&sample.text(), |l| re.is_match(l), 0.8);
            }
            Kind::Pjs => {
                static RE: OnceLock<Regex> = OnceLock::new();
                let re = regex(&RE, "^\\s*\\d*,\\s*\\d*,\".*\"");
                let text = sample.text();
                let (mut i, mut matches) = (0usize, 0usize);
                let mut check_line = String::new();
                for line in read_lines(&text, 1024) {
                    check_line.push_str(line);
                    if !line.contains(",\"") || line.ends_with('"') {
                        if re.is_match(&check_line) {
                            matches += 1;
                        }
                        check_line.clear();
                        i += 1;
                    }
                }
                if i == 0 || (matches as f64 / i as f64) < 0.8 || matches == 0 {
                    self.is_candidate = false;
                }
            }
            Kind::Js => {
                static RE: OnceLock<Regex> = OnceLock::new();
                let re = regex(&RE, r"^\d*:\d*:\d*\.\d*\s*\d*:\d*:\d*\.\d*.*");
                let text = sample.text();
                let (mut i, mut matches) = (0usize, 0usize);
                let mut has_continuation = false;
                let mut has_magic = false;
                for line in read_lines(&text, 1024) {
                    has_magic |= line.to_lowercase().contains("jaco");
                    if line.starts_with('#') || line.is_empty() {
                        continue;
                    }
                    let is_start = re.is_match(line);
                    if has_continuation || is_start {
                        matches += 1;
                    }
                    has_continuation = (is_start && line.ends_with('\\')) || (has_continuation && line.ends_with('\\'));
                    i += 1;
                }
                let ratio = if i == 0 { 0.0 } else { matches as f64 / i as f64 };
                let is_match = (ratio > 0.5 && has_magic) || ratio > 0.8;
                if !is_match || matches == 0 {
                    self.is_candidate = false;
                }
            }
            Kind::Tts => {
                static RE: OnceLock<Regex> = OnceLock::new();
                let re = regex(&RE, r"^\d*:\d*:\d*\.\d*,\d*:\d*:\d*\.\d*,.*");
                let text = sample.text();
                let (mut i, mut matches) = (0usize, 0usize);
                for line in read_lines(&text, 1024).iter().map(|l| l.trim()) {
                    if line.starts_with('#') || line.is_empty() {
                        continue;
                    }
                    let is_match = re.is_match(line);
                    if is_match {
                        matches += 1;
                    } else {
                        let n = line.chars().count();
                        let good = line.chars().filter(|c| c.is_alphabetic() || c.is_whitespace() || c.is_ascii_punctuation()).count();
                        if n > 0 && good as f64 / n as f64 > 0.8 {
                            continue;
                        }
                    }
                    i += 1;
                }
                if i == 0 || (matches as f64 / i as f64) < 0.8 || matches == 0 {
                    self.is_candidate = false;
                }
            }
            Kind::Rt => {
                let text = sample.text();
                let (mut j, mut matches) = (0usize, 0usize);
                for line in read_lines(&text, 1024).iter().map(|l| l.to_lowercase()) {
                    if line.is_empty() || !line.starts_with('<') {
                        continue;
                    }
                    j += 1;
                    if line.starts_with("<window") || line.starts_with("<time begin") {
                        matches += 1;
                    }
                }
                if j == 0 || (matches as f64 / j as f64) < 0.6 || matches == 0 || j < 10 {
                    self.is_candidate = false;
                }
            }
            Kind::Xss => {
                let text = decode_text(&d[..d.len().min(4096)]);
                let s: String = text.chars().take(2048).collect::<String>().to_lowercase();
                self.is_candidate = s.contains("script=xombiesub");
            }
            Kind::Webvtt => {
                let text = sample.text();
                let mut score = 0;
                for line in text.lines() {
                    let l = line.to_lowercase();
                    if l.starts_with("region") || l.starts_with("style") || l.starts_with("note") {
                        score += 10;
                    }
                    if l.starts_with("-->") {
                        score += 1;
                    }
                }
                self.is_candidate = score > 10;
            }
            Kind::Sup => {
                self.is_candidate = d.len() >= 12 && d[0] == 0x50 && d[1] == 0x47 && d[10] == 0x16 && d[11] == 0x00;
            }
            Kind::Smil => {
                let text = sample.text();
                let matches = read_lines(&text, 1024).iter().filter(|l| l.to_lowercase().trim().starts_with("<smil>")).count();
                if matches == 0 {
                    self.is_candidate = false;
                }
            }
            Kind::FanSubber => {
                static RE: OnceLock<Regex> = OnceLock::new();
                let re = regex(&RE, r"FanSubber v[0-9]+(\.[0-9]+)?");
                let text = sample.text();
                let first = read_lines(&text, 1024).first().copied().unwrap_or("");
                self.is_candidate &= re.is_match(first);
            }
            Kind::Sami => {
                let text = decode_text(&d[..d.len().min(4096)]);
                let s: String = text.chars().take(2048).collect::<String>().to_lowercase();
                let head: String = s.chars().take(10).collect();
                if !head.contains("<sami>") {
                    self.is_candidate = false;
                }
            }
            Kind::Sub => {
                if Self::subviewer(d) {
                    if let Some(id) = &mut self.identifier {
                        id.push_str("subviewer");
                    }
                } else if Self::micro_dvd(sample) {
                    if let Some(id) = &mut self.identifier {
                        id.push_str("microdvd");
                    }
                } else {
                    self.is_candidate = false;
                }
            }
            Kind::ZeroG => {
                let text = decode_text(&d[..d.len().min(4096)]);
                let s: String = text.chars().take(2048).collect::<String>().to_lowercase();
                if !s.contains("% zerog") {
                    self.is_candidate = false;
                }
            }
            Kind::Avi => {
                self.is_candidate &= d.len() >= 16 && &d[8..16] == b"AVI LIST";
            }
            _ => {}
        }
    }

    fn ratio_check(&mut self, text: &str, pred: impl Fn(&str) -> bool, threshold: f64) {
        let (mut i, mut matches) = (0usize, 0usize);
        for line in read_lines(text, 1024) {
            if pred(line) {
                matches += 1;
            }
            i += 1;
        }
        if i == 0 || (matches as f64 / i as f64) < threshold || matches == 0 {
            self.is_candidate = false;
        }
    }

    fn subviewer(d: &[u8]) -> bool {
        let text = decode_text(&d[..d.len().min(2048)]);
        let s: String = text.chars().take(1024).collect::<String>().to_uppercase();
        let keys = ["[BEGIN]", "[INFORMATION]", "[TITLE]", "[AUTHOR]", "[SOURCE]", "[PRG]", "[FILEPATH]", "[DELAY]", "[CD TRACK]", "[COMMENT]", "[END INFORMATION]", "[SUBTITLE]", "[COLF]", "[STYLE]", "[SIZE]", "[FONT]"];
        keys.iter().filter(|k| s.contains(*k)).count() >= 5
    }

    fn micro_dvd(sample: &FileSample) -> bool {
        if sample.length > 10 * 1024 * 1024 {
            return false;
        }
        static RE: OnceLock<Regex> = OnceLock::new();
        let re = regex(&RE, r"^\{\d*\}\{\d*\}.*$");
        let text = sample.text();
        let (mut count, mut matches) = (0usize, 0usize);
        for line in read_lines(&text, 1024).iter().map(|l| l.trim()) {
            if line.is_empty() {
                continue;
            }
            if re.is_match(line) {
                matches += 1;
            }
            count += 1;
            if count > 20 {
                break;
            }
        }
        count > 4 && matches as f64 / count as f64 > 0.8
    }

    fn add_info(&self, provider: &mut MetaProvider) {
        match self.kind {
            Kind::SevenZip | Kind::Zip | Kind::Rar => {
                if let Some(id) = &self.identifier {
                    provider.add("FileTypeIdentifier", keys::DIMENSIONLESS, Value::Str(id.clone()));
                }
            }
            Kind::Idx if self.idx.is_some() => {
                let idx = self.idx.as_ref().unwrap();
                for (i, sub) in idx.subtitles.iter().enumerate() {
                    if sub.subtitle_count == 0 {
                        continue;
                    }
                    let mut c = MetaInfoContainer::new(i as u64, self.container_type);
                    provider.add_to(&mut c, "ContainerCodecId", keys::DIMENSIONLESS, self.identifier.clone().map(Value::Str));
                    let lang = if !sub.language.is_empty() { sub.language.clone() } else { sub.language_id.clone() };
                    provider.add_to(&mut c, "Language", keys::DIMENSIONLESS, Some(Value::Str(lang)));
                    provider.add_to(&mut c, "Index", keys::DIMENSIONLESS, Some(Value::I32(sub.index)));
                    provider.add_node(c);
                }
            }
            _ => {
                if let Some(id) = &self.identifier {
                    if !id.is_empty() {
                        let mut c = MetaInfoContainer::new(0, self.container_type);
                        provider.add_to(&mut c, "ContainerCodecId", keys::DIMENSIONLESS, Some(Value::Str(id.clone())));
                        provider.add_node(c);
                    }
                }
            }
        }
    }
}

struct IdxSub {
    index: i32,
    language_id: String,
    language: String,
    subtitle_count: usize,
}

struct Idx {
    subtitles: Vec<IdxSub>,
}

impl Idx {
    fn parse(source: &str) -> Option<Self> {
        static TAG: OnceLock<Regex> = OnceLock::new();
        static ID: OnceLock<Regex> = OnceLock::new();
        static ALT: OnceLock<Regex> = OnceLock::new();
        let tag = regex(&TAG, r"(?m)^([^#\r\n]+?):[ ]?([^\r\n]+)");
        let id_re = regex(&ID, r"([^,]+), index: (.+)");
        let alt_re = regex(&ALT, r"alt: ([^\r\n]+)");
        let matches: Vec<regex::Captures> = tag.captures_iter(source).collect();
        let mut subs: Vec<IdxSub> = Vec::new();
        for (i, m) in matches.iter().enumerate() {
            let key = m.get(1).map(|x| x.as_str().trim()).unwrap_or("");
            let value = m.get(2).map(|x| x.as_str()).unwrap_or("");
            if key == "timestamp" {
                if let Some(last) = subs.last_mut() {
                    last.subtitle_count += 1;
                }
            } else if key == "id" {
                let sm = id_re.captures(value)?;
                let language_id = sm.get(1)?.as_str().to_string();
                let index: i32 = sm.get(2)?.as_str().trim().parse().ok()?;
                let start = m.get(0)?.start();
                let end = matches.get(i + 1).and_then(|n| n.get(0)).map(|n| n.start()).unwrap_or(source.len());
                let language = alt_re.captures(&source[start..end]).and_then(|c| c.get(1)).map(|x| x.as_str().to_string()).unwrap_or_default();
                subs.push(IdxSub { index, language_id, language, subtitle_count: 0 });
            }
        }
        Some(Self { subtitles: subs })
    }
}

fn all_file_types() -> Vec<FileType> {
    let s = |x: &str| x.as_bytes().to_vec();
    vec![
        FileType::new(Kind::Srt, vec![vec![]], Some("text/srt"), ct::SUBTITLE_STREAM, vec!["srt"]),
        FileType::new(Kind::AssSsa, vec![vec![]], Some("text/"), ct::SUBTITLE_STREAM, vec!["ssa", "ass"]),
        FileType::new(Kind::Idx, vec![vec![]], Some("text/idx"), ct::SUBTITLE_STREAM, vec!["idx"]),
        FileType::new(Kind::Sami, vec![vec![]], Some("text/sami"), ct::SUBTITLE_STREAM, vec!["smi"]),
        FileType::new(Kind::SevenZip, vec![vec![0x37, 0x7A, 0xBC, 0xAF, 0x27, 0x1C]], Some("compression/7z"), ct::MEDIA_STREAM, vec!["7z"]),
        FileType::new(Kind::Zip, vec![vec![0x50, 0x4b, 0x03, 0x04], vec![0x50, 0x4b, 0x30, 0x30, 0x50, 0x4b]], Some("compression/zip"), ct::MEDIA_STREAM, vec!["zip"]),
        FileType::new(Kind::Rar, vec![vec![0x52, 0x61, 0x72, 0x21]], Some("compression/rar"), ct::MEDIA_STREAM, vec!["rar"]),
        FileType::new(Kind::Ra, vec![vec![b'.', b'r', b'a', 0xFD]], None, ct::AUDIO_STREAM, vec!["ra"]),
        FileType::new(Kind::Flac, vec![s("fLaC")], None, ct::AUDIO_STREAM, vec!["flac"]),
        FileType::new(Kind::Lrc, vec![vec![]], Some("text/lyric"), ct::SUBTITLE_STREAM, vec!["lrc"]),
        FileType::new(Kind::Avi, vec![s("RIFF")], None, ct::VIDEO_STREAM, vec!["avi"]),
        FileType::new(Kind::Sub, vec![vec![]], Some("text/"), ct::SUBTITLE_STREAM, vec!["sub"]),
        FileType::new(Kind::Tmp, vec![vec![]], Some("text/TMPlayer"), ct::SUBTITLE_STREAM, vec!["tmp"]),
        FileType::new(Kind::Pjs, vec![vec![]], Some("text/PhoenixJapanimationSociety"), ct::SUBTITLE_STREAM, vec!["pjs"]),
        FileType::new(Kind::Webvtt, vec![vec![0xFE, 0xFF, b'W', b'E', b'B', b'V', b'T', b'T'], s("WEBVTT")], Some("text/vtt"), ct::SUBTITLE_STREAM, vec!["vtt"]),
        FileType::new(Kind::Js, vec![vec![]], Some("text/JS"), ct::SUBTITLE_STREAM, vec!["js"]),
        FileType::new(Kind::Rt, vec![vec![]], Some("text/RT"), ct::SUBTITLE_STREAM, vec!["rt"]),
        FileType::new(Kind::Smil, vec![vec![]], Some("SMIL"), ct::MEDIA_STREAM, vec!["smil"]),
        FileType::new(Kind::Tts, vec![vec![]], Some("text/TTS"), ct::SUBTITLE_STREAM, vec!["tts"]),
        FileType::new(Kind::Xss, vec![vec![]], Some("text/XombieSub"), ct::SUBTITLE_STREAM, vec!["xss"]),
        FileType::new(Kind::ZeroG, vec![vec![]], Some("text/ZeroG"), ct::SUBTITLE_STREAM, vec!["zeg"]),
        FileType::new(Kind::Sup, vec![vec![]], Some("text/SubtitleBitmapFile"), ct::SUBTITLE_STREAM, vec!["sup"]),
        FileType::new(Kind::FanSubber, vec![s("FanSubber v")], Some("text/FanSubber"), ct::SUBTITLE_STREAM, vec!["fsb"]),
        FileType::new(Kind::Sasami2k, vec![s("// translated by Sami2Sasami")], Some("text/Sasami2k"), ct::SUBTITLE_STREAM, vec!["s2k"]),
    ]
}

pub struct FormatInfoProvider;

impl FormatInfoProvider {
    pub const NAME: &'static str = "FormatInfoProvider";

    pub fn create(path: &Path) -> MetaProvider {
        let mut p = MetaProvider::new(Self::NAME, ct::MEDIA_PROVIDER);
        if let Some(sample) = FileSample::read(path) {
            Self::detect(&mut p, &sample);
        }
        p
    }

    /// Run detection on an in-memory sample (exposed for tests).
    pub fn detect(provider: &mut MetaProvider, sample: &FileSample) {
        let mut types = all_file_types();
        let mut pos = 0usize;
        let mut needs_more = true;
        while needs_more {
            if pos >= sample.data.len() {
                // Ran out of data (EOF or 10 MiB scanned) before all magic sequences were decided.
                return;
            }
            let b = sample.data[pos];
            pos += 1;
            needs_more = false;
            for t in types.iter_mut().filter(|t| t.is_candidate) {
                t.check_magic_byte(b);
                needs_more |= t.needs_more_magic_bytes;
            }
        }
        for t in types.iter_mut().filter(|t| t.is_candidate) {
            t.elaborate_check(sample);
        }

        let mut candidates: Vec<&FileType> = types.iter().filter(|t| t.is_candidate).collect();
        if candidates.len() != 1 {
            for kind in [Kind::Zip, Kind::SevenZip, Kind::Rar] {
                if candidates.iter().any(|t| t.kind == kind) {
                    candidates = types.iter().filter(|t| t.kind == kind).collect();
                }
            }
        }
        if candidates.is_empty() {
            return;
        }
        let exts: Vec<&str> = candidates.iter().flat_map(|t| t.possible_extensions.iter().copied()).collect();
        if !exts.is_empty() {
            provider.add("SuggestedFileExtension", keys::DIMENSIONLESS, Value::Strings(vec![exts.join(" ")]));
        }
        if candidates.len() == 1 {
            candidates[0].add_info(provider);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn detect(data: &[u8]) -> Option<String> {
        let sample = FileSample { data: data.to_vec(), length: data.len() as u64 };
        let mut p = MetaProvider::new("test", ct::MEDIA_PROVIDER);
        FormatInfoProvider::detect(&mut p, &sample);
        p.select("SuggestedFileExtension").and_then(|i| i.value.as_strings().map(|s| s.join(" ")))
    }

    #[test]
    fn detects_srt() {
        let srt = b"1\n00:00:01,000 --> 00:00:02,000\nHello\n\n2\n00:00:03,000 --> 00:00:04,000\nWorld\n";
        assert_eq!(detect(srt).as_deref(), Some("srt"));
    }

    #[test]
    fn detects_archives() {
        assert_eq!(detect(&[0x50, 0x4b, 0x03, 0x04, 0, 0, 0, 0, 0, 0]).as_deref(), Some("zip"));
        assert_eq!(detect(&[0x52, 0x61, 0x72, 0x21, 0x1A, 0x07, 0x00]).as_deref(), Some("rar"));
        assert_eq!(detect(&[0x37, 0x7A, 0xBC, 0xAF, 0x27, 0x1C, 0, 0]).as_deref(), Some("7z"));
    }

    #[test]
    fn detects_ass() {
        let ass = b"[Script Info]\nScriptType: v4.00+\n\n[V4+ Styles]\nFormat: Name\n\n[Events]\nDialogue: 0,0:00:01.00,0:00:02.00,Default,,0,0,0,,Hi\n";
        assert_eq!(detect(ass).as_deref(), Some("ass"));
    }

    #[test]
    fn detects_flac_and_avi() {
        assert_eq!(detect(b"fLaC\0\0\0\x22\0\0\0\0").as_deref(), Some("flac"));
        assert_eq!(detect(b"RIFF\0\0\0\0AVI LIST\0\0\0\0").as_deref(), Some("avi"));
        assert_eq!(detect(b"RIFF\0\0\0\0WAVEfmt \0\0\0\0"), None);
    }

    #[test]
    fn random_binary_is_unknown() {
        let data: Vec<u8> = (0..4096u32).map(|i| (i.wrapping_mul(2654435761u32) >> 24) as u8).collect();
        assert_eq!(detect(&data), None);
    }
}
