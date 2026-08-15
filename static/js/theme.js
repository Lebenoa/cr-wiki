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
  var NAMES = ['default', 'light', 'tokyo_night', 'cappuccino', 'dracula', 'nord', 'gruvbox', 'rose_pine'];

  function stored() {
    try {
      return localStorage.getItem(KEY);
    } catch (e) {
      return null;
    }
  }
  var active = NAMES.indexOf(stored()) >= 0 ? stored() : 'default';

  // 'default' is the base `html {}` block in the preflight — no attribute
  function apply(name) {
    var root = document.documentElement;
    if (name === 'default') {
      delete root.dataset.theme;
    } else {
      root.dataset.theme = name;
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
  document.addEventListener('htmx:afterSwap', onReady);

  window.CRTheme = {
    apply: apply,
    save: save,
    active: function () { return active; },
  };
})();
