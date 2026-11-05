use base64::{engine::general_purpose::STANDARD, Engine as _};
use regex::Regex;
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    ffi::{c_char, c_void, CStr},
    panic::catch_unwind,
    sync::{LazyLock, RwLock},
};

type Rules = HashMap<String, Vec<Regex>>;
static RULES: LazyLock<RwLock<Rules>> = LazyLock::new(|| RwLock::new(HashMap::new()));

fn parse_rules(bytes: &[u8]) -> Result<Rules, String> {
    let config: serde_norway::Value =
        serde_norway::from_slice(bytes).map_err(|_| "Invalid model access configuration")?;
    let mut rules = HashMap::new();
    if let Some(entries) = config
        .get("rules")
        .and_then(serde_norway::Value::as_mapping)
    {
        for (scope, values) in entries {
            let scope = scope.as_str().ok_or("Invalid caller scope")?;
            if scope.len() != 64 || !scope.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err("Invalid caller scope".into());
            }
            let values = values
                .as_sequence()
                .ok_or("Excluded models must be a list")?;
            let mut patterns = Vec::new();
            for value in values {
                let rule = value.as_str().ok_or("Model rules must be strings")?.trim();
                if rule.is_empty() || rule.len() > 256 || rule.chars().any(char::is_control) {
                    return Err("Invalid model rule".into());
                }
                let pattern = regex::escape(rule).replace("\\*", ".*");
                patterns.push(
                    Regex::new(&format!("(?i)^{pattern}$")).map_err(|_| "Invalid model rule")?,
                );
            }
            if !patterns.is_empty() {
                rules.insert(scope.to_string(), patterns);
            }
        }
    }
    Ok(rules)
}

fn denied() -> Value {
    json!({"Terminate": true, "StatusCode": 403,
        "ResponseHeaders": {"Content-Type": ["application/json"]},
        "ResponseBody": STANDARD.encode(br#"{"error":{"message":"This API key is not permitted to use the requested model.","type":"permission_error","code":"model_not_allowed"}}"#)})
}

fn intercept(request: &Value, rules: &Rules) -> Value {
    let scope = request
        .get("CallerScope")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .or_else(|| {
            request
                .pointer("/Metadata/caller_scope")
                .and_then(Value::as_str)
        });
    let patterns = match scope {
        Some(scope) => match rules.get(scope) {
            Some(patterns) => patterns,
            None => return json!({}),
        },
        None if rules.is_empty() => return json!({}),
        None => return denied(),
    };
    for model in ["RequestedModel", "Model"]
        .iter()
        .filter_map(|key| request.get(key).and_then(Value::as_str))
    {
        let model = model.trim();
        // Thinking suffixes such as model(high) must not bypass an exact rule.
        let base = model.split_once('(').map_or(model, |(base, _)| base);
        if patterns
            .iter()
            .any(|rule| rule.is_match(model) || rule.is_match(base))
        {
            return denied();
        }
    }
    if !["RequestedModel", "Model"].iter().any(|key| {
        request
            .get(key)
            .and_then(Value::as_str)
            .is_some_and(|m| !m.trim().is_empty())
    }) {
        return denied();
    }
    json!({})
}

fn dispatch(method: &str, request: Value) -> Result<Value, String> {
    match method {
        "plugin.register" | "plugin.reconfigure" => {
            let encoded = request
                .get("config_yaml")
                .and_then(Value::as_str)
                .unwrap_or("");
            let bytes = STANDARD
                .decode(encoded)
                .map_err(|_| "Invalid plugin configuration")?;
            let rules = parse_rules(&bytes)?;
            *RULES.write().unwrap_or_else(|e| e.into_inner()) = rules;
            Ok(
                json!({"schema_version": 7, "metadata": {"Name": "model-access", "Version": "0.1.0", "Author": "HighDelay", "GitHubRepository": "https://github.com/HighDelay/EasyCLIProxyAPI"},
                "capabilities": {"request_interceptor": true, "request_interceptor_omit_body_before": true, "request_interceptor_omit_body_after": true}}),
            )
        }
        "request.intercept_before" | "request.intercept_after" => Ok(intercept(
            &request,
            &RULES.read().unwrap_or_else(|e| e.into_inner()),
        )),
        "plugin.quiesce" | "plugin.shutdown" => Ok(json!({})),
        _ => Err("Unsupported policy plugin method".into()),
    }
}

#[repr(C)]
pub struct Buffer {
    ptr: *mut u8,
    len: usize,
}
#[repr(C)]
pub struct HostApi {
    abi_version: u32,
    host_ctx: *mut c_void,
    call: *const c_void,
    free_buffer: *const c_void,
}
#[repr(C)]
pub struct PluginApi {
    abi_version: u32,
    call: Option<unsafe extern "C" fn(*const c_char, *const u8, usize, *mut Buffer) -> i32>,
    free_buffer: Option<unsafe extern "C" fn(*mut u8, usize)>,
    shutdown: Option<extern "C" fn()>,
}

#[no_mangle]
pub unsafe extern "C" fn cliproxy_plugin_init(host: *const HostApi, plugin: *mut PluginApi) -> i32 {
    if host.is_null() || plugin.is_null() || (*host).abi_version != 1 {
        return 1;
    }
    *plugin = PluginApi {
        abi_version: 1,
        call: Some(call),
        free_buffer: Some(free_buffer),
        shutdown: Some(shutdown),
    };
    0
}

unsafe extern "C" fn call(
    method: *const c_char,
    input: *const u8,
    len: usize,
    output: *mut Buffer,
) -> i32 {
    if output.is_null() {
        return 1;
    }
    *output = Buffer {
        ptr: std::ptr::null_mut(),
        len: 0,
    };
    let result = catch_unwind(|| {
        if method.is_null() || (len > 0 && input.is_null()) || len > 16 << 20 {
            return Err("Invalid policy request".to_string());
        }
        let method = CStr::from_ptr(method)
            .to_str()
            .map_err(|_| "Invalid method")?;
        let bytes = if len == 0 {
            &[]
        } else {
            std::slice::from_raw_parts(input, len)
        };
        let request = serde_json::from_slice(bytes).map_err(|_| "Invalid policy request")?;
        dispatch(method, request)
    })
    .unwrap_or_else(|_| Err("Policy plugin failure".into()));
    let (code, value) = match result {
        Ok(value) => (0, json!({"ok": true, "result": value})),
        Err(message) => (
            1,
            json!({"ok": false, "error": {"code": "model_access", "message": message}}),
        ),
    };
    let bytes = serde_json::to_vec(&value)
        .unwrap_or_default()
        .into_boxed_slice();
    (*output).len = bytes.len();
    (*output).ptr = Box::into_raw(bytes) as *mut u8;
    code
}

unsafe extern "C" fn free_buffer(ptr: *mut u8, len: usize) {
    if !ptr.is_null() {
        drop(Box::from_raw(std::ptr::slice_from_raw_parts_mut(ptr, len)));
    }
}
extern "C" fn shutdown() {}

#[cfg(test)]
mod tests {
    use super::*;
    fn rules() -> Rules {
        parse_rules(
            format!(
                "rules:\n  {}: [gpt-locked, 'claude-opus-*']\n",
                "a".repeat(64)
            )
            .as_bytes(),
        )
        .unwrap()
    }
    #[test]
    fn exclusions_are_scoped_to_authenticated_key_and_match_alias_targets() {
        let rules = rules();
        for (scope, requested, resolved, blocked) in [
            ("a", "gpt-locked", "gpt-locked", true),
            ("b", "gpt-locked", "gpt-locked", false),
            ("a", "gpt-allowed", "gpt-allowed", false),
            ("a", "friendly-alias", "gpt-locked", true),
            ("a", "CLAUDE-OPUS-LATEST", "other", true),
            ("a", "gpt-locked(high)", "other", true),
        ] {
            assert_eq!(intercept(&json!({"CallerScope": scope.repeat(64), "RequestedModel": requested, "Model": resolved}), &rules).get("Terminate").and_then(Value::as_bool).unwrap_or(false), blocked);
        }
    }
    #[test]
    fn missing_identity_is_rejected_when_policies_exist() {
        assert_eq!(
            intercept(&json!({"Model":"gpt-allowed"}), &rules())["StatusCode"],
            403
        );
        assert_eq!(
            intercept(&json!({"Model":"gpt-allowed"}), &HashMap::new()),
            json!({})
        );
    }
    #[test]
    fn patterns_are_literals_except_for_star() {
        let rules =
            parse_rules(format!("rules:\n  {}: ['a.b']", "a".repeat(64)).as_bytes()).unwrap();
        assert_eq!(
            intercept(&json!({"CallerScope":"a".repeat(64),"Model":"axb"}), &rules),
            json!({})
        );
    }
}
