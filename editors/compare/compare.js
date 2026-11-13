#!/usr/bin/env node
// Compares php-lsp with the original Intelephense server on a project:
// diagnostics, hover, definition and member completion at sampled
// positions. Intelephense is taken from the VS Code extension package
// downloaded by ../vscode-test/run.js (it is only run locally, nothing of it
// is part of this repository).
//
//   node compare.js <project dir> [--files N] [--probes N] [--out report.json]
//        [--intelephense path/to/intelephense.js] [--php-lsp path/to/php-lsp]
//
// PHPV=8.5.0 sets the targeted PHP version, EXACT=1 compares diagnostics by
// range, severity and message (and stores both lists in the report).
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
        if (msg.error) (this.errors[p.method] ||= []).push(msg.error.message);
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
      this.pending.set(id, { resolve, timer, method });
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
    this.errors = {};
    const init = await this.request('initialize', {
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
          references: {},
          rename: { prepareSupport: true },
          formatting: {},
          codeAction: { codeActionLiteralSupport: { codeActionKind: { valueSet: ['', 'quickfix', 'refactor', 'source'] } } },
        },
      },
    }, 600000);
    this.caps = (init && init.capabilities) || {};
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

/** Positions right after `->` (and `Name::` with STATIC=1) for completion. */
function arrows(text, max) {
  const out = [];
  const re = process.env.STATIC ? /\$[A-Za-z_]\w*->|\b[A-Z]\w*::/g : /\$[A-Za-z_]\w*->/g;
  let m;
  while ((m = re.exec(text)) && out.length < max) out.push(m.index + m[0].length);
  return out;
}


/** Applies LSP text edits to `text`. */
function applyEdits(text, edits) {
  const lines = [0];
  for (let i = 0; i < text.length; i++) if (text[i] === '\n') lines.push(i + 1);
  const off = (p) => Math.min(text.length, (lines[p.line] ?? text.length) + p.character);
  const sorted = [...edits].sort((a, b) => off(b.range.start) - off(a.range.start));
  for (const e of sorted) text = text.slice(0, off(e.range.start)) + e.newText + text.slice(off(e.range.end));
  return text;
}

/** Edits of a WorkspaceEdit as a sorted list of `file:line:col-line:col=text`. */
function editKeys(we, project) {
  if (!we) return null;
  const out = [];
  const add = (uri, edits) => {
    const f = path.relative(project, decodeURIComponent(uri.replace(/^file:\/\//, '')));
    for (const e of edits) out.push(`${f}:${e.range.start.line}:${e.range.start.character}-${e.range.end.line}:${e.range.end.character}=${e.newText}`);
  };
  for (const [uri, edits] of Object.entries(we.changes || {})) add(uri, edits);
  for (const dc of we.documentChanges || []) if (dc.edits) add(dc.textDocument.uri, dc.edits);
  return out.sort();
}

const locKeys = (locs, project) =>
  (locs || []).map((l) => `${path.relative(project, decodeURIComponent(l.uri.replace(/^file:\/\//, '')))}:${l.range.start.line}:${l.range.start.character}`).sort();

const hoverText = (h) => {
  if (!h || !h.contents) return '';
  const c = h.contents;
  if (typeof c === 'string') return c;
  if (Array.isArray(c)) return c.map((x) => (typeof x === 'string' ? x : x.value)).join('\n');
  return c.value || '';
};

/** First definition target: file name (stubs live at different places) and ranges. */
const defTarget = (d, project) => {
  if (!d) return null;
  const list = Array.isArray(d) ? d : [d];
  if (!list.length) return null;
  const l = list[0];
  const uri = decodeURIComponent(l.targetUri || l.uri);
  const file = uri.split('/').pop();
  const external = !uri.startsWith(`file://${project}/`);
  const range = l.targetRange || l.range;
  const sel = l.targetSelectionRange || l.range;
  return { file, external, start: range.start.line, end: range.end.line, line: sel.start.line, key: `${file}:${sel.start.line + 1}` };
};

/** Same target: same file and one position inside the other's declaration range. */
const sameTarget = (a, b) =>
  a.file === b.file &&
  // stubs: both servers ship different stub versions
  ((a.external && b.external) || (b.line >= a.start && b.line <= a.end) || (a.line >= b.start && a.line <= b.end));

// ------------------------------------------------------------------ main

async function main() {
  const opts = parseArgs();
  const settings = { environment: { phpVersion: process.env.PHPV || '8.4.0' }, diagnostics: { enable: true }, telemetry: { enabled: false }, completion: { maxItems: Number(process.env.MAXITEMS || 100) } };
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
  // REFACTOR=1: formatting, references, rename and code actions
  const R = process.env.REFACTOR;
  if (R) {
    const caps = (c) => ['referencesProvider', 'renameProvider', 'documentFormattingProvider', 'codeActionProvider'].map((k) => `${k}=${JSON.stringify(c.caps[k] ?? null)}`).join(' ');
    console.log(`capabilities intelephense: ${caps(theirs)}`);
    console.log(`capabilities php-lsp: ${caps(ours)}`);
    report.refactor = {
      format: { both: 0, same: 0, onlyTheirs: 0, onlyOurs: 0, examples: [] },
      references: { both: 0, same: 0, onlyTheirs: 0, onlyOurs: 0, theirs: 0, ours: 0, common: 0, examples: [] },
      rename: { both: 0, same: 0, onlyTheirs: 0, onlyOurs: 0, examples: [] },
      codeAction: { positions: 0, theirTitles: 0, matched: 0, ourTitles: 0, examples: [] },
    };
  }
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
    // EXACT=1 compares whole ranges instead of start lines
    const key = (d) =>
      process.env.EXACT
        ? `${d.code}:${JSON.stringify(d.range)}:${d.severity}:${d.message}`
        : `${d.code}:${d.range.start.line}`;
    const listT = (mt ? mt.params.diagnostics : []).filter((d) => d.source !== 'php-lsp-tools');
    const listY = my ? my.params.diagnostics : [];
    if (process.env.EXACT) (report.exact ||= []).push({ file: rel, theirs: listT, ours: listY });
    const keysY = new Set(listY.map(key));
    const keysT = new Set(listT.map(key));
    for (const d of listT) {
      const b = diagBucket(d.code);
      if (keysY.has(key(d))) b.both++;
      else {
        b.onlyTheirs++;
        const at = process.env.EXACT ? JSON.stringify(d.range) + ' s' + d.severity : '';
        example(b.examples, `- intelephense ${rel}:${d.range.start.line + 1}: ${d.message} ${at}`);
      }
    }
    for (const d of listY) {
      if (keysT.has(key(d))) continue;
      const b = diagBucket(d.code);
      b.onlyOurs++;
      const at = process.env.EXACT ? JSON.stringify(d.range) + ' s' + d.severity : '';
      example(b.examples, `+ php-lsp ${rel}:${d.range.start.line + 1}: ${d.message} ${at}`);
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
      const [ka, kb] = [defTarget(gt, opts.project), defTarget(gy, opts.project)];
      const d = report.definition;
      // DEFS=1: every definition difference with the line, for analysis
      if (process.env.DEFS && !(ka && kb && sameTarget(ka, kb)) && (ka || kb)) {
        (d.all ||= []).push({
          at,
          kind: p.kind,
          word: p.word,
          text: text.split('\n')[position.line].trim(),
          theirs: ka && ka.key,
          ours: kb && kb.key,
          theirsExternal: ka && ka.external,
          oursExternal: kb && kb.external,
        });
      }
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

    for (const offset of arrows(text, Number(process.env.ARROWS || 3))) {
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
      // MISSES=1: every difference with the line, for analysis
      if (process.env.MISSES && (missing.length || [...ly].some((l) => !lt.has(l)))) {
        (report.completion.misses ||= []).push({
          file: rel,
          line: position.line + 1,
          col: position.character + 1,
          text: text.split('\n')[position.line].trim(),
          missing,
          extra: [...ly].filter((l) => !lt.has(l)),
        });
      }
    }

    if (R) {
      const rf = report.refactor;
      const fparams = { textDocument: { uri }, options: { tabSize: 4, insertSpaces: !process.env.FMTTABS } };
      const [ft, fy] = await Promise.all([theirs.request('textDocument/formatting', fparams, 60000), ours.request('textDocument/formatting', fparams, 60000)]);
      if (ft && fy) {
        rf.format.both++;
        const [a, b] = [applyEdits(text, ft), applyEdits(text, fy)];
        if (a === b) rf.format.same++;
        else {
          // FORMATDUMP=dir: both results for diffing
          if (process.env.FORMATDUMP) {
            for (const [who, t] of [['intelephense', a], ['php-lsp', b]]) {
              const f = path.join(process.env.FORMATDUMP, who, rel);
              fs.mkdirSync(path.dirname(f), { recursive: true });
              fs.writeFileSync(f, t);
            }
          }
          const la = a.split('\n');
          const lb = b.split('\n');
          const i = la.findIndex((l, i) => l !== lb[i]);
          (rf.format.diffs ||= []).push({ file: rel, line: i + 1, theirs: la.slice(i, i + 3), ours: lb.slice(i, i + 3) });
          example(rf.format.examples, `~ ${rel}:${i + 1}: intelephense ${JSON.stringify(la[i])} php-lsp ${JSON.stringify(lb[i])}`);
        }
      } else if (ft) rf.format.onlyTheirs++;
      else if (fy) rf.format.onlyOurs++;

      for (const p of probes(text, Math.max(3, Math.floor(opts.probes / 3)))) {
        const position = positionAt(text, p.offset);
        const at = `${rel}:${position.line + 1}:${position.character + 1} ${p.kind} ${p.word}`;
        const rp = { textDocument: { uri }, position, context: { includeDeclaration: true } };
        const [xt, xy] = await Promise.all([theirs.request('textDocument/references', rp, 60000), ours.request('textDocument/references', rp, 60000)]);
        const [kt, ky] = [locKeys(xt, opts.project), locKeys(xy, opts.project)];
        if (kt.length && ky.length) {
          rf.references.both++;
          const sy = new Set(ky);
          const common = kt.filter((k) => sy.has(k)).length;
          rf.references.theirs += kt.length;
          rf.references.ours += ky.length;
          rf.references.common += common;
          if (common === kt.length && common === ky.length) rf.references.same++;
          else {
            const st = new Set(kt);
            (rf.references.diffs ||= []).push({ at, theirs: kt.length, ours: ky.length, common, missing: kt.filter((k) => !sy.has(k)).slice(0, 10), extra: ky.filter((k) => !st.has(k)).slice(0, 10) });
            example(rf.references.examples, `~ ${at}: ${kt.length} vs ${ky.length} (common ${common})`);
          }
        } else if (kt.length) {
          rf.references.onlyTheirs++;
          example(rf.references.examples, `- ${at}: ${kt.length}`);
        } else if (ky.length) {
          rf.references.onlyOurs++;
          example(rf.references.examples, `+ ${at}: ${ky.length}`);
        }

        if (p.word === '$this') continue;
        const np = { textDocument: { uri }, position, newName: p.kind === 'variable' ? '$renamedX' : 'renamedX' };
        const [wt, wy] = await Promise.all([theirs.request('textDocument/rename', np, 60000), ours.request('textDocument/rename', np, 60000)]);
        const [et, ey] = [editKeys(wt, opts.project), editKeys(wy, opts.project)];
        if (et && et.length && ey && ey.length) {
          rf.rename.both++;
          if (et.join('\n') === ey.join('\n')) rf.rename.same++;
          else {
            const [st, sy] = [new Set(et), new Set(ey)];
            (rf.rename.diffs ||= []).push({ at, missing: et.filter((k) => !sy.has(k)).slice(0, 10), extra: ey.filter((k) => !st.has(k)).slice(0, 10) });
            example(rf.rename.examples, `~ ${at}: ${et.length} vs ${ey.length} edits`);
          }
        } else if (et && et.length) {
          rf.rename.onlyTheirs++;
          example(rf.rename.examples, `- ${at}`);
        } else if (ey && ey.length) {
          rf.rename.onlyOurs++;
          example(rf.rename.examples, `+ ${at}`);
        }
      }

      for (const d of listT.slice(0, 10)) {
        const cp = { textDocument: { uri }, range: d.range, context: { diagnostics: [d] } };
        const [at, ay] = await Promise.all([theirs.request('textDocument/codeAction', cp), ours.request('textDocument/codeAction', cp)]);
        const titles = (r) => new Set((r || []).map((a) => a.title));
        const [tt, ty] = [titles(at), titles(ay)];
        if (!tt.size && !ty.size) continue;
        rf.codeAction.positions++;
        rf.codeAction.theirTitles += tt.size;
        rf.codeAction.ourTitles += ty.size;
        const missing = [...tt].filter((t) => !ty.has(t));
        rf.codeAction.matched += tt.size - missing.length;
        if (missing.length || [...ty].some((t) => !tt.has(t)))
          (rf.codeAction.diffs ||= []).push({ at: `${rel}:${d.range.start.line + 1} ${d.code}`, missing, extra: [...ty].filter((t) => !tt.has(t)) });
      }
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
  if (R) {
    const rf = report.refactor;
    console.log(`formatting: both ${rf.format.both} (same ${rf.format.same}), only intelephense ${rf.format.onlyTheirs}, only php-lsp ${rf.format.onlyOurs}`);
    const r = rf.references;
    console.log(`references: both ${r.both} (same ${r.same}, ${r.common}/${r.theirs} of intelephense's found, ${r.ours} ours), only intelephense ${r.onlyTheirs}, only php-lsp ${r.onlyOurs}`);
    console.log(`rename: both ${rf.rename.both} (same ${rf.rename.same}), only intelephense ${rf.rename.onlyTheirs}, only php-lsp ${rf.rename.onlyOurs}`);
    const a = rf.codeAction;
    console.log(`code actions: ${a.matched}/${a.theirTitles} of intelephense's titles at ${a.positions} diagnostics, ${a.ourTitles} ours`);
    for (const c of [theirs, ours]) for (const [m, e] of Object.entries(c.errors)) console.log(`  ${c.name} errors ${m}: ${e.length} (${[...new Set(e)].slice(0, 3).join(' | ')})`);
  }
  if (opts.out) {
    fs.writeFileSync(opts.out, JSON.stringify(report, null, 2));
    console.log(`report: ${opts.out}`);
  }
  fs.rmSync(storage, { recursive: true, force: true });
}

if (require.main === module) {
  main().catch((e) => {
    console.error(e);
    process.exit(1);
  });
}

module.exports = { Client, intelephenseServer, uriOf, applyEdits };
