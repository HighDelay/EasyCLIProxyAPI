use super::*;
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

async fn wait_for_models(core: &TestCore) {
    for attempt in 0..50 {
        let mut missing = Vec::new();
        for name in ["exhausted.json", "healthy.json"] {
            let catalog: serde_json::Value = core
                .client
                .get(format!("{}/v8/management/credentials/models", core.origin))
                .query(&[("name", name)])
                .bearer_auth("isolated-test-secret")
                .send()
                .await
                .unwrap()
                .json()
                .await
                .unwrap();
            if !catalog["models"]
                .as_array()
                .is_some_and(|models| models.iter().any(|m| m["id"] == "claude-opus-5-5-high"))
            {
                missing.push(name);
            }
        }
        if missing.is_empty() {
            return;
        }
        assert!(
            attempt < 49,
            "Antigravity fixture model discovery failed: {missing:?}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

#[tokio::test]
#[ignore = "requires CPA_V8_TEST_CORE pointing to a v8.0.56+ executable; uses only local fake accounts"]
async fn antigravity_quota_and_transient_cooldowns_follow_provider_policy() {
    let upstream = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream_port = upstream.local_addr().unwrap().port();
    let quota_calls = Arc::new(AtomicUsize::new(0));
    let healthy_calls = Arc::new(AtomicUsize::new(0));
    let fail_next = Arc::new(AtomicBool::new(false));
    let mut quota: serde_json::Value =
        serde_json::from_str(include_str!("../fixtures/antigravity-quota-exhausted.json")).unwrap();
    // Retain the supplied response, but move its absolute reset time forward
    // so the regression test also works after November 2026.
    quota["error"]["details"][0]["metadata"]["quotaResetTimeStamp"] = (chrono::Utc::now()
        + chrono::Duration::seconds(17115))
    .to_rfc3339()
    .into();
    let quota_body = quota.to_string();
    let (seen_quota, seen_healthy, should_fail) = (
        quota_calls.clone(),
        healthy_calls.clone(),
        fail_next.clone(),
    );
    let upstream_task = tokio::spawn(async move {
        loop {
            let (mut connection, _) = upstream.accept().await.unwrap();
            let (seen_quota, seen_healthy, should_fail, quota_body) = (
                seen_quota.clone(),
                seen_healthy.clone(),
                should_fail.clone(),
                quota_body.clone(),
            );
            tokio::spawn(async move {
                let mut bytes = Vec::new();
                let mut chunk = [0u8; 8192];
                loop {
                    let read = connection.read(&mut chunk).await.unwrap();
                    if read == 0 {
                        return;
                    }
                    bytes.extend_from_slice(&chunk[..read]);
                    if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                        let headers = String::from_utf8_lossy(&bytes[..end]).to_lowercase();
                        let length = headers
                            .lines()
                            .find_map(|line| {
                                line.strip_prefix("content-length:")?
                                    .trim()
                                    .parse::<usize>()
                                    .ok()
                            })
                            .unwrap_or(0);
                        if bytes.len() >= end + 4 + length {
                            break;
                        }
                    }
                }
                let headers = String::from_utf8_lossy(&bytes).to_lowercase();
                let (status, body) = if headers.contains("fetchavailablemodels") {
                    ("200 OK", r#"{"models":{"claude-opus-5-5-high":{}}}"#.into())
                } else if headers.contains("authorization: bearer exhausted-test-token") {
                    seen_quota.fetch_add(1, Ordering::SeqCst);
                    ("429 Too Many Requests", quota_body)
                } else {
                    assert!(
                        headers.contains("authorization: bearer healthy-test-token"),
                        "Unexpected local fixture credential"
                    );
                    seen_healthy.fetch_add(1, Ordering::SeqCst);
                    if should_fail.swap(false, Ordering::SeqCst) {
                        ("503 Service Unavailable", r#"{"error":{"code":503,"message":"Temporary test outage","status":"UNAVAILABLE"}}"#.into())
                    } else {
                        ("200 OK", r#"{"response":{"candidates":[{"content":{"role":"model","parts":[{"text":"local fixture success"}]},"finishReason":"STOP"}],"modelVersion":"claude-opus-5-5-high","responseId":"local-test-response","usageMetadata":{"promptTokenCount":1,"candidatesTokenCount":1,"totalTokenCount":2}}}"#.into())
                    }
                };
                let response = format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                connection.write_all(response.as_bytes()).await.unwrap();
            });
        }
    });
    let executable =
        fs::canonicalize(std::env::var_os("CPA_V8_TEST_CORE").expect("set CPA_V8_TEST_CORE"))
            .unwrap();
    let directory = agent_test_home("v8-contract-antigravity-cooldowns");
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let mut core = TestCore {
        child: None,
        config: directory.join("config.yaml"),
        directory,
        executable,
        origin: format!("http://127.0.0.1:{port}"),
        client: reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(8))
            .build()
            .unwrap(),
    };
    let auth_dir = core.directory.join("auth");
    fs::create_dir_all(&auth_dir).unwrap();
    for (name, token, priority) in [
        ("exhausted", "exhausted-test-token", 100),
        ("healthy", "healthy-test-token", 0),
    ] {
        fs::write(auth_dir.join(format!("{name}.json")), serde_json::json!({
            "type":"antigravity", "access_token":token, "expired":"2099-01-01T00:00:00Z",
            "project_id":"isolated-test-project", "base_url":format!("http://127.0.0.1:{upstream_port}"),
            "proxy_url":"direct", "priority":priority,
        }).to_string()).unwrap();
    }
    fs::write(&core.config, format!(r#"config-version: 8
server: {{host: 127.0.0.1, port: {port}}}
management: {{secret-key: isolated-test-secret, disable-control-panel: true, disable-auto-update-panel: true}}
access: {{api-keys: [cooldown-test-client]}}
oauth: {{auth-dir: {auth_dir:?}}}
requests: {{proxy-url: 'http://127.0.0.1:1'}}
routing:
  strategy: fill-first
  retry: {{request-retry: 0, max-retry-interval: 0}}
  cooldown: {{disable-cooling: false, transient-error-cooldown-seconds: -1, save-cooldown-status: true}}
plugins:
  configs:
    model-access: {{enabled: false, cooldown-providers: [antigravity]}}
api-keys:
  openai-compatibility:
    - name: PolicyFixture
      base-url: http://127.0.0.1:{upstream_port}/v1
      models: [{{name: unused-fixture-model}}]
      keys: [{{api-key: fake-inherited}}, {{api-key: fake-override, disable-cooling: false}}]
"#, auth_dir = path_to_string(&auth_dir))).unwrap();
    let gui = GuiConfigFile {
        port,
        auth_dir: path_to_string(&auth_dir),
        management_secret_key: "isolated-test-secret".into(),
        ..GuiConfigFile::default()
    };
    crate::cooldown_policy::reconcile_for_start(&core.config, &gui).unwrap();
    core.start().await;
    wait_for_models(&core).await;
    let groups: serde_json::Value = core
        .client
        .get(format!("{}/v8/management/config/api-keys", core.origin))
        .bearer_auth("isolated-test-secret")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let group = &groups["openai-compatibility"][0];
    assert_eq!(
        group["disable-cooling"], true,
        "API group did not inherit provider default: {groups}"
    );
    assert_eq!(
        group["_easycli_cooldown_managed"], true,
        "Core API dropped managed default ownership: {groups}"
    );
    assert_eq!(
        group["keys"][1]["disable-cooling"], false,
        "API-key override was overwritten"
    );
    let request = || {
        core.client.post(format!("{}/v1/chat/completions", core.origin)).bearer_auth("cooldown-test-client")
        .json(&serde_json::json!({"model":"claude-opus-5-5-high", "messages":[{"role":"user","content":"Local test"}], "stream":false}))
    };
    let response = request().send().await.unwrap();
    let status = response.status();
    let body = response.text().await.unwrap();
    assert!(
        status.is_success(),
        "Quota failover failed: {status} {body}"
    );
    assert_eq!(
        quota_calls.load(Ordering::SeqCst),
        1,
        "First exhausted account was not tried exactly once"
    );
    assert_eq!(healthy_calls.load(Ordering::SeqCst), 1);
    let snapshot: serde_json::Value = core
        .client
        .get(format!("{}/v8/management/credentials", core.origin))
        .bearer_auth("isolated-test-secret")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let exhausted = snapshot["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["name"] == "exhausted.json")
        .unwrap();
    let cooldown = exhausted["cooldowns"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["reason"] == "quota")
        .expect("Missing structured quota cooldown");
    assert!(
        cooldown["remaining_seconds"].as_i64().unwrap() > 17000,
        "Provider reset delay was not retained: {cooldown}"
    );
    fail_next.store(true, Ordering::SeqCst);
    assert_eq!(request().send().await.unwrap().status().as_u16(), 503);
    let response = request().send().await.unwrap();
    assert!(
        response.status().is_success(),
        "A temporary failure cooled down a healthy account: {}",
        response.text().await.unwrap()
    );
    assert_eq!(
        quota_calls.load(Ordering::SeqCst),
        1,
        "Known exhausted account was retried"
    );
    assert_eq!(healthy_calls.load(Ordering::SeqCst), 3);
    core.stop();
    core.start().await;
    wait_for_models(&core).await;
    let response = core.client.post(format!("{}/v1/chat/completions", core.origin)).bearer_auth("cooldown-test-client")
        .json(&serde_json::json!({"model":"claude-opus-5-5-high", "messages":[{"role":"user","content":"After restart"}]})).send().await.unwrap();
    assert!(response.status().is_success());
    assert_eq!(
        quota_calls.load(Ordering::SeqCst),
        1,
        "Persisted quota cooldown was lost at restart"
    );
    core.stop();
    let content = fs::read_to_string(&core.config).unwrap().replace(
        "cooldown-providers: [antigravity]",
        "cooldown-providers: [codex]",
    );
    fs::write(&core.config, content).unwrap();
    crate::cooldown_policy::reconcile_for_start(&core.config, &gui).unwrap();
    core.start().await;
    wait_for_models(&core).await;
    for _ in 0..2 {
        let response = core.client.post(format!("{}/v1/chat/completions", core.origin)).bearer_auth("cooldown-test-client")
            .json(&serde_json::json!({"model":"claude-opus-5-5-high", "messages":[{"role":"user","content":"Provider disabled"}]})).send().await.unwrap();
        assert!(response.status().is_success());
    }
    assert_eq!(
        quota_calls.load(Ordering::SeqCst),
        3,
        "Unselected provider still used quota cooldowns"
    );
    // Restore provider inheritance while the core is running. These updates go
    // through its atomic metadata PATCH rather than overwriting live token JSON.
    let content = fs::read_to_string(&core.config).unwrap().replace(
        "cooldown-providers: [codex]",
        "cooldown-providers: [antigravity]",
    );
    fs::write(&core.config, content).unwrap();
    core.client
        .patch(format!("{}/v8/management/credentials/fields", core.origin))
        .bearer_auth("isolated-test-secret")
        .json(&serde_json::json!({"name":"healthy.json", "refresh_marker":"preserved"}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    let (path, config) = (core.config.clone(), gui.clone());
    tokio::task::spawn_blocking(move || crate::cooldown_policy::reconcile_live_at(&path, &config))
        .await
        .unwrap()
        .unwrap();
    wait_for_models(&core).await;
    let healthy: serde_json::Value = core
        .client
        .get(format!(
            "{}/v8/management/credentials/download",
            core.origin
        ))
        .query(&[("name", "healthy.json")])
        .bearer_auth("isolated-test-secret")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(healthy["access_token"], "healthy-test-token");
    assert_eq!(healthy["refresh_marker"], "preserved");
    // Arbitrary metadata PATCH uses JSON null for inheritance rather than
    // deleting the key; the core treats that as an absent boolean override.
    assert!(healthy["disable_cooling"].is_null());
    assert!(healthy["_easycli_cooldown_managed"].is_null());
    let before = quota_calls.load(Ordering::SeqCst);
    for _ in 0..2 {
        let response = core.client.post(format!("{}/v1/chat/completions", core.origin)).bearer_auth("cooldown-test-client")
            .json(&serde_json::json!({"model":"claude-opus-5-5-high", "messages":[{"role":"user","content":"Live provider change"}]})).send().await.unwrap();
        assert!(response.status().is_success());
    }
    assert!(
        quota_calls.load(Ordering::SeqCst) <= before + 1,
        "Live provider update did not restore quota cooldowns"
    );
    upstream_task.abort();
}
