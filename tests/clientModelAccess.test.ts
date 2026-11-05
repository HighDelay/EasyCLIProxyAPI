import { afterEach, expect, test } from 'bun:test';
import { mockIPC, clearMocks } from '@tauri-apps/api/mocks';
import { getClientModelExclusions, parseClientModelExclusions, setClientModelExclusions,
  getClientModelChoices, setClientModelSelected, clientModelExcluded } from '../src/services/clientModelAccess';

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
