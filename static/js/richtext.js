// richtext.js — cookie-name autocomplete for rich text admin fields.
//
// Any <textarea data-richtext> gets a dropdown triggered by typing "[[" (e.g.
// "[[ging" lists matching cookie names). Selecting a name completes the markup
// to "[[Name]]", matching what render_rich_text turns into a link.
//
// The name list is fetched from /api/richtext-names?lang=<page lang> — the same
// resolution set render_rich_text uses (that language plus en fallback) — and
// re-fetched when the form's language <select id="lang"> changes.
//
// The dropdown is position:fixed and placed from getBoundingClientRect()
// (viewport coordinates); an absolutely-positioned body child would be offset
// by the page scroll and appear above the field on scrolled pages.
(function () {
    'use strict';

    var NAMES = [];
    var fetchSeq = 0;   // guards out-of-order /api responses on lang switches
    var box = null;     // single dropdown reused across fields
    var current = null; // { textarea, q, items, active }

    function langOf() {
        var el = document.getElementById('lang');
        if (el && el.value) return el.value;
        return document.documentElement.lang || 'en';
    }

    function loadNames(cb) {
        var mySeq = ++fetchSeq;
        fetch('/api/richtext-names?lang=' + encodeURIComponent(langOf()))
            .then(function (r) { return r.json(); })
            .then(function (a) {
                if (mySeq !== fetchSeq) return; // a newer lang request superseded this one
                NAMES = Array.isArray(a) ? a : [];
                if (cb) cb();
            })
            .catch(function () {
                if (mySeq !== fetchSeq) return;
                NAMES = [];
                if (cb) cb();
            });
    }

    // text between the last open "[[" before the caret and the caret, or null
    function openQuery(ta) {
        var before = ta.value.slice(0, ta.selectionStart);
        var m = /\[\[([^\[\]]*)$/.exec(before);
        return m ? m[1] : null;
    }

    function matches(q) {
        var ql = q.toLowerCase();
        var out = [];
        for (var i = 0; i < NAMES.length && out.length < 50; i++) {
            if (NAMES[i].toLowerCase().indexOf(ql) !== -1) out.push(NAMES[i]);
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
        box.style.top = (r.bottom + 4) + 'px';
        box.style.left = r.left + 'px';
        box.style.width = r.width + 'px';
    }

    function closeBox() {
        if (box) box.classList.add('hidden');
        current = null;
    }

    function insert(ta, q, name) {
        var v = ta.value;
        var pos = ta.selectionStart;
        var before = v.slice(0, pos);
        var m = /\[\[([^\[\]]*)$/.exec(before);
        if (!m) { closeBox(); return; }
        var start = before.length - m[0].length;
        ta.value = v.slice(0, start) + '[[' + name + ']]' + v.slice(pos);
        var np = start + 2 + name.length + 2;
        ta.selectionStart = ta.selectionEnd = np;
        closeBox();
        ta.focus();
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

    function renderList(ta, q) {
        var items = matches(q);
        if (items.length === 0) { closeBox(); return; }
        ensureBox();
        box.textContent = '';
        current = { textarea: ta, q: q, items: items, active: 0 };
        items.forEach(function (name, i) {
            var li = document.createElement('div');
            li.className = 'px-4 py-2 cursor-pointer hover:bg-primary/10 text-foreground flex items-center gap-2';
            li.setAttribute('role', 'option');
            li.textContent = name;
            li.addEventListener('mousedown', function (e) {
                e.preventDefault(); // keep the textarea focused
                insert(ta, q, name);
            });
            // fallback for synthetic clicks (no mousedown): skip when a real
            // mousedown already inserted (the [[ is then closed, no open query)
            li.addEventListener('click', function () {
                var q2 = openQuery(ta);
                if (q2 === null) return;
                insert(ta, q2, name);
            });
            box.appendChild(li);
        });
        highlight(0);
        positionBox(ta);
        box.classList.remove('hidden');
    }

    function refresh(ta) {
        var q = openQuery(ta);
        if (q === null) { closeBox(); return; }
        renderList(ta, q);
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
            insert(ta, current.q, current.items[current.active]);
        } else if (e.key === 'Escape') {
            closeBox();
        }
    }

    function init() {
        loadNames(null);
        var tas = document.querySelectorAll('textarea[data-richtext]');
        for (var i = 0; i < tas.length; i++) (function (ta) {
            ta.addEventListener('input', function () { refresh(ta); });
            ta.addEventListener('keydown', function (e) { onKeydown(ta, e); });
            ta.addEventListener('click', function () { refresh(ta); });
            ta.addEventListener('blur', function () { closeBox(); });
        })(tas[i]);

        var langSel = document.getElementById('lang');
        if (langSel) langSel.addEventListener('change', function () {
            closeBox();
            loadNames(null);
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
