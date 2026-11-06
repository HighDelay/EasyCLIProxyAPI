import { afterEach, expect, test } from 'bun:test';
import { mockIPC, clearMocks } from '@tauri-apps/api/mocks';
import { getClientModelExclusions, parseClientModelExclusions, setClientModelExclusions,
  getClientModelChoices, includeSavedModelRules, setClientModelSelected, clientModelExcluded } from '../src/services/clientModelAccess';

afterEach(() => { if (typeof window !== 'undefined') clearMocks(); });
test('normalizes exclusion rules while preserving wildcard and literal punctuation', () => {
  expect(parseClientModelExclusions(' GPT-*\ngpt-* , model.v2\r\n  ')).toEqual(['gpt-*', 'model.v2']);
  expect(parseClientModelExclusions('')).toEqual([]);
});
test('collects OAuth and API key provider catalogs independently of client key permissions', async () => {
  const calls: string[] = [];
  const result = await getClientModelChoices({ get: async (path, query) => {
    calls.push(path);
    if (path === '/credentials') return { files: [
      { name: 'oauth.json', provider: 'codex' },
      { name: 'claude.json', provider: 'claude' },
    ] };
    if (path === '/config/api-keys') return { 'openai-compatibility': [{ name: 'openai-compatible', models: [{ name: 'gpt-test' }], keys: [{ 'api-key': 'fake-upstream-key' }] }] };
    return { models: query?.name === 'claude.json' ? [{ id: 'claude-sonnet' }]
      : [{ id: 'gpt-test', display_name: 'GPT Test' }] };
  } });
  expect(result.models).toEqual([
    { id: 'claude-sonnet', displayName: undefined, providers: ['claude'] },
    { id: 'gpt-test', displayName: 'GPT Test', providers: ['codex', 'openai-compatible'] },
  ]);
  expect(result.failedProviders).toBe(0);
  expect(calls).toEqual(['/credentials', '/config/api-keys', '/credentials/models', '/credentials/models']);
});
test('keeps successful provider catalogs when one credential fails', async () => {
  const result = await getClientModelChoices({ get: async (path, query) => {
    if (path === '/credentials') return { files: [{ name: 'good', type: 'gemini' }, { name: 'failed', provider: 'claude' }] };
    if (path === '/config/api-keys') return {};
    if (query?.name === 'failed') throw new Error('not available');
    return { models: [{ id: 'gemini-test' }] };
  } });
  expect(result.models[0]?.id).toBe('gemini-test');
  expect(result.failedProviders).toBe(1);
});
test('keeps OAuth models when the API provider endpoint fails', async () => {
  const result = await getClientModelChoices({ get: async path => {
    if (path === '/config/api-keys') throw new Error('HTTP 403');
    if (path === '/credentials') return { files: [{ name: 'codex.json', provider: 'codex' }] };
    return { models: [{ id: 'gpt-test' }] };
  } });
  expect(result.models.map(model => model.id)).toEqual(['gpt-test']);
  expect(result.failedProviders).toBe(1);
  expect(result.errors.join(' ')).toContain('HTTP 403');
});
test('keeps API key models when the OAuth endpoint fails', async () => {
  const result = await getClientModelChoices({ get: async path => {
    if (path === '/credentials') throw new Error('unavailable');
    return { 'openai-compatibility': [{ name: 'custom', models: [{ name: 'configured-model' }] }] };
  } });
  expect(result.models.map(model => model.id)).toEqual(['configured-model']);
  expect(result.failedProviders).toBe(1);
});
test('accepts wrapped API groups, null OAuth lists and legacy key entries', async () => {
  const result = await getClientModelChoices({ get: async path => path === '/credentials' ? { files: null }
    : { 'api-keys': { 'openai-compatibility': [{ name: 'custom', prefix: 'group',
      'api-key-entries': [{ prefix: '', models: [{ name: 'key-model' }] }] }] } } });
  expect(result.models.map(model => model.id)).toEqual(['key-model']);
  expect(result.failedProviders).toBe(0);
});
test('uses the live catalog if both management catalogs fail', async () => {
  const result = await getClientModelChoices({ get: async () => { throw new Error('management unavailable'); } },
    async () => ({ data: [{ id: 'gpt-6-astra', owned_by: 'openai' }, { id: 'claude-test', owned_by: 'antigravity' }], partial: false }));
  expect(result.models.map(model => model.id)).toEqual(['claude-test', 'gpt-6-astra']);
  expect(result.models[1]?.providers).toEqual(['openai']);
  expect(result.failedProviders).toBe(0);
  expect(result.errors.join(' ')).toContain('management unavailable');
});
test('retains successful models if the live fallback also fails', async () => {
  const result = await getClientModelChoices({ get: async path => {
    if (path === '/credentials') throw new Error('failed OAuth');
    return { 'openai-compatibility': [{ models: [{ name: 'still-available' }] }] };
  } }, async () => { throw new Error('live unavailable'); });
  expect(result.models.map(model => model.id)).toEqual(['still-available']);
  expect(result.errors.join(' ')).toContain('live unavailable');
});
test('flags potentially incomplete catalogs when every client key is restricted', async () => {
  const result = await getClientModelChoices({ get: async path => path === '/credentials' ? { files: [] } : {} },
    async () => ({ data: [{ id: 'allowed-model' }], partial: true }));
  expect(result.models.map(model => model.id)).toEqual(['allowed-model']);
  expect(result.failedProviders).toBe(1);
});
test('keeps saved literal exclusions selectable when live catalogs are unavailable', () => {
  const rows = includeSavedModelRules([], ['gpt-6-astra', 'gpt-5.6', 'gpt-6.1-terra', 'claude-*']);
  expect(rows.map(row => row.id)).toEqual(['gpt-6-astra', 'gpt-5.6', 'gpt-6.1-terra']);
  expect(rows.every(row => row.savedRule)).toBe(true);
  expect(includeSavedModelRules([{ id: 'GPT-6-astra', providers: ['codex'] }], ['gpt-6-astra'])).toHaveLength(1);
});
test('loads built-in provider defaults for API keys without explicit model lists', async () => {
  const calls: string[] = [];
  const result = await getClientModelChoices({ get: async path => {
    calls.push(path);
    if (path === '/credentials') return { files: [] };
    if (path === '/config/api-keys') return { gemini: [{ keys: [{ 'api-key': 'fake-key' }] }], interactions: [{ keys: [{ 'api-key': 'fake-interactions-key' }] }] };
    return { models: [{ id: 'gemini-api-key-model' }] };
  } });
  expect(result.models.map(model => model.id)).toEqual(['gemini-api-key-model']);
  expect(calls).toContain('/routing/model-definitions/gemini');
  expect(calls).toContain('/routing/model-definitions/gemini-interactions');
});
test('selection updates preserve wildcard rules and other selected models', () => {
  const selected = setClientModelSelected('gpt*\nprivate-model', 'claude-test', true);
  expect(parseClientModelExclusions(selected)).toEqual(['gpt*', 'private-model', 'claude-test']);
  expect(parseClientModelExclusions(setClientModelSelected(selected, 'claude-test', false))).toEqual(['gpt*', 'private-model']);
  expect(clientModelExcluded('GPT-6', ['gpt*'])).toBe(true);
  expect(clientModelExcluded('claude-test', ['gpt*'])).toBe(false);
});
test('addresses the selected client key and allows clearing its policy', async () => {
  const originalWindow = Object.getOwnPropertyDescriptor(globalThis, 'window');
  Object.defineProperty(globalThis, 'window', { value: {}, configurable: true });
  try {
    const calls: unknown[] = [];
    mockIPC((command, args) => { calls.push([command, args]); return command === 'get_core_api_key_model_exclusions' ? ['private-*'] : null; });
    expect(await getClientModelExclusions('test-client')).toEqual(['private-*']);
    await setClientModelExclusions('test-client', []);
    expect(calls).toEqual([
      ['get_core_api_key_model_exclusions', { apiKey: 'test-client' }],
      ['set_core_api_key_model_exclusions', { apiKey: 'test-client', excludedModels: [] }],
    ]);
  } finally {
    clearMocks();
    if (originalWindow) Object.defineProperty(globalThis, 'window', originalWindow);
    else Reflect.deleteProperty(globalThis, 'window');
  }
});
