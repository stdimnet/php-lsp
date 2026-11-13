#!/usr/bin/env node
// Launcher for the native php-lsp binary, usable wherever a Node.js language
// server module is expected (e.g. `vscode-languageclient` with
// `TransportKind.ipc`, as used by the Intelephense VS Code extension).
//
// It starts `php-lsp` and forwards messages between the client and the
// binary. The binary is looked up in $PHP_LSP_BIN, next to this file
// (`./bin/php-lsp`) and finally on $PATH.
'use strict';

const { spawn } = require('child_process');
const fs = require('fs');
const path = require('path');

function findBinary() {
  if (process.env.PHP_LSP_BIN) {
    return process.env.PHP_LSP_BIN;
  }
  const exe = process.platform === 'win32' ? 'php-lsp.exe' : 'php-lsp';
  const local = path.join(__dirname, 'bin', exe);
  return fs.existsSync(local) ? local : exe;
}

const args = process.argv.slice(2);
const binary = findBinary();

if (process.send && args.includes('--node-ipc')) {
  // Bridge our IPC channel to the binary's IPC channel.
  const child = spawn(binary, ['--node-ipc'], { stdio: ['ignore', 'ignore', 'inherit', 'ipc'] });
  process.on('message', (msg) => child.send(msg));
  child.on('message', (msg) => process.send(msg));
  process.on('disconnect', () => child.kill());
  child.on('exit', (code) => process.exit(code === null ? 1 : code));
  child.on('error', (err) => {
    console.error(`php-lsp: cannot start ${binary}: ${err.message}`);
    process.exit(1);
  });
} else {
  // --stdio / --socket=<port>: let the binary talk to the client directly.
  const passthrough = args.filter((a) => a === '--stdio' || a.startsWith('--socket='));
  const child = spawn(binary, passthrough.length ? passthrough : ['--stdio'], { stdio: 'inherit' });
  child.on('exit', (code) => process.exit(code === null ? 1 : code));
  child.on('error', (err) => {
    console.error(`php-lsp: cannot start ${binary}: ${err.message}`);
    process.exit(1);
  });
}
