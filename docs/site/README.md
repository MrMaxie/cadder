# Cadder documentation site

This workspace contains the Astro Starlight documentation source for Cadder.

## Commands

From the repository root, prefer the canonical task runner:

```sh
mise run docs-check
mise run docs-build
```

When working directly in this package, run commands from `docs/site`.

```sh
npm ci
npm run dev
npm run check
npm run build
npm run preview
```

`npm run build` writes generated output to `docs/site/dist`. Do not commit generated output, `.astro`, cache directories, or preview artifacts.

## Content source

The durable architecture notes in `../ARCHITECTURE.md` remain the compact source for process boundaries and runtime behavior. The Starlight pages migrate that material into user-facing documentation and link back to the original file where useful.

## CI handoff

The repository CI validates documentation from source on `master` through `mise run check`, which includes:

```sh
mise run docs-check
```

Release or publishing workflows may additionally run `mise run docs-build` and upload `docs/site/dist`, but they should not write generated site output back to the repository.

Production documentation is published below `https://maxie.dev/cadder/`.
