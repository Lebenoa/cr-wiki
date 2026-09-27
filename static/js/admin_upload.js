// Admin image upload: posts the chosen file to the section's upload endpoint
// and drops the stored filename into the form's image field, so the form
// still submits a plain name and the page never reloads mid-edit.
// The #imagePreview thumbnail mirrors the field: live FileReader preview as
// soon as a file is chosen, swapped to the stored sprite once the upload
// lands, and updated again when the filename is typed or cleared.
(function () {
  'use strict';

  function showPreview(src) {
    var img = document.getElementById('imagePreview');
    if (!img) return;
    if (src) {
      img.src = src;
      img.classList.remove('hidden');
    } else {
      img.removeAttribute('src');
      img.classList.add('hidden');
    }
  }

  function showStored(section, name) {
    var trimmed = (name || '').trim();
    showPreview(trimmed ? '/static/img/' + encodeURIComponent(section) + '/' +
      trimmed.split('/').map(encodeURIComponent).join('/') : null);
  }

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

    // instant local preview while the upload runs
    var reader = new FileReader();
    reader.onload = function (ev) { showPreview(ev.target.result); };
    reader.readAsDataURL(input.files[0]);

    var body = new FormData();
    body.append('image', input.files[0]);
    try {
      var res = await fetch(input.dataset.upload, { method: 'POST', body: body });
      var data = await res.json();
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
