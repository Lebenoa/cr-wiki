module models

// Quest is one quest/achievement with its reward. Text is content (English,
// as scraped), not UI chrome, so it lives directly on the row; group is the
// quest-chain title ("Jump King"), name the full quest text, requirement the
// action, reward the prize ("1 Crystal").
@[table: 'quest']
pub struct Quest {
pub:
	quest_id   ?int @[primary; serial]
	episode_id int @[required; references: 'episode(episode_id)'; index]
	group      string
	name       string @[required]
	requirement string
	reward     string
}
