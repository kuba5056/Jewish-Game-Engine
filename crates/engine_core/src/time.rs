use std::time::Duration;

/// Frame clock with a fixed-step accumulator.
///
/// Rendering and `update` run once per frame with a variable `delta`;
/// simulation (`fixed_update`) runs zero or more times per frame with a
/// constant `fixed_delta`, so physics behaves the same at any frame rate.
#[derive(Debug, Clone)]
pub struct Time {
    delta: Duration,
    elapsed: Duration,
    fixed_delta: Duration,
    accumulator: Duration,
    max_delta: Duration,
    frame: u64,
}

impl Time {
    /// `fixed_hz` is the simulation rate in steps per second.
    pub fn new(fixed_hz: u32) -> Self {
        assert!(fixed_hz > 0, "fixed_hz must be positive");
        Self {
            delta: Duration::ZERO,
            elapsed: Duration::ZERO,
            fixed_delta: Duration::from_secs(1) / fixed_hz,
            accumulator: Duration::ZERO,
            // A long stall (debugger, window drag) must not trigger a burst of
            // catch-up steps, so a single frame never counts for more than this.
            max_delta: Duration::from_millis(250),
            frame: 0,
        }
    }

    /// Advances the clock by one frame of real time.
    pub fn advance(&mut self, real_delta: Duration) {
        self.delta = real_delta.min(self.max_delta);
        self.elapsed += self.delta;
        self.accumulator += self.delta;
        self.frame += 1;
    }

    /// Consumes one fixed step if enough time has accumulated.
    /// Call in a loop: `while time.step_fixed() { simulate(); }`.
    pub fn step_fixed(&mut self) -> bool {
        if self.accumulator >= self.fixed_delta {
            self.accumulator -= self.fixed_delta;
            true
        } else {
            false
        }
    }

    /// Time since the previous frame.
    pub fn delta(&self) -> Duration {
        self.delta
    }

    /// Time since the previous frame, in seconds.
    pub fn delta_secs(&self) -> f32 {
        self.delta.as_secs_f32()
    }

    /// Total time the clock has run.
    pub fn elapsed(&self) -> Duration {
        self.elapsed
    }

    /// Length of one simulation step.
    pub fn fixed_delta(&self) -> Duration {
        self.fixed_delta
    }

    /// Length of one simulation step, in seconds.
    pub fn fixed_delta_secs(&self) -> f32 {
        self.fixed_delta.as_secs_f32()
    }

    /// How far the current frame is between two simulation steps, in `0..1`.
    /// Use it to interpolate rendered positions.
    pub fn alpha(&self) -> f32 {
        self.accumulator.as_secs_f32() / self.fixed_delta.as_secs_f32()
    }

    /// Number of frames advanced so far.
    pub fn frame(&self) -> u64 {
        self.frame
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn steps(time: &mut Time) -> u32 {
        let mut n = 0;
        while time.step_fixed() {
            n += 1;
        }
        n
    }

    #[test]
    fn fixed_steps_follow_real_time() {
        let mut time = Time::new(50); // 20 ms steps
        time.advance(Duration::from_millis(50));
        assert_eq!(steps(&mut time), 2);
        assert!((time.alpha() - 0.5).abs() < 1e-4);

        // The 10 ms remainder carries over to the next frame.
        time.advance(Duration::from_millis(10));
        assert_eq!(steps(&mut time), 1);
    }

    #[test]
    fn short_frames_accumulate() {
        let mut time = Time::new(50);
        let mut total = 0;
        for _ in 0..10 {
            time.advance(Duration::from_millis(5));
            total += steps(&mut time);
        }
        assert_eq!(total, 2);
        assert_eq!(time.frame(), 10);
        assert_eq!(time.elapsed(), Duration::from_millis(50));
    }

    #[test]
    fn long_stall_is_clamped() {
        let mut time = Time::new(50);
        time.advance(Duration::from_secs(10));
        assert_eq!(time.delta(), Duration::from_millis(250));
        assert_eq!(steps(&mut time), 12);
    }
}
