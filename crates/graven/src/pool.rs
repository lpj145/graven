//! Buffer pools.

use bytes::BytesMut;

/// Source of reusable buffers. `take` returns an empty buffer; `give` returns it.
pub trait BufPool {
    fn take(&self) -> BytesMut;
    fn give(&self, b: BytesMut);
    /// Preferred buffer size; `Builder::flush` reserves this much when a
    /// buffer runs low.
    fn chunk_size(&self) -> usize {
        4096
    }
}

/// Mutex-protected free list of chunk buffers.
///
/// Finished messages are `split()` off the front of a chunk, so consecutive
/// messages pack into the same allocation (bump-arena style). `take` only
/// asks for a fresh chunk when less than a quarter of one is left; that
/// `reserve` reuses the allocation for free once every message cut from it
/// has been dropped, so steady state is zero allocations, and with messages
/// in flight it costs about one allocation per chunk instead of per message.
/// Memory pinned by in-flight messages is roughly their own size plus one chunk.
#[cfg(feature = "std")]
pub struct SimplePool {
    free: std::sync::Mutex<alloc::vec::Vec<BytesMut>>,
    capacity: usize,
}

#[cfg(feature = "std")]
impl SimplePool {
    pub fn new(initial_capacity: usize) -> Self {
        SimplePool { free: std::sync::Mutex::new(alloc::vec::Vec::with_capacity(16)), capacity: initial_capacity }
    }
}

#[cfg(feature = "std")]
impl BufPool for SimplePool {
    fn take(&self) -> BytesMut {
        let popped = self.free.lock().map(|mut f| f.pop()).unwrap_or(None);
        match popped {
            Some(mut b) => {
                b.clear();
                if b.capacity() < self.capacity / 4 {
                    b.reserve(self.capacity);
                }
                b
            }
            None => BytesMut::with_capacity(self.capacity),
        }
    }
    fn chunk_size(&self) -> usize {
        self.capacity
    }
    fn give(&self, b: BytesMut) {
        if let Ok(mut f) = self.free.lock()
            && f.len() < f.capacity() {
                f.push(b);
            }
    }
}
