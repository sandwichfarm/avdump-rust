//! Deterministic file system traversal (files first, sorted; then sub directories, sorted).

use std::path::Path;

pub fn traverse(
    entries: &[String],
    include_sub_folders: bool,
    on_file: &mut dyn FnMut(&str),
    on_error: &mut dyn FnMut(String),
) {
    for path in entries {
        let p = Path::new(path);
        if !p.is_dir() && !p.is_file() {
            on_error(format!("Path not found: {path}"));
        }
    }
    for path in entries.iter().filter(|p| Path::new(p).is_file()) {
        on_file(path);
    }
    let dirs: Vec<String> = entries.iter().filter(|p| Path::new(p).is_dir()).cloned().collect();
    traverse_directories(&dirs, include_sub_folders, on_file, on_error);
}

pub fn traverse_directories(
    directories: &[String],
    include_sub_folders: bool,
    on_file: &mut dyn FnMut(&str),
    on_error: &mut dyn FnMut(String),
) {
    for dir in directories {
        let rd = match std::fs::read_dir(dir) {
            Ok(rd) => rd,
            Err(e) => {
                on_error(format!("{dir}: {e}"));
                continue;
            }
        };
        let mut files = Vec::new();
        let mut subdirs = Vec::new();
        for entry in rd {
            match entry {
                Ok(entry) => {
                    let path = entry.path();
                    let path_str = path.to_string_lossy().into_owned();
                    // Follow symlinks like Directory.EnumerateFiles does.
                    match std::fs::metadata(&path) {
                        Ok(md) if md.is_dir() => subdirs.push(path_str),
                        Ok(_) => files.push(path_str),
                        Err(e) => on_error(format!("{path_str}: {e}")),
                    }
                }
                Err(e) => on_error(format!("{dir}: {e}")),
            }
        }
        files.sort();
        subdirs.sort();
        for f in &files {
            on_file(f);
        }
        if include_sub_folders {
            traverse_directories(&subdirs, true, on_file, on_error);
        }
    }
}
