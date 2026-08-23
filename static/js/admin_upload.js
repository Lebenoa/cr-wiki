// Admin image upload: posts the chosen file to the section's upload endpoint
// and drops the stored filename into the form's image field, so the form
// still submits a plain name and the page never reloads mid-edit.
(function () {
  'use strict';

  document.addEventListener('change', async function (e) {
    var input = e.target;
    if (!input || !input.dataset || !input.dataset.upload || !input.files || !input.files[0]) {
      return;
    }
    var field = input.closest('div').querySelector('input[name="image"]');
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
    } catch (err) {
      console.warn('upload failed', err);
    }
  });
})();
