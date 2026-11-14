//! Writer `XW<'b>` (`FixedW` if the table has only scalars, otherwise `TableW`), the list
//! element descriptor `XE: Elem` and, for a table of scalars only, `XListW`.

use super::*;

impl Cx<'_> {
    pub(super) fn writer(&self, t: &T<'_>) -> TokenStream {
        let (n, m) = (&t.name, &t.m);
        let nw = format_ident!("{}W", t.def.name);
        let ne = format_ident!("{}E", t.def.name);
        let nl = format_ident!("{}ListW", t.def.name);
        let fixed = t.def.all_scalar;
        let setters: Vec<TokenStream> = t
            .fields
            .iter()
            .map(|f| {
                let d = super::user_doc_only(self.ir, &format!("{}.{}", t.def.name, f.name));
                let s = self.setter(t, f);
                quote!(#d #s)
            })
            .collect();
        let udoc = super::user_doc(self.ir, &t.def.name);
        let doc = if fixed {
            format!(" Writer of `{}` (scalars only): writes through a `FixedW` whose size is known at compile time.", t.def.name)
        } else {
            format!(" Writer of `{}`.", t.def.name)
        };
        let (inner, root, push) = if fixed {
            (
                quote!(::graven::FixedW<'b, { #m::FIXED_LEN as usize }>),
                quote!(#nw(b.root_fixed())),
                quote!(#nw(l.push_fixed())),
            )
        } else {
            (quote!(::graven::TableW<'b>), quote!(#nw(b.root(#m::FIXED_LEN))), quote!(#nw(l.push())))
        };
        let to_bytes_doc = format!(" Writes a `{}` as the root of a new message and returns the envelope.", t.def.name);
        let list_w = if fixed {
            let doc = format!(" List of `{}` with a known count (scalar-only element: `FixedListW`).", t.def.name);
            quote! {
                #[doc = #doc]
                pub struct #nl<'a>(pub(crate) ::graven::FixedListW<'a, { #m::FIXED_LEN as usize }>);
                impl #nl<'_> {
                    #[inline(always)]
                    pub fn push(&mut self) -> #nw<'_> {
                        #nw(self.0.push())
                    }
                }
            }
        } else {
            quote!()
        };
        let elem_doc = format!(" List element descriptor of `{}`.", t.def.name);
        quote! {
            #udoc
            #[doc = #doc]
            pub struct #nw<'b>(pub(crate) #inner);

            // Field names come from the schema (`next`, `add`, …).
            #[allow(clippy::should_implement_trait)]
            impl<'b> #nw<'b> {
                #[inline]
                pub fn root(b: &'b mut ::graven::Builder<'_>) -> Self {
                    #root
                }
                #[doc = #to_bytes_doc]
                ///
                /// # Panics
                /// If `b` was not created with `Builder::new_enveloped` ("builder mode
                /// mismatch"), or if sealed envelopes are pending (`Builder::take_frames`).
                #[inline]
                pub fn to_bytes(b: &mut ::graven::Builder<'_>, f: impl ::core::ops::FnOnce(&mut #nw<'_>)) -> ::core::result::Result<::graven::Bytes, ::graven::Error> {
                    f(&mut #nw::root(b));
                    b.flush_envelope(#n::SCHEMA_ID)
                }
                /// Like [`to_bytes`](Self::to_bytes), but seals the envelope into the builder's batch
                /// (take it with `Builder::take_frames`).
                ///
                /// # Panics
                /// If `b` was not created with `Builder::new_enveloped`.
                #[inline]
                pub fn seal(b: &mut ::graven::Builder<'_>, f: impl ::core::ops::FnOnce(&mut #nw<'_>)) -> ::core::result::Result<(), ::graven::Error> {
                    f(&mut #nw::root(b));
                    b.seal_envelope(#n::SCHEMA_ID)
                }
                #(#setters)*
            }

            #[doc = #elem_doc]
            pub struct #ne;
            impl ::graven::rt::Elem for #ne {
                const FIXED_LEN: u16 = #m::FIXED_LEN;
                const PTR_SLOTS: &'static [u16] = #m::PTR_SLOTS;
                type W<'x> = #nw<'x>;
                #[inline]
                fn push_known<'x>(l: &'x mut ::graven::TableListW<'_>) -> #nw<'x> {
                    #push
                }
                #[inline]
                fn push_stream<'x>(l: &'x mut ::graven::StreamListW<'_, '_>) -> #nw<'x> {
                    #push
                }
            }

            #list_w
        }
    }

    fn setter(&self, t: &T<'_>, f: &F<'_>) -> TokenStream {
        let m = &t.m;
        let id = &f.id;
        let off = f.off.as_ref();
        match f.ty {
            Ty::Bool => {
                let bc = f.bitc.as_ref().unwrap();
                quote! {
                    #[inline]
                    pub fn #id(&mut self, v: bool) -> &mut Self {
                        self.0.set_bool(#m::#bc, v);
                        self
                    }
                }
            }
            Ty::Scalar(_) | Ty::Enum(_) | Ty::Struct(_) => {
                let bc = f.bitc.as_ref().unwrap();
                let rty = self.inline_ty(f.ty);
                let val = if matches!(f.ty, Ty::Enum(_)) { quote!(v.to_raw()) } else { quote!(v) };
                quote! {
                    #[inline]
                    pub fn #id(&mut self, v: #rty) -> &mut Self {
                        self.0.set(#m::#off, #m::#bc, #val);
                        self
                    }
                }
            }
            Ty::Str => quote! {
                #[inline]
                pub fn #id(&mut self, v: &str) -> &mut Self {
                    self.0.set_str(#m::#off, v);
                    self
                }
            },
            Ty::Bytes => quote! {
                #[inline]
                pub fn #id(&mut self, v: &[u8]) -> &mut Self {
                    self.0.set_bytes(#m::#off, v);
                    self
                }
            },
            Ty::List(elem) => {
                let streaming = format_ident!("{}_streaming", f.name);
                match &**elem {
                    Ty::Str => quote! {
                        /// `[string]`: header, then the elements, in order.
                        #[inline]
                        pub fn #id<'s>(&mut self, v: impl ::core::iter::IntoIterator<Item = &'s str, IntoIter: ::core::iter::ExactSizeIterator>) -> &mut Self {
                            self.0.set_strs(#m::#off, v.into_iter());
                            self
                        }
                    },
                    Ty::Bytes => quote! {
                        /// `[bytes]`: header, then the elements, in order.
                        #[inline]
                        pub fn #id<'s>(&mut self, v: impl ::core::iter::IntoIterator<Item = &'s [u8], IntoIter: ::core::iter::ExactSizeIterator>) -> &mut Self {
                            self.0.set_bytes_list(#m::#off, v.into_iter());
                            self
                        }
                    },
                    Ty::Table(tid) => {
                        let cm = self.table_mod(*tid);
                        let cn = &self.ir.tables[tid.0].name;
                        let ce = self.table_path(*tid, "E");
                        let known = if self.all_scalar(*tid) {
                            let cl = self.table_path(*tid, "ListW");
                            let doc = format!(" `{cn}` has only scalars: the list is a `FixedListW` (no per-element check).");
                            quote! {
                                #[doc = #doc]
                                #[inline]
                                pub fn #id(&mut self, n: u32) -> #cl<'_> {
                                    #cl(self.0.tables_fixed(#m::#off, n))
                                }
                            }
                        } else {
                            quote! {
                                #[inline]
                                pub fn #id(&mut self, n: u32) -> ::graven::rt::ListW<'_, #ce> {
                                    ::graven::rt::ListW::new(self.0.tables(#m::#off, n, #cm::FIXED_LEN))
                                }
                            }
                        };
                        quote! {
                            #known
                            /// List with an unknown count (close it with `close()` or on drop).
                            #[inline]
                            pub fn #streaming(&mut self) -> ::graven::rt::StreamW<'_, 'b, #ce> {
                                ::graven::rt::StreamW::new(self.0.tables_streaming(#m::#off, #cm::FIXED_LEN, #cm::PTR_SLOTS))
                            }
                        }
                    }
                    inline => {
                        let ety = self.inline_ty(inline);
                        quote! {
                            #[inline]
                            pub fn #id(&mut self, v: &[#ety]) -> &mut Self {
                                self.0.set_scalars(#m::#off, v);
                                self
                            }
                            /// List with an unknown count (the count is written on drop).
                            #[inline]
                            pub fn #streaming(&mut self) -> ::graven::ScalarListW<'_, #ety> {
                                self.0.scalars_streaming(#m::#off)
                            }
                        }
                    }
                }
            }
            Ty::Table(tid) => {
                let cm = self.table_mod(*tid);
                let cw = self.table_path(*tid, "W");
                let body = if self.all_scalar(*tid) {
                    quote!(#cw(self.0.table_fixed(#m::#off)))
                } else {
                    quote!(#cw(self.0.table(#m::#off, #cm::FIXED_LEN)))
                };
                quote! {
                    #[inline]
                    pub fn #id(&mut self) -> #cw<'_> {
                        #body
                    }
                }
            }
            Ty::Union(uid) => {
                let u = &self.ir.unions[uid.0];
                let mut out = TokenStream::new();
                for (v, tag, tid) in &u.variants {
                    let name = union_setter(f.name, v);
                    let tag = lit_u16(*tag);
                    let cm = self.table_mod(*tid);
                    let cw = self.table_path(*tid, "W");
                    let body = if self.all_scalar(*tid) {
                        quote!(#cw(self.0.union_fixed(#m::#off, #tag)))
                    } else {
                        quote!(#cw(self.0.union(#m::#off, #tag, #cm::FIXED_LEN)))
                    };
                    let doc = format!(" `{}` = `{}::{v}` (tag {}).", f.name, u.name, tag);
                    out.extend(quote! {
                        #[doc = #doc]
                        #[inline]
                        pub fn #name(&mut self) -> #cw<'_> {
                            #body
                        }
                    });
                }
                out
            }
        }
    }
}
