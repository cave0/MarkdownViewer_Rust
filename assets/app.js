// mdview page runtime. External file (CSP: script-src 'self', no inline script).
(function () {
    'use strict';
    var body = document.body;
    var $ = function (id) { return document.getElementById(id); };
    var store = {
        get: function (k) { try { return localStorage.getItem(k); } catch (e) { return null; } },
        set: function (k, v) { try { localStorage.setItem(k, v); } catch (e) {} }
    };
    function ipc(msg) { try { window.ipc.postMessage(msg); } catch (e) {} }
    function onKey(test, fn) {
        document.addEventListener('keydown', function (e) { if (test(e)) { e.preventDefault(); fn(e); } });
    }
    var ctrlShift = function (k) { return function (e) { return e.ctrlKey && e.shiftKey && !e.altKey && e.key.toLowerCase() === k; }; };

    // ---- clipboard ------------------------------------------------------------------------------
    function copyText(text, cb) {
        function fallback() {
            try {
                var t = document.createElement('textarea');
                t.value = text; t.style.cssText = 'position:fixed;left:-9999px;top:0';
                body.appendChild(t); t.select();
                var ok = document.execCommand('copy');
                body.removeChild(t); cb(ok);
            } catch (err) { cb(false); }
        }
        if (navigator.clipboard && navigator.clipboard.writeText) {
            navigator.clipboard.writeText(text).then(function () { cb(true); }, fallback);
        } else fallback();
    }
    function flash(btn, ok, label) {
        btn.textContent = ok ? 'Copied!' : 'Failed';
        setTimeout(function () { btn.textContent = label; }, 1600);
    }
    document.addEventListener('click', function (e) {
        var btn = e.target.closest && e.target.closest('.code-copy-btn');
        if (!btn) return;
        var code = btn.closest('.code-block-container').querySelector('pre code');
        copyText(code ? code.innerText : '', function (ok) { flash(btn, ok, 'Copy'); });
    });

    // ---- toolbar: night colors / background / fullscreen / print / back / source -----------
    function setNight(on) {
        body.classList.toggle('night-mode', on);
        var b = $('theme-toggle-btn');
        if (b) {
            b.textContent = on ? '\u2600' : '\u263D';
            b.title = (on ? 'Standard colors' : 'Night colors') + ' (Ctrl+Shift+N)';
        }
        store.set('mdview.nightColors', on ? '1' : '0');
    }
    setNight(body.classList.contains('night-mode'));
    function setBlack(on) {
        body.classList.toggle('theme-black', on);
        var b = $('bgtheme-toggle-btn');
        if (b) b.title = (on ? 'Pure black' : 'Standard dark') + ' background - click to switch (Ctrl+Shift+K)';
        store.set('mdview.bgTheme', on ? 'black' : 'dark');
    }
    setBlack(body.classList.contains('theme-black'));
    function on(id, fn) { var b = $(id); if (b) b.addEventListener('click', fn); }
    on('theme-toggle-btn', function () { setNight(!body.classList.contains('night-mode')); });
    on('bgtheme-toggle-btn', function () { setBlack(!body.classList.contains('theme-black')); });
    on('fullscreen-toggle-btn', function () { ipc('fullscreen'); });
    on('print-toggle-btn', function () { window.print(); });
    on('back-toggle-btn', function () { history.back(); });
    on('source-toggle-btn', function () {
        var s = !body.classList.contains('view-source');
        body.classList.toggle('view-source', s);
        $('source-toggle-btn').title = s ? 'Show rendered view' : 'View raw Markdown source';
    });
    onKey(ctrlShift('n'), function () { setNight(!body.classList.contains('night-mode')); });
    onKey(ctrlShift('k'), function () { setBlack(!body.classList.contains('theme-black')); });
    onKey(function (e) { return e.key === 'F11'; }, function () { ipc('fullscreen'); });
    onKey(function (e) { return e.ctrlKey && !e.shiftKey && !e.altKey && e.key.toLowerCase() === 'o'; }, function () { ipc('open'); });

    // ---- sidebar ---------------------------------------------------------------------------
    var host = body.classList.contains('has-sidebar') ? document.querySelector('.mdview-content') : null;
    var scroller = host || window;
    function setCollapsed(c) {
        body.classList.toggle('sidebar-collapsed', c);
        store.set('mdview.sidebarCollapsed', c ? '1' : '0');
    }
    function toggleSidebar() { setCollapsed(!body.classList.contains('sidebar-collapsed')); }
    on('sidebar-collapse-btn', toggleSidebar);
    on('sidebar-reopen-btn', toggleSidebar);
    onKey(function (e) { return e.ctrlKey && !e.shiftKey && !e.altKey && e.key.toLowerCase() === 'b'; }, toggleSidebar);

    // Keyboard scrolling (PgUp/PgDn/arrows/Space/Home/End) targets the focused pane.
    function focusContent() { if (host) host.focus({ preventScroll: true }); }
    focusContent();
    var sb = $('mdview-sidebar');
    if (sb) sb.addEventListener('click', function (e) { if (e.target.closest('a')) setTimeout(focusContent, 0); });

    // Scroll-spy: headings are in document order, so binary-search the last one above the line.
    var entries = [];
    document.querySelectorAll('#mdview-sidebar a[data-target]').forEach(function (a) {
        var el = document.getElementById(a.getAttribute('data-target'));
        if (el) entries.push({ el: el, link: a });
    });
    var current = -1, ticking = false;
    function updateActive() {
        ticking = false;
        if (!entries.length || entries[0].el.getClientRects().length === 0) return;
        var lo = 0, hi = entries.length - 1, best = 0;
        while (lo <= hi) {
            var mid = (lo + hi) >> 1;
            if (entries[mid].el.getBoundingClientRect().top - 88 <= 0) { best = mid; lo = mid + 1; } else hi = mid - 1;
        }
        if (best === current) return;
        if (current >= 0) entries[current].link.classList.remove('active');
        current = best;
        entries[current].link.classList.add('active');
        entries[current].link.scrollIntoView({ block: 'nearest' });
    }
    if (entries.length) {
        scroller.addEventListener('scroll', function () {
            if (!ticking) { ticking = true; requestAnimationFrame(updateActive); }
        }, { passive: true });
        window.addEventListener('resize', function () { requestAnimationFrame(updateActive); });
    }

    // ---- lazy rendering: only load/run heavy libraries for content that is actually near view -
    var scriptCache = {};
    function loadScript(src) {
        if (!scriptCache[src]) scriptCache[src] = new Promise(function (res, rej) {
            var s = document.createElement('script');
            s.src = src; s.onload = res; s.onerror = rej; document.head.appendChild(s);
        });
        return scriptCache[src];
    }
    var pending = [];   // functions that force-render everything still queued (used for printing)
    function lazy(selector, src, init, render) {
        var els = Array.prototype.slice.call(document.querySelectorAll(selector));
        if (!els.length) return;
        loadScript(src).then(function () {
            if (init) init();
            var todo = new Set(els);
            function run(el) { if (todo.delete(el)) { try { render(el); } catch (e) {} } }
            pending.push(function () { els.forEach(run); });
            if (!('IntersectionObserver' in window)) { els.forEach(run); return; }
            var io = new IntersectionObserver(function (list) {
                list.forEach(function (en) {
                    if (en.isIntersecting) { io.unobserve(en.target); run(en.target); }
                });
            }, { root: host, rootMargin: '1200px 0px' });
            els.forEach(function (el) { io.observe(el); });
        }, function () {});
    }

    lazy('pre code[class*="language-"]', '/_a/hljs.min.js', null, function (code) {
        var m = /language-([\w+#.-]+)/.exec(code.className);
        if (!m || code.textContent.length > 100000 || !window.hljs || !hljs.getLanguage(m[1])) return;
        hljs.highlightElement(code);
    });
    lazy('.math-inline, .math-display', '/_a/katex/katex.min.js', null, function (el) {
        var tex = el.textContent
            .replace(/\\(begin|end)\{eqnarray(\*?)\}/g, '\\$1{align$2}')
            .replace(/\\(begin|end)\{multline(\*?)\}/g, '\\$1{gather$2}');
        katex.render(tex, el, {
            displayMode: el.classList.contains('math-display'), throwOnError: false, trust: false, strict: 'ignore', output: 'htmlAndMathml'
        });
    });
    var mermaidQueue = Promise.resolve();
    lazy('pre.mermaid', '/_a/mermaid.min.js', function () {
        mermaid.initialize({ startOnLoad: false, theme: 'dark', securityLevel: 'strict', fontFamily: 'Segoe UI, Arial, sans-serif' });
    }, function (el) {
        // One render at a time: mermaid.run() names each svg "mermaid-<Date.now()>", so two runs started in the
        // same millisecond (several diagrams entering view together) share an id and draw into each other.
        mermaidQueue = mermaidQueue.then(function () {
            return mermaid.run({ nodes: [el], suppressErrors: true });
        }).catch(function () {});
    });
    window.addEventListener('beforeprint', function () { pending.forEach(function (f) { f(); }); });

    // ---- conversation turns (speaker headings / "**You:**" labels found by the renderer) ----------
    var turnSrcEl = $('md-turns');
    var turnSrc = turnSrcEl ? turnSrcEl.value : null;
    var markers = Array.prototype.slice.call(document.querySelectorAll('.markdown-container > [data-role]'));
    var turns = [];
    markers.forEach(function (m, i) {
        var sec = document.createElement('section');
        sec.className = 'turn turn-' + m.getAttribute('data-role');
        sec.setAttribute('data-role', m.getAttribute('data-role'));
        sec.setAttribute('data-o', m.getAttribute('data-o'));
        m.parentNode.insertBefore(sec, m);
        var n = m, stop = markers[i + 1];
        while (n && n !== stop) { var nx = n.nextSibling; sec.appendChild(n); n = nx; }
        var tools = document.createElement('div');
        tools.className = 'turn-tools';
        var fold = document.createElement('button');
        fold.type = 'button'; fold.className = 'turn-fold'; fold.title = 'Fold / unfold this turn'; fold.textContent = '\u25BE';
        tools.appendChild(fold);
        if (turnSrc !== null) {
            var cp = document.createElement('button');
            cp.type = 'button'; cp.className = 'turn-copy'; cp.title = 'Copy this turn as Markdown'; cp.textContent = 'Copy';
            tools.appendChild(cp);
        }
        sec.insertBefore(tools, sec.firstChild);
        turns.push(sec);
    });
    function setFolded(sec, folded) {
        if (folded) {
            var first = Array.prototype.filter.call(sec.children, function (c) {
                return !c.classList.contains('turn-tools') && !c.hasAttribute('data-role');
            })[0];
            var t = (first ? first.textContent : '').replace(/\s+/g, ' ').trim();
            sec.setAttribute('data-preview', t.length > 140 ? t.slice(0, 140) + '\u2026' : t);
        }
        sec.classList.toggle('folded', folded);
        var b = sec.querySelector('.turn-fold');
        if (b) b.textContent = folded ? '\u25B8' : '\u25BE';
    }
    document.addEventListener('click', function (e) {
        var b = e.target.closest && e.target.closest('.turn-fold, .turn-copy');
        if (!b) return;
        var sec = b.closest('.turn');
        if (b.classList.contains('turn-fold')) { setFolded(sec, !sec.classList.contains('folded')); return; }
        var i = turns.indexOf(sec);
        var start = +sec.getAttribute('data-o');
        var end = i + 1 < turns.length ? +turns[i + 1].getAttribute('data-o') : turnSrc.length;
        copyText(turnSrc.slice(start, end).trim() + '\n', function (ok) { flash(b, ok, 'Copy'); });
    });
    on('turns-fold-all', function () { turns.forEach(function (t) { setFolded(t, true); }); });
    on('turns-unfold-all', function () { turns.forEach(function (t) { setFolded(t, false); }); });
    // opening a heading from the outline unfolds the turn it lives in
    var outline = $('mdview-sidebar');
    if (outline) outline.addEventListener('click', function (e) {
        var a = e.target.closest && e.target.closest('a[data-target]');
        var el = a && document.getElementById(a.getAttribute('data-target'));
        var sec = el && el.closest && el.closest('.turn');
        if (sec && sec.classList.contains('folded')) setFolded(sec, false);
    });
    // [ and ] jump between turns
    function typing(e) { var t = e.target; return t && (t.tagName === 'INPUT' || t.tagName === 'TEXTAREA' || t.isContentEditable); }
    function goTurn(dir) {
        if (!turns.length) return;
        var vis = turns.filter(function (t) { return t.getClientRects().length; });
        var target = null, i;
        if (dir > 0) {
            for (i = 0; i < vis.length; i++) if (vis[i].getBoundingClientRect().top > 24) { target = vis[i]; break; }
        } else {
            for (i = vis.length - 1; i >= 0; i--) if (vis[i].getBoundingClientRect().top < -8) { target = vis[i]; break; }
        }
        if (target) target.scrollIntoView({ block: 'start' });
    }
    document.addEventListener('keydown', function (e) {
        if (typing(e) || e.ctrlKey || e.altKey || e.metaKey) return;
        if (e.key === ']') { e.preventDefault(); goTurn(1); }
        else if (e.key === '[') { e.preventDefault(); goTurn(-1); }
    });

    // ---- outline filter ---------------------------------------------------------------------------
    var filt = $('outline-filter');
    if (filt) {
        var links = Array.prototype.slice.call(document.querySelectorAll('#mdview-sidebar a[data-target]'));
        filt.addEventListener('input', function () {
            var q = filt.value.trim().toLowerCase();
            links.forEach(function (a) { a.hidden = !!q && a.textContent.toLowerCase().indexOf(q) < 0; });
        });
        filt.addEventListener('keydown', function (e) {
            if (e.key === 'Escape') { filt.value = ''; filt.dispatchEvent(new Event('input')); filt.blur(); focusContent(); }
        });
        document.addEventListener('keydown', function (e) {
            if (e.key === '/' && !typing(e) && !e.ctrlKey && !e.altKey && !e.metaKey) {
                e.preventDefault();
                if (body.classList.contains('sidebar-collapsed')) setCollapsed(false);
                filt.focus(); filt.select();
            }
        });
    }

    // ---- quick open (Ctrl+E): every Markdown file in this folder and its subfolders ------------------
    var qo = null;
    function closeQuick() { if (qo) { qo.remove(); qo = null; focusContent(); } }
    function openQuick() {
        if (qo) { closeQuick(); return; }
        qo = document.createElement('div');
        qo.id = 'quick-open';
        qo.innerHTML = '<div class="qo-box"><input type="text" placeholder="Open a Markdown file in this folder\u2026" spellcheck="false" autocomplete="off"><ul></ul><div class="qo-hint">\u2191\u2193 select &middot; Enter open &middot; Esc close &middot; Ctrl+O browse\u2026</div></div>';
        body.appendChild(qo);
        var input = qo.querySelector('input'), ul = qo.querySelector('ul');
        var files = [], shown = [], sel = 0;
        function render() {
            var q = input.value.trim().toLowerCase().split(/\s+/).filter(Boolean);
            shown = files.filter(function (f) { var n = f.n.toLowerCase(); return q.every(function (w) { return n.indexOf(w) >= 0; }); }).slice(0, 200);
            if (sel >= shown.length) sel = Math.max(0, shown.length - 1);
            ul.textContent = '';
            shown.forEach(function (f, i) {
                var li = document.createElement('li');
                if (i === sel) li.className = 'sel';
                if (decodeURIComponent(f.p) === decodeURIComponent(location.pathname)) li.className += ' cur';
                var n = document.createElement('span'); n.className = 'qo-name'; n.textContent = f.n;
                var d = document.createElement('span'); d.className = 'qo-date'; d.textContent = f.t ? new Date(f.t * 1000).toLocaleString([], { dateStyle: 'medium', timeStyle: 'short' }) : '';
                li.appendChild(n); li.appendChild(d);
                li.addEventListener('mousedown', function (ev) { ev.preventDefault(); location.href = f.p; });
                ul.appendChild(li);
            });
            if (!shown.length) { var e0 = document.createElement('li'); e0.className = 'qo-empty'; e0.textContent = files.length ? 'No match' : 'Loading\u2026'; ul.appendChild(e0); }
            var s0 = ul.querySelector('.sel'); if (s0) s0.scrollIntoView({ block: 'nearest' });
        }
        input.addEventListener('input', function () { sel = 0; render(); });
        input.addEventListener('keydown', function (e) {
            if (e.key === 'Escape') { e.preventDefault(); closeQuick(); }
            else if (e.key === 'ArrowDown') { e.preventDefault(); if (sel < shown.length - 1) { sel++; render(); } }
            else if (e.key === 'ArrowUp') { e.preventDefault(); if (sel > 0) { sel--; render(); } }
            else if (e.key === 'Enter') { e.preventDefault(); if (shown[sel]) location.href = shown[sel].p; }
        });
        qo.addEventListener('mousedown', function (e) { if (e.target === qo) closeQuick(); });
        input.focus(); render();
        fetch('/_files', { cache: 'no-store' }).then(function (r) { return r.json(); }).then(function (j) { files = j; render(); }, function () { files = []; render(); });
    }
    onKey(function (e) { return e.ctrlKey && !e.shiftKey && !e.altKey && e.key.toLowerCase() === 'e'; }, openQuick);

    // ---- click an image to view it full size --------------------------------------------------
    document.addEventListener('click', function (e) {
        var img = e.target.closest && e.target.closest('.markdown-container img');
        if (!img || img.closest('a')) return;
        var ov = document.createElement('div');
        ov.className = 'img-overlay';
        var big = document.createElement('img');
        big.src = img.currentSrc || img.src; big.alt = img.alt;
        ov.appendChild(big);
        function close() { ov.remove(); document.removeEventListener('keydown', esc, true); }
        function esc(ev) { if (ev.key === 'Escape') { ev.stopPropagation(); close(); } }
        ov.addEventListener('click', close);
        document.addEventListener('keydown', esc, true);
        body.appendChild(ov);
    });

    // ---- find in page (Ctrl+F) ----------------------------------------------------------------
    var bar = document.createElement('div');
    bar.id = 'find-bar';
    bar.innerHTML = '<input type="text" placeholder="Find in page" spellcheck="false"><button type="button" title="Previous (Shift+Enter)">\u2191</button><button type="button" title="Next (Enter)">\u2193</button><button type="button" title="Close (Esc)">\u2715</button>';
    body.appendChild(bar);
    var fin = bar.querySelector('input'), fb = bar.querySelectorAll('button');
    function find(back) {
        if (!fin.value) return;
        var ok = window.find(fin.value, false, !!back, true, false, false, false);
        bar.classList.toggle('miss', !ok);
    }
    function openFind() { bar.classList.add('open'); fin.focus(); fin.select(); }
    function closeFind() { bar.classList.remove('open', 'miss'); focusContent(); }
    fin.addEventListener('keydown', function (e) {
        if (e.key === 'Enter') { e.preventDefault(); find(e.shiftKey); }
        else if (e.key === 'Escape') closeFind();
    });
    fin.addEventListener('input', function () { find(false); });
    fb[0].onclick = function () { find(true); }; fb[1].onclick = function () { find(false); }; fb[2].onclick = closeFind;
    onKey(function (e) { return e.ctrlKey && !e.shiftKey && !e.altKey && e.key.toLowerCase() === 'f'; }, openFind);
    document.addEventListener('keydown', function (e) { if (e.key === 'Escape') closeFind(); });

    // ---- live reload that keeps your place -------------------------------------------------------
    var SKEY = 'mdview.scroll:' + location.pathname;
    window.__mdReload = function () {
        try { sessionStorage.setItem(SKEY, String(host ? host.scrollTop : window.scrollY)); } catch (e) {}
        location.reload();
    };
    try {
        var y = sessionStorage.getItem(SKEY);
        if (y !== null) {
            sessionStorage.removeItem(SKEY);
            var go = function () { if (host) host.scrollTop = +y; else window.scrollTo(0, +y); updateActive(); };
            go(); window.addEventListener('load', go);
        }
    } catch (e) {}
    updateActive();
})();
