//! The player icon catalog is shared by the account form and the artwork atlas.
use std::{fmt::Write, sync::LazyLock};

use serde::Deserialize;

pub const COUNT: u8 = 50;

#[derive(Deserialize)]
struct Icon {
    id: u8,
    name: String,
}

static ICONS: LazyLock<Vec<Icon>> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../public/player-icons.json"))
        .expect("the bundled player icon catalog must be valid JSON")
});

pub fn image(id: u8) -> String {
    format!(
        r#"<span class="player-avatar" aria-hidden="true" data-avatar-id="{id}" style="--avatar-col:{};--avatar-row:{}"></span>"#,
        id % 10,
        id / 10
    )
}

pub fn chooser(selected: u8) -> String {
    let mut choices = String::new();
    for icon in ICONS.iter() {
        let name = crate::render::escape(&icon.name);
        let checked = if icon.id == selected { " checked" } else { "" };
        let _ = write!(
            choices,
            r#"<label class="avatar-choice" title="{name}"><input type="radio" name="avatar_id" value="{}" aria-label="{name}"{checked}>{}</label>"#,
            icon.id,
            image(icon.id)
        );
    }
    let name = crate::render::escape(&ICONS[usize::from(selected)].name);
    format!(
        r#"<details class="avatar-picker">
<summary>{current}<span>Player icon<small>{name}</small></span><span class="avatar-change">Change</span></summary>
<form method="post" action="/settings/avatar"><fieldset class="avatar-grid" aria-label="Choose your player icon">{choices}</fieldset>
<div class="avatar-controls"><output class="avatar-selected" aria-live="polite">{name}</output><button type="submit" class="button">Save player icon</button></div>
</form></details>"#,
        current = image(selected),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_names_match_the_atlas_cells_in_order() {
        assert_eq!(ICONS.len(), usize::from(COUNT));
        let mut names = std::collections::HashSet::new();
        for (index, icon) in ICONS.iter().enumerate() {
            assert_eq!(usize::from(icon.id), index);
            assert!(!icon.name.trim().is_empty());
            assert!(names.insert(&icon.name));
        }
    }
}
