use crate::{
    app::AppState,
    error::AppError,
    models::{Game, GameStatus, SeatInvitation, SeatKind},
    render,
    routes::is_user_seated,
    session::{ApiAuthUser, AuthUser},
};
use axum::{
    Form, Json,
    extract::{Path, Query, State},
    response::{Html, Redirect},
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Deserialize)]
pub struct Search {
    #[serde(default)]
    q: String,
    game: Option<Uuid>,
}

#[derive(Serialize)]
pub struct Player {
    id: Uuid,
    name: String,
}

pub async fn search(
    State(state): State<AppState>,
    ApiAuthUser(user): ApiAuthUser,
    Query(query): Query<Search>,
) -> Result<Json<Vec<Player>>, AppError> {
    let game = if let Some(id) = query.game {
        let game = state
            .store
            .get(id)
            .await
            .ok_or_else(|| AppError::not_found("Game not found"))?;
        if !is_host(&game, user) {
            return Err(AppError::bad_request("Only the host can invite players"));
        }
        Some(game)
    } else {
        None
    };
    Ok(Json(
        state
            .users
            .search_players(user, &query.q)
            .await
            .into_iter()
            .filter(|(id, _)| {
                game.as_ref().is_none_or(|game| {
                    !is_user_seated(game, *id) && !game.seats.iter().any(|seat| seat.invited(*id))
                })
            })
            .map(|(id, name)| Player { id, name })
            .collect(),
    ))
}

fn is_host(game: &Game, user: Uuid) -> bool {
    game.status != GameStatus::Finished
        && game
            .seats
            .first()
            .is_some_and(|s| matches!(s.kind, SeatKind::Human {user_id: Some(id)} if id == user))
}

pub fn can_invite(game: &Game, user: Uuid) -> bool {
    is_host(game, user) && game.seats.iter().any(|s| s.can_invite())
}

pub async fn page(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Html<String>, AppError> {
    let game = state
        .store
        .get(id)
        .await
        .ok_or_else(|| AppError::not_found("Game not found"))?;
    if !can_invite(&game, user) {
        return Err(AppError::bad_request(
            "There is no seat available to invite a player",
        ));
    }
    Ok(Html(render::invite_page(id)))
}

#[derive(Deserialize)]
pub struct InviteForm {
    player: Uuid,
}

pub async fn send(
    State(state): State<AppState>,
    AuthUser(host): AuthUser,
    Path(id): Path<Uuid>,
    Form(form): Form<InviteForm>,
) -> Result<Redirect, AppError> {
    let player = state
        .users
        .invitable_player(form.player)
        .await
        .ok_or_else(|| AppError::not_found("Player not found"))?;
    let sender = state
        .store
        .update(id, |game| {
            if !is_host(game, host) {
                return Err(AppError::bad_request(
                    "Only the host of an unfinished game can invite a player",
                ));
            }
            if game.seats.iter().any(|s| s.invited(player.id)) {
                return Ok(None);
            }
            if is_user_seated(game, player.id) {
                return Err(AppError::bad_request("That player is already in this game"));
            }
            let seat = game
                .seats
                .iter_mut()
                .find(|s| s.can_invite())
                .ok_or_else(|| AppError::bad_request("No seat is available"))?;
            seat.invitation = Some(SeatInvitation {
                player: player.id,
                declined: false,
            });
            seat.name = format!("{} (invited)", player.display_name);
            Ok(Some(game.seats[0].name.clone()))
        })
        .await??;
    if let Some(sender) = sender {
        state
            .push
            .notify_invite(&state.users, player.id, id, &sender)
            .await;
    }
    Ok(Redirect::to(&format!("/games/{id}")))
}

pub async fn decline(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Redirect, AppError> {
    state
        .store
        .update(id, |game| {
            let seat = game
                .seats
                .iter_mut()
                .find(|s| s.invitation.as_ref().is_some_and(|i| i.player == user))
                .ok_or_else(|| AppError::not_found("Invitation not found"))?;
            if let Some(invitation) = &mut seat.invitation {
                invitation.declined = true;
                seat.name = "Invitation declined".into();
            }
            Ok::<_, AppError>(())
        })
        .await??;
    Ok(Redirect::to("/"))
}

pub fn inbox_html<'a>(games: impl Iterator<Item = &'a Game>, user: Uuid) -> String {
    let mut html = String::new();
    for game in games
        .filter(|g| g.status != GameStatus::Finished && g.seats.iter().any(|s| s.invited(user)))
    {
        let id = game.id;
        let host = render::escape(&game.seats[0].name);
        html.push_str(&format!(r#"<article class="invite-row"><p><strong>{host}</strong> invited you to play Scrabble.</p>
<div class="invite-actions"><form action="/games/{id}/join" method="post"><button class="button">Accept invitation</button></form>
<form action="/games/{id}/decline" method="post"><button class="button button-secondary">Decline</button></form></div></article>"#));
    }
    if html.is_empty() {
        html
    } else {
        format!("<h2>Game invitations</h2>{html}")
    }
}

pub async fn inbox(
    State(state): State<AppState>,
    ApiAuthUser(user): ApiAuthUser,
) -> impl axum::response::IntoResponse {
    let away = state
        .users
        .get(user)
        .await
        .is_some_and(|u| u.settings.active_away_until(chrono::Utc::now()).is_some());
    (
        [("x-scrabble-away", if away { "true" } else { "false" })],
        Html(inbox_html(
            state.store.snapshots().iter().map(AsRef::as_ref),
            user,
        )),
    )
}
