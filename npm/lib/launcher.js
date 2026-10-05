import { spawn } from 'node:child_process';
import { constants } from 'node:fs';
import { access, readFile, realpath, stat } from 'node:fs/promises';
import { createRequire } from 'node:module';
import { delimiter, dirname, extname, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import { selectPlatformTarget } from './platforms.js';

const commandNames = new Set(['cadder', 'cadderd', 'caddy']);
const defaultWindowsExtensions = ['.COM', '.EXE', '.BAT', '.CMD'];

function normalizedFilePath(value, platform) {
  const normalized = resolve(value).replaceAll('\\', '/');
  return platform === 'win32' ? normalized.toLowerCase() : normalized;
}

async function sameFile(left, right, platform) {
  try {
    const [leftPath, rightPath] = await Promise.all([realpath(left), realpath(right)]);
    return normalizedFilePath(leftPath, platform) === normalizedFilePath(rightPath, platform);
  } catch {
    return false;
  }
}

async function isFile(path) {
  try {
    return (await stat(path)).isFile();
  } catch {
    return false;
  }
}

async function resolveCommandFromEntry(entry, commandName, platform, pathExt) {
  if (!entry) return undefined;
  if (platform !== 'win32') {
    const candidate = resolve(entry, commandName);
    try {
      await access(candidate, constants.X_OK);
      return candidate;
    } catch {
      return undefined;
    }
  }

  const extensions = pathExt
    .split(';')
    .map((extension) => extension.trim())
    .filter(Boolean);
  for (const extension of extensions.length > 0 ? extensions : defaultWindowsExtensions) {
    const candidate = resolve(entry, `${commandName}${extension.toLowerCase()}`);
    if (await isFile(candidate)) return candidate;
  }
  return undefined;
}

async function wrapperTargetsLauncher(candidate, entry, launcherPath, platform) {
  if (await sameFile(candidate, launcherPath, platform)) return true;

  const extension = extname(candidate).toLowerCase();
  if (platform === 'win32' && extension !== '.cmd' && extension !== '.bat') return false;
  try {
    const candidateStat = await stat(candidate);
    if (candidateStat.size > 64 * 1024) return false;
    const wrapper = (await readFile(candidate, 'utf8')).replaceAll('\\', '/');
    const relativeLauncher = relative(entry, launcherPath).replaceAll('\\', '/');
    const normalizedWrapper = platform === 'win32' ? wrapper.toLowerCase() : wrapper;
    const normalizedRelative = platform === 'win32' ? relativeLauncher.toLowerCase() : relativeLauncher;
    const normalizedLauncher = normalizedFilePath(launcherPath, platform);
    return normalizedWrapper.includes(normalizedRelative) || normalizedWrapper.includes(normalizedLauncher);
  } catch {
    return false;
  }
}

export async function removeLauncherFromPath({
  pathValue,
  launcherPath,
  commandName = 'caddy',
  platform = process.platform,
  pathDelimiter = delimiter,
  pathExt = process.env.PATHEXT ?? '',
}) {
  if (!pathValue) return pathValue;
  const retainedEntries = [];
  for (const entry of pathValue.split(pathDelimiter)) {
    const candidate = await resolveCommandFromEntry(entry, commandName, platform, pathExt);
    if (candidate === undefined || !(await wrapperTargetsLauncher(candidate, entry, launcherPath, platform))) {
      retainedEntries.push(entry);
    }
  }
  return retainedEntries.join(pathDelimiter);
}

async function resolveNativeExecutable(commandName, target, requireFromLauncher) {
  let manifestPath;
  try {
    manifestPath = requireFromLauncher.resolve(`${target.packageName}/package.json`);
  } catch (error) {
    if (error?.code !== 'MODULE_NOT_FOUND') throw error;
    throw new Error(
      `The optional package ${target.packageName} is missing. Reinstall cadder without omitting optional dependencies.`,
    );
  }
  const executable = resolve(dirname(manifestPath), 'bin', `${commandName}${target.executableSuffix}`);
  try {
    await access(executable, process.platform === 'win32' ? constants.F_OK : constants.X_OK);
  } catch {
    throw new Error(`The native executable ${commandName} is missing from ${target.packageName}.`);
  }
  return executable;
}

function environmentPathKey(environment) {
  return Object.keys(environment).find((key) => key.toLowerCase() === 'path') ?? 'PATH';
}

function forwardableSignals(platform) {
  return platform === 'win32' ? ['SIGINT', 'SIGTERM'] : ['SIGHUP', 'SIGINT', 'SIGTERM'];
}

export async function launchNative(commandName, launcherUrl, argumentsValue = process.argv.slice(2)) {
  if (!commandNames.has(commandName)) throw new Error(`Unknown Cadder command: ${commandName}.`);
  const target = selectPlatformTarget();
  const launcherPath = fileURLToPath(launcherUrl);
  const requireFromLauncher = createRequire(launcherUrl);
  const executable = await resolveNativeExecutable(commandName, target, requireFromLauncher);
  const environment = { ...process.env };

  if (commandName === 'caddy') {
    const pathKey = environmentPathKey(environment);
    environment[pathKey] = await removeLauncherFromPath({
      pathValue: environment[pathKey],
      launcherPath,
      commandName,
    });
  }

  const child = spawn(executable, argumentsValue, {
    env: environment,
    shell: false,
    stdio: 'inherit',
    windowsHide: false,
  });
  const signalHandlers = new Map();
  const removeSignalHandlers = () => {
    for (const [signal, handler] of signalHandlers) process.off(signal, handler);
    signalHandlers.clear();
  };
  for (const signal of forwardableSignals(process.platform)) {
    const handler = () => {
      if (child.exitCode === null && child.signalCode === null) child.kill(signal);
    };
    signalHandlers.set(signal, handler);
    process.on(signal, handler);
  }

  try {
    const outcome = await new Promise((resolveExit, rejectExit) => {
      child.once('error', rejectExit);
      child.once('exit', (code, signal) => resolveExit({ code, signal }));
    });
    if (outcome.signal && process.platform !== 'win32') {
      removeSignalHandlers();
      process.kill(process.pid, outcome.signal);
      return;
    }
    process.exitCode = outcome.code ?? 1;
  } finally {
    removeSignalHandlers();
  }
}

export async function runLauncher(commandName, launcherUrl) {
  try {
    await launchNative(commandName, launcherUrl);
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    process.stderr.write(`${commandName}: ${message}\n`);
    process.exitCode = 1;
  }
}
