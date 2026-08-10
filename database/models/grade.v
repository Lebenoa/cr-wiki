module models

pub enum Grade as u8 {
	e
	c
	b
	a
	s
	s_plus
	l
}

// grade_values lists grades from lowest to highest; E (Extra) ranks above L (Legend).
pub const grade_values = ['c', 'b', 'a', 's', 's_plus', 'l', 'e']

// grade_name returns the full grade name for tooltips; grades without a
// distinct name keep their letter.
pub fn grade_name(g string) string {
	return match g {
		'e' { 'Extra' }
		'l' { 'Legend' }
		else { g }
	}
}
