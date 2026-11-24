//! CPU parallelism controls for batch export.
//!
//! The exporter always uses a bounded number of photos in flight so peak memory
//! stays predictable, while every photo uses the configured Rayon thread count
//! for its internal row-parallel work.

/// Upper bound for the "parallel photos" control. Each full-resolution RAW
/// decode can hold a large f32 buffer, so the UI intentionally stops at a
/// small number even on many-core machines.
pub const MAX_CONCURRENT_PHOTOS: usize = 8;

/// User-facing export performance settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PerformanceSettings {
    /// Use more than one CPU thread. When false, exports run one thread and
    /// one photo at a time.
    pub multithreaded: bool,
    /// Rayon worker count; `0` means all logical cores.
    pub threads: usize,
    /// Number of photos exported at the same time; `0` means the built-in
    /// memory-safe default (2 full-size or 4 resized photos).
    pub concurrent_photos: usize,
}

impl Default for PerformanceSettings {
    fn default() -> Self {
        Self {
            multithreaded: true,
            threads: 0,
            concurrent_photos: 0,
        }
    }
}

impl PerformanceSettings {
    /// Logical cores reported by the operating system (at least 1).
    pub fn max_threads() -> usize {
        std::thread::available_parallelism()
            .map(|count| count.get())
            .unwrap_or(1)
            .max(1)
    }

    /// Maximum value accepted by the "parallel photos" slider.
    pub fn max_concurrent_photos(&self) -> usize {
        if !self.multithreaded {
            return 1;
        }
        self.resolved_threads().clamp(1, MAX_CONCURRENT_PHOTOS)
    }

    /// Effective Rayon thread count.
    pub fn resolved_threads(&self) -> usize {
        if !self.multithreaded {
            return 1;
        }
        let max = Self::max_threads();
        if self.threads == 0 {
            max
        } else {
            self.threads.clamp(1, max)
        }
    }

    /// Effective number of photos exported concurrently.
    ///
    /// Full-resolution jobs are capped lower than resized jobs because each
    /// decode holds a full linear frame in memory.
    pub fn resolved_concurrency(&self, full_resolution: bool) -> usize {
        if !self.multithreaded {
            return 1;
        }
        let requested = if self.concurrent_photos == 0 {
            if full_resolution { 2 } else { 4 }
        } else {
            self.concurrent_photos
        };
        requested.clamp(1, self.resolved_threads().min(MAX_CONCURRENT_PHOTOS))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_uses_all_cores_with_memory_safe_concurrency() {
        let settings = PerformanceSettings::default();
        assert!(settings.multithreaded);
        assert_eq!(settings.threads, 0);
        assert_eq!(settings.concurrent_photos, 0);
        assert_eq!(
            settings.resolved_threads(),
            PerformanceSettings::max_threads()
        );
        assert_eq!(
            settings.resolved_concurrency(true),
            2.min(PerformanceSettings::max_threads())
        );
        assert_eq!(
            settings.resolved_concurrency(false),
            4.min(PerformanceSettings::max_threads())
        );
    }

    #[test]
    fn disabling_multithreading_forces_one_thread_and_one_photo() {
        let settings = PerformanceSettings {
            multithreaded: false,
            threads: 0,
            concurrent_photos: 0,
        };
        assert_eq!(settings.resolved_threads(), 1);
        assert_eq!(settings.resolved_concurrency(true), 1);
        assert_eq!(settings.resolved_concurrency(false), 1);
        assert_eq!(settings.max_concurrent_photos(), 1);
    }

    #[test]
    fn manual_threads_are_clamped_to_available_cores() {
        let max = PerformanceSettings::max_threads();
        let over = PerformanceSettings {
            threads: max + 100,
            ..Default::default()
        };
        assert_eq!(over.resolved_threads(), max);

        let under = PerformanceSettings {
            threads: 2,
            ..Default::default()
        };
        assert_eq!(under.resolved_threads(), 2.min(max));
    }

    #[test]
    fn concurrency_is_clamped_to_threads_and_hard_limit() {
        let settings = PerformanceSettings {
            multithreaded: true,
            threads: 2,
            concurrent_photos: 100,
        };
        assert_eq!(
            settings.resolved_concurrency(true),
            2.min(MAX_CONCURRENT_PHOTOS)
        );

        let capped = PerformanceSettings {
            threads: MAX_CONCURRENT_PHOTOS * 2,
            concurrent_photos: MAX_CONCURRENT_PHOTOS * 2,
            ..Default::default()
        };
        assert_eq!(
            capped.resolved_concurrency(true),
            MAX_CONCURRENT_PHOTOS
                .min(PerformanceSettings::max_threads())
                .max(1)
        );
    }

    #[test]
    fn explicit_single_thread_keeps_concurrency_at_one() {
        let settings = PerformanceSettings {
            threads: 1,
            concurrent_photos: 4,
            ..Default::default()
        };
        assert_eq!(settings.resolved_threads(), 1);
        assert_eq!(settings.resolved_concurrency(true), 1);
        assert_eq!(settings.resolved_concurrency(false), 1);
    }
}
