# Updating this fork

The desktop app follows releases from `HighDelay/EasyCLIProxyAPI`. Core updates
continue to use `router-for-me/CLIProxyAPI`. The app validates both the release
page and package URLs against our fork, so an upstream desktop release cannot
replace our custom changes through the updater.

## Install the fork updater once

The earlier Codex test build still follows the upstream desktop updater. Close
the app and copy the new executable **and `portable-app.json`** into the existing
installation, or copy the contents of a fork release over it. On Windows the
executable is `EasyCLIProxyAPI.exe`; on Linux it is `EasyCLIProxyAPI`. The portable
marker must match the compiled app version for in-app updates to be enabled.
Keep `config.toml`, `oauth`, and `cpa-core/config.yaml` in place. Do not remove the
installation directory first.

After that migration, **Version Management → Install Now** installs releases
from this fork. Checks run automatically; installing an update remains a user
action. A new fork with no published releases will report that its release
manifest is unavailable until the first release is published.

## Bring in upstream improvements

Run these commands from the repository. Add the remote only once:

```powershell
git remote add upstream https://github.com/router-for-me/EasyCLIProxyAPI.git
git fetch upstream
git merge upstream/main
```

Resolve any conflicts while retaining the Codex import conversion, the fork's
application update URLs, and the Windows/Linux release matrix. Commit the merge
and push `main` to `origin`. Use a merge to retain the fork's commits; GitHub's
**Discard commits** operation would remove these changes.

The release workflow runs the import and updater regression tests before
building packages. Review upstream changes before merging them; a successful
build alone cannot establish that every custom behavior was retained.

## Publish an update

1. Open the fork's **Actions → Build portable releases → Run workflow**.
2. Select `main` and enter a new release tag, for example `v0.3.77`. Choose a
   version higher than the last version published from this fork.
3. Set **Mark a manually published release as a pre-release** to **false**.
   The default stable updater uses GitHub's latest stable release.
4. Run the workflow. It builds Windows and Linux for x64 and ARM64, then
   publishes the packages and `portable-update-windows.json` /
   `portable-update-linux.json` with their SHA-256 hashes.
5. Open **Version Management** in the app and check for updates.

Leave the release tag empty to produce test artifacts without publishing a
release. The fork does not need macOS runners or Apple signing credentials.

The workflow uses GitHub's automatic `GITHUB_TOKEN`. Leave
`GITCODE_GUI_REPOSITORY` unset unless you operate a mirror of this fork;
an upstream application mirror does not contain our changes. The source code's
Cargo version is overridden by the release tag during the workflow, so the
compiled binary, portable marker, package names, and update manifest agree.
