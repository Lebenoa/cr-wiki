module api

import os
import i18n

@[inline]
pub fn available_lang() []string {
	files := os.walk_ext(i18n.default_translations_dir, '.tr')
	mut langs := []string{}
	for file in files {
		filename := os.file_name(file)
		if filename == "lang_map.tr" {
			continue
		}
		langs << filename.all_before_last('.tr')
	}
	return langs
}
