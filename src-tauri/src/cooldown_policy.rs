//! Provider defaults layered over the core's own quota/error classification.
//! Only overrides carrying our marker are changed. Explicit account/key choices win.
use crate::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

// Plugin configuration is an upstream-supported opaque namespace. Unknown
// top-level fields are commented out by the core's v8 migration.
const PROVIDERS_PATH: &[&str] = &["plugins", "configs", "model-access", "cooldown-providers"];
const MARKER: &str = "_easycli_cooldown_managed";
const COOLDOWN: &[&str] = &["routing", "cooldown"];
static LAST_ERROR: Mutex<Option<String>> = Mutex::new(None);
static CREDENTIAL_UPDATES: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

pub(crate) async fn lock_credential_updates() -> tokio::sync::MutexGuard<'static, ()> {
    CREDENTIAL_UPDATES.lock().await
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CooldownPolicy {
    /// None applies the global default to every provider; empty selects none.
    pub providers: Option<Vec<String>>,
    pub transient_seconds: i32,
    pub persist: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CooldownPolicySettings {
    policy: CooldownPolicy,
    available_providers: Vec<String>,
    warning: Option<String>,
}

fn yaml_at<'a>(
    document: &'a serde_norway::Value,
    path: &[&str],
) -> Option<&'a serde_norway::Value> {
    document
        .as_mapping()
        .and_then(|root| nested_yaml_value(root, path))
}

fn policy_from(document: &serde_norway::Value) -> Result<CooldownPolicy, String> {
    let providers = match yaml_at(document, PROVIDERS_PATH) {
        None | Some(serde_norway::Value::Null) => None,
        Some(value) => Some(
            serde_norway::from_value::<Vec<String>>(value.clone())
                .map_err(|_| "Invalid cooldown provider selection".to_string())?,
        ),
    };
    let read = |name| {
        yaml_at(document, &[COOLDOWN[0], COOLDOWN[1], name]).or_else(|| yaml_at(document, &[name]))
    };
    let transient_seconds = match read("transient-error-cooldown-seconds") {
        None => DEFAULT_TRANSIENT_ERROR_COOLDOWN_SECONDS as i32,
        Some(value) => value
            .as_i64()
            .and_then(|v| i32::try_from(v).ok())
            .filter(|v| *v >= -1)
            .ok_or("Invalid transient error cooldown duration")?,
    };
    let persist = match read("save-cooldown-status") {
        None => false,
        Some(value) => value
            .as_bool()
            .ok_or("Invalid cooldown persistence setting")?,
    };
    normalize(CooldownPolicy {
        providers,
        transient_seconds,
        persist,
    })
}

fn normalize(mut policy: CooldownPolicy) -> Result<CooldownPolicy, String> {
    if policy.transient_seconds < -1 {
        return Err("Cooldown duration must be -1 or greater".into());
    }
    if let Some(providers) = &mut policy.providers {
        if providers.len() > 256 {
            return Err("Select at most 256 providers".into());
        }
        for provider in providers.iter_mut() {
            *provider = provider.trim().to_lowercase();
            if provider.is_empty() || provider.len() > 200 || provider.chars().any(char::is_control)
            {
                return Err("Invalid cooldown provider name".into());
            }
        }
        providers.sort();
        providers.dedup();
    }
    Ok(policy)
}

fn disabled_for(policy: &CooldownPolicy, provider: &str) -> bool {
    policy.providers.as_ref().is_some_and(|providers| {
        !providers
            .iter()
            .any(|p| p.eq_ignore_ascii_case(provider.trim()))
    })
}

fn credential_provider(metadata: &serde_json::Value) -> Option<String> {
    let provider = metadata.get("type")?.as_str()?.trim().to_lowercase();
    if provider.is_empty() {
        return None;
    }
    // This is the core's OAuth file-provider mapping, distinct from API keys.
    Some(if provider == "gemini" {
        "gemini-cli".into()
    } else {
        provider
    })
}

fn explicit_json_bool(value: Option<&serde_json::Value>) -> bool {
    value.is_some_and(|value| !value.is_null())
}

/// A partial metadata patch. Never rewrites tokens in a running core.
fn credential_patch(
    metadata: &serde_json::Value,
    policy: &CooldownPolicy,
) -> serde_json::Map<String, serde_json::Value> {
    use serde_json::{json, Value};
    let mut patch = serde_json::Map::new();
    let Some(provider) = credential_provider(metadata) else {
        return patch;
    };
    let owned = metadata.get(MARKER) == Some(&Value::Bool(true));
    let explicit_legacy = explicit_json_bool(metadata.get("disable-cooling"));
    let current = metadata.get("disable_cooling");
    if owned && (current != Some(&Value::Bool(true)) || explicit_legacy) {
        // A direct edit takes ownership back from the provider default.
        patch.insert(MARKER.into(), Value::Null);
        if explicit_legacy && current == Some(&Value::Bool(true)) {
            patch.insert("disable_cooling".into(), Value::Null);
        }
    } else if owned && !disabled_for(policy, &provider) {
        patch.insert("disable_cooling".into(), Value::Null);
        patch.insert(MARKER.into(), Value::Null);
    } else if !owned
        && !explicit_legacy
        && !explicit_json_bool(current)
        && disabled_for(policy, &provider)
    {
        patch.insert("disable_cooling".into(), json!(true));
        patch.insert(MARKER.into(), json!(true));
    }
    patch
}

fn apply_json_patch(
    metadata: &mut serde_json::Value,
    patch: serde_json::Map<String, serde_json::Value>,
) {
    if let Some(object) = metadata.as_object_mut() {
        for (key, value) in patch {
            if value.is_null() {
                object.remove(&key);
            } else {
                object.insert(key, value);
            }
        }
    }
}

fn update_group(group: &mut serde_norway::Value, provider: &str, policy: &CooldownPolicy) -> bool {
    use serde_norway::Value;
    let Some(group) = group.as_mapping_mut() else {
        return false;
    };
    let flag = Value::String("disable-cooling".into());
    let marker = Value::String(MARKER.into());
    let owned = group.get(&marker) == Some(&Value::Bool(true));
    let value = group.get(&flag).filter(|v| !v.is_null());
    if owned && value != Some(&Value::Bool(true)) {
        group.remove(&marker);
        true
    } else if owned && !disabled_for(policy, provider) {
        group.remove(&flag);
        group.remove(&marker);
        true
    } else if !owned && value.is_none() && disabled_for(policy, provider) {
        group.insert(flag, Value::Bool(true));
        group.insert(marker, Value::Bool(true));
        true
    } else {
        false
    }
}

/// v8 groups inherit this default; explicit per-key values remain untouched.
fn update_api_groups(document: &mut serde_norway::Value, policy: &CooldownPolicy) -> bool {
    let mut changed = false;
    if let Some(groups) = document
        .get_mut("api-keys")
        .and_then(serde_norway::Value::as_mapping_mut)
    {
        for (section, entries) in groups.iter_mut() {
            let Some(section) = section.as_str() else {
                continue;
            };
            let Some(entries) = entries.as_sequence_mut() else {
                continue;
            };
            for entry in entries {
                let provider = if section == "openai-compatibility" {
                    entry
                        .get("name")
                        .and_then(serde_norway::Value::as_str)
                        .unwrap_or(section)
                } else {
                    section
                }
                .to_string();
                changed |= update_group(entry, &provider, policy);
            }
        }
    }
    // Legacy configurations are supported until the next v8 migration.
    for (section, provider) in [
        ("gemini-api-key", "gemini"),
        ("interactions-api-key", "interactions"),
        ("claude-api-key", "claude"),
        ("codex-api-key", "codex"),
        ("xai-api-key", "xai"),
        ("meta-api-key", "meta"),
        ("vertex-api-key", "vertex"),
        ("openai-compatibility", "openai-compatibility"),
    ] {
        if let Some(entries) = document
            .get_mut(section)
            .and_then(serde_norway::Value::as_sequence_mut)
        {
            for entry in entries {
                let name = if provider == "openai-compatibility" {
                    entry
                        .get("name")
                        .and_then(serde_norway::Value::as_str)
                        .unwrap_or(provider)
                } else {
                    provider
                }
                .to_string();
                changed |= update_group(entry, &name, policy);
            }
        }
    }
    changed
}

fn read_document(path: &Path) -> Result<serde_norway::Value, String> {
    serde_norway::from_str(
        &fs::read_to_string(path)
            .map_err(|e| format!("Failed to read cooldown configuration: {e}"))?,
    )
    .map_err(|e| format!("Failed to parse cooldown configuration: {e}"))
}

fn credential_files(config: &GuiConfigFile, install_dir: &Path) -> Result<Vec<PathBuf>, String> {
    let directory = auth_dir_path_for_core(&config.auth_dir, install_dir)?;
    if !directory.is_dir() {
        return Ok(Vec::new());
    }
    fs::read_dir(&directory)
        .map_err(|e| format!("Failed to read credentials directory: {e}"))?
        .filter_map(|entry| match entry {
            Ok(entry)
                if entry
                    .path()
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("json"))
                    && entry.path().is_file() =>
            {
                Some(Ok(entry.path()))
            }
            Ok(_) => None,
            Err(e) => Some(Err(format!("Failed to list credentials: {e}"))),
        })
        .collect()
}

fn credential_metadata(path: &Path) -> Result<Option<serde_json::Value>, String> {
    let bytes = fs::read(path)
        .map_err(|e| format!("Failed to read credential {}: {e}", path_to_string(path)))?;
    // The core ignores non-credential/malformed JSON files in this directory too.
    Ok(serde_json::from_slice::<serde_json::Value>(&bytes)
        .ok()
        .filter(|v| v.is_object()))
}

pub(crate) fn prepare_import(data: Vec<u8>) -> Result<Vec<u8>, String> {
    let path = core_install_dir()?.join(CORE_CONFIG_FILE);
    if !path.is_file() {
        return Ok(data);
    }
    let policy = policy_from(&read_document(&path)?)?;
    let Ok(mut metadata) = serde_json::from_slice(&data) else {
        return Ok(data);
    };
    let patch = credential_patch(&metadata, &policy);
    if patch.is_empty() {
        return Ok(data);
    }
    apply_json_patch(&mut metadata, patch);
    serde_json::to_vec_pretty(&metadata).map_err(|e| e.to_string())
}

pub(crate) fn reconcile_for_start(
    config_path: &Path,
    config: &GuiConfigFile,
) -> Result<(), String> {
    let policy = policy_from(&read_document(config_path)?)?;
    patch_existing_core_config_at(config_path, |document| {
        Ok(update_api_groups(document, &policy))
    })?;
    for path in credential_files(
        config,
        config_path.parent().ok_or("Invalid core directory")?,
    )? {
        let Some(mut metadata) = credential_metadata(&path)? else {
            continue;
        };
        let patch = credential_patch(&metadata, &policy);
        if patch.is_empty() {
            continue;
        }
        apply_json_patch(&mut metadata, patch);
        write_bytes_atomically(
            &path,
            &serde_json::to_vec_pretty(&metadata).map_err(|e| e.to_string())?,
        )?;
    }
    Ok(())
}

fn reconcile_live(config: &GuiConfigFile) -> Result<(), String> {
    let install_dir = core_install_dir()?;
    let path = install_dir.join(CORE_CONFIG_FILE);
    reconcile_live_at(&path, config)
}

pub(crate) fn reconcile_live_at(path: &Path, config: &GuiConfigFile) -> Result<(), String> {
    if !path.is_file() {
        return Ok(());
    }
    let document = read_document(path)?;
    let mut connection_config = config.clone();
    apply_core_settings_to_gui_config(
        &mut connection_config,
        &core_config_settings_from_value(&document)?,
    );
    let config = &connection_config;
    let policy = policy_from(&document)?;
    patch_existing_core_config_at(path, |document| Ok(update_api_groups(document, &policy)))?;
    let mut pending = Vec::new();
    for path in credential_files(config, path.parent().ok_or("Invalid core directory")?)? {
        let Some(metadata) = credential_metadata(&path)? else {
            continue;
        };
        let patch = credential_patch(&metadata, &policy);
        if patch.is_empty() {
            continue;
        }
        pending.push(path);
    }
    if pending.is_empty() {
        return Ok(());
    }
    let endpoint = format!(
        "{}/v8/management/credentials/fields",
        core_origin(
            &config.host,
            config.port,
            core_tls_settings_from_value(&document)?.enabled
        )
    );
    let authorization = management_authorization(config)?;
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?
        .block_on(async {
            let client = management_http_client()?;
            for path in pending {
                let _guard = lock_credential_updates().await;
                // Re-read after any queued manual override has completed.
                let Some(metadata) = credential_metadata(&path)? else {
                    continue;
                };
                let mut patch = credential_patch(&metadata, &policy);
                if patch.is_empty() {
                    continue;
                }
                let name = path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .ok_or("Invalid credential filename")?;
                patch.insert("name".into(), name.into());
                let response = client
                    .patch(&endpoint)
                    .header("Authorization", &authorization)
                    .json(&patch)
                    .timeout(Duration::from_secs(5))
                    .send()
                    .await
                    .map_err(|_| {
                        "Could not apply cooldown provider defaults to the running core".to_string()
                    })?;
                // A deleted credential may still have been in the directory snapshot.
                if !response.status().is_success()
                    && response.status() != reqwest::StatusCode::NOT_FOUND
                {
                    return Err(format!(
                        "Could not apply cooldown provider defaults (HTTP {})",
                        response.status()
                    ));
                }
            }
            Ok(())
        })
}

/// Covers OAuth logins and externally added credentials. Runtime updates use the
/// core's atomic metadata merge, so concurrent token refreshes retain their tokens.
pub(crate) fn start_reconciler(app: tauri::AppHandle) {
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(2));
        let process = app.state::<CoreProcessState>();
        if process.managed_pid().is_none() {
            continue;
        }
        let Ok(_guard) = lock_core_operation(process.inner()) else {
            continue;
        };
        let result = app
            .state::<GuiConfigState>()
            .snapshot()
            .and_then(|config| reconcile_live(&config));
        if let Ok(mut last) = LAST_ERROR.lock() {
            let next = result.err();
            if next != *last {
                if let Some(error) = &next {
                    eprintln!("Cooldown provider defaults: {error}");
                }
            }
            *last = next;
        }
    });
}

#[tauri::command]
pub(crate) fn get_cooldown_policy(
    gui_config_state: tauri::State<'_, GuiConfigState>,
) -> Result<CooldownPolicySettings, String> {
    let install_dir = core_install_dir()?;
    let path = install_dir.join(CORE_CONFIG_FILE);
    let document = if path.is_file() {
        read_document(&path)?
    } else {
        serde_norway::Value::Null
    };
    let mut policy = policy_from(&document)?;
    if !path.is_file() {
        policy.transient_seconds = -1;
    }
    let mut providers: BTreeSet<String> = [
        "antigravity",
        "claude",
        "codex",
        "gemini",
        "gemini-cli",
        "interactions",
        "xai",
        "meta",
        "vertex",
    ]
    .into_iter()
    .map(str::to_string)
    .collect();
    if let Some(selected) = &policy.providers {
        providers.extend(selected.iter().cloned());
    }
    if let Some(groups) = document
        .get("api-keys")
        .and_then(serde_norway::Value::as_mapping)
    {
        for (section, entries) in groups {
            if section.as_str() == Some("openai-compatibility") {
                if let Some(entries) = entries.as_sequence() {
                    providers.extend(
                        entries
                            .iter()
                            .filter_map(|g| g.get("name").and_then(serde_norway::Value::as_str))
                            .map(|s| s.trim().to_lowercase()),
                    );
                }
            } else if let Some(section) = section.as_str() {
                providers.insert(section.into());
            }
        }
    }
    for path in credential_files(&gui_config_state.snapshot()?, &install_dir)? {
        if let Some(metadata) = credential_metadata(&path)? {
            if let Some(provider) = credential_provider(&metadata) {
                providers.insert(provider);
            }
        }
    }
    Ok(CooldownPolicySettings {
        policy,
        available_providers: providers.into_iter().collect(),
        warning: LAST_ERROR.lock().ok().and_then(|v| v.clone()),
    })
}

#[tauri::command]
pub(crate) async fn set_cooldown_policy(
    app: tauri::AppHandle,
    policy: CooldownPolicy,
) -> Result<(), String> {
    let policy = normalize(policy)?;
    tauri::async_runtime::spawn_blocking(move || {
        let process = app.state::<CoreProcessState>();
        let _guard = lock_core_operation(process.inner())?;
        let install_dir = core_install_dir()?;
        if !install_dir.join(CORE_CONFIG_FILE).is_file() {
            return Err("Install and start the core before configuring cooldown policies".into());
        }
        let running = current_core_status(Some(process.inner()), None)?.running;
        let result = (|| {
            patch_existing_core_config(|document| {
                let mut changed = set_core_yaml_path_value(
                    document,
                    PROVIDERS_PATH,
                    serde_norway::to_value(&policy.providers).map_err(|e| e.to_string())?,
                )?;
                if yaml_at(document, &["plugins", "configs", "model-access", "enabled"]).is_none() {
                    changed |= set_core_yaml_path_value(
                        document,
                        &["plugins", "configs", "model-access", "enabled"],
                        serde_norway::Value::Bool(false),
                    )?;
                }
                changed |= set_core_yaml_schema_value(
                    document,
                    &["transient-error-cooldown-seconds"],
                    &["routing", "cooldown", "transient-error-cooldown-seconds"],
                    serde_norway::to_value(policy.transient_seconds).map_err(|e| e.to_string())?,
                )?;
                changed |= set_core_yaml_schema_value(
                    document,
                    &["save-cooldown-status"],
                    &["routing", "cooldown", "save-cooldown-status"],
                    serde_norway::Value::Bool(policy.persist),
                )?;
                changed |= update_api_groups(document, &policy);
                Ok(changed)
            })?;
            let config = app.state::<GuiConfigState>().update(|config| {
                config.transient_error_cooldown_seconds = i64::from(policy.transient_seconds);
                Ok(())
            })?;
            if running {
                reconcile_live(&config)?;
            } else {
                reconcile_for_start(&install_dir.join(CORE_CONFIG_FILE), &config)?;
            }
            Ok(())
        })();
        if result.is_ok() {
            let _ = app.emit(
                CONFIG_FILES_CHANGED_EVENT,
                ConfigFilesChangedPayload {
                    paths: vec![path_to_string(&install_dir.join(CORE_CONFIG_FILE))],
                    errors: Vec::new(),
                },
            );
        }
        if let Ok(status) = current_core_status(Some(process.inner()), None) {
            emit_core_status(&app, &status);
        }
        result
    })
    .await
    .map_err(|e| format!("Cooldown policy background task failed: {e}"))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn selected() -> CooldownPolicy {
        CooldownPolicy {
            providers: Some(vec!["antigravity".into()]),
            transient_seconds: -1,
            persist: true,
        }
    }

    #[test]
    fn provider_defaults_preserve_explicit_choices_and_are_reversible() {
        let policy = selected();
        assert!(credential_patch(&json!({"type":"antigravity"}), &policy).is_empty());
        for value in [json!(true), json!(false), json!("false")] {
            assert!(
                credential_patch(&json!({"type":"codex", "disable_cooling":value}), &policy)
                    .is_empty()
            );
            assert!(
                credential_patch(&json!({"type":"codex", "disable-cooling":value}), &policy)
                    .is_empty()
            );
        }
        let mut credential = json!({"type":"codex", "access_token":"unchanged", "other":42});
        let patch = credential_patch(&credential, &policy);
        apply_json_patch(&mut credential, patch);
        assert_eq!(credential["disable_cooling"], true);
        assert!(credential_patch(&credential, &policy).is_empty());
        let all = CooldownPolicy {
            providers: None,
            ..policy
        };
        let patch = credential_patch(&credential, &all);
        apply_json_patch(&mut credential, patch);
        assert_eq!(
            credential,
            json!({"type":"codex", "access_token":"unchanged", "other":42})
        );
    }

    #[test]
    fn direct_override_changes_release_managed_ownership() {
        let mut credential = json!({"type":"codex", "disable_cooling":false, (MARKER):true});
        let patch = credential_patch(&credential, &selected());
        apply_json_patch(&mut credential, patch);
        assert_eq!(credential, json!({"type":"codex", "disable_cooling":false}));
        assert!(credential_patch(&credential, &selected()).is_empty());
    }

    #[test]
    fn oauth_gemini_is_distinct_from_api_key_gemini() {
        let policy = CooldownPolicy {
            providers: Some(vec!["gemini-cli".into()]),
            ..selected()
        };
        assert!(credential_patch(&json!({"type":"gemini"}), &policy).is_empty());
        let mut group = serde_norway::from_str("keys: [{api-key: fake}]").unwrap();
        assert!(update_group(&mut group, "gemini", &policy));
        assert_eq!(group["disable-cooling"].as_bool(), Some(true));
    }

    #[test]
    fn offline_policy_preserves_tokens_permissions_and_explicit_overrides() {
        let directory = crate::unique_test_temp_path("cpa-cooldown-policy-");
        fs::create_dir_all(directory.join("auth")).unwrap();
        let config = directory.join("config.yaml");
        fs::write(&config, "config-version: 8\nplugins: {configs: {model-access: {enabled: false, cooldown-providers: [antigravity]}}}\n").unwrap();
        let credential = directory.join("auth/codex.json");
        let original = json!({"type":"codex", "access_token":"test-token", "refresh_token":"test-refresh", "extension":{"keep":true}});
        fs::write(&credential, original.to_string()).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&credential, fs::Permissions::from_mode(0o600)).unwrap();
        }
        let gui = GuiConfigFile {
            auth_dir: path_to_string(&directory.join("auth")),
            ..GuiConfigFile::default()
        };
        reconcile_for_start(&config, &gui).unwrap();
        let managed = credential_metadata(&credential).unwrap().unwrap();
        assert_eq!(managed["disable_cooling"], true);
        assert_eq!(managed["access_token"], original["access_token"]);
        assert_eq!(managed["refresh_token"], original["refresh_token"]);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&credential).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        patch_existing_core_config_at(&config, |document| {
            set_core_yaml_path_value(document, PROVIDERS_PATH, serde_norway::Value::Null)
        })
        .unwrap();
        reconcile_for_start(&config, &gui).unwrap();
        assert_eq!(credential_metadata(&credential).unwrap().unwrap(), original);
        assert_eq!(directory.parent(), Some(std::env::temp_dir().as_path()));
        assert!(directory
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("cpa-cooldown-policy-"));
        fs::remove_dir_all(&directory).unwrap();
    }

    #[test]
    fn api_groups_inherit_provider_defaults_without_overwriting_key_overrides() {
        let mut document: serde_norway::Value = serde_norway::from_str("api-keys:\n  codex:\n    - name: inherited\n      keys: [{api-key: a}, {api-key: b, disable-cooling: false}]\n    - name: explicit\n      disable-cooling: false\n      keys: [{api-key: c}]\n  openai-compatibility:\n    - name: Custom\n      api-key-entries: [{api-key: d}]\n").unwrap();
        let original = document.clone();
        assert!(update_api_groups(&mut document, &selected()));
        assert_eq!(
            document["api-keys"]["codex"][0]["disable-cooling"].as_bool(),
            Some(true)
        );
        assert_eq!(
            document["api-keys"]["codex"][0]["keys"],
            original["api-keys"]["codex"][0]["keys"]
        );
        assert_eq!(
            document["api-keys"]["codex"][1],
            original["api-keys"]["codex"][1]
        );
        assert_eq!(
            document["api-keys"]["openai-compatibility"][0]["disable-cooling"].as_bool(),
            Some(true)
        );
        assert!(!update_api_groups(&mut document, &selected()));
        assert!(update_api_groups(
            &mut document,
            &CooldownPolicy {
                providers: None,
                ..selected()
            }
        ));
        assert_eq!(document, original);
    }

    #[test]
    fn policy_keeps_existing_transient_settings_and_validates_selection() {
        for (yaml, seconds) in [
            ("{}", -1),
            (
                "routing: {cooldown: {transient-error-cooldown-seconds: -1}}",
                -1,
            ),
            ("transient-error-cooldown-seconds: 12", 12),
        ] {
            assert_eq!(
                policy_from(&serde_norway::from_str(yaml).unwrap())
                    .unwrap()
                    .transient_seconds,
                seconds
            );
        }
        assert!(policy_from(
            &serde_norway::from_str("routing: {cooldown: {transient-error-cooldown-seconds: -2}}")
                .unwrap()
        )
        .is_err());
        let mut policy = selected();
        policy.providers = Some(vec![" Antigravity ".into(), "antigravity".into()]);
        assert_eq!(normalize(policy).unwrap().providers, selected().providers);
    }
}
