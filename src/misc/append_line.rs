//! Append-only line writer shared across threads (processed logs, CRC32 error log, ...).

use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::sync::Mutex;

#[derive(Default)]
pub struct AppendLineManager {
    writers: Mutex<HashMap<String, File>>,
}

impl AppendLineManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Append `line` (plus newline) to `file_path`, creating the file if needed.
    pub fn append_line(&self, file_path: &str, line: &str) -> io::Result<()> {
        let mut writers = self.writers.lock().unwrap_or_else(|e| e.into_inner());
        if !writers.contains_key(file_path) {
            let f = OpenOptions::new().create(true).append(true).open(file_path)?;
            writers.insert(file_path.to_string(), f);
        }
        if line.is_empty() {
            return Ok(());
        }
        let f = writers.get_mut(file_path).expect("just inserted");
        f.write_all(line.as_bytes())?;
        f.write_all(b"\n")?;
        f.flush()
    }

    pub fn clear(&self) {
        self.writers.lock().unwrap_or_else(|e| e.into_inner()).clear();
    }
}
