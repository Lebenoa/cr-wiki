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
