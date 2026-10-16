//! One clock for the whole app, plus the civil-date conversions every
//! dated surface reads through (release dates, news publication dates).

/// Unix seconds now; 0 before the epoch rather than a panic, so every caller
/// — session TTL, rate-limit refill, build expiry — reads the same number.
pub fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(0))
}

/// `YYYY-MM-DD` -> unix seconds at midnight UTC, or `None` for anything that
/// is not a well-formed calendar date. A `<input type=date>` always sends
/// this shape; every other shape is rejected rather than guessed at.
// The Hinnant days-from-civil arithmetic is provably in-range: the parts are
// validated to YYYY-MM-DD just below.
#[allow(clippy::arithmetic_side_effects)]
pub fn parse_ymd(raw: &str) -> Option<i64> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    let mut parts = raw.split('-');
    let (y, m, d) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some() || y.len() != 4 || m.len() != 2 || d.len() != 2 {
        return None;
    }
    let (y, m, d) = (
        y.parse::<i64>().ok()?,
        m.parse::<i64>().ok()?,
        d.parse::<i64>().ok()?,
    );
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    // days-from-civil (Howard Hinnant's algorithm), then to unix seconds
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(days * 86400)
}

/// Unix seconds -> `YYYY-MM-DD`; the empty string for a non-positive
/// timestamp (an unset/unknown date), so a `<input type=date>` round-trips
/// cleanly and a card can hide an absent date.
// The inverse civil-from-days arithmetic is provably in-range: the input is
// unix seconds divided by 86400, so every intermediate stays far inside i64.
#[allow(clippy::arithmetic_side_effects)]
pub fn format_ymd(ts: i64) -> String {
    if ts <= 0 {
        return String::new();
    }
    let days = ts.div_euclid(86400);
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

/// A civil day in some zone -> the instant that day *begins* there, for a zone
/// `offset_minutes` east of UTC. `2026-10-20` at `+07:00` is the instant
/// `2026-10-19T17:00:00Z`, which is what makes a scheduled post go live at the
/// start of the chosen local day rather than at UTC midnight.
///
/// Deliberately not [`parse_ymd`]: a *release date* is a civil day with no time
/// of day in it at all — nothing ever compares it to a clock — so it stays UTC
/// midnight and reads back as the day it was written, while a *publication
/// date* is a moment, and a moment has to name a zone to mean anything.
pub fn parse_local_ymd(raw: &str, offset_minutes: i64) -> Option<i64> {
    parse_ymd(raw)?.checked_sub(offset_minutes.saturating_mul(60))
}

/// Unix seconds -> the civil day `offset_minutes` east of UTC: the inverse of
/// [`parse_local_ymd`], and the only shape a reader or an authoring admin ever
/// sees. The empty string stays the app's "unset" answer, since a
/// non-positive timestamp is no day in any zone.
pub fn format_local_ymd(ts: i64, offset_minutes: i64) -> String {
    if ts <= 0 {
        return String::new();
    }
    format_ymd(ts.saturating_add(offset_minutes.saturating_mul(60)))
}

/// Unix seconds -> `Fri, 16 Oct 2026 00:00:00`, the civil date and time both
/// timestamp shapes below are built on, so a change to one cannot leave the
/// other stamping a different day.
#[allow(clippy::arithmetic_side_effects)]
fn civil_stamp(ts: i64) -> String {
    const WEEKDAYS: [&str; 7] = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let civil = if ts <= 0 {
        "1970-01-01".to_string()
    } else {
        format_ymd(ts)
    };
    let mut parts = civil.split('-');
    let fallback = (1970_i64, 1_usize, 1_usize);
    let (y, m, d) = match (parts.next(), parts.next(), parts.next()) {
        (Some(y), Some(m), Some(d)) => (
            y.parse::<i64>().unwrap_or(fallback.0),
            m.parse::<usize>().unwrap_or(fallback.1),
            d.parse::<usize>().unwrap_or(fallback.2),
        ),
        _ => fallback,
    };
    let day_count = ts.max(0).div_euclid(86_400);
    let seconds = ts.max(0).rem_euclid(86_400);
    let weekday = WEEKDAYS
        .get(usize::try_from(day_count.rem_euclid(7)).unwrap_or(0))
        .copied()
        .unwrap_or("Thu");
    let month = MONTHS.get(m.saturating_sub(1)).copied().unwrap_or("Jan");
    format!(
        "{weekday}, {d:02} {month} {y} {:02}:{:02}:{:02}",
        seconds / 3600,
        (seconds % 3600) / 60,
        seconds % 60
    )
}

/// Unix seconds -> an RFC 2822 timestamp (`Fri, 16 Oct 2026 00:00:00 +0000`),
/// the shape `<pubDate>` wants in a feed. Civil-from-days is reused from
/// `format_ymd` rather than re-derived, so the two cannot disagree about
/// what day a second falls on; the weekday comes from the day count alone,
/// which is exact because 1970-01-01 was a Thursday.
///
/// A non-positive timestamp answers the epoch: the element is required, and a
/// valid-but-early date beats an absent one. Everything the app stores is UTC
/// midnight, so the offset is always `+0000`.
pub fn format_rfc2822(ts: i64) -> String {
    format!("{} +0000", civil_stamp(ts))
}

/// Unix seconds -> an `IMF-fixdate` (`Fri, 16 Oct 2026 00:00:00 GMT`), the one
/// shape a `Last-Modified` may carry: the HTTP-date grammar admits the literal
/// `GMT` and no numeric offset, so a feed's `+0000` would be a header no
/// conforming client is obliged to parse.
pub fn format_http_date(ts: i64) -> String {
    format!("{} GMT", civil_stamp(ts))
}

/// An HTTP-date -> unix seconds, accepting all three shapes RFC 9110 requires
/// a recipient to take: `IMF-fixdate` (`Sun, 06 Nov 1994 08:49:37 GMT`), the
/// obsolete RFC 850 (`Sunday, 06-Nov-94 08:49:37 GMT`) and asctime
/// (`Sun Nov  6 08:49:37 1994`). `None` for anything else — an unparseable
/// `If-Modified-Since` must fall back to sending the document, never to a 304
/// against a date we guessed at.
///
/// The zone is honoured (asctime carries none and means GMT); the weekday,
/// the one field that adds nothing, is ignored rather than validated, because
/// rejecting a right date over a wrong day name would only cost a re-download.
/// The civil halves are handed to `parse_ymd`, so this cannot drift from the
/// app's own date parser about what `1994-11-06` is.
#[allow(clippy::arithmetic_side_effects)]
pub fn parse_http_date(raw: &str) -> Option<i64> {
    let tokens: Vec<&str> = raw.split_whitespace().collect();
    let (y, m, d, time, zone) = if tokens.first()?.ends_with(',') {
        // IMF-fixdate (day month year) or the RFC 850 form, whose second token
        // carries a `06-Nov-94` date all of its own
        if let Some((d, m, y)) = dash_date(tokens.get(1)?) {
            (y, m, d, *tokens.get(2)?, tokens.get(3).copied())
        } else {
            let day = tokens.get(1)?.parse::<i64>().ok()?;
            let month = month_index(tokens.get(2)?)?;
            let year = tokens.get(3)?.parse::<i64>().ok()?;
            (year, month, day, *tokens.get(4)?, tokens.get(5).copied())
        }
    } else {
        // asctime: `Sun Nov  6 08:49:37 1994` — weekday, month, day, time,
        // year, with the weekday first and no zone at all
        let month = month_index(tokens.get(1)?)?;
        let day = tokens.get(2)?.parse::<i64>().ok()?;
        (tokens.get(4)?.parse::<i64>().ok()?, month, day, *tokens.get(3)?, None)
    };
    let civil = parse_ymd(&format!("{y:04}-{m:02}-{d:02}"))?;
    Some(civil + time_of_day(time)? - zone_offset(zone)?)
}

/// `06-Nov-94` -> the day, month number and year. RFC 850's two-digit year is
/// pinned to a fixed pivot rather than the clock: RFC 9110 asks for "the most
/// recent year in the past with those digits, unless that is over 50 years
/// ahead", which a `69` pivot satisfies for every date a client can send in
/// this era — and a misread year can only cost a re-download, never a wrong
/// 304, because an older date never passes the newer-or-equal test.
fn dash_date(token: &str) -> Option<(i64, i64, i64)> {
    let mut parts = token.split('-');
    let d = parts.next()?.parse::<i64>().ok()?;
    let m = month_index(parts.next()?)?;
    let y = parts.next()?.parse::<i64>().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some((d, m, if y < 100 { century_of(y) } else { y }))
}

/// A two-digit year -> the four-digit year a 1990s-to-2020s reader means.
fn century_of(y: i64) -> i64 {
    if y < 70 {
        y.saturating_add(2000)
    } else {
        y.saturating_add(1900)
    }
}

fn month_index(name: &str) -> Option<i64> {
    const MONTHS: [&str; 12] = [
        "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
    ];
    let name = name.trim_end_matches('.').to_ascii_lowercase();
    MONTHS
        .iter()
        .position(|m| *m == name)
        .and_then(|i| i64::try_from(i).ok())
        .map(|i| i.saturating_add(1))
}

/// `08:49:37` -> seconds since midnight; `None` for a malformed or impossible
/// clock reading, which is a header we refuse rather than round.
fn time_of_day(token: &str) -> Option<i64> {
    let mut parts = token.split(':');
    let h = parts.next()?.parse::<i64>().ok()?;
    let m = parts.next()?.parse::<i64>().ok()?;
    let s = parts.next()?.parse::<i64>().ok()?;
    if parts.next().is_some() || h > 23 || m > 59 || s > 60 {
        return None;
    }
    Some(h * 3600 + m * 60 + s)
}

/// The zone token -> seconds to subtract: `GMT`/`UT`/absent is zero, a numeric
/// `+hhhh`/`-hhhh` offset is itself. Anything else is a header we do not know
/// how to read.
fn zone_offset(token: Option<&str>) -> Option<i64> {
    let Some(token) = token else {
        return Some(0);
    };
    let upper = token.to_ascii_uppercase();
    if matches!(upper.as_str(), "GMT" | "UT" | "UTC") {
        return Some(0);
    }
    let (sign, rest) = match upper.strip_prefix('-') {
        Some(rest) => (-1, rest),
        None => (1, upper.strip_prefix('+')?),
    };
    let digits = rest.replace(':', "");
    if digits.len() != 4 || !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let hours = digits.get(..2)?.parse::<i64>().ok()?;
    let minutes = digits.get(2..)?.parse::<i64>().ok()?;
    if hours > 23 || minutes > 59 {
        return None;
    }
    Some(sign * (hours * 3600 + minutes * 60))
}

#[cfg(test)]
// tests use unwrap/expect/panic freely; production code does not (Cargo.toml [lints])
#[cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]
mod tests {
    use super::*;

    /// The feed timestamp is a real RFC 2822 date — weekday included, since a
    /// wrong or missing one is what makes a reader misplace an item.
    #[test]
    fn rfc2822_stamps_the_weekday_and_utc_time() {
        assert_eq!(format_rfc2822(1_792_108_800), "Fri, 16 Oct 2026 00:00:00 +0000");
        assert_eq!(format_rfc2822(0), "Thu, 01 Jan 1970 00:00:00 +0000");
        assert_eq!(format_rfc2822(-5), "Thu, 01 Jan 1970 00:00:00 +0000");
        // 2024-02-29 is a leap day and a Thursday, mid-day UTC included
        assert_eq!(
            format_rfc2822(1_709_210_045),
            "Thu, 29 Feb 2024 12:34:05 +0000"
        );
        // the same instant, in the shape a `Last-Modified` header may carry
        assert_eq!(
            format_http_date(1_709_210_045),
            "Thu, 29 Feb 2024 12:34:05 GMT"
        );
    }

    /// An HTTP-date parses in all three shapes RFC 9110 requires a recipient
    /// to accept, and the app's own stamps come back exactly as they went out —
    /// a revalidating client's `If-Modified-Since` is compared against what we
    /// advertised, so the two directions have to agree.
    #[test]
    fn http_dates_parse_in_every_required_shape() {
        // the RFC's own example: 784111977 is Sun, 06 Nov 1994 08:49:37 GMT
        assert_eq!(
            parse_http_date("Sun, 06 Nov 1994 08:49:37 GMT"),
            Some(784_111_777)
        );
        assert_eq!(format_http_date(784_111_777), "Sun, 06 Nov 1994 08:49:37 GMT");
        // obsolete RFC 850, whose two-digit year is read as 1994 — the year
        // the RFC's own example names — and its four-digit form too
        assert_eq!(
            parse_http_date("Sunday, 06-Nov-94 08:49:37 GMT"),
            Some(784_111_777)
        );
        let evening = parse_ymd("2026-11-06").unwrap() + 8 * 3600 + 49 * 60 + 37;
        assert_eq!(
            parse_http_date("Sunday, 06-Nov-2026 08:49:37 GMT"),
            Some(evening),
            "a four-digit year is taken as written"
        );
        assert_eq!(parse_http_date("Sun Nov  6 08:49:37 1994"), Some(784_111_777));
        // a numeric offset is honoured, and GMT/UT are the same zone
        assert_eq!(
            parse_http_date("Sun, 06 Nov 1994 03:49:37 -0500"),
            Some(784_111_777)
        );
        assert_eq!(parse_http_date("Sun, 06 Nov 1994 08:49:37 UT"), Some(784_111_777));
        // and every stamp this app emits, round-tripped
        for ts in [0, 1_792_108_800, 1_709_210_045, 1_792_972_799] {
            assert_eq!(parse_http_date(&format_http_date(ts)), Some(ts), "{ts}");
        }
    }

    /// A date we cannot read is refused rather than guessed at: a wrong parse
    /// aimed at a 304 serves a stale document silently, which is worse than
    /// one more download.
    #[test]
    fn unreadable_http_dates_are_refused() {
        for bad in [
            "",
            "  ",
            "not a date",
            "Sun, 06 Nov 1994 08:49:37 XYZ",
            "Sun, 06 Foo 1994 08:49:37 GMT",
            "Sun, 06 Nov 1994 25:49:37 GMT",
            "Sun, 06 Nov 1994 08:60:37 GMT",
            "Sun, 06 Nov 1994 08:49 GMT",
            "Sun, 06-11-1994 08:49:37 GMT",
            "Sun, 06 Nov 1994 08:49:37 +2500",
            "2026-10-16",
        ] {
            assert_eq!(parse_http_date(bad), None, "{bad:?}");
        }
    }

    /// A publication date is the start of that civil day *in the site's own
    /// zone*: the pair round-trips, the instant stored is the first second of
    /// the local day rather than an hour or a day out, and UTC is exactly the
    /// behaviour every deployment had before `[site]` existed.
    #[test]
    fn publication_days_are_local_midnights() {
        // no [site] configured: UTC midnight, unchanged
        assert_eq!(parse_local_ymd("2026-10-16", 0), Some(1_792_108_800));
        assert_eq!(format_local_ymd(1_792_108_800, 0), "2026-10-16");

        for offset in [60, 420, -480, 840, -720] {
            let start = parse_local_ymd("2026-10-20", offset).unwrap();
            assert_eq!(format_local_ymd(start, offset), "2026-10-20", "{offset}");
            assert_eq!(
                start,
                parse_ymd("2026-10-20").unwrap() - offset * 60,
                "the zone's offset is what separates it from UTC midnight: {offset}"
            );
            assert_eq!(
                format_local_ymd(start - 1, offset),
                "2026-10-19",
                "the stored instant is the day's first second, not one before it: {offset}"
            );
            // a zone's day begins at its own instant, never at UTC's: this is
            // the assertion the whole change exists for
            if offset != 0 {
                assert_ne!(
                    start,
                    parse_ymd("2026-10-20").unwrap(),
                    "local midnight is not UTC midnight: {offset}"
                );
            }
        }

        // the sentinel survives the conversion: an unset date has no day
        assert_eq!(format_local_ymd(0, 420), "");
        assert_eq!(parse_local_ymd("", 420), None);
        assert_eq!(parse_local_ymd("2026-1-1", 420), None);
    }

    /// The two directions agree, and a malformed or unset date is never
    /// silently rounded into a real one.
    #[test]
    fn ymd_round_trips_and_rejects_junk() {
        assert_eq!(parse_ymd("1970-01-01"), Some(0));
        assert_eq!(parse_ymd("2026-10-16"), Some(1_792_108_800));
        assert_eq!(format_ymd(1_792_108_800), "2026-10-16");
        // round-trip across a leap day and a zero timestamp
        assert_eq!(format_ymd(parse_ymd("2024-02-29").unwrap()), "2024-02-29");
        assert_eq!(format_ymd(0), "");
        assert_eq!(format_ymd(-5), "");

        for bad in ["", "  ", "2026-1-1", "2026-13-01", "2026-01-32", "not-a-date", "2026-01-01-01"] {
            assert_eq!(parse_ymd(bad), None, "{bad:?}");
        }
    }
}
