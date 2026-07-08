const cadderRuntimeAssetPatterns = {
  'windows-x64': /^cadder-v?\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?-windows-x64\.zip$/,
  'macos-arm64': /^cadder-v?\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?-macos-arm64\.tar\.gz$/,
  'macos-x64': /^cadder-v?\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?-macos-x64\.tar\.gz$/,
  'linux-x64': /^cadder-v?\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?-linux-x64\.tar\.gz$/,
};

const cadderRuntimeInstallerAssetPatterns = {
  'windows-x64-msi': /^cadder-runtime-.+-windows-x64\.msi$/,
  'macos-arm64-pkg': /^cadder-runtime-.+-macos-arm64\.pkg$/,
  'macos-x64-pkg': /^cadder-runtime-.+-macos-x64\.pkg$/,
  'linux-x64-deb': /^cadder-runtime-.+-linux-x64\.deb$/,
  'linux-x64-rpm': /^cadder-runtime-.+-linux-x64\.rpm$/,
};

function updateCadderDownloadGroup(release, selector, assetAttribute, patterns) {
  document.querySelectorAll(selector).forEach((link) => {
    const key = link.getAttribute(assetAttribute);
    const pattern = patterns[key];
    const asset = release.assets.find((candidate) => pattern?.test(candidate.name));
    if (asset?.browser_download_url) {
      link.href = asset.browser_download_url;
    }
  });
}

async function updateCadderDownloadLinks() {
  try {
    const response = await fetch(
      'https://api.github.com/repos/MrMaxie/cadder/releases?per_page=1'
    );
    if (!response.ok) return;

    const releases = await response.json();
    const release = Array.isArray(releases) ? releases[0] : null;
    if (!release?.assets) return;

    updateCadderDownloadGroup(
      release,
      '[data-cadder-asset]',
      'data-cadder-asset',
      cadderRuntimeAssetPatterns
    );
    updateCadderDownloadGroup(
      release,
      '[data-cadder-runtime-installer-asset]',
      'data-cadder-runtime-installer-asset',
      cadderRuntimeInstallerAssetPatterns
    );
  } catch {
    return;
  }
}

updateCadderDownloadLinks();
