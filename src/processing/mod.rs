//! The processing pipeline: data is read once from a source into a circular (mirrored) buffer and
//! handed to any number of block consumers in parallel without ever being copied.

pub mod block_stream;
pub mod cancel;
pub mod consumers;
pub mod mirrored_buffer;
pub mod progress;
pub mod stream_consumer;
pub mod stream_provider;

pub use block_stream::{BlockStream, BlockStreamReader};
pub use cancel::CancelToken;
pub use mirrored_buffer::{MirroredBuffer, MirroredBufferPool};

use std::fmt;

/// Error produced anywhere inside the processing pipeline.
#[derive(Debug, Clone)]
pub struct ProcessingError {
    pub kind: ProcessingErrorKind,
    pub message: String,
    pub data: Vec<(String, String)>,
    pub cause: Option<Box<ProcessingError>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessingErrorKind {
    Cancelled,
    Io,
    Consumer,
    Other,
}

impl ProcessingError {
    pub fn new(kind: ProcessingErrorKind, message: impl Into<String>) -> Self {
        Self { kind, message: message.into(), data: Vec::new(), cause: None }
    }
    pub fn cancelled() -> Self {
        Self::new(ProcessingErrorKind::Cancelled, "Operation was cancelled")
    }
    pub fn other(message: impl Into<String>) -> Self {
        Self::new(ProcessingErrorKind::Other, message)
    }
    pub fn io(err: &std::io::Error) -> Self {
        Self::new(ProcessingErrorKind::Io, err.to_string())
    }
    pub fn with_data(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.data.push((key.into(), value.into()));
        self
    }
    pub fn with_cause(mut self, cause: ProcessingError) -> Self {
        self.cause = Some(Box::new(cause));
        self
    }
    pub fn is_cancelled(&self) -> bool {
        self.kind == ProcessingErrorKind::Cancelled
            || self.cause.as_ref().map(|c| c.is_cancelled()).unwrap_or(false)
    }
    /// Innermost cause (`Exception.GetBaseException`).
    pub fn base(&self) -> &ProcessingError {
        match &self.cause {
            Some(c) => c.base(),
            None => self,
        }
    }
    pub fn type_name(&self) -> &'static str {
        match self.kind {
            ProcessingErrorKind::Cancelled => "OperationCanceledException",
            ProcessingErrorKind::Io => "IOException",
            ProcessingErrorKind::Consumer => "StreamConsumerException",
            ProcessingErrorKind::Other => "Exception",
        }
    }
}

impl fmt::Display for ProcessingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)?;
        if let Some(c) = &self.cause {
            write!(f, " ({c})")?;
        }
        Ok(())
    }
}

impl std::error::Error for ProcessingError {}

impl From<std::io::Error> for ProcessingError {
    fn from(e: std::io::Error) -> Self {
        ProcessingError::io(&e)
    }
}
