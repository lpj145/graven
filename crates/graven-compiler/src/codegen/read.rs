//! Layout consts and the checked reader `X<'a>` (`FromTable`, `REQ`/`MASK`, accessors,
//! `verify`, `read`/`verified`/`from_bytes`).

use super::*;

impl Cx<'_> {
    /// `pub mod <table> { FIXED_LEN, <FIELD>, <FIELD>_BIT, PTR_SLOTS }`. Reserved fields
    /// generate nothing (their slot stays in the layout but is never read or written).
    pub(super) fn layout_mod(&self, t: &T<'_>) -> TokenStream {
        let m = &t.m;
        let fixed = lit_u16(t.def.layout.fixed_len);
        let mut consts = Vec::new();
        let mut ptrs = Vec::new();
        for f in &t.fields {
            if let (Some(off), Some(slot)) = (&f.off, f.slot) {
                let s = lit_u16(slot);
                consts.push(quote!(pub const #off: u16 = #s;));
            }
            if let (Some(bc), Some((byte, shift))) = (&f.bitc, f.bit) {
                let (b, s) = (lit_u16(byte), lit_u8(shift));
                consts.push(quote!(pub const #bc: ::graven::Bit = ::graven::Bit { byte: #b, shift: #s };));
            }
            match kind(self.ir, f.ty) {
                FieldKind::Ptr => {
                    let off = f.off.as_ref().unwrap();
                    ptrs.push(quote!(#off));
                }
                FieldKind::Union => {
                    let off = f.off.as_ref().unwrap();
                    ptrs.push(quote!(#off + 2));
                }
                _ => {}
            }
        }
        let doc = format!(" Layout of the fixed part of `{}` (spec §3.3).", t.def.name);
        quote! {
            #[doc = #doc]
            // `pub mod bank { include!(…) }` + table `Bank` ⇒ `bank::bank`: unavoidable in the
            // documented pattern, and the client cannot edit the generated code.
            #[allow(clippy::module_inception)]
            pub mod #m {
                pub const FIXED_LEN: u16 = #fixed;
                #(#consts)*
                /// Offset of each `i32` pointer of the fixed part (for streaming lists).
                pub const PTR_SLOTS: &[u16] = &[#(#ptrs),*];
            }
        }
    }

    /// `Req` entries in ID order (spec §4.3; same order as the conformance interpreter).
    fn reqs(&self, t: &T<'_>) -> Vec<TokenStream> {
        let m = &t.m;
        let mut v = Vec::new();
        for f in &t.fields {
            let req = matches!(f.presence, Presence::Required);
            let name = f.name;
            match kind(self.ir, f.ty) {
                FieldKind::Scalar { size } => {
                    let off = f.off.as_ref().unwrap();
                    let size = lit_u16(size);
                    if req {
                        let bc = f.bitc.as_ref().unwrap();
                        v.push(quote!(::graven::rt::Req::Scalar { name: #name, off: #m::#off, size: #size, bit: #m::#bc }));
                    } else {
                        v.push(quote!(::graven::rt::Req::Slot { off: #m::#off, size: #size }));
                    }
                }
                FieldKind::Bool if req => {
                    let bc = f.bitc.as_ref().unwrap();
                    v.push(quote!(::graven::rt::Req::Bool { name: #name, bit: #m::#bc }));
                }
                FieldKind::Ptr if req => {
                    let off = f.off.as_ref().unwrap();
                    v.push(quote!(::graven::rt::Req::Ptr { name: #name, off: #m::#off }));
                }
                FieldKind::Union if req => {
                    let off = f.off.as_ref().unwrap();
                    v.push(quote!(::graven::rt::Req::Union { name: #name, off: #m::#off }));
                }
                _ => {}
            }
        }
        v
    }

    pub(super) fn reader(&self, t: &T<'_>) -> TokenStream {
        let (n, m) = (&t.name, &t.m);
        let nv = format_ident!("{}Verified", t.def.name);
        let req = format_ident!("{}_REQ", t.up);
        let mask = format_ident!("{}_MASK", t.up);
        let reqs = self.reqs(t);
        let bytes: Vec<TokenStream> = req_bytes(t)
            .into_iter()
            .map(|b| {
                let b = lit_u16(b);
                quote!((#b, ::graven::rt::mask_for(#req, #b)))
            })
            .collect();
        let req_ptrs: Vec<TokenStream> = t
            .fields
            .iter()
            .filter(|f| matches!(f.presence, Presence::Required) && kind(self.ir, f.ty) == FieldKind::Ptr)
            .map(|f| {
                let off = f.off.as_ref().unwrap();
                quote!(#m::#off)
            })
            .collect();
        let req_unions: Vec<TokenStream> = t
            .fields
            .iter()
            .filter(|f| matches!(f.presence, Presence::Required) && kind(self.ir, f.ty) == FieldKind::Union)
            .map(|f| {
                let off = f.off.as_ref().unwrap();
                quote!(#m::#off)
            })
            .collect();
        // Leaf data (strings, bytes, lists without tables) first, then the fields that
        // descend (tables, unions, `[Table]`); ID order inside each group. Same order as
        // the hand-written code (pre-plan): verifying `bank.Transaction.entries` before
        // `note` measured +9–16% on verified bank reads (Task 8 A/B). Precedence between
        // errors is not normative (spec §7.5) and the budget sum does not depend on the
        // order (§7.3), so this only changes speed.
        let (leaf, descend): (Vec<&F<'_>>, Vec<&F<'_>>) = t.fields.iter().partition(|f| !descends(f.ty));
        let verify: Vec<TokenStream> =
            leaf.into_iter().chain(descend).filter_map(|f| self.verify_stmt(t, f)).collect();
        let accessors: Vec<TokenStream> = t
            .fields
            .iter()
            .map(|f| {
                let d = super::user_doc_only(self.ir, &format!("{}.{}", t.def.name, f.name));
                let a = self.reader_accessor(t, f);
                quote!(#d #a)
            })
            .collect();
        let udoc = super::user_doc(self.ir, &t.def.name);
        let sid = self.schema_id(t.def);
        let qual = format!("{}.{}", self.ir.namespace, t.def.name);
        let doc = format!(
            " Checked reader of `{qual}`: accessors validate on demand. [`{n}::verified`] / [`{n}::from_bytes`] verify the whole message and return the view [`{nv}`]."
        );
        let doc_req = format!(" Required fields of `{}` (and optional scalar slots), in ID order.", t.def.name);
        let doc_sid = format!(" Schema id of `{qual}` (envelope header), read from the lock.");
        quote! {
            #udoc
            #[doc = #doc]
            #[derive(Clone, Copy)]
            pub struct #n<'a>(::graven::TableRef<'a>);

            #[doc = #doc_req]
            pub const #req: &[::graven::rt::Req] = &[#(#reqs),*];

            const #mask: ::graven::rt::ReqMask = ::graven::rt::ReqMask {
                len: #m::FIXED_LEN,
                bits: &[#(#bytes),*],
                ptrs: &[#(#req_ptrs),*],
                unions: &[#(#req_unions),*],
            };

            impl<'a> ::graven::rt::FromTable<'a> for #n<'a> {
                #[inline]
                fn from_ref(t: ::graven::TableRef<'a>) -> ::core::result::Result<Self, ::graven::Error> {
                    ::graven::rt::check_required_fast(&t, &#mask, #req)?;
                    ::core::result::Result::Ok(#n(t))
                }
                /// Verifies everything below this table (leaf data first, then tables, unions and
                /// `[Table]`; ID order within each group); does not charge its
                /// own header (the caller does).
                #[inline]
                fn verify(&self, ctx: &mut ::graven::VerifyCtx) -> ::core::result::Result<(), ::graven::Error> {
                    ctx.enter()?;
                    #(#verify)*
                    ctx.leave();
                    ::core::result::Result::Ok(())
                }
            }

            // Field names come from the schema (`next`, `add`, …).
            #[allow(clippy::should_implement_trait)]
            impl<'a> #n<'a> {
                #[doc = #doc_sid]
                pub const SCHEMA_ID: u64 = #sid;
                #[inline]
                pub fn read(buf: &'a [u8]) -> ::core::result::Result<Self, ::graven::Error> {
                    <Self as ::graven::rt::FromTable<'a>>::from_ref(::graven::TableRef::at(buf, 0)?)
                }
                /// [`read`](Self::read) + full verify (default limits). The view skips
                /// UTF-8 validation, which verify already did.
                #[inline]
                pub fn verified(buf: &'a [u8]) -> ::core::result::Result<#nv<'a>, ::graven::Error> {
                    Self::verified_with(buf, &mut ::graven::VerifyCtx::new(buf.len()))
                }
                /// [`verified`](Self::verified) with caller-supplied limits: [`read`](Self::read),
                /// then [`verify`](Self::verify) (which charges the root header). Use a
                /// **new** `VerifyCtx` per message.
                #[inline]
                pub fn verified_with(buf: &'a [u8], ctx: &mut ::graven::VerifyCtx) -> ::core::result::Result<#nv<'a>, ::graven::Error> {
                    let t = Self::read(buf)?;
                    t.verify(ctx)?;
                    ::core::result::Result::Ok(#nv(t))
                }
                /// Expects an envelope: decodes it, checks the schema id ([`Self::SCHEMA_ID`];
                /// otherwise `SchemaMismatch`) and runs [`verified`](Self::verified) on the payload.
                #[inline]
                pub fn from_bytes(buf: &'a [u8]) -> ::core::result::Result<#nv<'a>, ::graven::Error> {
                    Self::verified(::graven::rt::open_envelope(buf, Self::SCHEMA_ID)?)
                }
                #(#accessors)*
                /// Full verify of this table as the root (spec §7.2/§7.3): charges the header
                /// (`2 + fixed_len`) and calls `FromTable::verify` (everything below it).
                #[inline]
                pub fn verify(&self, ctx: &mut ::graven::VerifyCtx) -> ::core::result::Result<(), ::graven::Error> {
                    ctx.charge(2 + self.0.fixed_len() as usize)?;
                    ::graven::rt::FromTable::verify(self, ctx)
                }
            }
        }
    }

    /// Checked accessor of one field on `X<'a>`.
    fn reader_accessor(&self, t: &T<'_>, f: &F<'_>) -> TokenStream {
        let m = &t.m;
        let id = &f.id;
        let name = f.name;
        let req = matches!(f.presence, Presence::Required);
        let missing = quote!(::graven::Error::Missing { field: #name });
        let res = |ty: TokenStream| quote!(::core::result::Result<#ty, ::graven::Error>);
        let opt = |ty: TokenStream| quote!(::core::option::Option<#ty>);
        let (ret, body) = match f.ty {
            Ty::Bool => {
                let bc = f.bitc.as_ref().unwrap();
                match f.presence {
                    Presence::Optional => (opt(quote!(bool)), quote!(self.0.bool(#m::#bc))),
                    Presence::Default(v) => {
                        let d = self.default_lit(f.ty, v);
                        (quote!(bool), quote!(self.0.bool(#m::#bc).unwrap_or(#d)))
                    }
                    _ => (quote!(bool), quote!(self.0.bool(#m::#bc).unwrap_or(false))),
                }
            }
            Ty::Scalar(_) | Ty::Enum(_) | Ty::Struct(_) => {
                let off = f.off.as_ref().unwrap();
                let bc = f.bitc.as_ref().unwrap();
                let rty = self.inline_ty(f.ty);
                let wire = self.slot_ty(f.ty);
                let conv = |e: TokenStream| match f.ty {
                    Ty::Enum(_) => quote!(#rty::from_raw(#e)),
                    _ => e,
                };
                match f.presence {
                    Presence::Optional => {
                        let e = quote!(self.0.scalar::<#rty>(#m::#off, #m::#bc).ok().flatten());
                        (opt(rty.clone()), e)
                    }
                    Presence::Default(v) => {
                        let d = self.default_lit(f.ty, v);
                        (rty.clone(), conv(quote!(self.0.scalar_or::<#wire>(#m::#off, #m::#bc, #d))))
                    }
                    _ => (rty.clone(), conv(quote!(self.0.raw::<#wire>(#m::#off)))),
                }
            }
            Ty::Str | Ty::Bytes => {
                let off = f.off.as_ref().unwrap();
                let (call, ty, empty) = if matches!(f.ty, Ty::Str) {
                    (quote!(str), quote!(&'a str), quote!(""))
                } else {
                    (quote!(bytes), quote!(&'a [u8]), quote!(&[]))
                };
                if req {
                    (res(ty), quote!(::core::result::Result::Ok(self.0.#call(#m::#off)?.unwrap_or(#empty))))
                } else {
                    (res(opt(ty)), quote!(self.0.#call(#m::#off)))
                }
            }
            Ty::List(elem) => {
                let off = f.off.as_ref().unwrap();
                let (ty, get) = match &**elem {
                    Ty::Str | Ty::Bytes => (quote!(::graven::StrList<'a>), quote!(self.0.strs(#m::#off))),
                    Ty::Table(tid) => {
                        let tn = self.table_name(*tid);
                        if req {
                            // Absent reads as empty (like the hand-written `ListR`).
                            let ty = quote!(::graven::rt::ListR<'a, #tn<'a>>);
                            return accessor(id, res(ty), quote!(::core::result::Result::Ok(::graven::rt::ListR::new(self.0.tables(#m::#off)?))));
                        }
                        let ty = quote!(::graven::rt::ListR<'a, #tn<'a>>);
                        let body = quote!(::core::result::Result::Ok(self.0.tables(#m::#off)?.map(|l| ::graven::rt::ListR::new(::core::option::Option::Some(l)))));
                        return accessor(id, res(opt(ty)), body);
                    }
                    inline => {
                        let ety = self.inline_ty(inline);
                        (quote!(::graven::ScalarList<'a, #ety>), quote!(self.0.scalars::<#ety>(#m::#off)))
                    }
                };
                if req {
                    (res(ty), quote!(#get?.ok_or(#missing)))
                } else {
                    (res(opt(ty)), get)
                }
            }
            Ty::Table(tid) => {
                let off = f.off.as_ref().unwrap();
                let tn = self.table_name(*tid);
                let from = quote!(<#tn<'a> as ::graven::rt::FromTable<'a>>::from_ref);
                if req {
                    let body = quote! {
                        match self.0.table(#m::#off)? {
                            ::core::option::Option::Some(t) => #from(t),
                            ::core::option::Option::None => ::core::result::Result::Err(#missing),
                        }
                    };
                    (res(quote!(#tn<'a>)), body)
                } else {
                    let body = quote! {
                        match self.0.table(#m::#off)? {
                            ::core::option::Option::Some(t) => ::core::result::Result::Ok(::core::option::Option::Some(#from(t)?)),
                            ::core::option::Option::None => ::core::result::Result::Ok(::core::option::Option::None),
                        }
                    };
                    (res(opt(quote!(#tn<'a>))), body)
                }
            }
            Ty::Union(uid) => {
                let off = f.off.as_ref().unwrap();
                let u = &self.ir.unions[uid.0];
                let un = self.union_path(*uid, "");
                let arms: Vec<TokenStream> = u
                    .variants
                    .iter()
                    .map(|(v, tag, tid)| {
                        let v = ident(v);
                        let tag = lit_u16(*tag);
                        let tn = self.table_name(*tid);
                        quote! {
                            ::core::option::Option::Some((#tag, t)) => ::core::option::Option::Some(#un::#v(<#tn<'a> as ::graven::rt::FromTable<'a>>::from_ref(t)?)),
                        }
                    })
                    .collect();
                let m_expr = quote! {
                    match self.0.union(#m::#off)? {
                        #(#arms)*
                        ::core::option::Option::Some((tag, _)) => ::core::option::Option::Some(#un::Unknown(tag)),
                        ::core::option::Option::None => ::core::option::Option::None,
                    }
                };
                if req {
                    (res(quote!(#un<'a>)), quote!((#m_expr).ok_or(#missing)))
                } else {
                    (res(opt(quote!(#un<'a>))), quote!(::core::result::Result::Ok(#m_expr)))
                }
            }
        };
        accessor(id, ret, body)
    }

    /// Verify step of one field (spec §7.3), or `None` for inline fields.
    fn verify_stmt(&self, t: &T<'_>, f: &F<'_>) -> Option<TokenStream> {
        let m = &t.m;
        let off = f.off.as_ref();
        Some(match f.ty {
            Ty::Bool | Ty::Scalar(_) | Ty::Enum(_) | Ty::Struct(_) => return None,
            // Invariants 2 and 6: strings are validated by `str()` at the same offset the
            // view reads with `str_unchecked`.
            // Required pointer: `from_ref` already rejected an absent one (`Missing`), so the
            // `unwrap_or` arm is unreachable; the unconditional form is the hand-written
            // (pre-plan) shape and compiles to the same machine code (Task 8 A/B, `chunk`).
            Ty::Str if matches!(f.presence, Presence::Required) => quote! {
                ctx.charge(4 + self.0.str(#m::#off)?.unwrap_or("").len())?;
            },
            Ty::Bytes if matches!(f.presence, Presence::Required) => quote! {
                ctx.charge(4 + self.0.bytes(#m::#off)?.unwrap_or(&[]).len())?;
            },
            Ty::Str => quote! {
                if let ::core::option::Option::Some(s) = self.0.str(#m::#off)? {
                    ctx.charge(4 + s.len())?;
                }
            },
            Ty::Bytes => quote! {
                if let ::core::option::Option::Some(b) = self.0.bytes(#m::#off)? {
                    ctx.charge(4 + b.len())?;
                }
            },
            Ty::List(elem) => match &**elem {
                Ty::Str => quote!(::graven::rt::verify_strs(self.0.strs(#m::#off)?, ctx)?;),
                Ty::Bytes => quote!(::graven::rt::verify_bytes_list(self.0.strs(#m::#off)?, ctx)?;),
                Ty::Table(tid) => {
                    let tn = self.table_name(*tid);
                    quote!(::graven::rt::ListR::<'_, #tn<'_>>::new(self.0.tables(#m::#off)?).verify(ctx)?;)
                }
                inline => {
                    let ety = self.inline_ty(inline);
                    let per = Literal::usize_unsuffixed(self.ir.inline_size(inline).unwrap().max(1) as usize);
                    quote! {
                        if let ::core::option::Option::Some(l) = self.0.scalars::<#ety>(#m::#off)? {
                            ctx.charge(l.len().saturating_mul(#per).saturating_add(4))?;
                        }
                    }
                }
            },
            // Nested table: charges the child's header (inherent `verify`), then the child's
            // `FromTable::verify`.
            Ty::Table(tid) => {
                let tn = self.table_name(*tid);
                quote! {
                    if let ::core::option::Option::Some(t) = self.0.table(#m::#off)? {
                        <#tn<'_> as ::graven::rt::FromTable<'_>>::from_ref(t)?.verify(ctx)?;
                    }
                }
            }
            // Union: only known tags are descended into (invariant 3).
            Ty::Union(uid) => {
                let arms: Vec<TokenStream> = self.ir.unions[uid.0]
                    .variants
                    .iter()
                    .map(|(_, tag, tid)| {
                        let tag = lit_u16(*tag);
                        let tn = self.table_name(*tid);
                        quote! {
                            ::core::option::Option::Some((#tag, t)) => {
                                <#tn<'_> as ::graven::rt::FromTable<'_>>::from_ref(t)?.verify(ctx)?;
                            }
                        }
                    })
                    .collect();
                if let [(_, tag, tid)] = &self.ir.unions[uid.0].variants[..] {
                    let tag = lit_u16(*tag);
                    let tn = self.table_name(*tid);
                    return Some(quote! {
                        if let ::core::option::Option::Some((#tag, t)) = self.0.union(#m::#off)? {
                            <#tn<'_> as ::graven::rt::FromTable<'_>>::from_ref(t)?.verify(ctx)?;
                        }
                    });
                }
                quote! {
                    match self.0.union(#m::#off)? {
                        #(#arms)*
                        _ => {}
                    }
                }
            }
        })
    }
}

fn accessor(id: &Ident, ret: TokenStream, body: TokenStream) -> TokenStream {
    quote! {
        #[inline]
        pub fn #id(&self) -> #ret {
            #body
        }
    }
}

/// Whether verifying a field of this type descends into tables (nested table, union,
/// `[Table]`); the rest is leaf data (see the verify order in `table`).
fn descends(ty: &Ty) -> bool {
    match ty {
        Ty::Table(_) | Ty::Union(_) => true,
        Ty::List(elem) => matches!(**elem, Ty::Table(_)),
        _ => false,
    }
}
