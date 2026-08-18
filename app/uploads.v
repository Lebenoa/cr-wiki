module app

import os

// image_slug turns a display name into the snake_case stem every seed image
// is named after: "Mint Choco Cookie" -> "mint_choco_cookie".
pub fn image_slug(name string) string {
	mut out := []u8{cap: name.len}
	for c in name.to_lower() {
		if (c >= `a` && c <= `z`) || (c >= `0` && c <= `9`) {
			out << c
		} else if c == `'` {
			// possessives read better closed up: squirrel's -> squirrels
			continue
		} else if out.len > 0 && out.last() != `_` {
			out << `_`
		}
	}
	return out.bytestr().trim_right('_')
}

// upload_image saves the uploaded 'image' form file into static/img/<dir>,
// named after the entity (image_slug(name) + the uploaded extension) so the
// tree keeps one naming convention. Re-uploading for the same name overwrites,
// which is what an edit wants; two entities sharing a display name also share
// the file. Returns '' when no file was uploaded, the saved filename on
// success, or an error for an unsupported file type / write failure.
pub fn upload_image(mut ctx Context, dir string, name string) !string {
	if files := ctx.files['image'] {
		if files.len > 0 && files[0].filename != '' {
			file := files[0]
			ext := os.base(file.filename).all_after_last('.').to_lower()
			if ext !in ['png', 'jpg', 'jpeg', 'webp', 'gif', 'avif'] {
				return error('Invalid image: expected png, jpg, jpeg, webp, gif, or avif')
			}
			slug := image_slug(name)
			if slug == '' {
				return error('Invalid image: a name is required to save the file under')
			}
			filename := '${slug}.${ext}'
			os.mkdir_all(os.join_path('static', 'img', dir)) or {}
			os.write_file(os.join_path('static', 'img', dir, filename), file.data) or {
				return error('Failed to save image: ${err}')
			}
			return filename
		}
	}
	return ''
}
