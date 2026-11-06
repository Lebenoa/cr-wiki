// Form-guard state-machine check.
//
// `static/js/form_guard.js` is entirely about what a control looks like at a
// moment — locked until Turnstile hands back a token, open once it does,
// locked and not busy again after a submission comes back refused. None of
// that is visible to an assertion about markup or about the response body, so
// this drives the real file in a real browser: a fixture page carrying the
// markup contract the templates emit (a `.cf-turnstile` widget, a hint line,
// `data-turnstile-timeout`, an `hx-post` form), the guard loaded from the
// repo's `static/` tree, and the events htmx 4 dispatches replayed in the
// order a failed request produces them.
//
// usage: bun tools/form_guard_check.mjs [--static DIR]
//
// exit codes: 0 the state machine holds, 1 it regressed, 2 there is no browser
// to run it in — the caller skips on that rather than failing a machine with
// no Chromium, the way the database tests skip without `CR_SURREAL_URL`.
// `CR_BROWSER` names the executable when the search misses it.

import { existsSync, mkdtempSync, readdirSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve, sep } from "node:path";

const LAUNCH_TIMEOUT_MS = 20_000;
const LOAD_TIMEOUT_MS = 30_000;

/// The markup the templates emit, minus everything the guard does not read.
/// The button is deliberately left enabled here: if the guard never ran, the
/// first check would see an open form and say so.
const FIXTURE = `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>form guard</title>
<script>window.turnstile = { reset: function () { window.__resetCalls = (window.__resetCalls || 0) + 1; } };</script>
<script src="/static/js/form_guard.js"></script>
</head>
<body>
<form action="/login" hx-post="/login" data-turnstile-timeout="Bot check failed">
<div class="cf-turnstile"></div>
<input name="username">
<button type="submit">Sign in</button>
<p data-form-guard-hint hidden>Verifying</p>
</form>
</body>
</html>
`;

/// Runs in the page. htmx is not loaded — its events are dispatched by hand,
/// because the guard's contract is the event names and their order, and a
/// failed login produces exactly `submit` -> `htmx:before:request` ->
/// `htmx:finally:request`.
const RUN = `(async () => {
    const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
    const form = document.querySelector("form");
    const button = form.querySelector("button[type=submit]");
    const hint = form.querySelector("[data-form-guard-hint]");
    const checks = [];
    const now = () => ({
        disabled: button.disabled,
        busy: button.getAttribute("aria-busy"),
        hintHidden: hint.hidden,
    });
    const expect = (label, want, got = now()) => {
        const pass = got.disabled === want.disabled &&
            got.busy === want.busy &&
            got.hintHidden === want.hintHidden;
        checks.push({ label, pass, want, got });
    };
    const fire = (name) =>
        form.dispatchEvent(new Event(name, { bubbles: true, cancelable: true }));

    checks.push({
        label: "guard script loaded",
        pass: typeof window.crTurnstileToken === "function",
        want: "function",
        got: typeof window.crTurnstileToken,
    });

    // no token yet: the press is not offered, and the reason is on screen
    expect("locked before a token", { disabled: true, busy: null, hintHidden: false });

    // the widget hands back a token: the press is offered
    window.crTurnstileToken();
    expect("unlocked on a token", { disabled: false, busy: null, hintHidden: true });

    // a request goes out and is refused. The guard cannot disable inside the
    // submit event (htmx refuses a request whose submitter is disabled), so it
    // must be down while the request flies and settle afterwards.
    fire("submit");
    fire("htmx:before:request");
    expect("busy while a request is in flight", {
        disabled: true, busy: "true", hintHidden: true,
    });

    fire("htmx:finally:request");
    // settle repaints once more on a timer, to beat htmx restoring the
    // elements it disabled
    await sleep(30);
    expect("locked and not busy after a failed submission", {
        disabled: true, busy: null, hintHidden: false,
    });

    // the token was spent, so the next attempt needs a fresh challenge
    checks.push({
        label: "asked the widget for a new token",
        pass: window.__resetCalls === 1,
        want: 1,
        got: window.__resetCalls ?? 0,
    });

    // and the form opens again when that token lands
    window.crTurnstileToken();
    expect("unlocked again on the next token", {
        disabled: false, busy: null, hintHidden: true,
    });

    return { checks, url: location.href };
})()`;

function usage() {
    console.log("usage: bun tools/form_guard_check.mjs [--static DIR]");
}

function parseArgs(argv) {
    const opts = { static: "static" };
    for (let i = 0; i < argv.length; i += 1) {
        const arg = argv[i];
        if (arg === "--help" || arg === "-h") {
            usage();
            process.exit(0);
        }
        const [flag, inline] = arg.includes("=") ? arg.split(/=(.*)/s, 2) : [arg, null];
        const value = inline ?? argv[++i];
        if (flag === "--static") opts.static = value;
        else {
            console.error(`unknown argument: ${arg}`);
            usage();
            process.exit(1);
        }
    }
    return opts;
}

/// The browser is whatever this machine has: Playwright's cached Chromium (the
/// app's own browser panel uses it), then the system Chrome and Edge.
function findBrowser() {
    const explicit = process.env.CR_BROWSER;
    if (explicit) {
        if (!existsSync(explicit)) {
            console.error(`CR_BROWSER points at ${explicit}, which does not exist`);
            process.exit(2);
        }
        return explicit;
    }
    const roots = [];
    const local = process.env.LOCALAPPDATA;
    if (local) roots.push(join(local, "ms-playwright"));
    const home = process.env.HOME || process.env.USERPROFILE;
    if (home) {
        roots.push(join(home, "AppData/Local/ms-playwright"));
        roots.push(join(home, ".cache/ms-playwright"));
        roots.push(join(home, "Library/Caches/ms-playwright"));
    }
    const builds = [];
    for (const root of roots) {
        if (!existsSync(root)) continue;
        let entries;
        try {
            entries = readdirSync(root).sort();
        } catch {
            continue;
        }
        for (const entry of entries) {
            for (const name of [
                "chrome-headless-shell-win64/chrome-headless-shell.exe",
                "chrome-win64/chrome.exe",
                "chrome-win/chrome.exe",
                "chrome-headless-shell-linux64/chrome-headless-shell",
                "chrome-linux64/chrome",
                "chrome-linux/chrome",
                "chrome-mac/Chromium.app/Contents/MacOS/Chromium",
            ]) {
                builds.push(join(root, entry, name));
            }
        }
    }
    const system = [
        "C:/Program Files/Google/Chrome/Application/chrome.exe",
        "C:/Program Files (x86)/Google/Chrome/Application/chrome.exe",
        "C:/Program Files/Microsoft/Edge/Application/msedge.exe",
        "C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe",
        "/usr/bin/google-chrome",
        "/usr/bin/chromium",
        "/usr/bin/chromium-browser",
        "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
        "/Applications/Chromium.app/Contents/MacOS/Chromium",
    ];
    for (const path of [...builds, ...system]) {
        if (existsSync(path)) return path;
    }
    return null;
}

async function pickPort() {
    const probe = Bun.serve({ hostname: "127.0.0.1", port: 0, fetch: () => new Response("") });
    const port = probe.port;
    await probe.stop(true);
    return port;
}

async function browserEndpoint(port, deadline) {
    while (Date.now() < deadline) {
        try {
            const res = await fetch(`http://127.0.0.1:${port}/json/version`);
            if (res.ok) {
                const body = await res.json();
                if (body.webSocketDebuggerUrl) return body.webSocketDebuggerUrl;
            }
        } catch {
            // not listening yet
        }
        await Bun.sleep(120);
    }
    return null;
}

/// A minimal DevTools-protocol client: request/response with session routing,
/// which is all driving one page needs.
class Devtools {
    constructor(ws) {
        this.ws = ws;
        this.next = 1;
        this.pending = new Map();
        this.listeners = new Map();
        ws.addEventListener("message", (event) => {
            const message = JSON.parse(event.data);
            if (message.id && this.pending.has(message.id)) {
                const { resolve, reject } = this.pending.get(message.id);
                this.pending.delete(message.id);
                if (message.error) reject(new Error(message.error.message));
                else resolve(message.result);
                return;
            }
            for (const listener of this.listeners.get(message.method) ?? []) listener(message);
        });
    }

    static async connect(url, timeoutMs) {
        const ws = new WebSocket(url);
        await Promise.race([
            new Promise((resolve, reject) => {
                ws.addEventListener("open", resolve);
                ws.addEventListener("error", () => reject(new Error(`cannot attach to ${url}`)));
            }),
            Bun.sleep(timeoutMs).then(() => {
                throw new Error("the devtools endpoint never opened");
            }),
        ]);
        return new Devtools(ws);
    }

    send(method, params = {}, sessionId) {
        const id = this.next++;
        return new Promise((resolve, reject) => {
            this.pending.set(id, { resolve, reject });
            this.ws.send(JSON.stringify(sessionId ? { id, method, params, sessionId } : { id, method, params }));
        });
    }

    once(method, sessionId, timeoutMs) {
        return new Promise((resolve, reject) => {
            const listener = (message) => {
                if (message.sessionId !== sessionId) return;
                clearTimeout(timer);
                this.listeners.get(method).delete(listener);
                resolve(message.params);
            };
            const timer = setTimeout(() => {
                this.listeners.get(method).delete(listener);
                reject(new Error(`${method} never arrived`));
            }, timeoutMs);
            if (!this.listeners.has(method)) this.listeners.set(method, new Set());
            this.listeners.get(method).add(listener);
        });
    }

    close() {
        try {
            this.ws.close();
        } catch {
            // nothing left to close
        }
    }
}

/// A load event does not promise the fixture is the document on screen — a
/// browser that blocks loopback, or one that keeps its own first-run page,
/// fires one anyway — so wait for the markup the harness reads before running
/// it, and say where the browser actually went when it never arrives.
async function waitForForm(devtools, sessionId, deadline) {
    while (Date.now() < deadline) {
        const probe = await devtools.send(
            "Runtime.evaluate",
            { expression: "document.querySelector('form') !== null", returnByValue: true },
            sessionId,
        );
        if (probe.result?.value === true) return true;
        await Bun.sleep(100);
    }
    return false;
}

async function launchBrowser(exe, profile) {
    const headlessShell = exe.includes("headless-shell") || exe.includes("headless_shell");
    const port = await pickPort();
    const args = [
        `--remote-debugging-port=${port}`,
        `--user-data-dir=${profile}`,
        "--no-first-run",
        "--no-default-browser-check",
        "--disable-extensions",
        "--disable-gpu",
        "--disable-dev-shm-usage",
        "--window-size=1280,900",
        // newer Chrome retired `--headless=new`; the shell is headless already
        ...(headlessShell ? [] : ["--headless"]),
        "about:blank",
    ];
    const child = Bun.spawn([exe, ...args], { stdout: "ignore", stderr: "ignore" });
    const endpoint = await browserEndpoint(port, Date.now() + LAUNCH_TIMEOUT_MS);
    if (!endpoint) {
        child.kill();
        throw new Error(`no devtools endpoint on port ${port} after ${LAUNCH_TIMEOUT_MS}ms`);
    }
    return { child, endpoint };
}

async function main() {
    const opts = parseArgs(process.argv.slice(2));
    const guard = join(resolve(opts.static), "js", "form_guard.js");
    if (!existsSync(guard)) {
        console.error(`no guard at ${guard}`);
        process.exit(1);
    }

    const exe = findBrowser();
    if (!exe) {
        console.error("skip: no Chromium/Chrome/Edge found — set CR_BROWSER to one to run the form guard check");
        process.exit(2);
    }

    const server = Bun.serve({
        hostname: "127.0.0.1",
        port: 0,
        fetch(req) {
            const path = new URL(req.url).pathname;
            if (path === "/") {
                return new Response(FIXTURE, { headers: { "content-type": "text/html; charset=utf-8" } });
            }
            if (path === "/static/js/form_guard.js") {
                return new Response(Bun.file(guard), {
                    headers: { "content-type": "text/javascript; charset=utf-8" },
                });
            }
            return new Response("not found", { status: 404 });
        },
    });

    const profile = mkdtempSync(join(tmpdir(), "cr-form-guard-"));
    let browser;
    let devtools;
    try {
        browser = await launchBrowser(exe, profile);
        devtools = await Devtools.connect(browser.endpoint, LAUNCH_TIMEOUT_MS);
        const { targetId } = await devtools.send("Target.createTarget", { url: "about:blank" });
        const { sessionId } = await devtools.send("Target.attachToTarget", { targetId, flatten: true });
        await devtools.send("Page.enable", {}, sessionId);
        await devtools.send("Runtime.enable", {}, sessionId);

        console.log(`form guard: ${exe.split(sep).pop()}`);
        const loaded = devtools.once("Page.loadEventFired", sessionId, LOAD_TIMEOUT_MS);
        await devtools.send("Page.navigate", { url: `http://127.0.0.1:${server.port}/` }, sessionId);
        await loaded;
        if (!(await waitForForm(devtools, sessionId, Date.now() + 5000))) {
            const where = await devtools.send(
                "Runtime.evaluate",
                { expression: "location.href", returnByValue: true },
                sessionId,
            );
            throw new Error(`the fixture never rendered — the browser is at ${where.result?.value}`);
        }

        const evaluated = await devtools.send(
            "Runtime.evaluate",
            { expression: RUN, awaitPromise: true, returnByValue: true },
            sessionId,
        );
        // the protocol answers a remote object; with returnByValue the report is
        // under `.value`, and anything else means the harness did not run —
        // never that the guard passed
        if (evaluated.exceptionDetails) {
            const text = evaluated.exceptionDetails.exception?.description ?? evaluated.exceptionDetails.text;
            throw new Error(`the state machine threw: ${text}`);
        }
        const report = evaluated.result?.value;
        if (!report || !Array.isArray(report.checks)) {
            throw new Error("the page reported no checks");
        }

        let failures = 0;
        for (const check of report.checks) {
            if (check.pass) {
                console.log(`  ok   ${check.label}`);
                continue;
            }
            failures += 1;
            console.log(`  FAIL ${check.label}`);
            console.log(`         want ${JSON.stringify(check.want)}`);
            console.log(`         got  ${JSON.stringify(check.got)}`);
        }
        if (failures > 0) {
            console.error(`FAIL: ${failures} guard state${failures === 1 ? "" : "s"} wrong at ${report.url}`);
            process.exitCode = 1;
        } else {
            console.log(`ok: ${report.checks.length} guard states hold`);
        }
    } finally {
        devtools?.close();
        browser?.child?.kill();
        await server.stop(true);
        try {
            rmSync(profile, { recursive: true, force: true });
        } catch {
            // the browser may still hold the profile on Windows; it is a temp dir
        }
    }
}

await main();
