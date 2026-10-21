use std::time::Duration;

use serde::Serialize;

/// How long to wait before retrying a failed run.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub enum Backoff {
    /// No backoff: after an error, wait for the regular interval, or for the
    /// next update when there is no interval.
    None,
    /// `base × consecutive errors`, capped at `max`.
    Linear { base: Duration, max: Duration },
    /// `base × 2^(consecutive errors − 1)`, capped at `max`, then shortened
    /// by a random fraction of up to `jitter` (clamped to `0.0..=1.0`). With
    /// `jitter: 0.5` the wait lies in `[delay / 2, delay]`; `0.0` disables
    /// jitter and `1.0` is "full jitter", `[0, delay]`.
    Exponential {
        base: Duration,
        max: Duration,
        jitter: f64,
    },
}

impl Default for Backoff {
    fn default() -> Self {
        Backoff::Exponential {
            base: Duration::from_secs(1),
            max: Duration::from_secs(300),
            jitter: 0.5,
        }
    }
}

impl Backoff {
    /// The delay before the next attempt after `consecutive_errors` failures,
    /// before jitter. This is the upper bound of [`Backoff::jittered`].
    pub fn delay(&self, consecutive_errors: u32) -> Option<Duration> {
        let n = consecutive_errors.max(1);
        match *self {
            Backoff::None => None,
            Backoff::Linear { base, max } => Some(base.saturating_mul(n).min(max)),
            Backoff::Exponential { base, max, .. } => {
                let mut delay = base.min(max);

                if delay.is_zero() {
                    return Some(delay);
                }

                for _ in 1..n {
                    if delay >= max {
                        break;
                    }

                    delay = delay.saturating_mul(2).min(max);
                }

                Some(delay)
            }
        }
    }

    /// The delay to actually wait: [`Backoff::delay`] shortened by
    /// `jitter × r`. `r` is a uniform sample from `[0, 1)`; it is clamped to
    /// `0.0..=1.0`, and NaN counts as `0.0`. Only `Exponential` has jitter.
    pub fn jittered(&self, consecutive_errors: u32, r: f64) -> Option<Duration> {
        let delay = self.delay(consecutive_errors)?;
        let jitter = match *self {
            Backoff::Exponential { jitter, .. } => unit(jitter),
            Backoff::None | Backoff::Linear { .. } => 0.0,
        };

        Some(delay.mul_f64(1.0 - jitter * unit(r)))
    }
}

/// Clamps `x` to `0.0..=1.0`, mapping NaN to `0.0`.
fn unit(x: f64) -> f64 {
    if x.is_nan() { 0.0 } else { x.clamp(0.0, 1.0) }
}
