import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';

export default defineConfig({
  site: 'https://maxie.dev/',
  base: '/cadder',
  trailingSlash: 'always',
  compressHTML: true,
  devToolbar: {
    enabled: false,
  },
  integrations: [
    starlight({
      title: 'Cadder',
      description: 'Cadder coordinates Caddy routes across local repositories so their apps can use stable addresses through one shared web server and reverse proxy.',
      favicon: 'favicon.ico',
      logo: {
        src: './src/assets/logo.png',
        alt: 'Cadder logo',
      },
      customCss: ['./src/styles/cadder.css'],
      components: {
        Head: './src/components/CadderHead.astro',
        Hero: './src/components/CadderHero.astro',
        Footer: './src/components/CadderFooter.astro',
      },
      social: [
        {
          icon: 'github',
          label: 'GitHub',
          href: 'https://github.com/MrMaxie/Cadder',
        },
      ],
      sidebar: [
        {
          label: 'Start',
          items: [
            { label: 'Overview', slug: 'index' },
            { label: 'Getting started', slug: 'quick-start/getting-started' },
          ],
        },
        {
          label: 'Use Cadder',
          items: [
            { label: 'Daily workflow', slug: 'user-guide/how-to-use' },
            { label: 'CLI and TUI', slug: 'user-guide/cadder' },
            { label: 'Configure real Caddy', slug: 'user-guide/cadder-toml' },
            { label: 'PATH and caddy shim', slug: 'user-guide/path-and-shim' },
          ],
        },
        {
          label: 'Platform guides',
          items: [
            {
              label: 'Windows',
              items: [
                { label: 'Overview', slug: 'cookbooks/windows/overview' },
              ],
            },
            { label: 'macOS', slug: 'cookbooks/macos' },
            { label: 'Linux', slug: 'cookbooks/linux' },
          ],
        },
        {
          label: 'Reference',
          items: [
            { label: 'Releases', slug: 'reference/releases' },
            { label: 'Runtime configuration', slug: 'reference/runtime-configuration' },
            { label: 'Real Caddy resolution', slug: 'reference/real-caddy-resolution' },
          ],
        },
      ],
    }),
  ],
});
