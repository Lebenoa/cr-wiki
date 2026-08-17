module models

// GachaPool is one disclosed draw pool (e.g. the treasure draw's Normal,
// Great and Supreme chest tiers, the pet hatch tiers). tier is the chest
// tier label; name is the unique pool key ('treasure_draw_normal', …).
@[table: 'gacha_pool']
pub struct GachaPool {
pub:
	pool_id ?int @[primary; serial]
	name    string @[required; unique]
	tier    string
}

// GachaPoolEntry is one draw-pool row: the prize (a treasure or a pet) with
// its disclosed odds. treasure_id/pet_id are mutually exclusive; grade is the
// prize grade shown on the page.
@[table: 'gacha_pool_entry']
pub struct GachaPoolEntry {
pub:
	gacha_pool_entry_id ?int @[primary; serial]
	pool_id             int @[required; references: 'gacha_pool(pool_id)'; index]
	treasure_id         ?int @[index; references: 'treasure(treasure_id)']
	pet_id              ?int @[index; references: 'pet(pet_id)']
	grade               ?int // models.Grade value
	odds                f64 // percent, e.g. 3.08
	sort_order          int
}
