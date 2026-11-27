use std::collections::{BTreeMap, BTreeSet};

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    app::AppState,
    error::AppError,
    models::{Game, GameStatus, MoveKind, RACK_SIZE, SeatKind},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ItemKind {
    Tile,
    Rack,
}

#[derive(Debug, Serialize)]
pub struct Item {
    pub id: &'static str,
    pub name: &'static str,
    pub kind: ItemKind,
    pub price: u32,
    pub starter: bool,
    pub trophy: bool,
}

impl Item {
    const fn tile(
        id: &'static str,
        name: &'static str,
        price: u32,
        starter: bool,
        trophy: bool,
    ) -> Self {
        Self {
            id,
            name,
            price,
            starter,
            trophy,
            kind: ItemKind::Tile,
        }
    }
    const fn rack(id: &'static str, name: &'static str, price: u32) -> Self {
        Self {
            id,
            name,
            price,
            starter: false,
            trophy: false,
            kind: ItemKind::Rack,
        }
    }
}

pub fn catalog() -> &'static [Item] {
    static CATALOG: &[Item] = &[
        Item::tile("classic", "Classic Ivory", 0, false, false),
        Item::tile("cobalt", "Cobalt Enamel", 50, true, false),
        Item::tile("cherry", "Cherry Lacquer", 50, true, false),
        Item::tile("lavender", "Lavender Pearl", 50, true, false),
        Item::tile("forest", "Forest Green", 45, false, false),
        Item::tile("amber", "Burnished Amber", 45, false, false),
        Item::tile("slate", "Slate Blue", 60, false, false),
        Item::tile("glacier", "Glacier Cut", 100, false, false),
        Item::tile("obsidian", "Obsidian", 100, false, false),
        Item::tile("solar", "Solar Flare", 150, false, false),
        Item::tile("bingo", "Sevenfold", 0, false, true),
        Item::tile("aurora", "Aurora", 0, false, true),
        Item::rack("maple", "Natural Maple", 0),
        Item::rack("walnut", "Black Walnut", 40),
        Item::rack("midnight", "Midnight Lacquer", 65),
        Item::rack("rosewood", "Polished Rosewood", 80),
    ];
    CATALOG
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Equipment {
    tile: Option<String>,
    rack: Option<String>,
    pub uniform_board: bool,
}

impl Equipment {
    pub fn tile(&self) -> &str {
        self.tile.as_deref().unwrap_or("classic")
    }
    pub fn rack(&self) -> &str {
        self.rack.as_deref().unwrap_or("maple")
    }

    fn equip(&mut self, item: &Item) {
        match item.kind {
            ItemKind::Tile => self.tile = (item.id != "classic").then(|| item.id.into()),
            ItemKind::Rack => self.rack = (item.id != "maple").then(|| item.id.into()),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
enum MatchReward {
    Human,
    Bot,
}

impl MatchReward {
    fn for_game(game: &Game) -> Option<Self> {
        if game.status != GameStatus::Finished
            || game.seats.len() < 2
            || !crate::leaderboard::has_final_scores(game)
            || game
                .seats
                .iter()
                .any(|s| matches!(s.kind, SeatKind::Human { user_id: None }))
            || !game.moves.iter().any(|mv| {
                mv.points > 0
                    && mv.seat < game.seats.len()
                    && !mv.words.is_empty()
                    && matches!(&mv.kind,MoveKind::Play { placements } if !placements.is_empty())
            })
        {
            return None;
        }
        Some(if game.seats.iter().any(|s| s.is_bot()) {
            Self::Bot
        } else {
            Self::Human
        })
    }

    fn coins(self) -> u32 {
        match self {
            Self::Bot => 10,
            Self::Human => 20,
        }
    }
}

pub fn finish_reward(game: &Game, viewer: Uuid) -> Option<u32> {
    if !crate::routes::is_user_seated(game, viewer) {
        return None;
    }
    MatchReward::for_game(game).map(MatchReward::coins)
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
struct Rewards {
    daily: BTreeSet<NaiveDate>,
    finishes: BTreeMap<Uuid, MatchReward>,
    bingo: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Wallet {
    rewards: Rewards,
    starter: Option<String>,
    // Persist the actual price paid, so later catalog changes do not rewrite balances.
    purchases: BTreeMap<String, u32>,
    pub equipment: Equipment,
}

impl Rewards {
    fn credit(&mut self, user: Uuid, game: &Game) {
        let Some(index) = game
            .seats
            .iter()
            .position(|s| matches!(s.kind, SeatKind::Human {user_id:Some(id)} if id == user))
        else {
            return;
        };
        for mv in game
            .moves
            .iter()
            .filter(|m| m.seat == index && !m.words.is_empty())
        {
            if let MoveKind::Play { placements } = &mv.kind
                && !placements.is_empty()
            {
                if let Some(day) = mv.played_on {
                    self.daily.insert(day);
                }
                self.bingo |= placements.len() == RACK_SIZE;
            }
        }
        if let Some(reward) = MatchReward::for_game(game) {
            self.finishes.entry(game.id).or_insert(reward);
        }
    }

    fn merge(&mut self, other: Rewards) {
        self.daily.extend(other.daily);
        for (id, reward) in other.finishes {
            self.finishes.entry(id).or_insert(reward);
        }
        self.bingo |= other.bingo;
    }
}

impl Wallet {
    pub(crate) fn is_empty(&self) -> bool {
        self == &Self::default()
    }

    fn studio(&self) -> Studio {
        let human_finishes = self
            .rewards
            .finishes
            .values()
            .filter(|&&kind| kind == MatchReward::Human)
            .count();
        let bot_finishes = self.rewards.finishes.len() - human_finishes;
        let daily_words = self.rewards.daily.len();
        let earned = (human_finishes as u64) * u64::from(MatchReward::Human.coins())
            + (bot_finishes as u64) * u64::from(MatchReward::Bot.coins())
            + (daily_words as u64) * 5;
        let spent: u64 = self.purchases.values().copied().map(u64::from).sum();
        let balance = u32::try_from(earned.saturating_sub(spent)).unwrap_or(u32::MAX);
        let mut owned: BTreeSet<_> = ["classic".to_owned(), "maple".to_owned()]
            .into_iter()
            .chain(self.starter.iter().cloned())
            .chain(self.purchases.keys().cloned())
            .collect();
        if self.rewards.bingo {
            owned.insert("bingo".into());
        }
        if human_finishes >= 5 {
            owned.insert("aurora".into());
        }
        Studio {
            balance,
            starter_available: self.starter.is_none(),
            bingo: self.rewards.bingo,
            owned,
            tile: self.equipment.tile().to_owned(),
            rack: self.equipment.rack().to_owned(),
            uniform_board: self.equipment.uniform_board,
            human_finishes,
            bot_finishes,
            daily_words,
        }
    }

    fn apply(&mut self, action: &str, value: &str) -> Result<(), AppError> {
        if action == "uniform" {
            self.equipment.uniform_board = match value {
                "on" => true,
                "off" => false,
                _ => return Err(AppError::bad_request("Choose on or off.")),
            };
            return Ok(());
        }
        let item = catalog()
            .iter()
            .find(|i| i.id == value)
            .ok_or_else(|| AppError::bad_request("Unknown studio item."))?;
        let studio = self.studio();
        match action {
            "starter" => {
                if self.starter.as_deref() == Some(item.id) {
                    return Ok(());
                }
                if !item.starter || !studio.starter_available || studio.owned.contains(item.id) {
                    return Err(AppError::conflict("Choose one available starter tile set."));
                }
                self.starter = Some(item.id.into());
            }
            "buy" => {
                if studio.owned.contains(item.id) {
                    return Ok(());
                }
                if item.trophy {
                    return Err(AppError::conflict("Earn this tile set by playing."));
                }
                if studio.balance < item.price {
                    return Err(AppError::conflict("You need more coins for this item."));
                }
                self.purchases.insert(item.id.into(), item.price);
            }
            "equip" if studio.owned.contains(item.id) => {}
            "equip" => return Err(AppError::conflict("Unlock this item before equipping it.")),
            _ => return Err(AppError::bad_request("Unknown studio action.")),
        }
        self.equipment.equip(item);
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Studio {
    pub balance: u32,
    pub starter_available: bool,
    pub bingo: bool,
    pub owned: BTreeSet<String>,
    pub tile: String,
    pub rack: String,
    pub uniform_board: bool,
    pub daily_words: usize,
    pub human_finishes: usize,
    pub bot_finishes: usize,
}

async fn reconcile(
    state: &AppState,
    user: Uuid,
    operation: Option<(&str, &str)>,
) -> Result<Studio, AppError> {
    // The saved game is the durable source for new rewards. A crash after the
    // move commits but before visiting the shop can never lose the day's credit.
    let mut earned = Rewards::default();
    for game in state.store.snapshots() {
        earned.credit(user, &game);
    }
    state
        .users
        .edit_wallet(user, |wallet| {
            wallet.rewards.merge(earned);
            if let Some((action, item)) = operation {
                wallet.apply(action, item)?;
            }
            Ok(wallet.studio())
        })
        .await
}

pub async fn studio(state: &AppState, user: Uuid) -> Result<Studio, AppError> {
    reconcile(state, user, None).await
}
pub async fn change(
    state: &AppState,
    user: Uuid,
    action: &str,
    item: &str,
) -> Result<Studio, AppError> {
    reconcile(state, user, Some((action, item))).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        dict::Dictionary,
        game::{GameOptions, SeatSpec, apply_move, new_game},
        models::{Difficulty, Placement, Position, SCORELESS_LIMIT, Tile},
        users::UserStore,
    };

    fn game_for(player: Uuid, bot: bool) -> Game {
        let opponent = if bot {
            SeatKind::Bot {
                difficulty: Difficulty::Easy,
            }
        } else {
            SeatKind::Human {
                user_id: Some(Uuid::new_v4()),
            }
        };
        new_game(
            vec![
                SeatSpec {
                    kind: SeatKind::Human {
                        user_id: Some(player),
                    },
                    name: "Home".into(),
                },
                SeatSpec {
                    kind: opponent,
                    name: "Visitor".into(),
                },
            ],
            GameOptions::default(),
            &mut rand::thread_rng(),
        )
    }

    fn play_word(game: &mut Game, seat: usize, word: &str) {
        game.turn = seat;
        game.seats[seat].rack = word.chars().map(Tile::Letter).collect();
        let placements = word
            .chars()
            .enumerate()
            .map(|(offset, letter)| Placement {
                position: Position::new(7, 7 + offset),
                letter,
                is_blank: false,
            })
            .collect();
        apply_move(
            game,
            &Dictionary::from_words(word),
            seat,
            MoveKind::Play { placements },
            &mut rand::thread_rng(),
        )
        .unwrap();
    }

    #[test]
    fn rewards_use_only_real_human_moves_and_settled_matches() {
        let user = Uuid::new_v4();
        let mut game = game_for(user, false);
        let before = chrono::Utc::now().date_naive();
        play_word(&mut game, 0, "READING");
        let day = game.moves[0].played_on.unwrap();
        assert!(day == before || day == chrono::Utc::now().date_naive());
        let mut wallet = Wallet::default();
        wallet.rewards.credit(user, &game);
        wallet.rewards.credit(user, &game);
        assert_eq!(wallet.studio().balance, 5);
        assert!(wallet.studio().owned.contains("bingo"));
        crate::game::abandon(&mut game);
        wallet.rewards.credit(user, &game);
        assert_eq!(wallet.studio().balance, 5);

        let mut legacy = serde_json::to_value(&game).unwrap();
        legacy["moves"][0]
            .as_object_mut()
            .unwrap()
            .remove("played_on");
        let mut legacy: Game = serde_json::from_value(legacy).unwrap();
        legacy.consecutive_scoreless = SCORELESS_LIMIT;
        let mut retro = Wallet::default();
        retro.rewards.credit(user, &legacy);
        retro.rewards.credit(user, &legacy);
        assert_eq!(retro.studio().balance, 20);
        assert_eq!(finish_reward(&legacy, user), Some(20));
        assert_eq!(finish_reward(&legacy, Uuid::new_v4()), None);
        assert_eq!(retro.studio().daily_words, 0);
        assert!(retro.studio().owned.contains("bingo"));
        for _ in 0..4 {
            legacy.id = Uuid::new_v4();
            retro.rewards.credit(user, &legacy);
        }
        assert!(retro.studio().owned.contains("aurora"));
        assert_eq!(retro.studio().balance, 100);
        let mut bot_game = game_for(user, true);
        play_word(&mut bot_game, 1, "READING");
        assert_eq!(bot_game.moves[0].played_on, None);
        bot_game.status = GameStatus::Finished;
        bot_game.consecutive_scoreless = SCORELESS_LIMIT;
        let mut bot_wallet = Wallet::default();
        bot_wallet.rewards.credit(user, &bot_game);
        assert_eq!(bot_wallet.studio().balance, 10);
        assert_eq!(finish_reward(&bot_game, user), Some(10));
        bot_game.moves[0].points = 0;
        assert_eq!(finish_reward(&bot_game, user), None);
        bot_game.moves[0].points = 50;
        assert!(!bot_wallet.studio().bingo);
        bot_game.seats[1].kind = SeatKind::Human { user_id: None };
        assert!({
            let mut empty = Rewards::default();
            empty.credit(user, &bot_game);
            empty.finishes.is_empty()
        });
        game.moves.clear();
        game.consecutive_scoreless = SCORELESS_LIMIT;
        let mut abandoned = Wallet::default();
        abandoned.rewards.credit(user, &game);
        assert_eq!(abandoned.studio().balance, 0);
    }

    #[test]
    fn daily_credit_is_global_per_utc_date_including_out_of_order_games() {
        let user = Uuid::new_v4();
        let mut game = game_for(user, false);
        play_word(&mut game, 0, "AT");
        let mut wallet = Wallet::default();
        for day in [2, 2, 1, 3, 1] {
            game.id = Uuid::new_v4();
            game.moves[0].played_on = NaiveDate::from_ymd_opt(2026, 1, day);
            wallet.rewards.credit(user, &game);
        }
        assert_eq!(wallet.studio().balance, 15);
    }

    #[tokio::test]
    async fn a_failed_wallet_write_leaves_equipment_and_free_starter_available_for_retry() {
        let dir = std::env::temp_dir().join(format!("screwball-wallet-write-{}", Uuid::new_v4()));
        let store = UserStore::load(&dir).await.unwrap();
        let user = store
            .authentik_profile("wallet-outage", "Player")
            .await
            .unwrap();
        let users_dir = dir.join("users");
        let saved = dir.join("moved");
        tokio::fs::rename(&users_dir, &saved).await.unwrap();
        // An unchanged read succeeds even though the storage directory is unavailable.
        let available = store
            .edit_wallet(user, |wallet| Ok(wallet.studio().starter_available))
            .await
            .unwrap();
        assert!(available);
        assert!(
            store
                .edit_wallet(user, |wallet| wallet.apply("starter", "cherry"))
                .await
                .is_err()
        );
        assert_eq!(store.player_names().await.tile_for_user(user), "classic");
        tokio::fs::rename(&saved, &users_dir).await.unwrap();
        store
            .edit_wallet(user, |wallet| wallet.apply("starter", "cherry"))
            .await
            .unwrap();
        assert_eq!(store.player_names().await.tile_for_user(user), "cherry");
        tokio::fs::remove_dir_all(dir).await.unwrap();
    }

    #[tokio::test]
    async fn purchases_and_one_starter_survive_concurrent_requests_and_restarts() {
        let dir = std::env::temp_dir().join(format!("screwball-studio-{}", Uuid::new_v4()));
        let users = UserStore::load(&dir).await.unwrap();
        let user = users
            .authentik_profile("cosmetic", "Shopper")
            .await
            .unwrap();
        let buy = |item| users.edit_wallet(user, move |w| w.apply("buy", item));
        let starter = |item| users.edit_wallet(user, move |w| w.apply("starter", item));
        let (a, b) = tokio::join!(starter("cobalt"), starter("lavender"));
        assert_ne!(a.is_ok(), b.is_ok());
        assert!(buy("bingo").await.is_err());
        assert!(buy("aurora").await.is_err());
        assert!(buy("glacier").await.is_err());
        users
            .edit_wallet(user, |w| {
                for _ in 0..5 {
                    w.rewards
                        .finishes
                        .insert(Uuid::new_v4(), MatchReward::Human);
                }
                Ok(())
            })
            .await
            .unwrap();
        let (a, b) = tokio::join!(buy("glacier"), buy("obsidian"));
        assert_ne!(a.is_ok(), b.is_ok());
        let chosen = if a.is_ok() { "glacier" } else { "obsidian" };
        assert!(buy(chosen).await.is_ok());
        users
            .edit_wallet(user, |w| w.apply("uniform", "on"))
            .await
            .unwrap();
        assert_eq!(users.player_names().await.tile_for_user(user), chosen);
        drop(users);
        let reloaded = UserStore::load(&dir).await.unwrap();
        let state = reloaded
            .get(user)
            .await
            .unwrap()
            .settings
            .cosmetics
            .studio();
        assert_eq!(state.balance, 0);
        assert_eq!(state.tile, chosen);
        assert!(!state.starter_available);
        assert!(state.uniform_board);
        assert!(state.owned.contains("aurora"));
        assert!(reloaded.player_names().await.uniform_board(user));
        tokio::fs::remove_dir_all(dir).await.unwrap();
    }
}
