//! `CPY` consumer: writes the stream to another file while it is being hashed.

use super::BlockConsumer;
use crate::processing::{BlockStreamReader, CancelToken, ProcessingError};
use std::any::Any;
use std::io::Write;
use std::path::PathBuf;

pub struct CopyToFileConsumer {
    name: String,
    file_path: PathBuf,
}

impl CopyToFileConsumer {
    pub fn new(name: &str, file_path: PathBuf) -> Self {
        Self { name: name.to_string(), file_path }
    }
    pub fn file_path(&self) -> &std::path::Path {
        &self.file_path
    }
}

impl BlockConsumer for CopyToFileConsumer {
    fn name(&self) -> &str {
        &self.name
    }

    fn do_work(&mut self, reader: &mut BlockStreamReader, ct: &CancelToken) -> Result<(), ProcessingError> {
        if let Some(parent) = self.file_path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let mut file = std::fs::File::create(&self.file_path)?;
        loop {
            ct.check()?;
            let block = reader.get_block(reader.suggested_read_length())?;
            file.write_all(&block)?;
            let len = block.len();
            if !reader.advance(len) {
                break;
            }
        }
        file.flush()?;
        Ok(())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
