//! The planner's submitted shape and the validation it runs: the form struct
//! itself, the level/EP/tag parsers, and the one field mapping create and
//! update share so they cannot drift apart.

use serde::Deserialize;

use crate::builds;

/// The planner posts hidden inputs for every slot, so the shape is the same
/// whether the pick came from a dialog or a prefilled edit.
#[derive(Debug, Default, Deserialize)]
pub struct BuildForm {
    #[serde(default)]
    pub cookie: i64,
    #[serde(default)]
    pub c2: i64,
    #[serde(default)]
    pub pet: i64,
    #[serde(default)]
    pub t1: i64,
    #[serde(default)]
    pub t2: i64,
    #[serde(default)]
    pub t3: i64,
    #[serde(default)]
    pub blessed1: i64,
    #[serde(default)]
    pub blessed2: i64,
    #[serde(default)]
    pub blessed3: i64,
    pub t1_level: Option<String>,
    pub t2_level: Option<String>,
    pub t3_level: Option<String>,
    pub ep: Option<String>,
    pub score: Option<String>,
    pub coin: Option<String>,
    pub time: Option<String>,
    pub boxes: Option<String>,
    pub description: Option<String>,
    pub youtube_url: Option<String>,
    pub author: Option<String>,
    #[serde(default)]
    pub tag_score: Option<String>,
    #[serde(default)]
    pub tag_coin: Option<String>,
    #[serde(default)]
    pub tag_autofarm: Option<String>,
    #[serde(rename = "cf-turnstile-response")]
    pub turnstile: Option<String>,
}

/// A blank level field means the treasure sits at max. An unparseable one
/// does too, rather than silently storing level 0 — which is a legitimate
/// level, so a naive parse would persist the wrong value.
pub(super) fn level_field(raw: Option<&String>) -> i64 {
    match raw.map(|s| s.trim()) {
        None | Some("") => 9,
        Some(s) if s.bytes().all(|b| b.is_ascii_digit()) => s.parse::<i64>().unwrap_or(9).min(9),
        _ => 9,
    }
}

fn number(raw: Option<&String>) -> i64 {
    raw.and_then(|s| s.trim().parse::<i64>().ok())
        .filter(|n| *n >= 0)
        .unwrap_or(0)
}

/// "1".."7" is a regular tier, "s1".."s3" a special one.
pub(super) fn parse_ep(raw: &str) -> (i64, i64) {
    if let Some(rest) = raw.strip_prefix('s') {
        let n = rest.parse::<i64>().unwrap_or(0);
        return (0, if (1..=3).contains(&n) { n } else { 0 });
    }
    let n = raw.parse::<i64>().unwrap_or(0);
    (if (1..=7).contains(&n) { n } else { 0 }, 0)
}

pub(super) fn tags_of(form: &BuildForm) -> Vec<String> {
    let mut out = Vec::new();
    for (present, name) in [
        (&form.tag_score, "score"),
        (&form.tag_coin, "coin"),
        (&form.tag_autofarm, "autofarm"),
    ] {
        if present.as_deref().is_some_and(|v| !v.is_empty()) {
            out.push(name.to_string());
        }
    }
    out
}

/// http(s) only: the value renders as a live href on the detail page and the
/// list cards, and HTML escaping does not neutralise a `javascript:` target
/// that carries no markup characters. Submission is anonymous, so the scheme
/// check is the only thing standing between a visitor and stored XSS.
fn validated_youtube_url(raw: Option<&String>) -> Result<String, &'static str> {
    let u = raw.map_or("", |s| s.trim());
    if u.is_empty() {
        return Ok(String::new());
    }
    if u.len() > 200 {
        return Err("build_error_video_link");
    }
    let lower = u.to_ascii_lowercase();
    if !lower.starts_with("http://") && !lower.starts_with("https://") {
        return Err("build_error_video_link");
    }
    if u.bytes().any(|b| b < 0x20 || b == 0x7f) {
        return Err("build_error_video_link");
    }
    Ok(u.to_string())
}

fn validated_description(raw: Option<&String>) -> Result<String, &'static str> {
    let d = raw.map_or("", |s| s.trim());
    if d.chars().count() > 5000 {
        return Err("build_error_description");
    }
    Ok(d.to_string())
}

/// The shared field mapping, so create and update cannot drift apart.
#[allow(clippy::needless_pass_by_value)] // tags arrives pre-assembled for both callers
pub(super) fn changes_from(
    form: &BuildForm,
    ep: i64,
    ep_special: i64,
    tags: Vec<String>,
) -> Result<builds::BuildChanges, &'static str> {
    let description = validated_description(form.description.as_ref())?;
    let youtube_url = validated_youtube_url(form.youtube_url.as_ref())?;
    Ok(builds::BuildChanges {
        cookie: form.cookie,
        cookie2: if form.c2 > 0 { Some(form.c2) } else { None },
        pet: form.pet,
        treasures: [form.t1, form.t2, form.t3],
        blessed: [form.blessed1 != 0, form.blessed2 != 0, form.blessed3 != 0],
        levels: [
            level_field(form.t1_level.as_ref()),
            level_field(form.t2_level.as_ref()),
            level_field(form.t3_level.as_ref()),
        ],
        ep,
        ep_special,
        tags: tags.join(","),
        score: number(form.score.as_ref()),
        coin: number(form.coin.as_ref()),
        // the form is in seconds, the column in milliseconds
        time_ms: number(form.time.as_ref()).saturating_mul(1000),
        boxes: number(form.boxes.as_ref()),
        description,
        youtube_url,
    })
}

/// The insert-side record: the mutable fields plus the stamped author,
/// owner and expiry.
#[allow(clippy::needless_pass_by_value)] // tags arrives pre-assembled for both callers
pub(super) fn record_from(
    form: &BuildForm,
    ep: i64,
    ep_special: i64,
    tags: Vec<String>,
    author: String,
    user_id: i64,
    expires_at: Option<i64>,
) -> Result<builds::NewBuild, &'static str> {
    Ok(builds::NewBuild {
        changes: changes_from(form, ep, ep_special, tags)?,
        author,
        user_id,
        expires_at,
    })
}
