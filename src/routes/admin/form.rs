//! The admin form's shape and its gates: the template struct the page renders
//! through, the submitted entity form, and the two checks that stand between a
//! visitor and the editor.

use askama::Template;
use serde::Deserialize;

use crate::ctx::Ctx;
use crate::db::{self, CombiEditRow, Detail};
use crate::section::Section;

#[derive(Template)]
#[template(path = "admin_form.html")]
pub(super) struct AdminForm {
    pub(super) ctx: Ctx,
    /// cookies, pets or treasures
    pub(super) section: String,
    /// filled when editing
    pub(super) item: Option<Detail>,
    /// the combo pairings this entity takes part in; empty for a treasure
    pub(super) combi: Vec<CombiEditRow>,
    pub(super) error: String,
    /// which locale's translation the form shows and saves; the old V
    /// editor's language select, restored
    pub(super) edit_lang: String,
    /// the description rendered for the editing locale, the initial content
    /// of the preview box
    pub(super) preview_html: String,
    /// id+name options for the treasure form's link selects
    pub(super) cookie_opts: Vec<IdName>,
    pub(super) pet_opts: Vec<IdName>,
    pub(super) treasure_opts: Vec<IdName>,
    /// the edited treasure's stored link ids (zeros when none / not a
    /// treasure) so the selects preselect them
    pub(super) links: db::TreasureEditLinks,
}

impl AdminForm {
    /// The stored base-treasure id, for the select's `selected` mark.
    pub const fn base_treasure_id(&self) -> i64 {
        self.links.base
    }
    pub const fn unlock_cookie_id(&self) -> i64 {
        self.links.unlock_cookie
    }
    pub const fn unlock_pet_id(&self) -> i64 {
        self.links.unlock_pet
    }
}

/// One option row for the treasure form's link selects.
pub(super) struct IdName {
    pub(super) id: i64,
    pub(super) name: String,
}

#[derive(Debug, Default, Deserialize)]
pub struct EntityForm {
    pub name: String,
    #[serde(default)]
    pub abilities: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub power_plus: String,
    #[serde(default)]
    pub power_plus_requirement: String,
    #[serde(default)]
    pub unlock_goal: String,
    #[serde(default)]
    pub image: String,
    pub grade: Option<i64>,
    /// YYYY-MM-DD as the date input sends it; empty means "keep/now"
    #[serde(default)]
    pub release_date: String,
    /// treasures only
    #[serde(default)]
    pub is_evolved: Option<String>,
    #[serde(default)]
    pub is_power_plus: Option<String>,
    #[serde(default)]
    pub base_treasure_id: Option<i64>,
    #[serde(default)]
    pub unlock_cookie_id: Option<i64>,
    #[serde(default)]
    pub unlock_pet_id: Option<i64>,
    /// which locale's translation this submission writes; empty falls back
    /// to the viewer's language
    #[serde(default)]
    pub lang: String,
}

impl AdminForm {
    /// The `.tr` keys are per-section (`save_cookie_button`), so the template
    /// builds none of them itself. A method because askama calls it on the
    /// context struct.
    fn singular(&self) -> &'static str {
        Section::parse(&self.section).map_or("cookie", Section::singular)
    }

    /// True when the section's rows carry a wiki grade, so the form renders
    /// the grade strip and date input.
    fn graded(&self) -> bool {
        Section::parse(&self.section).is_some_and(Section::graded)
    }

    /// True when this is the treasure form, with its evolution/link fields.
    fn is_treasure(&self) -> bool {
        self.section == "treasures"
    }

    /// The grade ordinals the radio strip offers, lowest to highest with the
    /// Extra grade last, exactly the old form's order.
    #[allow(clippy::unused_self)]
    fn grade_values(&self) -> Vec<i64> {
        vec![1, 2, 3, 4, 5, 6, 0]
    }

    /// A grade ordinal's display label ("S+" for `s_plus`).
    #[allow(clippy::unused_self, clippy::trivially_copy_pass_by_ref)]
    // askama passes template arguments by reference, so the parameter has
    // to stay `&i64` even though i64 is Copy
    fn grade_name(&self, g: &i64) -> String {
        crate::grade::label(*g)
    }

    /// A grade ordinal's image slug for the radio strip.
    #[allow(clippy::unused_self, clippy::trivially_copy_pass_by_ref)]
    // askama passes template arguments by reference, so the parameter has
    // to stay `&i64` even though i64 is Copy
    fn grade_slug(&self, g: &i64) -> String {
        crate::grade::slug(*g).to_string()
    }

    fn title_key(&self) -> String {
        let verb = if self.item.is_some() { "edit" } else { "new" };
        format!("{verb}_{}_page_header", self.singular())
    }

    fn save_key(&self) -> String {
        let verb = if self.item.is_some() {
            "save"
        } else {
            "create"
        };
        format!("{verb}_{}_button", self.singular())
    }

    fn name_key(&self) -> String {
        format!("{}_name", self.singular())
    }
}

/// The admin editor writes exactly the editable sections; everything else is
/// a 404 that does not confirm existence. One gate, at the typed edge.
pub(super) fn editable(section: &str) -> Option<Section> {
    Section::parse(section).filter(|s| s.editable())
}

/// The locale the submitted form edits: the form's own `lang` field when it
/// is a loaded locale (the select the template renders), else the viewer's
/// language. Validated so an unknown value cannot create a rogue `tr.xx`.
pub(super) fn form_lang(form: &EntityForm, ctx: &Ctx) -> String {
    if crate::i18n::is_available(&form.lang) {
        form.lang.clone()
    } else {
        ctx.lang.clone()
    }
}
