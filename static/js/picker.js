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

    function filterOptions(kind, term) {
        var dlg = document.getElementById('dialog-' + kind);
        var t = term.trim().toLowerCase();
        // when picking the relay cookie, hide the already-picked main cookie
        // (its id sits in the sel-cookie hidden input; option ids are the
        // submit buttons' values) so the same cookie can't be chosen twice
        var excludeId = '';
        if (kind === 'cookie' && cookieSlot === 'cookie2') {
            var lead = document.getElementById('sel-cookie');
            excludeId = lead ? lead.value : '';
        }
        dlg.querySelectorAll('.pick-option').forEach(function (btn) {
            // the treasure filter's all/normal/evolved tabs hide options by
            // evolved state before the search term applies
            if (kind === 'treasure' && treasureTab !== 'all') {
                var isEvo = btn.dataset.evolved === 'true' || btn.dataset.evolved === '1';
                if ((treasureTab === 'normal' && isEvo) || (treasureTab === 'evo' && !isEvo)) {
                    btn.hidden = true;
                    return;
                }
            }
            var excluded = excludeId !== '' && btn.value === excludeId;
            if (t === '') { btn.hidden = excluded; return; }
            // match the localized name or the English name, so e.g. a th page
            // still finds "Wizard" -> คุกกี้พ่อมด in the picker; treasure
            // options also match their effect pill lines (.fx-effect)
            var hay = (btn.dataset.name || '') + ' ' + (btn.dataset.nameEn || '');
            btn.querySelectorAll('.fx-effect').forEach(function (fx) { hay += ' ' + fx.textContent; });
            btn.hidden = excluded || hay.toLowerCase().indexOf(t) === -1;
        });
    }

    // setTreasureTab flips the treasure filter dialog between its
    // all/normal/evolved tabs, re-applying the search filter afterwards.
    function setTreasureTab(tab) {
        treasureTab = tab;
        var dlg = document.getElementById('dialog-treasure');
        if (!dlg) {
            return;
        }
        dlg.querySelectorAll('.state-tab').forEach(function (btn) {
            var on = btn.dataset.tab === tab;
            btn.classList.toggle('bg-primary', on);
            btn.classList.toggle('text-foreground', on);
            btn.classList.toggle('border-2', !on);
            btn.classList.toggle('border-secondary/50', !on);
            btn.classList.toggle('text-secondary', !on);
        });
        var q = dlg.querySelector('input[type="search"]');
        filterOptions('treasure', q ? q.value : '');
    }

    function openPicker(kind) {
        var dlg = document.getElementById('dialog-' + kind);
        // a close via ✕/Esc must not re-fire the previous pick: returnValue
        // persists on the dialog element, so reset it (and the pick cache)
        // each time the dialog opens.
        dlg.returnValue = '';
        lastPick = null;
        // treasure options keep their normal/blessed toggle state on the DOM
        // between opens — reset every line back to normal so each pick starts
        // from a fresh, deterministic state (matches the server default).
        if (kind === 'treasure') {
            dlg.querySelectorAll('.fx-line').forEach(function (line) {
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
            });
            // the picker's level: the last level the user set on the top
            // slider (remembered across dialog opens), or — before any
            // manual choice — the target slot's stored level so re-picking
            // shows the values the build will store (9 when no slot exists,
            // e.g. the /builds filter dialog). The slider wiring itself
            // lives in level_slider.js (shared with the treasure detail
            // page); only the per-open level choice is picker logic.
            var slotScope = document.getElementById('slot-' + treasureSlot);
            slotScope = slotScope ? slotScope.closest('[data-level-scope]') : null;
            var slotLevel = window.getScopeLevel ? getScopeLevel(slotScope) : 9;
            var level = lastPickerLevel !== null ? lastPickerLevel : slotLevel;
            if (window.setPickerLevel) {
                setPickerLevel(level);
            } else if (window.resetLevelSlider) {
                resetLevelSlider();
            }
            // build the per-card steppers (lazily, once) and set every card
            // to the level above; individual cards can then be bumped via
            // their own stepper for side-by-side comparison (those overrides
            // reset when the dialog reopens).
            ensureSteppers();
            dlg.querySelectorAll('.pick-option').forEach(function (card) {
                setCardLevel(card, level);
            });
        }
        var q = dlg.querySelector('input[type="search"]');
        if (q) {
            q.value = '';
            filterOptions(kind, '');
            q.focus();
        }
        var stepSlot = kind === 'treasure' ? treasureSlot : (kind === 'cookie' ? cookieSlot : kind);
        updateDialogStep(stepSlot);
        dlg.showModal();
    }

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

    // ensureSteppers builds each card's tiny 0-9 level stepper lazily on the
    // first treasure-dialog open, so the initial page stays as light as it
    // can be (the dialog already server-renders every treasure). The step
    // buttons are JS-built; the container lives in the template. A click on
    // a step overrides just that card; stopPropagation (here and inline on
    // the container) keeps the event from activating the card's submit
    // button, mirroring the fx-pill pattern.
    function ensureSteppers() {
        var dlg = document.getElementById('dialog-treasure');
        if (!dlg || dlg.dataset.steppersBuilt === '1') {
            return;
        }
        dlg.dataset.steppersBuilt = '1';
        dlg.querySelectorAll('.pick-option').forEach(function (card) {
            var host = card.querySelector('.fx-stepper');
            if (!host) {
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
                setCardLevel(card, parseInt(step.getAttribute('data-l'), 10));
            });
        });
        // arrow-key cycling: the card is a single tab stop, so arrows set the
        // level without precise clicks on the tiny digits. Delegated on the
        // dialog (one listener, covers every card); the search box and any
        // inner spans are either inside a .pick-option or ignored.
        dlg.addEventListener('keydown', function (ev) {
            var card = ev.target && ev.target.closest ? ev.target.closest('.pick-option') : null;
            if (!card || !dlg.contains(card)) {
                return;
            }
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
                    val.textContent = e.value + ' ';
                    fxText.append(val);
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
                treasureDlg.querySelectorAll('.pick-option').forEach(function (card) {
                    setCardLevel(card, v);
                });
            });
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
    window.filterOptions = filterOptions;
    window.setPicked = setPicked;
    window.toggleFx = toggleFx;
    window.clearSlot = clearSlot;
    window.setTreasureTab = setTreasureTab;
    window.updateLegendCounts = updateLegendCounts;
})();
