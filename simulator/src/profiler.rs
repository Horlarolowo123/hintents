// Copyright 2026 Erst Users
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::Path;

use prost::Message as _;

/// Wraps `pprof::ProfilerGuard` to produce pprof-compatible CPU profiles.
pub struct PprofGuard<'a> {
    inner: Option<pprof::ProfilerGuard<'a>>,
}

impl<'a> PprofGuard<'a> {
    /// Start profiling at the given sampling frequency (Hz).
    pub fn start(frequency: i32) -> Self {
        let guard = pprof::ProfilerGuardBuilder::default()
            .frequency(frequency)
            .blocklist(&["libc", "libgcc", "pthread", "vdso"])
            .build()
            .ok();
        Self { inner: guard }
    }

    /// Stop profiling and return the raw pprof protobuf bytes.
    pub fn stop(&mut self) -> Option<Vec<u8>> {
        let guard = self.inner.take()?;
        let report = guard.report().build().ok()?;
        let profile = report.pprof().ok()?;
        let mut buf = Vec::new();
        profile.encode(&mut buf).ok()?;
        Some(buf)
    }

    /// Write raw pprof protobuf bytes to a file.
    pub fn write_file(bytes: &[u8], path: &Path) -> Result<(), String> {
        fs::write(path, bytes)
            .map_err(|e| format!("Failed to write pprof file {}: {}", path.display(), e))
    }
}

impl<'a> Drop for PprofGuard<'a> {
    fn drop(&mut self) {
        // Ensure the guard is consumed even if stop() was never called.
        if self.inner.is_some() {
            self.stop();
        }
    }
}
