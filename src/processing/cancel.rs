//! Cooperative cancellation shared by all pipeline threads.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[derive(Clone, Default, Debug)]
pub struct CancelToken {
    flag: Arc<AtomicBool>,
}

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn cancel(&self) {
        self.flag.store(true, Ordering::SeqCst);
    }
    pub fn is_cancelled(&self) -> bool {
        self.flag.load(Ordering::SeqCst)
    }
    pub fn check(&self) -> Result<(), super::ProcessingError> {
        if self.is_cancelled() {
            Err(super::ProcessingError::cancelled())
        } else {
            Ok(())
        }
    }
}
