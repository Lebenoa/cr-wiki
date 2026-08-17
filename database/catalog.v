module database

import db.sqlite
import database.models

// RelicView is one relic with its localized text and links, for the /relics
// list. episode_id none = event relic.
pub struct RelicView {
pub:
	relic_id         int
	name             string
	description      string
	image            ?string
	episode_id       ?int
	unlock_cookie_id ?int
	unlock_cookie    string
}

// RelicGroupView groups a page's relics under their owning episode (or the
// event-relic group when episode_id is none), for the /relics list.
pub struct RelicGroupView {
pub:
	episode_id   ?int
	episode_name string // '' for event relics
	relics       []RelicView
}

// EpisodeStageView is one numbered stage of an episode.
pub struct EpisodeStageView {
pub:
	stage_no int
	name     string
}

// EpisodeQuestView is one quest row.
pub struct EpisodeQuestView {
pub:
	name        string
	requirement string
	reward      string
}

// QuestGroupView is one quest-chain group ("Jump King") with its rows.
pub struct QuestGroupView {
pub:
	group  string
	quests []EpisodeQuestView
}

// BoxGradeView is one mystery-box grade row: the disclosed odds for the
// first, second and third+ boxes (none when that box isn't offered).
pub struct BoxGradeView {
pub mut:
	box_grade string
	first     ?f64
	second    ?f64
	third     ?f64
}

// EpisodeRelicView is one relic of the episode's completion set.
pub struct EpisodeRelicView {
pub:
	relic_id int
	name     string
	image    ?string
}

// DrawRewardView is one relic-draw reward row with its disclosed odds.
pub struct DrawRewardView {
pub:
	rank   int
	reward string
	odds   ?f64
}

// BoxOddsView is one mystery-box grade-odds row for an episode.
pub struct BoxOddsView {
pub:
	box_no    int
	box_grade string
	odds      ?f64
}

// IngredientView is one ingredient that drops in an episode.
pub struct IngredientView {
pub:
	ingredient_id      int
	name               string
	image              ?string
	grade              ?int
	coin_value         int
	breaks_into_powder int
	craft_from_powder  int
	obtained_from      string
}

// EpisodeView is one episode: the list fields plus the detail collections
// (filled only by select_episode).
pub struct EpisodeView {
pub:
	episode_id    int
	name          string
	description   string
	image         ?string
	kind          string
	stars         ?int
	league_ranked bool
	entry_cost    string
pub mut:
	stage_count   int
	quest_count   int
	relic_count   int
	stages        []EpisodeStageView
	relics        []EpisodeRelicView
	quest_groups  []QuestGroupView
	draw_rewards  []DrawRewardView
	box_grades    []BoxGradeView
	ingredients   []IngredientView
}

// lang_name resolves a (id -> [lang]name) map to the requested lang with an
// English fallback, mirroring the list-page translation pattern.
fn lang_name(names map[int]map[string]string, id int, lang string) string {
	if id !in names {
		return ''
	}
	n := names[id][lang] or { '' }
	if n != '' {
		return n
	}
	return names[id]['en'] or { '' }
}

// episode_translation_map loads every episode translation once per page.
fn episode_translation_map(conn sqlite.DB, lang string) map[int]map[string]string {
	mut out := map[int]map[string]string{}
	trs := sql conn {
		select from models.EpisodeTranslation where lang == lang || lang == 'en'
	} or { return out }
	for tr in trs {
		if tr.episode_id !in out {
			out[tr.episode_id] = {}
		}
		out[tr.episode_id][tr.lang] = tr.name
	}
	return out
}

// episode_kind_order ranks kinds for the browser: story, special, event.
fn episode_kind_order(kind string) int {
	return match kind {
		'story' { 0 }
		'special' { 1 }
		else { 2 }
	}
}

// select_relics loads every relic grouped by its owning episode (event relics
// last, under an empty episode name), with localized names/descriptions and
// the unlock-cookie name for event relics.
pub fn select_relics(conn sqlite.DB, lang string) []RelicGroupView {
	rels := sql conn {
		select from models.Relic order by relic_id
	} or { return [] }
	trs := sql conn {
		select from models.RelicTranslation where lang == lang || lang == 'en'
	} or { return [] }
	mut rel_names := map[int]map[string]string{}
	mut rel_descs := map[int]map[string]string{}
	for tr in trs {
		if tr.relic_id !in rel_names {
			rel_names[tr.relic_id] = {}
			rel_descs[tr.relic_id] = {}
		}
		rel_names[tr.relic_id][tr.lang] = tr.name
		rel_descs[tr.relic_id][tr.lang] = tr.description
	}
	ep_names := episode_translation_map(conn, lang)
	mut cids := []int{}
	for r in rels {
		if cid := r.unlock_cookie_id {
			cids << cid
		}
	}
	mut cookie_names := map[int]string{}
	if cids.len > 0 {
		ctrs := sql conn {
			select from models.CookieTranslation where owner_id in cids && (lang == lang
				|| lang == 'en')
		} or { return [] }
		for tr in ctrs {
			if tr.lang == lang {
				cookie_names[tr.owner_id] = tr.name
			}
		}
		for tr in ctrs {
			if tr.owner_id !in cookie_names {
				cookie_names[tr.owner_id] = tr.name
			}
		}
	}
	// group by owning episode; episode relics first, event relics last
	mut by_ep := map[int][]RelicView{}
	mut event := []RelicView{}
	for r in rels {
		rid := r.relic_id or { 0 }
		rv := RelicView{
			relic_id:         rid
			name:             lang_name(rel_names, rid, lang)
			description:      lang_name(rel_descs, rid, lang)
			image:            r.image
			episode_id:       r.episode_id
			unlock_cookie_id: r.unlock_cookie_id
			unlock_cookie:    cookie_names[r.unlock_cookie_id or { 0 }] or { '' }
		}
		if eid := r.episode_id {
			if eid !in by_ep {
				by_ep[eid] = []RelicView{}
			}
			by_ep[eid] << rv
		} else {
			event << rv
		}
	}
	eps := sql conn {
		select from models.Episode
	} or { return [] }
	mut ordered := eps.clone()
	ordered.sort_with_compare(fn (a &models.Episode, b &models.Episode) int {
		ka := episode_kind_order(a.kind)
		kb := episode_kind_order(b.kind)
		if ka != kb {
			return ka - kb
		}
		return (a.episode_id or { 0 }) - (b.episode_id or { 0 })
	})
	mut groups := []RelicGroupView{}
	for e in ordered {
		if relics := by_ep[e.episode_id or { 0 }] {
			groups << RelicGroupView{
				episode_id:   e.episode_id
				episode_name: lang_name(ep_names, e.episode_id or { 0 }, lang)
				relics:       relics
			}
		}
	}
	if event.len > 0 {
		groups << RelicGroupView{
			relics: event
		}
	}
	return groups
}

// select_episodes loads every episode for the /episodes browser: the list
// fields plus stage/quest/relic counts (the detail collections stay empty).
pub fn select_episodes(conn sqlite.DB, lang string) []EpisodeView {
	eps := sql conn {
		select from models.Episode
	} or { return [] }
	ep_names := episode_translation_map(conn, lang)
	stages := sql conn {
		select from models.EpisodeStage
	} or { return [] }
	quests := sql conn {
		select from models.Quest
	} or { return [] }
	relics := sql conn {
		select from models.EpisodeRelic
	} or { return [] }
	mut out := []EpisodeView{}
	for e in eps {
		mut stage_count := 0
		mut quest_count := 0
		mut relic_count := 0
		for s in stages {
			if s.episode_id == (e.episode_id or { 0 }) {
				stage_count++
			}
		}
		for q in quests {
			if q.episode_id == (e.episode_id or { 0 }) {
				quest_count++
			}
		}
		for r in relics {
			if r.episode_id == (e.episode_id or { 0 }) {
				relic_count++
			}
		}
		out << EpisodeView{
			episode_id:    e.episode_id or { 0 }
			name:          lang_name(ep_names, e.episode_id or { 0 }, lang)
			image:         e.image
			kind:          e.kind
			stars:         e.stars
			league_ranked: e.league_ranked
			entry_cost:    e.entry_cost
			stage_count:   stage_count
			quest_count:   quest_count
			relic_count:   relic_count
		}
	}
	out.sort_with_compare(fn (a &EpisodeView, b &EpisodeView) int {
		ka := episode_kind_order(a.kind)
		kb := episode_kind_order(b.kind)
		if ka != kb {
			return ka - kb
		}
		return a.episode_id - b.episode_id
	})
	return out
}

// select_episode loads one episode with every detail collection: stages,
// the relic completion set, quests, relic-draw rewards, mystery-box odds and
// the ingredients that drop there. Returns an error when the episode is
// unknown (404 for the detail route).
pub fn select_episode(conn sqlite.DB, lang string, id int) !EpisodeView {
	eps := sql conn {
		select from models.Episode where episode_id == id
	}!
	if eps.len == 0 {
		return error('episode (${id}) not found')
	}
	e := eps.first()
	ep_names := episode_translation_map(conn, lang)
	etrs := sql conn {
		select from models.EpisodeTranslation where episode_id == id && (lang == lang
			|| lang == 'en')
	} or { return error('episode translations missing') }
	mut desc_map := map[int]map[string]string{}
	for tr in etrs {
		if tr.episode_id !in desc_map {
			desc_map[tr.episode_id] = {}
		}
		desc_map[tr.episode_id][tr.lang] = tr.description
	}
	mut view := EpisodeView{
		episode_id:    id
		name:          lang_name(ep_names, id, lang)
		description:   lang_name(desc_map, id, lang)
		image:         e.image
		kind:          e.kind
		stars:         e.stars
		league_ranked: e.league_ranked
		entry_cost:    e.entry_cost
		stage_count:   0
		quest_count:   0
		relic_count:   0
	}
	stages := sql conn {
		select from models.EpisodeStage where episode_id == id order by stage_no
	} or { return error('episode stages missing') }
	for s in stages {
		view.stages << EpisodeStageView{
			stage_no: s.stage_no
			name:     s.name
		}
	}
	// the completion relic set, resolved through the junction
	erecs := sql conn {
		select from models.EpisodeRelic where episode_id == id order by relic_id
	} or { return error('episode relic set missing') }
	mut relic_ids := []int{}
	for er in erecs {
		relic_ids << er.relic_id
	}
	if relic_ids.len > 0 {
		rtrs := sql conn {
			select from models.RelicTranslation where relic_id in relic_ids && (lang == lang
				|| lang == 'en')
		} or { return error('relic translations missing') }
		mut rel_names := map[int]map[string]string{}
		for tr in rtrs {
			if tr.relic_id !in rel_names {
				rel_names[tr.relic_id] = {}
			}
			rel_names[tr.relic_id][tr.lang] = tr.name
		}
		rel_rows := sql conn {
			select from models.Relic
		} or { return error('relic images missing') }
		mut rel_images := map[int]?string{}
		for rr in rel_rows {
			rrid := rr.relic_id or { 0 }
			if rrid in relic_ids {
				rel_images[rrid] = rr.image
			}
		}
		for rid in relic_ids {
			view.relics << EpisodeRelicView{
				relic_id: rid
				name:     lang_name(rel_names, rid, lang)
				image:    rel_images[rid]
			}
		}
	}
	quest_rows := sql conn {
		select from models.Quest where episode_id == id order by quest_id
	} or { return error('quests missing') }
	// quests chain in groups ("Jump King"), preserving the source order
	mut group_order := []string{}
	mut quest_groups := map[string][]EpisodeQuestView{}
	for q in quest_rows {
		if q.group !in quest_groups {
			group_order << q.group
			quest_groups[q.group] = []EpisodeQuestView{}
		}
		quest_groups[q.group] << EpisodeQuestView{
			name:        q.name
			requirement: q.requirement
			reward:      q.reward
		}
	}
	for g in group_order {
		view.quest_groups << QuestGroupView{
			group:  g
			quests: quest_groups[g]
		}
	}

	draw_rows := sql conn {
		select from models.EpisodeDrawReward where episode_id == id order by rank
	} or { return error('draw rewards missing') }
	for dr in draw_rows {
		view.draw_rewards << DrawRewardView{
			rank:   dr.rank
			reward: dr.reward
			odds:   dr.odds
		}
	}
	box_rows := sql conn {
		select from models.EpisodeBoxOdds where episode_id == id order by box_no
	} or { return error('box odds missing') }
	// one row per grade with the three disclosed box columns
	mut grade_order := []string{}
	mut box_by_grade := map[string]BoxGradeView{}
	for bo in box_rows {
		g := bo.box_grade
		if g !in box_by_grade {
			grade_order << g
			box_by_grade[g] = BoxGradeView{
				box_grade: g
			}
		}
		mut row := box_by_grade[g]
		match bo.box_no {
			1 { row.first = bo.odds }
			2 { row.second = bo.odds }
			else { row.third = bo.odds }
		}
		box_by_grade[g] = row
	}
	for g in grade_order {
		view.box_grades << box_by_grade[g]
	}

	ing_rows := sql conn {
		select from models.Ingredient where drop_episode_id == id order by ingredient_id
	} or { return error('ingredients missing') }
	mut ids := []int{}
	for i in ing_rows {
		ids << i.ingredient_id or { 0 }
	}
	mut ing_names := map[int]map[string]string{}
	if ids.len > 0 {
		itrs := sql conn {
			select from models.IngredientTranslation where ingredient_id in ids && (lang == lang
				|| lang == 'en')
		} or { return error('ingredient translations missing') }
		for tr in itrs {
			if tr.ingredient_id !in ing_names {
				ing_names[tr.ingredient_id] = {}
			}
			ing_names[tr.ingredient_id][tr.lang] = tr.name
		}
	}
	for i in ing_rows {
		view.ingredients << IngredientView{
			ingredient_id:      i.ingredient_id or { 0 }
			name:               lang_name(ing_names, i.ingredient_id or { 0 }, lang)
			image:              i.image
			grade:              i.grade
			coin_value:         i.coin_value
			breaks_into_powder: i.breaks_into_powder
			craft_from_powder:  i.craft_from_powder
			obtained_from:      i.obtained_from
		}
	}
	view.quest_count = 0
	for g in view.quest_groups {
		view.quest_count += g.quests.len
	}
	view.stage_count = view.stages.len
	view.relic_count = view.relics.len
	return view
}
