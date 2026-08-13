// richtext.js — rich-text authoring helpers for admin fields.
//
// Any <textarea data-richtext> gets two features:
//   1. an entity-name autocomplete triggered by "[[" — "[[ging" lists cookies,
//      "[[pet:pep" pets, "[[treasure:..." treasures; Enter, Tab or click
//      completes the [[kind:Name]] markup, and
//   2. a live preview (the [data-richtext-preview] sibling box) that renders
//      the markup the same way render_rich_text does: [[...]] become links to
//      the entity pages, {color:...} becomes colored text, everything else is
//      escaped.
//
// Both use /api/richtext-names?lang=<page lang>&kind=... which returns
// [{id, name}] for the language render_rich_text resolves (that language plus
// the en fallback); the ids let the preview build real hrefs. The lists are
// re-fetched when the form's language <select id="lang"> changes.
//
// The dropdown is position:fixed and placed from getBoundingClientRect()
// (viewport coordinates); an absolutely-positioned body child would be offset
// by the page scroll and appear above the field on scrolled pages.
(function () {
    'use strict';

    var KINDS = ['cookie', 'pet', 'treasure'];
    var NAMES = { cookie: [], pet: [], treasure: [] }; // [{ id, name }]
    var fetchSeq = 0;   // guards out-of-order /api responses on lang switches
    var box = null;     // single dropdown reused across fields
    var current = null; // { textarea, kind, q, items, active }

    // ---- name lists -------------------------------------------------------

    function langOf() {
        var el = document.getElementById('lang');
        if (el && el.value) return el.value;
        return document.documentElement.lang || 'en';
    }

    function loadNames(cb) {
        var mySeq = ++fetchSeq;
        var remaining = KINDS.length;
        KINDS.forEach(function (kind) {
            fetch('/api/richtext-names?lang=' + encodeURIComponent(langOf()) + '&kind=' + kind)
                .then(function (r) { return r.json(); })
                .then(function (a) {
                    if (mySeq !== fetchSeq) return; // a newer lang request superseded this one
                    NAMES[kind] = Array.isArray(a) ? a : [];
                    if (--remaining === 0 && cb) cb();
                })
                .catch(function () {
                    if (mySeq !== fetchSeq) return;
                    NAMES[kind] = [];
                    if (--remaining === 0 && cb) cb();
                });
        });
    }

    // ---- markup helpers shared by autocomplete + preview -------------------

    // [[inner]] -> { kind, name }; "pet:Apple Rabbit" -> pet + Apple Rabbit,
    // anything without a known kind prefix stays a cookie link
    function parseLink(inner) {
        var t = inner.trim();
        var m = /^([a-z]+):(.*)$/i.exec(t);
        if (m && KINDS.indexOf(m[1].toLowerCase()) !== -1 && m[2].trim() !== '') {
            return { kind: m[1].toLowerCase(), name: m[2].trim() };
        }
        return { kind: 'cookie', name: t };
    }

    function findEntity(kind, name) {
        var list = NAMES[kind] || [];
        for (var i = 0; i < list.length; i++) {
            if (list[i].name === name) return list[i];
        }
        return null;
    }

    function kindPath(kind) {
        return kind === 'pet' ? 'pets' : kind === 'treasure' ? 'treasures' : 'cookies';
    }

    function escapeHtml(s) {
        return s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;')
            .replace(/"/g, '&quot;').replace(/'/g, '&#39;');
    }

    function unescapeHtml(s) {
        return s.replace(/&#39;/g, "'").replace(/&quot;/g, '"').replace(/&lt;/g, '<')
            .replace(/&gt;/g, '>').replace(/&amp;/g, '&');
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
        var fill = '[[' + prefix + opt.name + ']]';
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
        items.forEach(function (opt, i) {
            var li = document.createElement('div');
            li.className = 'px-4 py-2 cursor-pointer hover:bg-primary/10 text-foreground flex items-center gap-2';
            li.setAttribute('role', 'option');
            li.textContent = opt.name;
            li.addEventListener('mousedown', function (e) {
                e.preventDefault(); // keep the textarea focused
                insert(ta, cur, opt);
            });
            // fallback for synthetic clicks (no mousedown): skip when a real
            // mousedown already inserted (the [[ is then closed, no open query)
            li.addEventListener('click', function () {
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

    // mirrors render_rich_text: escape first, then [[links]] and {color:...}
    function renderPreview(ta) {
        var s = escapeHtml(ta.value);
        var out = '';
        var i = 0;
        while (i < s.length) {
            // link: [[Name]] or [[kind:Name]]
            if (s[i] === '[' && s[i + 1] === '[') {
                var end = s.indexOf(']]', i + 2);
                if (end !== -1) {
                    var parsed = parseLink(s.slice(i + 2, end));
                    var found = findEntity(parsed.kind, unescapeHtml(parsed.name));
                    if (found) {
                        out += '<a href="/' + kindPath(parsed.kind) + '/' + found.id +
                            '" class="font-bold underline decoration-primary decoration-2 underline-offset-4 hover:text-primary transition-colors">' +
                            parsed.name + '</a>';
                        i = end + 2;
                        continue;
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

    function updatePreview(ta) {
        var pre = ta.parentElement && ta.parentElement.querySelector('[data-richtext-preview]');
        if (!pre) return;
        var body = pre.querySelector('.richtext-preview-body');
        if (!body) return;
        var html = renderPreview(ta);
        body.innerHTML = html;
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
        loadNames(function () { updateAllPreviews(); });
        var tas = document.querySelectorAll('textarea[data-richtext]');
        for (var i = 0; i < tas.length; i++) (function (ta) {
            ta.addEventListener('input', function () { refresh(ta); updatePreview(ta); });
            ta.addEventListener('keydown', function (e) { onKeydown(ta, e); });
            ta.addEventListener('click', function () { refresh(ta); });
            ta.addEventListener('blur', function () { closeBox(); });
        })(tas[i]);
        updateAllPreviews();

        var langSel = document.getElementById('lang');
        if (langSel) langSel.addEventListener('change', function () {
            closeBox();
            loadNames(function () { updateAllPreviews(); });
        });

        // the field's page-scroll position can change under an open dropdown;
        // close it then (textarea-internal scroll does not bubble to window)
        window.addEventListener('scroll', closeBox);

        document.addEventListener('mousedown', function (e) {
            if (current && !box.contains(e.target) && e.target !== current.textarea) closeBox();
        });
    }

    if (document.readyState === 'loading') {
        document.addEventListener('DOMContentLoaded', init);
    } else {
        init();
    }
})();
