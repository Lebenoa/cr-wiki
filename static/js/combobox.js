// combobox.js — searchable combobox for admin forms.
//
// Markup (server-rendered inside a `data-combobox` root):
//
//   <div class="relative" data-combobox data-cb-mode="treasure|name">
//     <input type="text" data-cb-input autocomplete="off" ...>
//     <input type="hidden" data-cb-hidden name="..." value="...">
//     <ul class="hidden ..." data-cb-list>
//       <li data-cb-value="12">Pilgrim's Scroll</li>
//       ...
//       <li data-cb-value="__new__">+ New Treasure</li>
//     </ul>
//   </div>
//
// Semantics:
//   - typing filters the list; an exact match picks that option (hidden
//     value = its data-cb-value), a non-match is treated as "new"
//     (treasure mode: hidden = '__new__' with the visible input submitted
//     as the new name; name mode: hidden = the typed text itself).
//   - empty text clears the selection (hidden value = '').
//   - the '__new__' footer item confirms the typed text as a new value.
//   - ArrowDown/ArrowUp navigate, Enter selects, Escape closes.
//
// Exposed as window.Combobox.init(root) so dynamically added rows can be
// wired after cloning.
(function () {
    'use strict';

    function label(li) {
        return (li.textContent || '').trim();
    }

    function exactValue(list, text) {
        var t = text.toLowerCase();
        var items = list.querySelectorAll('li[data-cb-value]');
        for (var i = 0; i < items.length; i++) {
            var li = items[i];
            if (li.getAttribute('data-cb-value') === '__new__') continue;
            if (label(li).toLowerCase() === t) return li.getAttribute('data-cb-value');
        }
        return null;
    }

    function filterList(list, text) {
        var q = text.toLowerCase();
        var items = list.querySelectorAll('li[data-cb-value]');
        var anyVisible = false;
        for (var i = 0; i < items.length; i++) {
            var li = items[i];
            if (li.getAttribute('data-cb-value') === '__new__') {
                // footer: always visible, styled as a distinct action
                li.classList.remove('hidden');
                continue;
            }
            var show = label(li).toLowerCase().indexOf(q) !== -1;
            li.classList.toggle('hidden', !show);
            if (show) anyVisible = true;
        }
        return anyVisible;
    }

    function init(root) {
        if (root.__cbInit) return;
        root.__cbInit = true;
        var input = root.querySelector('[data-cb-input]');
        var hidden = root.querySelector('[data-cb-hidden]');
        var list = root.querySelector('[data-cb-list]');
        if (!input || !hidden || !list) return;

        var mode = root.getAttribute('data-cb-mode') || 'name';
        var items = list.querySelectorAll('li[data-cb-value]');
        var active = -1;

        function close() {
            list.classList.add('hidden');
            active = -1;
            items.forEach(function (li) { li.classList.remove('bg-primary/20'); });
        }

        function open() {
            filterList(list, input.value);
            list.classList.remove('hidden');
        }

        function pick(li) {
            var val = li.getAttribute('data-cb-value');
            if (val === '__new__') {
                // confirm the typed text as a new value
                hidden.value = mode === 'treasure' ? '__new__' : input.value.trim();
                close();
                input.focus();
                return;
            }
            input.value = label(li);
            hidden.value = val;
            close();
        }

        function highlight(i) {
            items.forEach(function (li, idx) { li.classList.toggle('bg-primary/20', idx === i); });
            active = i;
            var li = items[i];
            if (li && li.scrollIntoView) li.scrollIntoView({ block: 'nearest' });
        }

        input.addEventListener('focus', open);
        input.addEventListener('click', open);
        input.addEventListener('input', function () {
            var text = input.value.trim();
            if (text === '') {
                hidden.value = '';
            } else {
                var match = exactValue(list, text);
                hidden.value = match !== null ? match
                    : (mode === 'treasure' ? '__new__' : text);
            }
            open();
        });
        input.addEventListener('keydown', function (e) {
            var visible = Array.prototype.filter.call(items, function (li) {
                return !li.classList.contains('hidden');
            });
            if (e.key === 'ArrowDown') {
                e.preventDefault();
                if (list.classList.contains('hidden')) { open(); return; }
                highlight(active < visible.length - 1 ? active + 1 : 0);
            } else if (e.key === 'ArrowUp') {
                e.preventDefault();
                if (list.classList.contains('hidden')) { open(); return; }
                highlight(active > 0 ? active - 1 : visible.length - 1);
            } else if (e.key === 'Enter') {
                if (!list.classList.contains('hidden')) {
                    e.preventDefault();
                    if (active >= 0 && visible[active]) pick(visible[active]);
                    else close();
                }
            } else if (e.key === 'Escape') {
                close();
            }
        });

        list.addEventListener('mousedown', function (e) {
            var li = e.target.closest('li[data-cb-value]');
            if (li) {
                e.preventDefault(); // keep input focus
                pick(li);
            }
        });

        document.addEventListener('click', function (e) {
            if (!root.contains(e.target)) close();
        });

        // initial state: reflect the server-rendered hidden value
        if (hidden.value !== '' && hidden.value !== '__new__') {
            if (mode === 'treasure') {
                items.forEach(function (li) {
                    if (li.getAttribute('data-cb-value') === hidden.value) input.value = label(li);
                });
            } else {
                input.value = hidden.value;
            }
        }
    }

    document.addEventListener('DOMContentLoaded', function () {
        document.querySelectorAll('[data-combobox]').forEach(init);
    });

    window.Combobox = { init: init };
})();
