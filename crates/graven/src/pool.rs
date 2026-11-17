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
///
/// One pool shared by several threads serializes them on its lock; prefer [`LocalPool`] there.
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

/// Buffers kept per thread by [`LocalPool`].
#[cfg(feature = "std")]
const LOCAL_KEEP: usize = 4;
/// Chunk size of [`LocalPool`].
#[cfg(feature = "std")]
const LOCAL_CHUNK: usize = 4096;
/// A buffer handed back with more free capacity than this is dropped instead of kept, so one
/// large message does not hold its memory in the pool.
#[cfg(feature = "std")]
const LOCAL_MAX_KEPT: usize = 64 * 1024;

#[cfg(feature = "std")]
std::thread_local! {
    static LOCAL_FREE: core::cell::RefCell<alloc::vec::Vec<BytesMut>> =
        const { core::cell::RefCell::new(alloc::vec::Vec::new()) };
}

/// A pool with one free list per thread: no lock, no setup, nothing to share.
///
/// `Builder::new(&LocalPool)` works anywhere. Each thread keeps at most four 4 KiB chunks,
/// packed the same way as in [`SimplePool`]; a buffer returned on another thread (an async
/// task that moved) simply joins that thread's list. Buffers with more than 64 KiB of free
/// capacity are dropped instead of kept, and a thread's buffers are freed when it exits.
/// Requires the `std` feature (on by default).
#[cfg(feature = "std")]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LocalPool;

#[cfg(feature = "std")]
impl BufPool for LocalPool {
    #[inline]
    fn take(&self) -> BytesMut {
        // `try_with` fails only while the thread is being torn down: fall back to a fresh buffer.
        let popped = LOCAL_FREE.try_with(|f| f.try_borrow_mut().ok().and_then(|mut f| f.pop())).ok().flatten();
        match popped {
            Some(mut b) => {
                b.clear();
                if b.capacity() < LOCAL_CHUNK / 4 {
                    b.reserve(LOCAL_CHUNK);
                }
                b
            }
            None => BytesMut::with_capacity(LOCAL_CHUNK),
        }
    }
    #[inline]
    fn give(&self, b: BytesMut) {
        if b.capacity() > LOCAL_MAX_KEPT {
            return;
        }
        let _ = LOCAL_FREE.try_with(move |f| {
            if let Ok(mut f) = f.try_borrow_mut()
                && f.len() < LOCAL_KEEP
            {
                f.push(b);
            }
        });
    }
    fn chunk_size(&self) -> usize {
        LOCAL_CHUNK
    }
}

#[cfg(all(test, feature = "std"))]
mod tests {
    use super::*;

    #[test]
    fn local_pool_reuses_and_bounds() {
        let p = LocalPool;
        let a = p.take();
        assert!(a.capacity() >= LOCAL_CHUNK);
        let ptr = a.as_ptr();
        p.give(a);
        let b = p.take();
        assert_eq!(b.as_ptr(), ptr, "the same buffer comes back on the same thread");
        // At most LOCAL_KEEP buffers are kept.
        let bufs: alloc::vec::Vec<_> = (0..LOCAL_KEEP + 3).map(|_| p.take()).collect();
        for x in bufs {
            p.give(x);
        }
        assert_eq!(LOCAL_FREE.with(|f| f.borrow().len()), LOCAL_KEEP);
        // A buffer with a lot of free capacity is dropped.
        let before = LOCAL_FREE.with(|f| f.borrow().len());
        for _ in 0..before {
            drop(p.take());
        }
        p.give(BytesMut::with_capacity(LOCAL_MAX_KEPT + 1));
        assert_eq!(LOCAL_FREE.with(|f| f.borrow().len()), 0);
        drop(b);
    }

    #[test]
    fn local_pool_lists_are_per_thread() {
        let p = LocalPool;
        p.give(BytesMut::with_capacity(LOCAL_CHUNK));
        std::thread::spawn(|| assert_eq!(LOCAL_FREE.with(|f| f.borrow().len()), 0)).join().unwrap();
    }
}
