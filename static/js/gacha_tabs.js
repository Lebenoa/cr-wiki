// gacha_tabs.js — the /gacha pools are tab cards over a shared panel area.
// Nothing is open on load: a pool's prize grid renders only once its card is
// clicked, and clicking the open card closes it again, so the page starts as a
// short list of pools instead of every pool's full grid at once.
//
// Panels are server-rendered and toggled with the `hidden` attribute (not a
// class), so a page without this script still exposes them to find-in-page
// and to anything that unhides them.
//
// The open pool is mirrored into the URL as ?pool=<pool_id> so a link or a
// reload lands on the same tab. The server never reads it — /gacha renders
// every pool regardless — so an unknown or junk value simply opens nothing.
(function () {
    'use strict';

    var tabs = document.querySelectorAll('[data-gacha-tab]');
    if (tabs.length === 0) {
        return;
    }

    var POOL_PARAM = 'pool';

    function panelOf(tab) {
        return document.getElementById(tab.getAttribute('aria-controls'));
    }

    // replaceState, not pushState: the tab is view state, and a click per
    // history entry would make Back walk the tab strip instead of leaving the
    // page. The rest of the query string is preserved.
    function writeUrl(poolId) {
        if (!window.history || !window.history.replaceState) {
            return;
        }
        var url = new URL(window.location.href);
        if (poolId) {
            url.searchParams.set(POOL_PARAM, poolId);
        } else {
            url.searchParams.delete(POOL_PARAM);
        }
        window.history.replaceState(null, '', url.pathname + url.search + url.hash);
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

    function open(tab) {
        // one pool at a time: close every tab, then open the wanted one
        tabs.forEach(function (t) {
            setOpen(t, false);
        });
        if (tab) {
            setOpen(tab, true);
            // the filter box hides cards inside the panel; re-apply it so a
            // freshly opened pool honours a query typed while it was closed
            if (window.crFilter) {
                window.crFilter();
            }
        }
        writeUrl(tab ? tab.getAttribute('data-gacha-tab') : '');
    }

    tabs.forEach(function (tab) {
        setOpen(tab, false);
        tab.addEventListener('click', function () {
            var wasOpen = tab.getAttribute('aria-selected') === 'true';
            // clicking the open card collapses it
            open(wasOpen ? null : tab);
        });
    });

    // restore the pool named in the query, matched against the rendered tabs
    // so an unknown id opens nothing rather than erroring
    var wanted = new URLSearchParams(window.location.search).get(POOL_PARAM);
    if (wanted) {
        for (var i = 0; i < tabs.length; i++) {
            if (tabs[i].getAttribute('data-gacha-tab') === wanted) {
                open(tabs[i]);
                break;
            }
        }
    }

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
