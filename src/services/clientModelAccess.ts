import { invoke } from '@tauri-apps/api/core';
import { isRecord, managementApi, readString } from './managementApi';
import { modelMatchesRule } from './oauthModels';

export type ClientModelChoice = { id: string; displayName?: string; providers: string[] };
type CatalogApi = { get: (path: string, query?: Record<string, string>) => Promise<unknown> };

// Management catalogs include configured providers and are independent of
// restrictions on the client key being edited. No upstream inference is needed.
export async function getClientModelChoices(api: CatalogApi = managementApi): Promise<{
  models: ClientModelChoice[]; failedProviders: number;
}> {
  const [payload, configured] = await Promise.all([api.get('/credentials'), api.get('/config/api-keys')]);
  if (!isRecord(payload) || !Array.isArray(payload.files)) throw new Error('Invalid provider catalog response');
  if (!isRecord(configured)) throw new Error('Invalid API key provider catalog response');
  const sources = payload.files.filter(isRecord).flatMap(file => {
    const name = readString(file, 'name', 'id');
    return name ? [{ path: '/credentials/models', query: { name } as Record<string, string> | undefined, provider: readString(file, 'provider', 'type') || 'unknown' }] : [];
  });
  const models = new Map<string, ClientModelChoice>();
  const add = (id: string, provider: string, displayName?: string) => {
    if (!id) return;
    const key = id.toLowerCase();
    const model = models.get(key) ?? { id, displayName, providers: [] };
    model.displayName ??= displayName;
    if (!model.providers.includes(provider)) model.providers.push(provider);
    models.set(key, model);
  };
  const defaults = new Set<string>();
  for (const [provider, groups] of Object.entries(configured)) {
    if (!Array.isArray(groups)) continue;
    for (const group of groups.filter(isRecord)) {
      const label = provider === 'openai-compatibility' ? readString(group, 'name') || provider : provider;
      const keys = Array.isArray(group.keys) ? group.keys.filter(isRecord) : [];
      for (const key of keys.length ? keys : [{}]) {
        const definitions = key.models ?? group.models;
        const prefix = readString(key, 'prefix') || readString(group, 'prefix');
        if (Array.isArray(definitions) && definitions.length) {
          for (const definition of definitions) {
            const id = typeof definition === 'string' ? definition : isRecord(definition) ? readString(definition, 'name', 'id') : '';
            const alias = isRecord(definition) ? readString(definition, 'alias') : '';
            for (const name of new Set([id, alias].filter(Boolean))) {
              add(name, label);
              if (prefix) add(`${prefix}/${name}`, label);
            }
          }
        } else if (!defaults.has(provider) && provider !== 'openai-compatibility') {
          defaults.add(provider);
          const channel = ({ interactions: 'gemini-interactions', 'vertex-compat': 'vertex' } as Record<string, string>)[provider] ?? provider;
          sources.push({ path: `/routing/model-definitions/${encodeURIComponent(channel)}`, query: undefined, provider: label });
        }
      }
    }
  }
  const failed = new Set<string>();
  const pending = sources[Symbol.iterator]();
  await Promise.all(Array.from({ length: Math.min(6, sources.length) }, async () => {
    for (const source of pending) {
      try {
        const catalog = await api.get(source.path, source.query);
        if (!isRecord(catalog) || !Array.isArray(catalog.models)) throw new Error('Invalid model catalog');
        for (const item of catalog.models) {
          if (!isRecord(item)) continue;
          const id = readString(item, 'id', 'name');
          if (!id) continue;
          add(id, source.provider, readString(item, 'display_name', 'displayName') || undefined);
        }
      } catch { failed.add(source.provider); }
    }
  }));
  return {
    models: [...models.values()].map(model => ({ ...model, providers: model.providers.sort() }))
      .sort((a, b) => a.id.localeCompare(b.id)),
    failedProviders: failed.size,
  };
}

export const clientModelExcluded = (id: string, rules: string[]): boolean =>
  rules.some(rule => modelMatchesRule(id, rule));

export const setClientModelSelected = (text: string, id: string, excluded: boolean): string => {
  const rules = parseClientModelExclusions(text);
  return (excluded ? [...new Set([...rules, id.toLowerCase()])]
    : rules.filter(rule => rule !== id.toLowerCase())).join('\n');
};

export const parseClientModelExclusions = (text: string): string[] =>
  [...new Set(text.split(/[\r\n,]+/).map(value => value.trim().toLowerCase()).filter(Boolean))];

export const getClientModelExclusions = (apiKey: string): Promise<string[]> =>
  invoke('get_core_api_key_model_exclusions', { apiKey });

export const setClientModelExclusions = (apiKey: string, excludedModels: string[]): Promise<void> =>
  invoke('set_core_api_key_model_exclusions', { apiKey, excludedModels });
