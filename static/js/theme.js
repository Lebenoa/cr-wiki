// Theme selector. Palettes live in the UnoCSS preflight (uno.config.ts) as
// `html[data-theme="..."]` variable blocks — this script only toggles the
// `data-theme` attribute on <html> (the default theme has no attribute).
// The theme options live in a popover nested inside the account-dropdown so
// picking a theme never closes the account dropdown (clicks inside a nested
// popover don't light-dismiss its ancestor). The popover opens on click OR
// mouseover of the trigger button and is positioned to its left. Delegated
// listeners survive hx-boost body swaps (the navbar re-renders on every
// navigation).
(function () {
  'use strict';

  var KEY = 'cr-theme';
  var CUSTOM_KEY = 'cr-theme-custom';
  var PRESETS = ['default', 'light', 'tokyo_night', 'cappuccino', 'dracula', 'nord', 'gruvbox', 'rose_pine'];
  // the one editable theme. It is not a palette of its own: it names a preset
  // to inherit from and a set of per-token overrides on top, so the presets
  // themselves stay exactly as shipped.
  var CUSTOM = 'custom';
  var NAMES = PRESETS.concat([CUSTOM]);
  // the tokens the editor may override, in the order it lists them. The
  // remaining variables (the --on-* contrast pair of each) are derived, never
  // edited by hand: a readable foreground is not something to get wrong.
  var TOKENS = ['background', 'surface', 'border', 'primary', 'secondary', 'accent',
    'muted', 'foreground', 'foreground-muted', 'success', 'warning', 'error'];
  // tokens that carry an --on-<token> partner
  var ON_PAIRS = ['primary', 'secondary', 'accent', 'success', 'warning', 'error', 'surface', 'muted'];
  // the two contrast values every preset already uses
  var ON_LIGHT = '0.9612 0.0000 89.88';
  var ON_DARK = '0.2090 0.0000 89.88';

  function stored() {
    try {
      return localStorage.getItem(KEY);
    } catch (e) {
      return null;
    }
  }
  var active = NAMES.indexOf(stored()) >= 0 ? stored() : 'default';

  // custom = { base: <preset name>, vars: { <token>: "L C H" } }
  function readCustom() {
    var raw = null;
    try {
      raw = localStorage.getItem(CUSTOM_KEY);
    } catch (e) { /* private mode */ }
    var out = { base: 'default', vars: {} };
    if (!raw) {
      return out;
    }
    try {
      var parsed = JSON.parse(raw);
      if (parsed && PRESETS.indexOf(parsed.base) >= 0) {
        out.base = parsed.base;
      }
      if (parsed && parsed.vars && typeof parsed.vars === 'object') {
        TOKENS.forEach(function (t) {
          if (typeof parsed.vars[t] === 'string') {
            out.vars[t] = parsed.vars[t];
          }
        });
      }
    } catch (e) { /* corrupt entry: fall back to the empty custom */ }
    return out;
  }

  var custom = readCustom();

  function writeCustom() {
    try {
      localStorage.setItem(CUSTOM_KEY, JSON.stringify(custom));
    } catch (e) { /* private mode: this session only */ }
  }

  // clearOverrides drops every inline variable the custom theme wrote, so
  // switching back to a preset leaves the preflight block alone again.
  function clearOverrides() {
    var st = document.documentElement.style;
    TOKENS.forEach(function (t) {
      st.removeProperty('--' + t);
    });
    ON_PAIRS.forEach(function (t) {
      st.removeProperty('--on-' + t);
    });
  }

  // lightness of an "L C H" triple, 0..1
  function lightnessOf(v) {
    var l = parseFloat(String(v).trim().split(/\s+/)[0]);
    return isNaN(l) ? 0 : l;
  }

  function writeOverrides() {
    var st = document.documentElement.style;
    TOKENS.forEach(function (t) {
      var v = custom.vars[t];
      if (!v) {
        return;
      }
      st.setProperty('--' + t, v);
      if (ON_PAIRS.indexOf(t) >= 0) {
        // a light surface wants dark text on it and the other way round
        st.setProperty('--on-' + t, lightnessOf(v) > 0.62 ? ON_DARK : ON_LIGHT);
      }
    });
  }

  // 'default' is the base `html {}` block in the preflight — no attribute.
  // 'custom' wears its base preset's attribute and layers the overrides on as
  // inline variables, which beat the preflight block; that is the whole of the
  // inheritance, so changing the base keeps every override.
  function apply(name) {
    var root = document.documentElement;
    clearOverrides();
    var attr = name === CUSTOM ? custom.base : name;
    if (attr === 'default') {
      delete root.dataset.theme;
    } else {
      root.dataset.theme = attr;
    }
    if (name === CUSTOM) {
      root.dataset.custom = '1';
      writeOverrides();
    } else {
      delete root.dataset.custom;
    }
  }

  function save(name) {
    active = name;
    try {
      localStorage.setItem(KEY, name);
    } catch (e) { /* private mode: apply for the session only */ }
    apply(name);
    refresh();
  }

  // mark the active option in the dropdown and sync the trigger swatch
  function refresh() {
    document.querySelectorAll('.theme-option').forEach(function (btn) {
      var check = btn.querySelector('.theme-check');
      if (check) check.classList.toggle('hidden', btn.dataset.theme !== active);
    });
    var trig = document.getElementById('theme-trigger');
    var sw = trig && trig.querySelector('.theme-swatch');
    if (sw) sw.dataset.theme = active;
  }

  function trig() { return document.getElementById('theme-trigger'); }
  function pop() { return document.getElementById('theme-popover'); }

  function openPopover() {
    var t = trig(), p = pop();
    if (!t || !p || p.matches(':popover-open')) return;
    p.showPopover();
  }

  function closePopover() {
    var p = pop();
    if (p && p.matches(':popover-open')) p.hidePopover();
  }

  // click on the trigger OR mouseover over it — mouseover simulates a button
  // click so the popover toggles and stays open (sticky), same as clicking.
  // On touch devices a single tap fires a synthetic mouseover followed by a
  // real click, which would toggle twice (open then immediately close). The
  // click handler skips its toggle when a hover-toggle just happened, so one
  // tap = one toggle.
  var lastHoverToggle = 0;

  function togglePopover() {
    var p = pop();
    if (p && p.matches(':popover-open')) closePopover();
    else openPopover();
  }

  // boot: apply the saved theme before first paint (script runs in <head>)
  apply(active);

  // wire the trigger + options via delegation (survives hx-boost swaps)
  // mouseover fires on every child-element transition inside the button, so
  // only toggle on a genuine entry into the trigger (relatedTarget outside it)
  document.addEventListener('mouseover', function (e) {
    var el = e.target;
    if (!el || !el.closest) return;
    if (el.closest('#theme-trigger')) {
      var rel = e.relatedTarget;
      var fromInside = rel && rel.closest && rel.closest('#theme-trigger');
      if (!fromInside) {
        lastHoverToggle = Date.now();
        togglePopover();
      }
    }
  });
  document.addEventListener('click', function (e) {
    var el = e.target;
    var t = el && el.closest ? el.closest('#theme-trigger') : null;
    if (t) {
      e.preventDefault();
      // a tap = mouseover (already toggled) + click: skip the click's toggle
      if (Date.now() - lastHoverToggle < 400) return;
      togglePopover();
      return;
    }
    var opt = el && el.closest ? el.closest('.theme-option') : null;
    if (opt) {
      e.preventDefault();
      save(opt.dataset.theme);
      closePopover(); // account-dropdown stays open (nested popover)
    }
  });

  function onReady() {
    refresh();
  }
  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', onReady);
  } else {
    onReady();
  }
  // htmx 4 names its events colon-separated: 'htmx:afterSwap' matched
  // nothing, so after an hx-boost navigation swapped the navbar in, the
  // theme swatch and the option check mark kept the old markup's state.
  document.addEventListener('htmx:after:settle', onReady);

  // the editor (theme_editor.js) drives the custom theme through these: it
  // owns the colour picking and the oklch conversion, this file owns storage
  // and application.
  window.CRTheme = {
    apply: apply,
    save: save,
    active: function () { return active; },
    presets: function () { return PRESETS.slice(); },
    tokens: function () { return TOKENS.slice(); },
    custom: function () { return { base: custom.base, vars: Object.assign({}, custom.vars) }; },
    setBase: function (base) {
      if (PRESETS.indexOf(base) < 0) {
        return;
      }
      custom.base = base;
      writeCustom();
      if (active === CUSTOM) {
        apply(CUSTOM);
      }
    },
    // value null/'' clears the override, so the token falls back to the base
    setToken: function (token, value) {
      if (TOKENS.indexOf(token) < 0) {
        return;
      }
      if (value) {
        custom.vars[token] = value;
      } else {
        delete custom.vars[token];
      }
      writeCustom();
      if (active === CUSTOM) {
        apply(CUSTOM);
      }
    },
    resetCustom: function () {
      custom.vars = {};
      writeCustom();
      if (active === CUSTOM) {
        apply(CUSTOM);
      }
    },
  };
})();
