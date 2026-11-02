//! Reconnect delays: exponential from 0.5 s to 30 s, with jitter.

use std::time::Duration;

#[derive(Debug, Clone)]
pub struct Backoff {
    attempt: u32,
    min: Duration,
    max: Duration,
}

impl Default for Backoff {
    fn default() -> Self {
        Backoff::new(Duration::from_millis(500), Duration::from_secs(30))
    }
}

impl Backoff {
    pub fn new(min: Duration, max: Duration) -> Self {
        Backoff {
            attempt: 0,
            min,
            max,
        }
    }

    /// The delay before the next attempt. Each call doubles the base delay up to `max`;
    /// the result is the base times a random factor in [0.5, 1.5), kept in [min, max].
    pub fn next_delay(&mut self) -> Duration {
        let base = self.base();
        self.attempt = self.attempt.saturating_add(1);
        let factor = 0.5 + fastrand::f64();
        base.mul_f64(factor).clamp(self.min, self.max)
    }

    /// The base delay of the next attempt, without jitter.
    pub fn base(&self) -> Duration {
        let exp = self.attempt.min(20);
        self.min.saturating_mul(1u32 << exp).min(self.max)
    }

    pub fn attempt(&self) -> u32 {
        self.attempt
    }

    pub fn reset(&mut self) {
        self.attempt = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grows_to_cap_with_jitter_and_resets() {
        let mut b = Backoff::default();
        let mut bases = Vec::new();
        for _ in 0..12 {
            let base = b.base();
            let d = b.next_delay();
            assert!(
                d >= Duration::from_millis(500) && d <= Duration::from_secs(30),
                "{d:?}"
            );
            assert!(
                d >= base
                    .mul_f64(0.5)
                    .max(Duration::from_millis(500))
                    .min(Duration::from_secs(30))
            );
            assert!(d <= base.mul_f64(1.5).min(Duration::from_secs(30)));
            bases.push(base.as_millis());
        }
        println!("backoff bases ms: {bases:?}");
        assert_eq!(&bases[..7], &[500, 1000, 2000, 4000, 8000, 16000, 30000]);
        assert!(bases[7..].iter().all(|&b| b == 30000));
        b.reset();
        assert_eq!(b.base(), Duration::from_millis(500));
        // Jitter spreads the delays.
        let mut seen = std::collections::BTreeSet::new();
        for _ in 0..20 {
            let mut b = Backoff::default();
            b.next_delay();
            b.next_delay();
            seen.insert(b.next_delay().as_millis());
        }
        assert!(seen.len() > 5);
    }
}
