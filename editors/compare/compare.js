#!/usr/bin/env node
// Compares php-lsp with the original Intelephense server on a project:
// diagnostics, hover, definition and member completion at sampled
// positions. Intelephense is taken from the VS Code extension package
// downloaded by ../vscode-test/run.js (it is only run locally, nothing of it
// is part of this repository).
//
//   node compare.js <project dir> [--files N] [--probes N] [--out report.json]
//        [--intelephense path/to/intelephense.js] [--php-lsp path/to/php-lsp]
'use strict';

const cp = require('child_process');
const fs = require('fs');
const os = require('os');
const path = require('path');

const root = path.resolve(__dirname, '..', '..');

function parseArgs() {
  const args = process.argv.slice(2);
  const opts = { files: 200, probes: 15, out: null, intelephense: null, phpLsp: path.join(root, 'target', 'release', 'php-lsp') };
  const rest = [];
  for (let i = 0; i < args.length; i++) {
    const a = args[i];
    if (a === '--files') opts.files = Number(args[++i]);
    else if (a === '--probes') opts.probes = Number(args[++i]);
    else if (a === '--out') opts.out = args[++i];
    else if (a === '--intelephense') opts.intelephense = args[++i];
    else if (a === '--php-lsp') opts.phpLsp = args[++i];
    else rest.push(a);
  }
  if (!rest[0]) {
    console.error('usage: compare.js <project dir> [--files N] [--probes N] [--out report.json]');
    process.exit(2);
  }
  opts.project = path.resolve(rest[0]);
  return opts;
}

/** Path of the original server, extracted from the cached extension package. */
function intelephenseServer(explicit) {
  if (explicit) return explicit;
  const cache = path.join(__dirname, '.cache');
  const js = path.join(cache, 'extension', 'node_modules', 'intelephense', 'lib', 'intelephense.js');
  if (fs.existsSync(js)) return js;
  const vsix = path.join(__dirname, '..', 'vscode-test', '.vscode-test', 'intelephense.vsix');
  if (!fs.existsSync(vsix)) {
    throw new Error(`${vsix} not found: run ../vscode-test/run.js once (it downloads the extension)`);
  }
  fs.mkdirSync(cache, { recursive: true });
  cp.execFileSync('unzip', ['-oq', vsix, 'extension/node_modules/intelephense/*', '-d', cache]);
  return js;
}

// ------------------------------------------------------------------ client

class Client {
  constructor(name, command, args, settings) {
    this.name = name;
    this.settings = settings;
    this.proc = cp.spawn(command, args, { stdio: ['pipe', 'pipe', 'ignore'] });
    this.buf = Buffer.alloc(0);
    this.nextId = 1;
    this.pending = new Map();
    this.listeners = [];
    this.proc.stdout.on('data', (d) => this.onData(d));
  }

  onData(chunk) {
    this.buf = Buffer.concat([this.buf, chunk]);
    for (;;) {
      const sep = this.buf.indexOf('\r\n\r\n');
      if (sep < 0) return;
      const header = this.buf.slice(0, sep).toString();
      const len = Number(/Content-Length: (\d+)/i.exec(header)[1]);
      if (this.buf.length < sep + 4 + len) return;
      const body = JSON.parse(this.buf.slice(sep + 4, sep + 4 + len).toString());
      this.buf = this.buf.slice(sep + 4 + len);
      this.onMessage(body);
    }
  }

  onMessage(msg) {
    if (msg.id !== undefined && msg.method === undefined) {
      const p = this.pending.get(msg.id);
      if (p) {
        this.pending.delete(msg.id);
        clearTimeout(p.timer);
        p.resolve(msg.error ? null : msg.result);
      }
      return;
    }
    if (msg.id !== undefined) {
      // server -> client request
      let result = null;
      if (msg.method === 'workspace/configuration') {
        result = msg.params.items.map((i) => (i.section === 'intelephense' ? this.settings : null));
      }
      this.send({ jsonrpc: '2.0', id: msg.id, result });
      return;
    }
    for (const l of this.listeners) l(msg);
  }

  send(msg) {
    const s = JSON.stringify(msg);
    this.proc.stdin.write(`Content-Length: ${Buffer.byteLength(s)}\r\n\r\n${s}`);
  }

  request(method, params, timeout = 20000) {
    const id = this.nextId++;
    return new Promise((resolve) => {
      const timer = setTimeout(() => {
        this.pending.delete(id);
        resolve(undefined);
      }, timeout);
      this.pending.set(id, { resolve, timer });
      this.send({ jsonrpc: '2.0', id, method, params });
    });
  }

  notify(method, params) {
    this.send({ jsonrpc: '2.0', method, params });
  }

  waitFor(pred, timeout) {
    return new Promise((resolve) => {
      const timer = setTimeout(() => {
        this.listeners = this.listeners.filter((l) => l !== l2);
        resolve(null);
      }, timeout);
      const l2 = (msg) => {
        if (pred(msg)) {
          clearTimeout(timer);
          this.listeners = this.listeners.filter((l) => l !== l2);
          resolve(msg);
        }
      };
      this.listeners.push(l2);
    });
  }

  async start(rootUri, storage) {
    const indexed = this.waitFor((m) => m.method === 'indexingEnded', 600000);
    await this.request('initialize', {
      processId: process.pid,
      rootUri,
      workspaceFolders: [{ uri: rootUri, name: 'project' }],
      clientInfo: { name: 'php-lsp-compare' },
      initializationOptions: { storagePath: storage, globalStoragePath: storage, clearCache: true },
      capabilities: {
        workspace: { configuration: true, workspaceFolders: true },
        textDocument: {
          hover: { contentFormat: ['markdown', 'plaintext'] },
          completion: { completionItem: { snippetSupport: true } },
          definition: { linkSupport: false },
          publishDiagnostics: {},
        },
      },
    }, 600000);
    this.notify('initialized', {});
    const t = Date.now();
    await indexed;
    return Date.now() - t;
  }

  stop() {
    this.proc.kill();
  }
}

// ------------------------------------------------------------------ helpers

const uriOf = (p) => 'file://' + p.split(path.sep).map(encodeURIComponent).join('/').replace(/^file:\/\/\//, 'file:///');

function phpFiles(dir, out = []) {
  for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
    if (e.name.startsWith('.') || e.name === 'node_modules') continue;
    const p = path.join(dir, e.name);
    if (e.isDirectory()) phpFiles(p, out);
    else if (e.name.endsWith('.php') && fs.statSync(p).size < 300000) out.push(p);
  }
  return out;
}

function positionAt(text, offset) {
  const before = text.slice(0, offset);
  const line = before.split('\n').length - 1;
  return { line, character: offset - (before.lastIndexOf('\n') + 1) };
}

/** Sample positions: member names, static members, names before `(`, variables, class names. */
function probes(text, max) {
  const found = [];
  const re = /(->|::)\s*([A-Za-z_]\w*)|\bnew\s+(\\?[A-Za-z_][\w\\]*)|(\$[A-Za-z_]\w*)|\b([A-Za-z_]\w*)\s*\(/g;
  let m;
  while ((m = re.exec(text))) {
    const word = m[2] || m[3] || m[4] || m[5];
    if (/^(if|for|foreach|while|switch|function|fn|array|list|isset|empty|unset|return|echo|match|catch|elseif|print|exit|die)$/i.test(word)) continue;
    const start = m.index + m[0].indexOf(word);
    found.push({ offset: start + Math.min(1, word.length - 1), word, kind: m[2] ? 'member' : m[3] ? 'class' : m[4] ? 'variable' : 'call' });
  }
  if (found.length <= max) return found;
  const step = found.length / max;
  return Array.from({ length: max }, (_, i) => found[Math.floor(i * step)]);
}

/** Positions right after `->` for completion. */
function arrows(text, max) {
  const out = [];
  const re = /\$[A-Za-z_]\w*->/g;
  let m;
  while ((m = re.exec(text)) && out.length < max) out.push(m.index + m[0].length);
  return out;
}

const hoverText = (h) => {
  if (!h || !h.contents) return '';
  const c = h.contents;
  if (typeof c === 'string') return c;
  if (Array.isArray(c)) return c.map((x) => (typeof x === 'string' ? x : x.value)).join('\n');
  return c.value || '';
};

/** First definition target: file name (stubs live at different places) and ranges. */
const defTarget = (d) => {
  if (!d) return null;
  const list = Array.isArray(d) ? d : [d];
  if (!list.length) return null;
  const l = list[0];
  const file = decodeURIComponent(l.targetUri || l.uri).split('/').pop();
  const range = l.targetRange || l.range;
  const sel = l.targetSelectionRange || l.range;
  return { file, start: range.start.line, end: range.end.line, line: sel.start.line, key: `${file}:${sel.start.line + 1}` };
};

/** Same target: same file and one position inside the other's declaration range. */
const sameTarget = (a, b) =>
  a.file === b.file && ((b.line >= a.start && b.line <= a.end) || (a.line >= b.start && a.line <= b.end));

// ------------------------------------------------------------------ main

async function main() {
  const opts = parseArgs();
  const settings = { environment: { phpVersion: '8.4.0' }, diagnostics: { enable: true }, telemetry: { enabled: false } };
  const storage = fs.mkdtempSync(path.join(os.tmpdir(), 'php-lsp-compare-'));
  const theirs = new Client('intelephense', process.execPath, [intelephenseServer(opts.intelephense), '--stdio'], settings);
  const ours = new Client('php-lsp', opts.phpLsp, ['--stdio'], settings);
  const rootUri = uriOf(opts.project);
  const [tTheirs, tOurs] = await Promise.all([theirs.start(rootUri, path.join(storage, 'i')), ours.start(rootUri, path.join(storage, 'p'))]);
  console.log(`indexed: intelephense ${tTheirs} ms, php-lsp ${tOurs} ms`);

  let files = phpFiles(opts.project).filter((f) => !f.includes(`${path.sep}vendor${path.sep}`));
  if (files.length > opts.files) {
    const step = files.length / opts.files;
    files = Array.from({ length: opts.files }, (_, i) => files[Math.floor(i * step)]);
  }

  const report = {
    project: opts.project,
    files: files.length,
    index_ms: { intelephense: tTheirs, php_lsp: tOurs },
    diagnostics: {},
    definition: { same: 0, different: 0, onlyTheirs: 0, onlyOurs: 0, examples: [] },
    hover: { both: 0, onlyTheirs: 0, onlyOurs: 0, examples: [] },
    completion: { positions: 0, theirLabels: 0, matched: 0, examples: [] },
  };
  const diagBucket = (code) => (report.diagnostics[code] ||= { both: 0, onlyTheirs: 0, onlyOurs: 0, examples: [] });
  const example = (list, item) => list.length < 40 && list.push(item);

  for (const [n, file] of files.entries()) {
    const text = fs.readFileSync(file, 'utf8');
    const uri = uriOf(file);
    const rel = path.relative(opts.project, file);
    const diag = (c) => c.waitFor((m) => m.method === 'textDocument/publishDiagnostics' && m.params.uri === uri, 15000);
    const [dt, dy] = [diag(theirs), diag(ours)];
    for (const c of [theirs, ours]) c.notify('textDocument/didOpen', { textDocument: { uri, languageId: 'php', version: 1, text } });
    const [mt, my] = await Promise.all([dt, dy]);
    const key = (d) => `${d.code}:${d.range.start.line}`;
    const listT = (mt ? mt.params.diagnostics : []).filter((d) => d.source !== 'php-lsp-tools');
    const listY = my ? my.params.diagnostics : [];
    const keysY = new Set(listY.map(key));
    const keysT = new Set(listT.map(key));
    for (const d of listT) {
      const b = diagBucket(d.code);
      if (keysY.has(key(d))) b.both++;
      else {
        b.onlyTheirs++;
        example(b.examples, `- intelephense ${rel}:${d.range.start.line + 1}: ${d.message}`);
      }
    }
    for (const d of listY) {
      if (keysT.has(key(d))) continue;
      const b = diagBucket(d.code);
      b.onlyOurs++;
      example(b.examples, `+ php-lsp ${rel}:${d.range.start.line + 1}: ${d.message}`);
    }

    for (const p of probes(text, opts.probes)) {
      const position = positionAt(text, p.offset);
      const params = { textDocument: { uri }, position };
      const [ht, hy, gt, gy] = await Promise.all([
        theirs.request('textDocument/hover', params),
        ours.request('textDocument/hover', params),
        theirs.request('textDocument/definition', params),
        ours.request('textDocument/definition', params),
      ]);
      const at = `${rel}:${position.line + 1}:${position.character + 1} ${p.kind} ${p.word}`;
      const [a, b] = [hoverText(ht).trim(), hoverText(hy).trim()];
      if (a && b) report.hover.both++;
      else if (a) {
        report.hover.onlyTheirs++;
        example(report.hover.examples, `- ${at}`);
      } else if (b) report.hover.onlyOurs++;
      const [ka, kb] = [defTarget(gt), defTarget(gy)];
      const d = report.definition;
      if (ka && kb) {
        if (sameTarget(ka, kb)) d.same++;
        else {
          d.different++;
          example(d.examples, `~ ${at}: intelephense ${ka.key}, php-lsp ${kb.key}`);
        }
      } else if (ka) {
        d.onlyTheirs++;
        example(d.examples, `- ${at}: intelephense ${ka.key}`);
      } else if (kb) {
        d.onlyOurs++;
        example(d.examples, `+ ${at}: php-lsp ${kb.key}`);
      }
    }

    for (const offset of arrows(text, 3)) {
      const position = positionAt(text, offset);
      const params = { textDocument: { uri }, position, context: { triggerKind: 2, triggerCharacter: '>' } };
      const [ct, cy] = await Promise.all([theirs.request('textDocument/completion', params), ours.request('textDocument/completion', params)]);
      const labels = (c) => new Set(((c && (c.items || c)) || []).map((i) => i.label));
      const [lt, ly] = [labels(ct), labels(cy)];
      if (!lt.size) continue;
      report.completion.positions++;
      report.completion.theirLabels += lt.size;
      const missing = [...lt].filter((l) => !ly.has(l));
      report.completion.matched += lt.size - missing.length;
      if (missing.length) example(report.completion.examples, `- ${rel}:${position.line + 1}:${position.character + 1}: ${missing.slice(0, 8).join(', ')}`);
    }

    for (const c of [theirs, ours]) c.notify('textDocument/didClose', { textDocument: { uri } });
    if ((n + 1) % 25 === 0) console.log(`${n + 1}/${files.length}`);
  }
  theirs.stop();
  ours.stop();

  const d = report.definition;
  const c = report.completion;
  console.log('\ndiagnostics (code: both / only intelephense / only php-lsp):');
  for (const [code, b] of Object.entries(report.diagnostics).sort()) console.log(`  ${code}: ${b.both} / ${b.onlyTheirs} / ${b.onlyOurs}`);
  console.log(`definition: same ${d.same}, different ${d.different}, only intelephense ${d.onlyTheirs}, only php-lsp ${d.onlyOurs}`);
  console.log(`hover: both ${report.hover.both}, only intelephense ${report.hover.onlyTheirs}, only php-lsp ${report.hover.onlyOurs}`);
  console.log(`completion recall: ${c.theirLabels ? ((100 * c.matched) / c.theirLabels).toFixed(1) : '-'}% of ${c.theirLabels} labels at ${c.positions} positions`);
  if (opts.out) {
    fs.writeFileSync(opts.out, JSON.stringify(report, null, 2));
    console.log(`report: ${opts.out}`);
  }
  fs.rmSync(storage, { recursive: true, force: true });
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
