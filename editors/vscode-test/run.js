#!/usr/bin/env node
// Runs php-lsp inside the real Intelephense VS Code extension:
// downloads VS Code and the extension from the marketplace, replaces the
// bundled Intelephense server with php-lsp's Node launcher and runs the
// checks in `suite/` against the `fixture/` workspace.
//
//   cargo build --release -p php-lsp
//   cd editors/vscode-test && npm install && xvfb-run -a node run.js
'use strict';

const cp = require('child_process');
const fs = require('fs');
const https = require('https');
const path = require('path');
const zlib = require('zlib');
const { downloadAndUnzipVSCode, resolveCliArgsFromVSCodeExecutablePath, runTests } = require('@vscode/test-electron');

const EXTENSION = 'bmewburn.vscode-intelephense-client';
const root = path.resolve(__dirname, '..', '..');
const cache = path.join(__dirname, '.vscode-test');
const binary = process.env.PHP_LSP_BIN || path.join(root, 'target', 'release', 'php-lsp');

function download(url, dest) {
  return new Promise((resolve, reject) => {
    https.get(url, (res) => {
      if (res.statusCode >= 300 && res.statusCode < 400 && res.headers.location) {
        res.resume();
        return download(res.headers.location, dest).then(resolve, reject);
      }
      if (res.statusCode !== 200) {
        return reject(new Error(`${url}: HTTP ${res.statusCode}`));
      }
      const out = fs.createWriteStream(dest);
      // the marketplace serves VSIX files gzip encoded
      (res.headers['content-encoding'] === 'gzip' ? res.pipe(zlib.createGunzip()) : res).pipe(out);
      out.on('finish', () => out.close(resolve));
    }).on('error', reject);
  });
}

async function main() {
  if (!fs.existsSync(binary)) {
    throw new Error(`${binary} not found, run \`cargo build --release -p php-lsp\` first`);
  }
  fs.mkdirSync(cache, { recursive: true });
  const vscode = await downloadAndUnzipVSCode('stable');
  const [cli, ...cliArgs] = resolveCliArgsFromVSCodeExecutablePath(vscode);

  // install the extension into a private extensions dir
  const extensionsDir = path.join(cache, 'extensions');
  const vsix = path.join(cache, 'intelephense.vsix');
  if (!fs.existsSync(vsix)) {
    const [publisher, name] = EXTENSION.split('.');
    await download(
      `https://marketplace.visualstudio.com/_apis/public/gallery/publishers/${publisher}/vsextensions/${name}/latest/vspackage`,
      vsix,
    );
  }
  const args = cliArgs.map((a) => (a.startsWith('--extensions-dir=') ? `--extensions-dir=${extensionsDir}` : a));
  if (!args.some((a) => a.startsWith('--extensions-dir'))) args.push(`--extensions-dir=${extensionsDir}`);
  cp.execFileSync(cli, [...args, '--install-extension', vsix, '--force'], { stdio: 'inherit', shell: process.platform === 'win32' });

  // swap the bundled server for php-lsp
  const ext = fs.readdirSync(extensionsDir).find((d) => d.startsWith(EXTENSION));
  const lib = path.join(extensionsDir, ext, 'node_modules', 'intelephense', 'lib');
  fs.mkdirSync(path.join(lib, 'bin'), { recursive: true });
  fs.copyFileSync(path.join(root, 'editors', 'node', 'php-lsp.js'), path.join(lib, 'intelephense.js'));
  fs.rmSync(path.join(lib, 'bin', 'php-lsp'), { force: true });
  fs.symlinkSync(binary, path.join(lib, 'bin', 'php-lsp'));
  console.log(`using ${ext} with ${binary}`);

  // fresh copy of the fixture (the suite edits files)
  const workspace = path.join(cache, 'workspace');
  fs.rmSync(workspace, { recursive: true, force: true });
  fs.cpSync(path.join(__dirname, 'fixture'), workspace, { recursive: true });

  await runTests({
    vscodeExecutablePath: vscode,
    extensionDevelopmentPath: path.join(__dirname, 'suite'),
    extensionTestsPath: path.join(__dirname, 'suite', 'index.js'),
    launchArgs: [workspace, `--extensions-dir=${extensionsDir}`, '--disable-workspace-trust', '--skip-welcome', '--skip-release-notes'],
  });
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
