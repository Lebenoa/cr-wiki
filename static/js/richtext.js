// richtext.js — rich-text authoring helpers for admin fields.
//
// Any <textarea data-richtext> gets two features:
//   1. an entity-name autocomplete triggered by "[[" — "[[ging" lists cookies,
//      "[[pet:pep" pets, "[[treasure:..." treasures; Enter, Tab or click
//      completes the [[kind:id]] markup (ids, not names, so the stored text
//      stays language-independent), and
//   2. a live preview (the [data-richtext-preview] sibling box) that renders
//      the markup the same way render_rich_text does: [[kind:id]] become links
//      to the entity pages with the localized display name, {color:...}
//      becomes colored text, everything else is escaped.
//
// Both use /api/richtext-names?lang=<editing locale>&kind=... which returns
// { kind, names: [{id, name, en_name}] } for the language render_rich_text
// resolves (that language plus the en fallback); the preview resolves
// [[kind:id]] through this list to get the localized name and a real href.
//
// The editing locale is the admin form's own <select id="lang"> (which
// navigates with ?lang= on change, so the lists arrive for the right
// locale on every load); everywhere else — the build form — it is the
// page's locale.
//
// The dropdown is position:fixed and placed from getBoundingClientRect()
// (viewport coordinates); an absolutely-positioned body child would be offset
// by the page scroll and appear above the field on scrolled pages.
(function() {
    'use strict';

    var KINDS = ['cookie', 'pet', 'treasure'];
    var NAMES = { cookie: [], pet: [], treasure: [] }; // [{ id, name }]
    var fetchSeq = 0;   // guards out-of-order /api responses on lang switches
    var box = null;     // single dropdown reused across fields
    var current = null; // { textarea, kind, q, items, active }

    // ---- name lists -------------------------------------------------------

    // the locale the name lists and preview resolve in: the admin form's
    // language select when present (the locale being edited), else the page
    // locale — an admin editing th with an en UI must see th names
    function langOf() {
        var el = document.querySelector('form select#lang');
        if (el && el.value && i18nAvailable(el.value)) return el.value;
        var page = document.documentElement.lang;
        return page || 'en';
    }

    // mirrors i18n::is_available: only locales with a loaded .tr are real
    function i18nAvailable(lang) {
        return /^[a-z]{2}(-[a-z]{2})?$/i.test(lang);
    }

    function loadNames(cb) {
        var mySeq = ++fetchSeq;
        var remaining = KINDS.length;
        KINDS.forEach(function(kind) {
            fetch('/api/richtext-names?lang=' + encodeURIComponent(langOf()) + '&kind=' + kind)
                .then(function(r) { return r.json(); })
                .then(function(a) {
                    if (mySeq !== fetchSeq) return; // a newer lang request superseded this one
                    NAMES[kind] = a && Array.isArray(a.names) ? a.names : [];
                    if (--remaining === 0 && cb) cb();
                })
                .catch(function() {
                    if (mySeq !== fetchSeq) return;
                    NAMES[kind] = [];
                    if (--remaining === 0 && cb) cb();
                });
        });
    }

    // ---- markup helpers shared by autocomplete + preview -------------------

    // mirrors split_ref: "cookie:12" / "pet:5" / "treasure:9" -> { kind, id }.
    // Anything else — an untyped id included — is not a link and renders as
    // text. Names are not refs — authored markup must be language-independent,
    // so only ids link.
    function parseLink(inner) {
        var i = inner.indexOf(':');
        if (i === -1) return null;
        var kind = inner.slice(0, i).trim().toLowerCase();
        if (KINDS.indexOf(kind) === -1) return null;
        var id = parseInt(inner.slice(i + 1).trim(), 10);
        return id > 0 ? { kind: kind, id: id } : null;
    }

    // name lists are ordered page-language first (en fallback after), so the
    // id lookup yields the localized name the server renders for this page
    function findById(kind, id) {
        var list = NAMES[kind] || [];
        for (var i = 0; i < list.length; i++) {
            if (list[i].id === id) return list[i];
        }
        return null;
    }

    function kindPath(kind) {
        return kind === 'pet' ? 'pets' : kind === 'treasure' ? 'treasures' : 'cookies';
    }

    // the entity's sprite as a small inline thumbnail, mirroring the
    // server's render_with (same size-5 sprite ahead of the name)
    function thumbHtml(kind, opt) {
        if (!opt || !opt.image) return '';
        return '<img src="/static/img/' + kindPath(kind) + '/' + escapeHtml(opt.image) +
            '" alt="" loading="lazy" class="inline-block size-5 mr-1 object-contain align-text-bottom" />';
    }

    function escapeHtml(s) {
        return s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;')
            .replace(/"/g, '&quot;').replace(/'/g, '&#39;');
    }

    function validColor(val) {
        if (!val || val.length > 32) return false;
        return /^[a-zA-Z0-9#(),%.\- ]+$/.test(val);
    }

    // ---- autocomplete -----------------------------------------------------

    // kind + query for the open "[[" before the caret, or null
    function openQuery(ta) {
        var before = ta.value.slice(0, ta.selectionStart);
        var m = /\[\[([a-z]+):([^\[\]]*)$/i.exec(before);
        if (m && KINDS.indexOf(m[1].toLowerCase()) !== -1) {
            return { kind: m[1].toLowerCase(), q: m[2] };
        }
        var m2 = /\[\[([^\[\]]*)$/.exec(before);
        return m2 ? { kind: 'cookie', q: m2[1] } : null;
    }

    function matches(kind, q) {
        var ql = q.toLowerCase();
        var out = [];
        var list = NAMES[kind] || [];
        for (var i = 0; i < list.length && out.length < 50; i++) {
            if (list[i].name.toLowerCase().indexOf(ql) !== -1) out.push(list[i]);
        }
        return out;
    }

    function ensureBox() {
        if (box) return box;
        box = document.createElement('div');
        box.className = 'hidden z-10 bg-surface border-2 border-primary/30 rounded-xl shadow-lg max-h-60 overflow-y-auto text-sm';
        // inline position: fixed — the `fixed` utility is not in the generated
        // CSS (js files are outside uno's scan globs), and without it the box
        // would fall back to static and render at the page bottom
        box.style.position = 'fixed';
        box.setAttribute('role', 'listbox');
        document.body.appendChild(box);
        return box;
    }

    function positionBox(ta) {
        var r = ta.getBoundingClientRect();
        // measure the real height without a flash: hidden is display:none, an
        // inline display override makes it measurable (offsetHeight forces a
        // synchronous layout) before it is positioned and revealed
        box.style.display = 'block';
        var h = box.offsetHeight;
        box.style.display = '';
        // flip above the field when it would extend below the viewport fold
        // (and there is room above); otherwise open below it
        var gap = 4;
        if (window.innerHeight - r.bottom < h + gap * 2 && r.top > h + gap * 2) {
            box.style.top = '';
            box.style.bottom = (window.innerHeight - r.top + gap) + 'px';
        } else {
            box.style.top = (r.bottom + gap) + 'px';
            box.style.bottom = '';
        }
        box.style.left = r.left + 'px';
        box.style.width = r.width + 'px';
    }

    function closeBox() {
        if (box) box.classList.add('hidden');
        current = null;
    }

    function insert(ta, cur, opt) {
        var v = ta.value;
        var pos = ta.selectionStart;
        var before = v.slice(0, pos);
        var m = /\[\[([a-z]+:)?[^\[\]]*$/i.exec(before);
        if (!m) { closeBox(); return; }
        var start = before.length - m[0].length;
        var prefix = cur.kind !== 'cookie' ? cur.kind + ':' : '';
        var fill = '[[' + prefix + opt.id + ']]';
        ta.value = v.slice(0, start) + fill + v.slice(pos);
        var np = start + fill.length;
        ta.selectionStart = ta.selectionEnd = np;
        closeBox();
        ta.focus();
        updatePreview(ta);
    }

    function highlight(i) {
        if (!current) return;
        current.active = i;
        var kids = box.children;
        for (var k = 0; k < kids.length; k++) {
            kids[k].classList.toggle('bg-primary/20', k === i);
        }
        var el = kids[i];
        if (el && el.scrollIntoView) el.scrollIntoView({ block: 'nearest' });
    }

    function renderList(ta, cur) {
        var items = matches(cur.kind, cur.q);
        if (items.length === 0) { closeBox(); return; }
        ensureBox();
        box.textContent = '';
        current = { textarea: ta, kind: cur.kind, q: cur.q, items: items, active: 0 };
        items.forEach(function(opt, i) {
            var li = document.createElement('div');
            li.className = 'px-4 py-2 cursor-pointer hover:bg-primary/10 text-foreground flex items-center gap-2';
            li.setAttribute('role', 'option');
            // the entity sprite ahead of the name, mirroring the preview and
            // the server's rendered links; alt is empty because the name
            // beside it already names the entity
            var thumb = thumbHtml(cur.kind, opt);
            if (thumb) {
                var span = document.createElement('span');
                span.innerHTML = thumb;
                li.appendChild(span.firstChild);
            }
            li.appendChild(document.createTextNode(opt.name));
            li.addEventListener('mousedown', function(e) {
                e.preventDefault(); // keep the textarea focused
                insert(ta, cur, opt);
            });
            // fallback for synthetic clicks (no mousedown): skip when a real
            // mousedown already inserted (the [[ is then closed, no open query)
            li.addEventListener('click', function() {
                var q2 = openQuery(ta);
                if (q2 === null) return;
                insert(ta, q2, opt);
            });
            box.appendChild(li);
        });
        highlight(0);
        positionBox(ta);
        box.classList.remove('hidden');
    }

    function refresh(ta) {
        var cur = openQuery(ta);
        if (cur === null) { closeBox(); return; }
        renderList(ta, cur);
    }

    function onKeydown(ta, e) {
        if (!current) return;
        var len = current.items.length;
        if (e.key === 'ArrowDown') {
            e.preventDefault();
            highlight((current.active + 1) % len);
        } else if (e.key === 'ArrowUp') {
            e.preventDefault();
            highlight((current.active - 1 + len) % len);
        } else if (e.key === 'Enter' || e.key === 'Tab') {
            e.preventDefault();
            insert(ta, current, current.items[current.active]);
        } else if (e.key === 'Escape') {
            closeBox();
        }
    }

    // ---- live preview -----------------------------------------------------

    // mirrors render_inline: escape first, then the scanner — entity links,
    // ![alt](https://...) images, **strong** / *em*, {color:...} — over the
    // already-escaped text, with the same guard shapes as the server.
    function renderInline(s) {
        var out = '';
        var i = 0;
        while (i < s.length) {
            // entity link: [[kind:id]]
            if (s[i] === '[' && s[i + 1] === '[') {
                var end = s.indexOf(']]', i + 2);
                if (end !== -1) {
                    var parsed = parseLink(s.slice(i + 2, end));
                    if (parsed) {
                        var found = findById(parsed.kind, parsed.id);
                        if (found) {
                            out += '<a href="/' + kindPath(parsed.kind) + '/' + found.id +
                                '" class="font-bold hover:text-primary transition-colors">' +
                                thumbHtml(parsed.kind, found) +
                                '<span class="underline decoration-primary decoration-2 underline-offset-4">' +
                                escapeHtml(found.name) + '</span></a>';
                            i = end + 2;
                            continue;
                        }
                    }
                }
            }
            // image: ![alt](https://...) — http(s) sources only, both parts
            // required (mirrors image_at; escaping already neutralised any
            // markup characters inside the parts)
            if (s[i] === '!' && s[i + 1] === '[') {
                var altEnd = s.indexOf(']', i + 2);
                if (altEnd !== -1 && s[altEnd + 1] === '(') {
                    var imgClose = s.indexOf(')', altEnd + 2);
                    if (imgClose !== -1) {
                        var alt = s.slice(i + 2, altEnd);
                        var src = s.slice(altEnd + 2, imgClose);
                        var low = src.toLowerCase();
                        if (alt !== '' && src !== '' &&
                            (low.indexOf('http://') === 0 || low.indexOf('https://') === 0) &&
                            src.length <= 500 && alt.length <= 300) {
                            out += '<img src="' + src + '" alt="' + alt +
                                '" loading="lazy" class="mt-4 rounded-2xl border border-border" />';
                            i = imgClose + 1;
                            continue;
                        }
                    }
                }
            }
            // **strong**: scanned before *em* so the longer marker wins
            if (s[i] === '*' && s[i + 1] === '*') {
                var endS = s.indexOf('**', i + 2);
                if (endS !== -1 && endS > i + 2) {
                    out += '<strong>' + renderInline(s.slice(i + 2, endS)) + '</strong>';
                    i = endS + 2;
                    continue;
                }
            } else if (s[i] === '*') {
                // *em*: the closing marker must not be another '*'
                var endE = s.indexOf('*', i + 1);
                if (endE !== -1 && endE > i + 1 && s[i + 1] !== '*' && s[endE + 1] !== '*') {
                    out += '<em>' + renderInline(s.slice(i + 1, endE)) + '</em>';
                    i = endE + 1;
                    continue;
                }
            }
            // plain link: [text](https://...) — the image form without the
            // leading '!'; the label renders recursively, so entity links,
            // bold and colour nest in it (mirrors link_at + its scanner
            // branch: http(s) targets only, both parts required)
            if (s[i] === '[' && s[i + 1] !== '[') {
                var labelEnd = s.indexOf(']', i + 1);
                if (labelEnd !== -1 && s[labelEnd + 1] === '(') {
                    var hrefClose = s.indexOf(')', labelEnd + 2);
                    if (hrefClose !== -1) {
                        var label = s.slice(i + 1, labelEnd);
                        var href = s.slice(labelEnd + 2, hrefClose);
                        var lowHref = href.toLowerCase();
                        // "https://" alone has no host: the scheme length is
                        // the minimum, mirroring link_at
                        if (label !== '' && href !== '' &&
                            (lowHref.indexOf('http://') === 0 || lowHref.indexOf('https://') === 0) &&
                            href.length <= 200 && href.length > (lowHref.indexOf('https://') === 0 ? 8 : 7)) {
                            out += '<a href="' + href +
                                '" class="text-primary underline decoration-primary decoration-2 underline-offset-4 hover:text-accent transition-colors" target="_blank" rel="noopener noreferrer">' +
                                renderInline(label) + '</a>';
                            i = hrefClose + 1;
                            continue;
                        }
                    }
                }
            }
            // color span: {color:VAL}...{/color}
            if (s[i] === '{' && s.slice(i, i + 7) === '{color:') {
                var close = s.indexOf('}', i + 7);
                if (close !== -1) {
                    var val = s.slice(i + 7, close);
                    if (validColor(val)) {
                        var endC = s.indexOf('{/color}', close + 1);
                        if (endC !== -1) {
                            out += '<span style="color:' + val + '">' + s.slice(close + 1, endC) + '</span>';
                            i = endC + '{/color}'.length;
                            continue;
                        }
                    }
                }
            }
            out += s[i];
            i++;
        }
        return out;
    }

    // mirrors render_article_with: blank-line-separated blocks, each a
    // heading, a bullet/numbered list or a paragraph. The class strings are
    // deliberately identical to src/richtext.rs's, so unocss (which scans
    // both the templates and the Rust sources) generates them.
    function isBullet(line) { return bulletOf(line) !== null; }
    function isNumber(line) { return numberOf(line) !== null; }

    function renderBlocks(s) {
        var lines = s.replace(/\r\n/g, '\n').split('\n');
        var chunks = [];
        var chunk = [];
        function flush() { if (chunk.length) { chunks.push(chunk); chunk = []; } }
        lines.forEach(function(l) { if (l.trim() === '') flush(); else chunk.push(l.replace(/\s+$/, '')); });
        flush();
        var html = '';
        chunks.forEach(function(blk) {
            var head = headOf(blk);
            if (head) {
                var level = Math.min(head.hashes + 1, 4);
                var cls = level <= 2
                    ? 'mt-8 mb-3 font-headline text-2xl text-accent'
                    : 'mt-6 mb-2 font-headline text-xl text-accent';
                html += '<h' + level + ' class="' + cls + '">' + renderInline(head.text) + '</h' + level + '>';
                return;
            }
            if (blk.every(isBullet)) {
                html += list('ul', 'mt-4 list-disc list-inside space-y-1 text-foreground',
                    blk.map(bulletOf));
            } else if (blk.every(isNumber)) {
                html += list('ol', 'mt-4 list-decimal list-inside space-y-1 text-foreground',
                    blk.map(numberOf));
            } else {
                // single newlines stay in the block; the paragraph turns them
                // into <br /> — a tight wrap, not a paragraph gap (the
                // server's post-2026-11 newline behavior)
                html += '<p class="mt-4 leading-relaxed text-foreground">' +
                    renderInline(blk.join('\n')).replace(/\n/g, '<br />') + '</p>';
            }
        });
        return html;
    }

    // mirrors heading_of: 1-3 hashes, a space after them, non-empty trimmed
    // text; only considered when the block is a single line
    function headOf(blk) {
        if (blk.length !== 1) return null;
        var m = /^(#{1,3}) (.*)$/.exec(blk[0]);
        if (!m) return null;
        var text = m[2].trim();
        return text === '' ? null : { hashes: m[1].length, text: text };
    }

    function isBullet(line) { return bulletOf(line) !== null; }
    function isNumber(line) { return numberOf(line) !== null; }

    // mirrors bullet_of: the "- "/"* " prefix after any indent, trimmed rest
    function bulletOf(line) {
        var t = line.replace(/^\s+/, '');
        var m = /^[-*] (.*)$/.exec(t);
        return m ? m[1].trim() : null;
    }

    // mirrors number_of: digits, then ". ", trimmed rest
    function numberOf(line) {
        var t = line.replace(/^\s+/, '');
        var m = /^(\d+)\. (.*)$/.exec(t);
        return m ? m[2].trim() : null;
    }

    function list(tag, cls, items) {
        var html = '<' + tag + ' class="' + cls + '">';
        items.forEach(function(it) { html += '<li>' + renderInline(it) + '</li>'; });
        return html + '</' + tag + '>';
    }

    // the article textareas (news) opt into block rendering; every other
    // rich-text field is a single paragraph, exactly as before
    function renderPreview(ta) {
        var s = escapeHtml(ta.value);
        return ta.getAttribute('data-richtext') === 'article' ? renderBlocks(s) : renderInline(s);
    }

    function updatePreview(ta) {
        var pre = ta.parentElement && ta.parentElement.querySelector('[data-richtext-preview]');
        if (!pre) return;
        var body = pre.querySelector('.richtext-preview-body');
        if (!body) return;
        var html = renderPreview(ta);
        body.innerHTML = html;
        // the <details> is collapsible: hidden only tracks whether there is
        // anything to preview, open/closed stays the user's choice
        pre.classList.toggle('hidden', html === '');
    }

    function updateAllPreviews() {
        var tas = document.querySelectorAll('textarea[data-richtext]');
        for (var i = 0; i < tas.length; i++) updatePreview(tas[i]);
    }

    // ---- init -------------------------------------------------------------

    function init() {
        // re-render the previews once the name lists arrive: they are empty
        // during the first paint, so links in pre-filled fields would stay
        // literal until the user typed
        loadNames(function() { updateAllPreviews(); });
        var tas = document.querySelectorAll('textarea[data-richtext]');
        for (var i = 0; i < tas.length; i++) (function(ta) {
            ta.addEventListener('input', function() { refresh(ta); updatePreview(ta); });
            ta.addEventListener('keydown', function(e) { onKeydown(ta, e); });
            ta.addEventListener('click', function() { refresh(ta); });
            ta.addEventListener('blur', function() { closeBox(); });
        })(tas[i]);
        updateAllPreviews();

        // the form's language select navigates with ?lang= on change, so a
        // full page load refetches the lists for the new locale; nothing to
        // listen for here

        // the field's page-scroll position can change under an open dropdown;
        // close it then (textarea-internal scroll does not bubble to window)
        window.addEventListener('scroll', closeBox);

        document.addEventListener('mousedown', function(e) {
            if (current && !box.contains(e.target) && e.target !== current.textarea) closeBox();
        });
    }

    if (document.readyState === 'loading') {
        document.addEventListener('DOMContentLoaded', init);
    } else {
        init();
    }
})();
