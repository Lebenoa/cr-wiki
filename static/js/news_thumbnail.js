// News thumbnail upload: posts the chosen file to the news thumbnail
// endpoint, which forwards it to the image host and answers the stored
// vanity. The vanity drops into the form's `thumbnail` text field, keeping
// the field's "bare name" semantics — the detail page resolves it against
// the host.
(function() {
  'use strict';

  var uploadToken = 0;

  document.addEventListener('change', async function(e) {
    var input = e.target;
    if (!input || !input.matches || !input.matches('[data-thumbnail-upload]')) return;
    if (!input.files || !input.files[0]) return;
    var token = ++uploadToken;
    var form = input.closest('form');
    var field = form ? form.querySelector('input[name="thumbnail"]') : null;
    var notice = form ? form.querySelector('[data-thumbnail-status]') : null;

    function status(message, isError) {
      if (!notice) return;
      notice.textContent = message;
      notice.classList.toggle('text-error', !!isError);
      notice.classList.toggle('text-foreground-muted', !isError);
    }

    status('…', false);
    var body = new FormData();
    body.append('image', input.files[0]);
    try {
      var res = await fetch('/admin/news/upload-thumbnail', { method: 'POST', body: body });
      var data = await res.json().catch(function() { return {}; });
      // a newer selection superseded this upload: touch nothing
      if (token !== uploadToken) return;
      if (!res.ok || !data.image) {
        status(data.error || 'upload failed', true);
        return;
      }
      if (field) {
        field.value = data.image;
      }
      status('', false);
    } catch (err) {
      if (token === uploadToken) status('upload failed', true);
    }
  });
})();
