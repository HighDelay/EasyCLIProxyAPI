# Model exclusions for client API keys

Open **Advanced Features → General → Authentication Keys**, then add or edit a
key. In **Excluded models**, enter model IDs or `*` wildcard patterns, one per
line. Commas also separate rules. Matching is case insensitive; an empty list
allows all available models.

For example:

```text
private-model
premium-*
```

Requests authenticated with this key receive HTTP 403 and error code
`model_not_allowed` when a rule matches. Other keys keep their own permissions.
The policy checks both the requested model and the resolved upstream model, so
an alias cannot provide access to an excluded model. Restrictions apply to
OpenAI, Claude and Gemini inference, including streaming requests. Model list
responses can still advertise excluded models; inference remains blocked.
For Responses WebSocket requests, the core closes the connection when a model
is excluded; no upstream inference runs.

Changing exclusions restarts a running core to apply the policy before the key
is saved. Existing requests may be interrupted. The bundled policy plugin runs
inside CLIProxyAPI and continues enforcing the rules independently of the GUI.
Its configuration uses hashed caller identities rather than copies of the keys.
The app installs the embedded plugin automatically and verifies it during core
startup; if a configured policy cannot load, the app stops the core and reports
the error. Use CLIProxyAPI 8.0.56 or newer.

The policy plugin is built and embedded by `bun run build` / `bun tauri build`.
When developing with direct Cargo commands, run
`node scripts/build-model-access.mjs` first. No separate plugin download is
needed for packaged releases. Both Windows and Linux builds include it.

Release builds test the plugin against the bundled official core on Windows and
Linux. To run that test locally after preparing a portable build, use
`bun run test:core:client-model-access`, or set `CPA_V8_TEST_CORE` to an isolated
test core executable first. The test uses a local fake provider and temporary
configuration; it does not use saved accounts or paid provider requests.
