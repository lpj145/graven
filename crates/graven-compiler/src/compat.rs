//! Schema compatibility check (spec §9): compares the previous lock with the new IR
//! and reports what is an error, what is a warning (requires confirmation in the schema)
//! and what is allowed.
//!
//! The comparison is lock against lock: the new side becomes a lock through
//! [`lock_from_ir`]`(new, Some(old))`, so type, size, slot, bit and presence come out in
//! the same canonical text on both sides. The new IR only provides positions (`span`) and
//! the fields' `#[allow_breaking_readers]`.
//!
//! **One cause, one finding.** Changing the base of an enum or a struct changes the size of
//! every struct and field that uses it (inline); only the root cause is reported:
//! - a name that was an enum or struct in the lock and exists in the new schema as another
//!   kind (enum ↔ struct ↔ table ↔ union) produces a `type-changed` on the type itself
//!   (`table` = the type's name, no field), even with the same size;
//! - a type is *altered* if it changed kind like that, or is an enum on both sides with a
//!   different base, or a struct on both sides with different fields or a different size;
//! - a table or struct field with the same type text and a different size/kind does not
//!   produce `type-changed`/`struct-changed` if its (element) type is altered: the cause was
//!   (or will be) reported there. With no altered type to explain it, the difference is
//!   reported on the field itself (e.g. a table that became a union with the same name);
//! - fields that swapped IDs (`ids-reordered`) get no other findings;
//! - `layout-changed` (defensive: a different slot/bit with the same types and sizes, which
//!   the sequential layout never produces by itself) is only checked in a table with no
//!   type or size difference at all.
//!
//! `namespace-changed`: a new namespace changes every schema id (a new lockfile); it is
//! the only finding in that case, and nothing else is compared.
//!
//! **Order** (deterministic): enums, structs, unions (each in the old lock's order), then
//! tables in the old lock's order; within a table, by ID.

use std::collections::HashMap;
use std::fmt;

use crate::ast::Span;
use crate::ir::Ir;
use crate::lock::{Lock, LockField, LockTable, lock_from_ir};

/// Error rules (the compiler refuses).
pub const ERROR_RULES: [&str; 15] = [
    "type-changed",
    "id-reused",
    "ids-reordered",
    "field-removed",
    "enum-base-changed",
    "struct-changed",
    "new-required",
    "optional-to-required",
    "table-removed",
    "default-changed",
    "default-added",
    "enum-variant-changed",
    "union-variant-changed",
    "layout-changed",
    "namespace-changed",
];

/// Warning rules (an error without `#[allow_breaking_readers]`).
pub const WARNING_RULES: [&str; 2] = ["required-reserved", "required-relaxed"];

/// Allowed changes (never produce a finding); names for documentation and tests.
pub const ALLOWED_RULES: [&str; 6] = [
    "new-optional",
    "new-default",
    "reserved-optional",
    "new-enum-variant",
    "new-union-variant",
    "field-renamed",
];

/// Result of the check. `table` is the name of the type (table, enum, struct or union) and
/// `field` the field / variant when the rule is about one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Finding {
    Error { rule: &'static str, table: String, field: Option<String>, span: Span, msg: String },
    /// `confirmed` = the field has `#[allow_breaking_readers]`. Unconfirmed counts as an
    /// error for the caller (see [`blocking`]).
    Warning { rule: &'static str, table: String, field: Option<String>, span: Span, msg: String, confirmed: bool },
}

impl Finding {
    pub fn rule(&self) -> &'static str {
        match self {
            Finding::Error { rule, .. } | Finding::Warning { rule, .. } => rule,
        }
    }

    pub fn msg(&self) -> &str {
        match self {
            Finding::Error { msg, .. } | Finding::Warning { msg, .. } => msg,
        }
    }

    pub fn span(&self) -> Span {
        match self {
            Finding::Error { span, .. } | Finding::Warning { span, .. } => *span,
        }
    }

    pub fn is_error(&self) -> bool {
        matches!(self, Finding::Error { .. })
    }

    /// An error, or an unconfirmed warning.
    pub fn is_blocking(&self) -> bool {
        matches!(self, Finding::Error { .. } | Finding::Warning { confirmed: false, .. })
    }
}

impl fmt::Display for Finding {
    /// `error [rule] line:col: message` / `warning [rule] line:col: message`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let kind = if self.is_error() { "error" } else { "warning" };
        let s = self.span();
        write!(f, "{kind} [{}] {}:{}: {}", self.rule(), s.line, s.col, self.msg())
    }
}

/// `true` if some finding blocks compilation (an error or an unconfirmed warning).
pub fn blocking(findings: &[Finding]) -> bool {
    findings.iter().any(Finding::is_blocking)
}

/// Kind of a type name according to a lock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Builtin,
    Enum,
    Struct,
    Table,
    Union,
}

fn kinds(l: &Lock) -> HashMap<&str, Kind> {
    let mut m = HashMap::new();
    m.extend(l.enums.iter().map(|e| (e.name.as_str(), Kind::Enum)));
    m.extend(l.structs.iter().map(|s| (s.name.as_str(), Kind::Struct)));
    m.extend(l.tables.iter().map(|t| (t.name.as_str(), Kind::Table)));
    m.extend(l.unions.iter().map(|u| (u.name.as_str(), Kind::Union)));
    m
}

/// Element name of a canonical type: `"[[Leaf]]"` → `"Leaf"`.
fn elem(ty: &str) -> &str {
    ty.trim_start_matches('[').trim_end_matches(']')
}

fn kind_word(k: Kind) -> &'static str {
    match k {
        Kind::Builtin => "built-in",
        Kind::Enum => "enum",
        Kind::Struct => "struct",
        Kind::Table => "table",
        Kind::Union => "union",
    }
}

fn presence_word(p: &str) -> &str {
    match p {
        "required" => "required",
        "optional" => "optional",
        "reserved" => "reserved",
        _ => "defaulted",
    }
}

struct Cx<'a> {
    old: &'a Lock,
    new: &'a Lock,
    ir: &'a Ir,
    old_kinds: HashMap<&'a str, Kind>,
    new_kinds: HashMap<&'a str, Kind>,
    out: Vec<Finding>,
}

/// Compares the previous lock with the new IR (spec §9). See the module docs for the policy
/// on root causes and ordering.
pub fn compat(old: &Lock, new: &Ir) -> Vec<Finding> {
    let new_lock = lock_from_ir(new, Some(old));
    let mut cx = Cx { old, new: &new_lock, ir: new, old_kinds: kinds(old), new_kinds: kinds(&new_lock), out: Vec::new() };
    if old.namespace != new.namespace {
        // A new namespace changes every schema id: comparing the rest makes no sense.
        cx.error(
            "namespace-changed",
            &new.namespace,
            None,
            Span::default(),
            format!(
                "namespace changed from `{}` to `{}`: this changes the schema id of every table; if that is intended, start a new lockfile",
                old.namespace, new.namespace
            ),
        );
        return cx.out;
    }
    cx.enums();
    cx.structs();
    cx.unions();
    for ot in &old.tables {
        cx.table(ot);
    }
    imported(cx.out)
}

/// Rules for types imported from another namespace (their names are `namespace.Type`).
pub const IMPORT_RULES: [&str; 1] = ["import-changed"];

/// Findings about imported types become `import-changed`, naming the namespace where the
/// change was made. An imported table that is no longer used simply leaves the lock: that is
/// not a removal, since the namespace that declares it still has it.
fn imported(findings: Vec<Finding>) -> Vec<Finding> {
    findings
        .into_iter()
        .filter_map(|f| {
            let (rule, table) = match &f {
                Finding::Error { rule, table, .. } | Finding::Warning { rule, table, .. } => (*rule, table.clone()),
            };
            let Some((ns, _)) = table.rsplit_once('.') else { return Some(f) };
            if rule == "table-removed" {
                return None;
            }
            let wrap = |msg: &str| {
                format!(
                    "imported type `{table}` (declared in namespace `{ns}`) changed: {} (import-changed)",
                    msg.trim_end_matches(&format!(" ({rule})"))
                )
            };
            Some(match f {
                Finding::Error { table, field, span, msg, .. } => {
                    Finding::Error { rule: "import-changed", msg: wrap(&msg), table, field, span }
                }
                Finding::Warning { table, field, span, msg, confirmed, .. } => {
                    Finding::Warning { rule: "import-changed", msg: wrap(&msg), table, field, span, confirmed }
                }
            })
        })
        .collect()
}

impl<'a> Cx<'a> {
    fn error(&mut self, rule: &'static str, table: &str, field: Option<&str>, span: Span, msg: String) {
        let msg = format!("{msg} ({rule})");
        self.out.push(Finding::Error { rule, table: table.to_string(), field: field.map(String::from), span, msg });
    }

    /// The type `name` (element) changed representation: an enum on both sides with a different
    /// base, or a struct on both sides with different fields or a different size. The root cause
    /// of every such change is reported on the enum/struct (or on a type inside it).
    fn altered(&self, name: &str) -> bool {
        if self.kind_changed(name) {
            return true;
        }
        if let (Some(o), Some(n)) = (self.old.enums.iter().find(|e| e.name == name), self.new.enums.iter().find(|e| e.name == name)) {
            return o.base != n.base;
        }
        if let (Some(o), Some(n)) =
            (self.old.structs.iter().find(|s| s.name == name), self.new.structs.iter().find(|s| s.name == name))
        {
            return o.fields != n.fields || o.size != n.size;
        }
        false
    }

    /// `name` was an enum or struct in the lock and exists in the new schema as another kind
    /// (enum ↔ struct ↔ table ↔ union). It is a root cause: reported once, on the type.
    fn kind_changed(&self, name: &str) -> bool {
        match (self.old_kinds.get(name), self.new_kinds.get(name)) {
            (Some(o @ (Kind::Enum | Kind::Struct)), Some(n)) => o != n,
            _ => false,
        }
    }

    /// Position of a type in the new schema (any kind).
    fn new_span(&self, name: &str) -> Span {
        let ir = self.ir;
        ir.enums
            .iter()
            .find(|x| x.name == name)
            .map(|x| x.span)
            .or_else(|| ir.structs.iter().find(|x| x.name == name).map(|x| x.span))
            .or_else(|| ir.tables.iter().find(|x| x.name == name).map(|x| x.span))
            .or_else(|| ir.unions.iter().find(|x| x.name == name).map(|x| x.span))
            .unwrap_or_default()
    }

    fn kind_change_error(&mut self, name: &str) {
        let (o, n) = (self.old_kinds[name], self.new_kinds[name]);
        let msg = format!(
            "type `{name}` changed kind ({} → {}): every field that uses it would be read with another representation",
            kind_word(o),
            kind_word(n)
        );
        self.error("type-changed", name, None, self.new_span(name), msg);
    }

    fn enums(&mut self) {
        for oe in &self.old.enums {
            if self.kind_changed(&oe.name) {
                self.kind_change_error(&oe.name);
                continue;
            }
            let Some(ne) = self.new.enums.iter().find(|e| e.name == oe.name) else { continue };
            let span = self.ir.enums.iter().find(|e| e.name == oe.name).map_or_else(Span::default, |e| e.span);
            if oe.base != ne.base {
                self.error(
                    "enum-base-changed",
                    &oe.name,
                    None,
                    span,
                    format!("enum `{}`: base type changed from `{}` to `{}`", oe.name, oe.base, ne.base),
                );
            }
            for ov in &oe.values {
                let by_name = ne.values.iter().find(|v| v.name == ov.name);
                let msg = match by_name {
                    Some(nv) if nv.value != ov.value => format!(
                        "variant `{}.{}` changed value ({} → {}): old messages would be read as another variant",
                        oe.name, ov.name, ov.value, nv.value
                    ),
                    None if !ne.values.iter().any(|v| v.value == ov.value) => format!(
                        "variant `{}.{}` (= {}) was removed: old messages would carry an unknown value",
                        oe.name, ov.name, ov.value
                    ),
                    // Same name and value, or renamed (the value is still there under another name).
                    _ => continue,
                };
                self.error("enum-variant-changed", &oe.name, Some(&ov.name), span, msg);
            }
        }
    }

    fn structs(&mut self) {
        for os in &self.old.structs {
            if self.kind_changed(&os.name) {
                self.kind_change_error(&os.name);
                continue;
            }
            let Some(ns) = self.new.structs.iter().find(|s| s.name == os.name) else { continue };
            let span = self.ir.structs.iter().find(|s| s.name == os.name).map_or_else(Span::default, |s| s.span);
            let fmt_fields =
                |fs: &[crate::lock::LockStructField]| fs.iter().map(|f| format!("{}: {}", f.name, f.ty)).collect::<Vec<_>>().join(", ");
            if os.fields != ns.fields {
                let msg = format!(
                    "struct `{}` changed ({{ {} }} → {{ {} }}); a struct is frozen",
                    os.name,
                    fmt_fields(&os.fields),
                    fmt_fields(&ns.fields)
                );
                self.error("struct-changed", &os.name, None, span, msg);
            } else if os.size != ns.size && !ns.fields.iter().any(|f| self.altered(elem(&f.ty))) {
                // Defensive: same fields, different size and no altered type (including a
                // change of kind) to explain it; should not happen.
                let msg = format!(
                    "struct `{}` changed size ({} → {} bytes) without changing its fields",
                    os.name, os.size, ns.size
                );
                self.error("struct-changed", &os.name, None, span, msg);
            }
        }
    }

    fn unions(&mut self) {
        for ou in &self.old.unions {
            let Some(nu) = self.new.unions.iter().find(|u| u.name == ou.name) else { continue };
            let span = self.ir.unions.iter().find(|u| u.name == ou.name).map_or_else(Span::default, |u| u.span);
            for ov in &ou.variants {
                let msg = match nu.variants.iter().find(|v| v.tag == ov.tag) {
                    None => format!(
                        "variant `{}.{}` (tag {}) was removed: old messages would carry an unknown tag",
                        ou.name, ov.name, ov.tag
                    ),
                    Some(nv) if nv.table != ov.table => format!(
                        "tag {} of `{}` changed table (`{}` → `{}`): old messages would be read as another table",
                        ov.tag, ou.name, ov.table, nv.table
                    ),
                    // Same tag and table (maybe renamed).
                    Some(_) => continue,
                };
                self.error("union-variant-changed", &ou.name, Some(&ov.name), span, msg);
            }
        }
    }

    /// Did the field's type change? `Some(message)` if so (and the cause is not an altered type).
    fn type_change(&self, of: &LockField, nf: &LockField) -> Option<String> {
        if of.ty != nf.ty {
            return Some(format!("type changed from `{}` to `{}`", of.ty, nf.ty));
        }
        let name = elem(&of.ty);
        let ok = self.old_kinds.get(name).copied().unwrap_or(Kind::Builtin);
        let nk = self.new_kinds.get(name).copied().unwrap_or(Kind::Builtin);
        if ok != nk && !self.kind_changed(name) {
            return Some(format!("type `{}` changed kind ({ok:?} → {nk:?})", of.ty));
        }
        if of.size != nf.size && !self.altered(name) {
            return Some(format!("size of `{}` changed ({} → {} bytes)", of.ty, of.size, nf.size));
        }
        None
    }

    /// Value of a default for comparison: for an enum, the variant's numeric value
    /// (renaming the variant does not change the default); otherwise, the canonical text.
    fn default_value(&self, lock: &Lock, ty: &str, lit: &str) -> String {
        lock.enums
            .iter()
            .find(|e| e.name == ty)
            .and_then(|e| e.values.iter().find(|v| v.name == lit))
            .map_or_else(|| lit.to_string(), |v| v.value.to_string())
    }

    fn table(&mut self, ot: &LockTable) {
        let Some(nt) = self.new.tables.iter().find(|t| t.name == ot.name) else {
            self.error(
                "table-removed",
                &ot.name,
                None,
                Span::default(),
                format!("table `{}` was removed: old writers and readers still use it", ot.name),
            );
            return;
        };
        let it = self.ir.tables.iter().find(|t| t.name == ot.name).expect("table from the new lock is in the IR");
        let tname = ot.name.as_str();
        let active = |f: &LockField| f.presence != "reserved";

        // Swapped IDs: the field at ID `i` now has the name of another old active field.
        let old_ids: HashMap<&str, u16> = ot.fields.iter().filter(|f| active(f)).map(|f| (f.name.as_str(), f.id)).collect();
        let mut reordered = vec![false; nt.fields.len()];
        for (nf, of) in nt.fields.iter().zip(&ot.fields) {
            if active(nf)
                && active(of)
                && nf.name != of.name
                && let Some(&was) = old_ids.get(nf.name.as_str())
            {
                reordered[usize::from(nf.id)] = true;
                let span = it.fields[usize::from(nf.id)].span;
                self.error(
                    "ids-reordered",
                    tname,
                    Some(&nf.name),
                    span,
                    format!(
                        "field `{tname}.{}` moved from @{was} to @{}: old messages would be read into the wrong field",
                        nf.name, nf.id
                    ),
                );
            }
        }
        // A field moved to a new ID at the end, with its old ID now taken by another active
        // field. If the old ID became reserved, it is just a new field reusing a name.
        for nf in nt.fields.iter().skip(ot.fields.len()) {
            if active(nf)
                && let Some(&was) = old_ids.get(nf.name.as_str())
                && let Some(occupant) = nt.fields.get(usize::from(was))
                && active(occupant)
                && occupant.name != nf.name
            {
                reordered[usize::from(nf.id)] = true;
                let span = it.fields[usize::from(nf.id)].span;
                self.error(
                    "ids-reordered",
                    tname,
                    Some(&nf.name),
                    span,
                    format!(
                        "field `{tname}.{}` moved from @{was} to @{}, and @{was} is now `{}`: old messages would be read into the wrong field",
                        nf.name, nf.id, occupant.name
                    ),
                );
            }
        }

        let mut same_shape = true;
        for id in 0..ot.fields.len().max(nt.fields.len()) {
            match (ot.fields.get(id), nt.fields.get(id)) {
                (None, None) => unreachable!(),
                (Some(of), None) => {
                    same_shape = false;
                    self.error(
                        "field-removed",
                        tname,
                        Some(&of.name),
                        it.span,
                        format!(
                            "field `{tname}.{}` (@{id}) was removed; mark it `@{id}: reserved;` to keep the layout",
                            of.name
                        ),
                    );
                }
                (None, Some(nf)) => {
                    if nf.presence == "required" && reordered.get(id) != Some(&true) {
                        self.error(
                            "new-required",
                            tname,
                            Some(&nf.name),
                            it.fields[id].span,
                            format!(
                                "new field `{tname}.{}` (@{id}) is required: every new reader would fail on old messages; use `{}?` or a default",
                                nf.name, nf.ty
                            ),
                        );
                    }
                }
                (Some(of), Some(nf)) => {
                    if of.ty != nf.ty || of.size != nf.size {
                        same_shape = false;
                    }
                    if reordered.get(id) != Some(&true) {
                        self.field(tname, of, nf, it.fields[id].span, it.fields[id].allow_breaking);
                    }
                }
            }
        }

        // Defensive: with the same types and sizes, the sequential layout gives the same
        // slots and bits; a difference here means a hand-edited lock or a changed layout.
        if same_shape && ot.fields.len() <= nt.fields.len() {
            for (of, nf) in ot.fields.iter().zip(&nt.fields) {
                if (of.slot, of.bit) != (nf.slot, nf.bit) {
                    let name = if active(nf) { &nf.name } else { &of.name };
                    let span = it.fields[usize::from(nf.id)].span;
                    self.error(
                        "layout-changed",
                        tname,
                        Some(name),
                        span,
                        format!(
                            "layout of `{tname}.{name}` (@{}) differs from the lock (slot {:?}/bit {:?} → slot {:?}/bit {:?}) without a type change",
                            nf.id, of.slot, of.bit, nf.slot, nf.bit
                        ),
                    );
                }
            }
        }
    }

    fn field(&mut self, tname: &str, of: &LockField, nf: &LockField, span: Span, confirmed: bool) {
        let id = of.id;
        let (o_res, n_res) = (of.presence == "reserved", nf.presence == "reserved");
        if o_res && !n_res {
            self.error(
                "id-reused",
                tname,
                Some(&nf.name),
                span,
                format!(
                    "@{id} of `{tname}` is reserved (was `{}`) and cannot be reused by `{}`; use a new ID at the end",
                    of.ty, nf.name
                ),
            );
            return;
        }
        // Name in the finding: the new schema's, unless the field became reserved.
        let name = if n_res { of.name.as_str() } else { nf.name.as_str() };
        if let Some(why) = self.type_change(of, nf) {
            self.error("type-changed", tname, Some(name), span, format!("field `{tname}.{name}` (@{id}): {why}"));
            return;
        }
        let (op, np) = (of.presence.as_str(), nf.presence.as_str());
        if op == "required" && np != "required" {
            // Required → reserved/optional/default: new writers may omit the field and old
            // readers get `Missing`. Confirmation is required in both cases, so reserving in
            // two steps (→ optional, → reserved) does not escape the warning.
            let (rule, what) = if n_res {
                ("required-reserved", "was reserved")
            } else {
                ("required-relaxed", "is no longer required")
            };
            let msg = format!(
                "required field `{tname}.{name}` (@{id}) {what}: old readers get `Missing` from new messages without it{} ({rule})",
                if confirmed {
                    " (confirmed with `#[allow_breaking_readers]`)"
                } else {
                    "; confirm with `#[allow_breaking_readers]` on the field"
                }
            );
            self.out.push(Finding::Warning { rule, table: tname.to_string(), field: Some(name.to_string()), span, msg, confirmed });
        }
        if n_res {
            // Optional/default → reserved: old readers see absent (or the default).
            return;
        }
        if np == "required" && op != "required" {
            self.error(
                "optional-to-required",
                tname,
                Some(name),
                span,
                format!(
                    "field `{tname}.{name}` (@{id}) was {} and became required: old messages without it would fail with `Missing`",
                    presence_word(op)
                ),
            );
            return;
        }
        if let (Some(ol), Some(nl)) = (op.strip_prefix("default:"), np.strip_prefix("default:"))
            && ol != nl
            && self.default_value(self.old, &of.ty, ol) != self.default_value(self.new, &nf.ty, nl)
        {
            self.error(
                "default-changed",
                tname,
                Some(name),
                span,
                format!(
                    "default of `{tname}.{name}` (@{id}) changed from `{ol}` to `{nl}`: old messages without the field would change meaning"
                ),
            );
        }
        if op == "optional" && np.starts_with("default:") {
            // Old messages without the field meant "absent"; they would start reading the
            // default. This also closes the `= 5` → `?` → `= 6` bypass of `default-changed`.
            self.error(
                "default-added",
                tname,
                Some(name),
                span,
                format!(
                    "optional field `{tname}.{name}` (@{id}) gained default `{}`: old messages without the field would start reading the default",
                    &np["default:".len()..]
                ),
            );
        }
        // Other transitions are allowed: required → optional/default (with the warning
        // above), default → optional, renaming.
    }
}
