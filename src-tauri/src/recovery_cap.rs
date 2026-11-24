//! How often the page is brought back after its renderer died: by rebuilding
//! the window on Windows (`webview_recovery`), by recreating the activity on
//! Android (`android::renderer`).
//!
//! Capped, because a page that crashes while it is drawn would otherwise come
//! back and crash forever. After three in five minutes it is left as it is.
#![cfg_attr(not(any(windows, target_os = "android")), allow(dead_code))]

use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// How many times the page is brought back within [`RECOVERY_SPAN`].
pub(crate) const MAX_RECOVERIES: usize = 3;
pub(crate) const RECOVERY_SPAN: Duration = Duration::from_secs(5 * 60);

/// When the page was last brought back, for the cap.
pub(crate) struct RecoveryCap(Mutex<VecDeque<Instant>>);

impl Default for RecoveryCap {
    fn default() -> Self {
        Self::new()
    }
}

impl RecoveryCap {
    /// `const`, for Android's, which is a static: the JNI call that asks it
    /// has nowhere else to keep it.
    pub(crate) const fn new() -> Self {
        Self(Mutex::new(VecDeque::new()))
    }

    /// Whether one more recovery is within the cap; counts it if it is.
    pub(crate) fn allow(&self, now: Instant) -> bool {
        let mut recent = self
            .0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        while recent
            .front()
            .is_some_and(|at| now.duration_since(*at) > RECOVERY_SPAN)
        {
            recent.pop_front();
        }
        if recent.len() >= MAX_RECOVERIES {
            return false;
        }
        recent.push_back(now);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brings_the_page_back_a_few_times_then_leaves_it() {
        let cap = RecoveryCap::default();
        let start = Instant::now();
        for _ in 0..MAX_RECOVERIES {
            assert!(cap.allow(start));
        }
        assert!(!cap.allow(start + Duration::from_secs(1)));
    }

    #[test]
    fn a_crash_long_after_the_last_ones_is_recovered_again() {
        let cap = RecoveryCap::default();
        let start = Instant::now();
        for _ in 0..MAX_RECOVERIES {
            assert!(cap.allow(start));
        }
        assert!(cap.allow(start + RECOVERY_SPAN + Duration::from_secs(1)));
    }
}
