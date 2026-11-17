// Indexes `.graven` files: namespaces, imports, declarations and the places that refer to
// them. No VS Code API here, so it can be tested with plain Node.
//
// Name resolution follows the schema language: one namespace per file; a local name refers to
// a declaration of the file's own namespace; an imported name is qualified by the last segment
// of its namespace (`common.Money`) or by its alias (`import acme.common as ac;` -> `ac.Money`).
"use strict";

const KINDS = ["table", "struct", "enum", "union", "service"];
const KEYWORDS = new Set([
  ...KINDS, "namespace", "import", "as", "reserved", "stream", "Result", "true", "false",
  "u8", "u16", "u32", "u64", "u128", "i8", "i16", "i32", "i64", "i128", "f32", "f64",
  "bool", "string", "bytes",
]);

/** Replaces comments with spaces, keeping every offset (and line break) in place. */
function stripComments(text) {
  let out = "";
  let i = 0;
  while (i < text.length) {
    if (text.startsWith("//", i)) {
      const end = text.indexOf("\n", i);
      const stop = end < 0 ? text.length : end;
      out += " ".repeat(stop - i);
      i = stop;
    } else if (text.startsWith("/*", i)) {
      const end = text.indexOf("*/", i + 2);
      const stop = end < 0 ? text.length : end + 2;
      out += text.slice(i, stop).replace(/[^\n]/g, " ");
      i = stop;
    } else {
      out += text[i++];
    }
  }
  return out;
}

/** Blanks regions that never hold type references: attributes, enum bodies, namespace/import lines. */
function blankNonTypes(code) {
  const blank = (s) => s.replace(/[^\n]/g, " ");
  return code
    .replace(/#\[[^\]]*\]/g, blank)
    .replace(/\b(enum\s+\w+\s*(?::\s*\w+)?\s*)(\{[^}]*\})/g, (_, head, body) => head + blank(body))
    .replace(/\b(namespace|import)\b[^;]*;/g, blank);
}

function lineStarts(text) {
  const starts = [0];
  for (let i = 0; i < text.length; i++) if (text[i] === "\n") starts.push(i + 1);
  return starts;
}

function position(starts, offset) {
  let lo = 0;
  let hi = starts.length - 1;
  while (lo < hi) {
    const mid = (lo + hi + 1) >> 1;
    if (starts[mid] <= offset) lo = mid;
    else hi = mid - 1;
  }
  return { line: lo, character: offset - starts[lo] };
}

/** The `///` lines right above `offset` (attributes in between are skipped). */
function docAbove(lines, line) {
  const doc = [];
  for (let l = line - 1; l >= 0; l--) {
    const t = lines[l].trim();
    if (t.startsWith("///")) doc.unshift(t.slice(3).trim());
    else if (t.startsWith("#[")) continue;
    else break;
  }
  return doc.join("\n");
}

/** Parses one file: its namespace, imports, declarations (with members) and type references. */
function parse(uri, text) {
  const code = stripComments(text);
  const starts = lineStarts(text);
  const lines = text.split("\n");
  const ns = (code.match(/\bnamespace\s+([\w.]+)\s*;/) || [])[1] || "";
  const imports = [];
  for (const m of code.matchAll(/\bimport\s+([\w.]+)(?:\s+as\s+(\w+))?\s*;/g)) {
    imports.push({ namespace: m[1], alias: m[2] || m[1].split(".").pop() });
  }

  const decls = [];
  for (const m of code.matchAll(/\b(table|struct|enum|union|service)\s+([A-Za-z_]\w*)/g)) {
    const nameAt = m.index + m[0].length - m[2].length;
    const pos = position(starts, nameAt);
    // The body: from the first `{` after the name to its matching `}`.
    const open = code.indexOf("{", nameAt);
    let close = open;
    for (let depth = 0, i = open; open >= 0 && i < code.length; i++) {
      if (code[i] === "{") depth++;
      else if (code[i] === "}" && --depth === 0) { close = i; break; }
    }
    const members = [];
    if (open >= 0) {
      const body = blankNonTypes(code.slice(open + 1, close)).replace(/#\[[^\]]*\]/g, (s) => " ".repeat(s.length));
      const re = m[1] === "enum" ? /\b([A-Za-z_]\w*)\s*(=\s*[-\w]+)?/g : /\b([A-Za-z_]\w*)\s*(@\d+|:)/g;
      const src = m[1] === "enum" ? code.slice(open + 1, close) : body;
      for (const f of src.matchAll(re)) {
        if (KEYWORDS.has(f[1]) && m[1] !== "enum") continue;
        members.push({ name: f[1], ...position(starts, open + 1 + f.index) });
      }
    }
    decls.push({
      uri, namespace: ns, kind: m[1], name: m[2], line: pos.line, character: pos.character,
      endLine: position(starts, close).line, doc: docAbove(lines, pos.line), members,
    });
  }

  // Type references: identifiers, possibly qualified, outside comments, attributes, enum bodies,
  // namespace and import lines, field and member names (followed by `@` or `:`), and default
  // values (preceded by `=`).
  const refs = [];
  const scan = blankNonTypes(code);
  for (const m of scan.matchAll(/(?<![\w.])([A-Za-z_]\w*)(?:\.([A-Za-z_]\w*))?(?![\w.])/g)) {
    const [whole, a, b] = m;
    if (!b && KEYWORDS.has(a)) continue;
    const decl = /\b(table|struct|enum|union|service)\s+$/.test(scan.slice(Math.max(0, m.index - 12), m.index));
    const after = scan.slice(m.index + whole.length).match(/^\s*(\S)/);
    const before = scan.slice(0, m.index).match(/(\S)\s*$/);
    if (!decl && after && (after[1] === "@" || (after[1] === ":" && !b))) continue;
    if (before && before[1] === "=") continue;
    const nameOffset = b ? m.index + a.length + 1 : m.index;
    refs.push({
      qualifier: b ? a : undefined,
      name: b || a,
      ...position(starts, nameOffset),
      length: (b || a).length,
      isDeclaration: decl,
    });
  }
  return { uri, namespace: ns, imports, decls, refs };
}

class Index {
  constructor() {
    this.files = new Map(); // uri -> parsed file
  }

  update(uri, text) {
    this.files.set(uri, parse(uri, text));
  }

  remove(uri) {
    this.files.delete(uri);
  }

  /** Every declaration, across files. */
  declarations() {
    return [...this.files.values()].flatMap((f) => f.decls);
  }

  /** The declaration a reference in `file` names, or undefined. */
  resolve(file, ref) {
    let ns = file.namespace;
    if (ref.qualifier !== undefined) {
      const imp = file.imports.find((i) => i.alias === ref.qualifier);
      if (!imp) return undefined;
      ns = imp.namespace;
    }
    return this.declarations().find((d) => d.namespace === ns && d.name === ref.name);
  }

  /** The reference under (line, character) in `uri`, with the declaration it names. */
  at(uri, line, character) {
    const file = this.files.get(uri);
    if (!file) return undefined;
    const ref = file.refs.find((r) => r.line === line && character >= r.character && character <= r.character + r.length);
    if (!ref) return undefined;
    const decl = this.resolve(file, ref);
    return decl ? { ref, decl } : undefined;
  }

  /** Every reference to `decl`, in every indexed file. */
  references(decl, includeDeclaration = true) {
    const out = [];
    for (const file of this.files.values()) {
      for (const ref of file.refs) {
        if (ref.name !== decl.name) continue;
        if (!includeDeclaration && ref.isDeclaration) continue;
        if (this.resolve(file, ref) === decl) out.push({ uri: file.uri, ...ref });
      }
    }
    return out;
  }
}

module.exports = { Index, parse, stripComments };
