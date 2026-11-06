# Cooldown policy

Open **Advanced Features → Routing & reliability**. Enable **Enable cooldowns**
under **Retries & cooldowns** and save. The **Cooldown policy** card controls:

- **All providers** or **Selected providers**, including Antigravity and custom
  OpenAI-compatible providers configured in API Access.
- **Quota and credential errors**: keeps the core's quota, authentication, and
  unsupported-model handling while disabling its general transient-error
  cooldown. Provider-specific rate-limit backoff can still apply.
- **Also include temporary failures**: adds cooldowns for retryable failures
  such as timeouts and HTTP 503. Set a duration; `0` uses 60 seconds. A provider's
  retry delay can take precedence. This error policy is shared by providers
  using cooldowns.
- **Remember cooldowns across core restarts**: uses the core's existing `.cds`
  persistence next to OAuth credentials.

For Antigravity, select it and use **Quota and credential errors**. The supplied
HTTP 429 response has `ErrorInfo.reason: QUOTA_EXHAUSTED`; the core uses its
reset timestamp/duration and `RetryInfo.retryDelay`. After the first failure,
the affected account/model remains unavailable until recovery and another
eligible account is selected. The app does not treat every 429 as this exact
quota failure or guess eligibility from a displayed quota percentage.

New installations enable cooldowns and use the quota-focused error mode.
Existing explicit choices, including disabled cooldowns and custom transient
durations, are preserved. Account and API-key overrides take precedence over
the provider defaults. Choose inheritance in the individual credential editor
to follow the provider selection.

Provider defaults apply before core startup, immediately to files imported
through the app, and within approximately two seconds to new OAuth logins or
externally added credentials while the desktop app manages the running core.
Already exhausted accounts cannot be skipped until the core has observed a
quota error for them. All exhausted accounts can still be tried once initially.
The controls do not poll providers or send inference requests to check quota.

## Implementation and upstream merges

The official core remains unchanged. The app layers provider defaults using
the core's existing `disable_cooling` OAuth metadata and API group/key
`disable-cooling` inheritance. Only defaults marked
`_easycli_cooldown_managed` are removed or updated later. Explicit overrides are
preserved; credential and API-group editors show managed defaults as inherited.
Live OAuth updates use the core's atomic metadata PATCH, preserving concurrent
token refreshes. Offline replacements preserve existing file permissions.

The provider selection is stored at
`plugins.configs.model-access.cooldown-providers` in core YAML. This supported
opaque namespace survives the core's v8 migration, unlike unknown top-level
fields. Storing the selection does not enable the plugin. The shared error
mode maps to `routing.cooldown.transient-error-cooldown-seconds` (`-1` for
quota-focused), and persistence uses `routing.cooldown.save-cooldown-status`.

The fork's main-branch workflow packages Windows and Linux after upstream
merges and runs regression tests against the bundled core. Locally:

```powershell
bun run check
bun test tests/authFileSettings.test.ts tests/providerTemplate.test.ts
bun run test:core:cooldown-policy
bun run test:ui:cooldown-policy
```

The core contract test uses fake local Antigravity accounts and the recorded
quota-response fixture. It checks failover, the reset delay, a transient 503,
restart persistence, provider selection, and live metadata updates without
using production accounts. UI tests require Playwright and a Chromium browser;
set `PLAYWRIGHT_MODULE` if Playwright is installed outside the repository.
