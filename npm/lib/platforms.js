const targets = [
  {
    key: 'win32:x64',
    packageName: '@maxiedev/cadder-win32-x64',
    directory: 'win32-x64',
    archive: 'cadder-x86_64-pc-windows-msvc.zip',
    executableSuffix: '.exe',
    os: ['win32'],
    cpu: ['x64'],
  },
  {
    key: 'linux:x64:glibc',
    packageName: '@maxiedev/cadder-linux-x64-gnu',
    directory: 'linux-x64-gnu',
    archive: 'cadder-x86_64-unknown-linux-gnu.tar.xz',
    executableSuffix: '',
    os: ['linux'],
    cpu: ['x64'],
    libc: ['glibc'],
  },
  {
    key: 'darwin:x64',
    packageName: '@maxiedev/cadder-darwin-x64',
    directory: 'darwin-x64',
    archive: 'cadder-x86_64-apple-darwin.tar.xz',
    executableSuffix: '',
    os: ['darwin'],
    cpu: ['x64'],
  },
  {
    key: 'darwin:arm64',
    packageName: '@maxiedev/cadder-darwin-arm64',
    directory: 'darwin-arm64',
    archive: 'cadder-aarch64-apple-darwin.tar.xz',
    executableSuffix: '',
    os: ['darwin'],
    cpu: ['arm64'],
  },
];

export const platformTargets = Object.freeze(targets.map((target) => Object.freeze(target)));

export function runtimeLibc(report = process.report?.getReport?.()) {
  if (process.platform !== 'linux') return undefined;
  return report?.header?.glibcVersionRuntime ? 'glibc' : 'musl';
}

export function selectPlatformTarget({
  platform = process.platform,
  arch = process.arch,
  libc = platform === 'linux' ? runtimeLibc() : undefined,
} = {}) {
  const key = platform === 'linux' ? `${platform}:${arch}:${libc}` : `${platform}:${arch}`;
  const target = platformTargets.find((candidate) => candidate.key === key);
  if (target === undefined) {
    const libcSuffix = platform === 'linux' ? `/${libc ?? 'unknown-libc'}` : '';
    throw new Error(`Cadder does not provide an npm binary for ${platform}/${arch}${libcSuffix}.`);
  }
  return target;
}
