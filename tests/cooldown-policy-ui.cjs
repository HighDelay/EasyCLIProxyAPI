const { chromium } = require(process.env.PLAYWRIGHT_MODULE || 'playwright');
const assert = require('node:assert/strict');
const path = require('node:path');
const fs = require('node:fs/promises');

(async () => {
  const { createServer } = await import('vite');
  const react = (await import('@vitejs/plugin-react')).default;
  const server = await createServer({ configFile: false, root: path.resolve(__dirname, '..'), plugins: [react()],
    logLevel: 'error', server: { host: '127.0.0.1', port: 0,
      watch: { ignored: ['**/src-tauri/**', '**/bin-work/**'] } } });
  await server.listen();
  const browser = await chromium.launch({ channel: process.env.PLAYWRIGHT_CHANNEL || (process.platform === 'win32' ? 'msedge' : undefined), headless: true });
  const output = path.resolve('bin-work/cooldown-policy-ui');
  await fs.mkdir(output, { recursive: true });
  try {
    const page = await browser.newPage({ viewport: { width: 1440, height: 1000 } });
    const errors = [];
    page.on('pageerror', error => errors.push(String(error)));
    await page.addInitScript(() => localStorage.setItem('easy-cli-proxy-api.locale', 'en'));
    await page.goto(`http://127.0.0.1:${server.httpServer.address().port}/?mock=running`);
    await page.locator('.nav-section').getByRole('button', { name: 'Advanced Features', exact: true }).click();
    await page.locator('#config-subpage-tab-requests').click();
    const card = page.locator('#config-cooldown-policy');
    const scope = card.getByRole('combobox', { name: 'Apply provider defaults to', exact: true });
    const mode = card.getByRole('combobox', { name: 'Errors that trigger cooldowns', exact: true });
    await scope.waitFor();
    assert.equal(await scope.inputValue(), 'all');
    assert.equal(await mode.inputValue(), 'quota');
    await scope.selectOption('selected');
    assert.equal(await card.getByRole('checkbox', { name: 'Antigravity', exact: true }).isChecked(), true);
    await card.getByRole('checkbox', { name: 'Codex', exact: true }).check();
    await page.locator('#config-subpage-tab-general').click();
    await page.locator('#config-subpage-tab-requests').click();
    assert.equal(await card.getByRole('checkbox', { name: 'Codex', exact: true }).isChecked(), true, 'Switching settings tabs discarded the policy draft');
    await card.getByRole('checkbox', { name: 'Remember cooldowns across core restarts', exact: true }).check();
    await mode.selectOption('temporary');
    const seconds = card.getByRole('textbox', { name: 'Temporary failure cooldown (seconds)', exact: true });
    await seconds.fill('');
    assert.equal(await card.getByRole('button', { name: 'Save', exact: true }).isDisabled(), true);
    await seconds.fill('15');
    await card.getByRole('button', { name: 'Save', exact: true }).click();
    await card.getByText('Cooldown policy saved and applied.', { exact: true }).waitFor();
    await card.getByRole('button', { name: 'Reload cooldown policy', exact: true }).click();
    await card.getByRole('checkbox', { name: 'Codex', exact: true }).waitFor();
    assert.equal(await mode.inputValue(), 'temporary');
    assert.equal(await seconds.inputValue(), '15');
    assert.equal(await card.getByRole('checkbox', { name: 'Antigravity', exact: true }).isChecked(), true);
    assert.equal(await card.getByRole('checkbox', { name: 'Codex', exact: true }).isChecked(), true);
    assert.equal(await card.getByRole('checkbox', { name: 'Remember cooldowns across core restarts', exact: true }).isChecked(), true);
    await mode.selectOption('quota');
    await card.getByRole('button', { name: 'Save', exact: true }).click();
    await card.getByText('Cooldown policy saved and applied.', { exact: true }).waitFor();
    for (const width of [1440, 768, 390]) {
      await page.setViewportSize({ width, height: 1000 });
      await card.scrollIntoViewIfNeeded();
      await page.evaluate(() => Promise.all(document.getAnimations()
        .filter(animation => Number.isFinite(animation.effect?.getComputedTiming().endTime))
        .map(animation => animation.finished.catch(() => {}))));
      const bounds = await card.boundingBox();
      assert(bounds && bounds.x >= 0 && bounds.x + bounds.width <= width, `Policy card overflows at ${width}px`);
      assert.equal(await card.evaluate(element => element.scrollWidth > element.clientWidth + 1), false, `Policy fields overflow at ${width}px`);
      await page.screenshot({ path: path.join(output, `policy-${width}.png`), fullPage: true });
      if (width === 1440) {
        const snapshot = await page.evaluate(() => {
          const root = document.documentElement.cloneNode(true);
          const inputs = [...document.querySelectorAll('input')];
          root.querySelectorAll('input').forEach((copy, index) => {
            copy.toggleAttribute('checked', inputs[index].checked);
            copy.setAttribute('value', inputs[index].value);
          });
          const selects = [...document.querySelectorAll('select')];
          root.querySelectorAll('select').forEach((copy, index) => {
            [...copy.options].forEach(option => option.toggleAttribute('selected', option.value === selects[index].value));
          });
          root.querySelectorAll('script').forEach(script => script.remove());
          return '<!doctype html>' + root.outerHTML;
        });
        await fs.writeFile(path.join(output, 'policy.html'), snapshot);
      }
    }
    await page.setViewportSize({ width: 1440, height: 1000 });
    await scope.selectOption('all');
    await card.getByRole('button', { name: 'Save', exact: true }).click();
    await card.getByText('Cooldown policy saved and applied.', { exact: true }).waitFor();
    await card.getByRole('button', { name: 'Reload cooldown policy', exact: true }).click();
    await card.getByRole('combobox', { name: 'Apply provider defaults to', exact: true }).waitFor();
    assert.equal(await scope.inputValue(), 'all');
    assert.equal(await card.getByRole('group', { name: 'Providers with cooldowns' }).count(), 0);
    await scope.selectOption('selected');
    await card.getByRole('button', { name: 'Discard changes and reload cooldown policy', exact: true }).click();
    await card.getByRole('button', { name: 'Reload cooldown policy', exact: true }).waitFor();
    assert.equal(await scope.inputValue(), 'all', 'Discard did not restore saved scope');
    assert.deepEqual(errors, []);
    console.log('Provider scope, quota/temporary modes, duration validation, persistence, save/reload, and responsive layouts passed.');
  } finally { await browser.close(); await server.close(); }
})().catch(error => { console.error(error); process.exitCode = 1; });
