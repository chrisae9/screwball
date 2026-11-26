use std::{collections::HashMap, fmt::Write};

use axum::{extract::State, response::Html};
use uuid::Uuid;

use crate::{
    app::AppState,
    models::{Game, GameStatus, MoveKind, SCORELESS_LIMIT, SeatKind},
    render::{escape, layout},
    session::AuthUser,
};

#[derive(Default)]
struct Standing {
    name: String,
    played: usize,
    wins: usize,
    draws: usize,
}

impl Standing {
    fn points(&self) -> usize {
        2 * self.wins + self.draws
    }
}

fn ranked_game(game: &Game) -> bool {
    if game.status != GameStatus::Finished || game.seats.len() < 2 {
        return false;
    }
    // Abandonment also sets Finished, but never settles scores. In games where
    // every adjustment was zero, the engine does not add settlement moves.
    let settled = game.consecutive_scoreless >= SCORELESS_LIMIT
        || (game.bag.is_empty() && game.seats.iter().any(|seat| seat.rack.is_empty()))
        || game
            .moves
            .iter()
            .any(|m| matches!(m.kind, MoveKind::EndAdjustment { .. }));
    settled
        && game
            .seats
            .iter()
            .all(|s| matches!(s.kind, SeatKind::Human { user_id: Some(_) }))
        && game
            .moves
            .iter()
            .any(|m| matches!(m.kind, MoveKind::Play { .. }))
}

fn standings<'a>(games: impl Iterator<Item = &'a Game>) -> Vec<(Uuid, Standing)> {
    let mut players = HashMap::<Uuid, Standing>::new();
    for game in games.filter(|g| ranked_game(g)) {
        let leaders = crate::view::winners(game);
        for (index, seat) in game.seats.iter().enumerate() {
            if let SeatKind::Human { user_id: Some(id) } = seat.kind {
                let record = players.entry(id).or_insert_with(|| Standing {
                    name: seat.name.clone(),
                    ..Standing::default()
                });
                record.played += 1;
                if leaders.contains(&index) {
                    if leaders.len() == 1 {
                        record.wins += 1;
                    } else {
                        record.draws += 1;
                    }
                }
            }
        }
    }
    let mut players: Vec<_> = players.into_iter().collect();
    players
        .sort_by_cached_key(|(id, p)| (std::cmp::Reverse(p.points()), p.name.to_lowercase(), *id));
    players
}

pub async fn page(State(state): State<AppState>, AuthUser(viewer): AuthUser) -> Html<String> {
    let snapshots = state.store.snapshots();
    Html(render(
        &standings(snapshots.iter().map(AsRef::as_ref)),
        viewer,
    ))
}

fn render(players: &[(Uuid, Standing)], viewer: Uuid) -> String {
    let mut body = String::from(
        r#"<section class="card leaderboard"><h1>Leaderboard</h1>
<p class="leaderboard-rules">A win earns 2 points. A shared first place earns 1. Rankings include all completed human-only games with at least one word played.</p>"#,
    );
    if players.is_empty() {
        body.push_str(r#"<div class="leaderboard-empty"><h2>No ranked games yet</h2><p>Finish a game with another player to appear here.</p><a class="button" href="/games/new">Start a game</a></div>"#);
    } else {
        body.push_str(r#"<table aria-label="All-time player standings"><thead><tr><th scope="col">Rank</th><th scope="col">Player</th><th scope="col">Points</th></tr></thead><tbody>"#);
        let mut last_points = None;
        let mut rank = 0;
        for (index, (id, record)) in players.iter().enumerate() {
            let points = record.points();
            if last_points != Some(points) {
                rank = index + 1;
                last_points = Some(points);
            }
            let you = if *id == viewer { " (you)" } else { "" };
            let name = escape(&record.name);
            let games = record.played;
            let wins = record.wins;
            let draws = record.draws;
            let game_label = if games == 1 { "game" } else { "games" };
            let win_label = if wins == 1 { "win" } else { "wins" };
            let draw_label = if draws == 1 { "tie" } else { "ties" };
            write!(body, r#"<tr><td class="leaderboard-rank">{rank}</td><th scope="row">{name}{you}<span class="leaderboard-record">{games} {game_label} · {wins} {win_label} · {draws} {draw_label}</span></th><td class="leaderboard-points">{points}</td></tr>"#).unwrap();
        }
        body.push_str("</tbody></table>");
    }
    body.push_str("</section>");
    layout("Leaderboard", &body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        game::{GameOptions, SeatSpec, abandon, new_game},
        models::{Difficulty, Move, Tile},
    };

    fn match_with(scores: &[i32]) -> Game {
        let specs = scores
            .iter()
            .enumerate()
            .map(|(index, _)| SeatSpec {
                kind: SeatKind::Human {
                    user_id: Some(Uuid::from_u128(index as u128 + 1)),
                },
                name: format!("Player {}", index + 1),
            })
            .collect();
        let mut game = new_game(specs, GameOptions::default(), &mut rand::thread_rng());
        game.status = GameStatus::Finished;
        game.consecutive_scoreless = SCORELESS_LIMIT;
        for (seat, &score) in game.seats.iter_mut().zip(scores) {
            seat.score = score;
        }
        game.moves.push(Move {
            seat: 0,
            kind: MoveKind::Play { placements: vec![] },
            points: 4,
            words: vec!["AT".to_owned()],
            best: None,
        });
        game
    }

    #[test]
    fn active_abandoned_open_bot_and_pass_only_games_do_not_count() {
        let finished = match_with(&[2, 1]);
        assert!(ranked_game(&finished));
        let mut active = finished.clone();
        active.status = GameStatus::Active;
        assert!(!ranked_game(&active));
        active.consecutive_scoreless = 1;
        abandon(&mut active);
        assert!(!ranked_game(&active));
        let mut with_bot = finished.clone();
        with_bot.seats[1].kind = SeatKind::Bot {
            difficulty: Difficulty::Easy,
        };
        assert!(!ranked_game(&with_bot));
        let mut with_open = finished.clone();
        with_open.seats[0].kind = SeatKind::Human { user_id: None };
        assert!(!ranked_game(&with_open));
        let mut no_words = finished;
        no_words.moves[0].kind = MoveKind::Pass;
        assert!(!ranked_game(&no_words));
    }

    #[test]
    fn going_out_with_no_adjustment_counts() {
        let mut game = match_with(&[19, 17]);
        game.consecutive_scoreless = 0;
        game.bag.clear();
        game.seats[0].rack.clear();
        game.seats[1].rack = vec![Tile::Blank];
        assert!(ranked_game(&game));
    }

    #[test]
    fn tied_winners_and_equal_standings_share_points_and_rank() {
        let first = match_with(&[-2, -2, -10]);
        let mut second = match_with(&[14, 7, 4]);
        second.seats[0].name = "<Ann>".into();
        let third = match_with(&[14, 21, 16]);
        let players = standings([&second, &third, &first].into_iter());
        let points: Vec<_> = players
            .iter()
            .map(|(_, s)| (s.played, s.wins, s.draws, s.points()))
            .collect();
        assert_eq!(points, vec![(3, 1, 1, 3), (3, 1, 1, 3), (3, 0, 0, 0)]);
        let html = render(&players, Uuid::from_u128(1));
        assert_eq!(html.matches(r#"class="leaderboard-rank">1<"#).count(), 2);
        assert!(html.contains(r#"class="leaderboard-rank">3<"#));
        assert!(html.contains("&lt;Ann&gt; (you)"));
        assert!(render(&[], Uuid::nil()).contains("No ranked games yet"));
    }
}
