//! Application errors and the XML error files written with `--SaveErrors`.

use crate::misc::xml::XElement;
use crate::processing::ProcessingError;
use std::time::{SystemTime, UNIX_EPOCH};

/// Data attached to an error that may identify the user (file names); only written to error
/// files when `--IncludePersonalData` is given, otherwise replaced by a session-salted hash.
#[derive(Debug, Clone)]
pub struct SensitiveData(pub String);

#[derive(Debug, Clone)]
pub enum ErrorDatum {
    Plain(String),
    Sensitive(SensitiveData),
}

#[derive(Debug, Clone)]
pub struct AvdError {
    pub type_name: String,
    pub message: String,
    pub data: Vec<(String, ErrorDatum)>,
    pub cause: Option<Box<AvdError>>,
    /// Unix timestamp (seconds, fractional).
    pub thrown_on: f64,
    pub remedy: Option<String>,
}

fn now_unix() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
}

impl AvdError {
    pub fn new(type_name: &str, message: impl Into<String>) -> Self {
        Self { type_name: type_name.to_string(), message: message.into(), data: Vec::new(), cause: None, thrown_on: now_unix(), remedy: None }
    }

    pub fn ui(message: impl Into<String>) -> Self {
        Self::new("AVD3UIException", message)
    }

    pub fn with_cause(mut self, cause: AvdError) -> Self {
        self.cause = Some(Box::new(cause));
        self
    }

    pub fn with_sensitive(mut self, key: &str, value: impl Into<String>) -> Self {
        self.data.push((key.to_string(), ErrorDatum::Sensitive(SensitiveData(value.into()))));
        self
    }

    pub fn with_data(mut self, key: &str, value: impl Into<String>) -> Self {
        self.data.push((key.to_string(), ErrorDatum::Plain(value.into())));
        self
    }

    pub fn from_processing(e: &ProcessingError) -> Self {
        let mut out = Self::new(e.type_name(), e.message.clone());
        for (k, v) in &e.data {
            out.data.push((k.clone(), if k == "StreamTag" || k == "FileName" { ErrorDatum::Sensitive(SensitiveData(v.clone())) } else { ErrorDatum::Plain(v.clone()) }));
        }
        if let Some(c) = &e.cause {
            out.cause = Some(Box::new(Self::from_processing(c)));
        }
        out
    }

    /// Innermost error.
    pub fn base(&self) -> &AvdError {
        match &self.cause {
            Some(c) => c.base(),
            None => self,
        }
    }

    pub fn to_xml(&self, skip_environment: bool, include_personal_data: bool, effective_args: &[String]) -> XElement {
        let mut root = XElement::new(&self.type_name).attr("thrownOn", format_thrown_on(self.thrown_on));
        if !skip_environment {
            root.add(environment_element());
        }
        root.add(XElement::with_text("Message", &self.message));
        if include_personal_data && !effective_args.is_empty() {
            let mut args = XElement::new("EffectiveCommandLineArguments");
            for a in effective_args {
                args.add(XElement::with_text("Argument", a));
            }
            root.add(args);
        }
        root.add(self.data_element(include_personal_data));
        let mut cause = XElement::new("Cause");
        if let Some(c) = &self.cause {
            cause.add(c.to_xml_inner(include_personal_data));
        }
        root.add(cause);
        root
    }

    fn to_xml_inner(&self, include_personal_data: bool) -> XElement {
        let mut e = XElement::new(&self.type_name).attr("thrownOn", format_thrown_on(self.thrown_on));
        e.add(XElement::with_text("Message", &self.message));
        e.add(self.data_element(include_personal_data));
        let mut cause = XElement::new("Cause");
        if let Some(c) = &self.cause {
            cause.add(c.to_xml_inner(include_personal_data));
        }
        e.add(cause);
        e
    }

    fn data_element(&self, include_personal_data: bool) -> XElement {
        let mut data = XElement::new("Data");
        for (k, v) in &self.data {
            let text = match v {
                ErrorDatum::Plain(s) => s.clone(),
                ErrorDatum::Sensitive(s) => {
                    if include_personal_data { s.0.clone() } else { format!("Hidden({})", session_hash(&s.0)) }
                }
            };
            data.add(XElement::with_text(k, text));
        }
        data
    }
}

impl std::fmt::Display for AvdError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

fn format_thrown_on(unix: f64) -> String {
    let frac = ((unix - unix.floor()) * 10_000.0) as u32;
    format!("{}.{:04}", crate::info::value::format_datetime_iso(unix), frac)
}

/// Timestamp component of an error file name (`yyyyMMdd HHmmssffff`).
pub fn error_file_stamp(unix: f64) -> String {
    let secs = unix.floor() as i64;
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (y, m, d) = crate::info::value::civil_from_days(days);
    let frac = ((unix - unix.floor()) * 10_000.0) as u32;
    format!("{:04}{:02}{:02} {:02}{:02}{:02}{:04}", y, m, d, rem / 3600, (rem % 3600) / 60, rem % 60, frac)
}

fn session_id() -> &'static str {
    use std::sync::OnceLock;
    static ID: OnceLock<String> = OnceLock::new();
    ID.get_or_init(|| {
        let t = now_unix().to_bits();
        let pid = std::process::id() as u64;
        format!("{:016x}{:08x}", t ^ 0x9E37_79B9_7F4A_7C15u64.wrapping_mul(pid | 1), pid)
    })
}

fn session_hash(value: &str) -> String {
    use digest::Digest;
    let mut h = sha2::Sha512::new();
    h.update(session_id().as_bytes());
    h.update(value.as_bytes());
    crate::hashes::to_hex_upper(&h.finalize())
}

fn environment_element() -> XElement {
    let mut e = XElement::new("Information");
    e.add(XElement::with_text("EntryAssemblyVersion", crate::settings::help::VERSION));
    e.add(XElement::with_text("LibVersion", crate::settings::help::VERSION));
    e.add(XElement::with_text("Session", session_id()));
    e.add(XElement::with_text("Framework", format!("rust {}", option_env!("AVD3_RUSTC_VERSION").unwrap_or("unknown"))));
    e.add(XElement::with_text("OSVersion", std::env::consts::OS));
    e.add(XElement::with_text("ProcessArchitecture", std::env::consts::ARCH));
    e.add(XElement::with_text("Is64BitProcess", if cfg!(target_pointer_width = "64") { "True" } else { "False" }));
    e.add(XElement::with_text("ProcessorCount", crate::misc::processor_count().to_string()));
    e
}
