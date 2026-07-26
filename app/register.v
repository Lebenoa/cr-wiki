module app

import veb

@[get; post]
pub fn (wapp &App) register(mut ctx Context) veb.Result {
	if ctx.req.method == .post {
		panic("TODO")
	} else if ctx.req.method == .get {
		return $veb.html()
	}

	return ctx.not_found()
}
