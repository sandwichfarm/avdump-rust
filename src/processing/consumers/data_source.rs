//! Forward-only cursor over a [`BlockStreamReader`] used by the container parsers.
//!
//! Reads are served straight from the shared buffer; the reader position is only committed
//! (advanced) in larger steps so the producer is not woken up for every tiny element.

use crate::processing::{BlockStreamReader, ProcessingError};
use std::borrow::Cow;

pub struct ReaderDataSource<'a> {
    reader: &'a mut BlockStreamReader,
    offset: usize,
}

impl<'a> ReaderDataSource<'a> {
    pub fn new(reader: &'a mut BlockStreamReader) -> Self {
        Self { reader, offset: 0 }
    }

    pub fn position(&self) -> u64 {
        self.reader.bytes_read() + self.offset as u64
    }

    pub fn length(&self) -> u64 {
        self.reader.length()
    }

    pub fn is_end_of_stream(&self) -> bool {
        self.position() >= self.reader.length()
    }

    /// Largest amount of data that can be requested at once.
    pub fn max_request(&self) -> usize {
        self.reader.max_read_length()
    }

    /// Return at least `min` bytes starting at the current position (fewer only at the end).
    pub fn get(&mut self, min: usize) -> Result<Cow<'_, [u8]>, ProcessingError> {
        if self.offset >= self.reader.suggested_read_length() {
            self.commit();
        }
        let block = self.reader.get_block(min + self.offset)?;
        let offset = self.offset;
        Ok(match block {
            Cow::Borrowed(b) => Cow::Borrowed(&b[offset.min(b.len())..]),
            Cow::Owned(v) => Cow::Owned(v[offset.min(v.len())..].to_vec()),
        })
    }

    /// Advance the cursor by `n` bytes without blocking (data must already have been fetched).
    pub fn advance(&mut self, n: usize) {
        self.offset += n;
        if self.offset >= self.reader.suggested_read_length() {
            self.commit();
        }
    }

    /// Advance the cursor by `n` bytes, blocking as needed. Returns false when the end of the
    /// stream was reached before `n` bytes could be skipped.
    pub fn skip(&mut self, n: u64) -> Result<bool, ProcessingError> {
        self.commit();
        let skipped = self.reader.skip(n)?;
        Ok(skipped == n)
    }

    /// Move to an absolute position at or after the current one.
    pub fn seek_forward(&mut self, position: u64) -> Result<bool, ProcessingError> {
        let cur = self.position();
        if position <= cur {
            return Ok(true);
        }
        self.skip(position - cur)
    }

    pub fn commit(&mut self) {
        if self.offset > 0 {
            self.reader.advance(self.offset);
            self.offset = 0;
        }
    }

    /// Read exactly `n` bytes into an owned vector (or fewer at the end of the stream).
    pub fn read_exact(&mut self, n: usize) -> Result<Vec<u8>, ProcessingError> {
        let mut out = Vec::with_capacity(n);
        while out.len() < n {
            let want = (n - out.len()).min(self.reader.suggested_read_length());
            let block = self.get(want)?;
            let take = block.len().min(n - out.len());
            if take == 0 {
                break;
            }
            out.extend_from_slice(&block[..take]);
            self.advance(take);
        }
        Ok(out)
    }

    pub fn reader(&self) -> &BlockStreamReader {
        self.reader
    }
}
