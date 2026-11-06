import { invoke } from '@tauri-apps/api/core';

export type CooldownPolicy = { providers: string[] | null; transientSeconds: number; persist: boolean };
export type CooldownPolicySettings = { policy: CooldownPolicy; availableProviders: string[]; warning: string | null };
export const getCooldownPolicy = () => invoke<CooldownPolicySettings>('get_cooldown_policy');
export const setCooldownPolicy = (policy: CooldownPolicy) => invoke<void>('set_cooldown_policy', { policy });

export const cooldownProviderLabel = (provider: string) => ({ antigravity: 'Antigravity', codex: 'Codex',
  claude: 'Claude', gemini: 'Gemini', 'gemini-cli': 'Gemini CLI', interactions: 'Gemini Interactions',
  'gemini-interactions': 'Gemini Interactions', xai: 'xAI', meta: 'Meta', 'vertex-compat': 'Vertex',
  vertex: 'Vertex' } as Record<string, string>)[provider] ?? provider;
