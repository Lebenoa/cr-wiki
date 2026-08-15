module util

// run_boost_keys are the pre-run boosts a player can buy before starting a
// run (the "random boost" offer). A build records the one that was purchased;
// labels live in the .tr files under `build_purchase_boost_*`. The three
// always-on run toggles (energy / item_time / fast_start) are separate — see
// `build_boost_*`.
pub fn run_boost_keys() []string {
	return ['coin_double', 'health_decrease_slow', 'extra_score', 'base_speed_up',
		'revive_hp80', 'collision_ignore', 'gold_in_coin_magic', 'potion_recovery_up',
		'hole_rescue', 'collision_damage_down', 'magnet_activate']
}

// power_effect_keys are the owned Power+ effects a build can mark as used.
// The catalog mirrors cookierunhub's owned-effects list; labels live in the
// .tr files under `power_effect_*`.
pub fn power_effect_keys() []string {
	return ['cheerleader', 'special_force', 'fairy', 'cheesecake', 'sea_fairy',
		'serenade', 'exp_party']
}
