//! Run this file with `cargo test --test 03_newtype`.

//! TODO: Implement three newtypes (Length, Duration and Speed), which will wrap a `u32`.
//! Implement operator overloading for natural operations between these types:
//!
//! Length + Length => Length
//! Length / Duration => Speed
//! Speed * Duration => Length

#[derive(Debug, PartialEq, Eq)]
struct Length(u32);
#[derive(Debug, PartialEq, Eq)]
struct Duration(u32);
#[derive(Debug, PartialEq, Eq)]
struct Speed(u32);
impl Length {
    fn new(value: u32) -> Self {
        Self(value)
    }
}
impl Duration {
    fn new(value: u32) -> Self {
        Self(value)
    }
}
impl Speed {
    fn new(value: u32) -> Self {
        Self(value)
    }
}
impl std::ops::Add for Length {
    type Output = Length;
    fn add(self, rhs: Length) -> Length {
        Length(self.0 + rhs.0)
    }
}
impl std::ops::Div<Duration> for Length {
    type Output = Speed;
    fn div(self, rhs: Duration) -> Speed {
        Speed(self.0 / rhs.0)
    }
}
impl std::ops::Mul<Duration> for Speed {
    type Output = Length;
    fn mul(self, rhs: Duration) -> Length {
        Length(self.0 * rhs.0)
    }
}
impl std::ops::Mul<Speed> for Duration {
    type Output = Length;
    fn mul(self, rhs: Speed) -> Length {
        rhs * self
    }
}

/// Below you can find a set of unit tests.
#[cfg(test)]
mod tests {
    use crate::{Duration, Length, Speed};

    #[test]
    fn add_lengths() {
        assert_eq!(Length::new(1) + Length::new(2), Length::new(3));
    }

    #[test]
    fn divide_length_by_duration() {
        let length: Length = Length::new(500);
        let duration: Duration = Duration::new(25);
        let speed: Speed = length / duration;
        assert_eq!(speed, Speed::new(20));
    }

    #[test]
    fn multiply_speed_by_duration() {
        let speed: Speed = Speed::new(50);
        let duration: Duration = Duration::new(3);
        let length: Length = speed * duration;
        assert_eq!(length, Length::new(150));
    }

    #[test]
    fn multiply_duration_by_speed() {
        let speed: Speed = Speed::new(50);
        let duration: Duration = Duration::new(3);
        let length: Length = duration * speed;
        assert_eq!(length, Length::new(150));
    }
}
