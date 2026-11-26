use std::sync::OnceLock;

use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::models::{BOARD_SIZE, Board, Game, GameStatus, SeatKind};
use crate::view::{GameView, PositionView, SquareView, effective_updated_at, premium_code};

/// Content-hash of the static assets, set once at startup for release builds.
/// Debug builds hash each asset at render time so local edits do not keep
/// pointing browsers at stale immutable URLs.
static ASSET_VERSION: OnceLock<String> = OnceLock::new();

pub fn set_asset_version(version: String) {
    let _ = ASSET_VERSION.set(version);
}

fn asset_version() -> &'static str {
    ASSET_VERSION.get().map(String::as_str).unwrap_or("dev")
}

/// A `/public/...` URL with the current asset version appended for cache-busting.
fn asset(path: &str) -> String {
    format!("{path}?v={}", asset_version_for(path))
}

#[cfg(debug_assertions)]
fn asset_version_for(path: &str) -> String {
    use std::hash::{Hash, Hasher};

    let file = path.trim_start_matches('/');
    if let Ok(bytes) = std::fs::read(file) {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        bytes.hash(&mut hasher);
        return format!("{:x}", hasher.finish());
    }

    asset_version().to_string()
}

#[cfg(not(debug_assertions))]
fn asset_version_for(_path: &str) -> String {
    asset_version().to_string()
}

pub fn escape(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

pub fn layout(title: &str, body: &str) -> String {
    layout_with_head(title, body, "", "", "")
}

fn layout_with_head(
    title: &str,
    body: &str,
    head_extra: &str,
    body_class: &str,
    nav_actions: &str,
) -> String {
    layout_with_back(
        title,
        body,
        head_extra,
        body_class,
        nav_actions,
        BackTarget::Games,
    )
}

enum BackTarget {
    Games,
    Board(Uuid),
    Settings,
}

fn layout_with_back(
    title: &str,
    body: &str,
    head_extra: &str,
    body_class: &str,
    nav_actions: &str,
    parent: BackTarget,
) -> String {
    format!(
        r##"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width,initial-scale=1,maximum-scale=1,user-scalable=no,viewport-fit=cover">
  <meta name="apple-mobile-web-app-title" content="Scrabble">
  <meta name="apple-mobile-web-app-capable" content="yes">
  <meta name="apple-mobile-web-app-status-bar-style" content="default">
  <title>{title}</title>
  <link rel="icon" href="{favicon}">
  <link rel="apple-touch-icon" sizes="180x180" href="{touch_icon}">
  <link rel="manifest" href="{manifest}">
  <link rel="stylesheet" href="{css}">
  <style>:root{{--player-atlas:url('{player_atlas}')}}</style>
  <link id="dark-palette" rel="stylesheet" href="{dark_css}" media="(prefers-color-scheme: dark)">
  <meta name="theme-color" content="#f4f1ea" media="(prefers-color-scheme: light)">
  <meta name="theme-color" content="#11130f" media="(prefers-color-scheme: dark)">
  <script src="{theme_js}"></script>
  <script type="module" src="{presence_js}"></script>
  <script type="module" src="{players_js}"></script>
  {head_extra}
</head>
<body class="{body_class}">
  <div class="status-bar-edge" aria-hidden="true"></div>
  <nav class="nav">
  {back}
  <div class="nav-links">{nav_actions}</div>
</nav>
  <main class="page">
  {body}
  </main>
</body>
</html>"##,
        title = escape(title),
        css = asset("/public/app.css"),
        player_atlas = asset("/public/player-icons.png"),
        dark_css = asset("/public/dark.css"),
        theme_js = asset("/public/theme.js"),
        favicon = asset("/public/favicon.svg"),
        presence_js = asset("/public/presence.js"),
        players_js = asset("/public/players.js"),
        back = if let BackTarget::Board(id) = parent {
            format!(
                r#"<a class="back-link" href="/games/{id}" aria-label="Back to board"><span aria-hidden="true">‹</span> Board</a>"#
            )
        } else if matches!(parent, BackTarget::Settings) {
            r#"<a class="back-link" href="/settings" aria-label="Back to settings"><span aria-hidden="true">‹</span> Settings</a>"#.to_string()
        } else if title == "Scrabble" {
            r#"<a class="brand" href="/" aria-label="Scrabble home">Scrabble</a>"#.to_string()
        } else {
            r#"<a class="back-link" href="/" aria-label="Back to games"><span aria-hidden="true">‹</span> Back</a>"#.to_string()
        },
        touch_icon = asset("/public/apple-touch-icon.png"),
        manifest = asset("/public/manifest.webmanifest"),
        head_extra = head_extra,
        body_class = body_class,
        body = body,
    )
}

pub fn error_page(title: &str, message: &str) -> String {
    layout(
        title,
        &format!(
            r#"<section class="card">
  <h1>{}</h1>
  <p>{}</p>
</section>"#,
            escape(title),
            escape(message)
        ),
    )
}

#[derive(serde::Serialize)]
pub struct HomeActivity {
    pub version: &'static str,
    pub away: bool,
    pub invitations: String,
    pub open_games: String,
    pub games: String,
}

impl HomeActivity {
    /// Older open dashboards only replace the contents of #my-games. Deliver
    /// the stylesheet with that fragment so they can render the current cards.
    pub fn include_dashboard_styles(&mut self) {
        let css = asset("/public/app.css");
        self.games
            .insert_str(0, &format!(r#"<link rel="stylesheet" href="{css}">"#));
    }
}

pub fn home_activity(
    games: &[&Game],
    joinable_games: &[&Game],
    current: Uuid,
    names: &crate::users::PlayerNames,
    invitations: String,
    away: bool,
) -> HomeActivity {
    let list = if games.is_empty() {
        "<p class=\"muted\">No games yet. Create one to get started.</p>".to_string()
    } else {
        game_list(games, current, names)
    };
    let open_games = if joinable_games.is_empty() {
        String::new()
    } else {
        format!(
            r#"<h1>Open games</h1>
  {}"#,
            open_game_list(joinable_games, names),
        )
    };
    HomeActivity {
        version: asset_version(),
        away,
        games: list,
        open_games,
        invitations,
    }
}

pub fn home_page(activity: Option<HomeActivity>, profile: Option<&crate::users::User>) -> String {
    let Some(HomeActivity {
        version,
        away,
        invitations,
        open_games,
        games: list,
    }) = activity
    else {
        return layout_with_head(
            "Scrabble",
            &signed_out_panel(),
            &format!(
                r#"<script type="module" src="{}" defer></script>"#,
                asset("/public/auth.js")
            ),
            "",
            "",
        );
    };
    let greeting = profile
        .map(|u| {
            format!(
                "{} <span>Signed in as <strong>{}</strong></span>",
                crate::avatars::image(u.settings.avatar(u.id)),
                u.settings
                    .player_handle
                    .as_deref()
                    .map(|h| format!("@{}", escape(h)))
                    .unwrap_or_else(|| escape(&u.display_name))
            )
        })
        .unwrap_or_else(|| "Signed in".to_string());
    layout_with_head(
        "Scrabble",
        &format!(
            r#"<section class="card account">
  <p class="muted profile-greeting">{greeting}</p>
  <a class="button new-game-link" href="/games/new">New game</a>
  <a class="button button-secondary" href="/leaderboard">Leaderboard</a>
  <a class="button button-secondary" href="/settings">Settings</a>
  <form method="post" action="/auth/logout">
    <button type="submit" class="button button-secondary">Sign out</button>
  </form>
</section>
<section class="card" id="invitations" aria-live="polite">{invitations}</section>
<details id="install-guide" class="card" hidden><summary>Install Scrabble on your phone</summary><ol></ol></details>
<aside class="card notification-controls" id="phone-alerts" data-away="{away}" hidden>
  <strong>Phone alerts</strong><p class="muted" id="notif-status" role="status"></p>
  <button type="button" id="notif-enable" class="button" hidden>Enable notifications</button>
</aside>
<section class="card match-section">
  <h1>Your games</h1>
  <div id="my-games" data-dashboard-version="{version}">{list}</div>
</section>
<section class="card match-section" id="open-games">{open_games}</section>"#
        ),
        &format!(
            r#"<script type="module" src="{}"></script>"#,
            asset("/public/settings.js")
        ),
        "dashboard-page",
        "",
    )
}

/// The standalone create-game page linked from the home screen.
pub fn new_game_page() -> String {
    layout(
        "New game — Scrabble",
        &format!(
            r#"<section class="card">
  <h1>New game</h1>
  {form}
</section>"#,
            form = new_game_form(),
        ),
    )
}

pub fn invite_page(id: Uuid) -> String {
    layout_with_back(
        "Invite a player — Scrabble",
        &format!(
            r#"<section class="card"><h1>Invite a player</h1>
<form class="form" data-invite-form="{id}" method="post" action="/games/{id}/invite"><input name="player" type="hidden">
<button class="button">Send invitation</button></form></section>"#
        ),
        "",
        "",
        "",
        BackTarget::Board(id),
    )
}

pub fn settings_page(
    profile: &crate::users::User,
    error: Option<(&str, &str)>,
    saved: Option<&str>,
) -> String {
    let settings = &profile.settings;
    let avatar = settings.avatar(profile.id);
    let handle = settings.player_handle.as_deref();
    let summary = handle
        .map(|h| format!("@{}", escape(h)))
        .unwrap_or_else(|| "Choose a username".into());
    let value = escape(error.map(|(input, _)| input).or(handle).unwrap_or_default());
    let opened = if error.is_some() { " open" } else { "" };
    let invalid = if error.is_some() {
        r#" aria-invalid="true" aria-describedby="handle-hint handle-error""#
    } else {
        r#" aria-describedby="handle-hint""#
    };
    let error_text = error
        .map(|(_, message)| {
            format!(
                r#"<p class="handle-error" id="handle-error" role="alert">{}</p>"#,
                escape(message)
            )
        })
        .unwrap_or_default();
    let feedback = saved
        .map(|message| {
            format!(
                r#"<p class="handle-saved" role="status">{}</p>"#,
                escape(message)
            )
        })
        .unwrap_or_default();
    let remove = if handle.is_some() {
        r#"<button class="button button-secondary" name="remove" value="1" formnovalidate>Remove username</button>"#
    } else {
        ""
    };

    use crate::users::AwayState;
    let away = settings.away_state(chrono::Utc::now());
    let away_state = away.token();
    let away_status = match away {
        AwayState::Off => String::new(),
        AwayState::Indefinite => "<p>Notifications are paused until you resume them.</p>".into(),
        AwayState::Until(until) => format!(
            r#"<p>Notifications paused until <time id="away-until" datetime="{}">{}</time>.</p>"#,
            until.to_rfc3339(),
            until.format("%b %e at %H:%M UTC")
        ),
    };
    let (duration_label, pause_action, resume) = if away.is_active() {
        (
            "Change pause to",
            "Update pause",
            r#"<form method="post" action="/settings/away"><button class="button button-secondary" name="hours" value="0">Resume now</button></form>"#,
        )
    } else {
        ("Pause for", "Pause notifications", "")
    };
    layout_with_head(
        "Settings — Scrabble",
        &format!(
            r#"<section class="card">
  <h1>Settings</h1>
  <details id="player-username" class="username-picker"{opened}>
    <summary><span>Scrabble username<small>{summary}</small></span><span class="avatar-change">Edit</span></summary>
    <form method="post" action="/settings/username#player-username" class="form">
      <label for="public-handle">Player username</label>
      <input id="public-handle" type="text" name="handle" value="{value}" placeholder="e.g. tilefox" autocomplete="off" autocapitalize="none" spellcheck="false" maxlength="64" required{invalid}>
      <p id="handle-hint" class="muted">Friends can find you by this name. Use 3–20 letters, numbers or underscores, starting with a letter or number.</p>
      {error_text}<div class="handle-actions"><button class="button">Save username</button>{remove}</div>
    </form>
  </details>
  {feedback}
  {icon_chooser}
  <fieldset class="appearance">
    <legend>Appearance</legend>
    <p>Choose a theme for this device.</p>
    <div class="theme-options">
      <label><input type="radio" name="appearance" value="system" checked><span>System</span></label>
      <label><input type="radio" name="appearance" value="light"><span>Light</span></label>
      <label><input type="radio" name="appearance" value="dark"><span>Dark</span></label>
    </div>
  </fieldset>
  <form method="post" action="/settings" class="form">
    <div class="form-option-row">
      <label class="checkbox-label" for="auto-zoom">
        <input id="auto-zoom" type="checkbox" name="auto_zoom" value="on"{checked} />
        <span>Auto-zoom</span>
      </label>
    </div>
    <button type="submit" class="button">Save</button>
  </form>
</section>
<section class="card">
  <h2>Notifications</h2>
  <p class="muted" id="notif-status">Checking…</p>
  <button type="button" id="notif-enable" class="button button-secondary" hidden>Enable notifications</button>
</section>
<section class="card away-settings" data-away-state="{away_state}">
  <h2>Away mode</h2>
  <p class="muted">Pause invitation and turn alerts on all your devices. Your games will keep updating.</p>
  {away_status}
  <form class="away-controls" method="post" action="/settings/away">
    <label for="away-hours">{duration_label}</label>
    <select id="away-hours" name="hours">
      <option value="indefinite">Indefinite</option><option value="1">1 hour</option><option value="4">4 hours</option><option value="8">8 hours</option>
      <option value="24">1 day</option><option value="48">2 days</option><option value="168">1 week</option>
    </select>
    <button class="button button-secondary">{pause_action}</button>
  </form>
  {resume}
</section>
<details class="card diagnostics"><summary>Diagnostics</summary>
  <div class="diagnostic-actions">
    <a class="button button-secondary" href="/debug/notifications">Notification diagnostics</a>
    <a class="button button-secondary" href="/debug/touch">Touch diagnostics</a>
  </div>
</details>"#,
            icon_chooser = crate::avatars::chooser(avatar),
            checked = if settings.auto_zoom { " checked" } else { "" },
        ),
        &format!(
            r#"<script type="module" src="{}" defer></script>"#,
            asset("/public/settings.js")
        ),
        "settings-page",
        "",
    )
}

pub fn notification_debug_page() -> String {
    layout_with_back(
        "Notification debug",
        r#"<section class="card notification-debug">
  <h1>Notification debug</h1>
  <p class="muted">Use this to separate browser permission, service worker, subscription storage, and server push delivery.</p>

  <div class="debug-grid">
    <section class="debug-panel">
      <h2>Browser</h2>
      <dl class="debug-status">
        <dt>Support</dt><dd id="debug-support">checking...</dd>
        <dt>Permission</dt><dd id="debug-permission">checking...</dd>
        <dt>Service worker</dt><dd id="debug-service-worker">checking...</dd>
        <dt>Push subscription</dt><dd id="debug-subscription">checking...</dd>
      </dl>
    </section>
    <section class="debug-panel">
      <h2>Server</h2>
      <dl class="debug-status">
        <dt>Web push</dt><dd id="debug-server-configured">checking...</dd>
        <dt>Stored subscriptions</dt><dd id="debug-server-subscriptions">checking...</dd>
        <dt>VAPID public key</dt><dd id="debug-server-key">checking...</dd>
      </dl>
    </section>
  </div>

  <div class="debug-actions">
    <button type="button" class="button" id="debug-enable">Enable/register</button>
    <button type="button" class="button button-secondary" id="debug-local-test">Local notification</button>
    <button type="button" class="button button-secondary" id="debug-server-test">Send server test</button>
    <button type="button" class="button ghost" id="debug-unsubscribe">Unsubscribe</button>
  </div>

  <pre class="debug-output" id="debug-output" aria-live="polite"></pre>
</section>"#,
        &format!(
            r#"<script type="module" src="{}" defer></script>"#,
            asset("/public/notification-debug.js")
        ),
        "",
        "",
        BackTarget::Settings,
    )
}

pub fn touch_debug_page() -> String {
    layout_with_back(
        "Touch debug",
        r#"<section class="touch-debug" aria-labelledby="touch-debug-title">
  <div class="touch-debug-board-panel">
    <h1 id="touch-debug-title">Touch debug</h1>
    <p class="touch-debug-turn">Your turn</p>
    <div id="touch-debug-board" class="board touch-debug-board" role="grid" aria-label="Debug board"></div>
    <div id="touch-debug-rack" class="rack touch-debug-rack" aria-label="Debug rack"></div>
  </div>

  <form id="touch-debug-controls" class="touch-debug-controls">
    <h2>Offsets</h2>
    <fieldset>
      <legend>Drop point</legend>
      <div class="touch-debug-offset" data-offset-key="dropX">
        <label for="touch-debug-drop-x">X <output id="touch-debug-drop-x-output" for="touch-debug-drop-x">0 px</output></label>
        <div class="touch-debug-slider-row">
          <button type="button" class="button button-secondary" data-nudge="-10" aria-label="Decrease drop X">-10</button>
          <input id="touch-debug-drop-x" type="range" min="-140" max="140" step="2" value="0">
          <button type="button" class="button button-secondary" data-nudge="10" aria-label="Increase drop X">+10</button>
        </div>
      </div>
      <div class="touch-debug-offset" data-offset-key="dropY">
        <label for="touch-debug-drop-y">Y <output id="touch-debug-drop-y-output" for="touch-debug-drop-y">0 px</output></label>
        <div class="touch-debug-slider-row">
          <button type="button" class="button button-secondary" data-nudge="-10" aria-label="Decrease drop Y">-10</button>
          <input id="touch-debug-drop-y" type="range" min="-140" max="140" step="2" value="0">
          <button type="button" class="button button-secondary" data-nudge="10" aria-label="Increase drop Y">+10</button>
        </div>
      </div>
    </fieldset>

    <fieldset>
      <legend>Dragged tile</legend>
      <div class="touch-debug-offset" data-offset-key="tileX">
        <label for="touch-debug-tile-x">X <output id="touch-debug-tile-x-output" for="touch-debug-tile-x">0 px</output></label>
        <div class="touch-debug-slider-row">
          <button type="button" class="button button-secondary" data-nudge="-10" aria-label="Decrease dragged tile X">-10</button>
          <input id="touch-debug-tile-x" type="range" min="-140" max="140" step="2" value="0">
          <button type="button" class="button button-secondary" data-nudge="10" aria-label="Increase dragged tile X">+10</button>
        </div>
      </div>
      <div class="touch-debug-offset" data-offset-key="tileY">
        <label for="touch-debug-tile-y">Y <output id="touch-debug-tile-y-output" for="touch-debug-tile-y">0 px</output></label>
        <div class="touch-debug-slider-row">
          <button type="button" class="button button-secondary" data-nudge="-10" aria-label="Decrease dragged tile Y">-10</button>
          <input id="touch-debug-tile-y" type="range" min="-140" max="140" step="2" value="0">
          <button type="button" class="button button-secondary" data-nudge="10" aria-label="Increase dragged tile Y">+10</button>
        </div>
      </div>
    </fieldset>

    <div class="touch-debug-presets">
      <button type="button" class="button" id="touch-debug-preset-proposed">Proposed</button>
      <button type="button" class="button button-secondary" id="touch-debug-preset-old">Old target</button>
      <button type="button" class="button ghost" id="touch-debug-reset-board">Reset tiles</button>
    </div>

    <dl class="touch-debug-readout">
      <dt>Touch</dt><dd id="touch-debug-touch-readout">-</dd>
      <dt>Dragged</dt><dd id="touch-debug-drag-readout">-</dd>
      <dt>Drop</dt><dd id="touch-debug-drop-readout">-</dd>
      <dt>Cell</dt><dd id="touch-debug-cell-readout">-</dd>
    </dl>
  </form>
</section>"#,
        &format!(
            r#"<script type="module" src="{}" defer></script>"#,
            asset("/public/touch-debug.js")
        ),
        "touch-debug-page",
        "",
        BackTarget::Settings,
    )
}

/// The signed-out landing panel: passkey register + sign-in forms, wired up by
/// `public/auth.js`.
fn signed_out_panel() -> String {
    let body = r#"<section class="card">
  <h1>Welcome to Scrabble</h1>
  <p class="muted">A classic Scrabble-style word game. Sign in with a passkey to play — no passwords.</p>
  <p id="auth-error" class="auth-error" role="alert" hidden></p>
  <div class="auth-grid">
    <form id="login-form" class="form auth-form">
      <h2>Sign in</h2>
      <label>Username
        <input type="text" name="username" autocomplete="username webauthn" maxlength="32" required>
      </label>
      <button type="submit" class="button">Sign in with passkey</button>
    </form>
    <form id="register-form" class="form auth-form">
      <h2>Create account</h2>
      <label>Username
        <input type="text" name="username" autocomplete="username" maxlength="32" required>
      </label>
      <label>Display name <span class="muted">(optional)</span>
        <input type="text" name="display_name" maxlength="48">
      </label>
      <button type="submit" class="button">Register a passkey</button>
    </form>
  </div>
</section>"#;
    body.to_string()
}

fn new_game_form() -> String {
    let options = |selected_off: bool| {
        let off = if selected_off { " selected" } else { "" };
        format!(
            r#"<option value="off"{off}>— none —</option>
<option value="invite">Invite a player…</option>
<option value="open">Open seat (human)</option>
<option value="easy">Easy bot</option>
<option value="chill">Chill bot</option>
<option value="medium">Medium bot</option>
<option value="hard">Hard bot</option>
<option value="impossible">Impossible bot</option>"#
        )
    };
    format!(
        r#"<form method="post" action="/games" class="form new-game-form">
  <label>Seat 2
    <select name="seat2">{seat2}</select>
  </label>
  <label>Seat 3
    <select name="seat3">{seat3}</select>
  </label>
  <label>Seat 4
    <select name="seat4">{seat4}</select>
  </label>
  <details class="game-options"><summary>Game options</summary>
  <div class="form-options">
    <div class="form-option-row">
      <label class="checkbox-label" for="john-mode">
        <input id="john-mode" type="checkbox" name="john_mode" value="on" />
        <span>John Mode</span>
      </label>
      <span class="info-tooltip" tabindex="0" aria-describedby="john-mode-help">i
        <span id="john-mode-help" class="tooltip-content" role="tooltip">Show valid 2-letter words.</span>
      </span>
    </div>
    <div class="form-option-row">
      <label class="checkbox-label" for="grandpa-mode">
        <input id="grandpa-mode" type="checkbox" name="grandpa_mode" value="on" />
        <span>Grandpa Mode</span>
      </label>
      <span class="info-tooltip" tabindex="0" aria-describedby="grandpa-mode-help">i
        <span id="grandpa-mode-help" class="tooltip-content" role="tooltip">Disallow obscure 2-letter words; keep common ones like at, in, go, to.</span>
      </span>
    </div>
    <div class="form-option-row">
      <label class="checkbox-label" for="jax-mode">
        <input id="jax-mode" type="checkbox" name="jax_mode" value="on" />
        <span>Jax Mode</span>
      </label>
      <span class="info-tooltip" tabindex="0" aria-describedby="jax-mode-help">i
        <span id="jax-mode-help" class="tooltip-content" role="tooltip">Common names are valid words.</span>
      </span>
    </div>
    <div class="form-option-row">
      <label class="checkbox-label" for="shelli-mode">
        <input id="shelli-mode" type="checkbox" name="shelli_mode" value="on" />
        <span>Shelli Mode</span>
      </label>
      <span class="info-tooltip" tabindex="0" aria-describedby="shelli-mode-help">i
        <span id="shelli-mode-help" class="tooltip-content" role="tooltip">Bots may only use common 2-letter words; humans get the full list.</span>
      </span>
    </div>
    <div class="form-option-row">
      <label class="checkbox-label" for="scott-mode">
        <input id="scott-mode" type="checkbox" name="scott_mode" value="on" />
        <span>Scott Mode</span>
      </label>
      <span class="info-tooltip" tabindex="0" aria-describedby="scott-mode-help">i
        <span id="scott-mode-help" class="tooltip-content" role="tooltip">After each play, the move log shows the best word you could have made.</span>
      </span>
    </div>
    <div class="form-option-row">
      <label class="checkbox-label" for="august-mode">
        <input id="august-mode" type="checkbox" name="august_mode" value="on" />
        <span>August Mode</span>
      </label>
      <span class="info-tooltip" tabindex="0" aria-describedby="august-mode-help">i
        <span id="august-mode-help" class="tooltip-content" role="tooltip">Every tile is a letter from AUGUST (no blanks).</span>
      </span>
    </div>
    <div class="form-option-row">
      <label class="checkbox-label" for="mimi-mode">
        <input id="mimi-mode" type="checkbox" name="mimi_mode" value="on" />
        <span>Mimi Mode</span>
      </label>
      <span class="info-tooltip" tabindex="0" aria-describedby="mimi-mode-help">i
        <span id="mimi-mode-help" class="tooltip-content" role="tooltip">Reclaim a blank from the board by playing its real letter from your rack through it.</span>
      </span>
    </div>
    <label class="hints-label">Hints per player
      <select name="hints">
        <option value="0" selected>None</option>
        <option value="1">1</option>
        <option value="2">2</option>
        <option value="3">3</option>
      </select>
    </label>
  </div>
  </details>
  <button type="submit" class="button">Create game</button>
</form>"#,
        seat2 = options(false).replace(
            "<option value=\"invite\">Invite a player…</option>",
            "<option value=\"invite\" selected>Invite a player…</option>"
        ),
        seat3 = options(true),
        seat4 = options(true),
    )
}

fn game_list(games: &[&Game], current: Uuid, names: &crate::users::PlayerNames) -> String {
    let mut active_games: Vec<&Game> = games
        .iter()
        .copied()
        .filter(|game| game.status != GameStatus::Finished)
        .collect();
    active_games.sort_by(|a, b| {
        is_current_turn(b, current)
            .cmp(&is_current_turn(a, current))
            .then(effective_updated_at(b).cmp(&effective_updated_at(a)))
    });

    let mut active = String::new();
    for game in active_games {
        active.push_str(&game_list_item(game, current, names));
    }

    let mut finished_games: Vec<&Game> = games
        .iter()
        .copied()
        .filter(|game| game.status == GameStatus::Finished)
        .collect();
    finished_games.sort_by_key(|game| std::cmp::Reverse(effective_updated_at(game)));

    let mut finished = String::new();
    for game in finished_games {
        finished.push_str(&game_list_item(game, current, names));
    }

    let mut html = format!("<ul class=\"game-list\">{active}</ul>");
    if !finished.is_empty() {
        html.push_str(&format!(
            r#"<details class="finished-games">
  <summary class="game-list-divider"><span>Finished games</span></summary>
  <ul class="game-list finished-game-list">{finished}</ul>
</details>"#,
        ));
    }
    html
}

fn game_list_item(game: &Game, current: Uuid, names: &crate::users::PlayerNames) -> String {
    let your_turn = is_current_turn(game, current);
    let state = match game.status {
        GameStatus::Finished => "Final score".to_string(),
        GameStatus::Lobby => "Waiting for players".to_string(),
        GameStatus::Active if your_turn => "Your turn".to_string(),
        GameStatus::Active => match game.seats.get(game.turn) {
            Some(seat) if !matches!(seat.kind, SeatKind::Human { user_id: None }) => {
                format!("{}’s turn", escape(&names.seat(seat)))
            }
            _ => "Waiting for a player".to_string(),
        },
    };
    let mut seats = String::new();
    for seat in &game.seats {
        let name = if matches!(seat.kind, SeatKind::Human { user_id: Some(id) } if id == current) {
            "You".to_string()
        } else {
            escape(&names.seat(seat))
        };
        let score = if matches!(seat.kind, SeatKind::Human { user_id: None }) {
            String::new()
        } else {
            format!(
                r#"<strong class="match-score">{}<span class="match-sr"> points</span></strong>"#,
                seat.score
            )
        };
        seats.push_str(&format!(
            r#"<div class="match-player"><span>{name}</span> {score}</div>"#
        ));
    }
    format!(
        r#"<li class="game-list-item"><a class="match-card" href="/games/{id}">
  <div class="match-meta"><span class="match-status{turn_class}">{state}</span> <time class="game-list-time" datetime="{updated}">{time}</time></div>
  <div class="match-lineup">{seats}</div>
</a></li>"#,
        id = game.id,
        updated = effective_updated_at(game).to_rfc3339(),
        time = relative_time(effective_updated_at(game)),
        turn_class = if your_turn { " your-turn" } else { "" },
    )
}

fn open_game_list(games: &[&Game], names: &crate::users::PlayerNames) -> String {
    let mut games = games.to_vec();
    games.sort_by_key(|game| std::cmp::Reverse(effective_updated_at(game)));
    let rows: String = games
        .into_iter()
        .map(|g| open_game_list_item(g, names))
        .collect();
    format!(r#"<ul class="game-list open-game-list">{rows}</ul>"#)
}

fn open_game_list_item(game: &Game, names: &crate::users::PlayerNames) -> String {
    let players: Vec<_> = game
        .seats
        .iter()
        .filter(|s| !matches!(s.kind, SeatKind::Human { user_id: None }))
        .map(|s| escape(&names.seat(s)))
        .collect();
    let count = game.seats.iter().filter(|s| s.is_open()).count();
    let label = if count == 1 {
        "open seat"
    } else {
        "open seats"
    };
    format!(
        r#"<li class="open-match"><a class="open-match-info" href="/games/{id}"><strong>{players}</strong> <span>{count} {label}</span></a>
  <form class="inline-join-form" method="post" action="/games/{id}/join">
    <button type="submit" class="button button-secondary">Join</button>
  </form></li>"#,
        id = game.id,
        players = players.join(" · "),
    )
}

/// A coarse human-readable age for a game's last activity, e.g. "3h ago".
fn relative_time(then: DateTime<Utc>) -> String {
    let seconds = (Utc::now() - then).num_seconds().max(0);
    match seconds {
        0..=59 => "just now".to_string(),
        60..=3599 => format!("{}m ago", seconds / 60),
        3600..=86399 => format!("{}h ago", seconds / 3600),
        _ => format!("{}d ago", seconds / 86400),
    }
}

pub(crate) fn is_current_turn(game: &Game, current: Uuid) -> bool {
    if game.status != GameStatus::Active {
        return false;
    }
    game.seats.get(game.turn).is_some_and(
        |seat| matches!(seat.kind, SeatKind::Human { user_id } if user_id == Some(current)),
    )
}

pub fn game_page(
    view: &GameView,
    initial_json: &str,
    two_letter_json: &str,
    grandpa_two_letter_json: &str,
    logged_in: bool,
    auto_zoom: bool,
) -> String {
    let board = render_board_squares(&view.board, &view.last_play);
    let scoreboard = render_scoreboard(view);
    let log = render_move_log(view);
    let status_banner = render_status_banner(view);
    let join = render_join_form(view);
    // Neutralize any "</..." sequence (e.g. a player name containing "</script>")
    // so embedded JSON can't break out of the surrounding <script> element.
    let initial_json = initial_json.replace("</", "<\\/");
    let initial_json = initial_json.as_str();
    let head = format!(
        r#"<script type="module" src="{}" defer></script>"#,
        asset("/public/game.js")
    );
    let head = head.as_str();
    let mut nav_actions = if view.can_invite {
        format!(
            r#"<a class="invite-game-link" href="/games/{}/invite">Invite player</a>"#,
            view.id
        )
    } else {
        String::new()
    };
    if view.your_seat.is_some()
        && view
            .seats
            .iter()
            .filter(|seat| seat.kind == "human")
            .count()
            > 1
    {
        nav_actions.push_str(&format!(
            r#"<a class="game-chat-link" href="/games/{}/chat" aria-label="Chat" title="Game chat">
<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" focusable="false"><path d="M20.5 14.5a3 3 0 0 1-3 3h-8l-5 3v-4a3 3 0 0 1-1.5-2.6V6.5a3 3 0 0 1 3-3h11.5a3 3 0 0 1 3 3z"/><path d="M7.5 8.5h8.5m-8.5 4h5.5"/></svg>
</a><span class="match-sr chat-unread-status" role="status"></span>"#,
            view.id
        ));
    }
    let sign_in = if logged_in {
        String::new()
    } else {
        r#"<section class="card login-cta">
  <p class="muted">Sign in to join an open seat and play.</p>
  <a class="button" href="/">Sign in to play</a>
</section>"#
            .to_string()
    };
    let body = format!(
        r#"{sign_in}
<section class="card">
  <div id="ssr-fallback">
    {status_banner}
    <div class="game-layout">
      <div class="board-wrap">
        {board}
        {join}
      </div>
      <aside class="sidebar">
        {scoreboard}
        {log}
      </aside>
    </div>
    <noscript><p class="muted">Enable JavaScript to place tiles and play.</p></noscript>
  </div>
  <div id="game-island" data-game-id="{id}" data-auto-zoom="{auto_zoom}"></div>
  <script id="game-state" type="application/json">{initial_json}</script>
  <script id="two-letter-words" type="application/json">{two_letter_json}</script>
  <script id="grandpa-two-letter-words" type="application/json">{grandpa_two_letter_json}</script>
</section>"#,
        id = view.id,
    );
    // Raw names: `layout_with_head` escapes the title itself.
    let names: Vec<String> = view.seats.iter().map(|seat| seat.name.clone()).collect();
    let title = if names.is_empty() {
        "Game — Scrabble".to_string()
    } else {
        format!("{} — Scrabble", names.join(" vs "))
    };
    layout_with_head(&title, &body, head, "game-page", &nav_actions)
}

pub fn chat_page(id: Uuid, viewer: Uuid, participants: &str) -> String {
    let head = format!(
        r#"<link rel="stylesheet" href="{}"><script type="module" src="{}"></script>"#,
        asset("/public/chat.css"),
        asset("/public/chat.js")
    );
    let body = format!(
        r#"<section class="game-chat" data-chat-game="{id}" data-chat-viewer="{viewer}" aria-labelledby="chat-title">
  <header class="chat-heading"><h1 id="chat-title">Game chat</h1><p>{names}</p></header>
  <div class="chat-history" role="region" aria-label="Conversation" tabindex="0">
    <button type="button" class="chat-older" hidden>Earlier messages</button>
    <p class="chat-intro">Loading messages…</p>
    <ol class="chat-messages" aria-label="Messages" aria-live="polite" aria-relevant="additions"></ol>
  </div>
  <form class="chat-form">
    <p class="chat-status" role="status" aria-live="polite"></p>
    <label class="chat-label" for="chat-message">Message</label>
    <textarea rows="1" id="chat-message" maxlength="500" placeholder="Message the players…" required></textarea>
    <button type="submit">Send</button>
  </form>
</section>"#,
        names = escape(participants)
    );
    layout_with_back(
        "Game chat — Scrabble",
        &body,
        &head,
        "chat-page",
        "",
        BackTarget::Board(id),
    )
}

fn render_join_form(view: &GameView) -> String {
    let open_seat = view.seats.iter().any(|seat| seat.open);
    if view.your_seat.is_some() || !open_seat {
        return String::new();
    }
    format!(
        r#"<form class="join-form" method="post" action="/games/{id}/join">
  <p class="muted">An open seat is waiting. Join to play.</p>
  <button type="submit" class="button">Join game</button>
</form>"#,
        id = view.id,
    )
}

fn render_status_banner(view: &GameView) -> String {
    let text = match view.status {
        GameStatus::Lobby => "Waiting to start".to_string(),
        GameStatus::Active => {
            let seat = view.seats.get(view.turn);
            match seat {
                Some(seat) if seat.is_you => "Your turn".to_string(),
                Some(seat) => format!("{}'s turn", escape(&seat.name)),
                None => "In progress".to_string(),
            }
        }
        GameStatus::Finished => {
            let names: Vec<String> = view
                .winners
                .iter()
                .filter_map(|&i| view.seats.get(i))
                .map(|seat| escape(&seat.name))
                .collect();
            if names.is_empty() {
                "Game over".to_string()
            } else {
                format!("Game over — winner: {}", names.join(", "))
            }
        }
    };
    format!(r#"<h1 class="status">{text}</h1>"#)
}

fn render_scoreboard(view: &GameView) -> String {
    let top_score = view.seats.iter().map(|seat| seat.score).max().unwrap_or(0);
    let rows: String = view
        .seats
        .iter()
        .map(|seat| {
            let turn = if seat.on_turn { " on-turn" } else { "" };
            let you = if seat.is_you {
                " <span class=\"badge\">you</span>"
            } else {
                ""
            };
            let kind = if seat.open {
                "open".to_string()
            } else if let Some(difficulty) = seat.difficulty {
                format!("{difficulty:?} bot")
            } else {
                "human".to_string()
            };
            let hints = match seat.hints_remaining {
                Some(n) => {
                    format!(r#" <span class="hint-count" title="hints left">💡{n}</span>"#)
                }
                None => String::new(),
            };
            format!(
                r#"<tr class="seat{turn}">
  <td>{name}{you}{hints}</td>
  <td class="muted">{kind}</td>
  <td class="{score_class}">{score}</td>
  <td class="score muted">{avg}</td>
</tr>"#,
                name = escape(&seat.name),
                score = seat.score,
                score_class = if top_score > 0 && seat.score == top_score {
                    "score leader"
                } else {
                    "score"
                },
                avg = points_per_play(seat.score, seat.play_count),
            )
        })
        .collect();
    format!(
        r#"<table class="scoreboard">
  <thead><tr><th>Player</th><th>Type</th><th>Score</th><th title="Points per play">Pts/Play</th></tr></thead>
  <tbody>{rows}</tbody>
</table>
<p class="muted">Tiles in bag: {bag}</p>"#,
        bag = view.bag_count,
    )
}

/// Average points per scoring play, formatted to one decimal; a dash when the
/// seat has not played yet.
fn points_per_play(score: i32, play_count: usize) -> String {
    if play_count == 0 {
        "–".to_string()
    } else {
        format!("{:.1}", score as f64 / play_count as f64)
    }
}

fn render_move_log(view: &GameView) -> String {
    if view.moves.is_empty() {
        return "<p class=\"muted\">No moves yet.</p>".to_string();
    }
    let items: String = view
        .moves
        .iter()
        .rev()
        .take(10)
        .map(|mv| {
            let name = view
                .seats
                .get(mv.seat)
                .map(|s| escape(&s.name))
                .unwrap_or_default();
            let best = match (mv.kind, &mv.best) {
                ("play", Some(best)) => format!(
                    r#" <span class="best-play muted">best: {} (+{})</span>"#,
                    best.words.join(", "),
                    best.points
                ),
                _ => String::new(),
            };
            let detail = match mv.kind {
                "play" => format!("{} (+{}){best}", mv.words.join(", "), mv.points),
                "exchange" => "exchanged tiles".to_string(),
                "adjustment" if mv.delta >= 0 => {
                    format!("out bonus (+{})", mv.delta)
                }
                "adjustment" => {
                    format!("leftover {} ({})", mv.words.join(""), mv.delta)
                }
                _ => "passed".to_string(),
            };
            format!("<li><strong>{name}</strong>: {detail}</li>")
        })
        .collect();
    format!("<ul class=\"move-log\">{items}</ul>")
}

fn render_board_squares(squares: &[SquareView], last_play: &[PositionView]) -> String {
    let mut cells = String::new();
    for (index, square) in squares.iter().enumerate() {
        let row = index / BOARD_SIZE;
        let col = index % BOARD_SIZE;
        match square.letter {
            Some(letter) => {
                let blank = if square.is_blank { " tile-blank" } else { "" };
                // Match the client, which rings the most recent play's tiles.
                let last = if last_play.iter().any(|p| p.row == row && p.col == col) {
                    " last-play"
                } else {
                    ""
                };
                let points = crate::models::letter_points(letter);
                let shown = if square.is_blank { 0 } else { points };
                cells.push_str(&format!(
                    r#"<div class="cell tile{blank}{last}" data-row="{row}" data-col="{col}"><span class="tile-letter">{letter}</span><span class="tile-points">{shown}</span></div>"#,
                ));
            }
            None => {
                let class = format!("premium-{}", square.premium);
                let center = row == BOARD_SIZE / 2 && col == BOARD_SIZE / 2;
                let label = if center {
                    "★"
                } else {
                    premium_label(square.premium)
                };
                cells.push_str(&format!(
                    r#"<div class="cell {class}" data-row="{row}" data-col="{col}"><span class="premium-label">{label}</span></div>"#,
                ));
            }
        }
    }
    format!(r#"<div class="board" role="grid">{cells}</div>"#)
}

fn premium_label(code: &str) -> &'static str {
    match code {
        "dl" => "DL",
        "tl" => "TL",
        "dw" => "DW",
        "tw" => "TW",
        _ => "",
    }
}

/// Low-level board rendering helper used by tests.
pub fn render_board(board: &Board) -> String {
    let squares: Vec<SquareView> = board
        .squares
        .iter()
        .map(|square| SquareView {
            premium: premium_code(square.premium),
            letter: square.tile.map(|t| t.letter),
            is_blank: square.tile.is_some_and(|t| t.is_blank),
        })
        .collect();
    render_board_squares(&squares, &[])
}
