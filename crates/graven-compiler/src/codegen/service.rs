//! Codegen for `service` declarations (`--side client`, `--side server`) and RPC payload traits.

use super::validate::{has_any_rules, table_needs_validate};
use super::*;
use crate::names::pascal;

impl Cx<'_> {
    fn service_id_lit(&self, s: &Service) -> Literal {
        let ls = self
            .lock
            .services
            .iter()
            .find(|l| l.name == s.name)
            .unwrap_or_else(|| panic!("lock lacks service `{}` (run `gravenc update`)", s.name));
        let hex = ls
            .service_id
            .strip_prefix("0x")
            .unwrap_or_else(|| panic!("invalid service_id: {}", ls.service_id));
        let v = u64::from_str_radix(hex, 16)
            .unwrap_or_else(|_| panic!("invalid service_id: {}", ls.service_id));
        format!("{:#018x}", v).parse::<Literal>().unwrap()
    }

    fn writer_marker_ty(&self, ty: &Ty) -> TokenStream {
        match ty {
            Ty::Table(tid) => {
                let p = self.table_path(*tid, "W");
                quote!(#p<'static>)
            }
            Ty::Union(uid) => {
                let p = self.union_path(*uid, "W");
                quote!(#p<'static>)
            }
            _ => unreachable!("service payload must be table or union"),
        }
    }

    fn concrete_writer_ty(&self, ty: &Ty) -> TokenStream {
        match ty {
            Ty::Table(tid) => {
                let p = self.table_path(*tid, "W");
                quote!(#p<'_>)
            }
            Ty::Union(uid) => {
                let p = self.union_path(*uid, "W");
                quote!(#p<'_>)
            }
            _ => unreachable!("service payload must be table or union"),
        }
    }

    fn view_marker_ty(&self, ty: &Ty) -> TokenStream {
        match ty {
            Ty::Table(tid) => {
                let p = self.table_path(*tid, "");
                quote!(#p<'static>)
            }
            Ty::Union(uid) => {
                let p = self.union_path(*uid, "");
                quote!(#p<'static>)
            }
            _ => unreachable!("service payload must be table or union"),
        }
    }

    fn verified_ty(&self, ty: &Ty) -> TokenStream {
        match ty {
            Ty::Table(tid) => {
                let p = self.table_path(*tid, "Verified");
                quote!(#p<'a>)
            }
            Ty::Union(uid) => {
                let p = self.union_path(*uid, "Verified");
                quote!(#p<'a>)
            }
            _ => unreachable!("service payload must be table or union"),
        }
    }

    pub(super) fn rpc_items(&self, opts: GenOptions) -> TokenStream {
        if !opts.client && !opts.server {
            return quote!();
        }
        let mut out = TokenStream::new();

        // Emit RpcPayload / RpcView for local tables and unions, plus `<Union>W` writers.
        for t in self.ir.tables.iter().filter(|t| t.origin.is_none()) {
            let tn = format_ident!("{}", t.name);
            let tnw = format_ident!("{}W", t.name);
            let tnv = format_ident!("{}Verified", t.name);
            out.extend(quote! {
                impl ::graven::rpc::RpcPayload for #tnw<'static> {
                    type Writer<'b> = #tnw<'b>;

                    #[inline]
                    fn encode_with(
                        f: impl ::core::ops::FnOnce(&mut Self::Writer<'_>),
                    ) -> ::core::result::Result<(u16, u64, ::graven::rt::Vec<u8>), ::graven::Error> {
                        let pool = ::graven::rpc::NoPool;
                        let mut b = ::graven::Builder::new(&pool);
                        {
                            let mut w = #tnw::root(&mut b);
                            f(&mut w);
                        }
                        ::core::result::Result::Ok((0u16, #tn::SCHEMA_ID, b.finished()?.to_vec()))
                    }
                }

                impl ::graven::rpc::RpcView for #tn<'static> {
                    type View<'a> = #tnv<'a>;

                    #[inline]
                    fn verify_payload(
                        variant: u16,
                        schema_id: u64,
                        payload: &[u8],
                    ) -> ::core::result::Result<(), ::graven::rpc::ProtocolError> {
                        if variant != 0 {
                            return ::core::result::Result::Err(::graven::rpc::ProtocolError::UnexpectedVariant(variant));
                        }
                        if schema_id != #tn::SCHEMA_ID {
                            return ::core::result::Result::Err(::graven::rpc::ProtocolError::SchemaMismatch {
                                expected: #tn::SCHEMA_ID,
                                found: schema_id,
                            });
                        }
                        #tn::verified(payload)
                            .map(|_| ())
                            .map_err(::graven::rpc::ProtocolError::Payload)
                    }

                    #[inline]
                    fn view_verified(_variant: u16, payload: &[u8]) -> Self::View<'_> {
                        #tnv(#tn::read(payload).expect("verified at Response construction"))
                    }
                }
            });
        }

        for u in self.ir.unions.iter().filter(|u| u.origin.is_none()) {
            let un = format_ident!("{}", u.name);
            let unw = format_ident!("{}W", u.name);
            let unv = format_ident!("{}Verified", u.name);
            let var_methods: Vec<TokenStream> = u
                .variants
                .iter()
                .map(|(vname, tag, tid)| {
                    let mname = ident(&snake(vname));
                    let vt = self.table_path(*tid, "");
                    let vtw = self.table_path(*tid, "W");
                    let tag_lit = lit_u16(*tag);
                    quote! {
                        #[inline]
                        pub fn #mname(&mut self) -> #vtw<'_> {
                            self.chosen = ::core::option::Option::Some((#tag_lit, #vt::SCHEMA_ID));
                            #vtw::root(self.builder)
                        }
                    }
                })
                .collect();
            let verify_arms: Vec<TokenStream> = u
                .variants
                .iter()
                .map(|(_, tag, tid)| {
                    let vt = self.table_path(*tid, "");
                    let tag_lit = lit_u16(*tag);
                    quote! {
                        #tag_lit => {
                            if schema_id != #vt::SCHEMA_ID {
                                return ::core::result::Result::Err(::graven::rpc::ProtocolError::SchemaMismatch {
                                    expected: #vt::SCHEMA_ID,
                                    found: schema_id,
                                });
                            }
                            #vt::verified(payload)
                                .map(|_| ())
                                .map_err(::graven::rpc::ProtocolError::Payload)
                        }
                    }
                })
                .collect();
            let view_arms: Vec<TokenStream> = u
                .variants
                .iter()
                .map(|(vname, tag, tid)| {
                    let evar = ident(vname);
                    let vt = self.table_path(*tid, "");
                    let tag_lit = lit_u16(*tag);
                    let make_v = if self.ir.tables[tid.0].origin.is_none() {
                        let vtv = self.table_path(*tid, "Verified");
                        quote!(#vtv(#vt::read(payload).expect("verified at Response construction")))
                    } else {
                        quote!(#vt::verified(payload).expect("verified at Response construction"))
                    };
                    quote! {
                        #tag_lit => #unv::#evar(#make_v),
                    }
                })
                .collect();
            let u_doc = format!(" Top-level writer for union `{}` in RPC calls and replies.", u.name);
            out.extend(quote! {
                #[doc = #u_doc]
                pub struct #unw<'b> {
                    builder: &'b mut ::graven::Builder<'static>,
                    chosen: ::core::option::Option<(u16, u64)>,
                }

                impl<'b> #unw<'b> {
                    #[inline]
                    pub(crate) fn new(builder: &'b mut ::graven::Builder<'static>) -> Self {
                        Self { builder, chosen: ::core::option::Option::None }
                    }
                    #(#var_methods)*
                }

                impl ::graven::rpc::RpcPayload for #unw<'static> {
                    type Writer<'b> = #unw<'b>;

                    #[inline]
                    fn encode_with(
                        f: impl ::core::ops::FnOnce(&mut Self::Writer<'_>),
                    ) -> ::core::result::Result<(u16, u64, ::graven::rt::Vec<u8>), ::graven::Error> {
                        static POOL: ::graven::rpc::NoPool = ::graven::rpc::NoPool;
                        let mut b = ::graven::Builder::new(&POOL);
                        let (variant, sid) = {
                            let mut w = #unw::new(&mut b);
                            f(&mut w);
                            w.chosen.expect("union writer closure must select a variant")
                        };
                        ::core::result::Result::Ok((variant, sid, b.finished()?.to_vec()))
                    }
                }

                impl ::graven::rpc::RpcView for #un<'static> {
                    type View<'a> = #unv<'a>;

                    #[inline]
                    fn verify_payload(
                        variant: u16,
                        schema_id: u64,
                        payload: &[u8],
                    ) -> ::core::result::Result<(), ::graven::rpc::ProtocolError> {
                        match variant {
                            0 => ::core::result::Result::Err(::graven::rpc::ProtocolError::MissingVariant),
                            #(#verify_arms)*
                            _ => ::core::result::Result::Ok(()),
                        }
                    }

                    #[inline]
                    fn view_verified(variant: u16, payload: &[u8]) -> Self::View<'_> {
                        match variant {
                            #(#view_arms)*
                            tag => #unv::Unknown(tag),
                        }
                    }
                }
            });
        }

        for svc in &self.ir.services {
            out.extend(self.service_common(svc));
            if opts.server {
                out.extend(self.service_server(svc));
            }
            if opts.client {
                out.extend(self.service_client(svc));
            }
        }

        out
    }

    fn service_common(&self, svc: &Service) -> TokenStream {
        let sid_const = format_ident!("{}_SERVICE_ID", upper(&svc.name));
        let sid_lit = self.service_id_lit(svc);
        let sid_doc = format!(" Service id of `{}.{}` (RPC frame header).", self.ir.namespace, svc.name);
        let tmod = format_ident!("{}_timeouts", snake(&svc.name));
        let timeout_consts: Vec<TokenStream> = svc
            .methods
            .iter()
            .filter_map(|m| {
                let mname = m.name.as_deref()?;
                let ms = m.timeout_ms?;
                let cname = format_ident!("{}_MS", upper(mname));
                let lit = Literal::u32_unsuffixed(ms);
                Some(quote! {
                    pub const #cname: u32 = #lit;
                })
            })
            .collect();
        quote! {
            #[doc = #sid_doc]
            pub const #sid_const: u64 = #sid_lit;

            /// Declared method timeouts in milliseconds.
            pub mod #tmod {
                #(#timeout_consts)*
            }
        }
    }

    fn decode_input_arm(&self, sid_const: &Ident, m: &Method, validate: bool) -> TokenStream {
        let m_id = lit_u16(m.id);
        let mname = m.name.as_deref().unwrap();
        let evar = format_ident!("{}", pascal(mname));
        let shape = m.shape.unwrap();
        let one_way = shape == MethodShape::OneWay;
        let in_ty = m.input.as_ref().unwrap();

        let decode_val = match in_ty {
            Ty::Table(tid) => {
                let tpath = self.table_path(*tid, "");
                let val_stmt = if validate && table_needs_validate(self.ir, *tid) {
                    quote! {
                        if let ::core::result::Result::Err(e) = v.validate(checks) {
                            return ::core::result::Result::Err(::graven::rpc::Rejected::from_validate_error(#sid_const, #m_id, #one_way, e));
                        }
                    }
                } else {
                    quote!()
                };
                quote! {
                    if f.header.variant != 0 {
                        return ::core::result::Result::Err(::graven::rpc::Rejected::malformed(#sid_const, #m_id, #one_way, "unexpected variant on table input"));
                    }
                    if f.schema_id != #tpath::SCHEMA_ID {
                        return ::core::result::Result::Err(::graven::rpc::Rejected::malformed(#sid_const, #m_id, #one_way, "input schema id mismatch"));
                    }
                    let v = match #tpath::verified(f.payload) {
                        ::core::result::Result::Ok(v) => v,
                        ::core::result::Result::Err(e) => {
                            return ::core::result::Result::Err(::graven::rpc::Rejected::from_read_error(#sid_const, #m_id, #one_way, e));
                        }
                    };
                    #val_stmt
                }
            }
            Ty::Union(uid) => {
                let u = &self.ir.unions[uid.0];
                let upv = self.union_path(*uid, "Verified");
                let var_arms: Vec<TokenStream> = u
                    .variants
                    .iter()
                    .map(|(vname, tag, tid)| {
                        let vt = self.table_path(*tid, "");
                        let vv = ident(vname);
                        let tag_lit = lit_u16(*tag);
                        let val_stmt = if validate && table_needs_validate(self.ir, *tid) {
                            quote! {
                                if let ::core::result::Result::Err(e) = x.validate(checks) {
                                    return ::core::result::Result::Err(::graven::rpc::Rejected::from_validate_error(#sid_const, #m_id, #one_way, e));
                                }
                            }
                        } else {
                            quote!()
                        };
                        quote! {
                            #tag_lit => {
                                if f.schema_id != #vt::SCHEMA_ID {
                                    return ::core::result::Result::Err(::graven::rpc::Rejected::malformed(#sid_const, #m_id, #one_way, "union variant schema id mismatch"));
                                }
                                let x = match #vt::verified(f.payload) {
                                    ::core::result::Result::Ok(x) => x,
                                    ::core::result::Result::Err(e) => {
                                        return ::core::result::Result::Err(::graven::rpc::Rejected::from_read_error(#sid_const, #m_id, #one_way, e));
                                    }
                                };
                                #val_stmt
                                #upv::#vv(x)
                            }
                        }
                    })
                    .collect();
                quote! {
                    let v = match f.header.variant {
                        0 => {
                            return ::core::result::Result::Err(::graven::rpc::Rejected::malformed(#sid_const, #m_id, #one_way, "missing union variant"));
                        }
                        #(#var_arms)*
                        tag => #upv::Unknown(tag),
                    };
                }
            }
            _ => unreachable!(),
        };

        let construct = match shape {
            MethodShape::OneWay => quote!(Self::#evar(v)),
            MethodShape::Unary => quote!(Self::#evar(v, ::graven::rpc::Reply::new(#sid_const, #m_id))),
            MethodShape::Stream => quote!(Self::#evar(v, ::graven::rpc::StreamReply::new(#sid_const, #m_id))),
        };
        quote! {
            #m_id => {
                #decode_val
                ::core::result::Result::Ok(#construct)
            }
        }
    }

    fn service_server(&self, svc: &Service) -> TokenStream {
        let call_enum = format_ident!("{}Call", svc.name);
        let sid_const = format_ident!("{}_SERVICE_ID", upper(&svc.name));
        let svc_doc = user_doc(self.ir, &svc.name);

        let variants: Vec<TokenStream> = svc
            .methods
            .iter()
            .filter_map(|m| {
                let mname = m.name.as_deref()?;
                let evar = format_ident!("{}", pascal(mname));
                let mdoc = user_doc_only(self.ir, &format!("{}.{mname}", svc.name));
                let in_v = self.verified_ty(m.input.as_ref()?);
                let out_m = m.output.as_ref().map(|t| self.writer_marker_ty(t));
                let err_m = m.error.as_ref().map_or_else(|| quote!(()), |t| self.writer_marker_ty(t));
                let item = match m.shape? {
                    MethodShape::Unary => {
                        let om = out_m.unwrap();
                        quote!(#mdoc #evar(#in_v, ::graven::rpc::Reply<#om, #err_m>),)
                    }
                    MethodShape::Stream => {
                        let om = out_m.unwrap();
                        quote!(#mdoc #evar(#in_v, ::graven::rpc::StreamReply<#om, #err_m>),)
                    }
                    MethodShape::OneWay => quote!(#mdoc #evar(#in_v),),
                };
                Some(item)
            })
            .collect();

        let active: Vec<&Method> = svc.methods.iter().filter(|m| m.name.is_some()).collect();
        let one_way: Vec<Literal> =
            active.iter().filter(|m| m.shape == Some(MethodShape::OneWay)).map(|m| lit_u16(m.id)).collect();
        let one_way_ids = quote!(&[#(#one_way),*]);
        let decode_arms: Vec<TokenStream> = active.iter().map(|m| self.decode_input_arm(&sid_const, m, false)).collect();
        let decode_val_fn = if has_any_rules(self.ir) {
            let val_arms: Vec<TokenStream> = active.iter().map(|m| self.decode_input_arm(&sid_const, m, true)).collect();
            quote! {
                /// Decodes, verifies, and validates one `Call` frame of this service.
                pub fn decode_validated(frame: &'a [u8], checks: &impl Checks) -> ::core::result::Result<Self, ::graven::rpc::Rejected> {
                    let f = match ::graven::rpc::decode_frame(frame) {
                        ::core::result::Result::Ok(f) => f,
                        ::core::result::Result::Err(e) => return ::core::result::Result::Err(::graven::rpc::Rejected::from_frame_error_for(frame, e, #sid_const, #one_way_ids)),
                    };
                    if f.header.kind != ::graven::rpc::FrameKind::Call {
                        return ::core::result::Result::Err(::graven::rpc::Rejected::malformed(
                            f.header.service_id,
                            f.header.method,
                            false,
                            "expected Call frame",
                        ));
                    }
                    if f.header.service_id != #sid_const {
                        return ::core::result::Result::Err(::graven::rpc::Rejected::unsupported(
                            f.header.service_id,
                            f.header.method,
                            "unknown service",
                        ));
                    }
                    match f.header.method {
                        #(#val_arms)*
                        _ => ::core::result::Result::Err(::graven::rpc::Rejected::unsupported(
                            #sid_const,
                            f.header.method,
                            "unknown method",
                        )),
                    }
                }
            }
        } else {
            quote!()
        };

        let lt = if active.is_empty() { quote!() } else { quote!(<'a>) };
        let impl_lt = if active.is_empty() { quote!() } else { quote!(<'a>) };

        let decode_fn = if active.is_empty() {
            quote! {
                pub fn decode(frame: &[u8]) -> ::core::result::Result<Self, ::graven::rpc::Rejected> {
                    let f = match ::graven::rpc::decode_frame(frame) {
                        ::core::result::Result::Ok(f) => f,
                        ::core::result::Result::Err(e) => return ::core::result::Result::Err(::graven::rpc::Rejected::from_frame_error_for(frame, e, #sid_const, #one_way_ids)),
                    };
                    if f.header.kind != ::graven::rpc::FrameKind::Call {
                        return ::core::result::Result::Err(::graven::rpc::Rejected::malformed(
                            f.header.service_id,
                            f.header.method,
                            false,
                            "expected Call frame",
                        ));
                    }
                    if f.header.service_id != #sid_const {
                        return ::core::result::Result::Err(::graven::rpc::Rejected::unsupported(
                            f.header.service_id,
                            f.header.method,
                            "unknown service",
                        ));
                    }
                    ::core::result::Result::Err(::graven::rpc::Rejected::unsupported(
                        #sid_const,
                        f.header.method,
                        "unknown method",
                    ))
                }
            }
        } else {
            quote! {
                /// Decodes and verifies one `Call` frame of this service.
                pub fn decode(frame: &'a [u8]) -> ::core::result::Result<Self, ::graven::rpc::Rejected> {
                    let f = match ::graven::rpc::decode_frame(frame) {
                        ::core::result::Result::Ok(f) => f,
                        ::core::result::Result::Err(e) => return ::core::result::Result::Err(::graven::rpc::Rejected::from_frame_error_for(frame, e, #sid_const, #one_way_ids)),
                    };
                    if f.header.kind != ::graven::rpc::FrameKind::Call {
                        return ::core::result::Result::Err(::graven::rpc::Rejected::malformed(
                            f.header.service_id,
                            f.header.method,
                            false,
                            "expected Call frame",
                        ));
                    }
                    if f.header.service_id != #sid_const {
                        return ::core::result::Result::Err(::graven::rpc::Rejected::unsupported(
                            f.header.service_id,
                            f.header.method,
                            "unknown service",
                        ));
                    }
                    match f.header.method {
                        #(#decode_arms)*
                        _ => ::core::result::Result::Err(::graven::rpc::Rejected::unsupported(
                            #sid_const,
                            f.header.method,
                            "unknown method",
                        )),
                    }
                }
                #decode_val_fn
            }
        };

        let _ = lt;
        let enum_decl = if active.is_empty() {
            quote!(pub enum #call_enum {})
        } else {
            quote! {
                pub enum #call_enum<'a> {
                    #(#variants)*
                }
            }
        };
        let _ = impl_lt;
        let impl_header = if active.is_empty() {
            quote!(impl #call_enum)
        } else {
            quote!(impl<'a> #call_enum<'a>)
        };

        quote! {
            #svc_doc
            /// Incoming call dispatched by the generated server edge.
            #enum_decl

            #impl_header {
                #decode_fn
            }
        }
    }

    fn service_client(&self, svc: &Service) -> TokenStream {
        let client_name = format_ident!("{}Client", svc.name);
        let sid_const = format_ident!("{}_SERVICE_ID", upper(&svc.name));
        let svc_doc = user_doc(self.ir, &svc.name);

        let methods: Vec<TokenStream> = svc
            .methods
            .iter()
            .filter_map(|m| {
                let mname = m.name.as_deref()?;
                let fn_name = ident(mname);
                let fn_to_name = format_ident!("{}_with_timeout", mname);
                let mdoc = user_doc_only(self.ir, &format!("{}.{mname}", svc.name));
                let m_id = lit_u16(m.id);
                let in_ty = m.input.as_ref()?;
                let in_marker = self.writer_marker_ty(in_ty);
                let in_arg_w = self.concrete_writer_ty(in_ty);
                let default_timeout = match m.timeout_ms {
                    Some(ms) => {
                        let lit = Literal::u32_unsuffixed(ms);
                        quote!(::core::option::Option::Some(#lit))
                    }
                    None => quote!(::core::option::Option::None),
                };

                let code = match m.shape? {
                    MethodShape::OneWay => {
                        quote! {
                            #mdoc
                            pub async fn #fn_name(
                                &self,
                                f: impl ::core::ops::FnOnce(&mut #in_arg_w),
                            ) -> ::core::result::Result<(), ::graven::rpc::CallError<(), T::Error>> {
                                let frame = ::graven::rpc::encode_call_frame::<#in_marker>(#sid_const, #m_id, f);
                                let reply = self.send.send(frame, ::core::option::Option::None).await.map_err(::graven::rpc::CallError::Send)?;
                                ::graven::rpc::decode_oneway_reply(reply)
                            }
                        }
                    }
                    MethodShape::Unary => {
                        let out_marker = self.view_marker_ty(m.output.as_ref()?);
                        match m.error.as_ref() {
                            Some(err_ty) => {
                                let err_marker = self.view_marker_ty(err_ty);
                                quote! {
                                    #mdoc
                                    pub async fn #fn_name(
                                        &self,
                                        f: impl ::core::ops::FnOnce(&mut #in_arg_w),
                                    ) -> ::core::result::Result<::graven::rpc::Response<#out_marker>, ::graven::rpc::CallError<::graven::rpc::Response<#err_marker>, T::Error>> {
                                        self.#fn_to_name(f, #default_timeout).await
                                    }

                                    #mdoc
                                    pub async fn #fn_to_name(
                                        &self,
                                        f: impl ::core::ops::FnOnce(&mut #in_arg_w),
                                        timeout_ms: ::core::option::Option<u32>,
                                    ) -> ::core::result::Result<::graven::rpc::Response<#out_marker>, ::graven::rpc::CallError<::graven::rpc::Response<#err_marker>, T::Error>> {
                                        let frame = ::graven::rpc::encode_call_frame::<#in_marker>(#sid_const, #m_id, f);
                                        let reply = self.send.send(frame, timeout_ms).await.map_err(::graven::rpc::CallError::Send)?;
                                        ::graven::rpc::decode_unary::<#out_marker, #err_marker, T::Error>(#sid_const, #m_id, reply)
                                    }
                                }
                            }
                            None => quote! {
                                #mdoc
                                pub async fn #fn_name(
                                    &self,
                                    f: impl ::core::ops::FnOnce(&mut #in_arg_w),
                                ) -> ::core::result::Result<::graven::rpc::Response<#out_marker>, ::graven::rpc::CallError<(), T::Error>> {
                                    self.#fn_to_name(f, #default_timeout).await
                                }

                                #mdoc
                                pub async fn #fn_to_name(
                                    &self,
                                    f: impl ::core::ops::FnOnce(&mut #in_arg_w),
                                    timeout_ms: ::core::option::Option<u32>,
                                ) -> ::core::result::Result<::graven::rpc::Response<#out_marker>, ::graven::rpc::CallError<(), T::Error>> {
                                    let frame = ::graven::rpc::encode_call_frame::<#in_marker>(#sid_const, #m_id, f);
                                    let reply = self.send.send(frame, timeout_ms).await.map_err(::graven::rpc::CallError::Send)?;
                                    ::graven::rpc::decode_unary_ok::<#out_marker, T::Error>(#sid_const, #m_id, reply)
                                }
                            },
                        }
                    }
                    MethodShape::Stream => {
                        let out_marker = self.view_marker_ty(m.output.as_ref()?);
                        match m.error.as_ref() {
                            Some(err_ty) => {
                                let err_marker = self.view_marker_ty(err_ty);
                                quote! {
                                    #mdoc
                                    pub async fn #fn_name(
                                        &self,
                                        f: impl ::core::ops::FnOnce(&mut #in_arg_w),
                                    ) -> ::core::result::Result<::graven::rpc::StreamResponse<#out_marker, impl ::graven::rpc::ReplyStream<Error = T::Error> + '_>, ::graven::rpc::CallError<::graven::rpc::Response<#err_marker>, T::Error>> {
                                        self.#fn_to_name(f, #default_timeout).await
                                    }

                                    #mdoc
                                    pub async fn #fn_to_name(
                                        &self,
                                        f: impl ::core::ops::FnOnce(&mut #in_arg_w),
                                        timeout_ms: ::core::option::Option<u32>,
                                    ) -> ::core::result::Result<::graven::rpc::StreamResponse<#out_marker, impl ::graven::rpc::ReplyStream<Error = T::Error> + '_>, ::graven::rpc::CallError<::graven::rpc::Response<#err_marker>, T::Error>> {
                                        let frame = ::graven::rpc::encode_call_frame::<#in_marker>(#sid_const, #m_id, f);
                                        let reply = self.send.send_stream(frame, timeout_ms).await.map_err(::graven::rpc::CallError::Send)?;
                                        ::graven::rpc::decode_stream_start::<#out_marker, #err_marker, _>(#sid_const, #m_id, reply).await
                                    }
                                }
                            }
                            None => quote! {
                                #mdoc
                                pub async fn #fn_name(
                                    &self,
                                    f: impl ::core::ops::FnOnce(&mut #in_arg_w),
                                ) -> ::core::result::Result<::graven::rpc::StreamResponse<#out_marker, impl ::graven::rpc::ReplyStream<Error = T::Error> + '_>, ::graven::rpc::CallError<(), T::Error>> {
                                    self.#fn_to_name(f, #default_timeout).await
                                }

                                #mdoc
                                pub async fn #fn_to_name(
                                    &self,
                                    f: impl ::core::ops::FnOnce(&mut #in_arg_w),
                                    timeout_ms: ::core::option::Option<u32>,
                                ) -> ::core::result::Result<::graven::rpc::StreamResponse<#out_marker, impl ::graven::rpc::ReplyStream<Error = T::Error> + '_>, ::graven::rpc::CallError<(), T::Error>> {
                                    let frame = ::graven::rpc::encode_call_frame::<#in_marker>(#sid_const, #m_id, f);
                                    let reply = self.send.send_stream(frame, timeout_ms).await.map_err(::graven::rpc::CallError::Send)?;
                                    ::graven::rpc::decode_stream_ok::<#out_marker, _>(#sid_const, #m_id, reply).await
                                }
                            },
                        }
                    }
                };
                Some(code)
            })
            .collect();

        quote! {
            #svc_doc
            /// Typed RPC client generated for this service.
            pub struct #client_name<T> {
                send: T,
            }

            impl<T> #client_name<T> {
                /// Wraps the application's [`::graven::rpc::Sender`] in a typed client.
                #[inline]
                pub fn new(send: T) -> Self {
                    Self { send }
                }
            }

            impl<T: ::graven::rpc::Sender> #client_name<T> {
                #(#methods)*
            }
        }
    }
}
