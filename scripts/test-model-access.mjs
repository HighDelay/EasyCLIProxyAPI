import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { mkdtemp, readFile, readdir, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { basename, dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const run = (command, args, env = process.env) => {
  const result = spawnSync(command, args, { cwd: root, env, stdio: 'inherit', windowsHide: true });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`${command} failed with status ${result.status}`);
};
const findCore = async directory => {
  const name = process.platform === 'win32' ? 'cli-proxy-api.exe' : 'cli-proxy-api';
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    const path = join(directory, entry.name);
    if (entry.isFile() && entry.name === name) return path;
    if (entry.isDirectory()) {
      const found = await findCore(path);
      if (found) return found;
    }
  }
};

let temporary;
try {
  let executable = process.env.CPA_V8_TEST_CORE;
  if (!executable) {
    const os = { win32: 'windows', linux: 'linux' }[process.platform];
    const arch = { x64: 'amd64', arm64: 'aarch64' }[process.arch];
    if (!os || !arch) throw new Error('Model access contract tests support Windows and Linux');
    const version = (await readFile(join(root, 'core-version.txt'), 'utf8')).trim().replace(/^v/, '');
    if (!/^\d+(?:\.\d+)+$/.test(version)) throw new Error('Invalid bundled core version');
    const asset = `CLIProxyAPI_${version}_${os}_${arch}.${os === 'windows' ? 'zip' : 'tar.gz'}`;
    const archive = join(root, 'cpa-core', asset);
    const checksums = await readFile(join(root, 'cpa-core/checksums.txt'), 'utf8');
    const expected = checksums.split(/\r?\n/).map(line => line.trim().split(/\s+/))
      .find(parts => parts[1]?.replace(/^\*/, '') === asset)?.[0]?.toLowerCase();
    const actual = createHash('sha256').update(await readFile(archive)).digest('hex');
    if (!expected || actual !== expected) throw new Error(`Bundled core checksum failed: ${asset}`);
    temporary = await mkdtemp(join(tmpdir(), 'cpa-model-access-core-'));
    if (os === 'windows') {
      const quote = value => `'${value.replaceAll("'", "''")}'`;
      run('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command',
        `Expand-Archive -LiteralPath ${quote(archive)} -DestinationPath ${quote(temporary)} -Force`]);
    } else {
      run('tar', ['-xzf', archive, '-C', temporary]);
    }
    executable = await findCore(temporary);
    if (!executable) throw new Error(`Core executable missing from ${asset}`);
  }
  const filters = ['antigravity_quota_and_transient_cooldowns_follow_provider_policy'];
  if (!process.argv.includes('--cooldown-only')) filters.push('tests::core_v8_contract::client_model_exclusions_are_enforced_by_real_core');
  run('cargo', ['test', '--manifest-path', 'src-tauri/Cargo.toml', '--locked', '--', ...filters, '--ignored'],
  { ...process.env, CPA_V8_TEST_CORE: resolve(executable) });
} finally {
  if (temporary && dirname(resolve(temporary)) === resolve(tmpdir())
      && basename(temporary).startsWith('cpa-model-access-core-')) {
    await rm(temporary, { recursive: true, force: true });
  }
}
