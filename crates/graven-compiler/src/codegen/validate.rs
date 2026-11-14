//! Validation codegen: `pub trait Checks`, `XVerified::validate`, `XVerified::validate_all`,
//! and `XW::to_bytes_validated`.

use std::collections::{BTreeMap, BTreeSet};

use super::*;

pub(super) fn has_any_rules(ir: &Ir) -> bool {
    ir.tables.iter().any(|t| !t.checks.is_empty() || t.fields.iter().any(|f| !f.rules.is_empty()))
}

fn table_needs_validate(ir: &Ir, tid: TableId) -> bool {
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

impl Cx<'_> {
    pub(super) fn checks_trait(&self) -> TokenStream {
        if !has_any_rules(self.ir) {
            return quote!();
        }
        enum Target<'a> {
            Table(TableId),
            Field(&'a Ty),
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
            {
                let seg = ns.rsplit('.').next().unwrap_or(ns);
                supers.insert(seg.to_string());
            }
        }
        let super_bounds: Vec<TokenStream> = supers
            .iter()
            .map(|seg| {
                let m = ident(seg);
                quote!(super::#m::Checks)
            })
            .collect();

        let method_sigs: Vec<TokenStream> = methods
            .iter()
            .map(|(name, target)| {
                let fn_id = ident(name);
                let arg_ty = match target {
                    Target::Table(tid) => {
                        let tv = self.table_path(*tid, "Verified");
                        quote!(&#tv<'_>)
                    }
                    Target::Field(ty) => self.check_arg_ty(ty),
                };
                quote! {
                    fn #fn_id(&self, v: #arg_ty) -> bool;
                }
            })
            .collect();

        let colon = if super_bounds.is_empty() { quote!() } else { quote!(: #(#super_bounds)+*) };
        let unit_impl = if method_sigs.is_empty() && super_bounds.is_empty() {
            quote! {
                impl Checks for () {}
            }
        } else {
            quote!()
        };

        quote! {
            /// Application-provided validation functions declared with `#[check(name)]`.
            pub trait Checks #colon {
                #(#method_sigs)*
            }
            #unit_impl
        }
    }

    fn check_arg_ty(&self, ty: &Ty) -> TokenStream {
        match ty {
            Ty::Str => quote!(&str),
            Ty::Bytes => quote!(&[u8]),
            Ty::Table(tid) => {
                let tv = self.table_path(*tid, "Verified");
                quote!(&#tv<'_>)
            }
            Ty::Union(uid) => {
                let uv = self.union_path(*uid, "Verified");
                quote!(&#uv<'_>)
            }
            Ty::List(elem) => match &**elem {
                Ty::Str => quote!(::graven::rt::VStrList<'_>),
                Ty::Bytes => quote!(::graven::StrList<'_>),
                Ty::Table(tid) => {
                    let ev = self.table_path(*tid, "Verified");
                    quote!(::graven::rt::VListR<'_, #ev<'_>>)
                }
                inline => {
                    let ety = self.inline_ty(inline);
                    quote!(::graven::ScalarList<'_, #ety>)
                }
            },
            inline => self.inline_ty(inline),
        }
    }

    pub(super) fn validate_methods(&self, t: &T<'_>) -> TokenStream {
        if !has_any_rules(self.ir) {
            return quote!();
        }
        let fail_fast = self.validate_body(t, false);
        let collect_all = self.validate_body(t, true);
        quote! {
            /// Checks the schema's validation rules on this message, stopping at the first failure.
            #[allow(clippy::collapsible_if, clippy::neg_cmp_op_on_partial_ord, clippy::manual_range_contains)]
            pub fn validate(&self, checks: &impl Checks) -> ::core::result::Result<(), ::graven::ValidateError> {
                let _ = checks;
                #fail_fast
                ::core::result::Result::Ok(())
            }

            /// Checks the schema's validation rules on this message, collecting every failure.
            pub fn validate_all(&self, checks: &impl Checks) -> ::core::result::Result<::graven::rt::Vec<::graven::Invalid>, ::graven::Error> {
                let mut out = ::graven::rt::Vec::new();
                self.validate_all_into(checks, &mut out)?;
                ::core::result::Result::Ok(out)
            }

            #[doc(hidden)]
            #[allow(clippy::collapsible_if, clippy::neg_cmp_op_on_partial_ord, clippy::manual_range_contains)]
            pub fn validate_all_into(
                &self,
                checks: &impl Checks,
                out: &mut ::graven::rt::Vec<::graven::Invalid>,
            ) -> ::core::result::Result<(), ::graven::Error> {
                // A table with only built-in rules does not call any check.
                let _ = checks;
                #collect_all
                ::core::result::Result::Ok(())
            }
        }
    }

    pub(super) fn to_bytes_validated(&self, t: &T<'_>) -> TokenStream {
        if !has_any_rules(self.ir) {
            return quote!();
        }
        let n = &t.name;
        let nw = format_ident!("{}W", t.def.name);
        quote! {
            /// Writes a message as the root, verifies it and runs validation rules before returning the bytes.
            ///
            /// # Panics
            /// If `b` was not created with `Builder::new_enveloped` ("builder mode
            /// mismatch"), or if sealed envelopes are pending (`Builder::take_frames`).
            #[inline]
            pub fn to_bytes_validated(
                b: &mut ::graven::Builder<'_>,
                checks: &impl Checks,
                f: impl ::core::ops::FnOnce(&mut #nw<'_>),
            ) -> ::core::result::Result<::graven::Bytes, ::graven::ValidateError> {
                f(&mut #nw::root(b));
                let bytes = b.flush_envelope(#n::SCHEMA_ID)?;
                #n::from_bytes(&bytes)?.validate(checks)?;
                ::core::result::Result::Ok(bytes)
            }
        }
    }

    fn validate_body(&self, t: &T<'_>, collect_all: bool) -> TokenStream {
        let tname = Literal::string(&t.def.name);
        let mut stmts = Vec::new();

        for f in &t.fields {
            let needs_field = !f.rules.is_empty() || self.field_needs_descend(f.ty);
            if !needs_field {
                continue;
            }
            let id = &f.id;
            let fname = Literal::string(f.name);
            let is_inline = matches!(f.ty, Ty::Bool | Ty::Scalar(_) | Ty::Enum(_) | Ty::Struct(_));
            let is_opt = matches!(f.presence, Presence::Optional);

            let read_expr = if is_inline { quote!(self.#id()) } else { quote!(self.#id()?) };
            let inner = self.validate_field_value(&tname, &fname, f, collect_all);

            if is_opt {
                stmts.push(quote! {
                    if let ::core::option::Option::Some(v) = #read_expr {
                        #inner
                    }
                });
            } else {
                stmts.push(quote! {
                    {
                        let v = #read_expr;
                        #inner
                    }
                });
            }
        }

        for c in &t.def.checks {
            let fn_id = ident(c);
            let cname = Literal::string(c);
            let err_expr = quote! {
                ::graven::Invalid {
                    table: #tname,
                    field: ::core::option::Option::None,
                    index: ::core::option::Option::None,
                    rule: ::graven::Rule::Check(#cname),
                }
            };
            if collect_all {
                stmts.push(quote! {
                    if !checks.#fn_id(self) {
                        out.push(#err_expr);
                    }
                });
            } else {
                stmts.push(quote! {
                    if !checks.#fn_id(self) {
                        return ::core::result::Result::Err(::graven::ValidateError::Invalid(#err_expr));
                    }
                });
            }
        }

        quote!(#(#stmts)*)
    }

    fn field_needs_descend(&self, ty: &Ty) -> bool {
        match ty {
            Ty::Table(tid) => table_needs_validate(self.ir, *tid),
            Ty::List(elem) => match &**elem {
                Ty::Table(tid) => table_needs_validate(self.ir, *tid),
                _ => false,
            },
            Ty::Union(uid) => self.ir.unions[uid.0].variants.iter().any(|(_, _, tid)| table_needs_validate(self.ir, *tid)),
            _ => false,
        }
    }

    fn validate_field_value(&self, tname: &Literal, fname: &Literal, f: &F<'_>, collect_all: bool) -> TokenStream {
        let mut stmts = Vec::new();
        let emit_err = |rule: TokenStream, index: TokenStream| {
            let inv = quote! {
                ::graven::Invalid {
                    table: #tname,
                    field: ::core::option::Option::Some(#fname),
                    index: #index,
                    rule: #rule,
                }
            };
            if collect_all {
                quote!(out.push(#inv);)
            } else {
                quote!(return ::core::result::Result::Err(::graven::ValidateError::Invalid(#inv));)
            }
        };

        if let Some((min, max)) = &f.rules.range {
            let Ty::Scalar(p) = f.ty else { unreachable!() };
            let is_float = p.is_float();
            let lo_cond = min.as_ref().map(|lo| {
                let lit = range_bound_lit(*p, lo);
                if is_float { quote!(!(v >= #lit)) } else { quote!(v < #lit) }
            });
            let hi_cond = max.as_ref().map(|hi| {
                let lit = range_bound_lit(*p, hi);
                if is_float { quote!(!(v <= #lit)) } else { quote!(v > #lit) }
            });
            let cond = match (lo_cond, hi_cond) {
                (Some(a), Some(b)) => quote!(#a || #b),
                (Some(a), None) | (None, Some(a)) => a,
                (None, None) => unreachable!(),
            };
            let fail = emit_err(quote!(::graven::Rule::Range), quote!(::core::option::Option::None));
            stmts.push(quote!(if #cond { #fail }));
        }

        if let Some((min, max)) = f.rules.len {
            let lo_cond = min.map(|lo| {
                let lo_lit = Literal::u64_unsuffixed(lo);
                quote!((v.len() as u64) < #lo_lit)
            });
            let hi_cond = max.map(|hi| {
                let hi_lit = Literal::u64_unsuffixed(hi);
                quote!((v.len() as u64) > #hi_lit)
            });
            let cond = match (lo_cond, hi_cond) {
                (Some(a), Some(b)) => quote!(#a || #b),
                (Some(a), None) | (None, Some(a)) => a,
                (None, None) => unreachable!(),
            };
            let fail = emit_err(quote!(::graven::Rule::Len), quote!(::core::option::Option::None));
            stmts.push(quote!(if #cond { #fail }));
        }

        if f.rules.finite {
            let fail = emit_err(quote!(::graven::Rule::Finite), quote!(::core::option::Option::None));
            stmts.push(quote!(if !v.is_finite() { #fail }));
        }

        if f.rules.known {
            match f.ty {
                Ty::Enum(eid) => {
                    let ep = self.inline_ty(&Ty::Enum(*eid));
                    let fail = emit_err(quote!(::graven::Rule::Known), quote!(::core::option::Option::None));
                    stmts.push(quote!(if ::core::matches!(v, #ep::Unknown(_)) { #fail }));
                }
                Ty::List(elem) if matches!(&**elem, Ty::Enum(_)) => {
                    let Ty::Enum(eid) = &**elem else { unreachable!() };
                    let ep = self.inline_ty(&Ty::Enum(*eid));
                    let fail = emit_err(quote!(::graven::Rule::Known), quote!(::core::option::Option::Some(i as u32)));
                    stmts.push(quote! {
                        for (i, elem) in v.iter().enumerate() {
                            if ::core::matches!(elem, #ep::Unknown(_)) {
                                #fail
                            }
                        }
                    });
                }
                _ => {}
            }
        }

        for c in &f.rules.checks {
            let fn_id = ident(c);
            let cname = Literal::string(c);
            let pass_ref = matches!(f.ty, Ty::Table(_) | Ty::Union(_));
            let arg = if pass_ref { quote!(&v) } else { quote!(v) };
            let fail = emit_err(quote!(::graven::Rule::Check(#cname)), quote!(::core::option::Option::None));
            stmts.push(quote!(if !checks.#fn_id(#arg) { #fail }));
        }

        match f.ty {
            Ty::Table(tid) if table_needs_validate(self.ir, *tid) => {
                if collect_all {
                    stmts.push(quote!(v.validate_all_into(checks, out)?;));
                } else {
                    stmts.push(quote!(v.validate(checks)?;));
                }
            }
            Ty::List(elem) => {
                if let Ty::Table(tid) = &**elem
                    && table_needs_validate(self.ir, *tid)
                {
                    if collect_all {
                        stmts.push(quote! {
                            for (i, elem) in v.iter().enumerate() {
                                let start = out.len();
                                elem?.validate_all_into(checks, out)?;
                                for inv in &mut out[start..] {
                                    if inv.index.is_none() {
                                        inv.index = ::core::option::Option::Some(i as u32);
                                    }
                                }
                            }
                        });
                    } else {
                        stmts.push(quote! {
                            for (i, elem) in v.iter().enumerate() {
                                elem?.validate(checks).map_err(|e| e.at(i))?;
                            }
                        });
                    }
                }
            }
            Ty::Union(uid) if self.field_needs_descend(f.ty) => {
                let u = &self.ir.unions[uid.0];
                let uv = self.union_path(*uid, "Verified");
                let arms: Vec<TokenStream> = u
                    .variants
                    .iter()
                    .map(|(vname, _, vtid)| {
                        let vid = ident(vname);
                        if table_needs_validate(self.ir, *vtid) {
                            if collect_all {
                                quote!(#uv::#vid(inner) => inner.validate_all_into(checks, out)?,)
                            } else {
                                quote!(#uv::#vid(inner) => inner.validate(checks)?,)
                            }
                        } else {
                            quote!(#uv::#vid(_) => {},)
                        }
                    })
                    .collect();
                stmts.push(quote! {
                    match v {
                        #(#arms)*
                        #uv::Unknown(_) => {},
                    }
                });
            }
            _ => {}
        }

        quote!(#(#stmts)*)
    }
}

fn range_bound_lit(p: Prim, b: &RangeBound) -> TokenStream {
    match (p, b) {
        (Prim::U8, RangeBound::Int(i)) => {
            let l = Literal::u8_suffixed(*i as u8);
            quote!(#l)
        }
        (Prim::U16, RangeBound::Int(i)) => {
            let l = Literal::u16_suffixed(*i as u16);
            quote!(#l)
        }
        (Prim::U32, RangeBound::Int(i)) => {
            let l = Literal::u32_suffixed(*i as u32);
            quote!(#l)
        }
        (Prim::U64, RangeBound::Int(i)) => {
            let l = Literal::u64_suffixed(*i as u64);
            quote!(#l)
        }
        (Prim::U128, RangeBound::UInt(u)) => {
            let l = Literal::u128_suffixed(*u);
            quote!(#l)
        }
        (Prim::I8, RangeBound::Int(i)) => {
            let l = Literal::i8_suffixed(*i as i8);
            quote!(#l)
        }
        (Prim::I16, RangeBound::Int(i)) => {
            let l = Literal::i16_suffixed(*i as i16);
            quote!(#l)
        }
        (Prim::I32, RangeBound::Int(i)) => {
            let l = Literal::i32_suffixed(*i as i32);
            quote!(#l)
        }
        (Prim::I64, RangeBound::Int(i)) => {
            let l = Literal::i64_suffixed(*i as i64);
            quote!(#l)
        }
        (Prim::I128, RangeBound::Int(i)) => {
            let l = Literal::i128_suffixed(*i);
            quote!(#l)
        }
        (Prim::F32, RangeBound::Float(x)) => {
            let l = Literal::f32_suffixed(*x as f32);
            quote!(#l)
        }
        (Prim::F64, RangeBound::Float(x)) => {
            let l = Literal::f64_suffixed(*x);
            quote!(#l)
        }
        _ => unreachable!("range_bound_lit mismatch"),
    }
}
