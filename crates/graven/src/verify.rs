//! Whole-message verification limits.

use crate::Error;

/// Guards a recursive walk against cycles (backward pointers) and against
/// amplification via shared subtrees.
#[derive(Debug, Clone)]
pub struct VerifyCtx {
    depth: u32,
    max_depth: u32,
    budget: usize,
}

impl VerifyCtx {
    pub const DEFAULT_MAX_DEPTH: u32 = 64;

    /// Defaults: depth 64, budget `max(buf_len * 8, 1 MiB)` bytes.
    pub fn new(buf_len: usize) -> Self {
        Self::with_limits(Self::DEFAULT_MAX_DEPTH, buf_len.saturating_mul(8).max(1 << 20))
    }

    pub fn with_limits(max_depth: u32, budget: usize) -> Self {
        VerifyCtx { depth: 0, max_depth, budget }
    }

    pub fn max_depth(&self) -> u32 {
        self.max_depth
    }

    /// Remaining budget in bytes.
    pub fn budget(&self) -> usize {
        self.budget
    }

    #[inline]
    pub fn enter(&mut self) -> Result<(), Error> {
        if self.depth >= self.max_depth {
            return Err(Error::DepthExceeded);
        }
        self.depth += 1;
        Ok(())
    }

    #[inline]
    pub fn leave(&mut self) {
        self.depth = self.depth.saturating_sub(1);
    }

    #[inline]
    pub fn charge(&mut self, bytes: usize) -> Result<(), Error> {
        self.budget = self.budget.checked_sub(bytes).ok_or(Error::BudgetExceeded)?;
        Ok(())
    }
}
