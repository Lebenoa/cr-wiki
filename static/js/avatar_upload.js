// Avatar upload: posts the chosen file to /settings/avatar, which forwards
// it to the image host and stores the returned vanity on the account. The
// page reloads on success so the navbar and profile pick the new image up
// from the refreshed session snapshots.
(function() {
  'use strict';

  var input = document.querySelector('[data-avatar-upload]');
  if (!input) return;
  var statusEl = document.querySelector('[data-avatar-status]');

  function status(message) {
    if (statusEl) statusEl.textContent = message;
  }

  input.addEventListener('change', async function() {
    if (!input.files || !input.files[0]) return;
    status('…');
    var body = new FormData();
    body.append('image', input.files[0]);
    try {
      var res = await fetch('/settings/avatar', { method: 'POST', body: body });
      var data = await res.json().catch(function() { return {}; });
      if (!res.ok || !data.image) {
        status(data.error || res.status);
        return;
      }
      status('');
      window.location.reload();
    } catch (err) {
      status('upload failed');
    }
  });
})();
