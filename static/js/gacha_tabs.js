// gacha_tabs.js — the /gacha pools are tab cards over a shared panel area.
// Nothing is open on load: a pool's prize grid renders only once its card is
// clicked, and clicking the open card closes it again, so the page starts as a
// short list of pools instead of every pool's full grid at once.
//
// Panels are server-rendered and toggled with the `hidden` attribute (not a
// class), so a page without this script still exposes them to find-in-page
// and to anything that unhides them.
(function () {
    'use strict';

    var tabs = document.querySelectorAll('[data-gacha-tab]');
    if (tabs.length === 0) {
        return;
    }

    function panelOf(tab) {
        return document.getElementById(tab.getAttribute('aria-controls'));
    }

    // the open card's highlight is a preflight rule keyed off aria-selected
    // (see uno.config.ts) — classes referenced only from JS are never
    // generated, since UnoCSS scans templates and app/*.v only
    function setOpen(tab, open) {
        var panel = panelOf(tab);
        tab.setAttribute('aria-selected', open ? 'true' : 'false');
        if (panel) {
            panel.hidden = !open;
        }
    }

    tabs.forEach(function (tab) {
        setOpen(tab, false);
        tab.addEventListener('click', function () {
            var wasOpen = tab.getAttribute('aria-selected') === 'true';
            // one pool at a time: close every tab, then open the clicked one
            // unless it was the open one (click again to collapse)
            tabs.forEach(function (t) {
                setOpen(t, false);
            });
            if (!wasOpen) {
                setOpen(tab, true);
                // the filter box hides cards inside the panel; re-apply it so a
                // freshly opened pool honours a query typed while it was closed
                if (window.crFilter) {
                    window.crFilter();
                }
            }
        });
    });

    // arrow keys walk the tab strip, matching the tablist pattern
    document.addEventListener('keydown', function (ev) {
        if (ev.key !== 'ArrowLeft' && ev.key !== 'ArrowRight') {
            return;
        }
        var current = ev.target && ev.target.closest ? ev.target.closest('[data-gacha-tab]') : null;
        if (!current) {
            return;
        }
        var list = Array.prototype.slice.call(tabs);
        var i = list.indexOf(current);
        if (i === -1) {
            return;
        }
        ev.preventDefault();
        var next = list[(i + (ev.key === 'ArrowRight' ? 1 : list.length - 1)) % list.length];
        next.focus();
    });
})();
