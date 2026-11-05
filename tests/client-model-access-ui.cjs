const { chromium } = require(process.env.PLAYWRIGHT_MODULE || 'playwright');
const assert = require('node:assert/strict');
const path = require('node:path');
const fs = require('node:fs/promises');

(async () => {
  const { createServer } = await import('vite');
  const react = (await import('@vitejs/plugin-react')).default;
  const server = await createServer({ configFile: false, root: path.resolve(__dirname, '..'), plugins: [react()],
    logLevel: 'error', server: { host: '127.0.0.1', port: 0 } });
  await server.listen();
  const browser = await chromium.launch({ channel: process.env.PLAYWRIGHT_CHANNEL || (process.platform === 'win32' ? 'msedge' : undefined), headless: true });
  const output = path.resolve('bin-work/model-exclusions-ui');
  await fs.mkdir(output, { recursive: true });
  try {
    const page = await browser.newPage({ viewport: { width: 1280, height: 900 } });
    const errors = [];
    page.on('pageerror', error => errors.push(String(error)));
    await page.addInitScript(() => localStorage.setItem('easy-cli-proxy-api.locale', 'en'));
    await page.goto(`http://127.0.0.1:${server.httpServer.address().port}/?mock=running`);
    await page.locator('.nav-section').getByRole('button', { name: 'Advanced Features', exact: true }).click();
    await page.locator('#config-subpage-tab-general').click();
    const edit = number => page.getByRole('button', { name: `Edit authentication key ${number}`, exact: true });
    await edit(1).click();
    let dialog = page.getByRole('dialog');
    let exclusions = dialog.getByRole('textbox', { name: 'Excluded models', exact: true });
    await exclusions.waitFor();
    await exclusions.fill('private-model\npremium-*');
    await dialog.getByRole('button', { name: 'Save', exact: true }).click();
    await dialog.waitFor({ state: 'hidden' });
    await edit(1).click();
    dialog = page.getByRole('dialog');
    exclusions = dialog.getByRole('textbox', { name: 'Excluded models', exact: true });
    await exclusions.waitFor();
    await page.waitForFunction(() => document.querySelector('textarea[aria-labelledby="client-model-exclusions-label"]')?.value === 'private-model\npremium-*');
    for (const width of [1280, 768, 390]) {
      await page.setViewportSize({ width, height: 900 });
      await page.evaluate(() => Promise.all(document.getAnimations()
        .filter(animation => Number.isFinite(animation.effect?.getComputedTiming().endTime))
        .map(animation => animation.finished.catch(() => {}))));
      const box = await exclusions.boundingBox();
      assert(box && box.x >= 0 && box.x + box.width <= width, `Model rules overflow at ${width}px`);
      const bounds = await dialog.boundingBox();
      const save = await dialog.getByRole('button', { name: 'Save', exact: true }).boundingBox();
      assert(bounds && save && save.y + save.height <= bounds.y + bounds.height, `Save escapes dialog at ${width}px`);
      await page.screenshot({ path: path.join(output, `edit-key-${width}.png`) });
      if (width === 1280) await fs.writeFile(path.join(output, 'editor.html'),
        (await page.content()).replace(/<script\b[^>]*>[\s\S]*?<\/script>/gi, ''));
    }
    await page.setViewportSize({ width: 1280, height: 900 });
    await dialog.getByRole('button', { name: 'Close', exact: true }).click();
    await edit(2).click();
    dialog = page.getByRole('dialog');
    exclusions = dialog.getByRole('textbox', { name: 'Excluded models', exact: true });
    await page.waitForFunction(() => document.querySelector('textarea[aria-labelledby="client-model-exclusions-label"]')?.disabled === false);
    assert.equal(await exclusions.inputValue(), '', 'Rules leaked to another key');
    assert.deepEqual(errors, []);
    console.log('Client key exclusions save, reload, stay isolated, and fit 1280/768/390px.');
  } finally { await browser.close(); await server.close(); }
})().catch(error => { console.error(error); process.exitCode = 1; });
