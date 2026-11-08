// Real-browser regression tests for the content UI. No native host or desktop app needed.
// npm run e2e:fullscreen
import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { build } from 'esbuild';
import { chromium } from 'playwright';
import { fileURLToPath } from 'node:url';

const { outputFiles } = await build({
  stdin: {
    contents: `import { VideoButton } from './src/content/video-button';
      const start = () => {
        window.__testButton = new VideoButton();
        window.__testButton.setEnabled(true);
      };
      if (document.documentElement) start();
      else document.addEventListener('DOMContentLoaded', start, { once: true });`,
    resolveDir: fileURLToPath(new URL('..', import.meta.url)),
  },
  bundle: true,
  write: false,
  format: 'iife',
});

const server = createServer((req, res) => {
  res.setHeader('Content-Type', 'text/html');
  res.end(`<!doctype html><style>body{margin:40px} video,iframe{width:640px;height:360px} video{background:#223} #player{width:640px}</style>
    ${req.url === '/outer'
      ? `<div id="player"><iframe allowfullscreen src="http://localhost:${server.address().port}/video"></iframe></div>`
      : '<div id="player"><video controls></video></div>'}
    <button id="fullscreen">Fullscreen player</button>
    <script>document.querySelector('#fullscreen').onclick=()=>document.querySelector('#player').requestFullscreen()</script>`);
});
await new Promise((resolve) => server.listen(0, '0.0.0.0', resolve));
let browser;
try {
  browser = await chromium.launch({ headless: true });
  const context = await browser.newContext({ viewport: { width: 1000, height: 800 } });
  await context.addInitScript({ content: outputFiles[0].text });
  const page = await context.newPage();
  const shown = (frame) => frame.evaluate(() => !!document.querySelector('[data-prism-video-button]')?.shadowRoot?.querySelector('.wrap.show'));
  const until = async (predicate, message) => {
    const end = Date.now() + 5000;
    while (!(await predicate())) {
      if (Date.now() > end) throw new Error(message);
      await new Promise((resolve) => setTimeout(resolve, 30));
    }
  };
  const hover = async (frame) => {
    await frame.locator('video').hover({ position: { x: 200, y: 150 } });
    await until(() => shown(frame), 'download button did not appear on hover');
  };
  const hidden = async (frame) => {
    await until(async () => !(await shown(frame)), 'download UI remained visible in fullscreen');
    await frame.locator('video').hover({ position: { x: 220, y: 160 } });
    assert.equal(await shown(frame), false, 'pointer movement reopened the button');
    await frame.locator('video').click({ button: 'right', position: { x: 220, y: 160 } });
    assert.equal(await frame.evaluate(() => window.__testButton.openForContextMenu()), false, 'context menu reopened the overlay');
  };

  await page.goto(`http://127.0.0.1:${server.address().port}/video`);
  await hover(page);
  // An open menu must disappear too, even while options are loading.
  await page.locator('video').click({ button: 'right', position: { x: 200, y: 150 } });
  assert.equal(await page.evaluate(() => window.__testButton.openForContextMenu()), true);
  await page.click('#fullscreen');
  await until(() => page.evaluate(() => !!document.fullscreenElement), 'player did not enter fullscreen');
  await hidden(page);
  await page.evaluate(() => document.exitFullscreen());
  await hover(page);
  console.log('PASS: player fullscreen hides an open menu and button; exiting restores hover');

  await page.evaluate(() => {
    document.querySelector('#fullscreen').onclick = () => document.querySelector('video').requestFullscreen();
  });
  await page.click('#fullscreen');
  await until(() => page.evaluate(() => document.fullscreenElement?.tagName === 'VIDEO'), 'video did not enter fullscreen');
  await hidden(page);
  await page.evaluate(() => document.exitFullscreen());
  await hover(page);
  console.log('PASS: native video fullscreen suppresses the overlay');

  await page.goto(`http://127.0.0.1:${server.address().port}/outer`);
  await until(() => Promise.resolve(page.frames().length === 2), 'child frame did not load');
  const child = page.frames()[1];
  await child.waitForSelector('video');
  await hover(child);
  await page.click('#fullscreen');
  await until(() => page.evaluate(() => !!document.fullscreenElement), 'ancestor did not enter fullscreen');
  assert.equal(await child.evaluate(() => document.fullscreenElement), null, 'test must exercise ancestor-only fullscreen');
  await hidden(child);
  await page.evaluate(() => document.exitFullscreen());
  await hover(child);
  console.log('PASS: cross-origin iframe inherits fullscreen suppression and recovers after exit');
} finally {
  await browser?.close();
  await new Promise((resolve) => server.close(resolve));
}
