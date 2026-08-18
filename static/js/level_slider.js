// level_slider.js — wires every treasure level control rendered by
// templates/components/level_slider.html (slider + number input + tick
// marks). Each control is scoped to its nearest [data-level-scope] ancestor,
// or the whole page when none exists: its [data-values] / [data-diffs] cells
// (index = level) re-render on change, the ticks highlight and the themed
// fill follows. The scope's data-level-default prefills the starting level
// (the build editor stores per-slot levels this way); an optional
// [data-level-field] hidden input inside the scope receives the value for
// form submission. resetLevelSlider() returns only the unscoped controls
// (treasure detail page, planner picker) to 9 — the picker calls it on each
// dialog open so every pick starts deterministic; scoped editor controls
// keep their prefilled level. Large value grids (the picker's, hundreds of
// cells) repaint on a debounce so a drag does not rebuild them per pixel;
// flushLevelSlider() settles a pending repaint for code that reads the cells.
(function () {
    'use strict';

    var controls = [];
    // Value cells are the expensive half of a level change: an unscoped
    // control (the picker dialogs) repaints every [data-values] cell on the
    // page, which in the treasure picker is one per effect of the whole
    // server-rendered catalog. A drag fires `input` per pixel, so the cell
    // repaint is debounced to the last level of the burst while the knob,
    // ticks, fill and hidden field keep updating immediately. flushLevelCells
    // settles a pending repaint for callers that read the cells back.
    var CELL_DELAY = 120;
    // below this many cells the repaint is not worth deferring: the treasure
    // detail page and the build editor's slot controls stay instant, only the
    // picker-sized grids (hundreds of cells) debounce.
    var CELL_DEBOUNCE_MIN = 64;
    var pending = [];

    function flushLevelCells() {
        while (pending.length > 0) {
            pending.shift()();
        }
    }

    document.querySelectorAll('.level-slider-control').forEach(function (control) {
        var slider = control.querySelector('.level-slider');
        var input = control.querySelector('.level-slider-input');
        if (!slider || !input) {
            return;
        }
        // unscoped controls (treasure detail page, planner picker) never
        // bind a hidden form field: falling back to document here would find
        // the first [data-level-field] on the page (e.g. the t1 slot input)
        // and clobber it on every slider change.
        var scoped = control.closest('[data-level-scope]');
        var scope = scoped || document;
        var field = scoped ? scoped.querySelector('[data-level-field]') : null;
        var ticks = control.querySelectorAll('.level-slider-ticks [data-tick]');
        var raw = scope.getAttribute ? scope.getAttribute('data-level-default') : null;
        var current = (raw === null || raw === '') ? 9 : parseInt(raw, 10);
        if (isNaN(current) || current < 0 || current > 9) {
            current = 9;
        }

        var cellTimer = null;
        var cellPending = null;
        // cells are server-rendered and never added after load, so the count
        // that decides debounce-or-not is measured once
        var cellCount = scope.querySelectorAll('[data-values]').length +
            scope.querySelectorAll('[data-diffs]').length;

        function paintCells(l) {
            scope.querySelectorAll('[data-values]').forEach(function (el) {
                var vals = (el.getAttribute('data-values') || '').split('|');
                el.textContent = vals[l] || '';
            });
            scope.querySelectorAll('[data-diffs]').forEach(function (el) {
                var diffs = (el.getAttribute('data-diffs') || '').split('|');
                el.textContent = diffs[l] || '';
            });
        }

        // flushCells is queued in `pending` while a repaint is scheduled, so
        // flushLevelCells() can run it early; it is a no-op once it has.
        function flushCells() {
            if (cellTimer !== null) {
                clearTimeout(cellTimer);
                cellTimer = null;
            }
            if (cellPending === null) {
                return;
            }
            var l = cellPending;
            cellPending = null;
            var at = pending.indexOf(flushCells);
            if (at !== -1) {
                pending.splice(at, 1);
            }
            paintCells(l);
        }

        // immediate paints the cells right away: the one-shot entry points
        // (dialog open, prefill, a slot's stored level) have nothing to
        // coalesce and their caller may read the cells back at once.
        function setLevel(l, immediate) {
            if (immediate || cellCount <= CELL_DEBOUNCE_MIN) {
                cellPending = null;
                if (cellTimer !== null) {
                    clearTimeout(cellTimer);
                    cellTimer = null;
                }
                var at = pending.indexOf(flushCells);
                if (at !== -1) {
                    pending.splice(at, 1);
                }
                paintCells(l);
            } else {
                cellPending = l;
                if (pending.indexOf(flushCells) === -1) {
                    pending.push(flushCells);
                }
                if (cellTimer !== null) {
                    clearTimeout(cellTimer);
                }
                cellTimer = setTimeout(flushCells, CELL_DELAY);
            }
            ticks.forEach(function (t) {
                t.classList.toggle('bg-primary', parseInt(t.getAttribute('data-tick'), 10) <= l);
            });
            slider.style.setProperty('--fill', (l / 9 * 100) + '%');
            if (field) {
                field.value = l;
            }
        }

        slider.value = current;
        input.value = current;
        slider.addEventListener('input', function () {
            input.value = slider.value;
            setLevel(parseInt(slider.value, 10));
        });
        input.addEventListener('input', function () {
            var v = parseInt(input.value, 10);
            if (!isNaN(v) && v >= 0 && v <= 9) {
                slider.value = v;
                setLevel(v);
            }
        });
        // pointer release / keyboard commit ends the drag: no reason to sit
        // out the debounce window.
        slider.addEventListener('change', function () {
            flushLevelCells();
        });
        input.addEventListener('blur', function () {
            var v = parseInt(input.value, 10);
            if (isNaN(v) || v < 0 || v > 9) {
                input.value = slider.value;
            }
        });
        // the cells are server-rendered at the default level; re-applying it
        // paints the fill/ticks and writes the form field.
        setLevel(current, true);

        // setControl also moves the slider/input knobs — setLevel alone only
        // repaints cells/ticks/fill (the input handlers already positioned
        // the knob). The picker's per-open reset goes through setControl so
        // the knob lands on the default alongside everything else.
        function setControl(l) {
            slider.value = l;
            input.value = l;
            setLevel(l, true);
        }

        controls.push({
            set: setControl,
            scope: scoped || null,
            scoped: !!control.closest('[data-level-scope]')
        });
    });

    // flushLevelSlider settles every pending cell repaint. The picker calls it
    // before reading a card's values (a pick) or applying a per-card override.
    window.flushLevelSlider = flushLevelCells;

    window.resetLevelSlider = function () {
        controls.forEach(function (c) {
            if (!c.scoped) {
                c.set(9);
            }
        });
    };

    // setPickerLevel applies a level to every unscoped control (the picker
    // dialogs), so callers can open the picker at a remembered slot level
    // instead of always the 9 default.
    window.setPickerLevel = function (l) {
        var v = parseInt(l, 10);
        if (isNaN(v) || v < 0 || v > 9) {
            v = 9;
        }
        controls.forEach(function (c) {
            if (!c.scoped) {
                c.set(v);
            }
        });
    };

    // getScopeLevel reads the current level of a scoped slot control (its
    // slider value, which the hidden form field mirrors). Returns 9 when the
    // scope has no control or holds an out-of-range value.
    window.getScopeLevel = function (scopeEl) {
        if (!scopeEl) {
            return 9;
        }
        var slider = scopeEl.querySelector('.level-slider');
        var v = slider ? parseInt(slider.value, 10) : NaN;
        if (isNaN(v) || v < 0 || v > 9) {
            return 9;
        }
        return v;
    };

    // setScopeLevel applies a level to the scoped control inside a given
    // scope: knob + cells + ticks + fill + the hidden form field. The picker
    // uses it after a pick so the slot's stored level matches the level the
    // picked card displayed (per-card stepper or top-slider default). If the
    // scope holds no control (defensive), the field is still written.
    window.setScopeLevel = function (scopeEl, l) {
        var v = parseInt(l, 10);
        if (isNaN(v) || v < 0 || v > 9) {
            v = 9;
        }
        if (!scopeEl) {
            return;
        }
        var matched = false;
        controls.forEach(function (c) {
            if (c.scope && (c.scope === scopeEl || scopeEl.contains(c.scope))) {
                c.set(v);
                matched = true;
            }
        });
        if (!matched) {
            var field = scopeEl.querySelector('[data-level-field]');
            if (field) {
                field.value = v;
            }
        }
    };
})();
