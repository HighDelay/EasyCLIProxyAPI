import { useEffect, useRef, useState } from 'react';
import { Check, RefreshCw, ShieldCheck } from 'lucide-react';
import { useI18n } from '../../i18n';
import { getCooldownPolicy, setCooldownPolicy, cooldownProviderLabel, type CooldownPolicy } from '../../services/cooldownPolicy';
import { Checkbox } from '../Checkbox';
import { Notice } from '../Notice';
import { Select } from '../Select';
import './CooldownPolicySettings.css';

export function CooldownPolicySettings({ hidden, disabled }: { hidden: boolean; disabled: boolean }) {
  const { t } = useI18n();
  const [original, setOriginal] = useState<CooldownPolicy | null>(null);
  const [draft, setDraft] = useState<CooldownPolicy | null>(null);
  const [providers, setProviders] = useState<string[]>([]);
  const [duration, setDuration] = useState('0');
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState('');
  const [warning, setWarning] = useState('');
  const [saved, setSaved] = useState(false);
  const [reload, setReload] = useState(0);
  const dirtyRef = useRef(false);
  useEffect(() => {
    if (hidden || dirtyRef.current) return;
    let cancelled = false;
    setLoading(true);
    setError('');
    setSaved(false);
    void getCooldownPolicy().then(result => {
      if (cancelled) return;
      setOriginal(result.policy);
      setDraft(result.policy);
      setProviders(result.availableProviders);
      setDuration(String(Math.max(0, result.policy.transientSeconds)));
      setWarning(result.warning ?? '');
    }).catch(reason => { if (!cancelled) { setOriginal(null); setDraft(null); setError(String(reason)); } })
      .finally(() => { if (!cancelled) setLoading(false); });
    return () => { cancelled = true; };
  }, [hidden, reload]);
  const temporary = draft != null && draft.transientSeconds >= 0;
  const validDuration = /^\d+$/.test(duration) && Number(duration) <= 2147483647;
  const next = draft && { ...draft, transientSeconds: temporary ? Number(duration) : -1 };
  const dirty = original != null && JSON.stringify(original) !== JSON.stringify(next);
  dirtyRef.current = dirty;
  const locked = disabled || loading || saving;
  const change = (value: Partial<CooldownPolicy>) => {
    if (draft) setDraft({ ...draft, ...value });
    setSaved(false);
  };
  const save = async () => {
    if (!next || (temporary && !validDuration)) return;
    setSaving(true);
    setError('');
    setSaved(false);
    try {
      await setCooldownPolicy(next);
      setDraft(next);
      setOriginal(next);
      setWarning('');
      setSaved(true);
    } catch (reason) { setError(String(reason)); }
    finally { setSaving(false); }
  };
  return <section hidden={hidden} id="config-cooldown-policy" className="surface-card config-network-section cooldown-policy"
    aria-labelledby="cooldown-policy-title" aria-busy={loading}>
    <div className="config-network-section-heading config-section-heading-with-actions">
      <div className="config-network-section-title"><ShieldCheck size={16} aria-hidden="true" />
        <h3 id="cooldown-policy-title">{t('config.cooldown.title')}</h3></div>
      <div className="cooldown-policy-actions"><span className="config-card-status" aria-live="polite">{dirty ? t('config.network.unsaved') : ''}</span>
      <button type="button" className="icon-button quiet" aria-label={t(dirty ? 'config.cooldown.discard' : 'config.cooldown.reload')}
        title={t(dirty ? 'config.cooldown.discard' : 'config.cooldown.reload')} disabled={loading || saving} onClick={() => {
          setDraft(original);
          setDuration(String(Math.max(0, original?.transientSeconds ?? 0)));
          dirtyRef.current = false;
          setReload(value => value + 1);
        }}><RefreshCw size={16} aria-hidden="true" /></button></div>
    </div>
    <p className="cooldown-policy-hint">{t('config.cooldown.hint')}</p>
    {error && <Notice tone="error">{error}</Notice>}
    {warning && <Notice tone="warn">{warning}</Notice>}
    {saved && <Notice tone="success">{t('config.cooldown.saved')}</Notice>}
    {loading ? <Notice tone="info" variant="text" icon={false}>{t('common.loading')}</Notice> : draft && <>
      <div className="cooldown-policy-fields">
        <label className="cooldown-policy-field"><span>{t('config.cooldown.scope')}</span>
          <Select className="config-network-input" value={draft.providers === null ? 'all' : 'selected'} disabled={locked}
            onChange={event => change({ providers: event.currentTarget.value === 'all' ? null : ['antigravity'] })}>
            <option value="all">{t('config.cooldown.all')}</option><option value="selected">{t('config.cooldown.selected')}</option>
          </Select></label>
        <label className="cooldown-policy-field"><span>{t('config.cooldown.errors')}</span>
          <Select className="config-network-input" value={temporary ? 'temporary' : 'quota'} disabled={locked}
            onChange={event => change({ transientSeconds: event.currentTarget.value === 'quota' ? -1 : validDuration ? Number(duration) : 0 })}>
            <option value="quota">{t('config.cooldown.quota')}</option><option value="temporary">{t('config.cooldown.temporary')}</option>
          </Select></label>
      </div>
      {draft.providers !== null && <fieldset className="cooldown-provider-list" disabled={locked}>
        <legend>{t('config.cooldown.providers')}</legend>
        <div>{providers.map(provider => <Checkbox key={provider} label={cooldownProviderLabel(provider)}
          checked={draft.providers!.includes(provider)} onChange={event => change({ providers: event.currentTarget.checked
            ? [...draft.providers!, provider].sort() : draft.providers!.filter(value => value !== provider) })} />)}</div>
        {!draft.providers.length && <p className="cooldown-policy-hint">{t('config.cooldown.none')}</p>}
      </fieldset>}
      <p className="cooldown-policy-hint">{t(temporary ? 'config.cooldown.temporaryHint' : 'config.cooldown.quotaHint')}</p>
      {temporary && <label className="cooldown-policy-field cooldown-duration"><span id="cooldown-duration-label">{t('config.cooldown.duration')}</span>
        <input className="config-network-input" type="text" inputMode="numeric" value={duration} disabled={locked}
          aria-labelledby="cooldown-duration-label" aria-invalid={!validDuration} aria-describedby="cooldown-duration-hint" onChange={event => { setDuration(event.currentTarget.value); setSaved(false); }} />
        <small id="cooldown-duration-hint">{t(validDuration ? 'config.cooldown.durationHint' : 'config.cooldown.invalidDuration')}</small>
      </label>}
      <Checkbox label={t('config.cooldown.persist')} checked={draft.persist} disabled={locked}
        onChange={event => change({ persist: event.currentTarget.checked })} />
      <p className="cooldown-policy-hint">{t('config.cooldown.overrides')}</p>
      <div className="config-settings-save-row"><button type="button" className="primary-button compact-button"
        disabled={locked || !dirty || (temporary && !validDuration)} onClick={() => void save()}>
        <Check size={16} aria-hidden="true" />{t(saving ? 'common.saving' : 'common.save')}
      </button></div>
    </>}
  </section>;
}
