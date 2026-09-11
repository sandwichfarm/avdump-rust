//! Command line argument parsing (mirrors `CLSettingsHandler.ParseArgs`).
//!
//! Accepted forms: `--Name`, `--Name=Value`, `--NameSpace.Name=Value`, `-X` (single letter),
//! `-RX` (several single-letter switches), `--Help`, `--Help=<NameSpace>`.
//! Special leading argument `FROMFILE <path>` reads additional arguments from a file
//! (one per line, `//` comments allowed). Arguments consisting only of digits and upper case
//! letters (`PRINTARGS`, `UTF8OUT`, ...) are treated as host flags and skipped.

use super::{all_properties, Settings, SettingProperty, ValueKind};
use regex::Regex;
use std::sync::OnceLock;

#[derive(Debug, Clone, Default)]
pub struct ParseResult {
    pub success: bool,
    pub message: String,
    pub print_help: bool,
    pub print_help_topic: String,
    pub raw_args: Vec<String>,
    pub unnamed_args: Vec<String>,
    /// `(property index, value)` in the order given.
    pub setting_values: Vec<(usize, Option<String>)>,
}

fn arg_pattern() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^--?(?:([a-zA-Z0-9]+)\.)?([a-zA-Z0-9_][a-zA-Z0-9\-]*)(?:=(.*))?$").expect("valid regex"))
}

/// Is this a host flag (all digits/upper case, e.g. `PRINTARGS`)?
pub fn is_host_flag(arg: &str) -> bool {
    !arg.is_empty() && arg.chars().all(|c| c.is_ascii_digit() || c.is_ascii_uppercase())
}

/// Expand `FROMFILE <path> [args...]`.
pub fn expand_from_file(args: &[String]) -> Result<Vec<String>, String> {
    if args.first().map(|a| a == "FROMFILE").unwrap_or(false) {
        let path = match args.get(1) {
            Some(p) if std::path::Path::new(p).is_file() => p,
            _ => return Err("FROMFILE: File not found".to_string()),
        };
        let content = std::fs::read_to_string(path).map_err(|e| format!("FROMFILE: {e}"))?;
        let mut out: Vec<String> = content
            .lines()
            .map(|l| l.replace('\r', ""))
            .filter(|l| !l.starts_with("//") && !l.trim().is_empty())
            .collect();
        out.extend(args.iter().skip(2).cloned());
        return Ok(out);
    }
    Ok(args.to_vec())
}

struct PreArg {
    raw: String,
    namespace: Option<String>,
    name: String,
    param: Option<String>,
}

fn preprocess(args: &[String], out: &mut Vec<PreArg>) -> Result<(), String> {
    for arg in args {
        if !arg.starts_with('-') {
            out.push(PreArg { raw: arg.clone(), namespace: None, name: String::new(), param: None });
            continue;
        }
        let caps = arg_pattern().captures(arg).ok_or_else(|| format!("Invalid argument structure: {arg}"))?;
        let namespace = caps.get(1).map(|m| m.as_str().to_string());
        let name = caps.get(2).map(|m| m.as_str().to_string()).unwrap_or_default();
        let param = caps.get(3).map(|m| m.as_str().to_string());
        if name.is_empty() {
            return Err("Invalid argument structure".to_string());
        }
        if !arg.starts_with("--") {
            // Single dash: one-letter switches, possibly several at once.
            let ns = namespace.as_ref().map(|n| format!("{n}.")).unwrap_or_default();
            if name.chars().count() > 1 {
                if param.is_some() {
                    return Err("Multiple one letter arguments may not have parameters".to_string());
                }
                let expanded: Vec<String> = name.chars().map(|c| format!("--{ns}{c}")).collect();
                preprocess(&expanded, out)?;
            } else {
                let expanded = vec![format!("--{ns}{name}{}", param.as_ref().map(|p| format!("={p}")).unwrap_or_default())];
                preprocess(&expanded, out)?;
            }
        } else {
            out.push(PreArg { raw: arg.clone(), namespace, name, param });
        }
    }
    Ok(())
}

/// Parse `args` against the property registry.
pub fn parse_args(properties: &[SettingProperty], args: &[String]) -> Result<ParseResult, String> {
    let args = expand_from_file(args)?;
    let args: Vec<String> = args.into_iter().filter(|a| !a.trim().is_empty() && !is_host_flag(a)).collect();

    if args.is_empty() {
        return Ok(ParseResult { success: true, message: "Empty Args, printing help.".into(), print_help: true, raw_args: args, ..Default::default() });
    }

    let mut pre = Vec::new();
    preprocess(&args, &mut pre)?;

    let mut result = ParseResult { success: true, message: "OK".into(), raw_args: args.clone(), ..Default::default() };

    for a in pre {
        if a.name.is_empty() {
            result.unnamed_args.push(a.raw);
            continue;
        }
        if a.name.to_ascii_lowercase().starts_with("help") || a.name == "h" {
            result.print_help = true;
            if let Some(p) = a.param.filter(|p| !p.is_empty()) {
                result.print_help_topic = p;
            }
            continue;
        }
        let single_letter = a.name.chars().count() == 1;
        let matches_name = |candidate: &str| if single_letter { candidate == a.name } else { candidate.eq_ignore_ascii_case(&a.name) };
        let candidates: Vec<usize> = properties
            .iter()
            .enumerate()
            .filter(|(_, p)| a.namespace.as_ref().map(|ns| ns.eq_ignore_ascii_case(p.group.name())).unwrap_or(true))
            .filter(|(_, p)| matches_name(p.name) || p.alternative_names.iter().any(|alt| matches_name(alt)))
            .map(|(i, _)| i)
            .collect();
        let display = format!("{}{}", a.namespace.as_ref().map(|n| format!("{n}.")).unwrap_or_default(), a.name);
        match candidates.len() {
            0 => return Err(format!("Argument ({display}) is not registered")),
            1 => {}
            _ => {
                let names: Vec<String> = candidates.iter().map(|&i| properties[i].full_name()).collect();
                return Err(format!("Argument reference is ambiguous: {}", names.join(", ")));
            }
        }
        let idx = candidates[0];
        let value = match (&a.param, properties[idx].kind) {
            (Some(p), _) => Some(p.clone()),
            (None, ValueKind::Bool) => None,
            (None, ValueKind::Text) => None,
        };
        result.setting_values.push((idx, value));
    }
    Ok(result)
}

/// Parse and apply, returning the settings and the unnamed (path) arguments.
pub fn parse_settings(args: &[String]) -> Result<(Settings, ParseResult), String> {
    let properties = all_properties();
    let result = parse_args(&properties, args)?;
    let mut settings = Settings::default();
    for (idx, value) in &result.setting_values {
        let p = &properties[*idx];
        settings.apply(p, value.as_deref()).map_err(|e| format!("Property ({}) could not be set: {e}", p.full_name()))?;
    }
    Ok((settings, result))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn empty_args_print_help() {
        let r = parse_args(&all_properties(), &[]).unwrap();
        assert!(r.print_help);
    }

    #[test]
    fn parses_named_and_unnamed() {
        let (s, r) = parse_settings(&args(&["--Recursive", "--Consumers=CRC32,ED2K", "-R", "/tmp/a", "--Concurrent=2:/mnt,1"])).unwrap();
        assert!(s.recursive);
        assert_eq!(s.consumers.as_ref().unwrap().len(), 2);
        assert_eq!(r.unnamed_args, vec!["/tmp/a"]);
        assert_eq!(s.concurrent.concurrent_count, 2);
        assert_eq!(s.concurrent.partitions[0].path, "/mnt");
    }

    #[test]
    fn namespaced_and_case_insensitive() {
        let (s, _) = parse_settings(&args(&["--filemove.mode=PlaceholderInline", "--reporting.printhashes", "--BLength=16"])).unwrap();
        assert_eq!(s.file_move_mode, super::super::FileMoveMode::PlaceholderInline);
        assert!(s.print_hashes);
        assert_eq!(s.buffer_length, 16 << 20);
    }

    #[test]
    fn consumers_without_value_means_list() {
        let (s, _) = parse_settings(&args(&["--Consumers"])).unwrap();
        assert!(s.consumers.is_none());
        let (s, _) = parse_settings(&args(&["--Reports"])).unwrap();
        assert!(s.reports.is_none());
        let (s, _) = parse_settings(&args(&["--Consumers=TTH:4,NULL:8"])).unwrap();
        let c = s.consumers.unwrap();
        assert_eq!(c[0].arguments, vec!["4"]);
        assert_eq!(c[1].name, "NULL");
    }

    #[test]
    fn unknown_and_ambiguous_arguments() {
        assert!(parse_settings(&args(&["--Nope"])).unwrap_err().contains("not registered"));
        assert!(parse_settings(&args(&["--LogPath=x"])).is_ok());
        assert!(parse_settings(&args(&["--Test"])).is_ok());
    }

    #[test]
    fn host_flags_are_ignored() {
        let (_, r) = parse_settings(&args(&["PRINTARGS", "UTF8OUT", "--Recursive"])).unwrap();
        assert_eq!(r.raw_args, vec!["--Recursive"]);
    }

    #[test]
    fn help_topic() {
        let r = parse_args(&all_properties(), &args(&["--Help=Reporting"])).unwrap();
        assert!(r.print_help);
        assert_eq!(r.print_help_topic, "Reporting");
    }

    #[test]
    fn with_extensions_exclusion() {
        let (s, _) = parse_settings(&args(&["--WExts=-mkv,avi"])).unwrap();
        assert!(!s.with_extensions.allow);
        assert_eq!(s.with_extensions.items, vec!["mkv", "avi"]);
    }

    #[test]
    fn crc32_error_and_null_stream() {
        let (s, _) = parse_settings(&args(&["--CRC32Error=err.txt", "--NullStreamTest=2:100:1"])).unwrap();
        assert_eq!(s.crc32_error.as_ref().unwrap().1, "(?i)${CRC32}");
        assert_eq!(s.null_stream_test.stream_length, 100 << 20);
        assert!(parse_settings(&args(&["--CRC32Error=err.txt,(["])).is_err());
    }
}
