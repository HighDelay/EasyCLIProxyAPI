const { chromium } = require(process.env.PLAYWRIGHT_MODULE || 'playwright');
const assert = require('node:assert/strict');
const path = require('node:path');
const fs = require('node:fs/promises');

(async () => {
  const { createServer } = await import('vite');
  const react = (await import('@vitejs/plugin-react')).default;
  const server = await createServer({ configFile: false, root: path.resolve(__dirname, '..'), plugins: [react()],
    logLevel: 'error', server: { host: '127.0.0.1', port: 0, watch: { ignored: ['**/src-tauri/**', '**/bin-work/**'] } } });
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
    let exclusions = dialog.getByRole('textbox', { name: 'Model names and wildcard rules', exact: true });
    await exclusions.waitFor();
    const model = id => dialog.getByRole('checkbox', { name: id, exact: true });
    await model('gpt-5.2-codex').check();
    await model('claude-sonnet-4-6').check();
    const provider = dialog.getByRole('combobox', { name: 'Filter by provider', exact: true });
    await provider.selectOption('codex');
    assert.equal(await model('claude-sonnet-4-6').count(), 0, 'Provider filter did not narrow the list');
    await provider.selectOption('');
    assert.equal(await model('claude-sonnet-4-6').isChecked(), true, 'Provider filtering lost a selection');
    const search = dialog.getByRole('searchbox', { name: 'Search models or providers', exact: true });
    await search.fill('claude');
    await search.press('Enter');
    assert.equal(await dialog.isVisible(), true, 'Searching submitted the key editor');
    assert.equal(await model('gpt-5.2-codex').count(), 0, 'Search did not filter models');
    await search.fill('');
    await dialog.getByRole('button', { name: 'Save', exact: true }).click();
    await dialog.waitFor({ state: 'hidden' });
    await edit(1).click();
    dialog = page.getByRole('dialog');
    exclusions = dialog.getByRole('textbox', { name: 'Model names and wildcard rules', exact: true });
    await exclusions.waitFor();
    await page.waitForFunction(() => document.querySelector('textarea[aria-labelledby="client-model-exclusions-label"]')?.value === 'gpt-5.2-codex\nclaude-sonnet-4-6');
    await model('gpt-5.2-codex').waitFor();
    assert.equal(await model('gpt-5.2-codex').isChecked(), true, 'Model selection did not survive save');
    assert.equal(await model('claude-sonnet-4-6').isChecked(), true, 'Cross-provider selection did not survive save');
    await exclusions.fill('gpt*\nprivate-model\npremium-*');
    assert.equal(await model('gpt-5.2-codex').isChecked(), true, 'Wildcard did not select matching models');
    assert.equal(await model('gpt-5.2-codex').isDisabled(), true, 'Wildcard selection was shown as individually editable');
    assert.equal(await model('claude-sonnet-4-6').isChecked(), false, 'Custom rules did not update the checklist');
    for (const width of [1280, 768, 390]) {
      await page.setViewportSize({ width, height: 900 });
      await dialog.locator('.client-api-key-dialog-body').evaluate(element => { element.scrollTop = 0; });
      await page.evaluate(() => Promise.all(document.getAnimations()
        .filter(animation => Number.isFinite(animation.effect?.getComputedTiming().endTime))
        .map(animation => animation.finished.catch(() => {}))));
      const box = await exclusions.boundingBox();
      assert(box && box.x >= 0 && box.x + box.width <= width, `Model rules overflow at ${width}px`);
      const bounds = await dialog.boundingBox();
      const save = await dialog.getByRole('button', { name: 'Save', exact: true }).boundingBox();
      assert(bounds && save && save.y + save.height <= bounds.y + bounds.height, `Save escapes dialog at ${width}px`);
      await page.screenshot({ path: path.join(output, `edit-key-${width}.png`) });
      if (width === 1280) {
        const snapshot = await page.evaluate(() => {
          const root = document.documentElement.cloneNode(true);
          const inputs = [...document.querySelectorAll('input')];
          root.querySelectorAll('input').forEach((copy, index) => {
            copy.toggleAttribute('checked', inputs[index].checked);
            copy.setAttribute('value', inputs[index].value);
          });
          const textareas = [...document.querySelectorAll('textarea')];
          root.querySelectorAll('textarea').forEach((copy, index) => { copy.textContent = textareas[index].value; });
          root.querySelectorAll('script').forEach(script => script.remove());
          return '<!doctype html>' + root.outerHTML;
        });
        await fs.writeFile(path.join(output, 'editor.html'), snapshot);
      }
    }
    await page.setViewportSize({ width: 640, height: 600 });
    const compactSave = await dialog.getByRole('button', { name: 'Save', exact: true }).boundingBox();
    assert(compactSave && compactSave.y + compactSave.height <= 600, 'Save is clipped at the minimum desktop window size');
    await page.setViewportSize({ width: 1280, height: 900 });
    await dialog.getByRole('button', { name: 'Close', exact: true }).click();
    await edit(2).click();
    dialog = page.getByRole('dialog');
    exclusions = dialog.getByRole('textbox', { name: 'Model names and wildcard rules', exact: true });
    await page.waitForFunction(() => document.querySelector('textarea[aria-labelledby="client-model-exclusions-label"]')?.disabled === false);
    assert.equal(await exclusions.inputValue(), '', 'Rules leaked to another key');
    await dialog.getByRole('button', { name: 'Close', exact: true }).click();
    await page.evaluate(() => {
      const native = window.__TAURI_INTERNALS__;
      const original = native.invoke.bind(native);
      window.__catalogFailure = 'management';
      native.invoke = (command, args, ...rest) => {
        if (command === 'management_request' && ['/credentials', '/config/api-keys'].includes(args?.request?.path)) {
          return Promise.reject(new Error('Management catalog unavailable (test)'));
        }
        if (command === 'get_client_model_catalog' && window.__catalogFailure === 'all') {
          return Promise.reject(new Error('Live catalog unavailable (test)'));
        }
        return original(command, args, ...rest);
      };
    });
    await edit(1).click();
    dialog = page.getByRole('dialog');
    await dialog.getByRole('checkbox', { name: 'claude-sonnet-4-6', exact: true }).waitFor();
    assert.equal(await dialog.locator('.client-model-picker-error-detail').count(), 0, 'A recovered catalog still showed a loading error');
    await dialog.getByRole('button', { name: 'Close', exact: true }).click();
    await page.evaluate(async () => {
      const native = window.__TAURI_INTERNALS__;
      const config = await native.invoke('get_core_config_settings');
      await native.invoke('set_core_api_key_model_exclusions', { apiKey: config.apiKeys[1].apiKey,
        excludedModels: ['gpt-6-astra', 'gpt-5.6', 'gpt-6.1-terra'] });
      window.__catalogFailure = 'all';
    });
    await edit(2).click();
    dialog = page.getByRole('dialog');
    for (const id of ['gpt-6-astra', 'gpt-5.6', 'gpt-6.1-terra']) {
      const checkbox = dialog.getByRole('checkbox', { name: id, exact: true });
      await checkbox.waitFor();
      assert.equal(await checkbox.isChecked(), true, `Saved exclusion ${id} disappeared when catalogs failed`);
    }
    assert.equal(await dialog.getByText('Saved exclusion', { exact: true }).count(), 3);
    await page.setViewportSize({ width: 640, height: 600 });
    const failureSave = await dialog.getByRole('button', { name: 'Save', exact: true }).boundingBox();
    assert(failureSave && failureSave.y + failureSave.height <= 600, 'Catalog errors clipped Save at the minimum window size');
    await page.setViewportSize({ width: 1280, height: 900 });
    await page.screenshot({ path: path.join(output, 'saved-exclusions-during-failure.png') });
    assert.deepEqual(errors, []);
    console.log('Model selection, key isolation, live fallback, saved exclusions during catalog failures, and 1280/768/390px layouts passed.');
  } finally { await browser.close(); await server.close(); }
})().catch(error => { console.error(error); process.exitCode = 1; });
