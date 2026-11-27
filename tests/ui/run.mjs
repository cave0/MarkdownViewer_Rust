// Browser tests for the embedded UI (assets/app.js + style.css), run against the real files with Playwright.
// Usage: npm ci && npx playwright install chromium && npm test   (set CHROME_PATH to use another Chromium)
import fs from 'node:fs';
import http from 'node:http';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright';
import { spawnSync } from 'node:child_process';

const here = path.dirname(fileURLToPath(import.meta.url));
const A = path.join(here, '..', '..', 'assets') + path.sep;
const fixturePath = path.join(here, 'fixture.md');
const regressionPath = path.join(here, 'render-regressions.md');
const binary = process.env.MDVIEW_BIN || path.resolve(
  here, '..', '..', 'target', 'debug', process.platform === 'win32' ? 'mdview.exe' : 'mdview'
);

function renderMarkdown(file) {
  const result = spawnSync(binary, ['--dump-html', '-', file], {
    encoding: 'utf8',
    maxBuffer: 64 * 1024 * 1024
  });
  if (result.error) throw result.error;
  if (result.status !== 0) {
    throw new Error('Rust renderer failed for ' + file + ' (exit ' + result.status + '): ' + result.stderr);
  }
  if (!result.stdout.startsWith('<!DOCTYPE html>')) {
    throw new Error('Rust renderer returned unexpected output: ' + result.stdout.slice(0, 200));
  }
  return result.stdout;
}

const pages = new Map([
  ['/doc.md', renderMarkdown(fixturePath)],
  ['/regressions.md', renderMarkdown(regressionPath)]
]);

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
  const html = pages.get(new URL(q.url, 'http://localhost').pathname);
  if (!html) { s.statusCode = 404; return s.end('Not found'); }
  s.setHeader('content-type', 'text/html; charset=utf-8');
  s.end(html);
});
await new Promise((res) => server.listen(0, res));
const baseUrl = `http://localhost:${server.address().port}`;
const url = `${baseUrl}/doc.md`;

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

// ---- file export bundles local assets and protects the source file ------------------------------------
const exportRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'mdview-export-'));
const exportPath = path.join(exportRoot, 'fixture.html');
const exportResult = spawnSync(binary, ['--dump-html', exportPath, fixturePath], {
  encoding: 'utf8',
  maxBuffer: 64 * 1024 * 1024
});
const exportAssets = exportPath + '.assets';
const exportedHtml = fs.existsSync(exportPath) ? fs.readFileSync(exportPath, 'utf8') : '';
check('file export writes a companion asset directory',
  exportResult.status === 0 && fs.existsSync(path.join(exportAssets, 'app.js')) && fs.existsSync(path.join(exportAssets, 'mermaid.min.js')),
  exportResult.stderr || String(exportResult.status));
check('file export includes KaTeX and its fonts',
  fs.existsSync(path.join(exportAssets, 'katex', 'katex.min.css')) &&
  fs.existsSync(path.join(exportAssets, 'katex', 'fonts', 'KaTeX_Main-Regular.woff2')));
check('file export rewrites runtime paths and preserves source-relative links',
  exportedHtml.includes('file://') && exportedHtml.includes('.assets/app.js') && exportedHtml.includes('<base href="file://'),
  exportedHtml.slice(0, 300));
const protectedSource = spawnSync(binary, ['--dump-html', fixturePath, fixturePath], { encoding: 'utf8' });
check('HTML export refuses to overwrite the Markdown source', protectedSource.status === 2 &&
  fs.readFileSync(fixturePath, 'utf8').startsWith('---'));
fs.rmSync(exportRoot, { recursive: true, force: true });

const standalonePage = await ctx.newPage();
const standaloneErrors = [];
standalonePage.on('pageerror', (e) => standaloneErrors.push(e.message));
await standalonePage.goto('file://' + exportPath);
await standalonePage.waitForTimeout(2200);
const standaloneState = await standalonePage.evaluate(() => ({
  shellLoaded: !!document.querySelector('#source-toggle-btn'),
  mathRendered: !!document.querySelector('.math-inline .katex'),
  diagramRendered: !!document.querySelector('pre.mermaid svg')
}));
check('exported HTML opens directly as a local file', standaloneState.shellLoaded, JSON.stringify(standaloneState));
check('exported HTML loads its offline KaTeX runtime', standaloneState.mathRendered, JSON.stringify(standaloneState));
check('exported HTML loads its offline Mermaid runtime', standaloneState.diagramRendered, JSON.stringify(standaloneState));
check('standalone file has no uncaught page errors', standaloneErrors.length === 0, standaloneErrors.join(' | '));
await standalonePage.close();


// ---- actual Rust renderer integration ---------------------------------------------------------------
const rendererPage = await page1.evaluate(() => {
  const headings = [...document.querySelectorAll('.markdown-container h1[id], .markdown-container h2[id], .markdown-container h3[id], .markdown-container h4[id], .markdown-container h5[id], .markdown-container h6[id]')];
  const ids = headings.map((h) => h.id);
  const outlineTargets = [...document.querySelectorAll('#mdview-sidebar a[data-target]')].map((a) => a.getAttribute('data-target'));
  return {
    hasGeneratedPageShell: !!document.querySelector('#mdview-sidebar') && !!document.querySelector('#source-toggle-btn'),
    idsUnique: new Set(ids).size === ids.length,
    outlineTargetsExist: outlineTargets.every((id) => ids.includes(id)),
    mathPlaceholders: document.querySelectorAll('.math-inline, .math-display').length,
    mermaidBlocks: document.querySelectorAll('pre.mermaid').length
  };
});
check('browser loads HTML generated by the Rust renderer', rendererPage.hasGeneratedPageShell);
check('Rust renderer emits unique heading IDs', rendererPage.idsUnique, JSON.stringify(rendererPage));
check('every outline link targets an existing heading', rendererPage.outlineTargetsExist, JSON.stringify(rendererPage));
check('Rust renderer emits math and Mermaid placeholders', rendererPage.mathPlaceholders > 0 && rendererPage.mermaidBlocks === 9, JSON.stringify(rendererPage));

const regressionPage = await ctx.newPage();
const regressionErrors = [];
regressionPage.on('pageerror', (e) => regressionErrors.push(e.message));
await regressionPage.goto(baseUrl + '/regressions.md');
await regressionPage.waitForTimeout(1000);
const regressions = await regressionPage.evaluate(() => {
  const headings = [...document.querySelectorAll('.markdown-container h1[id], .markdown-container h2[id], .markdown-container h3[id], .markdown-container h4[id], .markdown-container h5[id], .markdown-container h6[id]')];
  const code = [...document.querySelectorAll('.markdown-container pre code')].map((e) => e.textContent).join('\n');
  return {
    ids: headings.map((h) => h.id),
    indentedCodeKeepsMathDelimiters: code.includes('\\(do not typeset this)'),
    actualMathRendered: !!document.querySelector('.math-inline .katex'),
    injectedScriptAbsent: !document.querySelector('#mdview-injected-script')
  };
});
check('explicit and generated heading IDs do not collide', new Set(regressions.ids).size === regressions.ids.length, JSON.stringify(regressions.ids));
check('math delimiters inside indented code remain literal', regressions.indentedCodeKeepsMathDelimiters, JSON.stringify(regressions));
check('real inline math is typeset by KaTeX', regressions.actualMathRendered, JSON.stringify(regressions));
check('Safe Mode removes raw script elements', regressions.injectedScriptAbsent, JSON.stringify(regressions));
check('regression fixture has no uncaught page errors', regressionErrors.length === 0, regressionErrors.join(' | '));
await regressionPage.close();

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

// Printing while the raw-source toggle is active should still print rendered Markdown.
await page1.click('#source-toggle-btn');
await page1.emulateMedia({ media: 'print' });
const printVisibility = await page1.evaluate(() => ({
  bodyHasSourceMode: document.body.classList.contains('view-source'),
  renderedDisplay: getComputedStyle(document.querySelector('.markdown-container')).display,
  sourceDisplay: getComputedStyle(document.querySelector('.raw-source-view')).display
}));
check('print mode restores rendered Markdown from source-view mode',
  printVisibility.bodyHasSourceMode && printVisibility.renderedDisplay !== 'none' && printVisibility.sourceDisplay === 'none',
  JSON.stringify(printVisibility));
await page1.emulateMedia({ media: 'screen' });
await page1.click('#source-toggle-btn');

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
