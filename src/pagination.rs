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
