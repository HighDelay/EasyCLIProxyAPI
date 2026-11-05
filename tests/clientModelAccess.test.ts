import { afterEach, expect, test } from 'bun:test';
import { mockIPC, clearMocks } from '@tauri-apps/api/mocks';
import { getClientModelExclusions, parseClientModelExclusions, setClientModelExclusions } from '../src/services/clientModelAccess';

afterEach(() => { if (typeof window !== 'undefined') clearMocks(); });
test('normalizes exclusion rules while preserving wildcard and literal punctuation', () => {
  expect(parseClientModelExclusions(' GPT-*\ngpt-* , model.v2\r\n  ')).toEqual(['gpt-*', 'model.v2']);
  expect(parseClientModelExclusions('')).toEqual([]);
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
