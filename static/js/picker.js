// build planner picker: modal dialogs for cookie (shared by the lead and the
// optional relay slot), pet, and treasure (shared by t1/t2/t3). Picks are
// captured via form method="dialog"; values land in the build form's hidden
// inputs and the slot buttons re-render from the clicked option. Filled slots
// get a corner ✕ that clears the pick back to its placeholder. The loadout
// preview below the slots re-renders live after each change by fetching
// /builds/preview with the current selection.
(function () {
    'use strict';

    var treasureSlot = 't1';
    var cookieSlot = 'cookie';
    var lastPick = null;
    // last level set on the picker's top slider, remembered across dialog
    // opens (null = the user hasn't moved it yet, so slot/9 fallback applies).
    var lastPickerLevel = null;

    // filter mode: /builds reuses the same picker dialogs for its cookie/pet/
    // treasure filters. A pick just re-filters the list via htmx — no planner
    // flow (no auto-advance), no live preview.
    var filterForm = document.querySelector('form[hx-get="/builds"]');
    var filterMode = !!filterForm;
    // treasureTab: the /builds treasure filter dialog's all/normal/evolved
    // state tabs (treasure list page style). Planner treasure picks ignore it.
    var treasureTab = 'all';

    // stepOrder is the picker flow; each dialog shows a step header ("2/6")
    // with the current slot's translated label and an animated progress fill.
    var stepDialog = { cookie: 'cookie', cookie2: 'cookie', pet: 'pet', t1: 'treasure', t2: 'treasure', t3: 'treasure' };
    // stepOrder is the picker flow, filtered to the dialogs actually present
    // on this page: the planner has all of them (1/6..6/6), the build edit
    // page only the treasure dialog (t1..t3 read 1/3..3/3), and the /builds
    // filter page reuses all six slots across its three dialogs.
    var presentDialogs = {};
    document.querySelectorAll('dialog').forEach(function (d) {
        presentDialogs[d.dataset.kind] = true;
    });
    var stepOrder = ['cookie', 'cookie2', 'pet', 't1', 't2', 't3'].filter(function (slot) {
        return presentDialogs[stepDialog[slot]];
    });

    // updateDialogStep fills the header inside the dialog that is about to
    // open: count ("n/6"), translated label (from the dialog's data-label-*
    // attributes), the modal title (data-title-*) and the fill width, which
    // animates via transition-all.
    function updateDialogStep(slot) {
        var header = document.getElementById('dialog-step-' + stepDialog[slot]);
        if (!header) {
            return;
        }
        var step = stepOrder.indexOf(slot) + 1;
        header.querySelector('.step-count').textContent = step + '/' + stepOrder.length;
        header.querySelector('.step-name').textContent = header.getAttribute('data-label-' + slot) || '';
        var title = header.getAttribute('data-title-' + slot);
        if (title) {
            var dlg = header.closest('dialog');
            var h2 = dlg ? dlg.querySelector('h2') : null;
            if (h2) {
                h2.textContent = title;
            }
        }
        var fill = header.querySelector('.step-fill');
        fill.style.width = '0';
        // let the width transition play from 0 on each open
        requestAnimationFrame(function () {
            fill.style.width = (step / 6 * 100) + '%';
        });
    }

    // applyExclusion hides the already-picked lead cookie while the relay
    // slot is being picked (its id sits in the sel-cookie hidden input; option
    // ids are the submit buttons' values), so the same cookie cannot be chosen
    // twice. It is the only filtering left in the browser — name search and
    // the treasure tabs are query params now, because the grid is paginated
    // and the page holds only the pages it has scrolled through.
    function applyExclusion(dlg) {
        if (!dlg || dlg.dataset.kind !== 'cookie') {
            return;
        }
        var lead = cookieSlot === 'cookie2' ? document.getElementById('sel-cookie') : null;
        var excludeId = lead ? lead.value : '';
        dlg.querySelectorAll('.pick-option').forEach(function (btn) {
            btn.hidden = excludeId !== '' && btn.value === excludeId;
        });
    }

    // refreshGrid re-runs the picker query from page 1: the controls form
    // carries the search term and the tab, so triggering it is all it takes.
    function refreshGrid(dlg) {
        var controls = dlg ? dlg.querySelector('.picker-controls') : null;
        if (controls && window.htmx) {
            htmx.trigger(controls, 'picker-refresh');
        }
    }

    // setTabStyles marks the active tab. It swaps the whole class, not
    // individual utilities: the look comes from the picker-tab/picker-tab-on
    // shortcuts, and toggling a utility the shortcut also sets would leave the
    // two fighting over the same rule.
    function setTabStyles(dlg, tab) {
        if (!dlg) {
            return;
        }
        dlg.querySelectorAll('.state-tab').forEach(function (btn) {
            btn.className = btn.dataset.tab === tab ? 'state-tab active picker-tab-on' : 'state-tab picker-tab';
        });
    }

    // setTreasureTab flips the treasure picker between its all/normal/evolved
    // tabs. The tab is a hidden field on the controls form, so the re-query
    // picks it up; the server does the filtering.
    function setTreasureTab(tab) {
        treasureTab = tab;
        var dlg = document.getElementById('dialog-treasure');
        if (!dlg) {
            return;
        }
        setTabStyles(dlg, tab);
        var hidden = dlg.querySelector('.picker-controls input[name="tab"]');
        if (hidden) {
            hidden.value = tab;
        }
        refreshGrid(dlg);
    }

    // pickerLevel is the level the treasure grid should show: whatever the
    // user last set on the picker's top slider, else the target slot's stored
    // level so re-picking shows the values the build will store (9 when there
    // is no slot, e.g. the /builds filter dialog).
    function pickerLevel() {
        var slotScope = document.getElementById('slot-' + treasureSlot);
        slotScope = slotScope ? slotScope.closest('[data-level-scope]') : null;
        var slotLevel = window.getScopeLevel ? getScopeLevel(slotScope) : 9;
        return lastPickerLevel !== null ? lastPickerLevel : slotLevel;
    }

    // gridLevel is the level the treasure grid is currently painted at: the
    // picker's own slider, which paintGridLevel and the per-card steppers work
    // against. Used when a scrolled-in page has to match the cards above it.
    function gridLevel() {
        var dlg = document.getElementById('dialog-treasure');
        var slider = dlg ? dlg.querySelector('.level-slider') : null;
        var l = slider ? parseInt(slider.value, 10) : 9;
        return isNaN(l) || l < 0 || l > 9 ? 9 : l;
    }

    // resetCard puts one treasure card back to its normal (non-blessed) effect
    // line and paints it at `level`.
    function resetCard(card, level) {
        var line = card.querySelector('.fx-line');
        if (line) {
            line.querySelectorAll('.fx-effects').forEach(function (g) {
                g.classList.toggle('hidden', g.dataset.state !== 'normal');
            });
            line.querySelectorAll('.fx-pill').forEach(function (p) {
                var on = p.dataset.state === 'normal';
                p.classList.toggle('active', on);
                p.classList.toggle('text-accent', on);
                p.classList.toggle('border-accent', on);
                p.classList.toggle('text-foreground-muted', !on);
                p.classList.toggle('border-secondary/40', !on);
            });
        }
        setCardLevel(card, level);
    }

    // initCards prepares the cards a swap just added: builds each one's 0-9
    // stepper and paints it at `level`. Marked per card, not per dialog, so a
    // scrolled-in page is set up without disturbing the cards above it — a
    // blessed toggle or a level override on an earlier page must survive the
    // next page landing.
    function initCards(dlg, level) {
        dlg.querySelectorAll('.pick-option:not([data-card-init])').forEach(function (card) {
            card.dataset.cardInit = '1';
            buildStepper(card);
            resetCard(card, level);
        });
    }

    // prepareCards resets every card in a dialog that is about to open: each
    // open starts from a fresh, deterministic state (the server default), so
    // a blessed toggle or a per-card level override does not leak across
    // picks. On the very first open the grid is still loading and there is
    // nothing to reset — the grid observer below picks it up.
    function prepareCards(kind) {
        var dlg = document.getElementById('dialog-' + kind);
        if (!dlg || kind !== 'treasure') {
            return;
        }
        var level = pickerLevel();
        // the slider wiring lives in level_slider.js (shared with the treasure
        // detail page); only the per-open level choice is picker logic
        if (window.setPickerLevel) {
            setPickerLevel(level);
        } else if (window.resetLevelSlider) {
            resetLevelSlider();
        }
        ensureDialogKeys(dlg);
        cancelGridLevel();
        dlg.querySelectorAll('.pick-option').forEach(function (card) {
            card.dataset.cardInit = '1';
            buildStepper(card);
            resetCard(card, level);
        });
    }

    // targetSlot is the slot this dialog is picking for: t1..t3 for treasures,
    // cookie/cookie2 for the two cookie slots, pet for the pet.
    function targetSlot(kind) {
        if (kind === 'treasure') {
            return treasureSlot;
        }
        return kind === 'cookie' ? cookieSlot : kind;
    }

    // currentPick is the id already in the target slot ('' when empty). The
    // server pins it to the front of the grid, which is the only way a filled
    // slot can show its pick: the grid is paginated, so the card may sit
    // hundreds of entries down a page nobody has scrolled to.
    function currentPick(kind) {
        var input = document.getElementById('sel-' + targetSlot(kind));
        var v = input ? input.value : '';
        return v && v !== '0' ? v : '';
    }

    function openPicker(kind) {
        var dlg = document.getElementById('dialog-' + kind);
        // a close via ✕/Esc must not re-fire the previous pick: returnValue
        // persists on the dialog element, so reset it (and the pick cache)
        // each time the dialog opens.
        dlg.returnValue = '';
        lastPick = null;
        // the option grid is not in the page HTML — the three grids together
        // ran to ~2.2MB on pages most visitors never open a dialog on, and the
        // treasure one alone was 1.9MB. It loads through the controls form
        // below, page 1 first and the rest as the user scrolls; the grid
        // observer sets up whatever lands.
        var grid = dlg.querySelector('[data-picker-grid]');
        var q = dlg.querySelector('.picker-controls input[type="search"]');
        var tabInput = dlg.querySelector('.picker-controls input[name="tab"]');
        var selInput = dlg.querySelector('.picker-controls input[name="sel"]');
        var pick = currentPick(kind);
        // reload when the grid has never been filled, when a search term or
        // tab is left over from the last open (it would show a filtered grid
        // under an empty search box), or when the slot's pick changed and the
        // grid is pinned to the wrong card.
        var stale = (q && q.value !== '') || (tabInput && tabInput.value !== 'all') ||
            (selInput && selInput.value !== (pick || '0')) ||
            !(grid && grid.querySelector('.pick-option'));
        if (q) {
            q.value = '';
        }
        if (selInput) {
            selInput.value = pick || '0';
        }
        if (tabInput) {
            tabInput.value = 'all';
        }
        setTabStyles(dlg, 'all');
        treasureTab = 'all';
        if (stale) {
            refreshGrid(dlg);
        }
        prepareCards(kind);
        applyExclusion(dlg);
        if (q) {
            q.focus();
        }
        updateDialogStep(targetSlot(kind));
        dlg.showModal();
    }

    // Cards arrive after the dialog is already open — page 1 on the first
    // open, later pages as the sentinel scrolls into view, a fresh page 1 on
    // every search — and each batch needs its steppers built and its level
    // painted. A MutationObserver rather than an htmx event: the sentinel
    // swaps itself with outerHTML, and htmx fires the after-swap/after-settle
    // events on the detached node, so a document-level listener never sees an
    // appended page. Watching the grid catches every path, htmx or not.
    function onGridChanged(dlg, grid) {
        var first = grid.querySelector('.pick-option');
        // a fresh page 1 (first open or a search) replaces the grid, so its
        // leading card is uninitialized; an appended page leaves it alone.
        // Only the replacement should jump the scroll back to the top.
        if (first && !first.dataset.cardInit) {
            grid.scrollTop = 0;
        }
        if (dlg.dataset.kind === 'treasure') {
            ensureDialogKeys(dlg);
            initCards(dlg, gridLevel());
        }
        applyExclusion(dlg);
    }

    document.querySelectorAll('dialog[data-kind]').forEach(function (dlg) {
        var grid = dlg.querySelector('[data-picker-grid]');
        if (!grid) {
            return;
        }
        new MutationObserver(function () {
            onGridChanged(dlg, grid);
        }).observe(grid, { childList: true });
    });

    function openCookie(slot) {
        cookieSlot = slot || 'cookie';
        openPicker('cookie');
    }

    function openTreasure(slot) {
        treasureSlot = slot;
        openPicker('treasure');
    }

    // toggleFx flips a treasure option between its normal and blessed
    // effect. The pills live inside the submit button, so the wrapper stops
    // propagation to keep a pill click from picking the treasure.
    function toggleFx(event, pill) {
        if (event) {
            event.preventDefault();
            event.stopPropagation();
        }
        var line = pill.closest('.fx-line');
        if (!line) {
            return;
        }
        var blessed = pill.dataset.state === 'blessed';
        line.querySelectorAll('.fx-pill').forEach(function (p) {
            var on = p === pill;
            p.classList.toggle('active', on);
            p.classList.toggle('text-accent', on);
            p.classList.toggle('border-accent', on);
            p.classList.toggle('text-foreground-muted', !on);
            p.classList.toggle('border-secondary/40', !on);
        });
        line.querySelectorAll('.fx-effects').forEach(function (g) {
            g.classList.toggle('hidden', (g.dataset.state === 'blessed') !== blessed);
        });
    }

    // Repainting the treasure grid at a new level is O(cards x effect cells)
    // and the dialog server-renders the whole catalog, so a slider drag —
    // which fires `input` per pixel — would rebuild it dozens of times over.
    // Grid repaints are therefore debounced to the last level of a burst;
    // anything that needs the grid settled (slider release, a pick, a stepper
    // override, a dialog open) flushes or cancels the pending paint first.
    var GRID_LEVEL_DELAY = 120;
    var gridLevelTimer = null;
    var pendingGridLevel = null;

    function paintGridLevel(level) {
        var dlg = document.getElementById('dialog-treasure');
        if (!dlg) {
            return;
        }
        dlg.querySelectorAll('.pick-option').forEach(function (card) {
            setCardLevel(card, level);
        });
    }

    function queueGridLevel(level) {
        pendingGridLevel = level;
        if (gridLevelTimer !== null) {
            clearTimeout(gridLevelTimer);
        }
        gridLevelTimer = setTimeout(flushGridLevel, GRID_LEVEL_DELAY);
    }

    // flushGridLevel paints a pending level now; a no-op when nothing is
    // pending, so callers can use it as a "settle the grid" guard.
    function flushGridLevel() {
        // level_slider.js debounces its own cell repaint (an unscoped control
        // repaints every [data-values] on the page); settle that too, or it
        // lands after ours and overwrites the level we just painted.
        if (window.flushLevelSlider) {
            flushLevelSlider();
        }
        if (gridLevelTimer !== null) {
            clearTimeout(gridLevelTimer);
            gridLevelTimer = null;
        }
        if (pendingGridLevel === null) {
            return;
        }
        var level = pendingGridLevel;
        pendingGridLevel = null;
        paintGridLevel(level);
    }

    // cancelGridLevel drops a pending paint: the dialog open path paints
    // every card itself, and a late repaint would fight it.
    function cancelGridLevel() {
        if (gridLevelTimer !== null) {
            clearTimeout(gridLevelTimer);
            gridLevelTimer = null;
        }
        pendingGridLevel = null;
    }

    // setCardLevel re-renders one picker card at a given level: its value
    // cells (from data-values; covers both the normal and hidden blessed
    // groups) and the stepper's active button.
    function setCardLevel(card, l) {
        if (l < 0 || l > 9) {
            l = 9;
        }
        // the card keeps its own displayed level: the picker captures it at
        // pick time so the slot's stored level matches what the planner
        // actually showed (per-card stepper override or top-slider default).
        card.dataset.level = l;
        card.querySelectorAll('.fx-val').forEach(function (el) {
            var vals = (el.getAttribute('data-values') || '').split('|');
            el.textContent = vals[l] || '';
            // +9 delta badge next to the value: how much the effect gains at
            // max level vs the level currently shown ('' at level 9, or when
            // either end has no parseable number). Signed, so a drain-type
            // value that goes -2.3% -> -3% reads "-0.7".
            var up = el.parentNode.querySelector('.fx-up');
            var cur = parseFloat(vals[l]);
            var max = parseFloat(vals[9]);
            if (l < 9 && !isNaN(cur) && !isNaN(max) && vals[l] !== undefined && vals[9] !== undefined) {
                var d = Math.round((max - cur) * 100) / 100;
                if (d !== 0) {
                    if (!up) {
                        up = document.createElement('span');
                        up.className = 'fx-up text-[10px] text-foreground-muted font-semibold';
                        up.setAttribute('aria-hidden', 'true');
                        el.parentNode.insertBefore(up, el.nextSibling);
                    }
                    up.textContent = ' ' + (d > 0 ? '+' : '') + d;
                    return;
                }
            }
            if (up) {
                up.remove();
            }
        });
        card.querySelectorAll('.fx-step').forEach(function (s) {
            var on = parseInt(s.getAttribute('data-l'), 10) === l;
            s.classList.toggle('bg-primary', on);
            s.classList.toggle('text-foreground', on);
            s.classList.toggle('text-foreground-muted', !on);
        });
        // hover hint keeps the current level and the arrow shortcut visible;
        // screen readers get the level from the live region (keyboard path)
        card.title = (card.dataset.name || '') + ' - level ' + l + ' (use left/right arrows to change)';
        // corner badge when this card's level differs from the top slider
        updateOverrideBadge(card);
    }

    // updateOverrideBadge marks a card whose per-card level differs from the
    // picker's top slider (the grid default), so overrides are visible at a
    // glance. The badge is a tiny accent chip in the card's top-right corner
    // showing the overridden level; it appears/updates as levels change and
    // is removed when the card matches the slider again (slider move or
    // dialog open). Clicking the badge must not pick the treasure, so its
    // click is swallowed like the stepper's.
    function updateOverrideBadge(card) {
        var dlg = card.closest('dialog');
        var slider = dlg ? dlg.querySelector('.level-slider') : null;
        var grid = slider ? parseInt(slider.value, 10) : 9;
        if (isNaN(grid) || grid < 0 || grid > 9) {
            grid = 9;
        }
        var lvl = parseInt(card.dataset.level, 10);
        if (isNaN(lvl) || lvl < 0 || lvl > 9) {
            lvl = 9;
        }
        var host = card.querySelector('.fx-override-badge');
        if (lvl === grid) {
            if (host) {
                host.remove();
            }
            return;
        }
        if (!host) {
            host = document.createElement('span');
            host.className = 'fx-override-badge absolute -top-2 -right-2 size-5 rounded-full bg-accent text-on-accent text-[10px] font-bold flex items-center justify-center shadow';
            host.addEventListener('click', function (ev) {
                ev.preventDefault();
                ev.stopPropagation();
            });
            card.appendChild(host);
        }
        host.textContent = lvl;
    }

    // buildStepper builds one card's tiny 0-9 level stepper. The step buttons
    // are JS-built (the container lives in the template) so a page of cards
    // stays as light on the wire as it can be. A click on a step overrides
    // just that card; stopPropagation (here and inline on the container)
    // keeps the event from activating the card's submit button, mirroring the
    // fx-pill pattern.
    function buildStepper(card) {
        var host = card.querySelector('.fx-stepper');
        if (!host || host.childElementCount > 0) {
            return;
        }
        for (var l = 0; l <= 9; l++) {
            var s = document.createElement('span');
            s.className = 'fx-step rounded px-1 cursor-pointer select-none text-foreground-muted hover:text-accent transition-colors';
            s.setAttribute('data-l', l);
            s.textContent = l;
            host.appendChild(s);
        }
        host.addEventListener('click', function (ev) {
            var step = ev.target.closest('.fx-step');
            if (!step) {
                return;
            }
            ev.preventDefault();
            ev.stopPropagation();
            // settle the grid first: a queued paint would wipe this override
            // when it lands
            flushGridLevel();
            setCardLevel(card, parseInt(step.getAttribute('data-l'), 10));
        });
    }

    // ensureDialogKeys wires the treasure dialog's arrow-key level cycling and
    // its screen-reader live region. Both are per dialog, not per card, so
    // they are installed once and keep working for pages scrolled in later.
    function ensureDialogKeys(dlg) {
        if (!dlg || dlg.dataset.keysBound === '1') {
            return;
        }
        dlg.dataset.keysBound = '1';
        // arrow-key cycling: the card is a single tab stop, so arrows set the
        // level without precise clicks on the tiny digits. Delegated on the
        // dialog (one listener, covers every card); the search box and any
        // inner spans are either inside a .pick-option or ignored.
        dlg.addEventListener('keydown', function (ev) {
            var card = ev.target && ev.target.closest ? ev.target.closest('.pick-option') : null;
            if (!card || !dlg.contains(card)) {
                return;
            }
            // step from what the card actually shows: a queued slider paint
            // has not written dataset.level yet
            flushGridLevel();
            var lvl = parseInt(card.dataset.level, 10);
            if (isNaN(lvl) || lvl < 0 || lvl > 9) {
                lvl = 9;
            }
            var next = -1;
            if (ev.key === 'ArrowLeft') {
                next = lvl - 1;
            } else if (ev.key === 'ArrowRight') {
                next = lvl + 1;
            } else if (ev.key === 'Home') {
                next = 0;
            } else if (ev.key === 'End') {
                next = 9;
            } else {
                return;
            }
            // clamp first, then bail on a real no-op: ArrowRight at max must
            // neither re-render nor announce a phantom change
            next = Math.max(0, Math.min(9, next));
            if (next === lvl) {
                return;
            }
            ev.preventDefault();
            ev.stopPropagation();
            setCardLevel(card, next);
            // announce to assistive tech; the live region is created below
            // (before this handler can run)
            var live = dlg.querySelector('.fx-level-live');
            if (live) {
                live.textContent = (card.dataset.name || 'Treasure') + ', level ' + next;
            }
        });
        // sr-only polite live region for keyboard level announcements
        var live = document.createElement('span');
        live.className = 'sr-only fx-level-live';
        live.setAttribute('aria-live', 'polite');
        dlg.appendChild(live);
    }

    function setPicked(btn) {
        // a pending slider repaint would leave this card showing the previous
        // level's values, which are what gets captured below
        flushGridLevel();
        var line = btn.querySelector('.fx-line');
        var blessedGroup = line ? line.querySelector('.fx-effects[data-state="blessed"]') : null;
        var blessed = !!blessedGroup && !blessedGroup.classList.contains('hidden');
        var group = blessed ? blessedGroup : (line ? line.querySelector('.fx-effects[data-state="normal"]') : null);
        var effects = [];
        if (group) {
            group.querySelectorAll('.fx-effect').forEach(function (fx) {
                var val = fx.querySelector('.fx-val');
                effects.push({
                    value: val ? val.textContent : '',
                    // the whole 0-9 ladder, not just the level shown: the slot
                    // gets its own level slider, and level_slider.js repaints a
                    // cell from data-values
                    values: val ? (val.getAttribute('data-values') || '') : '',
                    text: fx.querySelector('.fx-name').textContent,
                });
            });
        }
        var lvl = parseInt(btn.dataset.level, 10);
        if (isNaN(lvl) || lvl < 0 || lvl > 9) {
            lvl = 9;
        }
        lastPick = {
            name: btn.dataset.name,
            img: btn.querySelector('img').getAttribute('src'),
            effects: effects,
            blessed: blessed,
            level: lvl,
        };
    }

    function updateSlot(slot) {
        var el = document.getElementById('slot-' + slot);
        if (!el || !lastPick) {
            return;
        }
        el.replaceChildren();
        var img = document.createElement('img');
        img.src = lastPick.img;
        img.alt = lastPick.name;
        img.className = 'size-20 object-contain';
        var name = document.createElement('span');
        name.className = 'text-center text-sm font-bold text-on-surface leading-tight';
        name.textContent = lastPick.name;
        el.append(img, name);
        if (lastPick.effects && lastPick.effects.length) {
            var fxLine = document.createElement('span');
            fxLine.className = 'flex flex-col items-center gap-0.5';
            lastPick.effects.forEach(function (e) {
                var fxText = document.createElement('span');
                fxText.className = 'text-center text-xs text-foreground-muted leading-snug';
                if (e.value) {
                    var val = document.createElement('span');
                    val.className = 'text-accent';
                    // data-values makes the cell one the slot's own level
                    // slider can repaint; without it the slot kept showing the
                    // level it was picked at while the slider moved under it
                    if (e.values) {
                        val.setAttribute('data-values', e.values);
                    }
                    val.textContent = e.value;
                    fxText.append(val);
                    // the gap is its own text node, not part of the value: a
                    // repaint rewrites the cell's textContent and would eat a
                    // trailing space baked into it
                    fxText.append(document.createTextNode(' '));
                }
                fxText.append(document.createTextNode(e.text));
                fxLine.append(fxText);
            });
            if (lastPick.blessed && el.dataset.blessedLabel) {
                var tag = document.createElement('span');
                tag.className = 'text-[10px] font-label uppercase text-accent';
                tag.textContent = el.dataset.blessedLabel;
                fxLine.append(tag);
            }
            el.append(fxLine);
        }
        ensureClear(slot);
    }

    function ensureClear(slot) {
        var slotEl = document.getElementById('slot-' + slot);
        var wrap = slotEl ? slotEl.parentNode : null;
        if (!wrap || wrap.querySelector('.slot-clear')) {
            return;
        }
        var btn = document.createElement('button');
        btn.type = 'button';
        btn.title = slotEl.dataset.clearTitle || 'Clear';
        btn.className = 'slot-clear absolute -top-2 -right-2 size-6 rounded-full bg-surface border border-primary/40 text-foreground-muted hover:text-accent hover:border-accent flex items-center justify-center text-sm leading-none shadow';
        btn.textContent = '✕';
        btn.addEventListener('click', function () {
            clearSlot(slot);
        });
        wrap.append(btn);
    }

    function clearSlot(slot) {
        var input = document.getElementById('sel-' + slot);
        var el = document.getElementById('slot-' + slot);
        if (!input || !el) {
            return;
        }
        input.value = '';
        // treasure slots also carry the blessed flag; reset it with the pick
        var blessed = document.getElementById('blessed-' + slot);
        if (blessed) {
            blessed.value = '0';
        }
        el.replaceChildren();
        var label = document.createElement('span');
        label.className = 'my-auto text-xs font-label text-foreground-muted uppercase';
        label.textContent = el.dataset.empty || '';
        el.append(label);
        var clear = el.parentNode.querySelector('.slot-clear');
        if (clear) {
            clear.remove();
        }
        if (filterMode) {
            triggerFilter(slot);
        } else {
            refreshPreview();
        }
    }

    // triggerFilter re-runs the /builds htmx filter after a pick or clear:
    // assigning a hidden input's value fires no native event, so use htmx's
    // trigger API on the form (its hx-trigger="change" listener).
    function triggerFilter(slot) {
        if (!filterForm) {
            return;
        }
        if (window.htmx && window.htmx.trigger) {
            htmx.trigger(filterForm, 'change');
        } else {
            filterForm.dispatchEvent(new Event('change', { bubbles: true }));
        }
    }

    function refreshPreview() {
        var box = document.getElementById('build-preview');
        if (!box) {
            return;
        }
        var params = ['cookie', 'c2', 'pet', 't1', 't2', 't3'].map(function (name) {
            var inputId = name === 'c2' ? 'sel-cookie2' : 'sel-' + name;
            var input = document.getElementById(inputId);
            return name + '=' + encodeURIComponent(input ? input.value : '');
        }).join('&');
        fetch('/builds/preview?' + params)
            .then(function (r) {
                return r.text();
            })
            .then(function (html) {
                var hadCombo = !!box.querySelector('.combo-box');
                box.innerHTML = html;
                // the template carries animate-combo-in so the box plays its
                // entrance animation when first mounted; on later refreshes
                // (treasure picks) the box is already visible, so strip the
                // class to keep it from replaying.
                if (hadCombo) {
                    var combo = box.querySelector('.combo-box');
                    if (combo) {
                        combo.classList.remove('animate-[combo-in_.4s_ease-out]');
                    }
                }
            })
            .catch(function () {
                // leave the current preview untouched on network errors
            });
    }

    // advance opens the next picker in the flow and returns true when it did;
    // false means the flow ended (relay skip/end, after t3) and no dialog opens.
    function advance(kind) {
        if (kind === 'cookie') {
            // after the lead cookie pick, offer the optional relay picker;
            // after a relay pick the cookie flow continues to the pet picker
            // (relay stays optional — ✕/Esc on the relay dialog skips it and
            // ends the cookie flow without advancing).
            if (cookieSlot === 'cookie') {
                openCookie('cookie2');
                return true;
            }
            openPicker('pet');
            return true;
        } else if (kind === 'pet') {
            openTreasure('t1');
            return true;
        } else if (treasureSlot === 't1') {
            openTreasure('t2');
            return true;
        } else if (treasureSlot === 't2') {
            openTreasure('t3');
            return true;
        }
        // after the third treasure the flow ends; the user fills EP/tag and submits
        return false;
    }

    // validateBuild blocks the submit when the loadout is incomplete, so
    // invalid posts never reach the server. Mirrors the server-side checks
    // (server validation stays authoritative).
    function validateBuild(form) {
        var err = document.getElementById('build-error');
        // relay cookie (cookie2) is optional; the rest are required
        var slots = ['cookie', 'pet', 't1', 't2', 't3'];
        for (var i = 0; i < slots.length; i++) {
            var input = document.getElementById('sel-' + slots[i]);
            if (!input || input.value === '' || input.value === '0') {
                showBuildError(err, err ? err.dataset.msgSlots : '');
                return false;
            }
        }
        var ep = form.querySelector('select[name="ep"]');
        if (!ep || !ep.value) {
            showBuildError(err, err ? err.dataset.msgEp : '');
            return false;
        }
        var tag = form.querySelector('input[type="checkbox"][name^="tag_"]:checked');
        if (!tag) {
            showBuildError(err, err ? err.dataset.msgTag : '');
            return false;
        }
        return true;
    }

    function showBuildError(err, msg) {
        if (!err) {
            return;
        }
        err.textContent = msg;
        err.classList.remove('hidden');
    }

    document.querySelectorAll('dialog').forEach(function (dlg) {
        dlg.addEventListener('close', function () {
            var v = dlg.returnValue;
            if (!v || !lastPick) {
                return;
            }
            var kind = dlg.dataset.kind;
            var target = kind === 'treasure' ? treasureSlot : (kind === 'cookie' ? cookieSlot : kind);
            document.getElementById('sel-' + target).value = v;
            // treasure picks persist their blessed/normal toggle into the
            // form so the badge survives the submit; cookie/pet have none.
            if (kind === 'treasure') {
                var blessed = document.getElementById('blessed-' + target);
                if (blessed) {
                    blessed.value = lastPick.blessed ? '1' : '0';
                }
                // store the level the picked card displayed (its per-card
                // stepper or the picker's top slider) on the slot: the hidden
                // field, the scoped slider/input knobs and the slot's value
                // cells all follow, so the build records what the planner
                // showed. The /builds filter dialog has no slot scope —
                // nothing to sync there.
                if (window.setScopeLevel) {
                    var slotBtn = document.getElementById('slot-' + target);
                    var slotScope = slotBtn ? slotBtn.closest('[data-level-scope]') : null;
                    if (slotScope) {
                        setScopeLevel(slotScope, lastPick.level);
                    }
                }
            }
            updateSlot(target);
            if (filterMode) {
                // /builds: a pick is just a filter change — no next dialog.
                triggerFilter(target);
                return;
            }
            refreshPreview();
            // advance() opens the next dialog, whose openPicker renders its
            // step header; when the flow ends nothing opens.
            advance(kind);
        });
    });

    var buildForm = document.querySelector('form[action="/builds/new"]');
    if (buildForm) {
        buildForm.addEventListener('submit', function (ev) {
            if (!validateBuild(buildForm)) {
                ev.preventDefault();
            }
        });
    }

    // htmx's own submit listener ignores preventDefault() from other
    // handlers, so an invalid loadout would still POST. Cancel through
    // htmx's config:request event instead (preventDefault on it drops the
    // request); the native submit guard above stays for the no-htmx case.
    document.addEventListener('htmx:config:request', function (ev) {
        var elt = ev.detail && (ev.detail.elt || (ev.detail.ctx && ev.detail.ctx.sourceElement));
        if (elt && elt.matches && elt.matches('form[action="/builds/new"]')
            && !validateBuild(elt)) {
            ev.preventDefault();
        }
    });

    // updateLegendCounts refreshes each fieldset legend's "n/total" counter:
    // count the checked checkboxes inside that fieldset (scoped, so tags /
    // boosts / power effects never cross-contaminate). Runs on load so the
    // server-rendered initial (0/n on /builds/new, prefill on edit) is kept
    // in sync, and on every change.
    function updateLegendCounts() {
        document.querySelectorAll('fieldset').forEach(function (fs) {
            var count = fs.querySelector('.legend-count');
            if (!count) {
                return;
            }
            var total = count.dataset.total || '0';
            var n = fs.querySelectorAll('input[type="checkbox"]:checked').length;
            count.textContent = n + '/' + total;
        });
    }

    // syncChipStates mirrors each chip checkbox's checked state onto its
    // label's `is-on` class, which swaps the On/Off pill text. A JS class is
    // used because Chrome here neither invalidates descendant :has() rules
    // nor applies opacity/transform transitions on class-toggled elements
    // (both froze at stale computed values in testing); the pill's own
    // color morph is peer-checked + transition-colors, which does animate.
    function syncChipStates() {
        document.querySelectorAll('label.group input[type="checkbox"]').forEach(function (input) {
            input.closest('label').classList.toggle('is-on', input.checked);
        });
    }

    // the picker's top slider is the grid default: when it moves, reset
    // every card to the new level (clearing any per-card stepper override)
    // so the slider stays the source of truth for the grid.
    var treasureDlg = document.getElementById('dialog-treasure');
    if (treasureDlg) {
        var treasureSlider = treasureDlg.querySelector('.level-slider');
        if (treasureSlider) {
            treasureSlider.addEventListener('input', function () {
                var v = parseInt(treasureSlider.value, 10);
                lastPickerLevel = v;
                queueGridLevel(v);
            });
            // pointer release / keyboard commit: the drag is over, so paint
            // immediately instead of sitting out the debounce window.
            treasureSlider.addEventListener('change', flushGridLevel);
        }
    }

    document.addEventListener('change', function (ev) {
        if (ev.target && ev.target.type === 'checkbox') {
            updateLegendCounts();
            syncChipStates();
        }
    });
    updateLegendCounts();
    syncChipStates();

    window.openPicker = openPicker;
    window.openCookie = openCookie;
    window.openTreasure = openTreasure;
    window.setPicked = setPicked;
    window.toggleFx = toggleFx;
    window.clearSlot = clearSlot;
    window.setTreasureTab = setTreasureTab;
    window.updateLegendCounts = updateLegendCounts;
})();
