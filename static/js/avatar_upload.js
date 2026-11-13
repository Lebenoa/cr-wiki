// Avatar upload: posts the chosen file to /settings/avatar, which forwards
// it to the image host and stores the returned vanity on the account. The
// page reloads on success so the navbar and profile pick the new image up
// from the refreshed session snapshots.
//
// One upload at a time: a second file selection while one is in flight is
// ignored (the input is disabled too, so the pick cannot even start) — the
// news thumbnail uploader solves the same race with a token, but here the
// input disable is enough because there is exactly one input.
(function() {
  'use strict';

  var input = document.querySelector('[data-avatar-upload]');
  if (!input) return;
  var statusEl = document.querySelector('[data-avatar-status]');
  // server-inserted strings (the translated base text) for the states the
  // JS itself drives — read once, before any status write clobbers them
  var TEXT = {
    uploading: statusEl ? statusEl.getAttribute('data-text-uploading') || '…' : '…',
    failed: statusEl ? statusEl.getAttribute('data-text-failed') || 'upload failed' : 'upload failed',
  };

  function status(message, isError) {
    if (!statusEl) return;
    statusEl.textContent = message;
    statusEl.classList.toggle('text-error', !!isError);
    statusEl.classList.toggle('text-foreground-muted', !isError);
  }

  input.addEventListener('change', async function() {
    if (input.disabled) return;
    if (!input.files || !input.files[0]) return;
    input.disabled = true;
    status(TEXT.uploading, false);
    var body = new FormData();
    body.append('image', input.files[0]);
    try {
      var res = await fetch('/settings/avatar', { method: 'POST', body: body });
      var data = await res.json().catch(function() { return {}; });
      if (!res.ok || !data.image) {
        // a bare status code announces nothing useful; word it
        status(data.error ? data.error + '' : TEXT.failed, true);
        input.disabled = false;
        input.value = '';
        return;
      }
      status('', false);
      window.location.reload();
    } catch (err) {
      status(TEXT.failed, true);
      input.disabled = false;
      input.value = '';
    }
  });
})();
