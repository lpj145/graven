//! Validation codegen (`Checks`, `validate`, `validateAll`, `toBytesValidated`).

use std::collections::{BTreeMap, BTreeSet};

use super::{JsCx, Out, js_str, p, p_default};
use crate::ir::*;

pub(super) fn has_any_rules(ir: &Ir) -> bool {
    ir.tables.iter().any(|t| !t.checks.is_empty() || t.fields.iter().any(|f| !f.rules.is_empty()))
}

pub(super) fn has_custom_checks(ir: &Ir) -> bool {
    ir.tables
        .iter()
        .any(|t| !t.checks.is_empty() || t.fields.iter().any(|f| !f.rules.checks.is_empty()))
}

pub(super) fn table_needs_validate(ir: &Ir, tid: TableId) -> bool {
    fn visit(ir: &Ir, tid: TableId, seen: &mut Vec<bool>) -> bool {
        if seen[tid.0] {
            return false;
        }
        seen[tid.0] = true;
        let t = &ir.tables[tid.0];
        if !t.checks.is_empty() || t.fields.iter().any(|f| !f.rules.is_empty()) {
            return true;
        }
        for f in &t.fields {
            if matches!(f.presence, Presence::Reserved { .. }) {
                continue;
            }
            match &f.ty {
                Ty::Table(sub) if visit(ir, *sub, seen) => return true,
                Ty::List(elem) => {
                    if let Ty::Table(sub) = &**elem
                        && visit(ir, *sub, seen)
                    {
                        return true;
                    }
                }
                Ty::Union(uid) => {
                    for (_, _, vtid) in &ir.unions[uid.0].variants {
                        if visit(ir, *vtid, seen) {
                            return true;
                        }
                    }
                }
                _ => {}
            }
        }
        false
    }
    let mut seen = vec![false; ir.tables.len()];
    visit(ir, tid, &mut seen)
}

impl<'a> JsCx<'a> {
    pub(super) fn emit_checks_interface(&self, out: &mut Out) {
        if !has_any_rules(self.ir) {
            return;
        }
        enum Target<'b> {
            Table(TableId),
            Field(&'b Ty),
        }
        let mut methods: BTreeMap<String, Target<'_>> = BTreeMap::new();
        for (i, t) in self.ir.tables.iter().enumerate().filter(|(_, t)| t.origin.is_none()) {
            for c in &t.checks {
                methods.entry(c.clone()).or_insert(Target::Table(TableId(i)));
            }
            for f in &t.fields {
                for c in &f.rules.checks {
                    methods.entry(c.clone()).or_insert(Target::Field(&f.ty));
                }
            }
        }

        let mut supers: BTreeSet<String> = BTreeSet::new();
        for (i, t) in self.ir.tables.iter().enumerate() {
            if let Some(ns) = &t.origin
                && table_needs_validate(self.ir, TableId(i))
                && let Some(alias) = self.imports.get(ns)
            {
                supers.insert(format!("{alias}.Checks"));
            }
        }

        let ext = if supers.is_empty() {
            String::new()
        } else {
            format!(" extends {}", supers.into_iter().collect::<Vec<_>>().join(", "))
        };
        out.dts("/** Application-provided validation functions declared with `#[check(name)]`. */");
        out.dts(&format!("export interface Checks{ext} {{"));
        for (name, target) in &methods {
            let arg_ty = match target {
                Target::Table(tid) => self.table_path(*tid, "Verified"),
                Target::Field(ty) => self.check_arg_ty(ty),
            };
            out.dts(&format!("  {name}(v: {arg_ty}): boolean;"));
        }
        out.dts("}");
        out.dts("");
    }

    fn check_arg_ty(&self, ty: &Ty) -> String {
        match ty {
            Ty::Str => "string".to_string(),
            Ty::Bytes => "Uint8Array".to_string(),
            Ty::Table(tid) => self.table_path(*tid, "Verified"),
            Ty::Union(uid) => self.union_path(*uid, "Verified"),
            Ty::List(elem) => match &**elem {
                Ty::Str => "_graven.VStrList".to_string(),
                Ty::Bytes => "_graven.BytesList".to_string(),
                Ty::Table(tid) => format!("_graven.TableViewList<{}>", self.table_path(*tid, "Verified")),
                inline => format!("_graven.ScalarList<{}>", self.inline_ty(inline)),
            },
            inline => self.inline_ty(inline),
        }
    }

    pub(super) fn emit_validate_methods(&self, out: &mut Out, t: &Table, fields: &[(&Field, &str)]) {
        if !has_any_rules(self.ir) {
            return;
        }
        let checks = || if has_custom_checks(self.ir) { p("checks", "Checks") } else { p_default("checks", "Checks", "{}") };
        let mut body = vec!["void checks;".to_string()];
        body.extend(self.validate_body(t, fields, false));
        out.method("  ", false, "validate", &[checks()], "void", &body);
        out.method(
            "  ",
            false,
            "validateAll",
            &[checks()],
            "_graven.ValidationFailure[]",
            &["const out = [];".into(), "this._validateAllInto(checks, out);".into(), "return out;".into()],
        );
        let mut body = vec!["void checks;".to_string(), "void out;".to_string()];
        body.extend(self.validate_body(t, fields, true));
        out.method(
            "  ",
            false,
            "_validateAllInto",
            &[p("checks", "Checks"), p("out", "_graven.ValidationFailure[]")],
            "void",
            &body,
        );
    }

    pub(super) fn emit_to_bytes_validated(&self, out: &mut Out, name: &str, name_w: &str) {
        if !has_any_rules(self.ir) {
            return;
        }
        let checks = if has_custom_checks(self.ir) { p("checks", "Checks") } else { p_default("checks", "Checks", "{}") };
        out.method(
            "  ",
            true,
            "toBytesValidated",
            &[p("fn", format!("(w: {name_w}) => void")), checks],
            "Uint8Array",
            &[
                format!("const bytes = {name_w}.toBytes(fn);"),
                format!("{name}.fromBytes(bytes).validate(checks);"),
                "return bytes;".into(),
            ],
        );
    }

    fn validate_body(&self, t: &Table, fields: &[(&Field, &str)], collect_all: bool) -> Vec<String> {
        let mut out = Vec::new();
        let tname_lit = js_str(&t.name);
        for &(f, v_name) in fields {
            if f.rules.is_empty() && !self.field_needs_descend(&f.ty) {
                continue;
            }
            let fname_lit = js_str(&f.name);
            out.push("{".into());
            out.push(format!("  const v = this.{v_name}();"));
            if matches!(f.presence, Presence::Optional) {
                out.push("  if (v !== undefined) {".into());
                self.field_rules(&mut out, &tname_lit, &fname_lit, f, collect_all, "    ");
                out.push("  }".into());
            } else {
                self.field_rules(&mut out, &tname_lit, &fname_lit, f, collect_all, "  ");
            }
            out.push("}".into());
        }
        for c in &t.checks {
            let cname_lit = js_str(&format!("check({c})"));
            let fail = if collect_all {
                format!("out.push({{ table: {tname_lit}, rule: {cname_lit} }});")
            } else {
                format!("throw new _graven.ValidationError({{ table: {tname_lit}, rule: {cname_lit} }});")
            };
            out.push(format!("if (!checks.{c}(this)) {fail}"));
        }
        out
    }

    fn field_needs_descend(&self, ty: &Ty) -> bool {
        match ty {
            Ty::Table(tid) => table_needs_validate(self.ir, *tid),
            Ty::List(elem) => matches!(&**elem, Ty::Table(tid) if table_needs_validate(self.ir, *tid)),
            Ty::Union(uid) => self.ir.unions[uid.0].variants.iter().any(|(_, _, tid)| table_needs_validate(self.ir, *tid)),
            _ => false,
        }
    }

    fn field_rules(
        &self,
        out: &mut Vec<String>,
        tname_lit: &str,
        fname_lit: &str,
        f: &Field,
        collect_all: bool,
        ind: &str,
    ) {
        let emit_err = |rule_lit: &str, idx_expr: Option<&str>| -> String {
            let idx_part = match idx_expr {
                Some(i) => format!(", index: {i}"),
                None => String::new(),
            };
            let obj = format!("{{ table: {tname_lit}, field: {fname_lit}{idx_part}, rule: {rule_lit} }}");
            if collect_all { format!("out.push({obj});") } else { format!("throw new _graven.ValidationError({obj});") }
        };

        if let Some((min, max)) = &f.rules.range {
            let Ty::Scalar(p) = f.ty else { unreachable!() };
            let is_float = p.is_float();
            let lo = min.as_ref().map(|b| {
                let lit = range_bound_lit(p, b);
                if is_float { format!("!(v >= {lit})") } else { format!("v < {lit}") }
            });
            let hi = max.as_ref().map(|b| {
                let lit = range_bound_lit(p, b);
                if is_float { format!("!(v <= {lit})") } else { format!("v > {lit}") }
            });
            let cond = match (lo, hi) {
                (Some(a), Some(b)) => format!("{a} || {b}"),
                (Some(a), None) | (None, Some(a)) => a,
                (None, None) => unreachable!(),
            };
            out.push(format!("{ind}if ({cond}) {}", emit_err("\"range\"", None)));
        }

        if let Some((min, max)) = f.rules.len {
            let len_expr = match &f.ty {
                Ty::Str => "_graven.utf8Len(v)",
                _ => "v.length",
            };
            let lo = min.map(|m| format!("{len_expr} < {m}"));
            let hi = max.map(|m| format!("{len_expr} > {m}"));
            let cond = match (lo, hi) {
                (Some(a), Some(b)) => format!("{a} || {b}"),
                (Some(a), None) | (None, Some(a)) => a,
                (None, None) => unreachable!(),
            };
            out.push(format!("{ind}if ({cond}) {}", emit_err("\"len\"", None)));
        }

        if f.rules.finite {
            out.push(format!("{ind}if (!Number.isFinite(v)) {}", emit_err("\"finite\"", None)));
        }

        if f.rules.known {
            match &f.ty {
                Ty::Enum(eid) => {
                    let e = &self.ir.enums[eid.0];
                    let ep = self.type_path(&e.name, &e.origin, "");
                    out.push(format!("{ind}if ({ep}.name(v) === undefined) {}", emit_err("\"known\"", None)));
                }
                Ty::List(elem) if matches!(&**elem, Ty::Enum(_)) => {
                    let Ty::Enum(eid) = &**elem else { unreachable!() };
                    let e = &self.ir.enums[eid.0];
                    let ep = self.type_path(&e.name, &e.origin, "");
                    let fail = emit_err("\"known\"", Some("i"));
                    out.push(format!(
                        "{ind}for (let i = 0; i < v.length; i++) {{ const el = v.get(i); if (el !== undefined && {ep}.name(el) === undefined) {fail} }}"
                    ));
                }
                _ => {}
            }
        }

        for c in &f.rules.checks {
            let rule_lit = js_str(&format!("check({c})"));
            out.push(format!("{ind}if (!checks.{c}(v)) {}", emit_err(&rule_lit, None)));
        }

        match &f.ty {
            Ty::Table(tid) if table_needs_validate(self.ir, *tid) => {
                if collect_all {
                    out.push(format!("{ind}v._validateAllInto(checks, out);"));
                } else {
                    out.push(format!("{ind}v.validate(checks);"));
                }
            }
            Ty::List(elem) => {
                if let Ty::Table(tid) = &**elem
                    && table_needs_validate(self.ir, *tid)
                {
                    if collect_all {
                        out.push(format!("{ind}for (let i = 0; i < v.length; i++) {{"));
                        out.push(format!("{ind}  const start = out.length;"));
                        out.push(format!("{ind}  v.get(i)._validateAllInto(checks, out);"));
                        out.push(format!("{ind}  for (let k = start; k < out.length; k++) {{"));
                        out.push(format!("{ind}    if (out[k].index === undefined) out[k] = {{ ...out[k], index: i }};"));
                        out.push(format!("{ind}  }}"));
                        out.push(format!("{ind}}}"));
                    } else {
                        out.push(format!("{ind}for (let i = 0; i < v.length; i++) {{"));
                        out.push(format!("{ind}  try {{ v.get(i).validate(checks); }} catch (e) {{"));
                        out.push(format!(
                            "{ind}    if (e instanceof _graven.ValidationError && e.failure.index === undefined) throw new _graven.ValidationError({{ ...e.failure, index: i }});"
                        ));
                        out.push(format!("{ind}    throw e;"));
                        out.push(format!("{ind}  }}"));
                        out.push(format!("{ind}}}"));
                    }
                }
            }
            Ty::Union(uid) if self.field_needs_descend(&f.ty) => {
                let u = &self.ir.unions[uid.0];
                out.push(format!("{ind}switch (v.variant) {{"));
                for (vname, _, vtid) in &u.variants {
                    if table_needs_validate(self.ir, *vtid) {
                        let vlit = js_str(vname);
                        if collect_all {
                            out.push(format!("{ind}  case {vlit}: v.value._validateAllInto(checks, out); break;"));
                        } else {
                            out.push(format!("{ind}  case {vlit}: v.value.validate(checks); break;"));
                        }
                    }
                }
                out.push(format!("{ind}}}"));
            }
            _ => {}
        }
    }
}

fn range_bound_lit(p: Prim, b: &RangeBound) -> String {
    match (p, b) {
        (Prim::U64 | Prim::I64 | Prim::U128 | Prim::I128, RangeBound::Int(i)) => format!("{i}n"),
        (Prim::U64 | Prim::I64 | Prim::U128 | Prim::I128, RangeBound::UInt(u)) => format!("{u}n"),
        (_, RangeBound::Int(i)) => format!("{i}"),
        (_, RangeBound::UInt(u)) => format!("{u}"),
        (Prim::F32, RangeBound::Float(x)) => {
            let f = *x as f32;
            if f == 0.0 && f.is_sign_negative() { "-0".to_string() } else { format!("{f:?}") }
        }
        (Prim::F64, RangeBound::Float(x)) => {
            if *x == 0.0 && x.is_sign_negative() { "-0".to_string() } else { format!("{x:?}") }
        }
        _ => unreachable!("range_bound_lit mismatch"),
    }
}
