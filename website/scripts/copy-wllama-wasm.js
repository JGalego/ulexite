#!/usr/bin/env node
// Self-hosts `@wllama/wllama`'s wasm binary under `static/wllama/`, the
// same way `crates/ulx-wasm` is self-hosted under `static/wasm/` (a plain
// static asset the Run panel fetches by URL) rather than depending on the
// package's own CDN fallback — which, as of the pinned version, only ships
// a *-compat CDN URL, not the plain one the default (non-compat) code path
// needs. Runs as an npm `postinstall` hook, so a plain `npm ci`/`npm
// install` is enough to keep this in sync with the pinned dependency
// version; nothing here is hand-written or meant to be edited directly.

const fs = require('fs');
const path = require('path');

const src = path.join(
  __dirname,
  '..',
  'node_modules',
  '@wllama',
  'wllama',
  'esm',
  'wasm',
  'wllama.wasm',
);
const destDir = path.join(__dirname, '..', 'static', 'wllama');
const dest = path.join(destDir, 'wllama.wasm');

if (!fs.existsSync(src)) {
  console.warn(
    `[copy-wllama-wasm] ${src} not found — is @wllama/wllama installed? Skipping.`,
  );
  process.exit(0);
}

fs.mkdirSync(destDir, { recursive: true });
fs.copyFileSync(src, dest);
console.log(`[copy-wllama-wasm] copied wllama.wasm -> ${path.relative(process.cwd(), dest)}`);
