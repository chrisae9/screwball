//! Short-lived foreground leases, scoped to one user's registered push endpoint.
//! Each document reports independently so closing one window cannot clear another.
use std::{
    collections::HashMap,
    sync::Mutex,
    time::{Duration, Instant},
};

use axum::{Json, extract::State, http::StatusCode};
use serde::Deserialize;
use uuid::Uuid;

use crate::{app::AppState, session::ApiAuthUser};

const LEASE: Duration = Duration::from_secs(12);
// Remember the sequence after a release to ignore an in-flight older heartbeat.
const FORGET: Duration = Duration::from_secs(60);

#[derive(Deserialize)]
pub struct PresenceReport {
    endpoint: String,
    window_id: Uuid,
    sequence: u64,
    visible: bool,
}

struct Window {
    endpoint: String,
    sequence: u64,
    visible: bool,
    updated: Instant,
}

#[derive(Default)]
pub struct Foreground {
    windows: Mutex<HashMap<(Uuid, Uuid), Window>>,
}

impl Foreground {
    fn update(&self, user: Uuid, report: PresenceReport, now: Instant) {
        let mut windows = self.windows.lock().unwrap();
        windows.retain(|_, w| now.saturating_duration_since(w.updated) < FORGET);
        let key = (user, report.window_id);
        if windows
            .get(&key)
            .is_some_and(|w| w.sequence >= report.sequence)
        {
            return;
        }
        windows.insert(
            key,
            Window {
                endpoint: report.endpoint,
                sequence: report.sequence,
                visible: report.visible,
                updated: now,
            },
        );
    }

    pub fn visible(&self, user: Uuid, endpoint: &str, now: Instant) -> bool {
        self.windows
            .lock()
            .unwrap()
            .iter()
            .any(|((owner, _), window)| {
                *owner == user
                    && window.visible
                    && window.endpoint == endpoint
                    && now.saturating_duration_since(window.updated) < LEASE
            })
    }
}

pub async fn update(
    State(state): State<AppState>,
    ApiAuthUser(user): ApiAuthUser,
    Json(report): Json<PresenceReport>,
) -> StatusCode {
    if !state.users.has_push_endpoint(user, &report.endpoint).await {
        return StatusCode::NOT_FOUND;
    }
    state.push.foreground.update(user, report, Instant::now());
    StatusCode::NO_CONTENT
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_and_devices_release_independently_and_expire() {
        let foreground = Foreground::default();
        let user = Uuid::new_v4();
        let other_user = Uuid::new_v4();
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let now = Instant::now();
        let report = |window_id, sequence, visible| PresenceReport {
            endpoint: "phone".into(),
            window_id,
            sequence,
            visible,
        };
        foreground.update(user, report(a, 1, true), now);
        foreground.update(user, report(b, 1, true), now);
        assert!(foreground.visible(user, "phone", now));
        assert!(!foreground.visible(user, "tablet", now));
        assert!(!foreground.visible(other_user, "phone", now));
        foreground.update(user, report(a, 3, false), now);
        assert!(foreground.visible(user, "phone", now));
        foreground.update(user, report(b, 2, false), now);
        foreground.update(user, report(a, 2, true), now);
        assert!(!foreground.visible(user, "phone", now));
        foreground.update(user, report(a, 4, true), now);
        assert!(foreground.visible(user, "phone", now + LEASE - Duration::from_secs(1)));
        assert!(!foreground.visible(user, "phone", now + LEASE));
    }
}
