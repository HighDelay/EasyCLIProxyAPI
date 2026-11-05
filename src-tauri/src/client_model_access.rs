use crate::*;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

const PLUGIN: &[u8] = include_bytes!(env!("CPA_MODEL_POLICY_PLUGIN"));
const POLICY_PATH: &[&str] = &["plugins", "configs", "model-access", "rules"];
type Rules = BTreeMap<String, Vec<String>>;

fn caller_scope(key: &str) -> String {
    format!(
        "{:x}",
        Sha256::digest(format!("cli-proxy-api:caller-scope:v1\0{}", key.trim()).as_bytes())
    )
}

fn rules(document: &serde_norway::Value) -> Result<Rules, String> {
    let value = document
        .as_mapping()
        .and_then(|root| nested_yaml_value(root, POLICY_PATH));
    match value {
        None | Some(serde_norway::Value::Null) => Ok(Rules::new()),
        Some(value) => serde_norway::from_value(value.clone())
            .map_err(|_| "Invalid model exclusion policies in core configuration".into()),
    }
}

pub(crate) fn enable_for_policies(document: &mut serde_norway::Value) -> Result<bool, String> {
    if rules(document)?.values().all(Vec::is_empty) {
        return Ok(false);
    }
    let mut changed = set_core_yaml_path_value(
        document,
        &["plugins", "enabled"],
        serde_norway::Value::Bool(true),
    )?;
    changed |= set_core_yaml_path_value(
        document,
        &["plugins", "configs", "model-access", "enabled"],
        serde_norway::Value::Bool(true),
    )?;
    Ok(changed)
}

pub(crate) fn install_for_start(config_path: &Path, install_dir: &Path) -> Result<(), String> {
    let document: serde_norway::Value =
        serde_norway::from_str(&fs::read_to_string(config_path).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    if rules(&document)?.values().all(Vec::is_empty) {
        return Ok(());
    }
    if PLUGIN.is_empty() {
        return Err("This build does not include the model access plugin. Build with bun tauri build before configuring model exclusions.".into());
    }
    let directory = document
        .as_mapping()
        .and_then(|root| nested_yaml_value(root, &["plugins", "dir"]))
        .and_then(serde_norway::Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .unwrap_or("plugins");
    let directory = PathBuf::from(directory);
    let directory = if directory.is_absolute() {
        directory
    } else {
        install_dir.join(directory)
    };
    let (os, extension) = match std::env::consts::OS {
        "windows" => ("windows", "dll"),
        "linux" => ("linux", "so"),
        "macos" => ("darwin", "dylib"),
        _ => return Err("Unsupported model access platform".into()),
    };
    let arch = match std::env::consts::ARCH {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        _ => return Err("Unsupported model access architecture".into()),
    };
    let directory = directory.join(os).join(arch);
    fs::create_dir_all(&directory)
        .map_err(|e| format!("Failed to create model access plugin directory: {e}"))?;
    let target = directory.join(format!("model-access.{extension}"));
    if fs::read(&target).ok().as_deref() != Some(PLUGIN) {
        fs::write(target, PLUGIN)
            .map_err(|e| format!("Failed to install model access plugin: {e}"))?;
    }
    Ok(())
}

pub(crate) fn normalize_rules(values: Vec<String>) -> Result<Vec<String>, String> {
    let mut result = Vec::new();
    if values.len() > 256 {
        return Err("Use at most 256 excluded model rules per key".into());
    }
    for value in values {
        let value = value.trim().to_lowercase();
        if value.is_empty() {
            continue;
        }
        if value.len() > 256 || value.chars().any(char::is_control) {
            return Err(
                "Model rules must be printable names or wildcard patterns of at most 256 bytes"
                    .into(),
            );
        }
        if !result.contains(&value) {
            result.push(value);
        }
    }
    Ok(result)
}

#[tauri::command]
pub(crate) fn get_core_api_key_model_exclusions(api_key: String) -> Result<Vec<String>, String> {
    let _guard = lock_core_config_file()?;
    let path = core_install_dir()?.join(CORE_CONFIG_FILE);
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let (_, document) = read_core_config_document()?;
    Ok(rules(document.get())?
        .remove(&caller_scope(&api_key))
        .unwrap_or_default())
}

#[tauri::command]
pub(crate) async fn set_core_api_key_model_exclusions(
    app: tauri::AppHandle,
    api_key: String,
    excluded_models: Vec<String>,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let process_state = app.state::<CoreProcessState>();
        let gui_config_state = app.state::<GuiConfigState>();
        let _guard = lock_core_operation(process_state.inner())?;
        let result = set_exclusions_with_state(
            process_state.inner(),
            gui_config_state.inner(),
            api_key,
            excluded_models,
        );
        if let Ok(status) = current_core_status(Some(process_state.inner()), None) {
            emit_core_status(&app, &status);
        }
        result
    })
    .await
    .map_err(|error| format!("Model access background task failed: {error}"))?
}

fn set_exclusions_with_state(
    process_state: &CoreProcessState,
    gui_config_state: &GuiConfigState,
    api_key: String,
    excluded_models: Vec<String>,
) -> Result<(), String> {
    validate_core_api_key(api_key.trim())?;
    let excluded = normalize_rules(excluded_models)?;
    let install_dir = core_install_dir()?;
    let path = install_dir.join(CORE_CONFIG_FILE);
    if !path.is_file() {
        return if excluded.is_empty() {
            Ok(())
        } else {
            Err("Install and start the core before configuring model exclusions".into())
        };
    }
    let scope = caller_scope(&api_key);
    let changed = {
        let _guard = lock_core_config_file()?;
        let (_, document) = read_core_config_document()?;
        rules(document.get())?
            .get(&scope)
            .cloned()
            .unwrap_or_default()
            != excluded
    };
    if !changed {
        return Ok(());
    }
    if !excluded.is_empty() && PLUGIN.is_empty() {
        return Err("This build does not include the model access plugin. Install a complete fork build first.".into());
    }
    let running = current_core_status(Some(process_state), None)?.running;
    if running {
        stop_core_process_inner(process_state)?;
    }
    patch_existing_core_config(|document| {
        let mut policies = rules(document)?;
        if excluded.is_empty() {
            policies.remove(&scope);
        } else {
            policies.insert(scope.clone(), excluded.clone());
        }
        let mut changed = set_core_yaml_path_value(
            document,
            POLICY_PATH,
            serde_norway::to_value(policies).map_err(|e| e.to_string())?,
        )?;
        changed |= enable_for_policies(document)?;
        Ok(changed)
    })?;
    install_for_start(&path, &install_dir)?;
    if running {
        let config = gui_config_state.snapshot()?;
        start_core_process_inner(process_state, &config)?;
    }
    Ok(())
}

pub(crate) fn verify_for_start(config_path: &Path, config: &GuiConfigFile) -> Result<(), String> {
    let document: serde_norway::Value =
        serde_norway::from_str(&fs::read_to_string(config_path).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    if rules(&document)?.values().all(Vec::is_empty) {
        return Ok(());
    }
    let endpoint = format!(
        "{}/v8/management/plugins",
        core_origin(
            &config.host,
            config.port,
            core_tls_settings_from_value(&document)?.enabled
        ),
    );
    let authorization = management_authorization(config)?;
    // Core startup can run from either a Tauri worker or an async command.
    // Keep the HTTP probe off either caller's runtime thread.
    std::thread::spawn(move || {
        tauri::async_runtime::block_on(async move {
            let response = management_http_client()?
                .get(endpoint)
                .header("Authorization", authorization)
                .send()
                .await
                .map_err(|_| "Failed to verify model access plugin")?;
            let value = read_management_value(response).await?;
            let active = value.get("plugins").and_then(serde_json::Value::as_array)
                .is_some_and(|items| items.iter().any(|item| {
                    item.get("id").and_then(serde_json::Value::as_str) == Some("model-access")
                        && item.get("registered").and_then(serde_json::Value::as_bool) == Some(true)
                        && item.get("effective_enabled").and_then(serde_json::Value::as_bool) == Some(true)
                }));
            if active {
                Ok(())
            } else {
                Err("Model access plugin did not load. Update CLIProxyAPI to 8.0.56 or newer before using exclusions.".to_string())
            }
        })
    }).join().map_err(|_| "Model access verification failed".to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn policies_enable_enforcement_without_changing_other_settings() {
        let mut doc: serde_norway::Value = serde_norway::from_str("plugins: {enabled: false, configs: {other: {enabled: true}}}\naccess: {api-keys: [test-key]}\n").unwrap();
        assert!(!enable_for_policies(&mut doc).unwrap());
        let policy = [(caller_scope("test-key"), vec!["gpt-blocked".to_string()])]
            .into_iter()
            .collect::<Rules>();
        set_core_yaml_path_value(
            &mut doc,
            POLICY_PATH,
            serde_norway::to_value(policy).unwrap(),
        )
        .unwrap();
        assert!(enable_for_policies(&mut doc).unwrap());
        assert_eq!(
            doc["plugins"]["configs"]["other"]["enabled"].as_bool(),
            Some(true)
        );
        assert_eq!(doc["access"]["api-keys"][0].as_str(), Some("test-key"));
        assert_ne!(caller_scope("first"), caller_scope("second"));
        assert_eq!(caller_scope(" key "), caller_scope("key"));
    }
    #[test]
    fn validates_and_normalizes_rules() {
        assert_eq!(
            normalize_rules(vec![" GPT-* ".into(), "gpt-*".into(), "".into()]).unwrap(),
            vec!["gpt-*"]
        );
        assert!(normalize_rules(vec!["bad\nrule".into()]).is_err());
    }
}
