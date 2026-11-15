// The `.d.ts` files must describe exactly what the `.js` files export: every exported value,
// every class member and every key of an exported object, in both directions. The runtime's
// declarations come from `tsc`; the generated ones come from `gravenc`, which writes each
// member into both files from one description, and this test holds it to that.

import { describe, it } from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import ts from "typescript";

const HERE = path.dirname(fileURLToPath(import.meta.url)); // ts/dist/tests
const GEN_JS = path.join(HERE, "gen");
const GEN_DTS = path.resolve(HERE, "../../tests/gen");

interface ClassShape {
  statics: Set<string>;
  /** Members on the prototype: methods and accessors. */
  methods: Set<string>;
  /** Every instance member the declaration names (fields included). */
  instance: Set<string>;
}

interface Declared {
  values: Set<string>;
  classes: Map<string, ClassShape>;
  objects: Map<string, Set<string>>;
}

function hasModifier(node: ts.Node, kind: ts.SyntaxKind): boolean {
  return ts.canHaveModifiers(node) && (ts.getModifiers(node) ?? []).some((m) => m.kind === kind);
}

function memberName(m: ts.ClassElement | ts.TypeElement): string | undefined {
  if (m.name === undefined) return undefined;
  if (ts.isIdentifier(m.name) || ts.isStringLiteral(m.name) || ts.isPrivateIdentifier(m.name)) return m.name.text;
  return undefined;
}

function readDeclarations(file: string): Declared {
  const src = ts.createSourceFile(file, fs.readFileSync(file, "utf8"), ts.ScriptTarget.ES2022, true);
  const out: Declared = { values: new Set(), classes: new Map(), objects: new Map() };
  for (const st of src.statements) {
    if (!hasModifier(st, ts.SyntaxKind.ExportKeyword)) continue;
    if (ts.isClassDeclaration(st) && st.name) {
      const shape: ClassShape = { statics: new Set(), methods: new Set(), instance: new Set() };
      for (const m of st.members) {
        if (ts.isConstructorDeclaration(m)) continue;
        const name = memberName(m);
        if (name === undefined) continue;
        if (hasModifier(m, ts.SyntaxKind.StaticKeyword)) {
          shape.statics.add(name);
        } else {
          shape.instance.add(name);
          if (ts.isMethodDeclaration(m) || ts.isGetAccessorDeclaration(m) || ts.isSetAccessorDeclaration(m)) {
            shape.methods.add(name);
          }
        }
      }
      out.values.add(st.name.text);
      out.classes.set(st.name.text, shape);
    } else if (ts.isFunctionDeclaration(st) && st.name) {
      out.values.add(st.name.text);
    } else if (ts.isVariableStatement(st)) {
      for (const d of st.declarationList.declarations) {
        if (!ts.isIdentifier(d.name)) continue;
        out.values.add(d.name.text);
        if (d.type && ts.isTypeLiteralNode(d.type)) {
          const keys = new Set<string>();
          for (const m of d.type.members) {
            const name = memberName(m);
            if (name !== undefined) keys.add(name);
          }
          out.objects.set(d.name.text, keys);
        }
      }
    }
  }
  return out;
}

const sorted = (s: Iterable<string>): string[] => [...s].sort();

/** Members of a class at run time: own static properties and own prototype members. */
function runtimeShape(cls: Function): { statics: string[]; methods: string[] } {
  const statics = Object.getOwnPropertyNames(cls).filter((n) => !["length", "name", "prototype"].includes(n));
  const methods = Object.getOwnPropertyNames(cls.prototype).filter((n) => n !== "constructor");
  return { statics: sorted(statics), methods: sorted(methods) };
}

describe(".d.ts declarations match the .js code", () => {
  const modules = fs
    .readdirSync(GEN_JS)
    .filter((f) => f.endsWith(".js"))
    .map((f) => f.slice(0, -3));

  it("covers the runtime and every generated module", () => {
    assert.ok(modules.includes("graven"));
    assert.ok(modules.length > 10);
  });

  for (const name of modules) {
    it(name, async () => {
      const mod = (await import(`./gen/${name}.js`)) as Record<string, unknown>;
      const decl = readDeclarations(path.join(GEN_DTS, `${name}.d.ts`));

      assert.deepEqual(sorted(Object.keys(mod)), sorted(decl.values), `${name}: exported values`);

      for (const [cname, shape] of decl.classes) {
        const rt = runtimeShape(mod[cname] as Function);
        assert.deepEqual(rt.statics, sorted(shape.statics), `${name}.${cname}: static members`);
        // Every declared method exists on the prototype ...
        for (const m of shape.methods) {
          assert.ok(rt.methods.includes(m), `${name}.${cname}: declared method \`${m}\` missing from the .js`);
        }
        // ... and every prototype member is declared.
        for (const m of rt.methods) {
          assert.ok(shape.instance.has(m), `${name}.${cname}: \`${m}\` exists in the .js but is not declared`);
        }
      }

      for (const [oname, keys] of decl.objects) {
        const value = mod[oname];
        assert.ok(value !== null && typeof value === "object", `${name}.${oname}: expected an object`);
        assert.deepEqual(sorted(Object.keys(value as object)), sorted(keys), `${name}.${oname}: keys`);
      }
    });
  }
});
