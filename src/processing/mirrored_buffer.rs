//! A buffer whose address space is mapped twice back-to-back, so that a read or write that runs
//! past the end transparently continues at the beginning. On platforms without the required
//! memory mapping primitives a plain buffer with copy-on-wrap semantics is used instead.

use std::borrow::Cow;
use std::io;
use std::sync::{Arc, Mutex};

pub struct MirroredBuffer {
    inner: Backing,
    len: usize,
}

enum Backing {
    #[cfg(unix)]
    Mapped { base: *mut u8 },
    Plain { data: Box<[std::cell::UnsafeCell<u8>]> },
}

// SAFETY: Access is coordinated by the block stream protocol: the producer only writes to the
// region between the producer position and the slowest consumer, consumers only read the region
// between their own position and the producer position. The regions never overlap.
unsafe impl Send for MirroredBuffer {}
unsafe impl Sync for MirroredBuffer {}

fn page_size() -> usize {
    #[cfg(unix)]
    {
        let ps = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
        if ps > 0 {
            return ps as usize;
        }
    }
    4096
}

impl MirroredBuffer {
    /// Create a buffer of at least `min_length` bytes (rounded up to a page multiple).
    pub fn new(min_length: usize) -> io::Result<Self> {
        let ps = page_size();
        let len = min_length.max(ps).div_ceil(ps) * ps;
        #[cfg(unix)]
        {
            match Self::new_mapped(len) {
                Ok(b) => return Ok(b),
                Err(_) => { /* fall through to polyfill */ }
            }
        }
        Ok(Self::new_plain(len))
    }

    /// Force the copy-on-wrap implementation (useful for tests).
    pub fn new_plain(len: usize) -> Self {
        let data: Vec<std::cell::UnsafeCell<u8>> = (0..len).map(|_| std::cell::UnsafeCell::new(0)).collect();
        Self { inner: Backing::Plain { data: data.into_boxed_slice() }, len }
    }

    #[cfg(unix)]
    fn new_mapped(len: usize) -> io::Result<Self> {
        use std::ptr;
        unsafe {
            let fd = Self::create_shared_fd(len)?;
            let base = libc::mmap(
                ptr::null_mut(),
                len * 2,
                libc::PROT_NONE,
                libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
                -1,
                0,
            );
            if base == libc::MAP_FAILED {
                libc::close(fd);
                return Err(io::Error::last_os_error());
            }
            let base = base as *mut u8;
            for i in 0..2 {
                let addr = base.add(i * len);
                let mapped = libc::mmap(
                    addr as *mut libc::c_void,
                    len,
                    libc::PROT_READ | libc::PROT_WRITE,
                    libc::MAP_SHARED | libc::MAP_FIXED,
                    fd,
                    0,
                );
                if mapped == libc::MAP_FAILED {
                    let err = io::Error::last_os_error();
                    libc::munmap(base as *mut libc::c_void, len * 2);
                    libc::close(fd);
                    return Err(err);
                }
            }
            libc::close(fd);
            Ok(Self { inner: Backing::Mapped { base }, len })
        }
    }

    #[cfg(target_os = "linux")]
    unsafe fn create_shared_fd(len: usize) -> io::Result<libc::c_int> {
        let name = b"avdumpr-mirror\0";
        let fd = libc::memfd_create(name.as_ptr() as *const libc::c_char, libc::MFD_CLOEXEC);
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        if libc::ftruncate(fd, len as libc::off_t) != 0 {
            let err = io::Error::last_os_error();
            libc::close(fd);
            return Err(err);
        }
        Ok(fd)
    }

    #[cfg(all(unix, not(target_os = "linux")))]
    unsafe fn create_shared_fd(len: usize) -> io::Result<libc::c_int> {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let name = format!(
            "/avd3-{}-{}\0",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        );
        let fd = libc::shm_open(
            name.as_ptr() as *const libc::c_char,
            libc::O_RDWR | libc::O_CREAT | libc::O_EXCL,
            0o600,
        );
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        libc::shm_unlink(name.as_ptr() as *const libc::c_char);
        if libc::ftruncate(fd, len as libc::off_t) != 0 {
            let err = io::Error::last_os_error();
            libc::close(fd);
            return Err(err);
        }
        Ok(fd)
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn is_mirrored(&self) -> bool {
        match self.inner {
            #[cfg(unix)]
            Backing::Mapped { .. } => true,
            Backing::Plain { .. } => false,
        }
    }

    /// Read-only view of `length` bytes starting at `offset` (which may wrap around the end).
    ///
    /// # Safety
    /// The caller must guarantee that no concurrent write touches the requested region.
    pub unsafe fn slice(&self, offset: usize, length: usize) -> Cow<'_, [u8]> {
        debug_assert!(offset < self.len || self.len == 0);
        debug_assert!(length <= self.len);
        match &self.inner {
            #[cfg(unix)]
            Backing::Mapped { base } => Cow::Borrowed(std::slice::from_raw_parts(base.add(offset), length)),
            Backing::Plain { data } => {
                let first = self.len - offset;
                if length <= first {
                    Cow::Borrowed(std::slice::from_raw_parts(data[offset].get(), length))
                } else {
                    let mut v = Vec::with_capacity(length);
                    v.extend_from_slice(std::slice::from_raw_parts(data[offset].get(), first));
                    v.extend_from_slice(std::slice::from_raw_parts(data[0].get(), length - first));
                    Cow::Owned(v)
                }
            }
        }
    }

    /// Mutable view for the producer. For the non-mirrored backing the returned slice is
    /// truncated at the physical end of the buffer (the caller must tolerate short writes).
    ///
    /// # Safety
    /// The caller must guarantee that no concurrent access touches the requested region.
    #[allow(clippy::mut_from_ref)]
    pub unsafe fn slice_mut(&self, offset: usize, length: usize) -> &mut [u8] {
        match &self.inner {
            #[cfg(unix)]
            Backing::Mapped { base } => std::slice::from_raw_parts_mut(base.add(offset), length),
            Backing::Plain { data } => {
                let avail = (self.len - offset).min(length);
                std::slice::from_raw_parts_mut(data[offset].get(), avail)
            }
        }
    }
}

impl Drop for MirroredBuffer {
    fn drop(&mut self) {
        #[cfg(unix)]
        if let Backing::Mapped { base } = self.inner {
            unsafe {
                libc::munmap(base as *mut libc::c_void, self.len * 2);
            }
        }
    }
}

/// Pool of reusable mirrored buffers of one size.
pub struct MirroredBufferPool {
    buffer_size: usize,
    slots: Mutex<Vec<Arc<MirroredBuffer>>>,
}

impl MirroredBufferPool {
    pub fn new(buffer_size: usize) -> Self {
        Self { buffer_size, slots: Mutex::new(Vec::new()) }
    }

    pub fn buffer_size(&self) -> usize {
        self.buffer_size
    }

    pub fn take(&self) -> io::Result<Arc<MirroredBuffer>> {
        let mut slots = self.slots.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(b) = slots.pop() {
            return Ok(b);
        }
        drop(slots);
        Ok(Arc::new(MirroredBuffer::new(self.buffer_size)?))
    }

    pub fn release(&self, buffer: Arc<MirroredBuffer>) {
        self.slots.lock().unwrap_or_else(|e| e.into_inner()).push(buffer);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_around() {
        let b = MirroredBuffer::new(4096).unwrap();
        let len = b.len();
        unsafe {
            let w = b.slice_mut(len - 4, 8);
            if b.is_mirrored() {
                w.copy_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
                let r = b.slice(len - 4, 8);
                assert_eq!(&*r, &[1, 2, 3, 4, 5, 6, 7, 8]);
                assert_eq!(&*b.slice(0, 4), &[5, 6, 7, 8]);
            } else {
                assert_eq!(w.len(), 4);
            }
        }
    }

    #[test]
    fn plain_backing_copies_on_wrap() {
        let b = MirroredBuffer::new_plain(16);
        unsafe {
            b.slice_mut(12, 4).copy_from_slice(&[1, 2, 3, 4]);
            b.slice_mut(0, 4).copy_from_slice(&[5, 6, 7, 8]);
            assert_eq!(&*b.slice(12, 8), &[1, 2, 3, 4, 5, 6, 7, 8]);
        }
    }
}
