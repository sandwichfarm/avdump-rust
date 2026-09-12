//! Streaming EBML reader (element headers, values, master element nesting).

use crate::processing::consumers::data_source::ReaderDataSource;
use crate::processing::ProcessingError;

/// Largest element payload that will be materialised as a value.
pub const MAX_VALUE_LENGTH: u64 = 16 << 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ElementHeader {
    pub id: u32,
    /// `None` for unknown-size elements.
    pub size: Option<u64>,
    pub header_length: usize,
    pub data_position: u64,
}

impl ElementHeader {
    pub fn end(&self) -> Option<u64> {
        self.size.map(|s| self.data_position + s)
    }
}

struct Frame {
    id: u32,
    end: Option<u64>,
}

/// Parsed Matroska block header (`Block` / `SimpleBlock`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MatroskaBlock {
    pub track_number: u64,
    pub timecode: i16,
    pub flags: u8,
    pub frame_count_minus_one: u8,
    pub header_length: usize,
}

pub struct EbmlReader<'r, 'a> {
    src: &'r mut ReaderDataSource<'a>,
    frames: Vec<Frame>,
    current: Option<ElementHeader>,
    consumed: bool,
    /// `(parent_id, child_id) -> bool`: whether `child_id` ends an unknown-sized `parent_id`.
    unknown_size_terminator: fn(u32, u32) -> bool,
    pub strict: bool,
}

impl<'r, 'a> EbmlReader<'r, 'a> {
    pub fn new(src: &'r mut ReaderDataSource<'a>, unknown_size_terminator: fn(u32, u32) -> bool) -> Self {
        Self { src, frames: Vec::new(), current: None, consumed: false, unknown_size_terminator, strict: false }
    }

    pub fn position(&self) -> u64 {
        self.src.position()
    }

    pub fn current(&self) -> Option<&ElementHeader> {
        self.current.as_ref()
    }

    pub fn depth(&self) -> usize {
        self.frames.len()
    }

    fn finish_current(&mut self) -> Result<(), ProcessingError> {
        if let Some(cur) = self.current.take() {
            if !self.consumed {
                if let Some(end) = cur.end() {
                    self.src.seek_forward(end)?;
                }
                // Unknown size: nothing to skip, we simply continue reading children as siblings.
            }
        }
        self.consumed = false;
        Ok(())
    }

    /// Advance to the next element within the current master element. Returns `None` at the end
    /// of the parent element (or of the stream).
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Result<Option<ElementHeader>, ProcessingError> {
        self.finish_current()?;
        let frame_end = self.frames.last().and_then(|f| f.end);
        let pos = self.src.position();
        if let Some(end) = frame_end {
            if pos >= end {
                return Ok(None);
            }
        }
        if self.src.is_end_of_stream() {
            return Ok(None);
        }
        let header = match self.read_header()? {
            Some(h) => h,
            None => return Ok(None),
        };
        // Unknown-size master: stop when a sibling/top-level element shows up.
        if let Some(frame) = self.frames.last() {
            if frame.end.is_none() && (self.unknown_size_terminator)(frame.id, header.id) {
                return Ok(None);
            }
        }
        if let (Some(end), Some(elem_end)) = (frame_end, header.end()) {
            if elem_end > end && self.strict {
                return Err(ProcessingError::other("EBML element exceeds parent"));
            }
        }
        self.src.advance(header.header_length);
        self.current = Some(header);
        self.consumed = false;
        Ok(Some(header))
    }

    fn read_header(&mut self) -> Result<Option<ElementHeader>, ProcessingError> {
        let data_position_base = self.src.position();
        let block = self.src.get(12)?;
        let b = &block[..];
        if b.is_empty() {
            return Ok(None);
        }
        let id_len = vint_length(b[0]);
        if id_len == 0 || id_len > 4 || b.len() < id_len {
            return Ok(None);
        }
        let mut id: u32 = 0;
        for &byte in &b[..id_len] {
            id = (id << 8) | byte as u32;
        }
        if b.len() <= id_len {
            return Ok(None);
        }
        let size_len = vint_length(b[id_len]);
        if size_len == 0 || size_len > 8 || b.len() < id_len + size_len {
            return Ok(None);
        }
        let (size, unknown) = read_vint_value(&b[id_len..id_len + size_len]);
        let header_length = id_len + size_len;
        Ok(Some(ElementHeader {
            id,
            size: if unknown { None } else { Some(size) },
            header_length,
            data_position: data_position_base + header_length as u64,
        }))
    }

    /// Descend into the current (master) element.
    pub fn enter(&mut self) {
        if let Some(cur) = self.current.take() {
            self.frames.push(Frame { id: cur.id, end: cur.end() });
        }
        self.consumed = false;
    }

    /// Leave the innermost master element, skipping any unread children.
    pub fn leave(&mut self) -> Result<(), ProcessingError> {
        self.finish_current()?;
        if let Some(frame) = self.frames.pop() {
            if let Some(end) = frame.end {
                self.src.seek_forward(end)?;
            }
        }
        Ok(())
    }

    fn take_data(&mut self) -> Result<Vec<u8>, ProcessingError> {
        let cur = self.current.ok_or_else(|| ProcessingError::other("no current element"))?;
        let size = cur.size.ok_or_else(|| ProcessingError::other("unknown-size element has no value"))?;
        if size > MAX_VALUE_LENGTH || size as usize > self.src.max_request() {
            return Err(ProcessingError::other("element too large"));
        }
        let data = self.src.read_exact(size as usize)?;
        self.consumed = true;
        Ok(data)
    }

    pub fn read_uint(&mut self) -> Result<u64, ProcessingError> {
        let data = self.take_data()?;
        if data.len() > 8 {
            return Err(ProcessingError::other("uint too long"));
        }
        Ok(data.iter().fold(0u64, |acc, &b| (acc << 8) | b as u64))
    }

    pub fn read_int(&mut self) -> Result<i64, ProcessingError> {
        let data = self.take_data()?;
        if data.len() > 8 {
            return Err(ProcessingError::other("int too long"));
        }
        if data.is_empty() {
            return Ok(0);
        }
        let mut v: i64 = if data[0] & 0x80 != 0 { -1 } else { 0 };
        for &b in &data {
            v = (v << 8) | b as i64;
        }
        Ok(v)
    }

    pub fn read_float(&mut self) -> Result<f64, ProcessingError> {
        let data = self.take_data()?;
        match data.len() {
            0 => Ok(0.0),
            4 => Ok(f32::from_be_bytes([data[0], data[1], data[2], data[3]]) as f64),
            8 => Ok(f64::from_be_bytes([data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7]])),
            _ => Err(ProcessingError::other("invalid float length")),
        }
    }

    pub fn read_string(&mut self) -> Result<String, ProcessingError> {
        let data = self.take_data()?;
        let end = data.iter().position(|&b| b == 0).unwrap_or(data.len());
        Ok(String::from_utf8_lossy(&data[..end]).into_owned())
    }

    pub fn read_binary(&mut self) -> Result<Vec<u8>, ProcessingError> {
        self.take_data()
    }

    /// EBML date: nanoseconds since 2001-01-01T00:00:00 UTC. Returned as unix seconds (f64).
    pub fn read_date(&mut self) -> Result<f64, ProcessingError> {
        let ns = self.read_int()?;
        Ok(978_307_200.0 + ns as f64 / 1_000_000_000.0)
    }

    /// Parse the header of the current `Block`/`SimpleBlock` without consuming its payload.
    pub fn read_block_header(&mut self) -> Result<MatroskaBlock, ProcessingError> {
        let block = self.src.get(8)?;
        let b = &block[..];
        if b.is_empty() {
            return Err(ProcessingError::other("truncated block"));
        }
        let tn_len = vint_length(b[0]);
        if tn_len == 0 || b.len() < tn_len + 3 {
            return Err(ProcessingError::other("truncated block header"));
        }
        let (track_number, _) = read_vint_value(&b[..tn_len]);
        let timecode = i16::from_be_bytes([b[tn_len], b[tn_len + 1]]);
        let flags = b[tn_len + 2];
        let lacing = (flags & 0x06) >> 1;
        let mut header_length = tn_len + 3;
        let mut frame_count_minus_one = 0u8;
        if lacing != 0 {
            if b.len() <= header_length {
                return Err(ProcessingError::other("truncated lace header"));
            }
            frame_count_minus_one = b[header_length];
            header_length += 1;
        }
        Ok(MatroskaBlock { track_number, timecode, flags, frame_count_minus_one, header_length })
    }
}

/// Length in bytes of a variable-length integer given its first byte (0 = invalid).
pub fn vint_length(first: u8) -> usize {
    if first == 0 {
        return 0;
    }
    first.leading_zeros() as usize + 1
}

/// Decode a vint (marker bit removed). The bool is true when all value bits are set.
pub fn read_vint_value(bytes: &[u8]) -> (u64, bool) {
    let len = bytes.len();
    let mask: u8 = if len >= 8 { 0 } else { 0xFFu8 >> len };
    let mut value: u64 = (bytes[0] & mask) as u64;
    for &b in &bytes[1..] {
        value = (value << 8) | b as u64;
    }
    let all_ones = (1u64 << (7 * len)) - 1;
    (value, value == all_ones)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vint_decoding() {
        assert_eq!(vint_length(0x80), 1);
        assert_eq!(vint_length(0x40), 2);
        assert_eq!(vint_length(0x10), 4);
        assert_eq!(vint_length(0x01), 8);
        assert_eq!(read_vint_value(&[0x81]), (1, false));
        assert_eq!(read_vint_value(&[0x40, 0x02]), (2, false));
        assert_eq!(read_vint_value(&[0xFF]), (127, true));
        assert!(read_vint_value(&[0x01, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]).1);
    }
}
