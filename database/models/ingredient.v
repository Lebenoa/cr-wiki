module models

// Ingredient is a crafting material. Numeric facts (coin value, powder
// yields) are parsed from the catalog's text; obtained_from stays free text.
@[table: 'ingredient']
pub struct Ingredient {
pub:
	ingredient_id     ?int @[primary; serial]
	image             ?string
	grade             ?int // models.Grade value (scrape: A/B/C/S/S+); none = ungraded
	drop_episode_id   ?int @[index; references: 'episode(episode_id)'] // episode it appears in
	coin_value        int
	breaks_into_powder int
	craft_from_powder int
	obtained_from     string // "Fortune Cookie dough · Mystery Boxes · …"
}

@[table: 'ingredient_translation']
@[unique_key: 'ingredient_id, lang']
pub struct IngredientTranslation {
pub:
	ingredient_translation_id ?int @[primary; serial]

	ingredient_id int @[required; references: 'ingredient(ingredient_id)'; index]
	lang          string @[required; index]

	name        string @[required]
	description string
}

// IngredientRecipe links a crafting ingredient to the treasure it is used in.
@[table: 'ingredient_recipe']
@[unique_key: 'ingredient_id, treasure_id']
pub struct IngredientRecipe {
pub:
	ingredient_recipe_id ?int @[primary; serial]
	ingredient_id        int @[required; references: 'ingredient(ingredient_id)'; index]
	treasure_id          int @[required; references: 'treasure(treasure_id)'; index]
}
