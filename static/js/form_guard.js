// Submission guard for the site's forms.
//
// Two jobs, both about not offering a press that cannot work:
//
// 1. A form carrying a Turnstile widget stays locked until the widget hands
//    back a token, and locks again once that token is spent — the token is
//    single-use, so the next attempt has to pass the challenge first.
// 2. Every form refuses a second submission while one is in flight.
//
// No user-facing wording lives here: the checking line is rendered (and
// translated) by askama, and the fallback line arrives in
// `data-turnstile-timeout`, so this file stays locale-free.
//
// Timing matters as much as the logic. htmx builds its request out of the
// submitting form and refuses to send at all when the submitter it picked is
// disabled — so the in-flight disable cannot happen inside the submit event.
// For an htmx form it happens on `htmx:before:request`, after the request is
// built; for a native form it is deferred a tick, because a button disabled
// during the submit event is left out of the browser's own payload.
(function () {
    "use strict";

    // A blocked or offline Turnstile CDN would otherwise hold the button down
    // forever. After this long without the widget script the button is handed
    // back with the failure line: a release build still refuses a tokenless
    // POST, but the visitor gets a reason instead of a dead control.
    var WIDGET_SCRIPT_TIMEOUT_MS = 12000;

    // A native submit that never navigates — a cancelled post, a handler that
    // swallowed the submit — would otherwise leave its button disabled with no
    // request to end.
    var NATIVE_BUSY_TIMEOUT_MS = 10000;

    // Guards already built, keyed by form. Weak so a swapped-away form is
    // collectable.
    var guards = new WeakMap();

    function guardFor(form) {
        var guard = guards.get(form);
        if (guard) return guard;

        var hint = form.querySelector("[data-form-guard-hint]");
        guard = {
            htmx: form.hasAttribute("hx-post") || form.hasAttribute("hx-get") ||
                form.hasAttribute("hx-put") || form.hasAttribute("hx-delete") ||
                form.hasAttribute("hx-patch"),
            widget: form.querySelector(".cf-turnstile"),
            buttons: form.querySelectorAll("button[type=submit]"),
            hint: hint,
            hintText: hint ? hint.textContent : "",
            notice: null,
            ready: false,
            busy: false
        };
        guards.set(form, guard);
        return guard;
    }

    // The page's one widget and the form it belongs to. Turnstile's implicit
    // rendering calls its callbacks without handing over the element, and every
    // page here carries a single widget, so this lookup is unambiguous.
    function widgetForm() {
        var widget = document.querySelector(".cf-turnstile");
        return widget ? widget.closest("form") : null;
    }

    function awaitingToken(guard) {
        return guard.widget !== null && !guard.ready;
    }

    function paint(guard) {
        var locked = awaitingToken(guard);
        for (var i = 0; i < guard.buttons.length; i += 1) {
            var button = guard.buttons[i];
            button.disabled = locked || guard.busy;
            if (guard.busy) {
                button.setAttribute("aria-busy", "true");
            } else {
                button.removeAttribute("aria-busy");
            }
        }
        if (guard.hint) {
            guard.hint.textContent = guard.notice || guard.hintText;
            guard.hint.hidden = !(locked && !guard.busy);
        }
    }

    function paintAll() {
        var forms = document.querySelectorAll("form");
        for (var i = 0; i < forms.length; i += 1) {
            var guard = guards.get(forms[i]);
            if (guard) paint(guard);
        }
    }

    function resetWidget(guard) {
        if (!window.turnstile || typeof window.turnstile.reset !== "function") return;
        try {
            window.turnstile.reset(guard.widget);
        } catch (_) {
            // A widget torn out by a swap has nothing left to reset.
        }
    }

    // Turnstile callbacks can only name globals, so these are on `window`.
    // Neither receives the element, hence the page-wide widget lookup.
    window.crTurnstileToken = function () {
        var form = widgetForm();
        if (!form) return;
        var guard = guardFor(form);
        guard.ready = true;
        guard.notice = null;
        paint(guard);
    };

    // An expired token is replaced automatically by the widget, so the form
    // waits for the next one.
    window.crTurnstileSpent = function () {
        var form = widgetForm();
        if (!form) return;
        var guard = guardFor(form);
        guard.ready = false;
        paint(guard);
    };

    // A widget that failed outright will never mint a token, so holding the
    // button down would dead-end the form. Hand it back with the reason: the
    // server still makes the call, and debug builds skip the check entirely.
    window.crTurnstileError = function () {
        var form = widgetForm();
        if (!form) return;
        var guard = guardFor(form);
        guard.ready = true;
        guard.notice = form.getAttribute("data-turnstile-timeout");
        paint(guard);
    };

    // Capture phase on the document, so this runs before htmx's own handler and
    // can cancel the request outright.
    document.addEventListener(
        "submit",
        function (event) {
            var form = event.target;
            if (!form || form.tagName !== "FORM") return;
            var guard = guards.get(form);
            if (!guard) return;

            if (awaitingToken(guard) || guard.busy) {
                event.preventDefault();
                event.stopPropagation();
                if (!guard.busy) paint(guard);
                return;
            }

            guard.busy = true;
            if (guard.htmx) return; // played out on htmx:before:request

            window.setTimeout(function () {
                if (guard.busy) paint(guard);
            }, 0);
            window.setTimeout(function () {
                if (!guard.busy) return;
                guard.busy = false;
                paint(guard);
            }, NATIVE_BUSY_TIMEOUT_MS);
        },
        true
    );

    // htmx 4 names lifecycle events htmx:phase:action. before:request is the
    // first moment the request already exists, which is exactly when the
    // button may safely go down.
    document.addEventListener("htmx:before:request", function (event) {
        if (!event.target || event.target.tagName !== "FORM") return;
        var guard = guards.get(event.target);
        if (guard) paint(guard);
    });

    function settle(form) {
        var guard = guards.get(form);
        if (!guard) return;
        guard.busy = false;
        if (guard.widget) {
            // The server just spent the token, or never received it; either way
            // the next attempt needs a fresh challenge.
            guard.ready = false;
            resetWidget(guard);
        }
        paint(guard);
        // htmx restores the elements it disabled when the request ends, which
        // can land after the guard has decided the form is still locked.
        window.setTimeout(function () {
            paint(guard);
        }, 0);
    }

    // finally:request is the one "the request is over" event — it fires for a
    // success, an HTTP error and an abort alike. htmx:error catches whatever
    // breaks outside that path.
    ["htmx:finally:request", "htmx:error"].forEach(function (name) {
        document.addEventListener(name, function (event) {
            if (event.target && event.target.tagName === "FORM") settle(event.target);
        });
    });

    ["htmx:after:settle", "htmx:after:swap"].forEach(function (name) {
        document.addEventListener(name, function () {
            window.setTimeout(paintAll, 0);
        });
    });

    // A native form navigates away on submit, but a back/forward restore would
    // otherwise bring back a permanently disabled button.
    window.addEventListener("pageshow", function () {
        var forms = document.querySelectorAll("form");
        for (var i = 0; i < forms.length; i += 1) {
            var guard = guards.get(forms[i]);
            if (guard && guard.busy) {
                guard.busy = false;
                paint(guard);
            }
        }
    });

    function init() {
        var forms = document.querySelectorAll("form");
        for (var i = 0; i < forms.length; i += 1) paint(guardFor(forms[i]));
    }

    if (document.readyState === "loading") {
        document.addEventListener("DOMContentLoaded", init);
    } else {
        init();
    }

    window.addEventListener("load", function () {
        window.setTimeout(function () {
            if (window.turnstile) return;
            var form = widgetForm();
            if (!form) return;
            var guard = guardFor(form);
            guard.ready = true; // the server still enforces the check
            guard.notice = form.getAttribute("data-turnstile-timeout");
            paint(guard);
        }, WIDGET_SCRIPT_TIMEOUT_MS);
    });
})();
