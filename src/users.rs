use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use uuid::Uuid;
use webauthn_rs::prelude::Passkey;

use crate::error::AppError;

/// A registered player. `username` is the private passkey login or Authentik
/// subject key. Games and sessions identify players by the stable UUID.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct User {
    pub id: Uuid,
    pub username: String,
    pub display_name: String,
    pub credentials: Vec<Passkey>,
    #[serde(default)]
    pub push_subscriptions: Vec<PushSubscription>,
    #[serde(default)]
    pub settings: UserSettings,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct UserSettings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub player_handle: Option<String>,
    #[serde(default)]
    pub auto_zoom: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avatar_id: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub away_until: Option<DateTime<Utc>>,
    #[serde(default)]
    pub away_indefinitely: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AwayState {
    #[default]
    Off,
    Until(DateTime<Utc>),
    Indefinite,
}

impl AwayState {
    pub fn is_active(self) -> bool {
        self != Self::Off
    }

    /// This value changes when a pause is extended, replaced or expires.
    pub fn token(self) -> String {
        match self {
            Self::Off => "off".into(),
            Self::Indefinite => "indefinite".into(),
            Self::Until(until) => until.to_rfc3339(),
        }
    }
}

impl UserSettings {
    pub fn avatar(&self, user_id: Uuid) -> u8 {
        self.avatar_id
            .filter(|&id| id < crate::avatars::COUNT)
            .unwrap_or((user_id.as_u128() % u128::from(crate::avatars::COUNT)) as u8)
    }

    pub fn away_state(&self, now: DateTime<Utc>) -> AwayState {
        if self.away_indefinitely {
            AwayState::Indefinite
        } else {
            self.away_until
                .filter(|until| *until > now)
                .map_or(AwayState::Off, AwayState::Until)
        }
    }

    pub fn set_away(&mut self, state: AwayState) {
        self.away_indefinitely = state == AwayState::Indefinite;
        self.away_until = match state {
            AwayState::Until(until) => Some(until),
            _ => None,
        };
    }
}

/// Browser Push API subscription persisted per user. This mirrors the
/// `PushSubscription.toJSON()` payload used by service workers.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PushSubscription {
    pub endpoint: String,
    pub keys: PushSubscriptionKeys,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PushSubscriptionKeys {
    pub p256dh: String,
    pub auth: String,
}

/// Normalize a login handle so lookups are case- and whitespace-insensitive.
pub fn normalize_username(raw: &str) -> String {
    raw.trim().to_lowercase()
}

/// A cheap snapshot of public names, independent of sign-in and saved games.
#[derive(Clone, Default)]
pub struct PlayerNames(Arc<HashMap<Uuid, String>>);

impl PlayerNames {
    pub fn name<'a>(&'a self, id: Uuid, fallback: &'a str) -> &'a str {
        self.0.get(&id).map(String::as_str).unwrap_or(fallback)
    }

    pub fn seat<'a>(&'a self, seat: &'a crate::models::Seat) -> Cow<'a, str> {
        use crate::models::SeatKind;
        if let SeatKind::Human { user_id: Some(id) } = seat.kind {
            return Cow::Borrowed(self.name(id, &seat.name));
        }
        if let Some(invite) = &seat.invitation
            && !invite.declined
            && let Some(handle) = self.0.get(&invite.player)
        {
            return Cow::Owned(format!("{handle} (invited)"));
        }
        Cow::Borrowed(&seat.name)
    }
}

#[derive(Serialize)]
pub struct DirectoryPlayer {
    pub id: Uuid,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub handle: Option<String>,
    pub avatar_id: u8,
}

fn normalize_public_handle(raw: &str) -> Result<String, AppError> {
    let trimmed = raw.trim();
    let name = trimmed.strip_prefix('@').unwrap_or(trimmed);
    if !(3..=20).contains(&name.len())
        || !name.as_bytes()[0].is_ascii_alphanumeric()
        || !name.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_')
    {
        return Err(AppError::bad_request(
            "Use 3–20 letters, numbers or underscores. Start with a letter or number.",
        ));
    }
    Ok(name.to_ascii_lowercase())
}

struct Index {
    by_id: HashMap<Uuid, User>,
    names: PlayerNames,
    by_username: HashMap<String, Uuid>,
}

/// In-memory registry of users backed by atomic per-user JSON files on disk.
pub struct UserStore {
    index: Mutex<Index>,
    dir: PathBuf,
    hidden_player_handles: HashSet<String>,
}

impl UserStore {
    /// Load all persisted users under `<data_root>/users`, creating the
    /// directory if needed. Invalid files are skipped with a warning.
    pub async fn load(data_root: impl Into<PathBuf>) -> Result<Self, AppError> {
        let dir = data_root.into().join("users");
        tokio::fs::create_dir_all(&dir)
            .await
            .map_err(AppError::internal)?;

        let mut by_id = HashMap::new();
        let mut by_username = HashMap::new();
        let mut entries = tokio::fs::read_dir(&dir)
            .await
            .map_err(AppError::internal)?;
        while let Some(entry) = entries.next_entry().await.map_err(AppError::internal)? {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                continue;
            }
            match tokio::fs::read_to_string(&path).await {
                Ok(text) => match serde_json::from_str::<User>(&text) {
                    Ok(user) => {
                        by_username.insert(normalize_username(&user.username), user.id);
                        by_id.insert(user.id, user);
                    }
                    Err(err) => {
                        tracing::warn!(path = %path.display(), error = %err, "skipping invalid user file")
                    }
                },
                Err(err) => {
                    tracing::warn!(path = %path.display(), error = %err, "could not read user file")
                }
            }
        }

        let names = PlayerNames(Arc::new(
            by_id
                .values()
                .filter_map(|u| {
                    u.settings
                        .player_handle
                        .as_ref()
                        .map(|handle| (u.id, format!("@{handle}")))
                })
                .collect(),
        ));
        Ok(Self {
            index: Mutex::new(Index {
                by_id,
                by_username,
                names,
            }),
            dir,
            hidden_player_handles: std::env::var("SCREWBALL_HIDDEN_PLAYER_UIDS")
                .unwrap_or_default()
                .split(',')
                .map(str::trim)
                .filter(|uid| !uid.is_empty())
                .map(|uid| format!("ak-{uid}"))
                .collect(),
        })
    }

    pub async fn get(&self, id: Uuid) -> Option<User> {
        self.index.lock().await.by_id.get(&id).cloned()
    }

    pub async fn has_push_endpoint(&self, id: Uuid, endpoint: &str) -> bool {
        self.index.lock().await.by_id.get(&id).is_some_and(|user| {
            user.push_subscriptions
                .iter()
                .any(|subscription| subscription.endpoint == endpoint)
        })
    }

    pub async fn invitable_player(&self, id: Uuid) -> Option<User> {
        self.get(id)
            .await
            .filter(|user| !self.hidden_player_handles.contains(&user.username))
    }

    /// Use the same identity for roster provisioning and first sign-in.
    pub async fn authentik_profile(&self, uid: &str, name: &str) -> Result<Uuid, AppError> {
        let username = format!("ak-{uid}");
        if let Some(user) = self.get_by_username(&username).await {
            return Ok(user.id);
        }
        let id = Uuid::new_v4();
        let user = User {
            id,
            username: username.clone(),
            display_name: name
                .split('@')
                .next()
                .unwrap_or("Player")
                .chars()
                .take(48)
                .collect(),
            credentials: vec![],
            push_subscriptions: vec![],
            settings: UserSettings {
                auto_zoom: true,
                ..Default::default()
            },
            created_at: Utc::now(),
        };
        match self.insert(user).await {
            Ok(()) => Ok(id),
            Err(err) => self
                .get_by_username(&username)
                .await
                .map(|user| user.id)
                .ok_or(err),
        }
    }

    pub async fn player_names(&self) -> PlayerNames {
        self.index.lock().await.names.clone()
    }

    pub async fn set_player_handle(&self, id: Uuid, raw: Option<&str>) -> Result<(), AppError> {
        let handle = raw.map(normalize_public_handle).transpose()?;
        self.update(id, |user| user.settings.player_handle = handle)
            .await
    }

    pub async fn search_players(&self, viewer: Uuid, query: &str) -> Vec<DirectoryPlayer> {
        let query = query.trim().to_lowercase();
        let index = self.index.lock().await;
        let mut matches: Vec<_> = index
            .by_id
            .values()
            .filter(|u| {
                u.id != viewer
                    && !self.hidden_player_handles.contains(&u.username)
                    && (u.display_name.to_lowercase().contains(&query)
                        || u.settings
                            .player_handle
                            .as_ref()
                            .is_some_and(|h| h.contains(query.strip_prefix('@').unwrap_or(&query))))
            })
            .map(|u| DirectoryPlayer {
                id: u.id,
                name: u.display_name.clone(),
                handle: u.settings.player_handle.clone(),
                avatar_id: u.settings.avatar(u.id),
            })
            .collect();
        matches.sort_by_cached_key(|p| (p.name.to_lowercase(), p.handle.clone(), p.id));
        matches
    }

    pub async fn count(&self) -> usize {
        self.index.lock().await.by_id.len()
    }

    pub async fn get_by_username(&self, username: &str) -> Option<User> {
        let key = normalize_username(username);
        let guard = self.index.lock().await;
        let id = guard.by_username.get(&key)?;
        guard.by_id.get(id).cloned()
    }

    pub async fn username_taken(&self, username: &str) -> bool {
        let key = normalize_username(username);
        self.index.lock().await.by_username.contains_key(&key)
    }

    /// Persist a new user, then commit it to memory and the username index.
    /// Fails if the username is already taken.
    pub async fn insert(&self, user: User) -> Result<(), AppError> {
        let key = normalize_username(&user.username);
        let mut guard = self.index.lock().await;
        if guard.by_username.contains_key(&key) {
            return Err(AppError::conflict("that username is already taken"));
        }
        self.persist(&user).await?;
        guard.by_username.insert(key, user.id);
        guard.by_id.insert(user.id, user);
        Ok(())
    }

    /// Mutate a user under the registry lock, persisting before the change
    /// becomes visible in memory (mirrors `GameStore::update`). Keeps the
    /// username index consistent if the closure changes the username, and
    /// fails the update if the new username is already claimed by someone else.
    pub async fn update<R>(&self, id: Uuid, f: impl FnOnce(&mut User) -> R) -> Result<R, AppError> {
        let mut guard = self.index.lock().await;
        let mut working = guard
            .by_id
            .get(&id)
            .ok_or_else(|| AppError::not_found("user not found"))?
            .clone();
        let old_key = normalize_username(&working.username);
        let old_handle = working.settings.player_handle.clone();
        let outcome = f(&mut working);
        let new_key = normalize_username(&working.username);
        if new_key != old_key
            && guard
                .by_username
                .get(&new_key)
                .is_some_and(|existing| *existing != id)
        {
            return Err(AppError::conflict("that username is already taken"));
        }
        let handle_changed = working.settings.player_handle != old_handle;
        if handle_changed
            && let Some(handle) = &working.settings.player_handle
            && guard
                .by_id
                .values()
                .any(|u| u.id != id && u.settings.player_handle.as_ref() == Some(handle))
        {
            return Err(AppError::conflict(
                "That Scrabble username is taken. Try another.",
            ));
        }
        self.persist(&working).await?;
        if handle_changed {
            let names = Arc::make_mut(&mut guard.names.0);
            if let Some(handle) = &working.settings.player_handle {
                names.insert(id, format!("@{handle}"));
            } else {
                names.remove(&id);
            }
        }
        if new_key != old_key {
            guard.by_username.remove(&old_key);
            guard.by_username.insert(new_key, id);
        }
        guard.by_id.insert(id, working);
        Ok(outcome)
    }

    async fn persist(&self, user: &User) -> Result<(), AppError> {
        let path = self.dir.join(format!("{}.json", user.id));
        let tmp = temp_path(&path);
        let bytes = serde_json::to_vec_pretty(user).map_err(AppError::internal)?;
        tokio::fs::write(&tmp, bytes)
            .await
            .map_err(AppError::internal)?;
        tokio::fs::rename(&tmp, &path)
            .await
            .map_err(AppError::internal)?;
        Ok(())
    }
}

fn temp_path(path: &Path) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    path.with_extension(format!("json.tmp-{nanos}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch_dir() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default();
        std::env::temp_dir().join(format!("screwball-users-{nanos}-{}", Uuid::new_v4()))
    }

    fn sample(username: &str) -> User {
        User {
            id: Uuid::new_v4(),
            username: username.to_string(),
            display_name: username.to_string(),
            credentials: Vec::new(),
            push_subscriptions: Vec::new(),
            settings: UserSettings::default(),
            created_at: Utc::now(),
        }
    }

    #[tokio::test]
    async fn browse_all_invitable_players_and_filter_on_one_character() {
        let mut store = UserStore::load(scratch_dir()).await.unwrap();
        let viewer = store.authentik_profile("viewer", "Viewer").await.unwrap();
        let hidden = store.authentik_profile("test-uid", "Robot").await.unwrap();
        store.hidden_player_handles.insert("ak-test-uid".into());
        for index in (1..=16).rev() {
            let name = format!("Player {index:02}");
            store.insert(sample(&name)).await.unwrap();
        }
        let roster = store.search_players(viewer, "").await;
        assert_eq!(roster.len(), 16);
        assert_eq!(roster[0].name, "Player 01");
        assert_eq!(roster[15].name, "Player 16");
        assert_eq!(store.search_players(viewer, "6").await.len(), 2);
        assert!(store.invitable_player(hidden).await.is_none());
        assert!(store.get(hidden).await.is_some());
    }

    #[test]
    fn public_username_validation_matches_its_display_format() {
        assert_eq!(
            normalize_public_handle("  @Tile_Fox  ").unwrap(),
            "tile_fox"
        );
        for invalid in [
            "",
            "@",
            "ab",
            "_starts",
            "a b",
            "a-b",
            "Éclair",
            "@@fox",
            "012345678901234567890",
        ] {
            assert!(normalize_public_handle(invalid).is_err(), "{invalid:?}");
        }
        assert_eq!(normalize_public_handle("1_2").unwrap(), "1_2");
        assert!(normalize_public_handle("01234567890123456789").is_ok());
    }

    #[tokio::test]
    async fn concurrent_handle_claims_and_renames_keep_authentik_identity_after_restart() {
        let root = scratch_dir();
        let store = UserStore::load(&root).await.unwrap();
        let first = store
            .authentik_profile("subject-one", "Alex")
            .await
            .unwrap();
        let second = store
            .authentik_profile("subject-two", "Alex")
            .await
            .unwrap();
        let (a, b) = tokio::join!(
            store.set_player_handle(first, Some("TileFox")),
            store.set_player_handle(second, Some("@TILEFOX"))
        );
        assert_ne!(a.is_ok(), b.is_ok());
        let (winner, other, uid) = if a.is_ok() {
            (first, second, "subject-one")
        } else {
            (second, first, "subject-two")
        };
        assert_eq!(store.player_names().await.name(winner, "Alex"), "@tilefox");
        assert_eq!(store.player_names().await.name(other, "Alex"), "Alex");
        store
            .set_player_handle(winner, Some("Word_Bird"))
            .await
            .unwrap();
        store
            .set_player_handle(other, Some("TileFox"))
            .await
            .unwrap();
        assert!(store.get_by_username("word_bird").await.is_none());
        drop(store);
        let store = UserStore::load(&root).await.unwrap();
        assert_eq!(
            store
                .authentik_profile(uid, "New upstream label")
                .await
                .unwrap(),
            winner
        );
        assert_eq!(
            store.player_names().await.name(winner, "Alex"),
            "@word_bird"
        );
        let matches = store.search_players(other, "@Word_B").await;
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].id, winner);
        assert_eq!(matches[0].name, "Alex");
        assert_eq!(matches[0].handle.as_deref(), Some("word_bird"));
        assert!(store.search_players(other, uid).await.is_empty());
        store.set_player_handle(winner, None).await.unwrap();
        assert_eq!(store.player_names().await.name(winner, "Alex"), "Alex");
        tokio::fs::remove_dir_all(root).await.unwrap();
    }

    #[tokio::test]
    async fn update_keeps_username_index_consistent_on_rename() {
        let store = UserStore::load(scratch_dir()).await.unwrap();
        let user = sample("Alice");
        let id = user.id;
        store.insert(user).await.unwrap();

        store
            .update(id, |u| u.username = "Bob".to_string())
            .await
            .unwrap();

        assert!(store.get_by_username("alice").await.is_none());
        assert_eq!(store.get_by_username("BOB").await.unwrap().id, id);
        assert!(store.username_taken("bob").await);
    }

    #[tokio::test]
    async fn update_rejects_rename_to_taken_username() {
        let store = UserStore::load(scratch_dir()).await.unwrap();
        let alice = sample("alice");
        let alice_id = alice.id;
        store.insert(alice).await.unwrap();
        store.insert(sample("bob")).await.unwrap();

        let result = store
            .update(alice_id, |u| u.username = "BOB".to_string())
            .await;

        assert!(result.is_err());
        // The rejected rename leaves both index entries intact.
        assert_eq!(store.get_by_username("alice").await.unwrap().id, alice_id);
        assert!(store.username_taken("bob").await);
    }
}

#[cfg(test)]
mod away_tests {
    use super::*;

    #[test]
    fn indefinite_and_timed_pauses_replace_each_other_and_survive_reload() {
        let now = Utc::now();
        let later = now + chrono::Duration::days(2);
        let legacy = serde_json::json!({"auto_zoom": true, "away_until": later});
        let mut settings: UserSettings = serde_json::from_value(legacy).unwrap();
        assert_eq!(settings.away_state(now), AwayState::Until(later));
        settings.set_away(AwayState::Indefinite);
        let mut saved: UserSettings =
            serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();
        assert_eq!(saved.away_until, None);
        assert_eq!(
            saved.away_state(later + chrono::Duration::days(365)),
            AwayState::Indefinite
        );
        saved.set_away(AwayState::Until(later));
        assert!(!saved.away_indefinitely);
        assert_eq!(saved.away_state(now), AwayState::Until(later));
        saved.set_away(AwayState::Off);
        assert_eq!(saved.away_state(now), AwayState::Off);
        assert!(saved.auto_zoom);
    }

    #[test]
    fn legacy_settings_are_available_and_away_expires_at_the_deadline() {
        let mut settings: UserSettings = serde_json::from_str(r#"{"auto_zoom":true}"#).unwrap();
        let now = Utc::now();
        assert_eq!(settings.away_state(now), AwayState::Off);
        settings.away_until = Some(now);
        assert_eq!(
            settings.away_state(now - chrono::Duration::seconds(1)),
            AwayState::Until(now)
        );
        assert_eq!(settings.away_state(now), AwayState::Off);
        assert_eq!(
            settings.away_state(now + chrono::Duration::seconds(1)),
            AwayState::Off
        );
    }
}
