// Theme selector. Palettes live in the UnoCSS preflight (uno.config.ts) as
// `html[data-theme="..."]` variable blocks — this script only toggles the
// `data-theme` attribute on <html> (the default theme has no attribute).
// Hovering an option in the account dropdown live-previews it; clicking
// persists it in localStorage. Delegated listeners survive hx-boost body
// swaps (the navbar/account dropdown re-renders on every navigation).
(function () {
  'use strict';

  var KEY = 'cr-theme';
  var NAMES = ['default', 'light', 'tokyo_night', 'cappuccino', 'dracula', 'nord', 'gruvbox'];

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

  // mark the active option in the dropdown
  function refresh() {
    document.querySelectorAll('.theme-option').forEach(function (btn) {
      var check = btn.querySelector('.theme-check');
      if (check) check.classList.toggle('hidden', btn.dataset.theme !== active);
    });
  }

  // boot: apply the saved theme before first paint (script runs in <head>)
  apply(active);

  // wire the dropdown via delegation (survives hx-boost swaps)
  var hovered = null;
  document.addEventListener('mouseover', function (e) {
    var btn = e.target && e.target.closest ? e.target.closest('.theme-option') : null;
    if (!btn || btn === hovered) return;
    hovered = btn;
    apply(btn.dataset.theme);
    refresh();
  });
  document.addEventListener('mouseout', function (e) {
    if (!hovered) return;
    var rel = e.relatedTarget;
    if (rel && rel.closest && rel.closest('.theme-option')) return; // moved to another option
    hovered = null;
    apply(active);
    refresh();
  });
  document.addEventListener('click', function (e) {
    var btn = e.target && e.target.closest ? e.target.closest('.theme-option') : null;
    if (!btn) return;
    e.preventDefault();
    save(btn.dataset.theme);
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
