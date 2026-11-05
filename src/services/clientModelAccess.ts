import { invoke } from '@tauri-apps/api/core';

export const parseClientModelExclusions = (text: string): string[] =>
  [...new Set(text.split(/[\r\n,]+/).map(value => value.trim().toLowerCase()).filter(Boolean))];

export const getClientModelExclusions = (apiKey: string): Promise<string[]> =>
  invoke('get_core_api_key_model_exclusions', { apiKey });

export const setClientModelExclusions = (apiKey: string, excludedModels: string[]): Promise<void> =>
  invoke('set_core_api_key_model_exclusions', { apiKey, excludedModels });
