//! Limits the requests sent at the same time to one tracker.
//!
//! Instances are started and updated concurrently, so that a tracker that does not answer
//! (each request then waits for the timeout) never holds up the others. Without a limit, the
//! torrents of one tracker would all hit it at once (startup, retries): requests to one
//! tracker go through this gate, a few at a time, while the other trackers are not affected.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use crate::grid::primary_tracker_host;

/// Requests sent at the same time to one tracker.
pub const MAX_CONCURRENT_PER_TRACKER: usize = 4;

fn slots() -> &'static Mutex<HashMap<String, Arc<Semaphore>>> {
    static SLOTS: OnceLock<Mutex<HashMap<String, Arc<Semaphore>>>> = OnceLock::new();
    SLOTS.get_or_init(Mutex::default)
}

/// Key of the tracker of this URL (its host, or the URL itself when it has none).
fn tracker_key(tracker_url: &str) -> String {
    primary_tracker_host(tracker_url).unwrap_or_else(|| tracker_url.trim().to_string())
}

/// Waits for a free slot for this tracker. The slot is released when the permit is dropped.
pub async fn acquire(tracker_url: &str) -> Option<OwnedSemaphorePermit> {
    let semaphore = {
        let mut slots = slots().lock().unwrap_or_else(PoisonError::into_inner);
        Arc::clone(
            slots
                .entry(tracker_key(tracker_url))
                .or_insert_with(|| Arc::new(Semaphore::new(MAX_CONCURRENT_PER_TRACKER))),
        )
    };
    semaphore.acquire_owned().await.ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_of_one_tracker_share_a_key() {
        assert_eq!(
            tracker_key("https://tracker.example.org/announce?passkey=a"),
            tracker_key("https://tracker.example.org:443/announce?passkey=b")
        );
        assert_ne!(
            tracker_key("https://tracker.example.org/announce"),
            tracker_key("https://other.example.org/announce")
        );
    }

    #[tokio::test]
    async fn one_tracker_gets_a_limited_number_of_slots() {
        let url = "https://gate-test.example.org/announce";
        let mut permits = Vec::new();
        for _ in 0..MAX_CONCURRENT_PER_TRACKER {
            permits.push(acquire(url).await);
        }
        assert!(permits.iter().all(Option::is_some));

        // Full for this tracker...
        let blocked = tokio::time::timeout(std::time::Duration::from_millis(50), acquire(url));
        assert!(blocked.await.is_err());
        // ...but not for another one
        let other = tokio::time::timeout(
            std::time::Duration::from_millis(50),
            acquire("https://gate-test-other.example.org/announce"),
        );
        assert!(other.await.is_ok_and(|permit| permit.is_some()));

        drop(permits);
        let freed = tokio::time::timeout(std::time::Duration::from_millis(50), acquire(url));
        assert!(freed.await.is_ok_and(|permit| permit.is_some()));
    }
}
