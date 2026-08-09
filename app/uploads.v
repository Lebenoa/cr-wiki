module app

import os

// upload_image saves the uploaded 'image' form file into static/img/<dir>.
// Returns '' when no file was uploaded, the saved filename on success,
// or an error for an unsupported file type / write failure.
pub fn upload_image(mut ctx Context, dir string) !string {
	if files := ctx.files['image'] {
		if files.len > 0 && files[0].filename != '' {
			file := files[0]
			filename := os.base(file.filename)
			ext := filename.all_after_last('.').to_lower()
			if ext !in ['png', 'jpg', 'jpeg', 'webp', 'gif', 'avif'] {
				return error('Invalid image: expected png, jpg, jpeg, webp, gif, or avif')
			}
			os.mkdir_all(os.join_path('static', 'img', dir)) or {}
			os.write_file(os.join_path('static', 'img', dir, filename), file.data) or {
				return error('Failed to save image: ${err}')
			}
			return filename
		}
	}
	return ''
}
