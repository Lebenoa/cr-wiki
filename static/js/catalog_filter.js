// Shared list filter for every catalog page: hides the cards whose name does
// not match the search box, and (where the page shows grade buttons) whose
// grade is not the selected one. Cards opt in with [data-cr-card] plus
// data-name / data-name-en / data-grade; a page without a #filter-name input
// simply has nothing to do.
//
// This used to be copy-pasted into cookies.html, pets.html and treasures.html,
// which is why only those three pages could filter at all.
(function () {
    var GRADE_OFF = 'size-9 rounded-lg border-2 border-transparent transition-all';
    var GRADE_ON = 'size-9 rounded-lg border-2 border-primary ring-2 ring-primary transition-all';

    function crFilter() {
        var input = document.getElementById('filter-name');
        if (!input) return;
        var q = (input.value || '').toLowerCase();
        var g = (document.querySelector('.grade-filter.active') || {}).dataset;
        var grade = (g && g.grade) || '';
        document.querySelectorAll('[data-cr-card]').forEach(function (card) {
            // match the localized name or the English one, so e.g. a th page
            // still finds "Wizard" -> คุกกี้พ่อมด
            var names = ((card.dataset.name || '') + ' ' + (card.dataset.nameEn || '')).toLowerCase();
            var show = (!q || names.indexOf(q) !== -1) && (!grade || card.dataset.grade === grade);
            card.style.display = show ? '' : 'none';
        });
    }

    document.addEventListener('click', function (e) {
        var btn = e.target.closest ? e.target.closest('.grade-filter') : null;
        if (!btn) return;
        var wasActive = btn.classList.contains('active');
        document.querySelectorAll('.grade-filter').forEach(function (b) {
            b.classList.remove('active');
            var img = b.querySelector('img');
            if (img) img.className = GRADE_OFF;
        });
        if (!wasActive) {
            btn.classList.add('active');
            var img = btn.querySelector('img');
            if (img) img.className = GRADE_ON;
        }
        crFilter();
    });

    document.addEventListener('input', function (e) {
        if (e.target.id === 'filter-name') crFilter();
    });
    // htmx 4 swaps don't fire DOMNodeInserted; re-filter after each swap so
    // infinite-scroll pages keep the active filter applied
    document.addEventListener('htmx:after:settle', crFilter);
    // gacha_tabs.js re-applies the filter to a panel it just revealed
    window.crFilter = crFilter;
})();
