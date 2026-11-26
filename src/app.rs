use std::{borrow::Cow, env, net::SocketAddr, sync::Arc, time::Instant};

use anyhow::{Context, Result};
use axum::{
    Router,
    extract::{FromRef, FromRequestParts, Request, State},
    http::{HeaderValue, header::CACHE_CONTROL},
    middleware::{Next, from_fn, from_fn_with_state},
    response::Response,
    routing::{get, post},
};
use axum_extra::extract::cookie::Key;
use tower_http::{services::ServeDir, trace::TraceLayer};
use webauthn_rs::prelude::{Url, Webauthn, WebauthnBuilder};

use crate::{
    auth, define::DefinitionCache, dict::Dictionary, push::PushService, routes, session::MaybeUser,
    store::GameStore, users::UserStore,
};

const LOCAL_DEV_SESSION_SECRET: &str =
    "screwball-local-development-session-secret-v1-keep-browser-sessions-across-restarts";

#[derive(Clone)]
pub struct AppState {
    pub dict: Arc<Dictionary>,
    pub defs: Arc<DefinitionCache>,
    pub store: Arc<GameStore>,
    pub users: Arc<UserStore>,
    pub webauthn: Arc<Webauthn>,
    pub key: Key,
    pub push: PushService,
    /// When set, auth skips the WebAuthn ceremony and trusts the username alone.
    /// Dev-only escape hatch — browsers dislike passkeys on localhost.
    pub passkey_disabled: bool,
}

/// Lets the signed-cookie extractors pull the signing key out of `AppState`.
impl FromRef<AppState> for Key {
    fn from_ref(state: &AppState) -> Self {
        state.key.clone()
    }
}

pub fn router(state: AppState) -> Router {
    let router = Router::new()
        .route("/", get(routes::index))
        .route("/leaderboard", get(crate::leaderboard::page))
        .route("/debug/notifications", get(routes::notification_debug_page))
        .route("/debug/touch", get(routes::touch_debug_page))
        .route("/sw.js", get(routes::service_worker))
        .route("/healthcheck", get(routes::healthcheck))
        .route("/auth/register/begin", post(auth::register_begin))
        .route("/auth/register/finish", post(auth::register_finish))
        .route("/auth/login/begin", post(auth::login_begin))
        .route("/auth/login/finish", post(auth::login_finish))
        .route("/auth/logout", post(auth::logout))
        .route(
            "/settings",
            get(routes::settings_page).post(routes::update_settings),
        )
        .route("/settings/username", post(routes::update_username))
        .route("/settings/avatar", post(routes::update_avatar))
        .route("/settings/away", post(routes::update_away))
        .route("/games", post(routes::create_game))
        .route("/games/new", get(routes::new_game_page))
        .route("/api/players", get(crate::invitations::search))
        .route("/api/activity", get(routes::activity))
        .route("/api/activity/events", get(routes::activity_events))
        .route("/api/invitations", get(crate::invitations::inbox))
        .route(
            "/games/{id}/invite",
            get(crate::invitations::page).post(crate::invitations::send),
        )
        .route("/games/{id}/decline", post(crate::invitations::decline))
        .route("/api/my-games", get(routes::my_games))
        .route("/api/push/vapid-public-key", get(routes::push_public_key))
        .route("/api/push/debug", get(routes::push_debug_status))
        .route("/api/push/presence", post(crate::presence::update))
        .route("/api/push/subscribe", post(routes::push_subscribe))
        .route("/api/push/unsubscribe", post(routes::push_unsubscribe))
        .route("/api/push/test", post(routes::push_test))
        .route("/games/{id}", get(routes::game_page))
        .route("/games/{id}/chat", get(crate::chat::page))
        .route("/api/games/{id}/chat/read", post(crate::chat::mark_read))
        .route(
            "/api/games/{id}/chat",
            get(crate::chat::history).post(crate::chat::send),
        )
        .route("/games/{id}/join", post(routes::join_game))
        .route("/games/{id}/state", get(routes::game_state))
        .route("/games/{id}/events", get(routes::game_events))
        .route("/games/{id}/preview", post(routes::preview_move))
        .route("/games/{id}/move", post(routes::submit_move))
        .route("/games/{id}/abandon", post(routes::abandon_game))
        .route("/games/{id}/hint", post(routes::hint))
        .route("/api/define/{word}", get(routes::define))
        .nest_service("/public", ServeDir::new("public"))
        .layer(from_fn(cache_control))
        .layer(from_fn_with_state(state.clone(), log_request))
        .layer(TraceLayer::new_for_http());
    let router = if env_flag("SCREWBALL_AUTHENTIK") {
        router.layer(from_fn_with_state(state.clone(), authentik_player))
    } else {
        router
    };
    router.with_state(state)
}

// Phones can request install metadata without the signed-in browser cookies.
const INSTALL_ASSETS: &[&str] = &[
    "/public/manifest.webmanifest",
    "/public/apple-touch-icon.png",
    "/public/favicon.svg",
    "/public/scrabble-192.png",
    "/public/scrabble-512.png",
];

// Enabled only behind the header-clearing Authentik gateway and its ingress policy.
async fn authentik_player(State(state): State<AppState>, req: Request, next: Next) -> Response {
    use axum::http::{Method, StatusCode};
    use axum::response::{IntoResponse, Redirect};

    let (mut parts, body) = req.into_parts();
    let path = parts.uri.path();
    if path == "/healthcheck"
        || (matches!(parts.method, Method::GET | Method::HEAD) && INSTALL_ASSETS.contains(&path))
    {
        return next.run(Request::from_parts(parts, body)).await;
    }
    let header = |name: &str| parts.headers.get(name).and_then(|h| h.to_str().ok());
    let Some(uid) = header("x-authentik-uid").filter(|s| {
        s.len() >= 16 && s.len() <= 128 && s.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-')
    }) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    if !matches!(parts.method, Method::GET | Method::HEAD | Method::OPTIONS)
        && (header("sec-fetch-site") == Some("cross-site")
            || header("origin")
                .is_some_and(|origin| Some(origin) != env::var("RP_ORIGIN").ok().as_deref()))
    {
        return StatusCode::FORBIDDEN.into_response();
    }
    if path == "/auth/logout" {
        return Redirect::to("/outpost.goauthentik.io/sign_out").into_response();
    }
    if path.starts_with("/auth/") {
        return StatusCode::NOT_FOUND.into_response();
    }
    let player = match state
        .users
        .authentik_profile(uid, header("x-authentik-name").unwrap_or("Player"))
        .await
    {
        Ok(player) => player,
        Err(err) => return err.into_response(),
    };
    parts.extensions.insert(crate::session::AuthUser(player));
    next.run(Request::from_parts(parts, body)).await
}

/// Log every request's method, path, status, signed-in username, and wall-clock
/// duration so prod latency spikes are visible. Static assets and the health
/// check are skipped to keep the log readable; anything slow is bumped to a
/// warning. Guests log as `-`.
async fn log_request(State(state): State<AppState>, request: Request, next: Next) -> Response {
    let method = request.method().clone();
    let path = request.uri().path().to_string();
    if path.starts_with("/public/") || path == "/healthcheck" {
        return next.run(request).await;
    }

    // Resolve the username from the signed session cookie before the body is
    // consumed; falls back to the user id, then `-` for guests.
    let (mut parts, body) = request.into_parts();
    let user_id = MaybeUser::from_request_parts(&mut parts, &state)
        .await
        .map(|m| m.0)
        .unwrap_or(None);
    let request = Request::from_parts(parts, body);

    let start = Instant::now();
    let response = next.run(request).await;
    let elapsed_ms = start.elapsed().as_millis();
    let status = response.status().as_u16();

    let user = match user_id {
        Some(id) => state
            .users
            .get(id)
            .await
            .map(|u| u.username)
            .unwrap_or_else(|| id.to_string()),
        None => "-".to_string(),
    };

    if elapsed_ms >= 500 {
        tracing::warn!(%method, path, status, user, elapsed_ms, "slow request");
    } else {
        tracing::info!(%method, path, status, user, elapsed_ms, "request");
    }
    response
}

/// Cache policy: release `/public` asset URLs (`?v=hash`) are content-addressed
/// so cache them immutably. Debug builds use no-cache because local files can
/// change without restarting the server, and stale JS/CSS is painful to debug.
async fn cache_control(request: Request, next: Next) -> Response {
    let path = request.uri().path();
    let is_asset = path.starts_with("/public/") && !INSTALL_ASSETS.contains(&path);
    let private = path.starts_with("/api/")
        || path.ends_with("/events")
        || path.ends_with("/state")
        || path.ends_with("/chat");
    let mut response = next.run(request).await;
    let value = if private {
        "private, no-store"
    } else if is_asset && !cfg!(debug_assertions) {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static(value));
    response
}

pub async fn run() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            // Default to info so request-duration logs show up in prod even when
            // RUST_LOG is unset.
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    crate::render::set_asset_version(asset_version());

    let dict_source: Cow<'static, str> = match env::var("DICTIONARY_PATH") {
        Ok(path) if !path.is_empty() => std::fs::read_to_string(&path)
            .with_context(|| format!("reading dictionary at {path}"))?
            .into(),
        _ => crate::dict::DEFAULT_WORDS.into(),
    };

    let dict = Arc::new(Dictionary::from_words(&dict_source));
    tracing::info!("loaded dictionary with {} words", dict.word_count());

    let data_path = env::var("DATA_PATH").unwrap_or_else(|_| "data".to_string());
    let defs = Arc::new(DefinitionCache::from_words(&dict_source));
    let store = Arc::new(GameStore::load(&data_path).await?);
    let users = Arc::new(UserStore::load(&data_path).await?);
    // Pre-approved players can receive an invitation before their first visit.
    if env_flag("SCREWBALL_AUTHENTIK")
        && let Ok(raw) = env::var("SCREWBALL_INVITABLE_PLAYERS")
    {
        let roster: std::collections::BTreeMap<String, String> =
            serde_json::from_str(&raw).context("invalid invitable player roster")?;
        for (uid, name) in roster {
            users.authentik_profile(&uid, &name).await?;
        }
    }
    tracing::info!("loaded {} registered users", users.count().await);

    let webauthn = Arc::new(build_webauthn()?);
    let key = load_key();
    let push = PushService::from_env().context("failed to configure web push")?;
    if push.is_enabled() {
        tracing::info!("web push notifications enabled");
    } else {
        tracing::warn!("VAPID_PRIVATE_KEY is not set; web push notifications are disabled");
    }
    let passkey_disabled = env_flag("PASSKEY_DISABLED");
    if passkey_disabled {
        tracing::warn!("PASSKEY_DISABLED set; auth trusts username only (dev mode)");
    }

    let app = router(AppState {
        dict,
        defs,
        store,
        users,
        webauthn,
        key,
        push,
        passkey_disabled,
    });

    let port = env::var("PORT").unwrap_or_else(|_| "8080".to_string());
    let addr: SocketAddr = format!("0.0.0.0:{port}").parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    let bound = listener.local_addr()?;
    println!("listening on port {}", bound.port());
    tracing::info!("listening on http://{bound}");
    axum::serve(listener, app).await?;
    Ok(())
}

/// Build the WebAuthn relying party from `RP_ID` / `RP_ORIGIN` (defaults suit
/// local development on `http://localhost:8080`).
pub fn build_webauthn() -> Result<Webauthn> {
    let rp_id = env::var("RP_ID").unwrap_or_else(|_| "localhost".to_string());
    let rp_origin_raw =
        env::var("RP_ORIGIN").unwrap_or_else(|_| "http://localhost:8080".to_string());
    let rp_origin = Url::parse(&rp_origin_raw).context("RP_ORIGIN must be a valid URL")?;
    WebauthnBuilder::new(&rp_id, &rp_origin)
        .context("invalid WebAuthn relying-party configuration")?
        .rp_name("Scrabble")
        .build()
        .context("failed to build WebAuthn relying party")
}

/// A short hash of the static assets, used to cache-bust their URLs. Stable for
/// identical content across restarts; changes when any asset changes.
fn asset_version() -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    include_bytes!("render.rs").hash(&mut hasher);
    for file in [
        "public/manifest.webmanifest",
        "public/apple-touch-icon.png",
        "public/favicon.svg",
        "public/scrabble-192.png",
        "public/scrabble-512.png",
        "public/app.css",
        "public/dark.css",
        "public/theme.js",
        "public/game.js",
        "public/chat.js",
        "public/chat.css",
        "public/players.js",
        "public/presence.js",
        "public/player-icons.png",
        "public/player-icons.json",
        "public/settings.js",
        "public/auth.js",
        "public/touch-debug.js",
        "public/sw.js",
    ] {
        if let Ok(bytes) = std::fs::read(file) {
            bytes.hash(&mut hasher);
        }
    }
    format!("{:x}", hasher.finish())
}

/// Read a boolean env flag (`1`/`true`, case-insensitive); absent or anything
/// else is false.
fn env_flag(name: &str) -> bool {
    matches!(
        env::var(name).as_deref(),
        Ok("1") | Ok("true") | Ok("TRUE") | Ok("True")
    )
}

/// Derive the cookie-signing key from `SESSION_SECRET`. Debug builds use a
/// stable local-dev fallback so `cargo run` restarts keep browser sessions;
/// release builds keep the safer ephemeral fallback when the secret is unset.
fn load_key() -> Key {
    match env::var("SESSION_SECRET") {
        Ok(secret) if secret.len() >= 64 => Key::from(secret.as_bytes()),
        Ok(_) => {
            tracing::warn!(
                "SESSION_SECRET is shorter than 64 bytes; using an ephemeral key (sessions reset on restart)"
            );
            Key::generate()
        }
        Err(_) => {
            if cfg!(debug_assertions) {
                tracing::info!("SESSION_SECRET is not set; using stable local-dev session key");
                Key::from(LOCAL_DEV_SESSION_SECRET.as_bytes())
            } else {
                tracing::warn!(
                    "SESSION_SECRET is not set; using an ephemeral key (sessions reset on restart)"
                );
                Key::generate()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use cookie::{Cookie as RawCookie, CookieJar};

    use super::*;

    fn signed_sid_value(key: &Key) -> String {
        let mut jar = CookieJar::new();
        jar.signed_mut(key)
            .add(RawCookie::new("sid", "local-dev-user"));
        jar.get("sid").unwrap().value().to_string()
    }

    #[test]
    fn local_dev_session_secret_is_stable() {
        let first = Key::from(LOCAL_DEV_SESSION_SECRET.as_bytes());
        let second = Key::from(LOCAL_DEV_SESSION_SECRET.as_bytes());

        assert_eq!(signed_sid_value(&first), signed_sid_value(&second));
    }

    #[test]
    fn local_dev_session_secret_is_long_enough_for_cookie_key() {
        assert!(LOCAL_DEV_SESSION_SECRET.len() >= 64);
    }
}
