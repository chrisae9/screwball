use std::collections::{BTreeMap, BTreeSet};

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    app::AppState,
    error::AppError,
    models::{Game, GameStatus, MoveKind, Placement, RACK_SIZE, SeatKind},
};

#[derive(Debug, Serialize)]
pub struct Item {
    pub id: &'static str,
    pub name: &'static str,
    pub price: u32,
    pub trophy: bool,
    pub rarity: Option<&'static str>,
}

impl Item {
    const fn tile(id: &'static str, name: &'static str, price: u32, trophy: bool) -> Self {
        Self {
            id,
            name,
            price,
            trophy,
            rarity: None,
        }
    }
    const fn rarity(mut self, rarity: &'static str) -> Self {
        self.rarity = Some(rarity);
        self
    }
}

pub fn catalog() -> &'static [Item] {
    static CATALOG: &[Item] = &[
        Item::tile("classic", "Classic Ivory", 0, false),
        Item::tile("cobalt", "Cobalt Enamel", 50, false),
        Item::tile("cherry", "Cherry Lacquer", 50, false),
        Item::tile("lavender", "Lavender Pearl", 50, false),
        Item::tile("forest", "Forest Green", 45, false),
        Item::tile("amber", "Burnished Amber", 45, false),
        Item::tile("slate", "Slate Blue", 60, false),
        Item::tile("glacier", "Glacier Cut", 100, false),
        Item::tile("obsidian", "Obsidian", 100, false),
        Item::tile("solar", "Solar Flare", 150, false),
        Item::tile("bingo", "Sevenfold", 0, true),
        Item::tile("aurora", "Aurora", 0, true),
        Item::tile("koi", "Koi Pond", 90, false).rarity("Rare"),
        Item::tile("circuit", "Arcade Circuit", 125, false).rarity("Rare"),
        Item::tile("kintsugi", "Kintsugi", 200, false).rarity("Epic"),
        Item::tile("rift", "Event Horizon", 275, false).rarity("Epic"),
        Item::tile("serpent", "Dragon Scale", 0, true).rarity("Legendary"),
        Item::tile("orrery", "Celestial Atlas", 0, true).rarity("Legendary"),
        Item::tile("moth", "Lantern Moth", 0, true).rarity("Rare"),
        Item::tile("junction", "Neon Junction", 0, true).rarity("Epic"),
        Item::tile("kraken", "Kraken Ink", 0, true).rarity("Epic"),
        Item::tile("fox", "Mirror Fox", 0, true).rarity("Rare"),
        Item::tile("scarab", "Scarab Relic", 0, true).rarity("Legendary"),
        Item::tile("phoenix", "Ashborn Phoenix", 0, true).rarity("Legendary"),
    ];
    CATALOG
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Equipment {
    tile: Option<String>,
    pub uniform_board: bool,
}

impl Equipment {
    pub fn tile(&self) -> &str {
        self.tile.as_deref().unwrap_or("classic")
    }
    fn equip(&mut self, item: &Item) {
        self.tile = (item.id != "classic").then(|| item.id.into());
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
struct WordAchievements {
    played_blank: bool,
    most_words: usize,
    longest_word: usize,
    palindrome: bool,
    rare_letters: BTreeSet<char>,
    bingo_games: BTreeSet<Uuid>,
}

impl WordAchievements {
    fn credit(&mut self, game_id: Uuid, placements: &[Placement], words: &[String]) {
        self.most_words = self.most_words.max(words.len());
        for word in words {
            // Formed words in the move log are English board letters.
            self.longest_word = self.longest_word.max(word.len());
            self.palindrome |= word.len() >= 3
                && word
                    .bytes()
                    .zip(word.bytes().rev())
                    .all(|(a, b)| a.eq_ignore_ascii_case(&b));
        }
        for placement in placements {
            let letter = placement.letter.to_ascii_uppercase();
            if placement.is_blank {
                self.played_blank = true;
            } else if matches!(letter, 'J' | 'Q' | 'X' | 'Z') {
                self.rare_letters.insert(letter);
            }
        }
        if placements.len() == RACK_SIZE {
            self.bingo_games.insert(game_id);
        }
    }

    fn merge(&mut self, other: Self) {
        self.played_blank |= other.played_blank;
        self.most_words = self.most_words.max(other.most_words);
        self.longest_word = self.longest_word.max(other.longest_word);
        self.palindrome |= other.palindrome;
        self.rare_letters.extend(other.rare_letters);
        self.bingo_games.extend(other.bingo_games);
    }

    // Both the shop's progress text and its equip authorization use the same thresholds.
    fn requirement(&self, id: &str) -> Option<(&'static str, usize, usize)> {
        Some(match id {
            "moth" => ("Play a blank in a word", usize::from(self.played_blank), 1),
            "junction" => ("Form 3 words in one turn", self.most_words, 3),
            "kraken" => ("Form a word of 8 or more letters", self.longest_word, 8),
            "fox" => (
                "Form a palindrome of 3 or more letters",
                usize::from(self.palindrome),
                1,
            ),
            "scarab" => (
                "Place J, Q, X and Z without blanks",
                self.rare_letters.len(),
                4,
            ),
            "phoenix" => (
                "Play all 7 tiles in 3 different games",
                self.bingo_games.len(),
                3,
            ),
            _ => return None,
        })
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
struct Rewards {
    daily: BTreeSet<NaiveDate>,
    finishes: BTreeMap<Uuid, MatchReward>,
    bingo: bool,
    best_play: u32,
    words: WordAchievements,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Wallet {
    rewards: Rewards,
    // Original releases allowed one free claim. Keep ownership for those players
    // without creating a grant on new accounts.
    #[serde(rename = "starter", skip_serializing_if = "Option::is_none")]
    legacy_claim: Option<String>,
    // Persist the actual price paid, so later catalog changes do not rewrite balances.
    #[serde(default, deserialize_with = "read_tile_receipts")]
    purchases: BTreeMap<String, u32>,
    pub equipment: Equipment,
}

/// A wallet's balance is earned coins less its original receipts. Dropping only
/// retired rack purchases restores exactly what each player paid, once, regardless
/// of whether the profile has already been rewritten or is opened repeatedly.
fn read_tile_receipts<'de, D>(deserializer: D) -> Result<BTreeMap<String, u32>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let mut receipts = BTreeMap::<String, u32>::deserialize(deserializer)?;
    for retired in [
        "maple", "walnut", "midnight", "rosewood", "torii", "reactor",
    ] {
        receipts.remove(retired);
    }
    Ok(receipts)
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
                self.words.credit(game.id, placements, &mv.words);
                self.bingo |= placements.len() == RACK_SIZE;
                self.best_play = self.best_play.max(mv.points);
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
        self.best_play = self.best_play.max(other.best_play);
        self.words.merge(other.words);
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
        let mut owned: BTreeSet<_> = ["classic".to_owned()]
            .into_iter()
            .chain(self.legacy_claim.iter().cloned())
            .chain(self.purchases.keys().cloned())
            .collect();
        if self.rewards.bingo {
            owned.insert("bingo".into());
        }
        if human_finishes >= 5 {
            owned.insert("aurora".into());
        }
        if self.rewards.best_play >= 75 {
            owned.insert("serpent".into());
        }
        if daily_words >= 14 {
            owned.insert("orrery".into());
        }
        for item in catalog() {
            if let Some((_, count, target)) = self.rewards.words.requirement(item.id)
                && count >= target
            {
                owned.insert(item.id.into());
            }
        }
        Studio {
            words: self.rewards.words.clone(),
            best_play: self.rewards.best_play,
            balance,
            bingo: self.rewards.bingo,
            owned,
            tile: self.equipment.tile().to_owned(),
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
            "buy" => {
                if item.trophy {
                    return Err(AppError::conflict("Earn this tile set by playing."));
                }
                if studio.owned.contains(item.id) {
                    return Ok(());
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
    #[serde(skip)]
    words: WordAchievements,
    pub balance: u32,
    pub best_play: u32,
    pub bingo: bool,
    pub owned: BTreeSet<String>,
    pub tile: String,
    pub uniform_board: bool,
    pub daily_words: usize,
    pub human_finishes: usize,
    pub bot_finishes: usize,
}

impl Studio {
    pub fn achievement_rule(&self, id: &str) -> Option<String> {
        let (rule, count, target) = self.words.requirement(id)?;
        Some(format!("{rule} · {}/{target}", count.min(target)))
    }
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
        models::{Difficulty, Position, SCORELESS_LIMIT, Tile},
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
        play_at(game, seat, (7, 7), word, word);
    }

    // Lowercase denotes a blank represented by that letter.
    fn play_at(game: &mut Game, seat: usize, (row, col): (usize, usize), tiles: &str, dict: &str) {
        game.turn = seat;
        game.seats[seat].rack = tiles
            .chars()
            .map(|letter| {
                if letter.is_ascii_lowercase() {
                    Tile::Blank
                } else {
                    Tile::Letter(letter)
                }
            })
            .collect();
        let placements = tiles
            .chars()
            .enumerate()
            .map(|(offset, letter)| Placement {
                position: Position::new(row, col + offset),
                letter: letter.to_ascii_uppercase(),
                is_blank: letter.is_ascii_lowercase(),
            })
            .collect();
        apply_move(
            game,
            &Dictionary::from_words(dict),
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
    fn legendary_unlocks_use_personal_valid_plays_and_distinct_utc_dates() {
        let user = Uuid::new_v4();
        let mut game = game_for(user, false);
        play_word(&mut game, 0, "AT");
        let mut play = game.moves.pop().unwrap();
        play.played_on = None; // Older saved games still prove a score, but cannot prove a date.
        play.points = 74;
        game.moves.push(play.clone());
        let mut other = play.clone();
        other.seat = 1;
        other.points = 200;
        game.moves.push(other);
        let mut invalid = play.clone();
        invalid.points = 200;
        invalid.kind = MoveKind::Pass;
        game.moves.push(invalid);
        let mut wallet: Wallet =
            serde_json::from_str(r#"{"rewards":{"bingo":false,"daily":[]}}"#).unwrap();
        wallet.rewards.credit(user, &game);
        assert_eq!(wallet.studio().best_play, 74);
        assert!(!wallet.studio().owned.contains("serpent"));
        assert!(wallet.apply("buy", "serpent").is_err());
        assert!(wallet.apply("equip", "serpent").is_err());

        play.points = 75;
        game.moves = vec![play];
        let mut older = Rewards::default();
        older.credit(user, &game);
        wallet.rewards.merge(older);
        assert!(wallet.studio().owned.contains("serpent"));
        wallet.apply("equip", "serpent").unwrap();
        assert!(wallet.apply("buy", "serpent").is_err());
        assert_eq!(wallet.studio().daily_words, 0);

        let mut recent = Rewards::default();
        game.moves[0].points = 3;
        for day in (1..=25).step_by(2) {
            game.moves[0].played_on = NaiveDate::from_ymd_opt(2026, 1, day);
            recent.credit(user, &game);
            recent.credit(user, &game);
        }
        wallet.rewards.merge(recent);
        assert_eq!(wallet.studio().daily_words, 13);
        assert!(!wallet.studio().owned.contains("orrery"));
        assert!(wallet.apply("buy", "orrery").is_err());
        game.moves[0].played_on = NaiveDate::from_ymd_opt(2026, 2, 14);
        wallet.rewards.credit(user, &game);
        wallet.apply("equip", "orrery").unwrap();
        let reloaded: Wallet =
            serde_json::from_str(&serde_json::to_string(&wallet).unwrap()).unwrap();
        let studio = reloaded.studio();
        assert_eq!(studio.daily_words, 14);
        assert_eq!(studio.best_play, 75);
        assert_eq!(studio.tile, "orrery");
        assert_eq!(studio.balance, 70);
        assert!(studio.owned.contains("serpent"));
        assert!(studio.owned.contains("orrery"));
    }

    #[test]
    fn achievement_rules_and_permissions_follow_committed_personal_moves() {
        let player = Uuid::new_v4();
        let mut wallet: Wallet = serde_json::from_str(r#"{"rewards":{"bingo":true}}"#).unwrap();
        let items = [
            ("moth", "Lantern Moth", "Rare", "0/1"),
            ("junction", "Neon Junction", "Epic", "0/3"),
            ("kraken", "Kraken Ink", "Epic", "0/8"),
            ("fox", "Mirror Fox", "Rare", "0/1"),
            ("scarab", "Scarab Relic", "Legendary", "0/4"),
            ("phoenix", "Ashborn Phoenix", "Legendary", "0/3"),
        ];
        assert!(wallet.studio().achievement_rule("classic").is_none());
        for (id, title, tier, zero) in items {
            let item = catalog().iter().find(|item| item.id == id).unwrap();
            assert_eq!(
                (item.trophy, item.name, item.rarity),
                (true, title, Some(tier))
            );
            assert!(
                wallet
                    .studio()
                    .achievement_rule(id)
                    .unwrap()
                    .ends_with(zero)
            );
            assert!(wallet.apply("buy", id).is_err());
            assert!(wallet.apply("equip", id).is_err());
        }

        let mut game = game_for(player, false);
        play_at(&mut game, 1, (7, 7), "rACECAR", "RACECAR");
        play_at(&mut game, 0, (7, 14), "S", "RACECARS");
        let mut opponent = game.moves[0].clone();
        opponent.seat = 0;
        opponent.kind = MoveKind::Exchange {
            tiles: vec![Tile::Blank],
        };
        game.moves.push(opponent);
        game.moves[1].best = Some(crate::models::BestPlay {
            words: vec!["LEVEL".into(); 3],
            points: 100,
        });
        wallet.rewards.credit(player, &game);
        let studio = wallet.studio();
        assert!(studio.owned.contains("kraken"));
        for id in ["moth", "fox", "junction", "phoenix"] {
            assert!(!studio.owned.contains(id), "{id}");
        }

        let mut crossing = game_for(player, false);
        play_word(&mut crossing, 0, "AT");
        play_at(&mut crossing, 0, (8, 7), "TO", "TO\nAT");
        assert_eq!(crossing.moves[1].words.len(), 3);
        wallet.rewards.credit(player, &crossing);
        let mut palindrome = game_for(player, true);
        play_at(&mut palindrome, 0, (7, 7), "eVE", "EVE");
        palindrome.moves[0].played_on = None;
        wallet.rewards.credit(player, &palindrome);
        for (id, achieved) in [
            ("kraken", "8/8"),
            ("moth", "1/1"),
            ("fox", "1/1"),
            ("junction", "3/3"),
        ] {
            wallet.apply("equip", id).unwrap();
            assert!(wallet.apply("buy", id).is_err());
            assert!(
                wallet
                    .studio()
                    .achievement_rule(id)
                    .unwrap()
                    .ends_with(achieved)
            );
        }
        assert!(!wallet.studio().owned.contains("phoenix"));
    }

    #[test]
    fn word_achievement_boundaries_ignore_unplayed_tiles_and_recounted_games() {
        let player = Uuid::new_v4();
        let mut game = game_for(player, false);
        play_word(&mut game, 0, "AA");
        play_at(&mut game, 0, (8, 8), "TO", "TO\nAT");
        assert_eq!(game.moves[1].words.len(), 2);
        let mut wallet = Wallet::default();
        wallet.rewards.credit(player, &game);
        assert!(!wallet.studio().owned.contains("fox"));
        assert!(!wallet.studio().owned.contains("junction"));
        assert!(
            wallet
                .studio()
                .achievement_rule("junction")
                .unwrap()
                .ends_with("2/3")
        );

        // A saved word can contain J or Z when somebody else supplied the actual tile.
        let mut other = game_for(player, false);
        play_word(&mut other, 1, "JAZZ");
        play_at(&mut other, 0, (7, 11), "Y", "JAZZY");
        wallet.rewards.credit(player, &other);
        let mut rare = game_for(player, false);
        play_at(&mut rare, 0, (7, 7), "JaZz", "JAZZ");
        wallet.rewards.credit(player, &rare);
        wallet.rewards.credit(player, &rare);
        assert!(
            wallet
                .studio()
                .achievement_rule("scarab")
                .unwrap()
                .ends_with("2/4")
        );
        let mut blanks = game_for(player, false);
        play_at(&mut blanks, 0, (7, 7), "qI", "QI");
        wallet.rewards.credit(player, &blanks);
        let mut more = game_for(player, false);
        play_word(&mut more, 0, "OX");
        wallet.rewards.credit(player, &more);
        assert!(
            wallet
                .studio()
                .achievement_rule("scarab")
                .unwrap()
                .ends_with("3/4")
        );
        assert!(wallet.apply("equip", "scarab").is_err());
        let mut last = game_for(player, false);
        play_word(&mut last, 0, "QI");
        wallet.rewards.credit(player, &last);
        wallet.apply("equip", "scarab").unwrap();
        assert!(wallet.apply("buy", "scarab").is_err());

        // A legacy bingo flag alone cannot establish which games it came from.
        let mut bingo = game_for(player, false);
        play_word(&mut bingo, 0, "READING");
        bingo.moves[0].played_on = None;
        let mut snapshot = Rewards::default();
        snapshot.credit(player, &bingo);
        snapshot.credit(player, &bingo);
        assert!(
            wallet
                .studio()
                .achievement_rule("kraken")
                .unwrap()
                .ends_with("5/8")
        );
        wallet.rewards.merge(snapshot);
        assert!(
            wallet
                .studio()
                .achievement_rule("kraken")
                .unwrap()
                .ends_with("7/8")
        );
        assert!(!wallet.studio().owned.contains("kraken"));
        assert!(
            wallet
                .studio()
                .achievement_rule("phoenix")
                .unwrap()
                .ends_with("1/3")
        );

        bingo.id = Uuid::new_v4();
        wallet.rewards.credit(player, &bingo);
        assert!(wallet.apply("equip", "phoenix").is_err());
        let saved = serde_json::to_string(&wallet).unwrap();
        let mut recovered: Wallet = serde_json::from_str(&saved).unwrap();
        let mut third = Rewards::default();
        bingo.id = Uuid::new_v4();
        third.credit(player, &bingo);
        recovered.rewards.merge(third.clone());
        recovered.rewards.merge(third);
        recovered.rewards.merge(Rewards::default());
        for id in ["scarab", "phoenix"] {
            recovered.apply("equip", id).unwrap();
            assert!(recovered.apply("buy", id).is_err());
        }
        assert!(
            recovered
                .studio()
                .achievement_rule("phoenix")
                .unwrap()
                .ends_with("3/3")
        );
        assert!(
            recovered
                .studio()
                .achievement_rule("scarab")
                .unwrap()
                .ends_with("4/4")
        );
        assert_eq!(recovered.studio().balance, wallet.studio().balance);
    }

    #[test]
    fn new_catalog_offers_have_stable_prices_rarities_and_equip_correctly() {
        let mut wallet = Wallet::default();
        for _ in 0..50 {
            wallet
                .rewards
                .finishes
                .insert(Uuid::new_v4(), MatchReward::Human);
        }
        for (id, name, price, rarity) in [
            ("koi", "Koi Pond", 90, "Rare"),
            ("circuit", "Arcade Circuit", 125, "Rare"),
            ("kintsugi", "Kintsugi", 200, "Epic"),
            ("rift", "Event Horizon", 275, "Epic"),
        ] {
            let item = catalog().iter().find(|item| item.id == id).unwrap();
            assert_eq!(
                (item.name, item.price, item.rarity),
                (name, price, Some(rarity))
            );
            assert!(!item.trophy);
            assert!(wallet.apply("starter", id).is_err());
            let before = wallet.studio().balance;
            wallet.apply("buy", id).unwrap();
            wallet.apply("buy", id).unwrap();
            let studio = wallet.studio();
            assert_eq!(studio.balance, before - price);
            assert!(studio.owned.contains(id));
            assert_eq!(studio.tile, id);
        }
        for id in ["serpent", "orrery"] {
            let item = catalog().iter().find(|item| item.id == id).unwrap();
            assert_eq!(item.rarity, Some("Legendary"));
            assert!(item.trophy);
            assert!(wallet.apply("buy", id).is_err());
        }
        assert_eq!(
            catalog()
                .iter()
                .filter(|item| item.rarity.is_some())
                .count(),
            12
        );
    }

    #[test]
    fn retiring_rack_finishes_refunds_only_the_recorded_cost_once() {
        let mut wallet = Wallet::default();
        for _ in 0..30 {
            wallet
                .rewards
                .finishes
                .insert(Uuid::new_v4(), MatchReward::Human);
        }
        let mut saved = serde_json::to_value(wallet).unwrap();
        // Actual historic receipts, including a promotional price for Walnut.
        saved["purchases"] =
            serde_json::json!({"walnut":21,"rosewood":80,"midnight":65,"cherry":50,"glacier":95});
        saved["equipment"] =
            serde_json::json!({"tile":"cherry","rack":"walnut","uniform_board":true});
        let previous_balance = 600 - 21 - 80 - 65 - 50 - 95;
        let mut migrated: Wallet = serde_json::from_value(saved).unwrap();
        assert_eq!(catalog().len(), 24);
        for retired in [
            "maple", "walnut", "rosewood", "midnight", "torii", "reactor",
        ] {
            assert!(catalog().iter().all(|item| item.id != retired));
            assert!(!migrated.studio().owned.contains(retired));
            for action in ["buy", "equip", "starter"] {
                assert!(migrated.apply(action, retired).is_err());
            }
        }
        let studio = migrated.studio();
        assert_eq!(studio.balance, previous_balance + 21 + 80 + 65);
        assert_eq!(studio.tile, "cherry");
        assert!(studio.owned.contains("glacier"));
        assert!(studio.uniform_board);
        let saved = serde_json::to_value(&migrated).unwrap();
        assert!(saved["equipment"].get("rack").is_none());
        assert!(saved["purchases"].get("walnut").is_none());
        assert_eq!(saved["purchases"]["glacier"], 95);
        let mut reloaded: Wallet = serde_json::from_value(saved).unwrap();
        reloaded.apply("buy", "koi").unwrap();
        assert_eq!(reloaded.studio().balance, studio.balance - 90);
    }

    #[test]
    fn old_claims_remain_owned_and_new_accounts_earn_every_alternative() {
        let fresh = Wallet::default();
        assert_eq!(fresh.studio().owned, BTreeSet::from(["classic".into()]));
        for id in ["cobalt", "cherry", "lavender"] {
            assert_eq!(
                catalog().iter().find(|item| item.id == id).unwrap().price,
                50
            );
            let mut fresh = fresh.clone();
            assert!(fresh.apply("starter", id).is_err());
            assert!(fresh.apply("buy", id).is_err());
            for _ in 0..3 {
                fresh
                    .rewards
                    .finishes
                    .insert(Uuid::new_v4(), MatchReward::Human);
            }
            fresh.apply("buy", id).unwrap();
            assert_eq!(fresh.studio().balance, 10);
            assert_eq!(fresh.studio().tile, id);
            let mut legacy: Wallet =
                serde_json::from_value(serde_json::json!({"starter":id})).unwrap();
            assert!(legacy.apply("starter", id).is_err());
            assert!(legacy.apply("starter", "forest").is_err());
            legacy.apply("buy", id).unwrap();
            legacy.apply("equip", id).unwrap();
            assert!(legacy.purchases.is_empty());
            let saved: Wallet =
                serde_json::from_str(&serde_json::to_string(&legacy).unwrap()).unwrap();
            assert_eq!(
                saved.studio().owned,
                BTreeSet::from(["classic".into(), id.into()])
            );
            assert_eq!(saved.studio().tile, id);
        }
        assert!(
            serde_json::to_value(fresh)
                .unwrap()
                .get("starter")
                .is_none()
        );
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
    async fn a_failed_purchase_preserves_balance_and_equipment_for_retry() {
        let dir = std::env::temp_dir().join(format!("screwball-wallet-write-{}", Uuid::new_v4()));
        let store = UserStore::load(&dir).await.unwrap();
        let user = store
            .authentik_profile("wallet-outage", "Player")
            .await
            .unwrap();
        store
            .edit_wallet(user, |wallet| {
                for _ in 0..3 {
                    wallet
                        .rewards
                        .finishes
                        .insert(Uuid::new_v4(), MatchReward::Human);
                }
                Ok(())
            })
            .await
            .unwrap();
        let users_dir = dir.join("users");
        let saved = dir.join("moved");
        tokio::fs::rename(&users_dir, &saved).await.unwrap();
        // An unchanged read succeeds even though the storage directory is unavailable.
        let balance = store
            .edit_wallet(user, |wallet| Ok(wallet.studio().balance))
            .await
            .unwrap();
        assert_eq!(balance, 60);
        assert!(
            store
                .edit_wallet(user, |wallet| wallet.apply("buy", "cherry"))
                .await
                .is_err()
        );
        assert_eq!(store.player_names().await.tile_for_user(user), "classic");
        tokio::fs::rename(&saved, &users_dir).await.unwrap();
        store
            .edit_wallet(user, |wallet| wallet.apply("buy", "cherry"))
            .await
            .unwrap();
        assert_eq!(store.player_names().await.tile_for_user(user), "cherry");
        tokio::fs::remove_dir_all(dir).await.unwrap();
    }

    #[tokio::test]
    async fn purchases_survive_concurrent_requests_and_restarts() {
        let dir = std::env::temp_dir().join(format!("screwball-studio-{}", Uuid::new_v4()));
        let users = UserStore::load(&dir).await.unwrap();
        let user = users
            .authentik_profile("cosmetic", "Shopper")
            .await
            .unwrap();
        let buy = |item| users.edit_wallet(user, move |w| w.apply("buy", item));
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
        assert!(state.uniform_board);
        assert!(state.owned.contains("aurora"));
        assert!(reloaded.player_names().await.uniform_board(user));
        tokio::fs::remove_dir_all(dir).await.unwrap();
    }
}
