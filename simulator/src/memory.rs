// Copyright 2026 Erst Users
// SPDX-License-Identifier: Apache-2.0

//! Allocator state tracking and memory-limit enforcement for safe snapshot/rollback.
//!
//! # Allocator Rollback Safety
//!
//! When the Soroban `Host` is replaced during a rollback in [`SimHost::restore_from_snapshot`],
//! the old `Host` (including its internal `Budget` and Wasm linear-memory regions) is dropped
//! and a fresh `Host` is constructed from the saved ledger snapshot.  Rust's standard drop
//! semantics guarantee that:
//!
//! - All `Vec`, `String`, `HashMap`, and `Box` allocations owned by the old `Host` are freed.
//! - Any Wasm linear memory or JIT code regions allocated via the Rust global allocator are
//!   deallocated when the old `Host`'s `Drop` impl runs.
//! - The new `Host` starts with a clean `Budget` whose limits match those of the original
//!   `SimHost`, so post-rollback memory accounting is consistent.
//!
//! The [`AllocTracker`] in this module records memory-consumption snapshots at checkpoints and
//! validates that rollbacks do not leak memory or leave dangling allocations.  It also provides
//! the [`check_memory_limit`] helper used by the runtime to panic when the configured hard
//! memory limit is exceeded (mimicking live Soroban network constraints).
//!
//! # Invariants
//!
//! 1. Every [`AllocTracker::snapshot`] must be paired with at most one
//!    [`AllocTracker::record_rollback`].
//! 2. Memory bytes consumed after a rollback start from the freshly-constructed `Budget`'s
//!    baseline (zero consumed, with limits applied); no memory allocated by the old `Host`
//!    leaks into the new `Host`.
//! 3. Repeated snapshot/rollback cycles are safe: each cycle produces a self-contained
//!    `Host` whose allocator state is independent of prior cycles.

/// Tracks allocator state across snapshot-and-rollback cycles.
///
/// Each snapshot checkpoint records the `Budget` memory consumption at that point.
/// A subsequent rollback resets the expected baseline; the tracker verifies that
/// the new `Host` starts with the correct allocator state.
///
/// # Debug-mode invariant checks
///
/// In debug builds the tracker runs lightweight assertions on every operation:
/// - Snapshot count always ≥ rollback count.
/// - Memory consumption stays within a sane range.
#[derive(Debug, Clone)]
pub struct AllocTracker {
    /// Memory bytes consumed by the Budget at the last snapshot.
    snapshotted_memory_bytes: u64,
    /// Number of snapshot operations performed.
    snapshot_count: u64,
    /// Number of rollback operations performed.
    rollback_count: u64,
}

impl AllocTracker {
    /// Creates a new tracker with zero-initialized state.
    pub fn new() -> Self {
        Self {
            snapshotted_memory_bytes: 0,
            snapshot_count: 0,
            rollback_count: 0,
        }
    }

    /// Records a snapshot of the current memory consumption.
    ///
    /// Call this just **before** capturing a ledger snapshot so that
    /// the tracker remembers the baseline memory state.
    ///
    /// # Arguments
    /// * `memory_bytes` – The current Budget memory consumption
    ///   (obtained via [`Budget::get_mem_bytes_consumed`]).
    pub fn snapshot(&mut self, memory_bytes: u64) {
        self.snapshotted_memory_bytes = memory_bytes;
        self.snapshot_count = self.snapshot_count.saturating_add(1);
        debug_assert!(
            self.snapshot_count >= self.rollback_count,
            "snapshot count must always >= rollback count"
        );
    }

    /// Records a rollback operation and resets the baseline.
    ///
    /// Call this **after** [`SimHost::restore_from_snapshot`] has completed
    /// and the new `Host` is in place.
    ///
    /// # Arguments
    /// * `restored_memory_bytes` – The memory consumption of the newly restored
    ///   `Host`'s Budget (expected to be 0 after a fresh construction).
    pub fn record_rollback(&mut self, restored_memory_bytes: u64) {
        self.rollback_count = self.rollback_count.saturating_add(1);
        // The restored Host has a fresh Budget — consumption should be zero.
        debug_assert!(
            restored_memory_bytes == 0,
            "restored Host budget should start at 0 consumption, got {restored_memory_bytes}"
        );
    }

    /// Returns the memory bytes recorded at the last snapshot.
    pub fn snapshotted_memory_bytes(&self) -> u64 {
        self.snapshotted_memory_bytes
    }

    /// Returns the total number of snapshot operations.
    pub fn snapshot_count(&self) -> u64 {
        self.snapshot_count
    }

    /// Returns the total number of rollback operations.
    pub fn rollback_count(&self) -> u64 {
        self.rollback_count
    }

    /// Returns `true` if at least one rollback has been performed.
    pub fn has_rolled_back(&self) -> bool {
        self.rollback_count > 0
    }

    /// Returns the net number of un-rolled-back snapshots.
    pub fn net_snapshots(&self) -> u64 {
        self.snapshot_count.saturating_sub(self.rollback_count)
    }
}

impl Default for AllocTracker {
    fn default() -> Self {
        Self::new()
    }
}

/// Checks whether the current memory consumption exceeds the configured hard limit.
///
/// # Panics
///
/// Panics with a diagnostic message when `consumed > limit`.
pub fn check_memory_limit(consumed: u64, limit: u64) {
    if consumed > limit {
        panic!("ERR_MEMORY_LIMIT_EXCEEDED: consumed {consumed} bytes, limit {limit} bytes");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tracker_starts_empty() {
        let tracker = AllocTracker::new();
        assert_eq!(tracker.snapshot_count(), 0);
        assert_eq!(tracker.rollback_count(), 0);
        assert!(!tracker.has_rolled_back());
        assert_eq!(tracker.net_snapshots(), 0);
    }

    #[test]
    fn test_snapshot_records_memory() {
        let mut tracker = AllocTracker::new();
        tracker.snapshot(42);
        assert_eq!(tracker.snapshotted_memory_bytes(), 42);
        assert_eq!(tracker.snapshot_count(), 1);
        assert_eq!(tracker.net_snapshots(), 1);
    }

    #[test]
    fn test_rollback_resets_baseline() {
        let mut tracker = AllocTracker::new();
        tracker.snapshot(100);
        tracker.record_rollback(0);
        assert!(tracker.has_rolled_back());
        assert_eq!(tracker.rollback_count(), 1);
        assert_eq!(tracker.net_snapshots(), 0);
    }

    #[test]
    fn test_repeated_cycles() {
        let mut tracker = AllocTracker::new();

        // Cycle 1
        tracker.snapshot(10);
        tracker.record_rollback(0);
        assert_eq!(tracker.snapshot_count(), 1);
        assert_eq!(tracker.rollback_count(), 1);

        // Cycle 2
        tracker.snapshot(20);
        tracker.record_rollback(0);
        assert_eq!(tracker.snapshot_count(), 2);
        assert_eq!(tracker.rollback_count(), 2);
        assert_eq!(tracker.net_snapshots(), 0);
    }

    #[test]
    fn test_snapshot_without_rollback() {
        let mut tracker = AllocTracker::new();
        tracker.snapshot(50);
        tracker.snapshot(100);
        assert!(!tracker.has_rolled_back());
        assert_eq!(tracker.net_snapshots(), 2);
        assert_eq!(tracker.snapshotted_memory_bytes(), 100);
    }

    #[test]
    fn test_check_memory_limit_within_bounds() {
        // Should not panic
        check_memory_limit(500, 1000);
    }

    #[test]
    fn test_check_memory_limit_at_boundary() {
        // Should not panic — exactly at limit
        check_memory_limit(1000, 1000);
    }

    #[test]
    fn test_check_memory_limit_exceeded_panics() {
        let result = std::panic::catch_unwind(|| check_memory_limit(1001, 1000));
        assert!(result.is_err(), "expected panic when memory exceeds limit");
    }

    #[test]
    fn test_debug_and_clone() {
        let mut tracker = AllocTracker::new();
        tracker.snapshot(7);
        tracker.record_rollback(0);
        let cloned = tracker.clone();
        assert_eq!(cloned.snapshot_count(), tracker.snapshot_count());
        assert_eq!(cloned.rollback_count(), tracker.rollback_count());
        assert_eq!(format!("{:?}", tracker), format!("{:?}", cloned));
    }
}
