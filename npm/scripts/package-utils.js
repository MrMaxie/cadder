import { spawn } from 'node:child_process';
import { createHash } from 'node:crypto';
import { createReadStream } from 'node:fs';
import { mkdir, readdir, readFile, stat } from 'node:fs/promises';
import { dirname, resolve } from 'node:path';

export const documentationFiles = Object.freeze(['README.md', 'CHANGELOG.md', 'LICENSE', 'cadder.toml.example']);
export const commandNames = Object.freeze(['cadder', 'cadderd', 'caddy']);

export function parseArguments(argumentsValue) {
  const result = new Map();
  for (let index = 0; index < argumentsValue.length; index += 1) {
    const argument = argumentsValue[index];
    if (!argument.startsWith('--')) throw new Error(`Unexpected argument: ${argument}`);
    const key = argument.slice(2);
    const next = argumentsValue[index + 1];
    if (next === undefined || next.startsWith('--')) {
      result.set(key, true);
    } else {
      result.set(key, next);
      index += 1;
    }
  }
  return result;
}

export function requiredArgument(argumentsMap, name) {
  const value = argumentsMap.get(name);
  if (typeof value !== 'string' || value.length === 0) throw new Error(`Missing required --${name} argument.`);
  return value;
}

export async function ensureNewDirectory(path) {
  const absolute = resolve(path);
  try {
    await stat(absolute);
    throw new Error(`Output directory already exists: ${absolute}`);
  } catch (error) {
    if (error?.code !== 'ENOENT') throw error;
  }
  await mkdir(absolute, { recursive: true });
  return absolute;
}

export async function readJson(path) {
  return JSON.parse(await readFile(path, 'utf8'));
}

export function parseNpmPackMetadata(output) {
  const parsed = JSON.parse(output);
  const entries = Array.isArray(parsed) ? parsed : Object.values(parsed);
  if (entries.length !== 1) throw new Error(`npm pack returned ${entries.length} package entries.`);
  return entries[0];
}

export async function sha256(path) {
  const hash = createHash('sha256');
  const stream = createReadStream(path);
  for await (const chunk of stream) hash.update(chunk);
  return hash.digest('hex');
}

export async function run(command, argumentsValue, options = {}) {
  const child = spawn(command, argumentsValue, {
    cwd: options.cwd,
    env: options.env ?? process.env,
    shell: false,
    stdio: options.stdio ?? ['ignore', 'pipe', 'pipe'],
    windowsHide: true,
  });
  let stdout = '';
  let stderr = '';
  if (child.stdout) {
    child.stdout.setEncoding('utf8');
    child.stdout.on('data', (data) => {
      stdout += data;
    });
  }
  if (child.stderr) {
    child.stderr.setEncoding('utf8');
    child.stderr.on('data', (data) => {
      stderr += data;
    });
  }
  const exitCode = await new Promise((resolveExit, rejectExit) => {
    child.once('error', rejectExit);
    child.once('exit', resolveExit);
  });
  if (exitCode !== 0) {
    throw new Error(`${command} ${argumentsValue.join(' ')} failed with exit code ${exitCode}\n${stdout}${stderr}`);
  }
  return { stdout, stderr };
}

export async function runNpm(argumentsValue, options = {}) {
  if (process.platform !== 'win32') return run('npm', argumentsValue, options);
  const npmCli = resolve(dirname(process.execPath), 'node_modules', 'npm', 'bin', 'npm-cli.js');
  return run(process.execPath, [npmCli, ...argumentsValue], options);
}

export async function listFiles(root) {
  const files = [];
  async function walk(directory, prefix) {
    for (const entry of await readdir(directory, { withFileTypes: true })) {
      const relativePath = prefix ? `${prefix}/${entry.name}` : entry.name;
      const absolutePath = resolve(directory, entry.name);
      if (entry.isDirectory()) {
        await walk(absolutePath, relativePath);
      } else if (entry.isFile()) {
        files.push(relativePath);
      } else {
        throw new Error(`Unexpected non-file package entry: ${absolutePath}`);
      }
    }
  }
  await walk(root, '');
  return files.sort();
}

export function npmArchiveName(packageName, version) {
  return `${packageName.replace(/^@/, '').replace('/', '-')}-${version}.tgz`;
}
