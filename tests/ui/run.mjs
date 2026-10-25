// Browser tests for the embedded UI (assets/app.js + style.css), run against the real files with Playwright.
// Usage: npm ci && npx playwright install chromium && npm test   (set CHROME_PATH to use another Chromium)
import fs from 'node:fs';
import http from 'node:http';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright';
import { marked } from 'marked';

const here = path.dirname(fileURLToPath(import.meta.url));
const A = path.join(here, '..', '..', 'assets') + path.sep;
const esc = (s) => s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');

const r = new marked.Renderer();
r.code = ({ text, lang }) => {
  if (lang === 'mermaid') return `<pre class="mermaid">${esc(text)}\n</pre>`;
  if (lang === 'math') return `<span class="math-display">${esc(text)}</span>`;
  return `<div class="code-block-container"><pre><code class="language-${lang || ''}">${esc(text)}</code></pre></div>`;
};
r.heading = function ({ tokens, depth }) {
  const text = this.parser.parseInline(tokens);
  const id = text.toLowerCase().replace(/<[^>]+>/g, '').replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '');
  return `<h${depth} id="${id}">${text}</h${depth}>\n`;
};
const md = fs.readFileSync(path.join(here, 'fixture.md'), 'utf8').replace(/^---\n[\s\S]*?\n---\n/, '');
let body = marked.parse(md, { renderer: r, gfm: true }).replace(/\$([^$\n]+)\$/g, '<span class="math-inline">$1</span>');
const page = `<!DOCTYPE html><html><head><meta charset="utf-8"><meta name="color-scheme" content="dark"><style>${fs.readFileSync(A + 'style.css', 'utf8')}</style></head>` +
  `<body><script src="/_a/boot.js"></script><nav id="mdview-sidebar" class="mdview-sidebar"></nav>` +
  `<div class="mdview-content" tabindex="-1"><div class="markdown-container">${body}</div></div><script src="/_a/app.js" defer></script></body></html>`;

const server = http.createServer((q, s) => {
  if (q.url.startsWith('/_a/')) {
    const p = path.join(A, decodeURIComponent(q.url.slice(4).split('?')[0]));
    if (p.startsWith(A) && fs.existsSync(p)) {
      s.setHeader('content-type', p.endsWith('.js') ? 'text/javascript' : p.endsWith('.css') ? 'text/css' : 'application/octet-stream');
      return s.end(fs.readFileSync(p));
    }
    s.statusCode = 404; return s.end();
  }
  if (q.url.startsWith('/_files')) { s.setHeader('content-type', 'application/json'); return s.end('[]'); }
  s.setHeader('content-type', 'text/html'); s.end(page);
});
await new Promise((res) => server.listen(0, res));
const url = `http://localhost:${server.address().port}/doc.md`;

let failed = 0;
const check = (name, ok, extra = '') => { console.log(`${ok ? 'PASS' : 'FAIL'}  ${name}${ok ? '' : '  -> ' + extra}`); if (!ok) failed++; };

const browser = await chromium.launch({ executablePath: process.env.CHROME_PATH || undefined, args: ['--no-sandbox'] });
const ctx = await browser.newContext({ viewport: { width: 1100, height: 800 } });
const page1 = await ctx.newPage();
const errors = [];
page1.on('pageerror', (e) => errors.push(e.message));
await page1.goto(url);
const scrollAll = async (p) => {
  for (let i = 0; i < 80; i++) {
    const done = await p.evaluate(() => {
      const unrendered = [...document.querySelectorAll('pre.mermaid:not([data-processed])')];
      if (!unrendered.length) return true;
      unrendered[0].scrollIntoView();
      return false;
    });
    if (done) return;
    await p.waitForTimeout(250);
  }
};
await scrollAll(page1);
await page1.waitForTimeout(500);

// ---- diagrams ----
const d = await page1.evaluate(() => {
  const pres = [...document.querySelectorAll('pre.mermaid')];
  const ids = pres.map((e) => e.querySelector('svg')?.id || '');
  return { n: pres.length, rendered: pres.filter((e) => e.querySelector('svg')).length, uniq: new Set(ids).size, tools: document.querySelectorAll('.dgm-tools').length };
});
check('all diagrams render', d.n === 9 && d.rendered === d.n, JSON.stringify(d));
check('diagram svg ids are unique (same-millisecond collision regression)', d.uniq === d.n, JSON.stringify(d));
check('each diagram has hover tools', d.tools === d.n, JSON.stringify(d));

const wide = await page1.evaluate(() => {
  const pre = [...document.querySelectorAll('pre.mermaid')].pop();
  const svg = pre.querySelector('svg');
  return { client: pre.clientWidth, scroll: pre.scrollWidth, minW: svg.style.minWidth, vb: svg.viewBox.baseVal.width, shown: svg.getBoundingClientRect().width };
});
check('wide diagram keeps a readable scale (75%, capped at 2400px) and scrolls sideways', wide.scroll > wide.client && wide.shown >= Math.min(wide.vb * 0.74, 2399), JSON.stringify(wide));

await page1.evaluate(() => document.querySelector('pre.mermaid').scrollIntoView());
await page1.locator('pre.mermaid').first().locator('svg').click({ position: { x: 20, y: 20 } });
check('clicking a diagram opens the full-size overlay', await page1.locator('.dgm-overlay svg').count() === 1);
await page1.keyboard.press('Escape');
check('Escape closes the overlay', await page1.locator('.dgm-overlay').count() === 0);

// ---- math ----
await page1.evaluate(() => document.querySelector('.math-display').scrollIntoView());
await page1.waitForTimeout(800);
const m = await page1.evaluate(() => ({ katex: !!document.querySelector('.math-display .katex'), tex: document.querySelector('.math-display')?.__tex || '', btn: !!document.querySelector('.math-display .tex-copy') }));
check('display math renders with a Copy-TeX button', m.katex && m.btn && m.tex.includes('\\int_0^1'), JSON.stringify(m));

// ---- emoji / highlight ----
const t = await page1.evaluate(() => {
  const p = document.querySelector('.markdown-container p');
  const code = document.querySelector('.markdown-container p code');
  return { text: p.textContent, marks: [...document.querySelectorAll('mark')].map((x) => x.textContent), code: code ? code.textContent : '' };
});
check('emoji shortcodes are replaced', t.text.includes('\u{1F389}') && t.text.includes('\u{1F680}'), t.text);
check('==text== becomes <mark>', t.marks.includes('highlighted text'), JSON.stringify(t.marks));
check('unknown :a:b: and "x == y" are left alone', t.text.includes('a:b:c') && t.text.includes('x == y'), t.text);
check('code spans are untouched', t.code === ':smile:', t.code);

// ---- light theme ----
const bg = () => page1.evaluate(() => getComputedStyle(document.body).backgroundColor);
const dark = await bg();
await page1.keyboard.press('Control+Shift+L');
await page1.waitForLoadState('load');
await page1.waitForTimeout(800);
const light = await bg();
check('Ctrl+Shift+L switches to a light background', dark !== light && light === 'rgb(255, 255, 255)', `${dark} -> ${light}`);
await scrollAll(page1);
const lt = await page1.evaluate(() => ({ cls: document.body.classList.contains('theme-light'), meta: document.querySelector('meta[name=color-scheme]').content, svgs: document.querySelectorAll('pre.mermaid svg').length }));
check('light theme persists across reload and diagrams re-render', lt.cls && lt.meta === 'light' && lt.svgs === 9, JSON.stringify(lt));
await page1.keyboard.press('Control+Shift+L');
await page1.waitForLoadState('load');
await page1.waitForTimeout(500);
check('toggling back restores the dark background', (await bg()) === dark);

// ---- reading position memory ----
const page2 = await ctx.newPage();
await page2.goto(url);
await page2.waitForTimeout(800);
await page2.evaluate(() => { document.getElementById('filler-30').scrollIntoView(); });
await page2.waitForTimeout(900);
await page2.close();
const page3 = await ctx.newPage();
await page3.goto(url);
await page3.waitForTimeout(1800);
const topOfFiller = await page3.evaluate(() => Math.abs(document.getElementById('filler-30').getBoundingClientRect().top));
check('reopening the file returns to the last heading', topOfFiller < 120, `top=${topOfFiller}`);

check('no uncaught page errors', errors.length === 0, errors.join(' | '));
await browser.close();
server.close();
console.log(failed ? `\n${failed} check(s) failed` : '\nall checks passed');
process.exit(failed ? 1 : 0);
