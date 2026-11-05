# Client model access plugin

This library implements CLIProxyAPI's C ABI v1 and RPC schema 7. The official
core loads it as a request interceptor before and after provider selection.
The ABI is defined in the upstream `sdk/pluginabi` package; this implementation
is covered by a contract test against the bundled official executable.

Rules are stored under `plugins.configs.model-access.rules` in core YAML. Each
map key is the core's authenticated caller scope:
`SHA256("cli-proxy-api:caller-scope:v1\0" + trimmed_client_key)`.
Values are lists of case insensitive model names or `*` wildcard patterns.
Both requested and resolved names are checked, including thinking suffixes.
Request bodies are omitted from both hooks so large prompts do not change the
policy decision or incur extra body copies.

Build with `node scripts/build-model-access.mjs` from the repository root. It
copies the platform library into an ignored resource which the GUI embeds and
installs on core startup. See [usage and verification](../../docs/client-model-exclusions.md).
