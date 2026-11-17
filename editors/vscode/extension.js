// Navigation for `.graven` schemas: go to definition, find references, hover, highlights,
// outline and workspace symbols. The index (graven-index.js) covers every `.graven` file in
// the workspace and follows unsaved edits.
"use strict";

const vscode = require("vscode");
const { Index } = require("./graven-index.js");

const SELECTOR = { language: "graven" };
const KIND = {
  table: vscode.SymbolKind.Class,
  struct: vscode.SymbolKind.Struct,
  enum: vscode.SymbolKind.Enum,
  union: vscode.SymbolKind.Interface,
  service: vscode.SymbolKind.Module,
};
const MEMBER_KIND = {
  table: vscode.SymbolKind.Field,
  struct: vscode.SymbolKind.Field,
  enum: vscode.SymbolKind.EnumMember,
  union: vscode.SymbolKind.EnumMember,
  service: vscode.SymbolKind.Method,
};

function activate(context) {
  const index = new Index();
  const key = (uri) => uri.toString();
  const uriOf = new Map(); // index key -> vscode.Uri

  const put = (uri, text) => {
    uriOf.set(key(uri), uri);
    index.update(key(uri), text);
  };
  const load = async (uri) => {
    const open = vscode.workspace.textDocuments.find((d) => key(d.uri) === key(uri));
    if (open) return put(uri, open.getText());
    try {
      put(uri, Buffer.from(await vscode.workspace.fs.readFile(uri)).toString("utf8"));
    } catch {
      index.remove(key(uri));
    }
  };

  const ready = vscode.workspace
    .findFiles("**/*.graven", "**/{node_modules,target,.git}/**")
    .then((uris) => Promise.all(uris.map(load)));

  const watcher = vscode.workspace.createFileSystemWatcher("**/*.graven");
  watcher.onDidCreate(load);
  watcher.onDidChange(load);
  watcher.onDidDelete((uri) => index.remove(key(uri)));
  context.subscriptions.push(
    watcher,
    vscode.workspace.onDidOpenTextDocument((d) => d.languageId === "graven" && put(d.uri, d.getText())),
    vscode.workspace.onDidChangeTextDocument((e) => e.document.languageId === "graven" && put(e.document.uri, e.document.getText())),
  );

  const loc = (uriKey, line, character, length) =>
    new vscode.Location(uriOf.get(uriKey) || vscode.Uri.parse(uriKey), new vscode.Range(line, character, line, character + length));
  const declLoc = (d) => loc(d.uri, d.line, d.character, d.name.length);
  const lookup = async (doc, pos) => {
    await ready;
    put(doc.uri, doc.getText());
    return index.at(key(doc.uri), pos.line, pos.character);
  };

  context.subscriptions.push(
    vscode.languages.registerDefinitionProvider(SELECTOR, {
      async provideDefinition(doc, pos) {
        const hit = await lookup(doc, pos);
        return hit ? declLoc(hit.decl) : undefined;
      },
    }),

    vscode.languages.registerReferenceProvider(SELECTOR, {
      async provideReferences(doc, pos, ctx) {
        const hit = await lookup(doc, pos);
        if (!hit) return [];
        return index.references(hit.decl, ctx.includeDeclaration).map((r) => loc(r.uri, r.line, r.character, r.length));
      },
    }),

    vscode.languages.registerDocumentHighlightProvider(SELECTOR, {
      async provideDocumentHighlights(doc, pos) {
        const hit = await lookup(doc, pos);
        if (!hit) return [];
        return index
          .references(hit.decl)
          .filter((r) => r.uri === key(doc.uri))
          .map((r) => new vscode.DocumentHighlight(new vscode.Range(r.line, r.character, r.line, r.character + r.length)));
      },
    }),

    vscode.languages.registerHoverProvider(SELECTOR, {
      async provideHover(doc, pos) {
        const hit = await lookup(doc, pos);
        if (!hit) return undefined;
        const d = hit.decl;
        const md = new vscode.MarkdownString();
        md.appendCodeblock(`${d.kind} ${d.namespace ? d.namespace + "." : ""}${d.name}`, "graven");
        if (d.doc) md.appendMarkdown(d.doc);
        const r = hit.ref;
        return new vscode.Hover(md, new vscode.Range(r.line, r.character, r.line, r.character + r.length));
      },
    }),

    vscode.languages.registerDocumentSymbolProvider(SELECTOR, {
      provideDocumentSymbols(doc) {
        put(doc.uri, doc.getText());
        const file = index.files.get(key(doc.uri));
        if (!file) return [];
        return file.decls.map((d) => {
          const full = new vscode.Range(d.line, 0, Math.max(d.endLine, d.line), Number.MAX_SAFE_INTEGER);
          const sel = new vscode.Range(d.line, d.character, d.line, d.character + d.name.length);
          const sym = new vscode.DocumentSymbol(d.name, d.kind, KIND[d.kind], doc.validateRange(full), sel);
          sym.children = d.members.map((m) => {
            const r = new vscode.Range(m.line, m.character, m.line, m.character + m.name.length);
            return new vscode.DocumentSymbol(m.name, "", MEMBER_KIND[d.kind], r, r);
          });
          return sym;
        });
      },
    }),

    vscode.languages.registerWorkspaceSymbolProvider({
      async provideWorkspaceSymbols(query) {
        await ready;
        const q = query.toLowerCase();
        return index
          .declarations()
          .filter((d) => d.name.toLowerCase().includes(q))
          .map((d) => new vscode.SymbolInformation(d.name, KIND[d.kind], d.namespace, declLoc(d)));
      },
    }),
  );
}

function deactivate() {}

module.exports = { activate, deactivate };
