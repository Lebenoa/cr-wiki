//! The labels and class strings the catalog grids read off the context: grade
//! badges, episode numbers, star ratings and scores. Askama hands template
//! arguments out by reference, which is why the numbers arrive as `&i64` and
//! the grades as `&Option<i64>` rather than by value.

use super::Ctx;
impl Ctx {
    /// The draw pool's display name. The `name` column holds a .tr key
    /// rather than prose, so the tier is what actually reads.
    pub fn gacha_tier_label(&self, tier: &str) -> String {
        self.l.t(&format!("gacha_tier_{tier}"))
    }

    /// Localizes an episode kind code (story/special/event).
    pub fn episode_kind_label(&self, kind: &str) -> String {
        self.l.t(&format!("episode_kind_{kind}"))
    }

    /// An episode's difficulty as repeated stars ('' when unrated).
    #[allow(clippy::unused_self, clippy::trivially_copy_pass_by_ref)]
    // askama passes template arguments by reference, so the parameter has
    // to stay `&i64` even though i64 is Copy
    pub fn stars_label(&self, stars: &i64) -> String {
        "\u{2605}".repeat(usize::try_from((*stars).max(0)).unwrap_or(0))
    }

    /// A stored grade value as its label ("S+" for `s_plus`), or '' when the
    /// row has no grade. The ungraded grids (ingredients, skins) show it.
    #[allow(clippy::unused_self, clippy::ref_option)]
    // askama passes template arguments by reference: `&Option<T>`, not the
    // `Option<&T>` the lint prefers
    pub fn grade_label_int(&self, grade: &Option<i64>) -> String {
        (*grade).map_or_else(String::new, crate::grade::label)
    }

    /// The full badge class list for a stored grade value, so every grade
    /// reads as its own colour on the ungraded grids. Wind4's palette is
    /// replaced by the semantic block in uno.config.ts, so the colors these
    /// name are declared there.
    #[allow(clippy::unused_self, clippy::ref_option)]
    // askama passes template arguments by reference: `&Option<T>`, not the
    // `Option<&T>` the lint prefers
    pub fn grade_badge_cls(&self, grade: &Option<i64>) -> &'static str {
        match grade.unwrap_or(-1) {
            1 => "text-[10px] font-label uppercase rounded-full border px-2 py-0.5 border-lime-400 text-lime-400",
            2 => "text-[10px] font-label uppercase rounded-full border px-2 py-0.5 border-yellow-400 text-yellow-400",
            3 => "text-[10px] font-label uppercase rounded-full border px-2 py-0.5 border-orange-400 text-orange-400",
            4 => "text-[10px] font-label uppercase rounded-full border px-2 py-0.5 border-red-400 text-red-400",
            5 => "text-[10px] font-label uppercase rounded-full border px-2 py-0.5 border-fuchsia-400 text-fuchsia-400",
            6 => "text-[10px] font-label uppercase rounded-full border px-2 py-0.5 border-violet-400 text-violet-400",
            _ => "text-[10px] font-label uppercase rounded-full border px-2 py-0.5 border-secondary/40 text-foreground-muted",
        }
    }

    /// An episode id as the short badge the ingredient cards and the
    /// An episode id as the short badge the ingredient cards and the
    /// drop-location tile use: "EP 2" story, "SP 1" special (501+), "EP 6-1"
    /// event (601+). Empty for none, so a template can test it directly.
    // The subtraction is range-checked just above it; the div/mod by the
    // constant 100 cannot fault.
    #[allow(clippy::arithmetic_side_effects)]
    #[allow(clippy::ref_option)]
    // askama passes template arguments by reference: `&Option<T>`, not the
    // `Option<&T>` the lint prefers
    pub fn episode_short(&self, id: &Option<i64>) -> String {
        let Some(eid) = *id else {
            return String::new();
        };
        if (500..600).contains(&eid) {
            return format!("{} {}", self.l.t("special_episode_abbrev"), eid - 500);
        }
        if eid >= 600 {
            return format!("{} {}-{}", self.l.t("episode_abbrev"), eid / 100, eid % 100);
        }
        format!("{} {eid}", self.l.t("episode_abbrev"))
    }

    /// The badge class list for an episode id, so each episode reads as its
    /// own colour on the ingredient cards. Story episodes run cool-to-warm
    /// in play order, the special episodes take the remaining cool hues and
    /// the event ones the reds.
    #[allow(clippy::unused_self, clippy::ref_option)]
    // askama passes template arguments by reference: `&Option<T>`, not the
    // `Option<&T>` the lint prefers
    pub fn episode_badge_cls(&self, id: &Option<i64>) -> &'static str {
        match id.unwrap_or(0) {
            1 => "text-[10px] font-label uppercase rounded-full border px-2 py-0.5 border-emerald-400 text-emerald-400",
            2 => "text-[10px] font-label uppercase rounded-full border px-2 py-0.5 border-lime-400 text-lime-400",
            3 => "text-[10px] font-label uppercase rounded-full border px-2 py-0.5 border-orange-400 text-orange-400",
            4 => "text-[10px] font-label uppercase rounded-full border px-2 py-0.5 border-sky-400 text-sky-400",
            5 => "text-[10px] font-label uppercase rounded-full border px-2 py-0.5 border-pink-400 text-pink-400",
            6 => "text-[10px] font-label uppercase rounded-full border px-2 py-0.5 border-fuchsia-400 text-fuchsia-400",
            7 => "text-[10px] font-label uppercase rounded-full border px-2 py-0.5 border-amber-400 text-amber-400",
            501 => "text-[10px] font-label uppercase rounded-full border px-2 py-0.5 border-yellow-400 text-yellow-400",
            502 => "text-[10px] font-label uppercase rounded-full border px-2 py-0.5 border-cyan-400 text-cyan-400",
            503 => "text-[10px] font-label uppercase rounded-full border px-2 py-0.5 border-violet-400 text-violet-400",
            601 => "text-[10px] font-label uppercase rounded-full border px-2 py-0.5 border-teal-400 text-teal-400",
            602 => "text-[10px] font-label uppercase rounded-full border px-2 py-0.5 border-indigo-400 text-indigo-400",
            701 => "text-[10px] font-label uppercase rounded-full border px-2 py-0.5 border-red-400 text-red-400",
            _ => "text-[10px] font-label uppercase rounded-full border px-2 py-0.5 border-secondary/40 text-foreground-muted",
        }
    }

    /// The same palette as `episode_badge_cls` without the pill, for the
    /// drop-location tile where the number sits inline before the name.
    #[allow(clippy::unused_self, clippy::ref_option)]
    // askama passes template arguments by reference: `&Option<T>`, not the
    // `Option<&T>` the lint prefers
    pub fn episode_text_cls(&self, id: &Option<i64>) -> &'static str {
        match id.unwrap_or(0) {
            1 => "text-emerald-400",
            2 => "text-lime-400",
            3 => "text-orange-400",
            4 => "text-sky-400",
            5 => "text-pink-400",
            6 => "text-fuchsia-400",
            7 => "text-amber-400",
            501 => "text-yellow-400",
            502 => "text-cyan-400",
            503 => "text-violet-400",
            601 => "text-teal-400",
            602 => "text-indigo-400",
            701 => "text-red-400",
            _ => "text-accent",
        }
    }

    /// A jelly's score value without trailing zeroes (432.9 -> "432.9",
    /// 5000 -> "5000"). Formatted, then trimmed: no float comparisons or
    /// casts, and Display for f64 never prints "5000.0" anyway.
    #[allow(clippy::unused_self, clippy::trivially_copy_pass_by_ref)]
    // askama passes template arguments by reference, so the parameter has
    // to stay `&f64`
    pub fn score_label(&self, score: &f64) -> String {
        let mut s = format!("{score}");
        if s.contains('.') {
            s = s.trim_end_matches('0').trim_end_matches('.').to_string();
        }
        s
    }

    /// The grade-filter buttons a tabbed catalog shows: (slug, label) in
    /// declaration order. The filter JS keys on the data-grade slug.
    #[allow(clippy::unused_self)]
    pub fn grade_options(&self) -> Vec<(&'static str, &'static str)> {
        [
            ("c", "C"),
            ("b", "B"),
            ("a", "A"),
            ("s", "S"),
            ("s_plus", "S+"),
            ("l", "L"),
            ("e", "E"),
        ]
        .into_iter()
        .collect()
    }
}
