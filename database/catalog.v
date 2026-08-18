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
	// episode ids encode the kind order the groups need — story 1-7, then
	// special 501+, then event 601+ — so the id sort SQLite already does for
	// free is the kind-then-id sort (same invariant episode_short() reads the
	// badge from). No comparator, no second pass over the rows.
	eps := sql conn {
		select from models.Episode order by episode_id
	} or { return [] }
	mut groups := []RelicGroupView{}
	for e in eps {
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
	// ordered by id, which is the kind order the browser wants (see
	// select_relics): story 1-7, special 501+, event 601+
	eps := sql conn {
		select from models.Episode order by episode_id
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

// CraftRecipeView is one treasure an ingredient crafts (from
// ingredient_recipe), with its localized name.
pub struct CraftRecipeView {
pub:
	treasure_id int
	name        string
	image       ?string
}

// IngredientCraftView is one ingredient for the /crafting pages: its catalog
// row plus the treasures it crafts (filled by select_ingredient).
pub struct IngredientCraftView {
pub:
	ingredient_id      int
	name               string
	description        string
	image              ?string
	grade              ?int
	coin_value         int
	breaks_into_powder int
	craft_from_powder  int
	obtained_from      string
	drop_episode_id    ?int
	// the catalog's drop-location text, localized to the episode's own name
	// when it names one episode ("Episode 2" -> the episode's localized
	// title); the raw text otherwise ("Appears in all Episodes").
	drop_location      string
pub mut:
	recipe_count       int
	recipes            []CraftRecipeView
}

// episode_names_by_id resolves episode titles for `ids`, preferring `lang`
// with the English fallback the rest of the catalog uses.
fn episode_names_by_id(conn sqlite.DB, lang string, ids []int) map[int]string {
	mut out := map[int]string{}
	if ids.len == 0 {
		return out
	}
	trs := sql conn {
		select from models.EpisodeTranslation where episode_id in ids && (lang == lang
			|| lang == 'en')
	} or { return out }
	mut names := map[int]map[string]string{}
	for tr in trs {
		if tr.episode_id !in names {
			names[tr.episode_id] = {}
		}
		names[tr.episode_id][tr.lang] = tr.name
	}
	for id in ids {
		out[id] = lang_name(names, id, lang)
	}
	return out
}

// drop_location_text prefers the episode's localized name over the catalog's
// English drop text, so a Thai page reads the episode title in Thai. Falls
// back to the raw text for the ingredients that drop in every episode (and
// for any row whose episode failed to resolve).
fn drop_location_text(raw string, episode_id ?int, names map[int]string) string {
	if eid := episode_id {
		if name := names[eid] {
			if name != '' {
				return name
			}
		}
	}
	return raw
}

// ingredient_catalog_sql orders the /crafting grid: grade first, rarest down
// to commonest (ungraded last — SQLite sorts NULL below every value, so DESC
// puts them at the end), then drop episode in play order, since the id ranges
// already ascend story (1-7) -> special (501+) -> event (601+). The nine
// ingredients that drop everywhere carry no episode id and sort after the
// ones that name an episode, which needs the explicit `IS NULL` key: SQLite
// would otherwise put those NULLs first on an ASC sort. Id breaks ties so the
// order is stable.
//
// Raw SQL because the ORM cannot express it: orm.SelectConfig carries one
// `order` column plus one `order_type`, emitted as a quoted identifier — no
// second key, no expression, no NULLS LAST.
const ingredient_catalog_sql = 'SELECT ingredient_id, image, grade, drop_episode_id, ' +
	'drop_location, coin_value, breaks_into_powder, craft_from_powder, obtained_from ' +
	'FROM ingredient ' +
	'ORDER BY grade DESC, drop_episode_id IS NULL, drop_episode_id, ingredient_id'

// select_ingredients loads every ingredient with its localized name and the
// number of treasures it crafts, for the /crafting index.
pub fn select_ingredients(conn sqlite.DB, lang string) []IngredientCraftView {
	rows := conn.exec(ingredient_catalog_sql) or { return [] }
	mut ings := []models.Ingredient{cap: rows.len}
	for r in rows {
		// values arrive as strings and SQL NULL reads back as '', which is
		// the only way to tell a missing grade from grade 0 (a real grade)
		// — get_int maps both to 0. Same pattern as images_by_ids.
		img := r.get_string('image')
		grade := r.get_string('grade')
		episode := r.get_string('drop_episode_id')
		ings << models.Ingredient{
			ingredient_id:      r.get_int('ingredient_id')
			image:              if img == '' { none } else { img }
			grade:              if grade == '' { none } else { grade.int() }
			drop_episode_id:    if episode == '' { none } else { episode.int() }
			drop_location:      r.get_string('drop_location')
			coin_value:         r.get_int('coin_value')
			breaks_into_powder: r.get_int('breaks_into_powder')
			craft_from_powder:  r.get_int('craft_from_powder')
			obtained_from:      r.get_string('obtained_from')
		}
	}
	itrs := sql conn {
		select from models.IngredientTranslation where lang == lang || lang == 'en'
	} or { return [] }
	mut names := map[int]map[string]string{}
	for tr in itrs {
		if tr.ingredient_id !in names {
			names[tr.ingredient_id] = {}
		}
		names[tr.ingredient_id][tr.lang] = tr.name
	}
	recs := sql conn {
		select from models.IngredientRecipe
	} or { return [] }
	mut counts := map[int]int{}
	for r in recs {
		counts[r.ingredient_id]++
	}
	mut eids := []int{}
	for i in ings {
		if eid := i.drop_episode_id {
			if eid !in eids {
				eids << eid
			}
		}
	}
	enames := episode_names_by_id(conn, lang, eids)
	mut out := []IngredientCraftView{}
	for i in ings {
		iid := i.ingredient_id or { 0 }
		out << IngredientCraftView{
			ingredient_id:      iid
			name:               lang_name(names, iid, lang)
			image:              i.image
			grade:              i.grade
			coin_value:         i.coin_value
			breaks_into_powder: i.breaks_into_powder
			craft_from_powder:  i.craft_from_powder
			obtained_from:      i.obtained_from
			drop_episode_id:    i.drop_episode_id
			drop_location:      drop_location_text(i.drop_location, i.drop_episode_id, enames)
			recipe_count:       counts[iid]
		}
	}
	return out
}

// select_ingredient loads one ingredient with the treasures it crafts.
// Returns an error when the ingredient is unknown (404 for the detail route).
pub fn select_ingredient(conn sqlite.DB, lang string, id int) !IngredientCraftView {
	ings := sql conn {
		select from models.Ingredient where ingredient_id == id
	}!
	if ings.len == 0 {
		return error('ingredient (${id}) not found')
	}
	i := ings.first()
	itrs := sql conn {
		select from models.IngredientTranslation where ingredient_id == id && (lang == lang
			|| lang == 'en')
	} or { return error('ingredient translations missing') }
	mut names := map[int]map[string]string{}
	mut descs := map[int]map[string]string{}
	for tr in itrs {
		if tr.ingredient_id !in names {
			names[tr.ingredient_id] = {}
			descs[tr.ingredient_id] = {}
		}
		names[tr.ingredient_id][tr.lang] = tr.name
		descs[tr.ingredient_id][tr.lang] = tr.description
	}
	recs := sql conn {
		select from models.IngredientRecipe where ingredient_id == id order by treasure_id
	} or { return error('ingredient recipes missing') }
	mut tids := []int{}
	for r in recs {
		tids << r.treasure_id
	}
	ttrs := sql conn {
		select from models.TreasureTranslation where treasure_id in tids && (lang == lang
			|| lang == 'en')
	} or { return error('treasure translations missing') }
	mut tnames := map[int]map[string]string{}
	for tr in ttrs {
		if tr.treasure_id !in tnames {
			tnames[tr.treasure_id] = {}
		}
		tnames[tr.treasure_id][tr.lang] = tr.name
	}
	treasures := sql conn {
		select from models.Treasure
	} or { return error('treasures missing') }
	mut timages := map[int]?string{}
	for t in treasures {
		timages[t.treasure_id or { 0 }] = t.image
	}
	mut view := IngredientCraftView{
		ingredient_id:      id
		name:               lang_name(names, id, lang)
		description:        lang_name(descs, id, lang)
		image:              i.image
		grade:              i.grade
		coin_value:         i.coin_value
		breaks_into_powder: i.breaks_into_powder
		craft_from_powder:  i.craft_from_powder
		obtained_from:      i.obtained_from
		drop_episode_id:    i.drop_episode_id
		drop_location:      drop_location_text(i.drop_location, i.drop_episode_id, episode_names_by_id(conn,
			lang, if eid := i.drop_episode_id { [eid] } else { [] }))
		recipe_count:       recs.len
	}
	for tid in tids {
		view.recipes << CraftRecipeView{
			treasure_id: tid
			name:        lang_name(tnames, tid, lang)
			image:       timages[tid]
		}
	}
	return view
}

// TreasureIngredientView is one ingredient a treasure needs to be crafted
// (the reverse of ingredient_recipe), for the treasure detail page.
pub struct TreasureIngredientView {
pub:
	ingredient_id   int
	name            string
	image           ?string
	grade           ?int
	drop_episode_id ?int
}

// select_treasure_ingredients loads the ingredients a treasure is crafted
// from, in the user's language. Empty when the treasure has no recipe.
pub fn select_treasure_ingredients(conn sqlite.DB, lang string, tid int) []TreasureIngredientView {
	recs := sql conn {
		select from models.IngredientRecipe where treasure_id == tid order by ingredient_id
	} or { return [] }
	mut ids := []int{cap: recs.len}
	for r in recs {
		ids << r.ingredient_id
	}
	if ids.len == 0 {
		return []
	}
	itrs := sql conn {
		select from models.IngredientTranslation where ingredient_id in ids && (lang == lang
			|| lang == 'en')
	} or { return [] }
	mut names := map[int]map[string]string{}
	for tr in itrs {
		if tr.ingredient_id !in names {
			names[tr.ingredient_id] = {}
		}
		names[tr.ingredient_id][tr.lang] = tr.name
	}
	ings := sql conn {
		select from models.Ingredient
	} or { return [] }
	mut images := map[int]?string{}
	for i in ings {
		images[i.ingredient_id or { 0 }] = i.image
	}
	mut grades := map[int]?int{}
	for i in ings {
		grades[i.ingredient_id or { 0 }] = i.grade
	}
	mut drops := map[int]?int{}
	for i in ings {
		drops[i.ingredient_id or { 0 }] = i.drop_episode_id
	}
	mut out := []TreasureIngredientView{}
	for rid in ids {
		out << TreasureIngredientView{
			ingredient_id:   rid
			name:            lang_name(names, rid, lang)
			image:           images[rid]
			grade:           grades[rid]
			drop_episode_id: drops[rid]
		}
	}
	return out
}
// JellyMakerView is one creator of a jelly: the cookie/pet/treasure whose
// skill produces it. href is that entity's page path.
pub struct JellyMakerView {
pub:
	entity_kind string // 'cookie' | 'pet' | 'treasure'
	entity_id   int
	name        string
	image       ?string
	href        string
}

// JellyView is one in-run jelly with its score and makers, for the /jellies
// list and detail page.
pub struct JellyView {
pub:
	jelly_id int
	name     string
	image    ?string
	score    f64
	makers   []JellyMakerView
}

// SkinView is one cookie/pet costume with its owner, for the /skins list.
pub struct SkinView {
pub:
	skin_id    int
	name       string
	subtitle   string
	image      ?string
	grade      ?int
	collab     bool
	owner_name string // cookie or pet name ('' when unresolvable)
	owner_href string // '/cookies/:id' or '/pets/:id'
}

// GachaEntryView is one disclosed draw-pool row: prize name/image, grade and
// odds. id is the prize entity's id (route link derives from kind).
pub struct GachaEntryView {
pub mut:
	kind  string // 'treasure' | 'pet' (image dir + route prefix)
	id    int
	name  string
	image ?string
	grade ?int
	odds  f64
}

// GachaPoolView is one draw pool (treasure-draw chest tier or pet-hatch egg
// tier) with its entries in disclosed order. tier is the tier code the
// template turns into a tr key suffix.
pub struct GachaPoolView {
pub:
	pool_id int
	name    string // pool key ('treasure_draw_normal', …)
	tier    string
	entries []GachaEntryView
}

// EntityInfo is a batched name/image lookup for one entity kind.
struct EntityInfo {
pub mut:
	names  map[int]map[string]string
	images map[int]?string
}

fn cookie_info(conn sqlite.DB, lang string) EntityInfo {
	mut out := EntityInfo{
		names:  map[int]map[string]string{}
		images: map[int]?string{}
	}
	trs := sql conn {
		select from models.CookieTranslation where lang == lang || lang == 'en'
	} or { return out }
	for tr in trs {
		if tr.owner_id !in out.names {
			out.names[tr.owner_id] = {}
		}
		out.names[tr.owner_id][tr.lang] = tr.name
	}
	rows := sql conn {
		select from models.Cookie
	} or { return out }
	for c in rows {
		out.images[c.cookie_id or { 0 }] = c.image
	}
	return out
}

fn pet_info(conn sqlite.DB, lang string) EntityInfo {
	mut out := EntityInfo{
		names:  map[int]map[string]string{}
		images: map[int]?string{}
	}
	trs := sql conn {
		select from models.PetTranslation where lang == lang || lang == 'en'
	} or { return out }
	for tr in trs {
		if tr.pet_id !in out.names {
			out.names[tr.pet_id] = {}
		}
		out.names[tr.pet_id][tr.lang] = tr.name
	}
	rows := sql conn {
		select from models.Pet
	} or { return out }
	for p in rows {
		out.images[p.pet_id or { 0 }] = p.image
	}
	return out
}

fn treasure_info(conn sqlite.DB, lang string) EntityInfo {
	mut out := EntityInfo{
		names:  map[int]map[string]string{}
		images: map[int]?string{}
	}
	trs := sql conn {
		select from models.TreasureTranslation where lang == lang || lang == 'en'
	} or { return out }
	for tr in trs {
		if tr.treasure_id !in out.names {
			out.names[tr.treasure_id] = {}
		}
		out.names[tr.treasure_id][tr.lang] = tr.name
	}
	rows := sql conn {
		select from models.Treasure
	} or { return out }
	for t in rows {
		out.images[t.treasure_id or { 0 }] = t.image
	}
	return out
}

// select_jellies loads every jelly with its localized name and maker links.
pub fn select_jellies(conn sqlite.DB, lang string) []JellyView {
	jellies := sql conn {
		select from models.Jelly order by jelly_id
	} or { return [] }
	trs := sql conn {
		select from models.JellyTranslation where lang == lang || lang == 'en'
	} or { return [] }
	mut names := map[int]map[string]string{}
	for tr in trs {
		if tr.jelly_id !in names {
			names[tr.jelly_id] = {}
		}
		names[tr.jelly_id][tr.lang] = tr.name
	}
	makers := sql conn {
		select from models.JellyMaker order by jelly_id
	} or { return [] }
	ck := cookie_info(conn, lang)
	pt := pet_info(conn, lang)
	tr_info := treasure_info(conn, lang)
	mut by_jelly := map[int][]JellyMakerView{}
	for m in makers {
		v := match m.entity_kind {
			'cookie' {
				JellyMakerView{
					entity_kind: 'cookie'
					entity_id:   m.entity_id
					name:        lang_name(ck.names, m.entity_id, lang)
					image:       ck.images[m.entity_id]
					href:        '/cookies/${m.entity_id}'
				}
			}
			'pet' {
				JellyMakerView{
					entity_kind: 'pet'
					entity_id:   m.entity_id
					name:        lang_name(pt.names, m.entity_id, lang)
					image:       pt.images[m.entity_id]
					href:        '/pets/${m.entity_id}'
				}
			}
			else {
				JellyMakerView{
					entity_kind: 'treasure'
					entity_id:   m.entity_id
					name:        lang_name(tr_info.names, m.entity_id, lang)
					image:       tr_info.images[m.entity_id]
					href:        '/treasures/${m.entity_id}'
				}
			}
		}
		if m.jelly_id !in by_jelly {
			by_jelly[m.jelly_id] = []
		}
		by_jelly[m.jelly_id] << v
	}
	mut out := []JellyView{}
	for j in jellies {
		jid := j.jelly_id or { 0 }
		out << JellyView{
			jelly_id: jid
			name:     lang_name(names, jid, lang)
			image:    j.image
			score:    j.score
			makers:   by_jelly[jid] or { [] }
		}
	}
	return out
}

// select_jelly loads one jelly with its localized text and makers.
pub fn select_jelly(conn sqlite.DB, jelly_id int, lang string) ?JellyView {
	rows := sql conn {
		select from models.Jelly where jelly_id == jelly_id limit 1
	} or { return none }
	if rows.len == 0 {
		return none
	}
	j := rows.first()
	trs := sql conn {
		select from models.JellyTranslation where jelly_id == jelly_id && (lang == lang
			|| lang == 'en')
	} or { return none }
	mut names := map[int]map[string]string{}
	for tr in trs {
		if tr.jelly_id !in names {
			names[tr.jelly_id] = {}
		}
		names[tr.jelly_id][tr.lang] = tr.name
	}
	makers := sql conn {
		select from models.JellyMaker where jelly_id == jelly_id order by jelly_maker_id
	} or { return none }
	ck := cookie_info(conn, lang)
	pt := pet_info(conn, lang)
	tr_info := treasure_info(conn, lang)
	mut maker_views := []JellyMakerView{}
	for m in makers {
		v := match m.entity_kind {
			'cookie' {
				JellyMakerView{
					entity_kind: 'cookie'
					entity_id:   m.entity_id
					name:        lang_name(ck.names, m.entity_id, lang)
					image:       ck.images[m.entity_id]
					href:        '/cookies/${m.entity_id}'
				}
			}
			'pet' {
				JellyMakerView{
					entity_kind: 'pet'
					entity_id:   m.entity_id
					name:        lang_name(pt.names, m.entity_id, lang)
					image:       pt.images[m.entity_id]
					href:        '/pets/${m.entity_id}'
				}
			}
			else {
				JellyMakerView{
					entity_kind: 'treasure'
					entity_id:   m.entity_id
					name:        lang_name(tr_info.names, m.entity_id, lang)
					image:       tr_info.images[m.entity_id]
					href:        '/treasures/${m.entity_id}'
				}
			}
		}
		maker_views << v
	}
	return JellyView{
		jelly_id: j.jelly_id or { jelly_id }
		name:     lang_name(names, jelly_id, lang)
		image:    j.image
		score:    j.score
		makers:   maker_views
	}
}

// select_skins loads every costume with its localized name and owner link.
pub fn select_skins(conn sqlite.DB, lang string) []SkinView {
	skins := sql conn {
		select from models.Skin order by skin_id
	} or { return [] }
	trs := sql conn {
		select from models.SkinTranslation where lang == lang || lang == 'en'
	} or { return [] }
	mut names := map[int]map[string]string{}
	for tr in trs {
		if tr.skin_id !in names {
			names[tr.skin_id] = {}
		}
		names[tr.skin_id][tr.lang] = tr.name
	}
	ck := cookie_info(conn, lang)
	pt := pet_info(conn, lang)
	mut out := []SkinView{}
	for s in skins {
		sid := s.skin_id or { 0 }
		mut owner_name := ''
		mut owner_href := ''
		if cid := s.cookie_id {
			owner_name = lang_name(ck.names, cid, lang)
			owner_href = '/cookies/${cid}'
		} else if pid := s.pet_id {
			owner_name = lang_name(pt.names, pid, lang)
			owner_href = '/pets/${pid}'
		}
		out << SkinView{
			skin_id:    sid
			name:       lang_name(names, sid, lang)
			subtitle:   s.subtitle
			image:      s.image
			grade:      s.grade
			collab:     s.collab
			owner_name: owner_name
			owner_href: owner_href
		}
	}
	return out
}

// select_gacha loads every disclosed draw pool with its entries in order.
pub fn select_gacha(conn sqlite.DB, lang string) []GachaPoolView {
	pools := sql conn {
		select from models.GachaPool order by pool_id
	} or { return [] }
	// ordered by sort_order, not pool_id: the entries are grouped into a map
	// keyed by pool below, and a V map keeps insertion order, so each pool's
	// slice comes out in the catalog's own order without a per-pool sort.
	entries := sql conn {
		select from models.GachaPoolEntry order by sort_order
	} or { return [] }
	tr_info := treasure_info(conn, lang)
	pt := pet_info(conn, lang)
	mut by_pool := map[int][]models.GachaPoolEntry{}
	for e in entries {
		if e.pool_id !in by_pool {
			by_pool[e.pool_id] = []
		}
		by_pool[e.pool_id] << e
	}
	mut out := []GachaPoolView{}
	for p in pools {
		pid := p.pool_id or { 0 }
		es := by_pool[pid] or { [] }
		mut views := []GachaEntryView{}
		for e in es {
			mut v := GachaEntryView{
				grade: e.grade
				odds:  e.odds
			}
			if tid := e.treasure_id {
				v.kind = 'treasure'
				v.id = tid
				v.name = lang_name(tr_info.names, tid, lang)
				v.image = tr_info.images[tid]
			} else if p_id := e.pet_id {
				v.kind = 'pet'
				v.id = p_id
				v.name = lang_name(pt.names, p_id, lang)
				v.image = pt.images[p_id]
			}
			views << v
		}
		out << GachaPoolView{
			pool_id: pid
			name:    p.name
			tier:    p.tier
			entries: views
		}
	}
	return out
}
