use crate::db;
use crate::richtext;

/// Rich text: links resolve with their sprite, colours are constrained,
/// and everything else is escaped. Lives beside the detail page because
/// that is the surface the rendered prose reaches.
#[tokio::test]
async fn richtext_renders() {
    let Some(pool) = live_db().await else {
        eprintln!("skip: CR_SURREAL_URL not set");
        return;
    };

    let mut memo = richtext::LinkCache::new();
    let out = richtext::render_with(&pool, "en", "see [[89]] here", &mut memo, None).await;
    assert!(out.contains("href=\"/cookies/89\""));
    assert!(out.contains("<img src=\"/static/img/cookies/"));

    // an unresolvable ref stays literal
    let miss = richtext::render_with(&pool, "en", "[[cookie:99999999]]", &mut memo, None).await;
    assert!(miss.contains("[[cookie:99999999]]"));

    // a reference to the page's own entity renders as a highlight, not
    // an anchor
    let own = richtext::render_with(
        &pool,
        "en",
        "[[cookie:89]]",
        &mut memo,
        Some(("cookie", 89)),
    )
    .await;
    assert!(!own.contains("<a "), "{own}");
    assert!(own.contains("font-bold"), "{own}");

    let colored = richtext::render_with(
        &pool,
        "en",
        "a {color:red}red{/color} word",
        &mut memo,
        None,
    )
    .await;
    assert!(colored.contains("<span style=\"color:red\">red</span>"));

    // an injection attempt is not a valid colour, so the whole thing
    // renders as text: no span is opened and the quotes come out escaped
    let bad = richtext::render_with(
        &pool,
        "en",
        "{color:red\" onclick=\"x}y{/color}",
        &mut memo,
        None,
    )
    .await;
    assert!(!bad.contains("<span style="), "{bad}");
    assert!(!bad.contains("onclick=\""), "{bad}");
    assert!(bad.contains("&quot;"), "{bad}");

    // pasted markup is escaped
    let script =
        richtext::render_with(&pool, "en", "<script>alert(1)</script>", &mut memo, None).await;
    assert!(!script.contains("<script>"));
    assert!(script.contains("&lt;script&gt;"));
}

/// A treasure's unlock chain resolves to the entity that grants it.
#[tokio::test]
async fn treasure_links_resolve() {
    let Some(pool) = live_db().await else {
        eprintln!("skip: CR_SURREAL_URL not set");
        return;
    };

    // treasure 255 (Banana Lion Tail) is unlocked by pet 77, Banana Lion
    let links = db::treasure_links(&pool, "en", 255).await.expect("links");
    assert!(links.has_unlock());
    assert_eq!(links.unlock_section, "pets");
    assert_eq!(links.unlock_id, 77);
    assert!(!links.unlock_name.is_empty());

    // relics and skins have detail rows now. Their ids are not 1-based —
    // relics start at 500001 and skins at 1800001 — so the test takes an
    // id from the list rather than assuming one.
    for section in ["relics", "skins"] {
        let list = db::select_simple(&pool, "en", section).await.unwrap();
        let first = list.first().expect("a row");
        let detail = db::select_detail(&pool, "en", section, first.id)
            .await
            .unwrap()
            .unwrap_or_else(|| panic!("{section} {} has no detail", first.id));
        assert!(!detail.name.is_empty());
    }
}

/// A detail row carries the prose its kind has and nothing else.
#[tokio::test]
async fn detail_rows() {
    let Some(pool) = live_db().await else {
        eprintln!("skip: CR_SURREAL_URL not set");
        return;
    };

    let cookie = db::select_detail(&pool, "en", "cookies", 89)
        .await
        .unwrap()
        .expect("cookie 89");
    assert!(!cookie.name.is_empty());
    assert!(!cookie.abilities.is_empty());

    // a treasure has no abilities column, and effects come with ladders
    let treasure = db::select_detail(&pool, "en", "treasures", 317)
        .await
        .unwrap()
        .expect("treasure 317");
    assert!(treasure.abilities.is_empty());
    let effects = db::treasure_effects(&pool, "en", 317).await.unwrap();
    assert!(!effects.is_empty());
    assert!(
        effects.iter().all(|e| e.values.len() == 10),
        "ten levels per effect"
    );

    assert!(db::select_detail(&pool, "en", "cookies", 99999)
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn ingredient_crafting_and_treasure_variants() {
    let Some(pool) = live_db().await else {
        eprintln!("skip: CR_SURREAL_URL not set");
        return;
    };
    let recipes = db::ingredient_recipes(&pool, "en", 1).await.unwrap();
    assert_eq!(recipes.len(), 3);
    assert!(recipes
        .iter()
        .any(|r| r.treasure_id == 56 && r.name == "Mocking Carnival Mask"));

    let ingredients = db::treasure_craft_ingredients(&pool, "en", 56)
        .await
        .unwrap();
    assert!(ingredients
        .iter()
        .any(|i| i.ingredient_id == 1 && i.name == "Timber Board"));

    let base = db::treasure_variant(&pool, "en", 3, true)
        .await
        .unwrap()
        .expect("base");
    assert_eq!(base.0, 8);
    let evolved = db::treasure_variant(&pool, "en", 8, false)
        .await
        .unwrap()
        .expect("evolved");
    assert_eq!(evolved.0, 3);
}

/// The blessed toggle appears only when the blessed set actually differs
/// from the normal one — a treasure whose two sets match should not offer
/// a switch between identical readings.
#[test]
fn blessed_toggle_only_when_it_differs() {
    use db::EffectLine;
    let line = |text: &str, v: &[&str], blessed: bool| EffectLine {
        text: text.into(),
        values: v.iter().map(ToString::to_string).collect(),
        blessed,
    };

    // no blessed set at all
    assert!(!db::blessed_differs(&[line("Magnet", &["1"], false)]));
    // identical sets
    assert!(!db::blessed_differs(&[
        line("Magnet", &["1"], false),
        line("Magnet", &["1"], true),
    ]));
    // a different value is a difference worth showing
    assert!(db::blessed_differs(&[
        line("Magnet", &["1"], false),
        line("Magnet", &["2"], true),
    ]));
    // so is a different effect
    assert!(db::blessed_differs(&[
        line("Magnet", &["1"], false),
        line("Revive", &["1"], true),
    ]));
}

/// The combo editor lists a pairing with the row id it needs to remove it.
#[tokio::test]
async fn combi_editor_rows() {
    let Some(pool) = live_db().await else {
        eprintln!("skip: CR_SURREAL_URL not set");
        return;
    };

    let rows = db::combi_edit_rows(&pool, "en", "cookies", 89)
        .await
        .expect("rows");
    assert!(!rows.is_empty(), "cookie 89 pairs with a pet");
    assert!(
        rows.iter().all(|r| r.id > 0),
        "every row carries its own id"
    );
    assert!(rows.iter().all(|r| !r.partner_name.is_empty()));
    assert!(db::combi_edit_rows(&pool, "en", "treasures", 1)
        .await
        .unwrap()
        .is_empty());
}

/// The DB handle behind the gated tests: unset means they skip, so
/// `cargo test` stays green without a server. Points at a scratch
/// namespace/database — never at data you cannot lose.
async fn live_db() -> Option<crate::db::Db> {
    let url = std::env::var("CR_SURREAL_URL").ok()?;
    let ns = std::env::var("CR_SURREAL_NS").unwrap_or_else(|_| "cookierun".into());
    let database = std::env::var("CR_SURREAL_DB").unwrap_or_else(|_| "cookierun".into());
    let user = std::env::var("SURREAL_USER").unwrap_or_else(|_| "root".into());
    let pass = std::env::var("SURREAL_PASS").unwrap_or_default();
    match crate::db::connect_url(&url, &ns, &database, &user, &pass).await {
        Ok(db) => Some(db),
        // a URL that is set but unusable must not read as "no server
        // configured": that would turn a broken database into a green,
        // silently skipping run
        Err(e) => {
            eprintln!("skip: CR_SURREAL_URL is set but unusable ({e})");
            None
        }
    }
}
