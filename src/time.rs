use core::ops::{Add, AddAssign, Sub, SubAssign};
use core::time::Duration;

#[derive(Debug, Eq, PartialEq, Ord, PartialOrd, Clone, Copy)]
pub struct Instant {
    milliseconds: u64,
}

impl Instant {
    pub const fn from_duration_since_epoch(duration: Duration) -> Self {
        Self::from_millis_since_epoch(duration.as_millis() as u64)
    }
    pub const fn from_millis_since_epoch(milliseconds: u64) -> Self {
        Self { milliseconds }
    }
    pub const fn from_seconds_since_epoch(seconds: u64) -> Self {
        Self {
            milliseconds: seconds * 1000,
        }
    }

    pub const fn add_seconds(&self, seconds: u64) -> Self {
        Self {
            milliseconds: self.milliseconds + 1000 * seconds,
        }
    }
}

impl Sub<Instant> for Instant {
    type Output = Duration;

    fn sub(self, rhs: Instant) -> Self::Output {
        Duration::from_millis(self.milliseconds.saturating_sub(rhs.milliseconds))
    }
}

impl Sub<Duration> for Instant {
    type Output = Instant;
    fn sub(self, rhs: Duration) -> Self::Output {
        Instant::from_millis_since_epoch(self.milliseconds.saturating_sub(rhs.as_millis() as u64))
    }
}

impl SubAssign<Duration> for Instant {
    fn sub_assign(&mut self, rhs: Duration) {
        self.milliseconds = self.milliseconds.saturating_sub(rhs.as_millis() as u64);
    }
}

impl Add<Duration> for Instant {
    type Output = Instant;

    fn add(self, rhs: Duration) -> Self::Output {
        Instant::from_millis_since_epoch(self.milliseconds.saturating_add(rhs.as_millis() as u64))
    }
}

impl AddAssign<Duration> for Instant {
    fn add_assign(&mut self, rhs: Duration) {
        self.milliseconds = self.milliseconds.saturating_add(rhs.as_millis() as u64);
    }
}
