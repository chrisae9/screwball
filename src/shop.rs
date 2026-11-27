use std::fmt::Write;

use axum::{
    Form,
    extract::{Query, State},
    response::{Html, IntoResponse, Redirect, Response},
};
use serde::Deserialize;

use crate::{
    app::AppState,
    cosmetics::{self, Item, Studio},
    error::AppError,
    render::{asset, escape, layout_with_head},
    session::AuthUser,
};

#[derive(Default, Deserialize)]
pub struct Selection {
    tab: Option<String>,
    item: Option<String>,
    done: Option<String>,
}

#[derive(Deserialize)]
pub struct Choice {
    action: String,
    item: String,
    #[serde(default)]
    tab: String,
}

pub async fn page(
    State(state): State<AppState>,
    AuthUser(id): AuthUser,
    Query(q): Query<Selection>,
) -> Result<Html<String>, AppError> {
    let studio = cosmetics::studio(&state, id).await?;
    Ok(Html(render(&studio, &q, None)?))
}

pub async fn update(
    State(state): State<AppState>,
    AuthUser(id): AuthUser,
    Form(form): Form<Choice>,
) -> Result<Response, AppError> {
    let collection = form.tab == "collection";
    let item = if form.action == "uniform" {
        None
    } else {
        Some(find(&form.item).ok_or_else(|| AppError::not_found("This finish is unavailable."))?)
    };
    match cosmetics::change(&state, id, &form.action, &form.item).await {
        Ok(_) => {
            let mut url = if collection {
                "/shop?tab=collection".to_owned()
            } else {
                "/shop?tab=shop".to_owned()
            };
            if let Some(item) = item {
                write!(url, "&item={}", item.id).unwrap();
            }
            let result = if form.action == "uniform" {
                "saved"
            } else {
                "equipped"
            };
            write!(url, "&done={result}").unwrap();
            Ok(Redirect::to(&url).into_response())
        }
        Err(err @ (AppError::BadRequest(_) | AppError::Conflict(_))) => {
            let q = Selection {
                tab: Some(form.tab),
                item: item.map(|i| i.id.to_string()),
                done: None,
            };
            let studio = cosmetics::studio(&state, id).await?;
            Ok((
                err.status_code(),
                Html(render(&studio, &q, Some(err.detail()))?),
            )
                .into_response())
        }
        Err(err) => Err(err),
    }
}

fn find(id: &str) -> Option<&'static Item> {
    cosmetics::catalog().iter().find(|i| i.id == id)
}

fn detail_url(item: &Item, collection: bool) -> String {
    if collection {
        format!("/shop?tab=collection&amp;item={}", item.id)
    } else {
        format!("/shop?item={}", item.id)
    }
}

fn sample_word(id: &str) -> &'static str {
    match id {
        "classic" => "OAK",
        "cobalt" => "ARC",
        "cherry" => "RED",
        "lavender" => "IRIS",
        "forest" => "ELM",
        "amber" => "SUN",
        "slate" => "ASH",
        "glacier" => "ICE",
        "obsidian" => "ONYX",
        "solar" => "FLARE",
        "aurora" => "SKY",
        "bingo" => "ACE",
        "koi" => "KOI",
        "circuit" => "BYTE",
        "kintsugi" => "GILT",
        "rift" => "RIFT",
        "serpent" => "FANG",
        "orrery" => "NOVA",
        "moth" => "MOTH",
        "junction" => "GRID",
        "kraken" => "INK",
        "fox" => "EYE",
        "scarab" => "JADE",
        "phoenix" => "ASH",
        "kirby" => "STAR",
        "jett" => "GUST",
        "sage" => "JADE",
        "volleyball" => "ACE",
        "tracer" => "TIME",
        "dva" => "MECH",
        "calico" => "PURR",
        "tuxedo" => "TUX",
        "mooncat" => "MOON",
        _ => "ACE",
    }
}

fn tiles(word: &str, finish: &str) -> String {
    word.chars().map(|c| format!(r#"<span class="sample-tile" data-tile-style="{finish}">{c}<small>{}</small></span>"#,crate::models::letter_points(c))).collect()
}

fn specimen(item: &Item) -> String {
    let letters = tiles(sample_word(item.id), item.id);
    format!(
        r#"<div class="specimen" aria-hidden="true"><span class="sample-word">{letters}</span></div>"#
    )
}

fn equipped(item: &Item, studio: &Studio) -> bool {
    item.id == studio.tile
}

fn achievement_rule(item: &Item, s: &Studio) -> Option<String> {
    match item.id {
        "aurora" => Some(format!(
            "Finish 5 multiplayer games · {}/5",
            s.human_finishes.min(5)
        )),
        "serpent" => Some(format!("Score 75 in a turn · Best {}", s.best_play)),
        "orrery" => Some(format!("Play on 14 days · {}/14", s.daily_words.min(14))),
        "bingo" => Some(format!(
            "Play all 7 tiles in one turn · {}/1",
            usize::from(s.bingo)
        )),
        _ => s.achievement_rule(item.id),
    }
}

fn unlock_text(item: &Item, s: &Studio) -> String {
    let status = if equipped(item, s) {
        Some("Equipped")
    } else if s.owned.contains(item.id) {
        Some("Owned")
    } else {
        None
    };
    match (status, achievement_rule(item, s)) {
        (Some(status), Some(rule)) => format!(r#"<b class="finish-owned">{status}</b>{rule}"#),
        (None, Some(rule)) => rule,
        (Some(status), None) => status.to_owned(),
        (None, None) => format!("{} coins", item.price),
    }
}

fn card(item: &Item, s: &Studio, collection: bool) -> String {
    let rarity = item.rarity.unwrap_or("");
    let label = item.rarity.map_or(String::new(), |tier| {
        format!(r#"<span class="finish-rarity">{tier}</span>"#)
    });
    format!(
        r#"<a class="shop-card" data-rarity="{rarity}" href="{}">{}{label}<strong>{}</strong><small>{}</small></a>"#,
        detail_url(item, collection),
        specimen(item),
        item.name,
        unlock_text(item, s)
    )
}

fn form(action: &str, item: &str, collection: bool, label: &str) -> String {
    format!(
        r#"<form method="post" action="/shop"><input type="hidden" name="action" value="{action}"><input type="hidden" name="item" value="{item}"><input type="hidden" name="tab" value="{}"><button class="shop-buy">{label}</button></form>"#,
        if collection { "collection" } else { "shop" }
    )
}

fn earn(s: &Studio) -> String {
    format!(
        r#"<details class="shop-earn"><summary>How to earn coins</summary><dl><div><dt>First word each day <small>Resets at midnight UTC</small></dt><dd>+5</dd></div><div><dt>Finish a multiplayer game</dt><dd>+20</dd></div><div><dt>Finish a game with a bot</dt><dd>+10</dd></div></dl><p>Finish rewards count games with a scored word and final scores. Your record: {} multiplayer finishes · {} bot finishes.</p></details>"#,
        s.human_finishes, s.bot_finishes
    )
}

// Show how each finish looks on ordinary tiles, a blank, the last play and a draft.
fn board_preview(item: &Item) -> String {
    let style = item.id;
    let mut cells = String::new();
    for i in 0..15 {
        match i {
            5..=9 => {
                let c=b"FROST"[i-5] as char;
                let points=if i==8 {0} else {crate::models::letter_points(c)};
                let class=match i {8=>" is-blank",9=>" is-last",_=>""};
                write!(cells,r#"<span class="sample-tile{class}" data-tile-style="{style}">{c}<small>{points}</small></span>"#).unwrap();
            }
            12 => write!(cells,r#"<span class="sample-tile is-draft" data-tile-style="{style}">N<small>1</small></span>"#).unwrap(),
            0|14 => cells.push_str(r#"<span class="preview-cell is-dl">DL</span>"#),
            4 => cells.push_str(r#"<span class="preview-cell is-dw">DW</span>"#),
            _ => cells.push_str(r#"<span class="preview-cell"></span>"#),
        }
    }
    let closeup = if item.rarity.is_some() {
        format!(
            r#"<div class="finish-closeup" aria-hidden="true">{}</div>"#,
            tiles("Q", style)
        )
    } else {
        String::new()
    };
    format!(
        r#"<figure class="board-specimen" aria-label="Tile and board preview">{closeup}<div class="mini-board" aria-hidden="true">{cells}</div></figure>"#
    )
}

fn detail(item: &Item, s: &Studio, collection: bool) -> String {
    let (return_url, return_label) = if collection {
        ("/shop?tab=collection", "My collection")
    } else if item.trophy {
        ("/shop#achievements", "Achievements")
    } else {
        ("/shop", "Tile Shop")
    };
    let category = match (item.rarity, item.trophy) {
        (Some(rarity), true) => format!("{rarity} achievement"),
        (Some(rarity), false) => format!("{rarity} tile set"),
        (None, true) => "Achievement".into(),
        (None, false) => "Tile set".into(),
    };
    let rule = achievement_rule(item, s)
        .map(|text| format!(r#"<p class="unlock-rule">{text}</p>"#))
        .unwrap_or_default();
    let action = if equipped(item, s) {
        r#"<span class="equipped-label">✓ Equipped</span>"#.into()
    } else if s.owned.contains(item.id) {
        form("equip", item.id, collection, "Equip")
    } else if item.trophy {
        String::new()
    } else if s.balance >= item.price {
        form(
            "buy",
            item.id,
            collection,
            &format!("Unlock &amp; equip · {} coins", item.price),
        )
    } else {
        format!(
            r#"<p class="unlock-rule">{price} coins <span>Earn {} more to unlock</span></p>"#,
            item.price - s.balance,
            price = item.price
        )
    };
    format!(
        r#"<a class="shop-return" href="{return_url}">‹ {return_label}</a><div class="finish-detail"><div class="finish-info"><small>{category}</small><h1>{name}</h1>{rule}{action}</div>{preview}</div>{earn}"#,
        name = item.name,
        preview = board_preview(item),
        earn = if item.trophy { String::new() } else { earn(s) }
    )
}

fn inventory(s: &Studio, collection: bool) -> String {
    let mut out = String::new();
    if !collection {
        let feature = find("kirby").expect("featured achievement in catalog");
        write!(out, r##"<a class="shop-feature" href="#achievements"><div><small>Featured achievement</small><h2>{}</h2><span>Explore achievements ›</span></div>{}</a>"##, feature.name, specimen(feature)).unwrap();
    }
    out.push_str(&earn(s));
    let catalog = cosmetics::catalog();
    let trophies = catalog.iter().filter(|item| item.trophy).count();
    let earned = catalog
        .iter()
        .filter(|item| item.trophy && s.owned.contains(item.id))
        .count();
    for title in ["Achievements", "Rare finds", "Tile sets"] {
        let achievements = title == "Achievements";
        let list: String = catalog
            .iter()
            .filter(|item| {
                let shelf = if item.trophy {
                    "Achievements"
                } else if item.rarity.is_some() {
                    "Rare finds"
                } else {
                    "Tile sets"
                };
                shelf == title && (!collection || s.owned.contains(item.id))
            })
            .map(|i| card(i, s, collection))
            .collect();
        if list.is_empty() && !achievements {
            continue;
        }
        if achievements {
            write!(out, r#"<section class="shop-shelf" id="achievements"><header class="shop-shelf-heading"><h2>{title}</h2><span>{earned} of {trophies} earned</span></header>"#).unwrap();
        } else {
            write!(out, r#"<section class="shop-shelf"><h2>{title}</h2>"#).unwrap();
        }
        if list.is_empty() {
            out.push_str(r#"<a class="shop-subtle-link" href="/shop#achievements">Explore achievements ›</a>"#);
        } else {
            write!(out, r#"<div class="shop-grid">{list}</div>"#).unwrap();
        }
        out.push_str("</section>");
    }
    if collection {
        let (enabled, value, action) = if s.uniform_board {
            ("On", "off", "Use player colors")
        } else {
            ("Off", "on", "Use Classic on board")
        };
        write!(out,r#"<section class="classic-choice"><div><h2>Classic board</h2><p>{enabled} · Show every board tile in Classic.</p></div>{}</section>"#,form("uniform",value,true,action)).unwrap();
    }
    out
}

fn render(s: &Studio, q: &Selection, error: Option<&str>) -> Result<String, AppError> {
    let collection = q.tab.as_deref() == Some("collection");
    let selected = q
        .item
        .as_deref()
        .map(|v| find(v).ok_or_else(|| AppError::not_found("This finish is unavailable.")))
        .transpose()?;
    let flash = if let Some(e) = error {
        format!(r#"<p class="shop-feedback" role="alert">{}</p>"#, escape(e))
    } else if q.done.as_deref() == Some("equipped") && selected.is_some_and(|i| equipped(i, s)) {
        r#"<p class="shop-feedback" role="status">Equipped for your games.</p>"#.into()
    } else if q.done.as_deref() == Some("saved") {
        r#"<p class="shop-feedback" role="status">Board preference saved.</p>"#.into()
    } else {
        String::new()
    };
    let balance = format!(
        r#"<span class="coin-balance"><span class="coin" aria-hidden="true">◆</span> {} coins</span>"#,
        s.balance
    );
    let body = if let Some(item) = selected {
        format!(
            r#"<div class="shop detail-page">{balance}{flash}{}</div>"#,
            detail(item, s, collection)
        )
    } else {
        let current = |active| {
            if active {
                r#" aria-current="page""#
            } else {
                ""
            }
        };
        format!(
            r#"<div class="shop"><header class="shop-heading"><h1>Tile Shop</h1>{balance}</header><nav class="shop-tabs" aria-label="Shop views"><a href="/shop"{}>Shop</a><a href="/shop?tab=collection"{}>My collection</a></nav>{flash}{}</div>"#,
            current(!collection),
            current(collection),
            inventory(s, collection)
        )
    };
    let body = format!(
        r#"{body}<footer class="shop shop-credits"><a class="shop-subtle-link" href="/public/finishes/credits.html">Artwork credits</a></footer>"#
    );
    let head = format!(
        r#"<link rel="stylesheet" href="{}">"#,
        asset("/public/shop.css")
    );
    Ok(layout_with_head(
        "Tile Shop — Scrabble",
        &body,
        &head,
        "shop-page",
        "",
    ))
}
