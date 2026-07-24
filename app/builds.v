module app

import veb

pub fn (wapp &App) builds(mut ctx Context) veb.Result {
	return $veb.html()
}