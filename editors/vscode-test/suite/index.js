// Checks run inside the VS Code extension host, against the Intelephense
// extension whose server was replaced by php-lsp (see ../run.js).
'use strict';

const assert = require('assert');
const vscode = require('vscode');

const EXTENSION = 'bmewburn.vscode-intelephense-client';

function file(rel) {
  return vscode.Uri.joinPath(vscode.workspace.workspaceFolders[0].uri, rel);
}

async function open(rel) {
  const doc = await vscode.workspace.openTextDocument(file(rel));
  await vscode.window.showTextDocument(doc);
  return doc;
}

/** Position of `needle` (plus `delta` characters) in `doc`. */
function pos(doc, needle, delta = 0) {
  const i = doc.getText().indexOf(needle);
  assert.ok(i >= 0, `${needle} not found`);
  return doc.positionAt(i + delta);
}

async function until(what, fn, timeout = 30000) {
  const end = Date.now() + timeout;
  for (;;) {
    const v = await fn();
    if (v) return v;
    if (Date.now() > end) throw new Error(`timed out waiting for ${what}`);
    await new Promise((r) => setTimeout(r, 200));
  }
}

const exec = (cmd, ...args) => vscode.commands.executeCommand(cmd, ...args);
const labels = (list) => list.items.map((i) => (typeof i.label === 'string' ? i.label : i.label.label));
const hoverText = (hovers) =>
  hovers.flatMap((h) => h.contents.map((c) => (typeof c === 'string' ? c : c.value))).join('\n');

const checks = [
  ['diagnostics', async () => {
    const doc = await open('src/Service.php');
    // undefined symbols are reported once indexing has finished
    await until('undefined function diagnostic', () =>
      vscode.languages.getDiagnostics(doc.uri).some((d) => d.message.includes('undefined_function')),
    );
  }],
  ['hover', async () => {
    const doc = await open('src/Service.php');
    const h = await exec('vscode.executeHoverProvider', doc.uri, pos(doc, 'getName', 2));
    assert.match(hoverText(h), /Returns the display name/);
  }],
  ['definition', async () => {
    const doc = await open('src/Service.php');
    const locs = await exec('vscode.executeDefinitionProvider', doc.uri, pos(doc, 'User $user', 1));
    const uri = (locs[0].targetUri || locs[0].uri).toString();
    assert.ok(uri.endsWith('src/Models/User.php'), uri);
  }],
  ['member completion', async () => {
    const doc = await open('src/Service.php');
    const list = await exec('vscode.executeCompletionItemProvider', doc.uri, pos(doc, '$user->getName', 7));
    assert.ok(labels(list).includes('getName'), labels(list).join(','));
  }],
  ['html completion', async () => {
    const doc = await open('views/page.php');
    const list = await exec('vscode.executeCompletionItemProvider', doc.uri, pos(doc, '<div', 3));
    assert.ok(labels(list).includes('div'), labels(list).slice(0, 20).join(','));
  }],
  ['blade template', async () => {
    const doc = await open('resources/views/users/show.blade.php');
    const h = await exec('vscode.executeHoverProvider', doc.uri, pos(doc, 'getName', 2));
    assert.match(hoverText(h), /Returns the display name/);
    const list = await exec('vscode.executeCompletionItemProvider', doc.uri, pos(doc, '@foreach', 4));
    assert.ok(labels(list).includes('@foreach'), labels(list).slice(0, 20).join(','));
    const loop = await exec('vscode.executeCompletionItemProvider', doc.uri, pos(doc, '$loop->', 7));
    assert.ok(labels(loop).includes('index'), labels(loop).slice(0, 20).join(','));
    const syms = await exec('vscode.executeDocumentSymbolProvider', doc.uri);
    assert.deepStrictEqual(syms.map((s) => s.name), ['content']);
    const folds = await exec('vscode.executeFoldingRangeProvider', doc.uri);
    assert.ok(folds.length >= 2, JSON.stringify(folds));
  }],
  ['references', async () => {
    const doc = await open('src/Models/User.php');
    const refs = await exec('vscode.executeReferenceProvider', doc.uri, pos(doc, 'class User', 7));
    assert.ok(refs.some((r) => r.uri.toString().endsWith('src/Service.php')), JSON.stringify(refs));
  }],
  ['code lens', async () => {
    const doc = await open('src/Models/User.php');
    const lenses = await exec('vscode.executeCodeLensProvider', doc.uri, 10);
    const refs = lenses.find((l) => l.command && /reference/.test(l.command.title));
    assert.ok(refs, JSON.stringify(lenses.map((l) => l.command && l.command.title)));
    assert.strictEqual(refs.command.command, 'editor.action.peekLocations');
    assert.ok(refs.command.arguments[0] instanceof vscode.Uri);
    await exec(refs.command.command, ...refs.command.arguments);
  }],
  ['document links', async () => {
    const doc = await open('src/Service.php');
    const links = await exec('vscode.executeLinkProvider', doc.uri);
    assert.ok(links.some((l) => l.target && l.target.toString().endsWith('lib/helpers.php')), JSON.stringify(links));
  }],
  ['formatting', async () => {
    const doc = await open('src/Unformatted.php');
    const edits = await exec('vscode.executeFormatDocumentProvider', doc.uri, { tabSize: 4, insertSpaces: true });
    assert.ok(edits && edits.length > 0, 'no edits');
    const we = new vscode.WorkspaceEdit();
    we.set(doc.uri, edits);
    await vscode.workspace.applyEdit(we);
    assert.match(doc.getText(), /\n    public function f\(\)\n    \{\n        return 1;/);
  }],
  ['rename', async () => {
    const doc = await open('src/Models/User.php');
    const edit = await exec('vscode.executeDocumentRenameProvider', doc.uri, pos(doc, 'getName', 2), 'displayName');
    const files = edit.entries().map(([u]) => u.toString());
    assert.ok(files.some((f) => f.endsWith('src/Service.php')) && files.some((f) => f.endsWith('User.php')), files.join(','));
  }],
  ['signature help', async () => {
    const doc = await open('src/Service.php');
    const help = await exec('vscode.executeSignatureHelpProvider', doc.uri, pos(doc, 'greet(', 6));
    assert.match(help.signatures[0].label, /string \$name/);
  }],
  ['workspace symbols', async () => {
    const syms = await exec('vscode.executeWorkspaceSymbolProvider', 'Servi');
    assert.ok(syms.some((s) => s.name === 'Service'), JSON.stringify(syms.map((s) => s.name)));
  }],
  ['index workspace command', async () => {
    await exec('intelephense.index.workspace');
    const doc = await open('src/Service.php');
    await until('hover after restart', async () => {
      try {
        const h = await exec('vscode.executeHoverProvider', doc.uri, pos(doc, 'getName', 2));
        return /Returns the display name/.test(hoverText(h));
      } catch (_) {
        return false;
      }
    });
  }],
];

exports.run = async function () {
  const ext = vscode.extensions.getExtension(EXTENSION);
  assert.ok(ext, `${EXTENSION} is not installed`);
  await open('src/Service.php');
  await ext.activate();
  const failures = [];
  for (const [name, check] of checks) {
    try {
      await check();
      console.log(`  ok    ${name}`);
    } catch (err) {
      console.log(`  FAIL  ${name}: ${err.message}`);
      failures.push(name);
    }
  }
  console.log(`${checks.length - failures.length}/${checks.length} checks passed`);
  if (failures.length) throw new Error(`failed: ${failures.join(', ')}`);
};
