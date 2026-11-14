//! Semantic check: AST → resolved [`Ir`] with layout (spec §3–§6).
//!
//! All errors are collected (it does not stop at the first one). Rules:
//! - type names are unique and differ from the built-ins; field / variant names are unique;
//! - types resolve; no `[[T]]` and no `[union]`;
//! - enum: base `u8`/`u16`/`u32`, values fit the base, unique names and values;
//! - struct: only integers, floats, `bool` (1 byte), enums and structs; no struct cycles
//!   (recursion only through a table); size ≤ 65535;
//! - union: variants are tables only, tags `> 0` and unique;
//! - table: contiguous IDs `0..n` without repeats; defaults only on scalar/enum/bool, of the
//!   right type and not combined with `?`; reserved fields have a type (from the schema or the
//!   lock, equal if both exist); the only attribute is `#[allow_breaking_readers]`, on a reserved or formerly required field; fixed part
//!   ≤ 65535 bytes;
//! - names that would break or collide in the generated Rust (see [`crate::names`]): types start
//!   with an uppercase letter; no `self`/`Self`/`super`/`crate`/`_`; no `Unknown` variants; no
//!   generated identifier repeated in the same scope.

use std::collections::btree_map::Entry;
use std::collections::{BTreeMap, HashMap, HashSet};

use crate::ast::{self, Item, Literal, Span, TypeRef};
use crate::ir::*;
use crate::parse::parse_type;
use graven_layout::{FieldKind, TableLayout, compute_layout};

const ALLOW_BREAKING: &str = "allow_breaking_readers";
/// Prefix of the synthetic name of a reserved field (`_reserved_<id>`); forbidden in user names.
pub const RESERVED_PREFIX: &str = "_reserved_";

/// Checks `s` and produces the IR. `reserved_types(table, id)` gives the original type of a
/// reserved field according to the lock, as canonical text ([`Ir::ty_name`]: `"u32"`, `"[Leaf]"`,
/// `"Color"`), resolved here by name against `s` (`None` if there is no lock or the lock does not
/// know the field).
pub fn check(s: &ast::Schema, reserved_types: &dyn Fn(&str, u16) -> Option<String>) -> Result<Ir, Vec<Diag>> {
    check_with_imports(s, reserved_types, &HashMap::new())
}

/// [`check`] for a schema that imports other namespaces: `imports` maps each namespace
/// compiled together with `s` to its checked IR. The types `s` uses from them (and the
/// types those depend on) are copied into the result, marked with their `origin` and named
/// `namespace.Type`.
pub fn check_with_imports(
    s: &ast::Schema,
    reserved_types: &dyn Fn(&str, u16) -> Option<String>,
    imports: &HashMap<String, Ir>,
) -> Result<Ir, Vec<Diag>> {
    let mut cx = Cx { diags: Vec::new(), names: HashMap::new() };
    let mut ir = Ir {
        namespace: s.namespace.0.clone(),
        enums: Vec::new(),
        structs: Vec::new(),
        tables: Vec::new(),
        unions: Vec::new(),
        docs: s.docs.clone(),
    };

    // 1. Declarations (ids in declaration order within each item kind).
    for item in &s.items {
        let (name, span, ty) = match item {
            Item::Enum(e) => {
                ir.enums.push(Enum { name: e.name.clone(), base: Prim::U32, values: Vec::new(), span: e.span, origin: None });
                (&e.name, e.span, Ty::Enum(EnumId(ir.enums.len() - 1)))
            }
            Item::Struct(d) => {
                ir.structs.push(Struct { name: d.name.clone(), fields: Vec::new(), size: 0, span: d.span, origin: None });
                (&d.name, d.span, Ty::Struct(StructId(ir.structs.len() - 1)))
            }
            Item::Table(t) => {
                ir.tables.push(Table {
                    name: t.name.clone(),
                    fields: Vec::new(),
                    layout: TableLayout { fixed_len: 0, fields: Vec::new() },
                    all_scalar: true,
                    ptr_slots: Vec::new(),
                    checks: Vec::new(),
                    allow_breaking: false,
                    span: t.span,
                    origin: None,
                });
                (&t.name, t.span, Ty::Table(TableId(ir.tables.len() - 1)))
            }
            Item::Union(u) => {
                ir.unions.push(Union { name: u.name.clone(), variants: Vec::new(), span: u.span, origin: None });
                (&u.name, u.span, Ty::Union(UnionId(ir.unions.len() - 1)))
            }
        };
        if builtin(name).is_some() {
            cx.err(span, format!("`{name}` is a built-in type and cannot be redeclared"));
        } else if cx.names.contains_key(name) {
            cx.err(span, format!("`{name}` declared more than once"));
        } else {
            cx.names.insert(name.clone(), ty);
        }
    }

    // 1b. Imports: the types used from other namespaces, copied after the local ones.
    import_types(&mut cx, &mut ir, s, imports);

    // 2. Enums, structs (fields), unions.
    let (mut ei, mut si, mut ui) = (0, 0, 0);
    for item in &s.items {
        match item {
            Item::Enum(e) => {
                cx.enum_decl(&mut ir.enums[ei], e);
                ei += 1;
            }
            Item::Struct(d) => {
                let fields = cx.struct_fields(&ir, d);
                ir.structs[si].fields = fields;
                si += 1;
            }
            Item::Union(u) => {
                let variants = cx.union_variants(&ir, u);
                ir.unions[ui].variants = variants;
                ui += 1;
            }
            Item::Table(_) => {}
        }
    }

    // 3. Struct sizes (detects cycles).
    struct_sizes(&mut cx, &mut ir);

    // 4. Tables.
    let mut ti = 0;
    for item in &s.items {
        if let Item::Table(t) = item {
            let table = cx.table(&ir, t, reserved_types);
            ir.tables[ti] = table;
            ti += 1;
        }
    }
    cx.check_named_sigs(&ir);

    // 5. Names that would break or collide in the generated Rust.
    let names = &cx.names;
    crate::names::check_names(s, &|n: &str| builtin(n).or_else(|| names.get(n).cloned()), &mut cx.diags);

    if cx.diags.is_empty() { Ok(ir) } else { Err(cx.diags) }
}

struct Cx {
    diags: Vec<Diag>,
    names: HashMap<String, Ty>,
}

/// Every `import_name.Type` reference in the schema, with its position.
fn qualified_refs(s: &ast::Schema) -> Vec<(String, Span)> {
    fn walk(t: &TypeRef, sp: Span, out: &mut Vec<(String, Span)>) {
        match t {
            TypeRef::Named(n) if n.contains('.') => out.push((n.clone(), sp)),
            TypeRef::Named(_) => {}
            TypeRef::List(i) => walk(i, sp, out),
        }
    }
    let mut out = Vec::new();
    for item in &s.items {
        match item {
            Item::Table(t) => {
                for f in &t.fields {
                    if let Some(ty) = &f.ty {
                        walk(ty, f.span, &mut out);
                    }
                }
            }
            Item::Struct(d) => d.fields.iter().for_each(|(_, ty, sp)| walk(ty, *sp, &mut out)),
            Item::Union(u) => u.variants.iter().for_each(|(_, _, ty, sp)| walk(ty, *sp, &mut out)),
            Item::Enum(_) => {}
        }
    }
    out
}

fn import_types(cx: &mut Cx, ir: &mut Ir, s: &ast::Schema, imports: &HashMap<String, Ir>) {
    let mut by_name: HashMap<&str, &Ir> = HashMap::new();
    for im in &s.imports {
        if im.namespace == s.namespace.0 {
            cx.err(im.span, format!("a schema cannot import its own namespace `{}`", im.namespace));
            continue;
        }
        let Some(src) = imports.get(&im.namespace) else {
            cx.err(
                im.span,
                format!(
                    "namespace `{}` is not among the schemas compiled together; add the file that declares it",
                    im.namespace
                ),
            );
            continue;
        };
        if cx.names.contains_key(&im.name) || builtin(&im.name).is_some() {
            cx.err(im.span, format!("import name `{}` is also the name of a type; use `import {} as <name>;`", im.name, im.namespace));
        } else if by_name.insert(im.name.as_str(), src).is_some() {
            cx.err(
                im.span,
                format!("two imports are named `{}`; give one of them another name: `import {} as <name>;`", im.name, im.namespace),
            );
        }
    }
    let mut memo: HashMap<String, Ty> = HashMap::new();
    for (n, sp) in qualified_refs(s) {
        if cx.names.contains_key(&n) {
            continue;
        }
        let (import_name, tname) = n.split_once('.').expect("qualified");
        let Some(src) = by_name.get(import_name).copied() else {
            cx.err(sp, format!("`{n}`: no import is named `{import_name}`"));
            continue;
        };
        let is = |name: &String, origin: &Option<String>| origin.is_none() && name == tname;
        let src_ty = if let Some(i) = src.tables.iter().position(|t| is(&t.name, &t.origin)) {
            Ty::Table(TableId(i))
        } else if let Some(i) = src.structs.iter().position(|t| is(&t.name, &t.origin)) {
            Ty::Struct(StructId(i))
        } else if let Some(i) = src.enums.iter().position(|t| is(&t.name, &t.origin)) {
            Ty::Enum(EnumId(i))
        } else if let Some(i) = src.unions.iter().position(|t| is(&t.name, &t.origin)) {
            Ty::Union(UnionId(i))
        } else {
            cx.err(sp, format!("namespace `{}` has no type `{tname}`", src.namespace));
            continue;
        };
        let ty = copy_type(ir, src, &src_ty, &mut memo);
        cx.names.insert(n, ty);
    }
}

/// Canonical name and origin of a type of `src`: local ones become `namespace.Type`.
fn canonical(src: &Ir, name: &str, origin: &Option<String>) -> (String, String) {
    match origin {
        Some(o) => (name.to_string(), o.clone()),
        None => (format!("{}.{name}", src.namespace), src.namespace.clone()),
    }
}

/// Copies `ty` of `src` (and every type it refers to) into `ir`, once per canonical name.
fn copy_type(ir: &mut Ir, src: &Ir, ty: &Ty, memo: &mut HashMap<String, Ty>) -> Ty {
    match ty {
        Ty::Scalar(_) | Ty::Bool | Ty::Str | Ty::Bytes => ty.clone(),
        Ty::List(e) => Ty::List(Box::new(copy_type(ir, src, e, memo))),
        Ty::Enum(id) => {
            let e = &src.enums[id.0];
            let (name, origin) = canonical(src, &e.name, &e.origin);
            if let Some(t) = memo.get(&name) {
                return t.clone();
            }
            ir.enums.push(Enum { name: name.clone(), base: e.base, values: e.values.clone(), span: Span::default(), origin: Some(origin) });
            let t = Ty::Enum(EnumId(ir.enums.len() - 1));
            memo.insert(name, t.clone());
            t
        }
        Ty::Struct(id) => {
            let st = &src.structs[id.0];
            let (name, origin) = canonical(src, &st.name, &st.origin);
            if let Some(t) = memo.get(&name) {
                return t.clone();
            }
            let fields = st.fields.iter().map(|(n, ft)| (n.clone(), copy_type(ir, src, ft, memo))).collect();
            ir.structs.push(Struct { name: name.clone(), fields, size: st.size, span: Span::default(), origin: Some(origin) });
            let t = Ty::Struct(StructId(ir.structs.len() - 1));
            memo.insert(name, t.clone());
            t
        }
        Ty::Table(id) => {
            let tb = &src.tables[id.0];
            let (name, origin) = canonical(src, &tb.name, &tb.origin);
            if let Some(t) = memo.get(&name) {
                return t.clone();
            }
            // Pushed before its fields are copied: tables may refer to themselves.
            ir.tables.push(Table {
                name: name.clone(),
                fields: Vec::new(),
                layout: tb.layout.clone(),
                all_scalar: tb.all_scalar,
                ptr_slots: tb.ptr_slots.clone(),
                checks: tb.checks.clone(),
                allow_breaking: tb.allow_breaking,
                span: Span::default(),
                origin: Some(origin),
            });
            let idx = ir.tables.len() - 1;
            let t = Ty::Table(TableId(idx));
            memo.insert(name, t.clone());
            let fields = tb
                .fields
                .iter()
                .map(|f| Field { ty: copy_type(ir, src, &f.ty, memo), span: Span::default(), ..f.clone() })
                .collect();
            ir.tables[idx].fields = fields;
            t
        }
        Ty::Union(id) => {
            let u = &src.unions[id.0];
            let (name, origin) = canonical(src, &u.name, &u.origin);
            if let Some(t) = memo.get(&name) {
                return t.clone();
            }
            ir.unions.push(Union { name: name.clone(), variants: Vec::new(), span: Span::default(), origin: Some(origin) });
            let idx = ir.unions.len() - 1;
            let t = Ty::Union(UnionId(idx));
            memo.insert(name, t.clone());
            let variants = u
                .variants
                .iter()
                .map(|(vn, tag, tid)| {
                    let Ty::Table(new) = copy_type(ir, src, &Ty::Table(*tid), memo) else { unreachable!() };
                    (vn.clone(), *tag, new)
                })
                .collect();
            ir.unions[idx].variants = variants;
            t
        }
    }
}

fn builtin(name: &str) -> Option<Ty> {
    if let Some(p) = Prim::from_name(name) {
        return Some(Ty::Scalar(p));
    }
    match name {
        "bool" => Some(Ty::Bool),
        "string" => Some(Ty::Str),
        "bytes" => Some(Ty::Bytes),
        _ => None,
    }
}

fn typeref_str(t: &TypeRef) -> String {
    match t {
        TypeRef::Named(n) => n.clone(),
        TypeRef::List(i) => format!("[{}]", typeref_str(i)),
    }
}

fn literal_str(l: &Literal) -> String {
    match l {
        Literal::Int(n) => n.to_string(),
        Literal::UInt(n) => n.to_string(),
        Literal::Float(x) => format!("{x:?}"),
        Literal::Bool(b) => b.to_string(),
        Literal::Ident(s) => s.clone(),
    }
}

impl Cx {
    fn err(&mut self, span: Span, msg: impl Into<String>) {
        self.diags.push(Diag { span, msg: msg.into() });
    }

    /// Resolves a type; `None` (with a diagnostic) if unknown or forbidden.
    fn resolve(&mut self, t: &TypeRef, span: Span) -> Option<Ty> {
        match t {
            TypeRef::Named(n) => match builtin(n).or_else(|| self.names.get(n).cloned()) {
                Some(ty) => Some(ty),
                None => {
                    self.err(span, format!("unknown type `{n}`"));
                    None
                }
            },
            TypeRef::List(inner) => {
                if let TypeRef::List(_) = **inner {
                    self.err(span, format!("list of lists (`{}`) is not supported", typeref_str(t)));
                    return None;
                }
                let ty = self.resolve(inner, span)?;
                if let Ty::Union(_) = ty {
                    self.err(span, format!("list of unions (`{}`) is not supported", typeref_str(t)));
                    return None;
                }
                Some(Ty::List(Box::new(ty)))
            }
        }
    }

    fn enum_decl(&mut self, out: &mut Enum, e: &ast::EnumDecl) {
        let base = match &e.base {
            TypeRef::Named(n) if matches!(n.as_str(), "u8" | "u16" | "u32") => Prim::from_name(n),
            other => {
                self.err(
                    e.span,
                    format!("base type of enum `{}` must be u8, u16 or u32 (it is `{}`)", e.name, typeref_str(other)),
                );
                None
            }
        };
        let max = base.map_or(u32::MAX as i128, |b| b.int_range().unwrap().1);
        let mut names = HashSet::new();
        let mut values: HashMap<i128, &str> = HashMap::new();
        for (vn, v, sp) in &e.values {
            if !names.insert(vn.as_str()) {
                self.err(*sp, format!("duplicate variant `{vn}` in enum `{}`", e.name));
                continue;
            }
            if *v < 0 || *v > max {
                let b = base.map_or("u32", Prim::name);
                self.err(*sp, format!("value {v} of variant `{vn}` of enum `{}` does not fit in {b}", e.name));
                continue;
            }
            if let Some(prev) = values.insert(*v, vn) {
                self.err(*sp, format!("value {v} repeated in enum `{}` (variants `{prev}` and `{vn}`)", e.name));
                continue;
            }
            out.values.push((vn.clone(), *v as u32));
        }
        out.base = base.unwrap_or(Prim::U32);
    }

    fn struct_fields(&mut self, ir: &Ir, d: &ast::StructDecl) -> Vec<(String, Ty)> {
        if let Some(a) = d.attrs.first() {
            self.err(a.span, format!("attributes are not allowed on struct `{}`", d.name));
        }
        for fattrs in &d.field_attrs {
            if let Some(a) = fattrs.first() {
                self.err(a.span, format!("attributes are not allowed on fields of struct `{}`", d.name));
            }
        }
        let mut out = Vec::new();
        let mut names = HashSet::new();
        for (fname, tr, sp) in &d.fields {
            if !names.insert(fname.as_str()) {
                self.err(*sp, format!("duplicate field `{fname}` in struct `{}`", d.name));
            }
            let Some(ty) = self.resolve(tr, *sp) else { continue };
            match ty {
                Ty::Scalar(_) | Ty::Bool | Ty::Enum(_) | Ty::Struct(_) => out.push((fname.clone(), ty)),
                _ => self.err(
                    *sp,
                    format!(
                        "a struct cannot contain `{}` (field `{fname}` of `{}`): only integers, floats, bool, enums and structs",
                        ir.ty_name(&ty),
                        d.name
                    ),
                ),
            }
        }
        out
    }

    fn union_variants(&mut self, ir: &Ir, u: &ast::UnionDecl) -> Vec<(String, u16, TableId)> {
        let mut out = Vec::new();
        let mut names = HashSet::new();
        let mut tags = HashSet::new();
        if u.variants.is_empty() {
            // The generated type (`enum U<'a> { Unknown(u16) }`) would not use `'a` and would not compile.
            self.err(u.span, format!("union `{}` has no variants: declare at least one", u.name));
        }
        for (vn, tag, tr, sp) in &u.variants {
            if !names.insert(vn.as_str()) {
                self.err(*sp, format!("duplicate variant `{vn}` in union `{}`", u.name));
            }
            if *tag == 0 {
                self.err(*sp, format!("variant `{vn}` of union `{}`: tag must be > 0", u.name));
            } else if !tags.insert(*tag) {
                self.err(*sp, format!("duplicate tag {tag} in union `{}`", u.name));
            }
            match self.resolve(tr, *sp) {
                Some(Ty::Table(id)) => out.push((vn.clone(), *tag, id)),
                Some(ty) => self.err(
                    *sp,
                    format!("variant `{vn}` of union `{}` is not a table (it is `{}`)", u.name, ir.ty_name(&ty)),
                ),
                None => {}
            }
        }
        out
    }

    fn table(&mut self, ir: &Ir, t: &ast::TableDecl, reserved_types: &dyn Fn(&str, u16) -> Option<String>) -> Table {
        // IDs: unique and contiguous from 0.
        let mut by_id: BTreeMap<u16, &ast::FieldDecl> = BTreeMap::new();
        for f in &t.fields {
            match by_id.entry(f.id) {
                Entry::Vacant(v) => {
                    v.insert(f);
                }
                Entry::Occupied(_) => self.err(f.span, format!("duplicate id @{} in table `{}`", f.id, t.name)),
            }
        }
        let mut expect: u32 = 0;
        for &id in by_id.keys() {
            let id = id as u32;
            if id > expect {
                let hint = format!("ids must be contiguous from @0; a removed field becomes `@{expect}: reserved <type>;`");
                let msg = if id == expect + 1 {
                    format!("table `{}`: field @{expect} is missing ({hint})", t.name)
                } else {
                    format!("table `{}`: fields @{expect}..@{} are missing ({hint})", t.name, id - 1)
                };
                self.err(t.span, msg);
            }
            expect = id + 1;
        }

        let mut names = HashSet::new();
        let mut fields = Vec::with_capacity(by_id.len());
        for f in by_id.values() {
            if let Some(n) = &f.name
                && !names.insert(n.as_str())
            {
                self.err(f.span, format!("duplicate field `{n}` in table `{}`", t.name));
            }
            fields.push(self.field(ir, &t.name, f, reserved_types));
        }

        // Layout.
        let kinds: Vec<FieldKind> = fields.iter().map(|f| ir.field_kind(&f.ty)).collect();
        let bound: u64 = kinds
            .iter()
            .map(|k| match k {
                FieldKind::Scalar { size } => *size as u64 + 1,
                FieldKind::Bool => 1,
                FieldKind::Ptr => 4,
                FieldKind::Union => 6,
            })
            .sum();
        let layout = if bound > u16::MAX as u64 {
                self.err(t.span, format!("fixed part of table `{}` exceeds 65535 bytes", t.name));
            TableLayout { fixed_len: 0, fields: Vec::new() }
        } else {
            compute_layout(&kinds)
        };
        let mut ptr_slots = Vec::new();
        for (k, fl) in kinds.iter().zip(&layout.fields) {
            match k {
                FieldKind::Ptr => ptr_slots.push(fl.slot.unwrap()),
                FieldKind::Union => ptr_slots.push(fl.slot.unwrap() + 2),
                _ => {}
            }
        }
        let (checks, allow_breaking) = self.table_attrs(t);
        let all_scalar = !kinds.iter().any(|k| matches!(k, FieldKind::Ptr | FieldKind::Union));
        Table { name: t.name.clone(), fields, layout, all_scalar, ptr_slots, checks, allow_breaking, span: t.span, origin: None }
    }

    fn table_attrs(&mut self, t: &ast::TableDecl) -> (Vec<String>, bool) {
        let mut checks = Vec::new();
        let mut seen_checks = HashSet::new();
        let mut allow_breaking = false;
        for a in &t.attrs {
            match a.name.as_str() {
                ALLOW_BREAKING => {
                    if !a.args.is_empty() {
                        self.err(a.span, format!("`#[{ALLOW_BREAKING}]` does not take arguments"));
                    }
                    allow_breaking = true;
                }
                "check" => {
                    if let Some(name) = self.parse_check_arg(a)
                        && !seen_checks.insert(name.clone())
                    {
                        self.err(a.span, format!("duplicate `check({name})` on table `{}`", t.name));
                    } else if let Some(name) = self.parse_check_arg_quiet(a) {
                        checks.push(name);
                    }
                }
                other => {
                    self.err(a.span, format!("attribute `#[{other}]` is not valid on a table"));
                }
            }
        }
        (checks, allow_breaking)
    }

    fn parse_check_arg(&mut self, a: &ast::Attr) -> Option<String> {
        if a.args.len() != 1 || a.args[0].value.is_some() {
            self.err(a.span, "`check` expects one function name: `#[check(name)]`".to_string());
            return None;
        }
        Some(a.args[0].name.clone())
    }

    fn parse_check_arg_quiet(&self, a: &ast::Attr) -> Option<String> {
        if a.args.len() == 1 && a.args[0].value.is_none() {
            Some(a.args[0].name.clone())
        } else {
            None
        }
    }

    fn field(
        &mut self,
        ir: &Ir,
        table: &str,
        f: &ast::FieldDecl,
        reserved_types: &dyn Fn(&str, u16) -> Option<String>,
    ) -> Field {
        let reserved = f.name.is_none();
        // Placeholder type for when the type does not resolve (the IR is discarded if there are errors).
        let placeholder = Ty::Scalar(Prim::U8);

        if reserved {
            let mut allow_breaking = false;
            for a in &f.attrs {
                if a.name != ALLOW_BREAKING {
                    self.err(a.span, format!("attribute `#[{}]` is not valid on a reserved field", a.name));
                } else {
                    if !a.args.is_empty() {
                        self.err(a.span, format!("`#[{ALLOW_BREAKING}]` does not take arguments"));
                    }
                    allow_breaking = true;
                }
            }
            let name = format!("{RESERVED_PREFIX}{}", f.id);
            let presence = Presence::Reserved { allow_breaking };
            if f.optional || f.default.is_some() {
                self.err(f.span, format!("reserved field @{} of `{table}` cannot be optional or have a default", f.id));
            }
            let from_schema = f.ty.as_ref().map(|tr| self.resolve(tr, f.span));
            let from_lock = reserved_types(table, f.id).map(|text| self.resolve_lock(&text, table, f.id, f.span));
            let ty = match (from_schema, from_lock) {
                (Some(Some(st)), Some(Some(lt))) => {
                    if st != lt {
                        self.err(
                            f.span,
                            format!(
                                "type of reserved field @{} of `{table}` (`{}`) differs from the lock (`{}`)",
                                f.id,
                                ir.ty_name(&st),
                                ir.ty_name(&lt)
                            ),
                        );
                    }
                    st
                }
                (Some(Some(t)), _) | (None, Some(Some(t))) => t,
                (Some(None), _) | (None, Some(None)) => placeholder, // already reported
                (None, None) => {
                    self.err(
                        f.span,
                        format!(
                            "reserved field @{id} of `{table}` has no type: write `@{id}: reserved <type>;` or generate the lock with `gravenc update`",
                            id = f.id
                        ),
                    );
                    placeholder
                }
            };
            return Field { id: f.id, name, ty, presence, allow_breaking, rules: FieldRules::default(), span: f.span };
        }

        let name = f.name.clone().unwrap();
        if name.starts_with(RESERVED_PREFIX) {
            self.err(f.span, format!("invalid field name `{name}`: the prefix `{RESERVED_PREFIX}` belongs to reserved fields"));
        }
        let ty = match &f.ty {
            Some(tr) => self.resolve(tr, f.span),
            None => {
                self.err(f.span, format!("field `{name}` has no type"));
                None
            }
        };
        let presence = match (&f.default, f.optional) {
            (Some(_), true) => {
                self.err(f.span, format!("field `{name}` cannot be optional and have a default"));
                Presence::Optional
            }
            (Some(lit), false) => match &ty {
                Some(ty) => match default_value(ir, ty, lit) {
                    Ok(v) => Presence::Default(v),
                    Err(m) => {
                        self.err(f.span, format!("field `{name}`: {m}"));
                        Presence::Required
                    }
                },
                None => Presence::Required,
            },
            (None, true) => Presence::Optional,
            (None, false) => Presence::Required,
        };
        let resolved_ty = ty.unwrap_or(placeholder);
        let (rules, allow_breaking) = self.field_attrs(ir, &name, f, &resolved_ty, &presence);
        Field { id: f.id, name, ty: resolved_ty, presence, allow_breaking, rules, span: f.span }
    }

    fn field_attrs(
        &mut self,
        ir: &Ir,
        field_name: &str,
        f: &ast::FieldDecl,
        ty: &Ty,
        presence: &Presence,
    ) -> (FieldRules, bool) {
        let mut rules = FieldRules::default();
        let mut allow_breaking_attr = None;
        let mut seen_checks = HashSet::new();

        for a in &f.attrs {
            match a.name.as_str() {
                ALLOW_BREAKING => {
                    if !a.args.is_empty() {
                        self.err(a.span, format!("`#[{ALLOW_BREAKING}]` does not take arguments"));
                    }
                    allow_breaking_attr = Some(a.span);
                }
                "range" => {
                    if rules.range.is_some() {
                        self.err(a.span, format!("duplicate `range` attribute on field `{field_name}`"));
                        continue;
                    }
                    match ty {
                        Ty::Scalar(p) => {
                            if let Some(r) = self.parse_range(*p, a) {
                                rules.range = Some(r);
                            }
                        }
                        _ => {
                            self.err(
                                a.span,
                                format!("`range` is only valid on numeric fields, not `{}`", ir.ty_name(ty)),
                            );
                        }
                    }
                }
                "len" => {
                    if rules.len.is_some() {
                        self.err(a.span, format!("duplicate `len` attribute on field `{field_name}`"));
                        continue;
                    }
                    match ty {
                        Ty::Str | Ty::Bytes | Ty::List(_) => {
                            if let Some(l) = self.parse_len(a) {
                                rules.len = Some(l);
                            }
                        }
                        _ => {
                            self.err(
                                a.span,
                                format!("`len` is only valid on `string`, `bytes` or list fields, not `{}`", ir.ty_name(ty)),
                            );
                        }
                    }
                }
                "finite" => {
                    if !a.args.is_empty() {
                        self.err(a.span, "`finite` does not take arguments".to_string());
                    }
                    if rules.finite {
                        self.err(a.span, format!("duplicate `finite` attribute on field `{field_name}`"));
                        continue;
                    }
                    match ty {
                        Ty::Scalar(p) if p.is_float() => rules.finite = true,
                        _ => self.err(a.span, format!("`finite` is only valid on `f32` or `f64`, not `{}`", ir.ty_name(ty))),
                    }
                }
                "known" => {
                    if !a.args.is_empty() {
                        self.err(a.span, "`known` does not take arguments".to_string());
                    }
                    if rules.known {
                        self.err(a.span, format!("duplicate `known` attribute on field `{field_name}`"));
                        continue;
                    }
                    let is_enum = matches!(ty, Ty::Enum(_))
                        || matches!(ty, Ty::List(elem) if matches!(&**elem, Ty::Enum(_)));
                    if is_enum {
                        rules.known = true;
                    } else {
                        self.err(
                            a.span,
                            format!("`known` is only valid on enum fields or lists of enums, not `{}`", ir.ty_name(ty)),
                        );
                    }
                }
                "check" => {
                    if let Some(name) = self.parse_check_arg(a) {
                        if !seen_checks.insert(name.clone()) {
                            self.err(a.span, format!("duplicate `check({name})` on field `{field_name}`"));
                        } else {
                            rules.checks.push(name);
                        }
                    }
                }
                other => {
                    self.err(a.span, format!("unknown attribute `#[{other}]`"));
                }
            }
        }

        let allow_breaking = if let Some(sp) = allow_breaking_attr {
            if !f.optional && f.default.is_none() && rules.is_empty() {
                self.err(
                    sp,
                    format!("`#[{ALLOW_BREAKING}]` is only valid on a reserved, optional or defaulted field (formerly required)"),
                );
                false
            } else {
                true
            }
        } else {
            false
        };

        if let Presence::Default(val) = presence {
            self.check_default_rules(f.span, field_name, val, &rules);
        }

        (rules, allow_breaking)
    }

    fn parse_range(&mut self, p: Prim, a: &ast::Attr) -> Option<(Option<RangeBound>, Option<RangeBound>)> {
        let mut min: Option<RangeBound> = None;
        let mut max: Option<RangeBound> = None;
        for arg in &a.args {
            let Some(lit) = &arg.value else {
                self.err(arg.span, format!("`range` argument `{}` needs `= value`", arg.name));
                continue;
            };
            let bound = match parse_range_bound(p, lit) {
                Ok(b) => b,
                Err(m) => {
                    self.err(arg.span, format!("`range`: {m}"));
                    continue;
                }
            };
            match arg.name.as_str() {
                "min" => {
                    if min.is_some() {
                        self.err(arg.span, "duplicate `min` in `range`".to_string());
                    }
                    min = Some(bound);
                }
                "max" => {
                    if max.is_some() {
                        self.err(arg.span, "duplicate `max` in `range`".to_string());
                    }
                    max = Some(bound);
                }
                other => self.err(arg.span, format!("unknown `range` argument `{other}` (expected `min` or `max`)")),
            }
        }
        if min.is_none() && max.is_none() {
            if a.args.is_empty() {
                self.err(a.span, "`range` needs at least `min` or `max`".to_string());
            }
            return None;
        }
        if let (Some(lo), Some(hi)) = (&min, &max)
            && cmp_range_bound(lo, hi) == std::cmp::Ordering::Greater
        {
            self.err(a.span, "`range` has `min > max`".to_string());
        }
        Some((min, max))
    }

    fn parse_len(&mut self, a: &ast::Attr) -> Option<(Option<u64>, Option<u64>)> {
        let mut min: Option<u64> = None;
        let mut max: Option<u64> = None;
        for arg in &a.args {
            let Some(lit) = &arg.value else {
                self.err(arg.span, format!("`len` argument `{}` needs `= value`", arg.name));
                continue;
            };
            let val = match lit {
                ast::Literal::Int(i) if *i >= 0 => match u64::try_from(*i) {
                    Ok(v) => v,
                    Err(_) => {
                        self.err(arg.span, "`len` bound exceeds u64".to_string());
                        continue;
                    }
                },
                ast::Literal::UInt(u) => match u64::try_from(*u) {
                    Ok(v) => v,
                    Err(_) => {
                        self.err(arg.span, "`len` bound exceeds u64".to_string());
                        continue;
                    }
                },
                _ => {
                    self.err(arg.span, "`len` expects a non-negative integer".to_string());
                    continue;
                }
            };
            match arg.name.as_str() {
                "min" => {
                    if min.is_some() {
                        self.err(arg.span, "duplicate `min` in `len`".to_string());
                    }
                    min = Some(val);
                }
                "max" => {
                    if max.is_some() {
                        self.err(arg.span, "duplicate `max` in `len`".to_string());
                    }
                    max = Some(val);
                }
                other => self.err(arg.span, format!("unknown `len` argument `{other}` (expected `min` or `max`)")),
            }
        }
        if min.is_none() && max.is_none() {
            if a.args.is_empty() {
                self.err(a.span, "`len` needs at least `min` or `max`".to_string());
            }
            return None;
        }
        if let (Some(lo), Some(hi)) = (min, max)
            && lo > hi
        {
            self.err(a.span, "`len` has `min > max`".to_string());
        }
        Some((min, max))
    }

    fn check_default_rules(&mut self, span: Span, field_name: &str, val: &Value, rules: &FieldRules) {
        let Some((min, max)) = &rules.range else { return };
        let val_bound = match val {
            Value::Int(i) => RangeBound::Int(*i),
            Value::UInt(u) => RangeBound::UInt(*u),
            Value::Float(x) => RangeBound::Float(*x),
            _ => return,
        };
        if let Some(lo) = min
            && cmp_range_bound(&val_bound, lo) == std::cmp::Ordering::Less
        {
            self.err(span, format!("default value of field `{field_name}` is below `range` minimum"));
        }
        if let Some(hi) = max
            && cmp_range_bound(&val_bound, hi) == std::cmp::Ordering::Greater
        {
            self.err(span, format!("default value of field `{field_name}` exceeds `range` maximum"));
        }
    }

    fn check_named_sigs(&mut self, ir: &Ir) {
        #[derive(Clone, PartialEq, Eq)]
        enum Sig {
            Field(Ty),
            Table(String),
        }
        let mut sigs: HashMap<String, (Sig, String)> = HashMap::new();
        for t in ir.tables.iter().filter(|t| t.origin.is_none()) {
            for name in &t.checks {
                let sig = Sig::Table(t.name.clone());
                let desc = format!("table `{}`", t.name);
                if let Some((prev, prev_desc)) = sigs.get(name) {
                    if *prev != sig {
                        self.err(t.span, format!("`check({name})` used on {desc} after being used on {prev_desc}"));
                    }
                } else {
                    sigs.insert(name.clone(), (sig, desc));
                }
            }
            for f in &t.fields {
                for name in &f.rules.checks {
                    let sig = Sig::Field(f.ty.clone());
                    let desc = format!("`{}` (field `{}.{}`)", ir.ty_name(&f.ty), t.name, f.name);
                    if let Some((prev, prev_desc)) = sigs.get(name) {
                        if *prev != sig {
                            self.err(f.span, format!("`check({name})` used on {desc} after being used on {prev_desc}"));
                        }
                    } else {
                        sigs.insert(name.clone(), (sig, desc));
                    }
                }
            }
        }
    }

    /// Resolves the canonical type text coming from the lock (the form of [`Ir::ty_name`]) against
    /// the new schema. Errors become diagnostics with the lock's context.
    fn resolve_lock(&mut self, text: &str, table: &str, id: u16, span: Span) -> Option<Ty> {
        let ctx = format!("lock, reserved field @{id} of `{table}`");
        let tr = match parse_type(text) {
            Ok(tr) => tr,
            Err(e) => {
                self.err(span, format!("{ctx}: invalid type `{text}` ({})", e.msg));
                return None;
            }
        };
        let before = self.diags.len();
        let ty = self.resolve(&tr, span);
        for d in &mut self.diags[before..] {
            d.msg = format!("{ctx}: {}", d.msg);
        }
        ty
    }
}

fn default_value(ir: &Ir, ty: &Ty, lit: &Literal) -> Result<Value, String> {
    let ls = literal_str(lit);
    match ty {
        Ty::Scalar(p) if p.is_float() => {
            let x = match lit {
                Literal::Int(n) => *n as f64,
                Literal::UInt(n) => *n as f64,
                Literal::Float(x) => *x,
                _ => return Err(format!("default `{ls}` is not a {}", p.name())),
            };
            let stored = if *p == Prim::F32 { (x as f32) as f64 } else { x };
            if !stored.is_finite() {
                return Err(format!("default {ls} is too large for {}", p.name()));
            }
            // A non-zero value that rounds to 0 would silently become another value.
            if stored == 0.0 && x != 0.0 {
                return Err(format!("default {ls} is too small for {}: it would become 0", p.name()));
            }
            Ok(Value::Float(x))
        }
        Ty::Scalar(p) => {
            let unsigned_max: u128 = match p {
                Prim::U8 => u8::MAX as u128,
                Prim::U16 => u16::MAX as u128,
                Prim::U32 => u32::MAX as u128,
                Prim::U64 => u64::MAX as u128,
                Prim::U128 => u128::MAX,
                _ => 0,
            };
            let signed = matches!(p, Prim::I8 | Prim::I16 | Prim::I32 | Prim::I64 | Prim::I128);
            let fits = || format!("default {ls} does not fit in {}", p.name());
            match lit {
                Literal::Int(n) if signed => {
                    let (lo, hi) = p.int_range().unwrap();
                    if (lo..=hi).contains(n) { Ok(Value::Int(*n)) } else { Err(fits()) }
                }
                Literal::Int(n) => match u128::try_from(*n) {
                    Ok(u) if u <= unsigned_max => Ok(Value::UInt(u)),
                    _ => Err(fits()),
                },
                Literal::UInt(u) if !signed && *u <= unsigned_max => Ok(Value::UInt(*u)),
                Literal::UInt(_) => Err(fits()),
                _ => Err(format!("default `{ls}` is not a {}", p.name())),
            }
        }
        Ty::Bool => match lit {
            Literal::Bool(b) => Ok(Value::Bool(*b)),
            _ => Err(format!("default `{ls}` is not a bool")),
        },
        Ty::Enum(id) => {
            let e = &ir.enums[id.0];
            match lit {
                Literal::Ident(n) => match e.values.iter().find(|(vn, _)| vn == n) {
                    Some((_, raw)) => Ok(Value::Enum { name: n.clone(), raw: *raw }),
                    None => Err(format!("default `{n}` is not a variant of `{}`", e.name)),
                },
                _ => Err(format!("default of enum `{}` must be the name of a variant (got `{ls}`)", e.name)),
            }
        }
        other => Err(format!("defaults are only allowed on scalar, enum or bool fields (the type is `{}`)", ir.ty_name(other))),
    }
}

/// Computes `Struct::size` in topological order; a cycle ⇒ diagnostic.
fn struct_sizes(cx: &mut Cx, ir: &mut Ir) {
    #[derive(Clone, Copy, PartialEq)]
    enum St {
        New,
        Open,
        Done,
    }
    struct Walk<'a> {
        ir: &'a Ir,
        st: Vec<St>,
        sizes: Vec<u64>,
        stack: Vec<usize>,
        /// Cycles already reported (sequence of structs from the head), so that
        /// each cycle is reported only once, even with several back edges.
        reported: HashSet<Vec<usize>>,
    }
    fn visit(i: usize, w: &mut Walk<'_>, cx: &mut Cx) {
        let ir = w.ir;
        w.st[i] = St::Open;
        w.stack.push(i);
        let mut total: u64 = 0;
        for (_, ty) in &ir.structs[i].fields {
            total = total.saturating_add(match ty {
                Ty::Struct(j) => {
                    let j = j.0;
                    match w.st[j] {
                        St::Open => {
                            let from = w.stack.iter().position(|&k| k == j).unwrap();
                            let cycle = w.stack[from..].to_vec();
                            if !w.reported.insert(cycle.clone()) {
                                continue;
                            }
                            let mut path: Vec<&str> = cycle.iter().map(|&k| ir.structs[k].name.as_str()).collect();
                            path.push(&ir.structs[j].name);
                            cx.err(
                                ir.structs[j].span,
                                format!(
                                    "struct `{}` is recursive: {} (recursion is only allowed through a table)",
                                    ir.structs[j].name,
                                    path.join(" -> ")
                                ),
                            );
                            0
                        }
                        St::New => {
                            visit(j, w, cx);
                            w.sizes[j]
                        }
                        St::Done => w.sizes[j],
                    }
                }
                Ty::Enum(e) => ir.enums[e.0].base.size() as u64,
                Ty::Scalar(p) => p.size() as u64,
                _ => 1, // bool: 1 byte (the others were rejected)
            });
        }
        w.stack.pop();
        w.sizes[i] = total;
        w.st[i] = St::Done;
    }

    let n = ir.structs.len();
    let mut w = Walk { ir, st: vec![St::New; n], sizes: vec![0u64; n], stack: Vec::new(), reported: HashSet::new() };
    for i in 0..n {
        if w.st[i] == St::New {
            visit(i, &mut w, cx);
        }
    }
    let sizes = w.sizes;
    for (i, s) in ir.structs.iter_mut().enumerate() {
        if sizes[i] > u16::MAX as u64 {
            cx.err(s.span, format!("struct `{}` has {} bytes; the maximum is 65535", s.name, sizes[i]));
            s.size = 0;
        } else {
            s.size = sizes[i] as u16;
        }
    }
}

fn parse_range_bound(p: Prim, lit: &ast::Literal) -> Result<RangeBound, String> {
    if p.is_float() {
        let x = match lit {
            ast::Literal::Float(x) => *x,
            ast::Literal::Int(i) => *i as f64,
            ast::Literal::UInt(u) => *u as f64,
            _ => return Err(format!("expected a numeric bound for `{}`", p.name())),
        };
        if !x.is_finite() {
            return Err("bound must be finite".to_string());
        }
        if p == Prim::F32 && !(x as f32).is_finite() {
            return Err("bound overflows `f32`".to_string());
        }
        return Ok(RangeBound::Float(if p == Prim::F32 { (x as f32) as f64 } else { x }));
    }
    if p == Prim::U128 {
        return match lit {
            ast::Literal::Int(i) if *i >= 0 => Ok(RangeBound::UInt(*i as u128)),
            ast::Literal::UInt(u) => Ok(RangeBound::UInt(*u)),
            ast::Literal::Int(_) => Err("bound is out of range for `u128`".to_string()),
            _ => Err("expected an integer bound for `u128`".to_string()),
        };
    }
    let (lo, hi) = p.int_range().unwrap();
    match lit {
        ast::Literal::Int(i) if *i >= lo && *i <= hi => Ok(RangeBound::Int(*i)),
        ast::Literal::Int(_) | ast::Literal::UInt(_) => Err(format!("bound is out of range for `{}`", p.name())),
        _ => Err(format!("expected an integer bound for `{}`", p.name())),
    }
}

fn cmp_range_bound(a: &RangeBound, b: &RangeBound) -> std::cmp::Ordering {
    match (a, b) {
        (RangeBound::Int(x), RangeBound::Int(y)) => x.cmp(y),
        (RangeBound::UInt(x), RangeBound::UInt(y)) => x.cmp(y),
        (RangeBound::Int(x), RangeBound::UInt(y)) => {
            if *x < 0 {
                std::cmp::Ordering::Less
            } else {
                (*x as u128).cmp(y)
            }
        }
        (RangeBound::UInt(x), RangeBound::Int(y)) => {
            if *y < 0 {
                std::cmp::Ordering::Greater
            } else {
                x.cmp(&(*y as u128))
            }
        }
        (RangeBound::Float(x), RangeBound::Float(y)) => x.total_cmp(y),
        _ => std::cmp::Ordering::Equal,
    }
}
