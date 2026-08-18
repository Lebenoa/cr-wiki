module models

// EconomyTable is one scraped table from cookierundb's economy pages (league
// tiers, player levels, the Party Run Pass tracks, stat upgrades). Unlike the
// rest of the catalog these pages carry no entities — they are rendered
// tables whose column set differs per page (2 to 5 columns) and per pass
// season. Storing them generically (headers here, cells in EconomyRow) keeps
// every page lossless without inventing a schema per table; the templates
// render them as tables anyway.
@[table: 'economy_table']
@[unique_key: 'section, sort_order']
pub struct EconomyTable {
pub:
	economy_table_id ?int @[primary; serial]
	section          string @[required; index] // league | levels | pass | upgrades
	title            string // page title ("Party Run Pass")
	caption          string // per-table heading, '' when the page has one table
	headers          string // JSON array of header cells
	sort_order       int    // table position within the section
}

// EconomyRow is one body row of an EconomyTable. cells is a JSON array of the
// row's cells, in header order, kept as scraped text ("top 90-100%", "1,300")
// since these columns mix labels, ranges and formatted numbers.
@[table: 'economy_row']
pub struct EconomyRow {
pub:
	economy_row_id   ?int @[primary; serial]
	economy_table_id int @[required; references: 'economy_table(economy_table_id)'; index]
	cells            string @[required]
	sort_order       int
}
