module models

// Episode is one playable world: a story Episode (1-7), Special Episode
// (501-503, incl. the Tower of Frozen Waves) or event mode (601/602/701).
// episode_id is the stable game id, not a serial.
@[table: 'episode']
pub struct Episode {
pub:
	episode_id ?int @[primary]
	image      ?string
	kind       string // story | special | event
	stars      ?int // story difficulty rating; none for special/event modes
	league_ranked bool // "League ranking" mode
	entry_cost string // "1 × Life"
}

@[table: 'episode_translation']
@[unique_key: 'episode_id, lang']
pub struct EpisodeTranslation {
pub:
	episode_translation_id ?int @[primary; serial]

	episode_id int @[required; references: 'episode(episode_id)'; index]
	lang       string @[required; index]

	name        string @[required]
	description string
}

// EpisodeStage is one numbered stage inside an episode.
@[table: 'episode_stage']
@[unique_key: 'episode_id, stage_no']
pub struct EpisodeStage {
pub:
	episode_stage_id ?int @[primary; serial]
	episode_id       int @[required; references: 'episode(episode_id)'; index]
	stage_no         int @[required]
	name             string @[required]
}

// EpisodeRelic links an episode to the relic set completing it.
@[table: 'episode_relic']
@[unique_key: 'episode_id, relic_id']
pub struct EpisodeRelic {
pub:
	episode_relic_id ?int @[primary; serial]
	episode_id       int @[required; references: 'episode(episode_id)'; index]
	relic_id         int @[required; references: 'relic(relic_id)'; index]
}

// EpisodeDrawReward is one row of the episode's relic-draw reward pool
// (reward text + disclosed odds, e.g. "King Choco Drop 7.69%").
@[table: 'episode_draw_reward']
@[unique_key: 'episode_id, rank']
pub struct EpisodeDrawReward {
pub:
	episode_draw_reward_id ?int @[primary; serial]
	episode_id             int @[required; references: 'episode(episode_id)'; index]
	rank                   int @[required]
	reward                 string @[required]
	odds                   ?f64 // percent; none when the page shows no odds
}

// EpisodeBoxOdds is one mystery-box grade-odds row for an episode.
@[table: 'episode_box_odds']
pub struct EpisodeBoxOdds {
pub:
	episode_box_odds_id ?int @[primary; serial]
	episode_id          int @[required; references: 'episode(episode_id)'; index]
	box_no              int @[required] // 1 = first box, 2 = second box
	box_grade           string @[required]
	odds                ?f64
}
