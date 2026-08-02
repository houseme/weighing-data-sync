use std::{
    future::Future,
    time::{Duration, Instant},
};

use anyhow::Error as AnyhowError;
use tokio::time::sleep;

use crate::sync::error::UploadError;

const MIN_RETRY_DELAY: Duration = Duration::from_millis(1);

pub(crate) async fn retry_transient<T, Fut, Op, Notify>(
    initial_delay: Duration,
    max_delay: Duration,
    max_elapsed: Duration,
    mut operation: Op,
    mut notify: Notify,
) -> anyhow::Result<T>
where
    Op: FnMut() -> Fut,
    Fut: Future<Output = Result<T, UploadError>>,
    Notify: FnMut(&AnyhowError, Duration),
{
    let started_at = Instant::now();
    let mut delay = initial_delay.max(MIN_RETRY_DELAY);
    let max_delay = max_delay.max(delay);

    loop {
        match operation().await {
            Ok(value) => return Ok(value),
            Err(UploadError::Permanent(error)) => return Err(error),
            Err(UploadError::Transient(error)) => {
                let elapsed = started_at.elapsed();
                if elapsed >= max_elapsed {
                    return Err(error);
                }

                let sleep_for = delay.min(max_elapsed.saturating_sub(elapsed));
                notify(&error, sleep_for);
                sleep(sleep_for).await;
                delay = delay.checked_mul(2).unwrap_or(max_delay).min(max_delay);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use anyhow::anyhow;

    use super::*;

    #[tokio::test]
    async fn retry_transient_retries_until_success() {
        let attempts = Cell::new(0);

        let result = retry_transient(
            Duration::from_millis(1),
            Duration::from_millis(2),
            Duration::from_millis(50),
            || {
                let attempt = attempts.get() + 1;
                attempts.set(attempt);
                async move {
                    if attempt < 3 {
                        Err(UploadError::Transient(anyhow!("try again")))
                    } else {
                        Ok("done")
                    }
                }
            },
            |_, _| {},
        )
        .await
        .unwrap();

        assert_eq!(result, "done");
        assert_eq!(attempts.get(), 3);
    }

    #[tokio::test]
    async fn retry_transient_stops_on_permanent_error() {
        let attempts = Cell::new(0);

        let error = retry_transient::<(), _, _, _>(
            Duration::from_millis(1),
            Duration::from_millis(2),
            Duration::from_millis(50),
            || {
                attempts.set(attempts.get() + 1);
                async { Err(UploadError::Permanent(anyhow!("bad request"))) }
            },
            |_, _| {},
        )
        .await
        .unwrap_err();

        assert_eq!(error.to_string(), "bad request");
        assert_eq!(attempts.get(), 1);
    }
}
