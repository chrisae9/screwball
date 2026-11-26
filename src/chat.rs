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

type Room = Arc<AsyncMutex<Option<RoomData>>>;

// The first span across sequence 1 is the read cursor. Later spans allow a
// phone to read the newest page without acknowledging unseen older messages.
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct ReadSpans(Vec<[usize; 2]>);

impl ReadSpans {
    fn add(&mut self, seen: &[usize], messages: &[Message], viewer: Uuid) {
        self.0.extend(seen.iter().map(|seq| [*seq, *seq]));
        self.0.sort_unstable();
        let mut merged: Vec<[usize; 2]> = Vec::new();
        for [start, end] in self.0.drain(..) {
            let previous_end = merged.last().map_or(0, |previous| previous[1]);
            // Own messages can never be unread; bridge them so an ordinary
            // back-and-forth chat persists a single contiguous cursor.
            let adjacent = start <= previous_end.saturating_add(1)
                || messages[previous_end..start - 1]
                    .iter()
                    .all(|m| m.sender == viewer);
            if adjacent && let Some(previous) = merged.last_mut() {
                previous[1] = previous[1].max(end);
            } else {
                merged.push([if adjacent { 1 } else { start }, end]);
            }
        }
        self.0 = merged;
    }
}

struct RoomData {
    messages: Vec<Message>,
    reads: HashMap<Uuid, ReadSpans>,
    unread: HashMap<Uuid, bool>,
}

impl RoomData {
    fn unread(&mut self, viewer: Uuid) -> bool {
        *self.unread.entry(viewer).or_insert_with(|| {
            let mut end = self.messages.len();
            if let Some(spans) = self.reads.get(&viewer) {
                for &[first, last] in spans.0.iter().rev() {
                    if self.messages[last.min(end)..end]
                        .iter()
                        .any(|m| m.sender != viewer)
                    {
                        return true;
                    }
                    end = first.saturating_sub(1).min(end);
                }
            }
            self.messages[..end].iter().any(|m| m.sender != viewer)
        })
    }
}

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
            let path = self.dir.join(format!("{game}.reads.json"));
            let reads = match tokio::fs::read(path).await {
                Ok(data) => serde_json::from_slice(&data).map_err(AppError::internal)?,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => HashMap::new(),
                Err(e) => return Err(AppError::internal(e)),
            };
            *messages = Some(RoomData {
                messages: history,
                reads,
                unread: HashMap::new(),
            });
        }
        drop(messages);
        Ok(room)
    }

    async fn append(&self, game: Uuid, message: Message) -> Result<MessageView, AppError> {
        let room = self.room(game).await?;
        let mut guard = room.lock().await;
        let data = guard.as_mut().unwrap();
        let history = &data.messages;
        if let Some((index, previous)) =
            history.iter().enumerate().rev().find(|(_, previous)| {
                previous.id == message.id && previous.sender == message.sender
            })
        {
            return Ok(previous.view(index, message.sender));
        }
        let sender = message.sender;
        let result = message.view(history.len(), sender);
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
        data.messages = next;
        for (viewer, unread) in &mut data.unread {
            if *viewer != sender {
                *unread = true;
            }
        }
        Ok(result)
    }

    async fn mark_seen(&self, game: Uuid, viewer: Uuid, seen: &[usize]) -> Result<(), AppError> {
        let room = self.room(game).await?;
        let mut guard = room.lock().await;
        let data = guard.as_mut().unwrap();
        if seen
            .iter()
            .any(|seq| *seq == 0 || *seq > data.messages.len())
        {
            return Err(AppError::bad_request("Unknown chat message."));
        }
        let mut reads = data.reads.clone();
        let receipt = reads.entry(viewer).or_default();
        receipt.add(seen, &data.messages, viewer);
        if data.reads == reads {
            return Ok(());
        }
        let path = self.dir.join(format!("{game}.reads.json"));
        let temp = path.with_extension(format!("{}.tmp", Uuid::new_v4()));
        let bytes = serde_json::to_vec(&reads).map_err(AppError::internal)?;
        tokio::fs::write(&temp, bytes)
            .await
            .map_err(AppError::internal)?;
        tokio::fs::rename(temp, path)
            .await
            .map_err(AppError::internal)?;
        data.reads = reads;
        data.unread.remove(&viewer);
        Ok(())
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
    Ok(Html(render::chat_page(id, viewer, &names)))
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
    let messages = &guard.as_ref().unwrap().messages;
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

pub async fn has_unread(state: &AppState, game: Uuid, viewer: Uuid) -> Result<bool, AppError> {
    participants(state, game, viewer).await?;
    let room = state.store.chats.room(game).await?;
    Ok(room.lock().await.as_mut().unwrap().unread(viewer))
}

#[derive(Deserialize)]
pub struct ReadRequest {
    seen: Vec<usize>,
}

pub async fn mark_read(
    State(state): State<AppState>,
    ApiAuthUser(viewer): ApiAuthUser,
    Path(game): Path<Uuid>,
    Json(body): Json<ReadRequest>,
) -> Result<axum::http::StatusCode, AppError> {
    participants(&state, game, viewer).await?;
    if body.seen.is_empty() || body.seen.len() > 100 {
        return Err(AppError::bad_request("Send 1 to 100 displayed messages."));
    }
    let store = state.store.clone();
    tokio::spawn(async move { store.chats.mark_seen(game, viewer, &body.seen).await })
        .await
        .map_err(AppError::internal)??;
    Ok(axum::http::StatusCode::NO_CONTENT)
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
    async fn read_receipts_preserve_unseen_pages_and_sync_across_restarts() {
        let dir = std::env::temp_dir().join(format!("scrabble-reads-{}", Uuid::new_v4()));
        let id = Uuid::new_v4();
        let guest = Uuid::new_v4();
        let host = Uuid::new_v4();
        let message = |sender| Message {
            id: Uuid::new_v4(),
            sender,
            name: "Player".into(),
            text: "Hello".into(),
            sent_at: Utc::now(),
        };
        let store = ChatStore::open(dir.clone()).await.unwrap();
        let room = store.room(id).await.unwrap();
        async fn unread(room: &Room, viewer: Uuid) -> bool {
            room.lock().await.as_mut().unwrap().unread(viewer)
        }
        store.append(id, message(host)).await.unwrap();
        assert!(!unread(&room, host).await);
        assert!(unread(&room, guest).await);
        store.append(id, message(guest)).await.unwrap();
        store.append(id, message(host)).await.unwrap();
        store.mark_seen(id, guest, &[3]).await.unwrap();
        assert!(
            unread(&room, guest).await,
            "the unseen first page still counts"
        );
        assert!(store.mark_seen(id, guest, &[4]).await.is_err());
        drop(store);
        let store = ChatStore::open(dir.clone()).await.unwrap();
        let room = store.room(id).await.unwrap();
        assert!(unread(&room, guest).await);
        let (first, last) = tokio::join!(
            store.mark_seen(id, guest, &[1]),
            store.mark_seen(id, guest, &[3])
        );
        first.unwrap();
        last.unwrap();
        assert!(!unread(&room, guest).await);
        assert_eq!(
            room.lock().await.as_ref().unwrap().reads[&guest].0,
            vec![[1, 3]]
        );
        assert!(
            unread(&room, host).await,
            "reading does not mark a partner's chat read"
        );
        drop(store);
        let store = ChatStore::open(dir.clone()).await.unwrap();
        let room = store.room(id).await.unwrap();
        assert!(!unread(&room, guest).await);
        store.append(id, message(guest)).await.unwrap();
        assert!(!unread(&room, guest).await);
        store.append(id, message(host)).await.unwrap();
        assert!(unread(&room, guest).await);
        tokio::fs::remove_dir_all(dir).await.unwrap();
    }

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
        let persisted = &history.as_ref().unwrap().messages;
        assert_eq!(persisted.len(), 2);
        assert_eq!(persisted[0].text, messages[0].text);
        assert_eq!(persisted[1].text, messages[1].text);
        tokio::fs::remove_dir_all(dir).await.unwrap();
    }
}
