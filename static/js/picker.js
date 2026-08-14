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

    function filterOptions(kind, term) {
        var dlg = document.getElementById('dialog-' + kind);
        var t = term.trim().toLowerCase();
        dlg.querySelectorAll('.pick-option').forEach(function (btn) {
            btn.hidden = t !== '' && btn.dataset.name.toLowerCase().indexOf(t) === -1;
        });
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
        }
        var q = dlg.querySelector('input[type="search"]');
        if (q) {
            q.value = '';
            filterOptions(kind, '');
            q.focus();
        }
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
        lastPick = {
            name: btn.dataset.name,
            img: btn.querySelector('img').getAttribute('src'),
            effects: effects,
            blessed: blessed,
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
        var wrap = document.getElementById('slot-' + slot).parentNode;
        if (wrap.querySelector('.slot-clear')) {
            return;
        }
        var btn = document.createElement('button');
        btn.type = 'button';
        btn.title = 'Clear';
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
        el.replaceChildren();
        var label = document.createElement('span');
        label.className = 'my-auto text-xs font-label text-foreground-muted uppercase';
        label.textContent = el.dataset.empty || '';
        el.append(label);
        var clear = el.parentNode.querySelector('.slot-clear');
        if (clear) {
            clear.remove();
        }
        refreshPreview();
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

    function advance(kind) {
        if (kind === 'cookie') {
            openPicker('pet');
        } else if (kind === 'pet') {
            openTreasure('t1');
        } else if (treasureSlot === 't1') {
            openTreasure('t2');
        } else if (treasureSlot === 't2') {
            openTreasure('t3');
        }
        // after the third treasure the flow ends; the user fills EP/tag and submits
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
            updateSlot(target);
            refreshPreview();
            advance(kind);
        });
    });

    window.openPicker = openPicker;
    window.openCookie = openCookie;
    window.openTreasure = openTreasure;
    window.filterOptions = filterOptions;
    window.setPicked = setPicked;
    window.toggleFx = toggleFx;
    window.clearSlot = clearSlot;
})();
