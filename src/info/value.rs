//! Dynamically typed metadata values plus .NET-compatible formatting helpers.

use std::fmt::Write as _;

/// A tag target inside a `TargetedTag`.
#[derive(Debug, Clone, PartialEq)]
pub struct TagTarget {
    pub kind: TagTargetKind,
    pub id: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TagTargetKind {
    Track,
    Chapters,
    Chapter,
    Attachment,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Tag {
    pub name: String,
    pub value: TagValue,
    pub language: String,
    pub is_default: bool,
    pub children: Vec<Tag>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TagValue {
    Text(String),
    Binary(Vec<u8>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct TargetedTag {
    pub targets: Vec<TagTarget>,
    pub tags: Vec<Tag>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChapterTitle {
    pub title: String,
    pub languages: Vec<String>,
    pub countries: Vec<String>,
}

impl std::fmt::Display for ChapterTitle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} Languages({}) Countries({})", self.title, self.languages.join(", "), self.countries.join(", "))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Dimensions {
    pub width: i32,
    pub height: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CropSides {
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
    pub left: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChromaSubsampling {
    pub y: i32,
    pub cb: i32,
    pub cr: i32,
}

pub mod stereo_modes {
    pub const INVALID: u32 = 0;
    pub const MONO: u32 = 1 << 0;
    pub const LEFT_RIGHT: u32 = 1 << 1;
    pub const TOP_BOTTOM: u32 = 1 << 2;
    pub const CHECKBOARD: u32 = 1 << 3;
    pub const ROW_INTERLEAVED: u32 = 1 << 4;
    pub const COLUMN_INTERLEAVED: u32 = 1 << 5;
    pub const FRAME_ALTERNATING: u32 = 1 << 6;
    pub const ANAGLYPH: u32 = 1 << 7;
    pub const CYAN_RED: u32 = 1 << 8;
    pub const GREEN_MAGENTA: u32 = 1 << 9;
    pub const REVERSED: u32 = 1 << 30;
    pub const OTHER: u32 = 1 << 31;

    pub fn to_string(v: u32) -> String {
        if v == 0 {
            return "Invalid".to_string();
        }
        let names = [
            (MONO, "Mono"),
            (LEFT_RIGHT, "LeftRight"),
            (TOP_BOTTOM, "TopBottom"),
            (CHECKBOARD, "Checkboard"),
            (ROW_INTERLEAVED, "RowInterleaved"),
            (COLUMN_INTERLEAVED, "ColumnInterleaved"),
            (FRAME_ALTERNATING, "FrameAlternating"),
            (ANAGLYPH, "AnaGlyph"),
            (CYAN_RED, "CyanRed"),
            (GREEN_MAGENTA, "GreenMagenta"),
            (REVERSED, "Reversed"),
            (OTHER, "Other"),
        ];
        names.iter().filter(|(b, _)| v & b != 0).map(|(_, n)| *n).collect::<Vec<_>>().join(", ")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayUnit {
    Invalid,
    Pixel,
    Meter,
    AspectRatio,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AspectRatioBehavior {
    Invalid,
    FreeResizing,
    KeepAR,
    Fixed,
    Unknown,
}

/// A metadata value. The variant determines the `t` attribute in the AVD3 report.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Str(String),
    Bool(bool),
    I32(i32),
    I64(i64),
    U64(u64),
    F64(f64),
    /// Unix timestamp in seconds (UTC).
    DateTime(f64),
    /// Duration in seconds.
    TimeSpan(f64),
    Binary(Vec<u8>),
    Strings(Vec<String>),
    Dimensions(Dimensions),
    CropSides(CropSides),
    ChromaSubsampling(ChromaSubsampling),
    StereoModes(u32),
    DisplayUnit(DisplayUnit),
    AspectRatioBehavior(AspectRatioBehavior),
    SampleRateHistogram(Vec<(f64, i64)>),
    Tags(Vec<TargetedTag>),
    ChapterTitles(Vec<ChapterTitle>),
}

impl Value {
    /// .NET type name of the value (`Type.Name`), as printed in the `t` attribute.
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Str(_) => "String",
            Value::Bool(_) => "Boolean",
            Value::I32(_) => "Int32",
            Value::I64(_) => "Int64",
            Value::U64(_) => "UInt64",
            Value::F64(_) => "Double",
            Value::DateTime(_) => "DateTime",
            Value::TimeSpan(_) => "TimeSpan",
            Value::Binary(_) => "Binary",
            Value::Strings(_) => "ImmutableArray`1",
            Value::Dimensions(_) => "Dimensions",
            Value::CropSides(_) => "CropSides",
            Value::ChromaSubsampling(_) => "ChromeSubsampling",
            Value::StereoModes(_) => "StereoModes",
            Value::DisplayUnit(_) => "DisplayUnit",
            Value::AspectRatioBehavior(_) => "AspectRatioBehavior",
            Value::SampleRateHistogram(_) => "List`1",
            Value::Tags(_) => "List`1",
            Value::ChapterTitles(_) => "ImmutableArray`1",
        }
    }

    /// Whether the value is rendered as a list of `<Item>` elements.
    pub fn is_collection(&self) -> bool {
        matches!(self, Value::Strings(_) | Value::SampleRateHistogram(_) | Value::Tags(_) | Value::ChapterTitles(_))
    }

    /// Items of a collection value, each rendered like `.ToString()`.
    pub fn collection_items(&self) -> Vec<String> {
        match self {
            Value::Strings(v) => v.clone(),
            Value::SampleRateHistogram(v) => v.iter().map(|(r, c)| format!("{}, {}", format_f64(*r), c)).collect(),
            Value::Tags(v) => v.iter().map(format_targeted_tag).collect(),
            Value::ChapterTitles(v) => v.iter().map(|t| t.to_string()).collect(),
            _ => Vec::new(),
        }
    }

    /// Scalar rendering (`.ToString()` semantics).
    pub fn to_display_string(&self) -> String {
        match self {
            Value::Str(s) => s.clone(),
            Value::Bool(b) => if *b { "True".into() } else { "False".into() },
            Value::I32(v) => v.to_string(),
            Value::I64(v) => v.to_string(),
            Value::U64(v) => v.to_string(),
            Value::F64(v) => format_f64(*v),
            Value::DateTime(v) => format_datetime(*v),
            Value::TimeSpan(v) => format_timespan(*v),
            Value::Binary(b) => crate::hashes::to_hex_upper(b),
            Value::Dimensions(d) => format!("{}, {}", d.width, d.height),
            Value::CropSides(c) => format!("{}, {}, {}, {}", c.top, c.right, c.bottom, c.left),
            Value::ChromaSubsampling(c) => format!("{}:{}:{}", c.y, c.cb, c.cr),
            Value::StereoModes(m) => stereo_modes::to_string(*m),
            Value::DisplayUnit(u) => format!("{:?}", u),
            Value::AspectRatioBehavior(a) => format!("{:?}", a),
            other => other.collection_items().join(", "),
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }
    pub fn as_strings(&self) -> Option<&[String]> {
        match self {
            Value::Strings(s) => Some(s),
            _ => None,
        }
    }
    pub fn as_bytes(&self) -> Option<&[u8]> {
        match self {
            Value::Binary(b) => Some(b),
            _ => None,
        }
    }
}

fn format_targeted_tag(t: &TargetedTag) -> String {
    let targets: Vec<String> = t
        .targets
        .iter()
        .map(|x| format!("{:?}={}", x.kind, x.id))
        .collect();
    let tags: Vec<String> = t.tags.iter().map(format_tag).collect();
    format!("Targets({}) Tags({})", targets.join(", "), tags.join("; "))
}

fn format_tag(t: &Tag) -> String {
    let value = match &t.value {
        TagValue::Text(s) => s.clone(),
        TagValue::Binary(b) => format!("<{} bytes>", b.len()),
    };
    let mut s = format!("{}={}", t.name, value);
    if !t.children.is_empty() {
        let _ = write!(s, " [{}]", t.children.iter().map(format_tag).collect::<Vec<_>>().join("; "));
    }
    s
}

/// Shortest round-trip rendering, like .NET Core's `double.ToString()`.
pub fn format_f64(v: f64) -> String {
    if v.is_nan() {
        return "NaN".to_string();
    }
    if v.is_infinite() {
        return if v > 0.0 { "Infinity".to_string() } else { "-Infinity".to_string() };
    }
    if v == v.trunc() && v.abs() < 1e15 {
        return format!("{}", v as i64);
    }
    let s = format!("{}", v);
    // Rust prints e.g. 1e-7 as "0.0000001"; .NET switches to E notation for tiny/huge values.
    if v.abs() < 1e-4 || v.abs() >= 1e15 {
        format_e_notation(v)
    } else {
        s
    }
}

fn format_e_notation(v: f64) -> String {
    let s = format!("{:e}", v);
    // Rust: "1.5e-7" → .NET: "1.5E-07"
    if let Some((mant, exp)) = s.split_once('e') {
        let exp_i: i32 = exp.parse().unwrap_or(0);
        format!("{}E{}{:02}", mant, if exp_i < 0 { "-" } else { "+" }, exp_i.abs())
    } else {
        s
    }
}

/// Civil date from days since the unix epoch (proleptic Gregorian).
pub fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Format a unix timestamp like `DateTime.ToString()` under the invariant culture
/// (`MM/dd/yyyy HH:mm:ss`).
pub fn format_datetime(unix_seconds: f64) -> String {
    let secs = unix_seconds.floor() as i64;
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    format!("{:02}/{:02}/{:04} {:02}:{:02}:{:02}", m, d, y, rem / 3600, (rem % 3600) / 60, rem % 60)
}

/// ISO-8601 rendering of a unix timestamp (used for file names and logs).
pub fn format_datetime_iso(unix_seconds: f64) -> String {
    let secs = unix_seconds.floor() as i64;
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}", y, m, d, rem / 3600, (rem % 3600) / 60, rem % 60)
}

/// Format seconds like `TimeSpan.ToString()` (`[-][d.]hh:mm:ss[.fffffff]`).
pub fn format_timespan(seconds: f64) -> String {
    let negative = seconds < 0.0;
    let total_ticks = (seconds.abs() * 10_000_000.0).round() as u64;
    let ticks = total_ticks % 10_000_000;
    let total_secs = total_ticks / 10_000_000;
    let days = total_secs / 86_400;
    let rem = total_secs % 86_400;
    let mut s = String::new();
    if negative {
        s.push('-');
    }
    if days > 0 {
        let _ = write!(s, "{}.", days);
    }
    let _ = write!(s, "{:02}:{:02}:{:02}", rem / 3600, (rem % 3600) / 60, rem % 60);
    if ticks != 0 {
        let _ = write!(s, ".{:07}", ticks);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f64_formatting() {
        assert_eq!(format_f64(24.0), "24");
        assert_eq!(format_f64(23.976023976023978), "23.976023976023978");
        assert_eq!(format_f64(0.5), "0.5");
        assert_eq!(format_f64(1.5e-7), "1.5E-07");
    }

    #[test]
    fn datetime_formatting() {
        assert_eq!(format_datetime(0.0), "01/01/1970 00:00:00");
        assert_eq!(format_datetime(978_307_200.0), "01/01/2001 00:00:00");
        assert_eq!(format_datetime_iso(1_700_000_000.0), "2023-11-14 22:13:20");
    }

    #[test]
    fn timespan_formatting() {
        assert_eq!(format_timespan(0.0), "00:00:00");
        assert_eq!(format_timespan(3661.5), "01:01:01.5000000");
        assert_eq!(format_timespan(90_000.0), "1.01:00:00");
    }
}
