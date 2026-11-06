import { invoke } from '@tauri-apps/api/core';
import { isRecord, managementApi, readString } from './managementApi';
import { modelMatchesRule } from './oauthModels';

export type ClientModelChoice = { id: string; displayName?: string; providers: string[]; savedRule?: boolean };
type CatalogApi = { get: (path: string, query?: Record<string, string>) => Promise<unknown> };

// Management catalogs include configured providers and are independent of
// restrictions on the client key being edited. No upstream inference is needed.
export async function getClientModelChoices(api: CatalogApi = managementApi, liveCatalog?: () => Promise<unknown>): Promise<{
  models: ClientModelChoice[]; failedProviders: number; errors: string[];
}> {
  const failed = new Map<string, string>();
  let recovered = false;
  const fail = (source: string, reason: unknown) => failed.set(source, `${source}: ${String(reason).slice(0, 400)}`);
  const [credentials, apiKeys] = await Promise.allSettled([api.get('/credentials'), api.get('/config/api-keys')]);
  const payload = credentials.status === 'fulfilled' ? credentials.value : undefined;
  const keyPayload = apiKeys.status === 'fulfilled' ? apiKeys.value : undefined;
  const files = isRecord(payload) && Array.isArray(payload.files) ? payload.files : [];
  if (credentials.status === 'rejected') fail('OAuth accounts', credentials.reason);
  else if (!isRecord(payload) || (!Array.isArray(payload.files) && payload.files !== null)) fail('OAuth accounts', 'Invalid credential catalog response');
  const configured = isRecord(keyPayload) ? (isRecord(keyPayload['api-keys']) ? keyPayload['api-keys'] : keyPayload) : {};
  if (apiKeys.status === 'rejected') fail('API key providers', apiKeys.reason);
  else if (!isRecord(keyPayload)) fail('API key providers', 'Invalid provider catalog response');
  const sources = files.filter(isRecord).flatMap(file => {
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
      const rawKeys = Array.isArray(group.keys) ? group.keys : Array.isArray(group['api-key-entries']) ? group['api-key-entries'] : [];
      const keys = rawKeys.filter(isRecord);
      for (const key of keys.length ? keys : [{}]) {
        const definitions = key.models ?? group.models;
        const prefix = typeof key.prefix === 'string' ? key.prefix.trim() : readString(group, 'prefix');
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
  const pending = sources[Symbol.iterator]();
  await Promise.all(Array.from({ length: Math.min(6, sources.length) }, async () => {
    for (const source of pending) {
      try {
        const catalog = await api.get(source.path, source.query);
        const entries = isRecord(catalog) && (Array.isArray(catalog.models) ? catalog.models : Array.isArray(catalog.data) ? catalog.data : catalog.models === null ? [] : null);
        if (!entries) throw new Error('Invalid model catalog');
        for (const item of entries) {
          if (typeof item === 'string') { add(item, source.provider); continue; }
          if (!isRecord(item)) continue;
          const id = readString(item, 'id', 'name');
          if (!id) continue;
          add(id, source.provider, readString(item, 'display_name', 'displayName') || undefined);
        }
      } catch (error) { fail(source.provider, error); }
    }
  }));
  // /v1/models remains usable when management authentication or provider
  // detail endpoints fail. Use installed server settings and a suitable key.
  const fallback = liveCatalog ?? (api === managementApi ? () => invoke<unknown>('get_client_model_catalog') : undefined);
  if (fallback && (failed.size > 0 || models.size === 0)) {
    try {
      const live = await fallback();
      if (!isRecord(live) || !Array.isArray(live.data)) throw new Error('Invalid live model catalog');
      for (const item of live.data) {
        if (!isRecord(item)) continue;
        const id = readString(item, 'id', 'name');
        if (id) add(id, readString(item, 'owned_by', 'provider') || 'live', readString(item, 'display_name', 'displayName') || undefined);
      }
      recovered = live.partial === false && live.data.length > 0;
      if (live.partial === true) fail('Live catalog', 'All client keys have exclusions; some blocked models may be absent');
    } catch (error) { fail('Live catalog', error); }
  }
  return {
    models: [...models.values()].map(model => ({ ...model, providers: model.providers.sort() }))
      .sort((a, b) => a.id.localeCompare(b.id)),
    failedProviders: recovered ? 0 : failed.size,
    errors: [...failed.values()],
  };
}

export function includeSavedModelRules(models: ClientModelChoice[], rules: string[]): ClientModelChoice[] {
  const known = new Set(models.map(model => model.id.toLowerCase()));
  return [...models, ...rules.filter(rule => !rule.includes('*') && !known.has(rule.toLowerCase()))
    .map(id => ({ id, providers: [], savedRule: true }))];
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
