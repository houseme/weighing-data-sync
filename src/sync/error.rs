//! Error classification for cloud-upload retries.
//!
//! Not every failure should burn the retry budget. A `4xx` response (bad request,
//! unauthorized, payload too large) means retrying the identical request will keep
//! failing, so it is classified as [`UploadError::Permanent`] and surfaced
//! immediately instead of retrying for the full elapsed budget. Network errors,
//! timeouts, `5xx` and rate-limit (`429`) responses are [`UploadError::Transient`]
//! and follow the exponential-backoff policy.

use anyhow::{Error as AnyhowError, anyhow};
use reqwest::StatusCode;

/// A cloud-upload failure annotated with whether it is worth retrying.
#[derive(Debug)]
pub enum UploadError {
    /// Retryable: network blip, timeout, `5xx`, rate limit (`429`).
    Transient(AnyhowError),
    /// Not retryable: `4xx` (except `408`/`429`) — the request itself is rejected.
    Permanent(AnyhowError),
}

impl UploadError {
    /// Build an error from a non-success HTTP status, classifying it automatically.
    pub fn from_status(status: StatusCode, body: impl Into<String>) -> Self {
        let error = anyhow!("remote upload failed with status {status}: {}", body.into());
        if Self::status_is_permanent(status) {
            Self::Permanent(error)
        } else {
            Self::Transient(error)
        }
    }

    /// `true` when `status` indicates a permanent (non-retryable) failure.
    ///
    /// Client errors are treated as permanent except `408 Request Timeout` and
    /// `429 Too Many Requests`, which are conventionally retryable. Anything that
    /// is not a client error (`5xx`, informational, redirects) is retryable.
    pub fn status_is_permanent(status: StatusCode) -> bool {
        if status.is_client_error() {
            !matches!(status.as_u16(), 408 | 429)
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permanent_client_errors() {
        assert!(UploadError::status_is_permanent(StatusCode::BAD_REQUEST));
        assert!(UploadError::status_is_permanent(StatusCode::UNAUTHORIZED));
        assert!(UploadError::status_is_permanent(StatusCode::FORBIDDEN));
        assert!(UploadError::status_is_permanent(StatusCode::NOT_FOUND));
        assert!(UploadError::status_is_permanent(
            StatusCode::PAYLOAD_TOO_LARGE
        ));
    }

    #[test]
    fn retryable_status_codes() {
        // 4xx that are conventionally retryable.
        assert!(!UploadError::status_is_permanent(
            StatusCode::REQUEST_TIMEOUT
        ));
        assert!(!UploadError::status_is_permanent(
            StatusCode::TOO_MANY_REQUESTS
        ));
        // 5xx are transient.
        assert!(!UploadError::status_is_permanent(
            StatusCode::INTERNAL_SERVER_ERROR
        ));
        assert!(!UploadError::status_is_permanent(StatusCode::BAD_GATEWAY));
        assert!(!UploadError::status_is_permanent(
            StatusCode::SERVICE_UNAVAILABLE
        ));
    }

    #[test]
    fn from_status_classifies_correctly() {
        assert!(matches!(
            UploadError::from_status(StatusCode::UNAUTHORIZED, "no token"),
            UploadError::Permanent(_)
        ));
        assert!(matches!(
            UploadError::from_status(StatusCode::SERVICE_UNAVAILABLE, "down"),
            UploadError::Transient(_)
        ));
    }
}
