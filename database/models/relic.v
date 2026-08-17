module models

// Relic is one episode relic-set item. relic_id is the stable game id
// (500001-...; the episode-set relics encode their episode: 500101 = episode
// 1 relic 1), not a serial.
@[table: 'relic']
pub struct Relic {
pub:
	relic_id         ?int @[primary]
	image            ?string
	episode_id       ?int @[index; references: 'episode(episode_id)'] // owning relic set; none = event relic
	unlock_cookie_id ?int @[index; references: 'cookie(cookie_id)'] // cookie this relic unlocks
}

@[table: 'relic_translation']
@[unique_key: 'relic_id, lang']
pub struct RelicTranslation {
pub:
	relic_translation_id ?int @[primary; serial]

	relic_id int @[required; references: 'relic(relic_id)'; index]
	lang     string @[required; index]

	name        string @[required]
	description string
}
