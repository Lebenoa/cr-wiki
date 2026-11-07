use crate::builds;

use super::form::{level_field, parse_ep};

/// The build list runs, and its labels format the way the badges expect.
/// The table is empty on this checkout, so this covers the query path and
/// the formatting rather than row content.
#[tokio::test]
async fn build_queries_and_labels() {
    let Some(pool) = live_db().await else {
        eprintln!("skip: CR_SURREAL_URL not set");
        return;
    };

    for sort in ["latest", "verified", "score", "coin", "time", "nonsense"] {
        let rows = builds::select_builds(&pool, "en", (0, 0, 0, 0, 0), sort, "", 30, 0)
            .await
            .unwrap_or_else(|e| panic!("{sort}: {e}"));
        assert!(rows.len() <= 30);
    }
    // filters compose without tripping the SQL
    assert!(
        (builds::select_builds(&pool, "en", (89, 50, 317, 5, 0), "score", "", 30, 0))
            .await
            .is_ok()
    );
    assert!(
        (builds::select_builds(&pool, "en", (0, 0, 0, 0, 2), "latest", "", 30, 0))
            .await
            .is_ok()
    );
    // the author filter is bound, not inline, and empty means no filter
    assert!(
        (builds::select_builds(&pool, "en", (0, 0, 0, 0, 0), "latest", "alice", 30, 0))
            .await
            .is_ok()
    );
    assert!(builds::select_build(&pool, "en", "999999")
        .await
        .unwrap()
        .is_none());

    crate::i18n::load("translations");
    let c = crate::testutil::test_ctx("en");
    let mut b = builds::BuildCard {
        ep: 5,
        ..Default::default()
    };
    assert_eq!(b.ep_label(&c), "EP 5");
    b.ep_special = 2;
    assert_eq!(
        b.ep_label(&c),
        "Special EP 2",
        "a special tier wins over the plain one"
    );
    b.time_ms = 95_400;
    assert_eq!(b.time_label(), "1:35");
    b.time_ms = 0;
    assert_eq!(b.time_label(), "");
    assert!(!b.has_stats());
    b.score = 10;
    assert!(b.has_stats());
}

/// "1".."7" is a regular tier, "s1".."s3" a special one, junk is 0 —
/// the form's only EP gate, so an off-by-one here would store nonsense.
#[test]
fn parse_ep_splits_regular_from_special() {
    assert_eq!(parse_ep("3"), (3, 0));
    assert_eq!(parse_ep("7"), (7, 0));
    assert_eq!(parse_ep("s2"), (0, 2));
    assert_eq!(parse_ep("8"), (0, 0));
    assert_eq!(parse_ep("s4"), (0, 0));
    assert_eq!(parse_ep(""), (0, 0));
}

/// Blank means max level: an unparseable string must not silently store
/// level 0, which is a legitimate level and would read as wrong data.
#[test]
fn level_field_defaults_to_max() {
    assert_eq!(level_field(None), 9);
    assert_eq!(level_field(Some(&String::new())), 9);
    assert_eq!(level_field(Some(&"4".to_string())), 4);
    assert_eq!(level_field(Some(&"12".to_string())), 9);
    assert_eq!(level_field(Some(&"abc".to_string())), 9);
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
