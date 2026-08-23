//! What the planner form starts from: an empty loadout when composing, the
//! stored one when editing. Flattened here so the template reads one shape
//! either way instead of branching on an Option in a dozen places.

use crate::builds::BuildCard;

#[derive(Debug, Default)]
pub struct Prefill {
    /// 0 when the slot is empty, which is also what the hidden input carries
    pub cookie: i64,
    pub cookie2: i64,
    pub pet: i64,
    pub cookie_name: String,
    pub cookie_image: Option<String>,
    pub cookie2_name: String,
    pub cookie2_image: Option<String>,
    pub pet_name: String,
    pub pet_image: Option<String>,
    /// always three, padded with empties so the slots always render
    pub treasures: Vec<TreasureSlot>,
    pub score: i64,
    pub coin: i64,
    pub time_ms: i64,
    pub description: String,
}

#[derive(Debug, Default, Clone)]
pub struct TreasureSlot {
    pub id: i64,
    pub name: String,
    pub image: Option<String>,
    pub level: i64,
    pub blessed: bool,
}

impl TreasureSlot {
    /// The hidden input wants 1/0, not true/false.
    pub fn blessed_int(&self) -> i64 {
        self.blessed as i64
    }
}

/// One cookie or pet slot, with the opener the button calls.
pub struct EntitySlot {
    pub id: &'static str,
    pub section: &'static str,
    pub label: &'static str,
    pub opener: &'static str,
    pub name: String,
    pub image: Option<String>,
}

impl EntitySlot {
    pub fn filled(&self) -> bool {
        !self.name.is_empty()
    }
}

impl Prefill {
    /// An empty form.
    pub fn blank() -> Self {
        Self {
            treasures: vec![TreasureSlot { level: 9, ..Default::default() }; 3],
            ..Default::default()
        }
    }

    /// The form as an existing build left it.
    pub fn from_build(b: &BuildCard) -> Self {
        let mut treasures: Vec<TreasureSlot> = (0..3)
            .map(|i| match b.treasures.get(i) {
                Some(t) => TreasureSlot {
                    id: t.id,
                    name: t.name.clone(),
                    image: t.image.clone(),
                    level: b.treasure_levels.get(i).copied().unwrap_or(9),
                    blessed: b.treasure_blessed.get(i).copied().unwrap_or(false),
                },
                None => TreasureSlot { level: 9, ..Default::default() },
            })
            .collect();
        treasures.truncate(3);

        Self {
            cookie: b.cookie.as_ref().map(|c| c.id).unwrap_or(0),
            cookie_name: b.cookie.as_ref().map(|c| c.name.clone()).unwrap_or_default(),
            cookie_image: b.cookie.as_ref().and_then(|c| c.image.clone()),
            cookie2: b.cookie2.as_ref().map(|c| c.id).unwrap_or(0),
            cookie2_name: b.cookie2.as_ref().map(|c| c.name.clone()).unwrap_or_default(),
            cookie2_image: b.cookie2.as_ref().and_then(|c| c.image.clone()),
            pet: b.pet.as_ref().map(|p| p.id).unwrap_or(0),
            pet_name: b.pet.as_ref().map(|p| p.name.clone()).unwrap_or_default(),
            pet_image: b.pet.as_ref().and_then(|p| p.image.clone()),
            treasures,
            score: b.score,
            coin: b.coin,
            time_ms: b.time_ms,
            description: b.description.clone(),
        }
    }

    /// The three entity slots in display order.
    pub fn entity_slots(&self) -> Vec<EntitySlot> {
        vec![
            EntitySlot {
                id: "cookie",
                section: "cookies",
                label: "build_cookie_label",
                opener: "openCookie('cookie')",
                name: self.cookie_name.clone(),
                image: self.cookie_image.clone(),
            },
            EntitySlot {
                id: "cookie2",
                section: "cookies",
                label: "build_relay_label",
                opener: "openCookie('cookie2')",
                name: self.cookie2_name.clone(),
                image: self.cookie2_image.clone(),
            },
            EntitySlot {
                id: "pet",
                section: "pets",
                label: "build_pet_label",
                opener: "openPicker('pet')",
                name: self.pet_name.clone(),
                image: self.pet_image.clone(),
            },
        ]
    }

    /// A zero reads as "not set" in the form, so it renders blank rather than
    /// putting a literal 0 in the box.
    pub fn score_value(&self) -> String {
        blank_if_zero(self.score)
    }
    pub fn coin_value(&self) -> String {
        blank_if_zero(self.coin)
    }
    /// The column is milliseconds, the form is seconds.
    pub fn time_value(&self) -> String {
        blank_if_zero(self.time_ms / 1000)
    }
}

fn blank_if_zero(n: i64) -> String {
    if n > 0 {
        n.to_string()
    } else {
        String::new()
    }
}
