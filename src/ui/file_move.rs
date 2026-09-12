//! Placeholder expansion (`${Name}`) and the file move/rename scripts.

use crate::info::FileMetaInfo;
use crate::misc::base_convert;
use crate::settings::FileMoveMode;
use regex::Regex;
use std::collections::HashMap;
use std::path::Path;
use std::sync::OnceLock;
use std::time::SystemTime;

fn placeholder_pattern() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\$\{([A-Za-z0-9\-\.]+)\}").expect("valid regex"))
}

fn hash_token_pattern() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^Hash-([^-]+)-(\d+|32Hex|32Z)-(UC|LC|OC)$").expect("valid regex"))
}

/// Resolve one placeholder key against the file's metadata (`ReplaceToken` in the original).
pub fn replace_token(key: &str, fmi: &FileMetaInfo, additional: Option<&HashMap<String, String>>) -> Option<String> {
    let mut value = match key {
        "FileSize" => Some(fmi.resolved_length.to_string()),
        "FullName" => Some(fmi.full_name()),
        "FileName" => Some(fmi.file_name()),
        "FileExtension" => Some(fmi.extension()),
        "FileNameWithoutExtension" => Some(fmi.file_name_without_extension()),
        "DirectoryName" => Some(fmi.directory_name()),
        _ => None,
    };
    if key.starts_with("SuggestedExtension") {
        let exts = fmi.suggested_extensions();
        value = Some(match exts.first().and_then(|e| e.split(' ').next()) {
            Some(e) if !e.is_empty() => format!(".{e}"),
            _ => fmi.extension(),
        });
    }
    if key.starts_with("Hash") {
        if let Some(c) = hash_token_pattern().captures(key) {
            let hash_name = &c[1];
            let base = &c[2];
            let letter_case = &c[3];
            if let (Some(digest), Some(digits)) = (fmi.hash(hash_name), base_convert::digits_for(base)) {
                let s = base_convert::to_base(digest, digits);
                value = Some(match letter_case {
                    "UC" => s.to_uppercase(),
                    "LC" => s.to_lowercase(),
                    _ => s,
                });
            }
        }
    }
    if let Some(add) = additional {
        if let Some(v) = add.get(key) {
            value = Some(v.clone());
        }
    }
    value
}

/// Expand every `${Name}` in `pattern`. Unknown placeholders expand to an empty string.
pub fn expand_placeholders(pattern: &str, fmi: &FileMetaInfo, additional: Option<&HashMap<String, String>>) -> String {
    placeholder_pattern()
        .replace_all(pattern, |caps: &regex::Captures| replace_token(&caps[1], fmi, additional).unwrap_or_default())
        .into_owned()
}

/// A loaded file move script.
pub struct FileMoveScript {
    mode: FileMoveMode,
    source: String,
    pattern: Option<String>,
    last_write: Option<SystemTime>,
}

impl FileMoveScript {
    pub fn new(mode: FileMoveMode, source: &str) -> Result<Self, String> {
        if source.is_empty() {
            return Err("FileMove.Pattern may not be empty".to_string());
        }
        match mode {
            FileMoveMode::PlaceholderInline | FileMoveMode::PlaceholderFile => Ok(Self { mode, source: source.to_string(), pattern: None, last_write: None }),
            FileMoveMode::CSharpScriptInline | FileMoveMode::CSharpScriptFile | FileMoveMode::DotNetAssembly => {
                Err(format!("FileMove.Mode={} is not supported by this port; use PlaceholderInline or PlaceholderFile", mode.name()))
            }
            FileMoveMode::None => Err("FileMove.Mode is None".to_string()),
        }
    }

    pub fn can_reload(&self) -> bool {
        matches!(self.mode, FileMoveMode::PlaceholderFile)
    }

    pub fn load(&mut self) -> Result<(), String> {
        match self.mode {
            FileMoveMode::PlaceholderInline => self.pattern = Some(self.source.clone()),
            FileMoveMode::PlaceholderFile => {
                let text = std::fs::read_to_string(&self.source).map_err(|e| format!("Could not read FileMove script {}: {e}", self.source))?;
                self.last_write = std::fs::metadata(&self.source).ok().and_then(|m| m.modified().ok());
                // The script file is a placeholder pattern; ignore trailing line breaks.
                self.pattern = Some(text.trim_end_matches(['\r', '\n']).to_string());
            }
            _ => return Err("unsupported FileMove.Mode".to_string()),
        }
        Ok(())
    }

    pub fn source_changed(&self) -> bool {
        if !self.can_reload() {
            return false;
        }
        let now = std::fs::metadata(&self.source).ok().and_then(|m| m.modified().ok());
        now != self.last_write
    }

    /// The destination path for the file, made absolute.
    pub fn get_file_path(&self, fmi: &FileMetaInfo) -> Option<String> {
        let pattern = self.pattern.as_ref()?;
        let dest = expand_placeholders(pattern, fmi, None);
        if dest.is_empty() {
            return None;
        }
        Some(crate::misc::full_path(&dest))
    }
}

/// Apply replacements / disable flags to a computed destination (shared by test and real mode).
pub fn finalize_destination(mut dest: String, fmi: &FileMetaInfo, replacements: &[(String, String)], disable_move: bool, disable_rename: bool) -> String {
    for (from, to) in replacements {
        if !from.is_empty() {
            dest = dest.replace(from, to);
        }
    }
    if disable_move {
        dest = Path::new(&fmi.directory_name()).join(crate::misc::file_name(Path::new(&dest))).to_string_lossy().into_owned();
    }
    if disable_rename {
        dest = Path::new(&crate::misc::directory_name(Path::new(&dest))).join(fmi.file_name()).to_string_lossy().into_owned();
    }
    dest
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::info::meta::{container_types, MetaProvider};
    use crate::info::value::Value;

    fn fmi() -> FileMetaInfo {
        let mut hp = MetaProvider::new("HashProvider", container_types::HASH_PROVIDER);
        hp.add("CRC32", "", Value::Binary(vec![0xCB, 0xF4, 0x39, 0x26]));
        let mut mp = MetaProvider::new("Test", container_types::MEDIA_PROVIDER);
        mp.add("SuggestedFileExtension", "", Value::Strings(vec!["mkv".into()]));
        FileMetaInfo::new(Path::new("/data/anime/Show - 01 [ABCD1234].avi"), vec![hp, mp])
    }

    #[test]
    fn expands_basic_tokens() {
        let f = fmi();
        assert_eq!(expand_placeholders("${DirectoryName}/${FileNameWithoutExtension}${SuggestedExtension}", &f, None), "/data/anime/Show - 01 [ABCD1234].mkv");
        assert_eq!(expand_placeholders("${FileName}|${FileExtension}|${Nope}", &f, None), "Show - 01 [ABCD1234].avi|.avi|");
    }

    #[test]
    fn expands_hash_tokens() {
        let f = fmi();
        assert_eq!(expand_placeholders("${Hash-CRC32-16-UC}", &f, None), "CBF43926");
        assert_eq!(expand_placeholders("${Hash-CRC32-16-LC}", &f, None), "cbf43926");
        assert_eq!(expand_placeholders("${Hash-CRC32-10-OC}", &f, None), "3421780262");
        assert_eq!(expand_placeholders("${Hash-MD5-16-UC}", &f, None), "");
    }

    #[test]
    #[cfg(unix)]
    fn additional_tokens_and_finalize() {
        let f = fmi();
        let mut add = HashMap::new();
        add.insert("ReportName".to_string(), "AVD3".to_string());
        assert_eq!(expand_placeholders("${FileName}.${ReportName}.xml", &f, Some(&add)), "Show - 01 [ABCD1234].avi.AVD3.xml");
        let dest = finalize_destination("/other/dir/new.mkv".into(), &f, &[("new".into(), "renamed".into())], false, false);
        assert_eq!(dest, "/other/dir/renamed.mkv");
        let dest = finalize_destination("/other/dir/new.mkv".into(), &f, &[], true, false);
        assert_eq!(dest, "/data/anime/new.mkv");
        let dest = finalize_destination("/other/dir/new.mkv".into(), &f, &[], false, true);
        assert_eq!(dest, "/other/dir/Show - 01 [ABCD1234].avi");
    }

    #[test]
    #[cfg(unix)]
    fn inline_script() {
        let mut s = FileMoveScript::new(FileMoveMode::PlaceholderInline, "${DirectoryName}/x${FileExtension}").unwrap();
        assert!(!s.can_reload());
        s.load().unwrap();
        assert_eq!(s.get_file_path(&fmi()).unwrap(), "/data/anime/x.avi");
        assert!(FileMoveScript::new(FileMoveMode::CSharpScriptInline, "x").is_err());
    }
}
