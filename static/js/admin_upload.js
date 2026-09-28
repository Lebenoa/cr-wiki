// Admin image upload: posts the chosen file to the section's upload endpoint
// and drops the stored filename into the form's image field, so the form
// still submits a plain name and the page never reloads mid-edit.
// The #imagePreview thumbnail mirrors the field: live FileReader preview as
// soon as a file is chosen, swapped to the stored sprite once the upload
// lands, and updated again when the filename is typed or cleared. A filename
// that does not resolve to a file flips the tile to the #imageMissing
// placeholder instead of a broken image.
(function () {
  'use strict';

  function showPreview(src) {
    var img = document.getElementById('imagePreview');
    var missing = document.getElementById('imageMissing');
    if (!img || !missing) return;
    if (!src) {
      img.removeAttribute('src');
      img.classList.add('hidden');
      missing.classList.remove('hidden');
      return;
    }
    // a src that fails to load (typed a name with no file behind it) must
    // land on the placeholder, not the browser's broken-image glyph
    img.onerror = function () {
      img.onerror = null;
      img.removeAttribute('src');
      img.classList.add('hidden');
      missing.classList.remove('hidden');
    };
    img.onload = function () {
      img.onload = null;
      img.classList.remove('hidden');
      missing.classList.add('hidden');
    };
    img.src = src;
  }

  function showStored(section, name) {
    var trimmed = (name || '').trim();
    showPreview(trimmed ? '/static/img/' + encodeURIComponent(section) + '/' +
      trimmed.split('/').map(encodeURIComponent).join('/') : null);
  }

  // One upload in flight at a time; each selection gets a token and only
  // the response of the LATEST selection may touch the form. A slower
  // earlier upload completing after a newer selection is discarded —
  // otherwise the stale filename/thumbnail overwrites the newer choice.
  var uploadToken = 0;

  document.addEventListener('change', async function (e) {
    var input = e.target;
    if (!input || !input.dataset || !input.dataset.upload) {
      // a plain image field edit also refreshes the thumbnail
      if (input && input.name === 'image' && input.form) {
        var section = (input.form.getAttribute('action') || '').split('/')[1];
        showStored(section, input.value);
      }
      return;
    }
    if (!input.files || !input.files[0]) return;
    var field = input.closest('div').querySelector('input[name="image"]');
    var token = ++uploadToken;

    // instant local preview while the upload runs
    var reader = new FileReader();
    reader.onload = function (ev) {
      if (token !== uploadToken) return;
      showPreview(ev.target.result);
    };
    reader.readAsDataURL(input.files[0]);

    var body = new FormData();
    body.append('image', input.files[0]);
    try {
      var res = await fetch(input.dataset.upload, { method: 'POST', body: body });
      var data = await res.json();
      // a newer selection superseded this upload: discard, touch nothing
      if (token !== uploadToken) return;
      if (!res.ok || !data.image) {
        // leave the typed name alone: a failed upload must not clear it
        console.warn('upload failed', data.error || res.status);
        return;
      }
      if (field) {
        field.value = data.image;
      }
      var section = (input.dataset.upload || '').split('/')[1];
      showStored(section, data.image);
    } catch (err) {
      console.warn('upload failed', err);
    }
  });
})();
