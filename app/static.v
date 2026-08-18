module app

import os

// Static files are served from a cache the app owns rather than from
// veb.StaticHandler: veb scans the folder once at startup and never learns
// about files written later, so an admin upload would 404 until a restart.
// The cache here is `shared` (App.static_files), so upload_image can add its
// file to it while request threads read it.

// static_mime_types covers the extensions under static/, plus the image types
// upload_image accepts. An extension that is not listed is not served.
const static_mime_types = {
	'.avif':  'image/avif'
	'.css':   'text/css; charset=utf-8'
	'.gif':   'image/gif'
	'.ico':   'image/x-icon'
	'.jpeg':  'image/jpeg'
	'.jpg':   'image/jpeg'
	'.js':    'text/javascript; charset=utf-8'
	'.json':  'application/json'
	'.png':   'image/png'
	'.svg':   'image/svg+xml'
	'.webp':  'image/webp'
	'.woff2': 'font/woff2'
}

// static_url is the request path a file under `static/` is served at:
// static/img/cookies/x.png -> /img/cookies/x.png.
fn static_url(file_path string) string {
	rel := file_path.replace(os.path_separator, '/').trim_string_left('static')
	return if rel.starts_with('/') { rel } else { '/${rel}' }
}

// scan_static_dir walks `dir` and records every file with a known MIME type.
// Dotfiles are skipped, as are extensions static_mime_types does not cover.
fn scan_static_dir(mut files map[string]string, dir string) ! {
	for entry in os.ls(dir)! {
		if entry.starts_with('.') {
			continue
		}
		path := os.join_path(dir, entry)
		if os.is_dir(path) {
			scan_static_dir(mut files, path)!
		} else if os.file_ext(entry).to_lower() in static_mime_types {
			files[static_url(path)] = path
		}
	}
}

// serve_static answers a request from the static cache. It is registered as
// the first global middleware, which veb runs before routing, so static files
// keep taking precedence over routes; returning false ends the chain.
pub fn (mut wapp App) serve_static(mut ctx Context) bool {
	url := ctx.req.url.all_before('?')
	mut path := ''
	rlock wapp.static_files {
		path = wapp.static_files[url] or { '' }
	}
	if path == '' {
		return true
	}
	ctx.content_type = static_mime_types[os.file_ext(path).to_lower()] or { return true }
	ctx.file(path)
	return false
}

// remember_static adds a file written at runtime to the cache, so it is served
// from the next request on instead of only after a restart.
fn (mut wapp App) remember_static(file_path string) {
	lock wapp.static_files {
		wapp.static_files[static_url(file_path)] = file_path
	}
}
