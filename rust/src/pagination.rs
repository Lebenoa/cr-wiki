//! Shared paging window.
//!
//! Ported deliberately, not incidentally: the V original computed
//! `(page - 1) * size` in 32-bit int on a number straight off the query
//! string, which wrapped negative for a large enough page and panicked the
//! slice. i64 here, and past-the-end is a `None` rather than a bad window.

pub fn slice_page(page: i64, size: i64, total: usize) -> Option<(usize, usize)> {
    let total = total as i64;
    if page < 1 || size < 1 || total <= 0 {
        return None;
    }
    let start = (page - 1).checked_mul(size)?;
    if start >= total {
        return None;
    }
    let end = (start + size).min(total);
    Some((start as usize, end as usize))
}
