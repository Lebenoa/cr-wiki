//! Grades. The stored value is the V `Grade` enum's ordinal, which is *not*
//! the display order: `grade_values` in models/grade.v ranks lowest to
//! highest as c, b, a, s, s+, l, e — E ("Extra") outranks L ("Legend").

/// enum ordinal -> slug, matching `enum Grade { e c b a s s_plus l }`
pub const fn slug(value: i64) -> &'static str {
    match value {
        0 => "e",
        1 => "c",
        2 => "b",
        3 => "a",
        4 => "s",
        5 => "s_plus",
        6 => "l",
        _ => "",
    }
}

/// enum ordinal -> sort rank, lowest grade first. Ungraded ranks below all.
pub const fn rank(value: i64) -> i64 {
    match value {
        1 => 0, // c
        2 => 1, // b
        3 => 2, // a
        4 => 3, // s
        5 => 4, // s_plus
        6 => 5, // l
        0 => 6, // e
        _ => -1,
    }
}

/// `s_plus` renders "S+", every other grade its uppercase letter.
pub fn label(value: i64) -> String {
    match slug(value) {
        "s_plus" => "S+".to_string(),
        s => s.to_uppercase(),
    }
}

