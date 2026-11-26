//! Private, match-scoped conversation. Chat has its own persistence so a message
//! neither changes the board revision nor wakes unrelated game subscribers.
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use axum::{
    Json,
    extract::{Path, Query, State},
    response::Html,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex as AsyncMutex;
use uuid::Uuid;

use crate::{
    app::AppState,
    error::AppError,
    models::{Game, SeatKind},
    render,
    session::{ApiAuthUser, AuthUser},
};

type Room = Arc<AsyncMutex<Option<Vec<Message>>>>;

pub struct ChatStore {
    dir: PathBuf,
    rooms: Mutex<HashMap<Uuid, Room>>,
}

#[derive(Clone, Serialize, Deserialize)]
struct Message {
    id: Uuid,
    sender: Uuid,
    name: String,
    text: String,
    sent_at: DateTime<Utc>,
}

#[derive(Serialize)]
pub struct MessageView {
    seq: usize,
    name: String,
    mine: bool,
    text: String,
    sent_at: DateTime<Utc>,
}

impl Message {
    fn view(&self, index: usize, viewer: Uuid) -> MessageView {
        MessageView {
            seq: index + 1,
            name: self.name.clone(),
            mine: self.sender == viewer,
            text: self.text.clone(),
            sent_at: self.sent_at,
        }
    }
}

impl ChatStore {
    pub async fn open(dir: PathBuf) -> Result<Self, AppError> {
        tokio::fs::create_dir_all(&dir)
            .await
            .map_err(AppError::internal)?;
        Ok(Self {
            dir,
            rooms: Mutex::default(),
        })
    }

    async fn room(&self, game: Uuid) -> Result<Room, AppError> {
        let room = self.rooms.lock().unwrap().entry(game).or_default().clone();
        let mut messages = room.lock().await;
        if messages.is_none() {
            let path = self.dir.join(format!("{game}.json"));
            let history = match tokio::fs::read(path).await {
                Ok(data) => serde_json::from_slice(&data).map_err(AppError::internal)?,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
                Err(e) => return Err(AppError::internal(e)),
            };
            *messages = Some(history);
        }
        drop(messages);
        Ok(room)
    }

    async fn append(&self, game: Uuid, message: Message) -> Result<MessageView, AppError> {
        let room = self.room(game).await?;
        let mut guard = room.lock().await;
        let history = guard.as_ref().unwrap();
        if let Some((index, previous)) =
            history.iter().enumerate().rev().find(|(_, previous)| {
                previous.id == message.id && previous.sender == message.sender
            })
        {
            return Ok(previous.view(index, message.sender));
        }
        let result = message.view(history.len(), message.sender);
        let mut next = history.clone();
        next.push(message);
        let bytes = serde_json::to_vec(&next).map_err(AppError::internal)?;
        let path = self.dir.join(format!("{game}.json"));
        let temp = path.with_extension(format!("{}.tmp", Uuid::new_v4()));
        tokio::fs::write(&temp, bytes)
            .await
            .map_err(AppError::internal)?;
        tokio::fs::rename(temp, path)
            .await
            .map_err(AppError::internal)?;
        *guard = Some(next);
        Ok(result)
    }
}

fn denied() -> AppError {
    AppError::not_found("Game chat not found")
}

async fn participants(
    state: &AppState,
    id: Uuid,
    viewer: Uuid,
) -> Result<(Arc<Game>, String), AppError> {
    let game = state.store.snapshot(id).ok_or_else(denied)?;
    if game
        .seats
        .iter()
        .filter(|s| matches!(s.kind, SeatKind::Human { .. }))
        .count()
        < 2
    {
        return Err(denied());
    }
    let name = game
        .seats
        .iter()
        .find(|s| matches!(s.kind, SeatKind::Human { user_id: Some(id) } if id == viewer))
        .map(|s| s.name.clone())
        .ok_or_else(denied)?;
    Ok((game, name))
}

pub async fn page(
    State(state): State<AppState>,
    AuthUser(viewer): AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Html<String>, AppError> {
    let (game, _) = participants(&state, id, viewer).await?;
    let names = game
        .seats
        .iter()
        .filter(|s| matches!(s.kind, SeatKind::Human { user_id: Some(_) }))
        .map(|s| s.name.as_str())
        .collect::<Vec<_>>()
        .join(" · ");
    Ok(Html(render::chat_page(id, &names)))
}

#[derive(Deserialize)]
pub struct Range {
    before: Option<usize>,
    after: Option<usize>,
}

#[derive(Serialize)]
pub struct History {
    messages: Vec<MessageView>,
    more_before: bool,
}

pub async fn history(
    State(state): State<AppState>,
    ApiAuthUser(viewer): ApiAuthUser,
    Path(id): Path<Uuid>,
    Query(range): Query<Range>,
) -> Result<Json<History>, AppError> {
    participants(&state, id, viewer).await?;
    let room = state.store.chats.room(id).await?;
    let guard = room.lock().await;
    let messages = guard.as_ref().unwrap();
    let (start, end) = if let Some(after) = range.after {
        let start = after.min(messages.len());
        (start, start.saturating_add(50).min(messages.len()))
    } else {
        let end = range
            .before
            .map(|seq| seq.saturating_sub(1))
            .unwrap_or(messages.len())
            .min(messages.len());
        (end.saturating_sub(50), end)
    };
    Ok(Json(History {
        messages: messages[start..end]
            .iter()
            .enumerate()
            .map(|(i, msg)| msg.view(start + i, viewer))
            .collect(),
        more_before: start > 0,
    }))
}

#[derive(Deserialize)]
pub struct Post {
    id: Uuid,
    text: String,
}

pub async fn send(
    State(state): State<AppState>,
    ApiAuthUser(viewer): ApiAuthUser,
    Path(id): Path<Uuid>,
    Json(body): Json<Post>,
) -> Result<Json<MessageView>, AppError> {
    let (_, name) = participants(&state, id, viewer).await?;
    let text = body.text.trim();
    if text.is_empty() || text.chars().count() > 500 {
        return Err(AppError::bad_request("Use 1 to 500 characters."));
    }
    let store = state.store.clone();
    let message = Message {
        id: body.id,
        sender: viewer,
        name,
        text: text.to_owned(),
        sent_at: Utc::now(),
    };
    // Finish the atomic write even if a phone disconnects mid-request. Retrying
    // with the same message id returns the committed copy.
    tokio::spawn(async move { store.chats.append(id, message).await })
        .await
        .map_err(AppError::internal)?
        .map(Json)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn simultaneous_messages_survive_reopening_the_store_in_order() {
        let dir = std::env::temp_dir().join(format!("scrabble-chat-{}", Uuid::new_v4()));
        let game = Uuid::new_v4();
        let viewer = Uuid::new_v4();
        let msg = |text: &str| Message {
            id: Uuid::new_v4(),
            sender: viewer,
            name: "Host".into(),
            text: text.into(),
            sent_at: Utc::now(),
        };
        let store = ChatStore::open(dir.clone()).await.unwrap();
        let (one, two) = tokio::join!(
            store.append(game, msg("one")),
            store.append(game, msg("two"))
        );
        let mut messages = [one.unwrap(), two.unwrap()];
        messages.sort_by_key(|m| m.seq);
        assert_eq!([messages[0].seq, messages[1].seq], [1, 2]);
        drop(store);
        let reopened = ChatStore::open(dir.clone()).await.unwrap();
        let room = reopened.room(game).await.unwrap();
        let history = room.lock().await;
        let persisted = history.as_ref().unwrap();
        assert_eq!(persisted.len(), 2);
        assert_eq!(persisted[0].text, messages[0].text);
        assert_eq!(persisted[1].text, messages[1].text);
        tokio::fs::remove_dir_all(dir).await.unwrap();
    }
}
