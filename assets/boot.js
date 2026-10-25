// Runs synchronously at the top of <body>: applies saved view settings before first paint (no flash).
(function () {
    var c = document.body.classList;
    try {
        if (localStorage.getItem('mdview.nightColors') === '1') c.add('night-mode');
        if (localStorage.getItem('mdview.bgTheme') === 'black') c.add('theme-black');
        if (localStorage.getItem('mdview.sidebarCollapsed') === '1') c.add('sidebar-collapsed');
    } catch (e) {}
})();
