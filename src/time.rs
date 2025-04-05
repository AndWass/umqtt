use core::ops::{Sub, SubAssign, Add, AddAssign};
use std::time::Duration;

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
    type Output = Duration;

    fn sub(self, rhs: Instant) -> Self::Output {
        Duration::from_secs(self.seconds.saturating_sub(rhs.seconds))
    }
}

impl Sub<Duration> for Instant {
    type Output = Instant;
    fn sub(self, rhs: Duration) -> Self::Output {
        Instant::from_seconds_since_epoch(self.seconds.saturating_sub(rhs.as_secs()))
    }
}

impl SubAssign<Duration> for Instant {
    fn sub_assign(&mut self, rhs: Duration) {
        self.seconds = self.seconds.saturating_sub(rhs.as_secs());
    }
}

impl Add<Duration> for Instant {
    type Output = Instant;

    fn add(self, rhs: Duration) -> Self::Output {
        Instant::from_seconds_since_epoch(self.seconds.saturating_add(rhs.as_secs()))
    }
}

impl AddAssign<Duration> for Instant {
    fn add_assign(&mut self, rhs: Duration) {
        self.seconds = self.seconds.saturating_add(rhs.as_secs());
    }
}
