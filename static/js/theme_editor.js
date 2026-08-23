// Custom-theme editor. The presets in uno.config.ts are read-only; this edits
// the single `custom` theme, which is a chosen preset plus per-token
// overrides (storage and application live in theme.js, behind window.CRTheme).
//
// The theme variables hold bare OKLCH components ("L C H") because the colour
// config wraps them as oklch(var(--token)). <input type="color"> speaks hex,
// so the two conversions below bridge them. sRGB <-> OKLab is the standard
// matrix pair; nothing here is approximate except the gamut clamp on the way
// back, which only matters for colours a screen cannot show anyway.
(function () {
  'use strict';

  function srgbToLinear(c) {
    return c <= 0.04045 ? c / 12.92 : Math.pow((c + 0.055) / 1.055, 2.4);
  }

  function linearToSrgb(c) {
    return c <= 0.0031308 ? c * 12.92 : 1.055 * Math.pow(c, 1 / 2.4) - 0.055;
  }

  function hexToOklch(hex) {
    var m = /^#?([0-9a-f]{6})$/i.exec(String(hex).trim());
    if (!m) {
      return null;
    }
    var n = parseInt(m[1], 16);
    var r = srgbToLinear(((n >> 16) & 255) / 255);
    var g = srgbToLinear(((n >> 8) & 255) / 255);
    var b = srgbToLinear((n & 255) / 255);
    var l = Math.cbrt(0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b);
    var m2 = Math.cbrt(0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b);
    var s = Math.cbrt(0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b);
    var L = 0.2104542553 * l + 0.7936177850 * m2 - 0.0040720468 * s;
    var A = 1.9779984951 * l - 2.4285922050 * m2 + 0.4505937099 * s;
    var B = 0.0259040371 * l + 0.7827717662 * m2 - 0.8086757660 * s;
    var C = Math.sqrt(A * A + B * B);
    var H = C < 1e-6 ? 0 : (Math.atan2(B, A) * 180) / Math.PI;
    if (H < 0) {
      H += 360;
    }
    return L.toFixed(4) + ' ' + C.toFixed(4) + ' ' + H.toFixed(2);
  }

  function clamp01(x) {
    return x < 0 ? 0 : (x > 1 ? 1 : x);
  }

  function oklchToHex(value) {
    var p = String(value).trim().split(/\s+/);
    if (p.length < 3) {
      return null;
    }
    var L = parseFloat(p[0]), C = parseFloat(p[1]), H = parseFloat(p[2]);
    if (isNaN(L) || isNaN(C) || isNaN(H)) {
      return null;
    }
    var h = (H * Math.PI) / 180;
    var A = C * Math.cos(h), B = C * Math.sin(h);
    var l = Math.pow(L + 0.3963377774 * A + 0.2158037573 * B, 3);
    var m = Math.pow(L - 0.1055613458 * A - 0.0638541728 * B, 3);
    var s = Math.pow(L - 0.0894841775 * A - 1.2914855480 * B, 3);
    var r = linearToSrgb(clamp01(4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s));
    var g = linearToSrgb(clamp01(-1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s));
    var b = linearToSrgb(clamp01(-0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s));
    function hx(x) {
      var v = Math.round(clamp01(x) * 255).toString(16);
      return v.length === 1 ? '0' + v : v;
    }
    return '#' + hx(r) + hx(g) + hx(b);
  }

  // baseValues reads what each token resolves to under a preset, so a row
  // with no override still shows the colour it is inheriting. The default
  // preset is the bare `html {}` block, which a detached probe cannot inherit
  // — so the root itself is switched to the base, read, and switched straight
  // back. It is one synchronous task with no paint in the middle, so nothing
  // flickers; the overrides come off first or they would mask the base.
  function baseValues(base) {
    var root = document.documentElement;
    var tokens = CRTheme.tokens();
    var prevAttr = root.getAttribute('data-theme');
    var prevInline = {};
    tokens.forEach(function (t) {
      prevInline[t] = root.style.getPropertyValue('--' + t);
      root.style.removeProperty('--' + t);
    });
    if (base === 'default') {
      root.removeAttribute('data-theme');
    } else {
      root.setAttribute('data-theme', base);
    }
    var cs = getComputedStyle(root);
    var out = {};
    tokens.forEach(function (t) {
      out[t] = cs.getPropertyValue('--' + t).trim();
    });
    if (prevAttr === null) {
      root.removeAttribute('data-theme');
    } else {
      root.setAttribute('data-theme', prevAttr);
    }
    tokens.forEach(function (t) {
      if (prevInline[t]) {
        root.style.setProperty('--' + t, prevInline[t]);
      }
    });
    return out;
  }

  function dialog() { return document.getElementById('theme-editor'); }

  // render fills the rows from storage: each swatch shows the override when
  // there is one, otherwise the inherited colour, and the reset button only
  // appears for rows that actually override something.
  function render() {
    var dlg = dialog();
    if (!dlg) {
      return;
    }
    var c = CRTheme.custom();
    var inherited = baseValues(c.base);
    var sel = dlg.querySelector('[data-theme-base]');
    if (sel) {
      sel.value = c.base;
    }
    dlg.querySelectorAll('[data-token-row]').forEach(function (row) {
      var token = row.dataset.tokenRow;
      var value = c.vars[token] || inherited[token];
      var input = row.querySelector('input[type="color"]');
      var hex = oklchToHex(value);
      if (input && hex) {
        input.value = hex;
      }
      var reset = row.querySelector('[data-token-reset]');
      if (reset) {
        reset.classList.toggle('invisible', !c.vars[token]);
      }
      row.classList.toggle('border-primary', !!c.vars[token]);
      row.classList.toggle('border-secondary/40', !c.vars[token]);
    });
  }

  document.addEventListener('input', function (e) {
    var row = e.target && e.target.closest ? e.target.closest('[data-token-row]') : null;
    if (!row || e.target.type !== 'color') {
      return;
    }
    var oklch = hexToOklch(e.target.value);
    if (!oklch) {
      return;
    }
    CRTheme.setToken(row.dataset.tokenRow, oklch);
    // editing implies wanting to see it: switch to the custom theme on the
    // first change rather than making the user pick it afterwards
    if (CRTheme.active() !== 'custom') {
      CRTheme.save('custom');
    }
    render();
  });

  document.addEventListener('change', function (e) {
    if (!e.target || !e.target.matches || !e.target.matches('[data-theme-base]')) {
      return;
    }
    CRTheme.setBase(e.target.value);
    if (CRTheme.active() !== 'custom') {
      CRTheme.save('custom');
    }
    render();
  });

  document.addEventListener('click', function (e) {
    var el = e.target;
    if (!el || !el.closest) {
      return;
    }
    if (el.closest('[data-theme-edit]')) {
      e.preventDefault();
      var dlg = dialog();
      if (dlg) {
        render();
        dlg.showModal();
      }
      return;
    }
    var reset = el.closest('[data-token-reset]');
    if (reset) {
      e.preventDefault();
      var row = reset.closest('[data-token-row]');
      if (row) {
        CRTheme.setToken(row.dataset.tokenRow, null);
        render();
      }
      return;
    }
    if (el.closest('[data-theme-reset-all]')) {
      e.preventDefault();
      CRTheme.resetCustom();
      render();
    }
  });
})();
