// Horizontal-overflow guard.
//
// A page that scrolls sideways on a phone is broken however it looks on the
// machine it was written on, and no Rust assertion can see layout — so the
// layout test renders the pages through the real router, and this script
// measures what a browser actually lays out.
//
// It serves the rendered fixtures as `/` and the repo's `static/` tree as
// `/static/` (the asset URLs in the markup are absolute, so the stylesheet and
// sprites only resolve under that prefix), drives a headless Chromium over the
// DevTools protocol, and reports every page that scrolls sideways at any width
// it was asked for — either because a box sticks out of the scrollport or
// because the document can still be pushed sideways past the gutter a classic
// scrollbar reserves (which is what a body sized to the viewport does).
//
// usage: bun tools/overflow_check.mjs [--fixtures DIR] [--static DIR] [--widths 320,768,1024]
//
// exit codes: 0 every page fits, 1 at least one page overflows, 2 there is no
// browser to measure with — the caller skips on that rather than failing a
// machine that has no Chromium, the way the database tests skip without
// CR_SURREAL_URL. `CR_BROWSER` names the executable when the search misses it.

import { existsSync, mkdtempSync, readdirSync, rmSync, statSync } from "node:fs";
import { tmpdir } from "node:os";
import { extname, join, resolve, sep } from "node:path";

const TYPES = {
    ".html": "text/html; charset=utf-8",
    ".css": "text/css; charset=utf-8",
    ".js": "text/javascript; charset=utf-8",
    ".mjs": "text/javascript; charset=utf-8",
    ".json": "application/json; charset=utf-8",
    ".xml": "application/xml; charset=utf-8",
    ".txt": "text/plain; charset=utf-8",
    ".svg": "image/svg+xml",
    ".png": "image/png",
    ".jpg": "image/jpeg",
    ".jpeg": "image/jpeg",
    ".webp": "image/webp",
    ".gif": "image/gif",
    ".avif": "image/avif",
    ".ico": "image/x-icon",
    ".woff": "font/woff",
    ".woff2": "font/woff2",
    ".mp3": "audio/mpeg",
    ".wasm": "application/wasm",
};

/// Sub-pixel rounding can leave a pixel of scroll range, so only a real one —
/// something that cannot fit — is a failure. The navbar regression this guards
/// against measured 57px, not 1px.
const TOLERANCE = 1;
const LOAD_TIMEOUT_MS = 30_000;
const LAUNCH_TIMEOUT_MS = 20_000;

/// The measurement runs in the page: settle, then report the document's own
/// overflow plus the elements sticking out of it, which is what a human would
/// have to hunt for when the number alone does not say who is to blame.
const MEASURE = `(async () => {
    if (document.fonts && document.fonts.ready) {
        await Promise.race([document.fonts.ready, new Promise((r) => setTimeout(r, 1500))]);
    }
    // a transition parked mid-flight (display is discrete now) measures as the
    // state it is leaving, not the one it settles into
    for (const a of document.getAnimations()) { try { a.finish(); } catch (e) {} }
    await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));

    const de = document.documentElement;
    const viewport = de.clientWidth;
    // How far the document can be pushed sideways, minus the space a classic
    // scrollbar reserves: a stable scrollbar-gutter lives inside the root's own
    // scrollable range, so the raw range reports 15-odd pixels on a tall page
    // that has nothing wide in it at all.
    const gutter = Math.max(0, window.innerWidth - viewport);
    const before = window.scrollX || de.scrollLeft;
    de.scrollLeft = 100000;
    const scrolledBy = Math.round(Math.max(window.scrollX || 0, de.scrollLeft));
    de.scrollLeft = before;
    const range = Math.max(0, scrolledBy - gutter);

    const overflows = (el) => {
        const r = el.getBoundingClientRect();
        return r.width > 0 && r.height > 0 && (r.right > viewport + 0.5 || r.left < -0.5);
    };
    // a box inside a container that scrolls sideways on its own is clipped
    // there, the way a wide code block is: it never widens the page
    const clipped = (el) => {
        for (let p = el.parentElement; p && p !== document.body; p = p.parentElement) {
            if (getComputedStyle(p).overflowX !== "visible") return true;
        }
        return false;
    };
    // a body stretched by one wide child reaches the same edge as the child,
    // so naming every overflowing box buries the culprit: the boxes worth
    // reporting are the ones whose parent already fits, where the extra width
    // enters the page
    const offenders = [];
    for (const el of document.querySelectorAll("body, body *")) {
        const style = getComputedStyle(el);
        // fixed boxes rule the viewport instead of widening the page: a popover
        // pinned near an edge is not what makes a document scroll sideways
        if (style.position === "fixed" || style.display === "none") continue;
        if (!overflows(el)) continue;
        const parent = el.parentElement;
        if (parent && overflows(parent)) continue;
        if (clipped(el)) continue;
        const r = el.getBoundingClientRect();
        offenders.push({
            tag: el.tagName.toLowerCase(),
            id: el.id,
            cls: String(el.className || "").slice(0, 70),
            left: Math.round(r.left),
            right: Math.round(r.right),
        });
    }
    offenders.sort((a, b) => b.right - a.right);
    // a document can come out exactly viewport-wide with nothing sticking out
    // (a box sized to the viewport does that), and then the widest boxes are
    // the only clue to who set the page's width
    const widest = [];
    for (const el of document.querySelectorAll("body, body *")) {
        const r = el.getBoundingClientRect();
        if (r.width > 0) {
            widest.push({
                tag: el.tagName.toLowerCase(),
                id: el.id,
                cls: String(el.className || "").slice(0, 60),
                width: Math.round(r.width),
            });
        }
    }
    widest.sort((a, b) => b.width - a.width);
    return {
        url: location.href,
        viewport,
        scrollWidth: de.scrollWidth,
        scrolledBy: range,
        offenders: offenders.slice(0, 6),
        widest: widest.slice(0, 5),
    };
})()`;

function usage() {
    console.log("usage: bun tools/overflow_check.mjs [--fixtures DIR] [--static DIR] [--widths 320,768,1024]");
}

function parseArgs(argv) {
    const opts = { fixtures: "target/overflow", static: "static", widths: null };
    for (let i = 0; i < argv.length; i++) {
        const arg = argv[i];
        if (arg === "--help" || arg === "-h") {
            usage();
            process.exit(0);
        }
        const [flag, inline] = arg.includes("=") ? arg.split(/=(.*)/s, 2) : [arg, null];
        const value = inline ?? argv[++i];
        if (flag === "--fixtures") opts.fixtures = value;
        else if (flag === "--static") opts.static = value;
        else if (flag === "--widths") opts.widths = value;
        else {
            console.error(`unknown argument: ${arg}`);
            usage();
            process.exit(1);
        }
    }
    const widths = (opts.widths ?? process.env.CR_OVERFLOW_WIDTHS ?? "320,768,1024")
        .split(",")
        .map((w) => Number.parseInt(w.trim(), 10))
        .filter((w) => Number.isFinite(w) && w > 0);
    if (widths.length === 0) {
        console.error("no usable --widths");
        process.exit(1);
    }
    return { ...opts, widths };
}

/// The browser is whatever this machine has: Playwright's cached Chromium (the
/// app's own browser panel uses it), then the system Chrome and Edge. A
/// `CR_BROWSER` path wins so a machine can point at something else.
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
    // playwright keeps several versions side by side, newest last
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
    // a headless shell boots faster and cannot pop a window; a full browser is
    // the fallback when only it is installed
    for (const path of [...builds, ...system]) {
        if (existsSync(path)) return path;
    }
    return null;
}

function contentType(path) {
    return TYPES[extname(path).toLowerCase()] ?? "application/octet-stream";
}

/// Fixtures are served from their directory and `/static/...` from the repo's
/// tree — the layout only matches the real site if the stylesheet and the
/// sprites load from the prefixes the markup names.
function serveFixtures(dir, staticDir) {
    const root = resolve(dir);
    const staticRoot = resolve(staticDir);
    const fileFor = (path) => {
        const within = (base, candidate) =>
            candidate === base || candidate.startsWith(base + sep);
        const full = resolve(path);
        if (!within(root, full) && !within(staticRoot, full)) return null;
        try {
            return statSync(full).isFile() ? full : null;
        } catch {
            return null;
        }
    };
    return Bun.serve({
        hostname: "127.0.0.1",
        port: 0,
        fetch(req) {
            const url = new URL(req.url);
            const path = decodeURIComponent(url.pathname);
            const target = path.startsWith("/static/")
                ? fileFor(join(staticRoot, path.slice("/static/".length)))
                : fileFor(join(root, path.replace(/^\/+/, "")));
            if (!target) return new Response(path === "/" ? "index missing" : "not found", { status: 404 });
            return new Response(Bun.file(target), { headers: { "content-type": contentType(target) } });
        },
    });
}

async function pickPort() {
    const probe = Bun.serve({ hostname: "127.0.0.1", port: 0, fetch: () => new Response("") });
    const port = probe.port;
    await probe.stop(true);
    return port;
}

/// Chrome only prints its endpoint, and only once it is ready; polling the
/// port is steadier than parsing a banner that some builds truncate.
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
/// which is all a layout measurement needs.
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

    /// One event, with the session it must belong to: the load event of the
    /// navigation this waiter was set up for, not of the previous page.
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
    const fixtures = resolve(opts.fixtures);

    let files = [];
    try {
        files = readdirSync(fixtures).filter((f) => f.endsWith(".html")).sort();
    } catch {
        console.error(`no fixture directory at ${fixtures} — the layout test writes it`);
        process.exit(1);
    }
    if (files.length === 0) {
        console.error(`${fixtures} holds no .html fixtures — the layout test writes them`);
        process.exit(1);
    }

    const exe = findBrowser();
    if (!exe) {
        console.error("skip: no Chromium/Chrome/Edge found — set CR_BROWSER to one to run the overflow check");
        process.exit(2);
    }

    const server = serveFixtures(fixtures, resolve(opts.static));
    const profile = mkdtempSync(join(tmpdir(), "cr-overflow-"));
    const origin = `http://127.0.0.1:${server.port}`;
    let browser;
    let devtools;
    try {
        browser = await launchBrowser(exe, profile);
        devtools = await Devtools.connect(browser.endpoint, LAUNCH_TIMEOUT_MS);
        const { targetId } = await devtools.send("Target.createTarget", { url: "about:blank" });
        const { sessionId } = await devtools.send("Target.attachToTarget", { targetId, flatten: true });
        await devtools.send("Page.enable", {}, sessionId);
        await devtools.send("Runtime.enable", {}, sessionId);

        console.log(
            `overflow check: ${files.length} pages x ${opts.widths.join("/")}px, ${exe.split(sep).pop()}`,
        );
        let failures = 0;
        for (const file of files) {
            const cells = [];
            const offenders = [];
            for (const width of opts.widths) {
                await devtools.send(
                    "Emulation.setDeviceMetricsOverride",
                    { width, height: 900, deviceScaleFactor: 1, mobile: false },
                    sessionId,
                );
                const loaded = devtools.once("Page.loadEventFired", sessionId, LOAD_TIMEOUT_MS);
                await devtools.send("Page.navigate", { url: `${origin}/${file}` }, sessionId);
                await loaded;
                const evaluated = await devtools.send(
                    "Runtime.evaluate",
                    { expression: MEASURE, awaitPromise: true, returnByValue: true },
                    sessionId,
                );
                // the protocol answers a remote object; with returnByValue the
                // measurement is under `.value`, and anything else means the
                // expression did not run — never that the page passed
                if (evaluated.exceptionDetails) {
                    const text = evaluated.exceptionDetails.exception?.description ?? evaluated.exceptionDetails.text;
                    throw new Error(`${file}: the measurement threw: ${text}`);
                }
                const result = evaluated.result?.value;
                if (!result || typeof result.scrolledBy !== "number") {
                    throw new Error(`${file}: the page reported no layout to measure`);
                }
                // either a box sticks out of the scrollport, or the document
                // can be pushed sideways past its reserved gutter: the first
                // names a culprit, the second catches width that comes from
                // something without a box of its own
                const failed = result.offenders.length > 0 || result.scrolledBy > TOLERANCE;
                if (failed) {
                    failures++;
                    cells.push(`${width}px FAIL +${result.scrolledBy}`);
                    offenders.push({ width, result });
                } else {
                    cells.push(`${width}px ok`);
                }
            }
            console.log(`  ${file.padEnd(28)} ${cells.join("  ")}`);
            for (const { width, result } of offenders) {
                const reason =
                    result.offenders.length > 0
                        ? "content sticks out of the scrollport"
                        : `the document scrolls ${result.scrolledBy}px sideways`;
                console.log(
                    `      at ${width}px ${result.url}: ${reason}\n` +
                        `        content is ${result.scrollWidth}px wide for a ${result.viewport}px scrollport`,
                );
                for (const el of result.offenders) {
                    const id = el.id ? `#${el.id}` : "";
                    console.log(`        ${el.tag}${id} [${el.left}..${el.right}] ${el.cls}`);
                }
                if (result.offenders.length === 0) {
                    for (const el of result.widest) {
                        const id = el.id ? `#${el.id}` : "";
                        console.log(`        widest: ${el.tag}${id} ${el.width}px ${el.cls}`);
                    }
                }
            }
        }
        if (failures > 0) {
            console.error(`FAIL: ${failures} page/width combination${failures === 1 ? "" : "s"} scroll sideways`);
            process.exitCode = 1;
        } else {
            console.log("ok: no page scrolls horizontally at these widths");
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
