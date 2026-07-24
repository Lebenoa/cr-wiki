module app

import veb

pub fn (wapp &App) pets(mut ctx Context) veb.Result {
	return $veb.html()
}