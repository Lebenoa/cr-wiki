//! The navbar, theme and EP lists as the templates ask for them. `crate::chrome`
//! owns the lists and the class strings and knows nothing about requests;
//! these methods are the context half, so askama can call them on `ctx`.

use super::Ctx;
impl Ctx {
    /// The sections behind the Wiki dropdown, in navbar order.
    #[allow(clippy::unused_self)]
    pub fn wiki_sections(&self) -> Vec<&'static str> {
        crate::chrome::NAV_SECTIONS
            .iter()
            .map(|s| s.as_str())
            .collect()
    }

    #[allow(dead_code)] // used through templates, invisible to rustc
    pub fn nav_section_active(&self, section: &str) -> bool {
        crate::chrome::nav_section_active(&self.path, section)
    }

    pub fn nav_class(&self, section: &str) -> &'static str {
        crate::chrome::nav_class(&self.path, section)
    }

    /// True when the current page sits under any wiki section, so the
    /// dropdown trigger can carry the active styling its entries would.
    #[allow(dead_code)] // used through templates, invisible to rustc
    pub fn wiki_active(&self) -> bool {
        crate::chrome::wiki_active(&self.path)
    }

    pub fn wiki_class(&self) -> &'static str {
        crate::chrome::wiki_class(&self.path)
    }

    /// The theme names the picker offers.
    #[allow(clippy::unused_self)]
    pub fn themes(&self) -> Vec<&'static str> {
        crate::chrome::THEMES.to_vec()
    }

    /// The palette tokens the custom-theme editor may override.
    #[allow(clippy::unused_self)]
    pub fn theme_tokens(&self) -> Vec<&'static str> {
        crate::chrome::THEME_TOKENS.to_vec()
    }

    #[allow(clippy::unused_self)]
    pub fn theme_token_key(&self, token: &str) -> String {
        crate::chrome::theme_token_key(token)
    }

    #[allow(clippy::unused_self)]
    pub fn theme_key(&self, name: &str) -> String {
        crate::chrome::theme_key(name)
    }

    /// A combo pairs a cookie with a pet, so the partner of one is the other.
    #[allow(clippy::unused_self, clippy::missing_const_for_fn)]
    pub fn combi_partner_section(&self, section: &str) -> &'static str {
        crate::chrome::combi_partner_section(section)
    }

    /// "EP 5" / "Special EP 2", localized. A special tier wins over the
    /// plain one, matching the badge.
    pub fn build_ep_label(&self, ep: i64, ep_special: i64) -> String {
        if ep_special > 0 {
            return self
                .l
                .t("build_ep_special")
                .replace("{n}", &ep_special.to_string());
        }
        self.l.t("build_ep_tier").replace("{n}", &ep.to_string())
    }

    /// The combobox option labels. Askama hands loop variables out by
    /// reference, so these take one rather than making every call site deref.
    #[allow(clippy::trivially_copy_pass_by_ref)]
    pub fn ep_tier_label(&self, n: &i64) -> String {
        self.build_ep_label(*n, 0)
    }

    #[allow(clippy::trivially_copy_pass_by_ref)]
    pub fn ep_special_label(&self, n: &i64) -> String {
        self.build_ep_label(0, *n)
    }

    /// The regular EP tiers and the special ones, for the filter combobox.
    #[allow(clippy::unused_self)]
    pub fn ep_tiers(&self) -> Vec<i64> {
        crate::chrome::EP_TIERS.to_vec()
    }

    #[allow(clippy::unused_self)]
    pub fn ep_specials(&self) -> Vec<i64> {
        crate::chrome::EP_SPECIALS.to_vec()
    }
}
