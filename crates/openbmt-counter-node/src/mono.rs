//! The monotonic clock the node waits by. With the `network` feature it is
//! the standard library's `Instant`. Without it (the build for a browser,
//! where `Instant::now` is not there) it is a clock that never moves, so a
//! wait of any length is over at once and the node only takes what is
//! already queued.

#[cfg(feature = "network")]
pub use std::time::Instant;

#[cfg(not(feature = "network"))]
pub use frozen::Instant;

#[cfg(not(feature = "network"))]
mod frozen {
    use std::ops::Add;
    use std::time::Duration;

    /// An instant that is always the same one.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    pub struct Instant;

    impl Instant {
        /// The one instant.
        pub fn now() -> Self {
            Self
        }

        /// No time has passed since any instant.
        pub fn saturating_duration_since(&self, _earlier: Self) -> Duration {
            Duration::ZERO
        }
    }

    impl Add<Duration> for Instant {
        type Output = Self;

        fn add(self, _by: Duration) -> Self {
            self
        }
    }
}
