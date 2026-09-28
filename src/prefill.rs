//! What the planner form starts from: an empty loadout when composing, the
//! stored one when editing. Flattened here so the template reads one shape
//! either way instead of branching on an Option in a dozen places.

use crate::builds::BuildCard;
use crate::db::Card;

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
    /// the stored video link, prefilled on edit; empty on create
    pub youtube_url: String,
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
        i64::from(self.blessed)
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
    pub const fn filled(&self) -> bool {
        !self.name.is_empty()
    }
}

impl Prefill {
    /// An empty form.
    pub fn blank() -> Self {
        Self {
            treasures: vec![
                TreasureSlot {
                    level: 9,
                    ..Default::default()
                };
                3
            ],
            ..Default::default()
        }
    }

    /// The form as an existing build left it.
    pub fn from_build(b: &BuildCard) -> Self {
        let mut treasures: Vec<TreasureSlot> = (0..3)
            .map(|i| {
                b.treasures.get(i).map_or_else(
                    || TreasureSlot {
                        level: 9,
                        ..Default::default()
                    },
                    |t| TreasureSlot {
                        id: t.id,
                        name: t.name.clone(),
                        image: t.image.clone(),
                        level: b.treasure_levels.get(i).copied().unwrap_or(9),
                        blessed: b.treasure_blessed.get(i).copied().unwrap_or(false),
                    },
                )
            })
            .collect();
        treasures.truncate(3);

        let (cookie, cookie_name, cookie_image) = Self::slot_parts(b.cookie.as_ref());
        let (relay, relay_name, relay_image) = Self::slot_parts(b.cookie2.as_ref());
        let (pet, pet_name, pet_image) = Self::slot_parts(b.pet.as_ref());
        Self {
            cookie,
            cookie_name,
            cookie_image,
            cookie2: relay,
            cookie2_name: relay_name,
            cookie2_image: relay_image,
            pet,
            pet_name,
            pet_image,
            treasures,
            score: b.score,
            coin: b.coin,
            time_ms: b.time_ms,
            description: b.description.clone(),
            youtube_url: b.youtube_url.clone(),
        }
    }

    /// (id, name, image) out of an optional card, blank when absent.
    fn slot_parts(card: Option<&Card>) -> (i64, String, Option<String>) {
        card.map_or_else(
            || (0, String::new(), None),
            |c| (c.id, c.name.clone(), c.image.clone()),
        )
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
