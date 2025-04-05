use std::ops::Sub;

#[derive(Debug, Eq, PartialEq, Ord, PartialOrd, Clone, Copy)]
pub struct Instant {
    seconds: u64,
}

impl Instant {
    pub const fn from_seconds_since_epoch(seconds: u64) -> Self {
        Self {
            seconds,
        }
    }

    pub const fn add_seconds(&self, seconds: u64) -> Self {
        Self {
            seconds: self.seconds + seconds,
        }
    }
}

impl Sub<Instant> for Instant {
    type Output = core::time::Duration;

    fn sub(self, rhs: Instant) -> Self::Output {
        core::time::Duration::from_secs(self.seconds.saturating_sub(rhs.seconds))
    }
}
