//! `FileMetaInfo`: all providers for one file plus the condensed (merged) view.

use super::meta::MetaProvider;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct FileMetaInfo {
    pub path: PathBuf,
    /// Path with symlinks resolved (falls back to `path`).
    pub resolved_path: PathBuf,
    /// Size of the resolved file in bytes.
    pub resolved_length: u64,
    pub providers: Vec<MetaProvider>,
    pub condensed_providers: Vec<MetaProvider>,
}

impl FileMetaInfo {
    pub fn new(path: &Path, providers: Vec<MetaProvider>) -> Self {
        let resolved_path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        let resolved_length = std::fs::metadata(&resolved_path).or_else(|_| std::fs::metadata(path)).map(|m| m.len()).unwrap_or(0);

        let mut condensed: Vec<MetaProvider> = Vec::new();
        let mut seen: Vec<String> = Vec::new();
        for p in &providers {
            let ty = p.container_type().to_string();
            if seen.contains(&ty) {
                continue;
            }
            seen.push(ty.clone());
            let group: Vec<&MetaProvider> = providers.iter().filter(|x| x.container_type() == ty).collect();
            if group.len() > 1 {
                condensed.push(MetaProvider::composite(&ty, &group));
            } else {
                condensed.push(p.clone());
            }
        }
        Self { path: path.to_path_buf(), resolved_path, resolved_length, providers, condensed_providers: condensed }
    }

    pub fn full_name(&self) -> String {
        self.path.to_string_lossy().into_owned()
    }

    pub fn file_name(&self) -> String {
        crate::misc::file_name(&self.path)
    }

    /// Extension including the leading dot (empty when none).
    pub fn extension(&self) -> String {
        crate::misc::file_name_parts(&self.path).1
    }

    pub fn file_name_without_extension(&self) -> String {
        crate::misc::file_name_parts(&self.path).0
    }

    pub fn directory_name(&self) -> String {
        crate::misc::directory_name(&self.path)
    }

    pub fn condensed(&self, container_type: &str) -> Option<&MetaProvider> {
        self.condensed_providers.iter().find(|p| p.container_type() == container_type)
    }

    pub fn provider(&self, name: &str) -> Option<&MetaProvider> {
        self.providers.iter().find(|p| p.name == name)
    }

    /// Suggested file extensions determined by the media providers.
    pub fn suggested_extensions(&self) -> Vec<String> {
        self.condensed(super::meta::container_types::MEDIA_PROVIDER)
            .and_then(|p| p.select("SuggestedFileExtension"))
            .and_then(|i| i.value.as_strings().map(|s| s.to_vec()))
            .unwrap_or_default()
    }

    /// Hash digest by consumer name (e.g. `ED2K`, `CRC32`).
    pub fn hash(&self, name: &str) -> Option<&[u8]> {
        self.condensed(super::meta::container_types::HASH_PROVIDER)
            .and_then(|p| p.root.items.iter().find(|i| i.key.eq_ignore_ascii_case(name)))
            .and_then(|i| i.value.as_bytes())
    }
}
