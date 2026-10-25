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
    // Light theme (Ctrl+Shift+L): colors come from CSS variables; Mermaid picks its theme at load, so reload.
    function isLight() { return body.classList.contains('theme-light'); }
    function syncLight() {
        var m = document.querySelector('meta[name=color-scheme]');
        if (m) m.setAttribute('content', isLight() ? 'light' : 'dark');
        var b = $('light-toggle-btn');
        if (b) { b.textContent = isLight() ? '\u263E' : '\u2600'; b.title = (isLight() ? 'Dark theme' : 'Light theme') + ' (Ctrl+Shift+L)'; }
    }
    function setLight(on, reload) {
        body.classList.toggle('theme-light', on);
        store.set('mdview.light', on ? '1' : '0');
        syncLight();
        if (reload && document.querySelector('pre.mermaid') && window.__mdReload) window.__mdReload();
    }
    (function () {
        var b = document.createElement('button');
        b.id = 'light-toggle-btn'; b.type = 'button'; b.setAttribute('aria-label', 'Toggle light theme');
        body.appendChild(b);
        syncLight();
    })();
    function on(id, fn) { var b = $(id); if (b) b.addEventListener('click', fn); }
    on('light-toggle-btn', function () { setLight(!isLight(), true); });
    document.addEventListener('keydown', function (e) {
        if (e.ctrlKey && e.shiftKey && !e.altKey && e.key.toLowerCase() === 'l') { e.preventDefault(); setLight(!isLight(), true); }
    });
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
        var raw = el.textContent;
        katex.render(tex, el, {
            displayMode: el.classList.contains('math-display'), throwOnError: false, trust: false, strict: 'ignore', output: 'htmlAndMathml'
        });
        if (el.classList.contains('math-display')) {
            el.__tex = raw.trim();
            var cb = document.createElement('button');
            cb.type = 'button'; cb.className = 'tex-copy'; cb.title = 'Copy LaTeX source'; cb.textContent = 'TeX';
            el.appendChild(cb);
        }
    });
    var mermaidQueue = Promise.resolve();
    // Very wide diagrams keep a readable scale (75%, max 2400px) and scroll sideways instead of shrinking to fit.
    function decorateDiagram(el, src) {
        var svg = el.querySelector('svg');
        if (!svg || el.querySelector('.dgm-tools')) return;
        el.__src = src;
        var vb = svg.viewBox && svg.viewBox.baseVal;
        // only diagrams that would shrink below ~60% get the sideways-scroll treatment; others still fit the page
        if (vb && vb.width && el.clientWidth < vb.width * 0.6) svg.style.minWidth = Math.min(Math.round(vb.width * 0.75), 2400) + 'px';
        var tools = document.createElement('div');
        tools.className = 'dgm-tools';
        [['zoom', '\u2922', 'Open full size'], ['svg', 'SVG', 'Copy as SVG'], ['src', 'Mermaid', 'Copy Mermaid source']].forEach(function (d) {
            var b = document.createElement('button');
            b.type = 'button'; b.setAttribute('data-act', d[0]); b.title = d[2]; b.textContent = d[1];
            tools.appendChild(b);
        });
        el.appendChild(tools);
    }
    function svgText(svg) {
        var c = svg.cloneNode(true), vb = svg.viewBox.baseVal;
        c.setAttribute('xmlns', 'http://www.w3.org/2000/svg');
        c.removeAttribute('style');
        c.setAttribute('width', Math.round(vb.width)); c.setAttribute('height', Math.round(vb.height));
        return new XMLSerializer().serializeToString(c);
    }
    function openDiagram(el) {
        var svg = el.querySelector('svg');
        if (!svg) return;
        var vb = svg.viewBox.baseVal, c = svg.cloneNode(true);
        var w = Math.max(Math.round(vb.width * 1.1), Math.min(Math.round(window.innerWidth * 0.94), Math.round(vb.width * 1.8)));
        c.removeAttribute('style'); c.setAttribute('width', w); c.setAttribute('height', Math.round(w * vb.height / vb.width));
        var ov = document.createElement('div');
        ov.className = 'dgm-overlay';
        var x = document.createElement('button');
        x.type = 'button'; x.className = 'dgm-close'; x.title = 'Close (Esc)'; x.textContent = '\u2715';
        ov.appendChild(x); ov.appendChild(c);
        function close() { ov.remove(); document.removeEventListener('keydown', esc, true); }
        function esc(ev) { if (ev.key === 'Escape') { ev.stopPropagation(); close(); } }
        ov.addEventListener('click', function (ev) { if (ev.target === ov || ev.target === x) close(); });
        document.addEventListener('keydown', esc, true);
        body.appendChild(ov);
    }
    document.addEventListener('click', function (e) {
        var t = e.target;
        if (!t || !t.closest) return;
        var b = t.closest('.dgm-tools button');
        if (b) {
            var el = b.closest('pre.mermaid'), act = b.getAttribute('data-act'), label = b.textContent;
            if (act === 'zoom') openDiagram(el);
            else if (act === 'svg') copyText(svgText(el.querySelector('svg')), function (ok) { flash(b, ok, label); });
            else copyText(el.__src || '', function (ok) { flash(b, ok, label); });
            return;
        }
        var tb = t.closest('.tex-copy');
        if (tb) { var m = tb.closest('.math-display'); copyText((m && m.__tex) || '', function (ok) { flash(tb, ok, 'TeX'); }); return; }
        var sv = t.closest('pre.mermaid svg');
        if (sv && !String(window.getSelection()).length) openDiagram(sv.closest('pre.mermaid'));
    });
    lazy('pre.mermaid', '/_a/mermaid.min.js', function () {
        mermaid.initialize({ startOnLoad: false, theme: isLight() ? 'default' : 'dark', securityLevel: 'strict', fontFamily: 'Segoe UI, Arial, sans-serif' });
    }, function (el) {
        // One render at a time: mermaid.run() names each svg "mermaid-<Date.now()>", so two runs started in the
        // same millisecond (several diagrams entering view together) share an id and draw into each other.
        var src = el.textContent;
        mermaidQueue = mermaidQueue.then(function () {
            return mermaid.run({ nodes: [el], suppressErrors: true });
        }).then(function () { decorateDiagram(el, src); }).catch(function () {});
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

    // ---- :emoji: shortcodes and ==highlight== in body text (never inside code, links, math or diagrams) ----
    (function () {
        var E = { smile: '\u{1F604}', grin: '\u{1F601}', joy: '\u{1F602}', wink: '\u{1F609}', sweat_smile: '\u{1F605}', thinking: '\u{1F914}',
            sob: '\u{1F62D}', cry: '\u{1F622}', angry: '\u{1F620}', hugs: '\u{1F917}', eyes: '\u{1F440}', skull: '\u{1F480}', ghost: '\u{1F47B}',
            robot: '\u{1F916}', brain: '\u{1F9E0}', heart: '\u2764\uFE0F', broken_heart: '\u{1F494}', star: '\u2B50', sparkles: '\u2728', zap: '\u26A1',
            fire: '\u{1F525}', tada: '\u{1F389}', rocket: '\u{1F680}', bulb: '\u{1F4A1}', warning: '\u26A0\uFE0F', no_entry: '\u26D4', stop_sign: '\u{1F6D1}',
            construction: '\u{1F6A7}', white_check_mark: '\u2705', heavy_check_mark: '\u2714\uFE0F', x: '\u274C', question: '\u2753', exclamation: '\u2757',
            information_source: '\u2139\uFE0F', '+1': '\u{1F44D}', thumbsup: '\u{1F44D}', '-1': '\u{1F44E}', thumbsdown: '\u{1F44E}', ok_hand: '\u{1F44C}',
            clap: '\u{1F44F}', pray: '\u{1F64F}', muscle: '\u{1F4AA}', wave: '\u{1F44B}', raised_hands: '\u{1F64C}', point_right: '\u{1F449}',
            arrow_right: '\u27A1\uFE0F', arrow_left: '\u2B05\uFE0F', arrow_up: '\u2B06\uFE0F', arrow_down: '\u2B07\uFE0F', memo: '\u{1F4DD}',
            book: '\u{1F4D6}', books: '\u{1F4DA}', bookmark: '\u{1F516}', link: '\u{1F517}', pushpin: '\u{1F4CC}', mag: '\u{1F50D}', lock: '\u{1F512}',
            key: '\u{1F511}', wrench: '\u{1F527}', hammer: '\u{1F528}', bug: '\u{1F41B}', calendar: '\u{1F4C5}', hourglass: '\u231B', coffee: '\u2615',
            chart_with_upwards_trend: '\u{1F4C8}', chart_with_downwards_trend: '\u{1F4C9}', sunny: '\u2600\uFE0F', cloud: '\u2601\uFE0F',
            snowflake: '\u2744\uFE0F', seedling: '\u{1F331}', earth_africa: '\u{1F30D}', speech_balloon: '\u{1F4AC}', balance_scale: '\u2696\uFE0F',
            microscope: '\u{1F52C}', telescope: '\u{1F52D}', dna: '\u{1F9EC}', infinity: '\u267E\uFE0F', hourglass_flowing_sand: '\u23F3' };
        var re = /:([a-z0-9_+-]+):|==([^=\s](?:[^=]*[^=\s])?)==/g;
        var skip = 'pre, code, kbd, script, style, textarea, svg, a, mark, .math-inline, .math-display, .mermaid, .katex, .raw-source-view';
        var root = document.querySelector('.markdown-container');
        if (!root) return;
        var w = document.createTreeWalker(root, NodeFilter.SHOW_TEXT, null), hits = [], n;
        while ((n = w.nextNode())) {
            var v = n.nodeValue;
            if ((v.indexOf(':') < 0 && v.indexOf('==') < 0) || !n.parentElement || n.parentElement.closest(skip)) continue;
            hits.push(n);
        }
        hits.forEach(function (node) {
            var text = node.nodeValue, last = 0, m, frag = null;
            re.lastIndex = 0;
            while ((m = re.exec(text))) {
                var rep = null;
                if (m[1] !== undefined) { if (Object.prototype.hasOwnProperty.call(E, m[1])) rep = document.createTextNode(E[m[1]]); }
                else { rep = document.createElement('mark'); rep.textContent = m[2]; }
                if (!rep) continue;
                if (!frag) frag = document.createDocumentFragment();
                frag.appendChild(document.createTextNode(text.slice(last, m.index)));
                frag.appendChild(rep);
                last = m.index + m[0].length;
            }
            if (frag) { frag.appendChild(document.createTextNode(text.slice(last))); node.parentNode.replaceChild(frag, node); }
        });
    })();

    // ---- remember where you stopped reading in each file (anchored to a heading, not a pixel offset, because
    //      lazily rendered diagrams change the heights above you) ---------------------------------------------
    (function () {
        var KEY = 'mdview.pos', path = location.pathname, moved = false, timer = 0;
        function load() { try { return JSON.parse(store.get(KEY) || '{}') || {}; } catch (e) { return {}; } }
        var heads = Array.prototype.slice.call(document.querySelectorAll('.markdown-container h1[id], .markdown-container h2[id], .markdown-container h3[id], .markdown-container h4[id]'));
        function topOf(el) { return el.getBoundingClientRect().top - (host ? host.getBoundingClientRect().top : 0); }
        function save() {
            timer = 0;
            if (!heads.length) return;
            var lo = 0, hi = heads.length - 1, best = -1;
            while (lo <= hi) { var mid = (lo + hi) >> 1; if (topOf(heads[mid]) <= 40) { best = mid; lo = mid + 1; } else hi = mid - 1; }
            var map = load();
            if (best < 0) delete map[path]; else map[path] = [heads[best].id, Math.round(-topOf(heads[best])), Date.now()];
            var keys = Object.keys(map).sort(function (a, b) { return map[b][2] - map[a][2]; });
            keys.slice(60).forEach(function (k) { delete map[k]; });
            store.set(KEY, JSON.stringify(map));
        }
        scroller.addEventListener('scroll', function () { if (!timer) timer = setTimeout(save, 500); }, { passive: true });
        ['wheel', 'touchstart', 'keydown', 'mousedown'].forEach(function (ev) { window.addEventListener(ev, function () { moved = true; }, { passive: true, capture: true }); });
        var reloadPos = null;
        try { reloadPos = sessionStorage.getItem('mdview.scroll:' + path); } catch (e) {}
        var saved = load()[path];
        if (!saved || reloadPos !== null || location.hash) return;
        function restore() {
            if (moved) return;
            var el = document.getElementById(saved[0]);
            if (!el) return;
            var delta = topOf(el) + saved[1];
            if (host) host.scrollTop += delta; else window.scrollBy(0, delta);
        }
        restore(); window.addEventListener('load', restore); setTimeout(restore, 900);
    })();

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
