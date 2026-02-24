//! Generalized retry with exponential backoff.

use std::future::Future;
use std::time::Duration;

use tokio::time::sleep;
use tracing::warn;

/// Retry configuration.
#[derive(Clone, Debug)]
pub struct RetryConfig {
    pub max_attempts: u32,
    pub initial_delay_ms: u64,
    pub max_delay_ms: u64,
}

/// Runs `op` until it succeeds or
/// - `is_retryable` returns false or
/// - `max_attempts` is reached
pub async fn retry<F, Fut, T, E>(
    mut op: F,
    is_retryable: impl Fn(&E) -> bool,
    config: RetryConfig,
) -> Result<T, E>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, E>>,
    E: std::fmt::Display,
{
    let mut delay = config.initial_delay_ms;
    for attempt in 0..config.max_attempts {
        match op().await {
            Ok(v) => return Ok(v),
            Err(e) => {
                if !is_retryable(&e) {
                    return Err(e);
                }
                if attempt + 1 >= config.max_attempts {
                    return Err(e);
                }
                delay = if delay >= config.max_delay_ms {
                    delay.min(config.max_delay_ms)
                } else {
                    config.initial_delay_ms * 2u64.pow(attempt)
                };
                warn!(attempt, %delay, error = %e, "Retrying");
                sleep(Duration::from_millis(delay)).await;
            }
        }
    }
    unreachable!("retry loop always returns")
}
