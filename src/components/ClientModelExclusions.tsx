import { useEffect, useState } from 'react';
import { LoaderCircle, RefreshCw, Search } from 'lucide-react';
import { useI18n } from '../i18n';
import { getClientModelChoices, includeSavedModelRules, parseClientModelExclusions, clientModelExcluded,
  setClientModelSelected, type ClientModelChoice } from '../services/clientModelAccess';
import { modelMatchesRule } from '../services/oauthModels';
import { Checkbox } from './Checkbox';
import { Notice } from './Notice';
import { Select } from './Select';
import './ClientModelExclusions.css';

type Props = { value: string; disabled: boolean; onChange: (value: string) => void };
const providerLabel = (provider: string) => ({ codex: 'Codex', claude: 'Claude', gemini: 'Gemini',
  antigravity: 'Antigravity', openai: 'OpenAI', xai: 'xAI', meta: 'Meta', deepseek: 'DeepSeek', vertex: 'Vertex',
  interactions: 'Gemini Interactions', 'gemini-interactions': 'Gemini Interactions',
  'openai-compatibility': 'OpenAI compatible' } as Record<string, string>)[provider] ?? provider;

export function ClientModelExclusions({ value, disabled, onChange }: Props) {
  const { t } = useI18n();
  const [catalog, setModels] = useState<ClientModelChoice[]>([]);
  const [loading, setLoading] = useState(true);
  const [failed, setFailed] = useState(false);
  const [errorDetail, setErrorDetail] = useState('');
  const [reload, setReload] = useState(0);
  const [search, setSearch] = useState('');
  const [provider, setProvider] = useState('');
  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setFailed(false);
    setErrorDetail('');
    void getClientModelChoices().then(result => {
      if (!cancelled) {
        setModels(current => result.models.length || !result.failedProviders ? result.models : current);
        setFailed(result.failedProviders > 0);
        setErrorDetail(result.errors[0] ?? '');
        setProvider(current => result.models.some(model => model.providers.includes(current)) ? current : '');
      }
    }).catch(error => { if (!cancelled) { setFailed(true); setErrorDetail(String(error)); } })
      .finally(() => { if (!cancelled) setLoading(false); });
    return () => { cancelled = true; };
  }, [reload]);
  const rules = parseClientModelExclusions(value);
  const models = includeSavedModelRules(catalog, rules);
  const query = search.trim().toLowerCase();
  const providers = [...new Set(models.flatMap(model => model.providers))].sort();
  const visible = models.filter(model => (!provider || model.providers.includes(provider))
    && `${model.id} ${model.displayName ?? ''} ${model.providers.join(' ')} ${model.providers.map(providerLabel).join(' ')}`.toLowerCase().includes(query));
  const selected = models.filter(model => clientModelExcluded(model.id, rules)).length;
  return <fieldset className="client-model-picker" disabled={disabled} aria-describedby="client-model-exclusions-hint">
    <legend>{t('config.keys.excludedModels')}</legend>
    <p id="client-model-exclusions-hint">{t('config.keys.modelsHint')}</p>
    <div className="client-model-picker-filters">
      <label className="search-control">
        <Search size={16} aria-hidden="true" />
        <input type="search" spellCheck={false} value={search} onChange={event => setSearch(event.currentTarget.value)}
          onKeyDown={event => { if (event.key === 'Enter') event.preventDefault(); }}
          aria-label={t('config.keys.modelsSearch')} placeholder={t('config.keys.modelsSearch')} />
      </label>
      <Select value={provider} onChange={event => setProvider(event.currentTarget.value)} aria-label={t('config.keys.modelsProvider')}>
        <option value="">{t('config.keys.modelsAllProviders')}</option>
        {providers.map(name => <option key={name} value={name}>{providerLabel(name)}</option>)}
      </Select>
      <button type="button" className="icon-button quiet" disabled={loading || disabled}
        onClick={() => setReload(version => version + 1)} title={t('common.refresh')} aria-label={t('common.refresh')}>
        <RefreshCw size={16} aria-hidden="true" />
      </button>
    </div>
    {failed ? <Notice tone="warn" variant="text" icon={false}>{t('config.keys.modelsLoadError')}</Notice> : null}
    {failed && errorDetail ? <p className="client-model-picker-error-detail">{errorDetail}</p> : null}
    <div className="client-model-picker-list" aria-busy={loading}>
      {loading ? <p className="client-model-picker-message"><LoaderCircle size={16} className="spin" aria-hidden="true" />{t('common.loading')}</p>
        : visible.length === 0 ? <p className="client-model-picker-message">{t(models.length ? 'config.keys.modelsNoMatch' : 'config.keys.modelsEmpty')}</p>
        : visible.map(model => {
          const pattern = rules.find(rule => rule.includes('*') && modelMatchesRule(model.id, rule));
          return <label key={model.id} className="client-model-picker-row" title={pattern ? t('config.keys.modelsPatternHint') : undefined}>
            <Checkbox checked={clientModelExcluded(model.id, rules)} disabled={disabled || Boolean(pattern)}
              aria-label={model.id} onChange={event => onChange(setClientModelSelected(value, model.id, event.currentTarget.checked))} />
            <span className="client-model-picker-name" title={model.displayName}>{model.id}</span>
            <span className="client-model-picker-provider">{model.savedRule ? t('config.keys.modelsSavedRule') : model.providers.map(providerLabel).join(' · ')}{pattern ? <small>{pattern}</small> : null}</span>
          </label>;
        })}
    </div>
    <div className="client-model-picker-summary" aria-live="polite">
      <span>{t('config.keys.modelsSelected', { selected, total: models.length })}</span>
      <button type="button" className="secondary-button client-model-clear" disabled={disabled || rules.length === 0} onClick={() => onChange('')}>{t('config.keys.modelsClear')}</button>
    </div>
    {rules.some(rule => rule.includes('*')) ? <p>{t('config.keys.modelsPatternHint')}</p> : null}
    <label className="client-model-picker-rules">
      <span id="client-model-exclusions-label">{t('config.keys.modelsCustomRules')}</span>
      <textarea className="config-dialog-text-input" spellCheck={false} rows={3} value={value}
        aria-labelledby="client-model-exclusions-label" placeholder={'private-model\ngpt*'}
        onChange={event => onChange(event.currentTarget.value)} />
      <small>{t('config.keys.excludedModelsHint')}</small>
    </label>
  </fieldset>;
}
