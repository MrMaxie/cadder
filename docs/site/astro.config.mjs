import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';

export default defineConfig({
  site: 'https://maxie.dev',
  base: '/Cadder',
  integrations: [
    starlight({
      title: 'Cadder',
      description:
        'Documentation for the Cadder cross-platform Rust Caddy coordinator.',
      favicon: 'favicon.ico',
      logo: {
        src: './src/assets/logo.png',
        alt: 'Cadder logo',
      },
      customCss: ['./src/styles/cadder.css'],
      sidebar: [
        {
          label: 'Quick Start',
          items: [
            { label: 'Overview', slug: 'index' },
            { label: 'Getting started', slug: 'quick-start/getting-started' },
          ],
        },
        {
          label: 'User guide',
          items: [
            { label: 'How to use', slug: 'user-guide/how-to-use' },
            { label: 'cadderctl', slug: 'user-guide/cadderctl' },
            { label: 'cadder.toml', slug: 'user-guide/cadder-toml' },
            { label: 'PATH and shim strategy', slug: 'user-guide/path-and-shim' },
            { label: 'TUI and diagnostics', slug: 'user-guide/tui-diagnostics' },
          ],
        },
        {
          label: 'Cookbooks',
          items: [
            {
              label: 'Windows',
              items: [
                { label: 'Overview', slug: 'cookbooks/windows/overview' },
                { label: 'IIS handoff', slug: 'cookbooks/windows/iis' },
              ],
            },
            { label: 'macOS', slug: 'cookbooks/macos' },
            { label: 'Linux', slug: 'cookbooks/linux' },
          ],
        },
        {
          label: 'Reference',
          items: [
            { label: 'Runtime and configuration', slug: 'reference/runtime-configuration' },
            { label: 'Real Caddy resolution', slug: 'reference/real-caddy-resolution' },
          ],
        },
      ],
    }),
  ],
});
