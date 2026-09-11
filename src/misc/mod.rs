//! Small shared helpers.

pub mod append_line;
pub mod base_convert;
pub mod file_traversal;
pub mod xml;

use std::path::Path;

/// Split the file name of `path` into `(stem, extension)` the way .NET's `Path` helpers do:
/// the extension includes the leading dot and is empty when there is none.
pub fn file_name_parts(path: &Path) -> (String, String) {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    match name.rfind('.') {
        Some(idx) if idx > 0 || name.len() > 1 => (name[..idx].to_string(), name[idx..].to_string()),
        _ => (name, String::new()),
    }
}

/// `Path.GetFileName` equivalent (empty string when there is none).
pub fn file_name(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

/// `Path.GetDirectoryName` equivalent (empty string when there is none).
pub fn directory_name(path: &Path) -> String {
    path.parent().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default()
}

/// Ensure the directory chain for a path exists. When `is_directory` is false the parent
/// directory of `path` is created instead.
pub fn create_directory_chain(path: &str, is_directory: bool) -> std::io::Result<()> {
    if path.is_empty() {
        return Ok(());
    }
    let p = Path::new(path);
    let dir = if is_directory { Some(p) } else { p.parent() };
    match dir {
        Some(d) if !d.as_os_str().is_empty() => std::fs::create_dir_all(d),
        _ => Ok(()),
    }
}

/// `Path.GetFullPath` equivalent: absolute path without touching the file system beyond cwd.
pub fn full_path(path: &str) -> String {
    let p = Path::new(path);
    if p.is_absolute() {
        normalize(p)
    } else {
        match std::env::current_dir() {
            Ok(cwd) => normalize(&cwd.join(p)),
            Err(_) => path.to_string(),
        }
    }
}

fn normalize(p: &Path) -> String {
    use std::path::Component;
    let mut out = std::path::PathBuf::new();
    for c in p.components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out.to_string_lossy().into_owned()
}

/// Number of logical processors (never zero).
pub fn processor_count() -> usize {
    std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1)
}
