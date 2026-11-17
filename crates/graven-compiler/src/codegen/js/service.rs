//! RPC client codegen (`--side client`).

use std::collections::BTreeMap;

use crate::ir::*;

use super::{JsCx, Out, escape_with, js_str, member_scope, p, p_opt, upper_snake};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum RpcPayloadId {
    Table(TableId),
    Union(UnionId),
}

impl<'a> JsCx<'a> {
    pub(super) fn emit_services(&self, out: &mut Out, opts: super::super::GenOptions) {
        if !opts.client {
            return;
        }

        for u in self.ir.unions.iter().filter(|u| u.origin.is_none()) {
            self.emit_union_writer(out, u);
        }

        if self.ir.services.is_empty() {
            return;
        }

        // One codec per payload type used by the client methods.
        let mut codecs: BTreeMap<RpcPayloadId, String> = BTreeMap::new();
        for svc in &self.ir.services {
            for m in svc.methods.iter().filter(|m| m.name.is_some()) {
                for ty in [&m.input, &m.output, &m.error].into_iter().flatten() {
                    let pid = payload_id(ty);
                    if !codecs.contains_key(&pid) {
                        let cname = format!("_rpcCodec_{}", codecs.len());
                        codecs.insert(pid, cname);
                    }
                }
            }
        }

        for (pid, cname) in &codecs {
            self.emit_rpc_codec(out, *pid, cname);
        }

        for svc in &self.ir.services {
            self.emit_service_common(out, svc);
            self.emit_service_client(out, svc, &codecs);
        }
    }

    fn emit_union_writer(&self, out: &mut Out, u: &Union) {
        let uw = self.top_ident(&format!("{}W", u.name));
        let mut used = member_scope(&["_b", "_chosen"]);

        out.doc("", Some(&format!(
            "Writer for union `{}` in RPC calls and replies.\n\nThe function given to a client method must choose exactly one variant\n(`w.<variant>()`); choosing none throws.",
            u.name
        )));
        out.js(&format!("export class {uw} {{"));
        out.dts(&format!("export declare class {uw} {{"));
        out.dts("  private readonly _b;");
        out.dts("  _chosen?: { readonly tag: number };");
        out.dts("  constructor(b: _graven.Builder);");
        out.js("  constructor(b) { this._b = b; this._chosen = undefined; }");
        for (vname, tag, tid) in &u.variants {
            let mname = escape_with(vname, &mut used);
            let vtw = self.table_path(*tid, "W");
            out.method(
                "  ",
                false,
                &mname,
                &[],
                &vtw,
                &[format!("this._chosen = {{ tag: {tag} }};"), format!("return {vtw}.root(this._b);")],
            );
        }
        out.both("}");
        out.blank();
    }

    /// Codecs are private to the module: only `.js` has them.
    fn emit_rpc_codec(&self, out: &mut Out, pid: RpcPayloadId, cname: &str) {
        match pid {
            RpcPayloadId::Table(tid) => {
                let tr = self.table_path(tid, "");
                let tw = self.table_path(tid, "W");
                out.js(&format!("const {cname} = {{"));
                out.js("  encode(fn) {");
                out.js("    const b = new _graven.Builder();");
                out.js(&format!("    fn({tw}.root(b));"));
                out.js("    return { variant: 0, payload: b.finish() };");
                out.js("  },");
                out.js("  encodeTo(b, fn) {");
                out.js(&format!("    fn({tw}.root(b));"));
                out.js("    return { variant: 0 };");
                out.js("  },");
                out.js("  verify(variant, payload) {");
                out.js(&format!("    return _graven.rpcVerifyTable(variant, payload, (buf) => {tr}.verified(buf));"));
                out.js("  },");
                out.js("};");
                out.js("");
            }
            RpcPayloadId::Union(uid) => {
                let u = &self.ir.unions[uid.0];
                let uw = self.union_path(uid, "W");
                out.js(&format!("const {cname} = {{"));
                out.js("  encode(fn) {");
                out.js("    const b = new _graven.Builder();");
                out.js(&format!("    const w = new {uw}(b);"));
                out.js("    fn(w);");
                out.js("    if (w._chosen === undefined) {");
                out.js("      throw new Error(\"union writer function must select a variant\");");
                out.js("    }");
                out.js("    return { variant: w._chosen.tag, payload: b.finish() };");
                out.js("  },");
                out.js("  encodeTo(b, fn) {");
                out.js(&format!("    const w = new {uw}(b);"));
                out.js("    fn(w);");
                out.js("    if (w._chosen === undefined) {");
                out.js("      throw new Error(\"union writer function must select a variant\");");
                out.js("    }");
                out.js("    return { variant: w._chosen.tag };");
                out.js("  },");
                out.js("  verify(variant, payload) {");
                out.js("    if (variant === 0) throw new _graven.RpcProtocolError(\"MissingVariant\");");
                out.js("    switch (variant) {");
                for (vname, tag, tid) in &u.variants {
                    let vt = self.table_path(*tid, "");
                    out.js(&format!(
                        "      case {tag}: return {{ tag: {tag}, variant: {}, value: _graven.rpcVerifyTable(0, payload, (buf) => {vt}.verified(buf)) }};",
                        js_str(vname)
                    ));
                }
                out.js("      default: return { tag: variant, variant: \"Unknown\" };");
                out.js("    }");
                out.js("  },");
                out.js("};");
                out.js("");
            }
        }
    }

    fn service_id_hex(&self, svc: &Service) -> &str {
        self.lock
            .services
            .iter()
            .find(|l| l.name == svc.name)
            .map(|l| l.service_id.as_str())
            .unwrap_or_else(|| panic!("lock lacks service `{}` (run `gravenc update`)", svc.name))
    }

    fn rpc_writer(&self, ty: &Ty) -> String {
        match ty {
            Ty::Table(tid) => self.table_path(*tid, "W"),
            Ty::Union(uid) => self.union_path(*uid, "W"),
            _ => unreachable!("service payload must be table or union"),
        }
    }

    fn rpc_verified(&self, ty: &Ty) -> String {
        match ty {
            Ty::Table(tid) => self.table_path(*tid, "Verified"),
            Ty::Union(uid) => self.union_path(*uid, "Verified"),
            _ => unreachable!("service payload must be table or union"),
        }
    }

    fn emit_service_common(&self, out: &mut Out, svc: &Service) {
        let sid_const = self.top_ident(&format!("{}_SERVICE_ID", upper_snake(&svc.name)));
        out.both(&format!("/** Service id of `{}.{}` (RPC frame header). */", self.ir.namespace, svc.name));
        out.js(&format!("export const {sid_const} = {}n;", self.service_id_hex(svc)));
        out.dts(&format!("export declare const {sid_const}: bigint;"));
        let tname = self.top_ident(&format!("{}_timeouts", super::super::snake(&svc.name)));
        out.both("/** Declared method timeouts in milliseconds. */");
        out.js(&format!("export const {tname} = Object.freeze({{"));
        out.dts(&format!("export declare const {tname}: {{"));
        for m in &svc.methods {
            let (Some(mname), Some(ms)) = (m.name.as_deref(), m.timeout_ms) else { continue };
            let cname = format!("{}_MS", upper_snake(mname));
            out.js(&format!("  {cname}: {ms},"));
            out.dts(&format!("  readonly {cname}: {ms};"));
        }
        out.js("});");
        out.dts("};");
        out.blank();
    }

    fn emit_service_client(&self, out: &mut Out, svc: &Service, codecs: &BTreeMap<RpcPayloadId, String>) {
        let cname = self.top_ident(&format!("{}Client", svc.name));
        let sid_const = self.top_ident(&format!("{}_SERVICE_ID", upper_snake(&svc.name)));
        out.doc("", self.ir.doc(&svc.name));
        out.both(&format!("/** Typed RPC client generated for service `{}`. */", svc.name));
        out.js(&format!("export class {cname} {{"));
        out.dts(&format!("export declare class {cname} {{"));
        out.dts("  private readonly _send;");
        out.dts("  constructor(send: _graven.Sender);");
        out.js("  constructor(send) { this._send = send; }");

        let mut used = member_scope(&["_send"]);
        let codec = |ty: &Ty| codecs.get(&payload_id(ty)).expect("codec registered for service payload").as_str();

        for m in &svc.methods {
            let (Some(mname), Some(shape), Some(in_ty)) = (m.name.as_deref(), m.shape, m.input.as_ref()) else { continue };
            let js_name = escape_with(mname, &mut used);
            let in_codec = codec(in_ty);
            let def_timeout = m.timeout_ms.map_or("undefined".to_string(), |ms| ms.to_string());
            let params = [p("fn", format!("(w: {}) => void", self.rpc_writer(in_ty))), p_opt("opts", "_graven.CallOptions")];
            let id = m.id;

            let (ret, call) = match (shape, m.output.as_ref(), m.error.as_ref()) {
                (MethodShape::OneWay, _, _) => {
                    ("Promise<void>".to_string(), format!("_graven.rpcOneWay(this._send, {in_codec}, {sid_const}, {id}, fn, opts)"))
                }
                (MethodShape::Unary, Some(o), None) => (
                    format!("Promise<{}>", self.rpc_verified(o)),
                    format!("_graven.rpcUnary(this._send, {in_codec}, {}, {sid_const}, {id}, {def_timeout}, fn, opts)", codec(o)),
                ),
                (MethodShape::Unary, Some(o), Some(e)) => (
                    format!("Promise<_graven.RpcResult<{}, {}>>", self.rpc_verified(o), self.rpc_verified(e)),
                    format!(
                        "_graven.rpcUnaryResult(this._send, {in_codec}, {}, {}, {sid_const}, {id}, {def_timeout}, fn, opts)",
                        codec(o),
                        codec(e)
                    ),
                ),
                (MethodShape::Stream, Some(o), None) => (
                    format!("Promise<_graven.ItemStream<{}>>", self.rpc_verified(o)),
                    format!("_graven.rpcStream(this._send, {in_codec}, {}, {sid_const}, {id}, {def_timeout}, fn, opts)", codec(o)),
                ),
                (MethodShape::Stream, Some(o), Some(e)) => (
                    format!("Promise<_graven.RpcResult<_graven.ItemStream<{}>, {}>>", self.rpc_verified(o), self.rpc_verified(e)),
                    format!(
                        "_graven.rpcStreamResult(this._send, {in_codec}, {}, {}, {sid_const}, {id}, {def_timeout}, fn, opts)",
                        codec(o),
                        codec(e)
                    ),
                ),
                (_, None, _) => unreachable!("unary and stream methods have an output"),
            };
            out.blank();
            out.doc("  ", self.ir.doc(&format!("{}.{mname}", svc.name)));
            out.method("  ", false, &js_name, &params, &ret, &[format!("return {call};")]);
        }
        out.both("}");
        out.blank();
    }
}

fn payload_id(ty: &Ty) -> RpcPayloadId {
    match ty {
        Ty::Table(tid) => RpcPayloadId::Table(*tid),
        Ty::Union(uid) => RpcPayloadId::Union(*uid),
        _ => unreachable!("service payload must be table or union"),
    }
}
