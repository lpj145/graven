//! Table reader (`X`), verified view (`XVerified`) and writer (`XW`).

use graven_layout::FieldKind;

use super::{JsCx, Out, escape_with, js_str, member_scope, p};
use crate::ir::*;

struct TField<'b> {
    f: &'b Field,
    slot: Option<u16>,
    bit: Option<(u16, u8)>,
    r_name: String,
    v_name: String,
    w_name: String,
    w_stream: Option<String>,
    w_unions: Vec<(String, u16, TableId)>,
}

impl<'a> JsCx<'a> {
    pub(super) fn emit_table(&self, out: &mut Out, tid: TableId) {
        let t = &self.ir.tables[tid.0];
        let name = self.top_ident(&t.name);
        let name_v = self.top_ident(&format!("{}Verified", t.name));
        let name_w = self.top_ident(&format!("{}W", t.name));
        let fixed_len = t.layout.fixed_len;

        let fields: Vec<_> = t
            .fields
            .iter()
            .zip(&t.layout.fields)
            .filter(|(f, _)| !matches!(f.presence, Presence::Reserved { .. }))
            .map(|(f, l)| (f, l.slot, l.presence))
            .collect();

        // Pointer slots, visited by the streaming list writer.
        let mut ptr_slots = Vec::new();
        for (f, slot, _) in &fields {
            match self.ir.field_kind(&f.ty) {
                FieldKind::Ptr => ptr_slots.push(slot.unwrap()),
                FieldKind::Union => ptr_slots.push(slot.unwrap() + 2),
                _ => {}
            }
        }

        // Member names already taken on each class.
        let mut r_used = member_scope(&["_ref", "verify", "_verifyContent"]);
        let mut v_used = member_scope(&["_ref", "reader", "validate", "validateAll", "_validateAllInto"]);
        let mut w_used = member_scope(&["_w"]);

        let tfields: Vec<TField<'_>> = fields
            .into_iter()
            .map(|(f, slot, bit)| {
                let r_name = escape_with(&f.name, &mut r_used);
                let v_name = escape_with(&f.name, &mut v_used);
                let mut w_name = String::new();
                let mut w_stream = None;
                let mut w_unions = Vec::new();
                match &f.ty {
                    Ty::Union(uid) => {
                        for (vname, tag, vtid) in &self.ir.unions[uid.0].variants {
                            let uname = escape_with(&format!("{}_{vname}", f.name), &mut w_used);
                            w_unions.push((uname, *tag, *vtid));
                        }
                    }
                    Ty::List(elem) if !matches!(&**elem, Ty::Str | Ty::Bytes) => {
                        w_name = escape_with(&f.name, &mut w_used);
                        w_stream = Some(escape_with(&format!("{}_streaming", f.name), &mut w_used));
                    }
                    _ => w_name = escape_with(&f.name, &mut w_used),
                }
                TField { f, slot, bit, r_name, v_name, w_name, w_stream, w_unions }
            })
            .collect();

        // ---- Reader `X` ----
        out.doc("", self.ir.doc(&t.name));
        out.js(&format!("export class {name} {{"));
        out.dts(&format!("export declare class {name} {{"));
        out.static_field("  ", "SCHEMA_ID", "bigint", &self.schema_id_lit(t));
        out.static_field("  ", "FIXED_LEN", "number", &fixed_len.to_string());
        let slots: Vec<String> = ptr_slots.iter().map(|s| s.to_string()).collect();
        out.static_field("  ", "PTR_SLOTS", "readonly number[]", &format!("[{}]", slots.join(", ")));
        out.private_static("  ", "_REQS", &self.reqs_literal(&tfields));
        out.private_static("  ", "_MASK", &self.mask_literal(&tfields, fixed_len));
        out.dts("  readonly _ref: _graven.TableRef;");
        out.dts("  private constructor();");
        out.js("  constructor(ref) { this._ref = ref; }");
        out.js(&format!(
            "  static fromRef = (ref) => {{ _graven.checkRequiredFast(ref, {name}._MASK, {name}._REQS); return new {name}(ref); }};"
        ));
        out.dts(&format!("  static fromRef: (ref: _graven.TableRef) => {name};"));
        out.method("  ", true, "read", &[p("buf", "Uint8Array")], &name, &[format!(
            "return {name}.fromRef(_graven.TableRef.at(buf, 0));"
        )]);
        out.method("  ", true, "verified", &[p("buf", "Uint8Array")], &name_v, &[format!(
            "return {name}.verifiedWith(buf, new _graven.VerifyCtx(buf.length));"
        )]);
        out.method(
            "  ",
            true,
            "verifiedWith",
            &[p("buf", "Uint8Array"), p("ctx", "_graven.VerifyCtx")],
            &name_v,
            &[format!("const r = {name}.read(buf);"), "r.verify(ctx);".into(), format!("return {name_v}._fromVerified(r._ref);")],
        );
        out.method("  ", true, "fromBytes", &[p("buf", "Uint8Array")], &name_v, &[format!(
            "return {name}.verified(_graven.openEnvelope(buf, {name}.SCHEMA_ID));"
        )]);
        for tf in &tfields {
            out.doc("  ", self.ir.doc(&format!("{}.{}", t.name, tf.f.name)));
            self.emit_accessor(out, tf.f, &tf.r_name, tf.slot, tf.bit, false);
        }
        // Leaf data first, then the fields that descend.
        let mut body = vec!["ctx.enter();".to_string()];
        let (leaf, descend): (Vec<&TField<'_>>, Vec<&TField<'_>>) = tfields.iter().partition(|tf| !descends(&tf.f.ty));
        for tf in leaf.into_iter().chain(descend) {
            body.extend(self.verify_stmt(tf.f, tf.slot));
        }
        body.push("ctx.leave();".into());
        out.method("  ", false, "_verifyContent", &[p("ctx", "_graven.VerifyCtx")], "void", &body);
        out.method(
            "  ",
            false,
            "verify",
            &[p("ctx", "_graven.VerifyCtx")],
            "void",
            &["ctx.charge(2 + this._ref.fixedLen);".into(), "this._verifyContent(ctx);".into()],
        );
        out.both("}");
        out.blank();

        // ---- Verified view `XVerified` ----
        out.doc("", self.ir.doc(&t.name));
        out.js(&format!("export class {name_v} {{"));
        out.dts(&format!("export declare class {name_v} {{"));
        out.dts("  readonly _ref: _graven.TableRef;");
        out.dts("  private constructor();");
        out.js("  constructor(ref) { this._ref = ref; }");
        out.js(&format!("  static _fromVerified = (ref) => new {name_v}(ref);"));
        out.dts(&format!("  static _fromVerified: (ref: _graven.TableRef) => {name_v};"));
        out.method("  ", false, "reader", &[], &name, &[format!("return {name}.fromRef(this._ref);")]);
        for tf in &tfields {
            out.doc("  ", self.ir.doc(&format!("{}.{}", t.name, tf.f.name)));
            self.emit_accessor(out, tf.f, &tf.v_name, tf.slot, tf.bit, true);
        }
        let val_fields: Vec<(&Field, &str)> = tfields.iter().map(|tf| (tf.f, tf.v_name.as_str())).collect();
        self.emit_validate_methods(out, t, &val_fields);
        out.both("}");
        out.blank();

        // ---- Writer `XW` ----
        out.doc("", self.ir.doc(&t.name));
        out.js(&format!("export class {name_w} {{"));
        out.dts(&format!("export declare class {name_w} {{"));
        out.dts("  readonly _w: _graven.TableW;");
        out.dts("  constructor(w: _graven.TableW);");
        out.js("  constructor(w) { this._w = w; }");
        let wfn = format!("(w: {name_w}) => void");
        out.method("  ", true, "root", &[p("b", "_graven.Builder")], &name_w, &[format!(
            "return new {name_w}(b.root({fixed_len}));"
        )]);
        out.method(
            "  ",
            true,
            "toBytes",
            &[p("fn", wfn.clone())],
            "Uint8Array",
            &[format!(
                "return _graven.withBuilder((b) => {{ fn({name_w}.root(b)); return b.finishEnvelope({name}.SCHEMA_ID); }});"
            )],
        );
        out.method(
            "  ",
            true,
            "toBytesWith",
            &[p("b", "_graven.Builder"), p("fn", wfn.clone())],
            "Uint8Array",
            &[format!("fn({name_w}.root(b));"), format!("return b.finishEnvelope({name}.SCHEMA_ID);")],
        );
        out.method(
            "  ",
            true,
            "seal",
            &[p("b", "_graven.Builder"), p("fn", wfn)],
            "void",
            &[format!("fn({name_w}.root(b));"), format!("b.sealEnvelope({name}.SCHEMA_ID);")],
        );
        self.emit_to_bytes_validated(out, &name, &name_w);
        for tf in &tfields {
            self.emit_setter(out, t, tf);
        }
        out.both("}");
        out.blank();

        let upper_sid = format!("{}_SCHEMA_ID", super::upper_snake(&t.name));
        out.js(&format!("export const {upper_sid} = {name}.SCHEMA_ID;"));
        out.dts(&format!("export declare const {upper_sid}: bigint;"));
        out.blank();
    }

    /// The required fields, checked when a reader is created.
    fn reqs_literal(&self, tfields: &[TField<'_>]) -> String {
        let mut s = String::from("[\n");
        for tf in tfields {
            let req = matches!(tf.f.presence, Presence::Required);
            let fname = js_str(&tf.f.name);
            let line = match self.ir.field_kind(&tf.f.ty) {
                FieldKind::Scalar { size } => {
                    let off = tf.slot.unwrap();
                    if req {
                        let (byte, shift) = tf.bit.unwrap();
                        format!("{{ kind: \"scalar\", name: {fname}, off: {off}, size: {size}, bit: {{ byte: {byte}, shift: {shift} }} }}")
                    } else {
                        format!("{{ kind: \"slot\", off: {off}, size: {size} }}")
                    }
                }
                FieldKind::Bool if req => {
                    let (byte, shift) = tf.bit.unwrap();
                    format!("{{ kind: \"bool\", name: {fname}, bit: {{ byte: {byte}, shift: {shift} }} }}")
                }
                FieldKind::Ptr if req => format!("{{ kind: \"ptr\", name: {fname}, off: {} }}", tf.slot.unwrap()),
                FieldKind::Union if req => format!("{{ kind: \"union\", name: {fname}, off: {} }}", tf.slot.unwrap()),
                _ => continue,
            };
            s.push_str(&format!("    {line},\n"));
        }
        s.push_str("  ]");
        s
    }

    fn mask_literal(&self, tfields: &[TField<'_>], fixed_len: u16) -> String {
        let mut req_bytes: std::collections::BTreeMap<u16, u8> = std::collections::BTreeMap::new();
        let mut req_ptrs = Vec::new();
        let mut req_unions = Vec::new();
        for tf in tfields {
            if !matches!(tf.f.presence, Presence::Required) {
                continue;
            }
            if let Some((b, s)) = tf.bit {
                *req_bytes.entry(b).or_insert(0) |= 1u8 << s;
            }
            match self.ir.field_kind(&tf.f.ty) {
                FieldKind::Ptr => req_ptrs.push(tf.slot.unwrap().to_string()),
                FieldKind::Union => req_unions.push(tf.slot.unwrap().to_string()),
                _ => {}
            }
        }
        let bits: Vec<String> = req_bytes.iter().map(|(b, m)| format!("[{b}, {m}]")).collect();
        format!(
            "{{ len: {fixed_len}, bits: [{}], ptrs: [{}], unions: [{}] }}",
            bits.join(", "),
            req_ptrs.join(", "),
            req_unions.join(", ")
        )
    }

    fn emit_accessor(&self, out: &mut Out, f: &Field, mname: &str, slot: Option<u16>, bit: Option<(u16, u8)>, verified: bool) {
        let req = matches!(f.presence, Presence::Required);
        let opt = matches!(f.presence, Presence::Optional);
        let fname = js_str(&f.name);
        let mut one = |ret: String, body: String| out.method("  ", false, mname, &[], &ret, &[body]);
        match &f.ty {
            Ty::Bool => {
                let (byte, shift) = bit.unwrap();
                let get = format!("this._ref.bool({{ byte: {byte}, shift: {shift} }})");
                match &f.presence {
                    Presence::Optional => one("boolean | undefined".into(), format!("return {get};")),
                    Presence::Default(v) => one("boolean".into(), format!("return {get} ?? {};", self.default_lit(&f.ty, v))),
                    _ => one("boolean".into(), format!("return {get} ?? false;")),
                }
            }
            Ty::Scalar(_) | Ty::Enum(_) | Ty::Struct(_) => {
                let off = slot.unwrap();
                let (byte, shift) = bit.unwrap();
                let rty = self.inline_ty(&f.ty);
                let codec = self.codec(&f.ty);
                match &f.presence {
                    Presence::Optional => one(
                        format!("{rty} | undefined"),
                        format!("return this._ref.scalar({off}, {{ byte: {byte}, shift: {shift} }}, {codec});"),
                    ),
                    Presence::Default(v) => one(
                        rty,
                        format!(
                            "return this._ref.scalarOr({off}, {{ byte: {byte}, shift: {shift} }}, {codec}, {});",
                            self.default_lit(&f.ty, v)
                        ),
                    ),
                    _ => one(rty, format!("return this._ref.raw({off}, {codec}, {});", self.fallback(&f.ty))),
                }
            }
            Ty::Str => {
                let off = slot.unwrap();
                let call = if verified { "strUnchecked" } else { "str" };
                if req {
                    one("string".into(), format!("return this._ref.{call}({off}) ?? \"\";"));
                } else {
                    one("string | undefined".into(), format!("return this._ref.{call}({off});"));
                }
            }
            Ty::Bytes => {
                let off = slot.unwrap();
                if req {
                    one("Uint8Array".into(), format!("return this._ref.bytes({off}) ?? _graven.EMPTY_BYTES;"));
                } else {
                    one("Uint8Array | undefined".into(), format!("return this._ref.bytes({off});"));
                }
            }
            Ty::List(elem) => {
                let off = slot.unwrap();
                match &**elem {
                    Ty::Str if verified => {
                        if req {
                            one("_graven.VStrList".into(), format!("return _graven.reqVStrs(this._ref.strs({off}), {fname});"))
                        } else {
                            one("_graven.VStrList | undefined".into(), format!("return _graven.optVStrs(this._ref.strs({off}));"))
                        }
                    }
                    Ty::Str => {
                        if req {
                            one("_graven.StrList".into(), format!("return _graven.reqPtr(this._ref.strs({off}), {fname});"))
                        } else {
                            one("_graven.StrList | undefined".into(), format!("return this._ref.strs({off});"))
                        }
                    }
                    Ty::Bytes => {
                        if req {
                            one("_graven.BytesList".into(), format!("return _graven.reqPtr(this._ref.bytesList({off}), {fname});"))
                        } else {
                            one("_graven.BytesList | undefined".into(), format!("return this._ref.bytesList({off});"))
                        }
                    }
                    Ty::Table(ctid) => {
                        let cty = self.table_path(*ctid, if verified { "Verified" } else { "" });
                        let wrap = if verified { format!("{cty}._fromVerified") } else { format!("{cty}.fromRef") };
                        if req {
                            one(
                                format!("_graven.TableViewList<{cty}>"),
                                format!("return new _graven.TableViewList(this._ref.tables({off}), {wrap});"),
                            )
                        } else {
                            one(
                                format!("_graven.TableViewList<{cty}> | undefined"),
                                format!("const l = this._ref.tables({off}); return l === undefined ? undefined : new _graven.TableViewList(l, {wrap});"),
                            )
                        }
                    }
                    inline => {
                        let ety = self.inline_ty(inline);
                        let codec = self.codec(inline);
                        if req {
                            one(
                                format!("_graven.ScalarList<{ety}>"),
                                format!("return _graven.reqPtr(this._ref.scalars({off}, {codec}), {fname});"),
                            )
                        } else {
                            one(format!("_graven.ScalarList<{ety}> | undefined"), format!("return this._ref.scalars({off}, {codec});"))
                        }
                    }
                }
            }
            Ty::Table(ctid) => {
                let off = slot.unwrap();
                let cty = self.table_path(*ctid, if verified { "Verified" } else { "" });
                let wrap = if verified { format!("{cty}._fromVerified") } else { format!("{cty}.fromRef") };
                if opt {
                    one(
                        format!("{cty} | undefined"),
                        format!("const t = this._ref.table({off}); return t === undefined ? undefined : {wrap}(t);"),
                    )
                } else {
                    one(cty, format!("return {wrap}(_graven.reqPtr(this._ref.table({off}), {fname}));"))
                }
            }
            Ty::Union(uid) => {
                let off = slot.unwrap();
                let u = &self.ir.unions[uid.0];
                let suffix = if verified { "Verified" } else { "" };
                let mut ret = self.union_path(*uid, suffix);
                if !req {
                    ret.push_str(" | undefined");
                }
                let mut body = vec![format!("const u = this._ref.union({off});")];
                if req {
                    body.push(format!("if (u === undefined) throw new _graven.GravenError(\"Missing\", {{ field: {fname} }});"));
                } else {
                    body.push("if (u === undefined) return undefined;".into());
                }
                body.push("switch (u.tag) {".into());
                for (vname, tag, vtid) in &u.variants {
                    let cty = self.table_path(*vtid, suffix);
                    let wrap = if verified { format!("{cty}._fromVerified") } else { format!("{cty}.fromRef") };
                    body.push(format!("  case {tag}: return {{ tag: {tag}, variant: {}, value: {wrap}(u.table) }};", js_str(vname)));
                }
                body.push("  default: return { tag: u.tag, variant: \"Unknown\" };".into());
                body.push("}".into());
                out.method("  ", false, mname, &[], &ret, &body);
            }
        }
    }

    /// The statements `_verifyContent` runs for one field.
    fn verify_stmt(&self, f: &Field, slot: Option<u16>) -> Vec<String> {
        let Some(off) = slot else { return Vec::new() };
        match &f.ty {
            Ty::Bool | Ty::Scalar(_) | Ty::Enum(_) | Ty::Struct(_) => Vec::new(),
            Ty::Str => vec![format!("this._ref.verifyStr({off}, ctx);")],
            Ty::Bytes => vec![format!("this._ref.verifyBytes({off}, ctx);")],
            Ty::List(elem) => match &**elem {
                Ty::Str => vec![format!("_graven.verifyStrs(this._ref.strs({off}), ctx);")],
                Ty::Bytes => vec![format!("_graven.verifyBytesList(this._ref.strs({off}), ctx);")],
                Ty::Table(ctid) => {
                    vec![format!("_graven.verifyTableList(this._ref.tables({off}), {}.fromRef, ctx);", self.table_path(*ctid, ""))]
                }
                inline => {
                    let per = self.ir.inline_size(inline).unwrap().max(1);
                    vec![format!("this._ref.verifyScalars({off}, {per}, ctx);")]
                }
            },
            Ty::Table(ctid) => {
                vec![format!("{{ const t = this._ref.table({off}); if (t !== undefined) {}.fromRef(t).verify(ctx); }}", self.table_path(*ctid, ""))]
            }
            Ty::Union(uid) => {
                let u = &self.ir.unions[uid.0];
                if u.variants.is_empty() {
                    return Vec::new();
                }
                let mut v = vec![format!("{{ const u = this._ref.union({off}); if (u !== undefined) {{ switch (u.tag) {{")];
                for (_, tag, vtid) in &u.variants {
                    v.push(format!("  case {tag}: {}.fromRef(u.table).verify(ctx); break;", self.table_path(*vtid, "")));
                }
                v.push("} } }".into());
                v
            }
        }
    }

    fn emit_setter(&self, out: &mut Out, t: &Table, tf: &TField<'_>) {
        let f = tf.f;
        let w_name = tf.w_name.as_str();
        let doc = self.ir.doc(&format!("{}.{}", t.name, f.name));
        let set = |out: &mut Out, ty: String, stmt: String| {
            out.doc("  ", doc);
            out.method("  ", false, w_name, &[p("v", ty)], "this", &[format!("{stmt} return this;")]);
        };
        match &f.ty {
            Ty::Bool => {
                let (byte, shift) = tf.bit.unwrap();
                set(out, "boolean".into(), format!("this._w.setBool({{ byte: {byte}, shift: {shift} }}, v);"));
            }
            Ty::Scalar(_) | Ty::Enum(_) | Ty::Struct(_) => {
                let off = tf.slot.unwrap();
                let (byte, shift) = tf.bit.unwrap();
                set(
                    out,
                    self.inline_ty(&f.ty),
                    format!("this._w.set({off}, {{ byte: {byte}, shift: {shift} }}, {}, v);", self.codec(&f.ty)),
                );
            }
            Ty::Str => set(out, "string".into(), format!("this._w.setStr({}, v);", tf.slot.unwrap())),
            Ty::Bytes => set(out, "Uint8Array".into(), format!("this._w.setBytes({}, v);", tf.slot.unwrap())),
            Ty::List(elem) => {
                let off = tf.slot.unwrap();
                match &**elem {
                    Ty::Str => set(out, "Iterable<string>".into(), format!("this._w.setStrs({off}, v);")),
                    Ty::Bytes => set(out, "Iterable<Uint8Array>".into(), format!("this._w.setBytesList({off}, v);")),
                    Ty::Table(ctid) => {
                        let cw = self.table_path(*ctid, "W");
                        let cty = self.table_path(*ctid, "");
                        out.doc("  ", doc);
                        out.method(
                            "  ",
                            false,
                            w_name,
                            &[p("count", "number")],
                            &format!("_graven.ListW<{cw}>"),
                            &[format!("return new _graven.ListW(this._w.tables({off}, count, {cty}.FIXED_LEN), (w) => new {cw}(w));")],
                        );
                        if let Some(sn) = &tf.w_stream {
                            out.method(
                                "  ",
                                false,
                                sn,
                                &[],
                                &format!("_graven.StreamW<{cw}>"),
                                &[format!(
                                    "return new _graven.StreamW(this._w.tablesStreaming({off}, {cty}.FIXED_LEN, {cty}.PTR_SLOTS), (w) => new {cw}(w));"
                                )],
                            );
                        }
                    }
                    inline => {
                        let ety = self.inline_ty(inline);
                        let codec = self.codec(inline);
                        set(out, format!("Iterable<{ety}>"), format!("this._w.setScalars({off}, {codec}, v);"));
                        if let Some(sn) = &tf.w_stream {
                            out.method(
                                "  ",
                                false,
                                sn,
                                &[],
                                &format!("_graven.ScalarListW<{ety}>"),
                                &[format!("return this._w.scalarsStreaming({off}, {codec});")],
                            );
                        }
                    }
                }
            }
            Ty::Table(ctid) => {
                let cw = self.table_path(*ctid, "W");
                let cty = self.table_path(*ctid, "");
                out.doc("  ", doc);
                out.method("  ", false, w_name, &[], &cw, &[format!(
                    "return new {cw}(this._w.table({}, {cty}.FIXED_LEN));",
                    tf.slot.unwrap()
                )]);
            }
            Ty::Union(_) => {
                let off = tf.slot.unwrap();
                for (uname, tag, vtid) in &tf.w_unions {
                    let cw = self.table_path(*vtid, "W");
                    let cty = self.table_path(*vtid, "");
                    out.method("  ", false, uname, &[], &cw, &[format!(
                        "return new {cw}(this._w.union({off}, {tag}, {cty}.FIXED_LEN));"
                    )]);
                }
            }
        }
    }
}

fn descends(ty: &Ty) -> bool {
    match ty {
        Ty::Table(_) | Ty::Union(_) => true,
        Ty::List(elem) => matches!(&**elem, Ty::Table(_)),
        _ => false,
    }
}
