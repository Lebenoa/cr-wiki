//! Shared paging window.
//!
//! Ported deliberately, not incidentally: the V original computed
//! `(page - 1) * size` in 32-bit int on a number straight off the query
//! string, which wrapped negative for a large enough page and panicked the
//! slice. i64 here with checked arithmetic, and past-the-end is a `None`
//! rather than a bad window.

pub fn slice_page(page: i64, size: i64, total: usize) -> Option<(usize, usize)> {
    let total = i64::try_from(total).ok()?;
    if page < 1 || size < 1 || total <= 0 {
        return None;
    }
    let start = page.saturating_sub(1).checked_mul(size)?;
    if start >= total {
        return None;
    }
    let end = start.checked_add(size)?.min(total);
    Some((usize::try_from(start).ok()?, usize::try_from(end).ok()?))
}

#[cfg(test)]
// tests use unwrap/expect/panic freely; production code does not (Cargo.toml [lints])
#[cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]
mod tests {
    use super::*;

    /// The overflow that panicked the V original: ?page=100000000 at 30 a page
    /// wrapped negative in 32-bit and blew up the slice.
    #[test]
    fn paging_survives_absurd_pages() {
        assert_eq!(slice_page(1, 30, 200), Some((0, 30)));
        assert_eq!(slice_page(7, 30, 200), Some((180, 200)));
        assert_eq!(slice_page(8, 30, 200), None);
        assert_eq!(slice_page(100_000_000, 30, 200), None);
        assert_eq!(slice_page(i64::MAX, 30, 200), None);
        assert_eq!(slice_page(0, 30, 200), None);
    }
}
