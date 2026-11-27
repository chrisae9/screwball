use uuid::Uuid;

use super::{HomeActivity, asset, asset_version, escape, is_current_turn, relative_time};
use crate::{
    avatars,
    models::{Game, GameStatus, MoveKind, Seat, SeatKind},
    users::PlayerNames,
    view::{effective_updated_at, winners},
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum LobbyView {
    Games,
    History,
}

struct PlayerDisplay<'a> {
    user: Uuid,
    names: &'a PlayerNames,
}

impl PlayerDisplay<'_> {
    fn avatar(&self, seat: &Seat) -> String {
        avatars::image(self.names.avatar_for_seat(seat))
    }

    fn mine(&self, seat: &Seat) -> bool {
        matches!(seat.kind, SeatKind::Human { user_id: Some(id) } if id == self.user)
    }
}

pub fn lobby_activity(
    view: LobbyView,
    games: &[&Game],
    open: &[&Game],
    current: Uuid,
    names: &PlayerNames,
    invitations: String,
    away: bool,
) -> HomeActivity {
    let players = PlayerDisplay {
        user: current,
        names,
    };
    let mut groups: [Vec<String>; 3] = Default::default();
    let mut mine = games.to_vec();
    mine.sort_by_key(|g| (std::cmp::Reverse(effective_updated_at(g)), g.id));
    for game in mine {
        if (game.status == GameStatus::Finished) != (view == LobbyView::History) {
            continue;
        }
        let group = if view == LobbyView::History || is_current_turn(game, current) {
            0
        } else if game.status == GameStatus::Lobby
            || game
                .seats
                .get(game.turn)
                .is_some_and(|s| matches!(s.kind, SeatKind::Human { user_id: None }))
        {
            2
        } else {
            1
        };
        if let Some(row) = match_row(game, &players) {
            groups[group].push(row);
        }
    }
    let mut open_games = String::new();
    let turns = games.iter().filter(|g| is_current_turn(g, current)).count();
    let games = if view == LobbyView::History {
        section("finished", "Finished games", &groups[0])
    } else {
        let mut open = open.to_vec();
        open.sort_by_key(|g| (std::cmp::Reverse(effective_updated_at(g)), g.id));
        let rows = open
            .into_iter()
            .filter_map(|g| open_row(g, &players))
            .collect::<Vec<_>>();
        open_games = section("open", "Open to join", &rows);
        [
            ("turn", "Your turn"),
            ("waiting", "Waiting for a turn"),
            ("lobby", "Waiting for players"),
        ]
        .iter()
        .zip(&groups)
        .map(|((key, label), rows)| section(key, label, rows))
        .collect()
    };
    HomeActivity {
        version: asset_version(),
        away,
        turns,
        games,
        open_games,
        invitations,
    }
}

fn section(key: &str, label: &str, rows: &[String]) -> String {
    if rows.is_empty() {
        return String::new();
    }
    format!(
        r#"<section class="lobby-section" data-group="{key}"><h2>{label} <span class="lobby-count">{}</span></h2><ul class="lobby-list">{}</ul></section>"#,
        rows.len(),
        rows.concat()
    )
}

fn last_move(game: &Game, players: &PlayerDisplay<'_>) -> String {
    let Some(mv) = game
        .moves
        .iter()
        .rev()
        .find(|m| !matches!(m.kind, MoveKind::EndAdjustment { .. }))
    else {
        return "Game started".into();
    };
    let Some(seat) = game.seats.get(mv.seat) else {
        return "Game updated".into();
    };
    let who = if players.mine(seat) {
        "You".into()
    } else {
        escape(&players.names.seat(seat))
    };
    match &mv.kind {
        MoveKind::Play { .. } => format!(
            "{who} played <b>{}</b> +{}",
            escape(&mv.words.join(", ")),
            mv.points
        ),
        MoveKind::Exchange { .. } => format!("{who} swapped tiles"),
        MoveKind::Pass => format!("{who} passed"),
        MoveKind::EndAdjustment { .. } => unreachable!(),
    }
}

fn match_row(game: &Game, p: &PlayerDisplay<'_>) -> Option<String> {
    let (index, own) = game
        .seats
        .iter()
        .enumerate()
        .find(|(_, seat)| p.mine(seat))?;
    let opponents: Vec<_> = game.seats.iter().filter(|s| !p.mine(s)).collect();
    let first = opponents.first()?;
    let names = opponents
        .iter()
        .map(|s| p.names.seat(s))
        .collect::<Vec<_>>()
        .join(" & ");
    let mut search = game
        .seats
        .iter()
        .map(|s| format!("{} {}", p.names.seat(s), s.name))
        .collect::<Vec<_>>()
        .join(" ");
    for mv in &game.moves {
        for word in &mv.words {
            search.push(' ');
            search.push_str(word);
        }
    }
    let time = effective_updated_at(game);
    let mut result = "";
    let meta = if game.status == GameStatus::Finished {
        let leaders = winners(game);
        result = if !crate::leaderboard::has_final_scores(game) {
            "Ended"
        } else if !leaders.contains(&index) {
            "Lost"
        } else if leaders.len() > 1 {
            "Tied"
        } else {
            "Won"
        };
        format!(
            r#"<span class="lobby-outcome" data-result="{result}">{result}</span> <time datetime="{}">{}</time>"#,
            time.to_rfc3339(),
            time.format("%b %-d, %Y")
        )
    } else {
        format!(
            r#"{} · <time datetime="{}">{}</time>"#,
            last_move(game, p),
            time.to_rfc3339(),
            relative_time(time)
        )
    };
    let score = if opponents.len() == 1 && !matches!(first.kind, SeatKind::Human { user_id: None })
    {
        format!(
            r#"<strong>{} – {}</strong><small>You / {}</small>"#,
            own.score,
            first.score,
            escape(&p.names.seat(first))
        )
    } else {
        format!(r#"<strong>{}</strong><small>Your score</small>"#, own.score)
    };
    let closest = opponents
        .iter()
        .map(|seat| own.score.abs_diff(seat.score))
        .min()
        .unwrap_or_default();
    Some(format!(
        r#"<li data-search="{}" data-result="{result}" data-time="{}" data-bot="{}" data-score="{}" data-gap="{closest}">
<a class="lobby-row" href="/games/{}">{}<span class="lobby-info"><span class="lobby-name">{}</span><span class="lobby-meta">{meta}</span></span><span class="lobby-scores">{score}</span><span class="lobby-chevron" aria-hidden="true">›</span></a></li>"#,
        escape(&search),
        time.timestamp(),
        opponents.iter().any(|s| s.is_bot()),
        own.score,
        game.id,
        p.avatar(first),
        escape(&names)
    ))
}

fn open_row(game: &Game, p: &PlayerDisplay<'_>) -> Option<String> {
    let host = game.seats.first()?;
    let names = game
        .seats
        .iter()
        .filter(|s| !matches!(s.kind, SeatKind::Human { user_id: None }))
        .map(|s| p.names.seat(s))
        .collect::<Vec<_>>()
        .join(" & ");
    let count = game.seats.iter().filter(|s| s.is_open()).count();
    let seats = if count == 1 {
        "open seat"
    } else {
        "open seats"
    };
    Some(format!(
        r#"<li class="open-match" data-search="{names}"><a class="lobby-row" href="/games/{id}">{avatar}<span class="lobby-info"><span class="lobby-name">{names}</span><span class="lobby-meta">{count} {seats}</span></span></a><form method="post" action="/games/{id}/join"><button class="lobby-button">Join</button></form></li>"#,
        names = escape(&names),
        avatar = p.avatar(host),
        id = game.id
    ))
}

pub fn history_page(activity: HomeActivity) -> String {
    page(activity, LobbyView::History)
}

pub(super) fn page(activity: HomeActivity, view: LobbyView) -> String {
    let history = view == LobbyView::History;
    let title = if history { "History" } else { "Games" };
    let search = if history {
        "Search players or words"
    } else {
        "Find a game or player"
    };
    let new_game = if history {
        ""
    } else {
        r#"<a href="/games/new" class="lobby-button lobby-primary new-game-link"><span aria-hidden="true">＋</span> New game</a>"#
    };
    let filter_button = if history {
        r#"<button type="button" class="lobby-button" id="lobby-filters" aria-controls="lobby-options" aria-expanded="false"><svg viewBox="0 0 24 24" aria-hidden="true"><path d="M3 6h6m4 0h8M3 12h12m4 0h2M3 18h2m4 0h12"/><circle cx="11" cy="6" r="2"/><circle cx="17" cy="12" r="2"/><circle cx="7" cy="18" r="2"/></svg><span>Filters</span></button>"#
    } else {
        ""
    };
    let tabs: &[(&str, &str)] = if history {
        &[
            ("all", "All"),
            ("Won", "Won"),
            ("Lost", "Lost"),
            ("Tied", "Tied"),
        ]
    } else {
        &[
            ("mine", "My games"),
            ("turn", "Your turn"),
            ("open", "Open games"),
        ]
    };
    let tabs = tabs
        .iter()
        .enumerate()
        .map(|(i, (key, label))| {
            format!(
                r#"<button type="button" data-tab="{key}" aria-pressed="{}">{label}</button>"#,
                i == 0
            )
        })
        .collect::<String>();
    let options = if history {
        r#"<div id="lobby-options" hidden>
<label>Played<select name="period"><option value="0">Any time</option><option value="7">Last 7 days</option><option value="30">Last 30 days</option><option value="90">Last 90 days</option></select></label>
<label>Opponent<select name="kind"><option value="all">Everyone</option><option value="people">People</option><option value="bots">Bots</option></select></label>
<label>Sort by<select name="sort"><option value="newest">Newest first</option><option value="oldest">Oldest first</option><option value="score">Highest score</option><option value="closest">Closest score</option></select></label>
<button type="button" class="lobby-reset">Reset filters</button></div>"#
    } else {
        ""
    };
    let empty_msg = if history {
        "No finished games yet."
    } else {
        "No games yet."
    };
    let empty_hidden = if activity.games.is_empty() && (history || activity.invitations.is_empty())
    {
        ""
    } else {
        " hidden"
    };
    let body = format!(
        r#"<div class="lobby" data-view="{title}">
<div class="lobby-head"><h1>{title}</h1>{new_game}</div>
<div class="lobby-controls"><label class="lobby-search"><svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="10.5" cy="10.5" r="6.5"/><path d="m16 16 5 5"/></svg><input type="search" autocomplete="off" enterkeyhint="search" aria-label="Search {title}" placeholder="{search}"></label>{filter_button}</div>
<div class="lobby-tabs" role="group" aria-label="{title} views">{tabs}</div>{options}
<div id="invitations" tabindex="-1"{inbox_hidden}>{invitations}</div>
<div id="my-games" tabindex="-1" data-dashboard-version="{version}">{games}</div>
<div id="open-games" hidden>{open}</div>
<div class="lobby-empty"{empty_hidden}><h2>{empty_msg}</h2><button class="lobby-reset" type="button" hidden>Clear search and filters</button></div>
<p class="match-sr" role="status" id="lobby-count"></p></div>"#,
        inbox_hidden = if history { " hidden" } else { "" },
        invitations = if history { "" } else { &activity.invitations },
        version = activity.version,
        games = activity.games,
        open = activity.open_games
    );
    let head = format!(
        r#"<link rel="stylesheet" href="{}"><script type="module" src="{}"></script>"#,
        asset("/public/lobby.css"),
        asset("/public/lobby.js")
    );
    super::layout_with_head(
        if history {
            "History — Scrabble"
        } else {
            "Scrabble"
        },
        &body,
        &head,
        if history {
            "dashboard-page lobby-page history-page"
        } else {
            "dashboard-page lobby-page"
        },
        "",
    )
}
