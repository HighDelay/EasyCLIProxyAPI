import { spawnSync } from 'node:child_process';
import { copyFile, mkdir } from 'node:fs/promises';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const target = resolve(process.env.CARGO_TARGET_DIR || join(root, 'src-tauri', 'target'));
const result = spawnSync('cargo', ['build', '--locked', '--release', '--manifest-path', join(root, 'plugins/model-access/Cargo.toml'), '--target-dir', target], { stdio: 'inherit', windowsHide: true });
if (result.error) throw result.error;
if (result.status !== 0) process.exit(result.status || 1);
const os = { win32: 'windows', linux: 'linux', darwin: 'macos' }[process.platform];
const arch = { x64: 'x86_64', arm64: 'aarch64' }[process.arch];
if (!os || !arch) throw new Error('Unsupported model policy plugin platform');
const library = process.platform === 'win32' ? 'cpa_model_access.dll' : process.platform === 'darwin' ? 'libcpa_model_access.dylib' : 'libcpa_model_access.so';
const directory = join(root, 'src-tauri/resources/model-access');
await mkdir(directory, { recursive: true });
await copyFile(join(target, 'release', library), join(directory, `${os}-${arch}.bin`));
