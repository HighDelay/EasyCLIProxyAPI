use base64::{engine::general_purpose, Engine as _};
use chrono::{DateTime, SecondsFormat, Utc};
use serde_json::{Map, Value};

// The Codex CLI nests tokens in its login cache. The core instead reads a
// provider type and OAuth fields at the top level of a credential file.
pub(super) fn normalize_auth_file(data: Vec<u8>) -> Result<Vec<u8>, String> {
    let mut value: Value = serde_json::from_slice(&data)
        .map_err(|_| "Credential file must contain valid JSON".to_string())?;
    let Some(root) = value.as_object_mut() else {
        return Ok(data);
    };

    // Exports and plugin credentials retain their original provider and bytes.
    if ["type", "provider"]
        .iter()
        .any(|key| string_field(root, key).is_some_and(|provider| provider != "codex"))
        || string_field(root, "access_token").is_some()
    {
        return Ok(data);
    }
    let native_codex = root.contains_key("OPENAI_API_KEY")
        || matches!(string_field(root, "auth_mode"), Some("chatgpt" | "api"));
    if !native_codex {
        return Ok(data);
    }

    if string_field(root, "auth_mode").is_some_and(|mode| mode != "chatgpt")
        || string_field(root, "OPENAI_API_KEY").is_some()
    {
        return Err("This Codex auth.json uses API key authentication. Add the key under API Access, or sign in to Codex with ChatGPT before importing its OAuth credentials.".to_string());
    }
    let tokens = match root.remove("tokens") {
        Some(Value::Object(tokens)) => tokens,
        _ => return Err("Codex auth.json has no OAuth tokens. Sign in to Codex with ChatGPT and import the file again.".to_string()),
    };
    let access_token = string_field(&tokens, "access_token")
        .ok_or("Codex auth.json is missing tokens.access_token. Sign in to Codex with ChatGPT and import the file again.")?;
    let access_claims = jwt_claims(Some(access_token));
    let id_claims = jwt_claims(string_field(&tokens, "id_token"));

    root.remove("auth_mode");
    root.remove("OPENAI_API_KEY");
    root.insert("type".into(), Value::String("codex".into()));
    for key in ["access_token", "refresh_token", "id_token", "account_id"] {
        if let Some(text) = string_field(&tokens, key) {
            root.insert(key.into(), Value::String(text.to_string()));
        }
    }

    // Claims only supply display/routing metadata; the upstream validates the
    // original token. Do not substitute the ID token's expiry for access expiry.
    for (key, pointer) in [
        (
            "account_id",
            "/https:~1~1api.openai.com~1auth/chatgpt_account_id",
        ),
        (
            "plan_type",
            "/https:~1~1api.openai.com~1auth/chatgpt_plan_type",
        ),
        ("email", "/email"),
    ] {
        if string_field(root, key).is_none() {
            let claim = [&id_claims, &access_claims].into_iter().find_map(|claims| {
                claims
                    .as_ref()?
                    .pointer(pointer)?
                    .as_str()
                    .filter(|text| !text.trim().is_empty())
            });
            if let Some(text) = claim {
                root.insert(key.into(), Value::String(text.to_string()));
            }
        }
    }
    if string_field(root, "expired").is_none() {
        if let Some(expiry) = access_claims
            .as_ref()
            .and_then(|claims| claims.get("exp"))
            .and_then(Value::as_i64)
            .filter(|timestamp| *timestamp > 0)
            .and_then(|timestamp| DateTime::<Utc>::from_timestamp(timestamp, 0))
        {
            root.insert(
                "expired".into(),
                Value::String(expiry.to_rfc3339_opts(SecondsFormat::Secs, true)),
            );
        }
    }

    serde_json::to_vec(&value).map_err(|_| "Failed to prepare Codex credential import".to_string())
}

fn string_field<'a>(object: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    object
        .get(key)?
        .as_str()
        .filter(|text| !text.trim().is_empty())
}

fn jwt_claims(token: Option<&str>) -> Option<Value> {
    let parts: Vec<_> = token?.split('.').collect();
    if parts.len() != 3 {
        return None;
    }
    let payload = general_purpose::URL_SAFE_NO_PAD
        .decode(parts[1])
        .or_else(|_| general_purpose::URL_SAFE.decode(parts[1]))
        .ok()?;
    serde_json::from_slice(&payload).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn jwt(claims: Value) -> String {
        format!(
            "e30.{}.test-signature",
            general_purpose::URL_SAFE_NO_PAD.encode(claims.to_string())
        )
    }

    fn normalized(value: Value) -> Value {
        serde_json::from_slice(&normalize_auth_file(serde_json::to_vec(&value).unwrap()).unwrap())
            .unwrap()
    }

    #[test]
    fn imports_codex_chatgpt_login_with_flat_tokens_and_account_metadata() {
        let access_token = jwt(json!({"exp": 4102444800_i64}));
        let id_token = jwt(json!({
            "email": "test@example.invalid", "exp": 4102531200_i64,
            "https://api.openai.com/auth": {"chatgpt_account_id": "jwt-account", "chatgpt_plan_type": "plus"}
        }));
        let imported = normalized(json!({
            "auth_mode": "chatgpt", "OPENAI_API_KEY": null,
            "tokens": {"access_token": access_token, "refresh_token": "test-refresh", "id_token": id_token, "account_id": "selected-account"},
            "last_refresh": "2026-11-05T09:00:00Z", "custom_metadata": {"keep": true}
        }));

        assert_eq!(
            imported,
            json!({
                "type": "codex", "access_token": access_token, "refresh_token": "test-refresh", "id_token": id_token,
                "account_id": "selected-account", "email": "test@example.invalid", "plan_type": "plus",
                "expired": "2100-01-01T00:00:00Z", "last_refresh": "2026-11-05T09:00:00Z", "custom_metadata": {"keep": true}
            })
        );
    }

    #[test]
    fn imports_legacy_codex_cache_and_recovers_missing_account_from_claims() {
        let access_token =
            jwt(json!({"https://api.openai.com/auth": {"chatgpt_account_id": "legacy-account"}}));
        let imported = normalized(
            json!({"OPENAI_API_KEY": null, "tokens": {"access_token": access_token, "refresh_token": "test-refresh"}}),
        );
        assert_eq!(imported["type"], "codex");
        assert_eq!(imported["account_id"], "legacy-account");
        assert_eq!(imported["refresh_token"], "test-refresh");
        assert!(imported.get("tokens").is_none());
    }

    #[test]
    fn keeps_core_exports_and_other_provider_credentials_byte_for_byte() {
        for data in [
            r#"{ "type": "codex", "access_token": "test-current", "tokens": {"access_token": "test-stale"}, "priority": 3 }"#,
            r#"{ "type": "claude", "auth_mode": "chatgpt", "tokens": {"access_token": "test-other"} }"#,
            r#"{ "provider": "custom-plugin", "OPENAI_API_KEY": "test-plugin-key" }"#,
            r#"{ "tokens": {"access_token": "test-unknown", "refresh_token": "test-refresh"}, "custom": true }"#,
        ] {
            assert_eq!(
                normalize_auth_file(data.as_bytes().to_vec()).unwrap(),
                data.as_bytes()
            );
        }
    }

    #[test]
    fn ignores_undecodable_optional_claims_and_never_uses_id_token_expiry() {
        for access_token in [
            "opaque-token",
            "header.%%%.signature",
            "header.W10.signature",
        ] {
            let imported = normalized(json!({
                "auth_mode": "chatgpt", "tokens": {"access_token": access_token, "id_token": jwt(json!({"exp": 4102444800_i64}))}
            }));
            assert_eq!(imported["access_token"], access_token);
            assert_eq!(imported["type"], "codex");
            assert!(imported.get("expired").is_none());
        }
    }

    #[test]
    fn rejects_incomplete_codex_logins_without_exposing_credentials() {
        for tokens in [
            Value::Null,
            json!({}),
            json!({"access_token": " "}),
            json!({"access_token": 123}),
        ] {
            let data = serde_json::to_vec(
                &json!({"auth_mode": "chatgpt", "tokens": tokens, "private": "test-secret"}),
            )
            .unwrap();
            let error = normalize_auth_file(data).unwrap_err();
            assert!(error.contains("Codex auth.json"));
            assert!(!error.contains("test-secret"));
        }
        assert_eq!(
            normalize_auth_file(b"{\"private\": test-secret}".to_vec()).unwrap_err(),
            "Credential file must contain valid JSON"
        );
    }

    #[test]
    fn directs_native_api_key_logins_to_api_access() {
        for value in [
            json!({"auth_mode": "api", "OPENAI_API_KEY": "test-api-key", "tokens": null}),
            json!({"OPENAI_API_KEY": "test-api-key"}),
        ] {
            let error = normalize_auth_file(serde_json::to_vec(&value).unwrap()).unwrap_err();
            assert!(error.contains("API Access"));
            assert!(!error.contains("test-api-key"));
        }
    }
}
