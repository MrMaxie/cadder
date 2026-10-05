import { mkdir } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import sharp from 'sharp';

const root = new URL('../', import.meta.url);
const logoPath = fileURLToPath(new URL('src/assets/logo.png', root));
const outputPath = fileURLToPath(new URL('public/og-banner.png', root));

await mkdir(fileURLToPath(new URL('public/', root)), { recursive: true });

const logo = await sharp(logoPath).resize(142, 142).png().toBuffer();
const type = Buffer.from(`
  <svg width="1200" height="630" xmlns="http://www.w3.org/2000/svg">
    <style>
      .eyebrow { font: 600 22px Arial, sans-serif; letter-spacing: 4px; fill: #5bdabf; }
      .title { font: 700 78px Arial, sans-serif; letter-spacing: -3px; fill: #e7ebee; }
      .body { font: 400 30px Arial, sans-serif; fill: #7f8a91; }
      .mono { font: 600 23px Consolas, monospace; fill: #e7ebee; }
      .label { font: 600 18px Consolas, monospace; fill: #5bdabf; }
    </style>
    <text x="236" y="100" class="eyebrow">LOCAL DEVELOPMENT ROUTING</text>
    <text x="236" y="190" class="title">Cadder</text>
    <text x="70" y="292" class="body">Open local apps by name.</text>
    <line x1="70" y1="352" x2="1130" y2="352" stroke="#374047" />
    <text x="70" y="410" class="label">PROJECT ROUTES</text>
    <text x="70" y="452" class="mono">stay with the code</text>
    <line x1="390" y1="392" x2="390" y2="502" stroke="#374047" />
    <text x="440" y="410" class="label">CONFLICT CHECKS</text>
    <text x="440" y="452" class="mono">fail before reload</text>
    <line x1="780" y1="392" x2="780" y2="502" stroke="#374047" />
    <text x="830" y="410" class="label">OPERATOR</text>
    <text x="830" y="452" class="mono">one CLI + TUI</text>
    <text x="70" y="575" class="body">maxie.dev/cadder</text>
  </svg>
`);

await sharp({
  create: {
    width: 1200,
    height: 630,
    channels: 4,
    background: '#0a0c0f',
  },
})
  .composite([
    { input: logo, left: 70, top: 61 },
    { input: type, left: 0, top: 0 },
  ])
  .png({ compressionLevel: 9, adaptiveFiltering: true })
  .toFile(outputPath);
