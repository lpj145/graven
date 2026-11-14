//! Verified view `XVerified<'a>`: explicit accessors (never `Deref`) + `reader()`.
//!
//! Each accessor only follows edges that `X`'s `verify` walked, at the same offset and with the
//! same type (invariants 1–4 of `graven::rt`); the `unsafe` blocks carry a `// SAFETY:`
//! comment citing the invariant.

use super::*;

impl Cx<'_> {
    pub(super) fn view(&self, t: &T<'_>) -> TokenStream {
        let n = &t.name;
        let nv = format_ident!("{}Verified", t.def.name);
        let accessors: Vec<TokenStream> = t
            .fields
            .iter()
            .map(|f| {
                let d = super::user_doc_only(self.ir, &format!("{}.{}", t.def.name, f.name));
                let a = self.view_accessor(t, f);
                quote!(#d #a)
            })
            .collect();
        let udoc = super::user_doc(self.ir, &t.def.name);
        let doc = format!(
            " A [`{n}`] whose `verify()` passed. Only constructed by [`{n}::verified`] / [`{n}::from_bytes`] (or as a field/element of a verified view); strings are read without revalidating UTF-8."
        );
        let from_doc = format!(
            " The required fields were checked by `{n}::from_ref` when the root's `verify()` reached `t` as a `{n}` along this edge (this fn's contract, condition 1)."
        );
        let validate_fns = self.validate_methods(t);
        quote! {
            #udoc
            #[doc = #doc]
            #[derive(Clone, Copy)]
            pub struct #nv<'a>(#n<'a>);

            impl<'a> ::graven::rt::FromVerified<'a> for #nv<'a> {
                #[doc = #from_doc]
                #[allow(unsafe_code)]
                #[inline]
                unsafe fn from_verified(t: ::graven::TableRef<'a>) -> Self {
                    #nv(#n(t))
                }
            }

            // Field names come from the schema (`next`, `add`, …).
            #[allow(clippy::should_implement_trait)]
            impl<'a> #nv<'a> {
                /// The checked reader over the same table.
                #[inline]
                pub fn reader(&self) -> #n<'a> {
                    self.0
                }
                #(#accessors)*
                #validate_fns
            }
        }
    }

    fn view_accessor(&self, t: &T<'_>, f: &F<'_>) -> TokenStream {
        let (n, m) = (&t.def.name, &t.m);
        let id = &f.id;
        let name = f.name;
        let req = matches!(f.presence, Presence::Required);
        let missing = quote!(::graven::Error::Missing { field: #name });
        let res = |ty: TokenStream| quote!(::core::result::Result<#ty, ::graven::Error>);
        let opt = |ty: TokenStream| quote!(::core::option::Option<#ty>);
        let off = f.off.as_ref();
        let off_s = off.map(|o| format!("{m}::{o}")).unwrap_or_default();
        match f.ty {
            Ty::Str => {
                let s = safety(&format!(
                    "a `{n}Verified` is only built after `{n}::verify()` succeeded on this very table (by `verified`, or `from_verified` under its contract: reached as a `{n}` along the same edge, condition 1); `{n}::verify` calls the validating `str({off_s})` at this same offset (condition 2)."
                ));
                if req {
                    unsafe_accessor(
                        id,
                        res(quote!(&'a str)),
                        quote! {
                            #s
                            let v = unsafe { self.0.0.str_unchecked(#m::#off) }?;
                            ::core::result::Result::Ok(v.unwrap_or(""))
                        },
                    )
                } else {
                    unsafe_accessor(
                        id,
                        res(opt(quote!(&'a str))),
                        quote! {
                            #s
                            unsafe { self.0.0.str_unchecked(#m::#off) }
                        },
                    )
                }
            }
            Ty::List(elem) if matches!(**elem, Ty::Str) => {
                let s = safety(&format!(
                    "`{n}::verify` followed this same edge (`{off_s}`) and passed this very list to `verify_strs`, which validated every element as UTF-8 (`VStrList::new` contract; invariants 2 and 6)."
                ));
                if req {
                    unsafe_accessor(
                        id,
                        res(quote!(::graven::rt::VStrList<'a>)),
                        quote! {
                            let l = self.0.0.strs(#m::#off)?;
                            #s
                            let v = unsafe { ::graven::rt::VStrList::new(l) };
                            ::core::result::Result::Ok(v)
                        },
                    )
                } else {
                    unsafe_accessor(
                        id,
                        res(opt(quote!(::graven::rt::VStrList<'a>))),
                        quote! {
                            let ::core::option::Option::Some(l) = self.0.0.strs(#m::#off)? else {
                                return ::core::result::Result::Ok(::core::option::Option::None);
                            };
                            #s
                            let v = unsafe { ::graven::rt::VStrList::new(::core::option::Option::Some(l)) };
                            ::core::result::Result::Ok(::core::option::Option::Some(v))
                        },
                    )
                }
            }
            Ty::List(elem) if matches!(**elem, Ty::Table(_)) => {
                let Ty::Table(tid) = **elem else { unreachable!() };
                let en = &self.ir.tables[tid.0].name;
                let ev = self.table_path(tid, "Verified");
                let s = safety(&format!(
                    "`{n}::verify` followed this same edge (`{off_s}`) and verified it as `ListR<{en}>`, i.e. every element as `{en}`; `{en}Verified` wraps exactly `{en}`, so `T` matches what verify used for this list (`VListR::new` contract)."
                ));
                if req {
                    unsafe_accessor(
                        id,
                        res(quote!(::graven::rt::VListR<'a, #ev<'a>>)),
                        quote! {
                            let l = self.0.0.tables(#m::#off)?;
                            #s
                            let v = unsafe { ::graven::rt::VListR::new(l) };
                            ::core::result::Result::Ok(v)
                        },
                    )
                } else {
                    unsafe_accessor(
                        id,
                        res(opt(quote!(::graven::rt::VListR<'a, #ev<'a>>))),
                        quote! {
                            let ::core::option::Option::Some(l) = self.0.0.tables(#m::#off)? else {
                                return ::core::result::Result::Ok(::core::option::Option::None);
                            };
                            #s
                            let v = unsafe { ::graven::rt::VListR::new(::core::option::Option::Some(l)) };
                            ::core::result::Result::Ok(::core::option::Option::Some(v))
                        },
                    )
                }
            }
            Ty::Table(tid) => {
                let cn = &self.ir.tables[tid.0].name;
                let cv = self.table_path(*tid, "Verified");
                let s = safety(&format!(
                    "`{n}::verify` followed this same edge (`{off_s}`), ran `{cn}::from_ref` on the target and verified it as `{cn}`; `{cn}Verified` wraps exactly `{cn}` (`from_verified` contract, condition 1)."
                ));
                if req {
                    unsafe_accessor(
                        id,
                        res(quote!(#cv<'a>)),
                        quote! {
                            let t = self.0.0.table(#m::#off)?.ok_or(#missing)?;
                            #s
                            let v = unsafe { <#cv<'a> as ::graven::rt::FromVerified<'a>>::from_verified(t) };
                            ::core::result::Result::Ok(v)
                        },
                    )
                } else {
                    unsafe_accessor(
                        id,
                        res(opt(quote!(#cv<'a>))),
                        quote! {
                            let ::core::option::Option::Some(t) = self.0.0.table(#m::#off)? else {
                                return ::core::result::Result::Ok(::core::option::Option::None);
                            };
                            #s
                            let v = unsafe { <#cv<'a> as ::graven::rt::FromVerified<'a>>::from_verified(t) };
                            ::core::result::Result::Ok(::core::option::Option::Some(v))
                        },
                    )
                }
            }
            Ty::Union(uid) => {
                let u = &self.ir.unions[uid.0];
                let uv = self.union_path(*uid, "Verified");
                let s = safety(&format!(
                    "`{n}::verify` followed this same edge (`{off_s}`) for every tag known to `{}` and verified the target as that variant's table; the matching `*Verified` wraps exactly that table type (`from_verified` contract, condition 1). Unknown tags are not descended into (condition 3).",
                    u.name
                ));
                let arms: Vec<TokenStream> = u
                    .variants
                    .iter()
                    .map(|(v, tag, tid)| {
                        let v = ident(v);
                        let tag = lit_u16(*tag);
                        let cv = self.table_path(*tid, "Verified");
                        quote! {
                            ::core::option::Option::Some((#tag, t)) => {
                                #s
                                let v = unsafe { <#cv<'a> as ::graven::rt::FromVerified<'a>>::from_verified(t) };
                                ::core::option::Option::Some(#uv::#v(v))
                            }
                        }
                    })
                    .collect();
                let body = quote! {
                    let v = match self.0.0.union(#m::#off)? {
                        #(#arms)*
                        ::core::option::Option::Some((tag, _)) => ::core::option::Option::Some(#uv::Unknown(tag)),
                        ::core::option::Option::None => ::core::option::Option::None,
                    };
                };
                if req {
                    unsafe_accessor(
                        id,
                        res(quote!(#uv<'a>)),
                        quote! {
                            #body
                            v.ok_or(#missing)
                        },
                    )
                } else {
                    unsafe_accessor(
                        id,
                        res(opt(quote!(#uv<'a>))),
                        quote! {
                            #body
                            ::core::result::Result::Ok(v)
                        },
                    )
                }
            }
            // Inline values, bytes, `[bytes]` and scalar lists: the checked accessor reads
            // the same thing without any validation to skip.
            _ => {
                let ret = self.reader_ret(f);
                quote! {
                    #[inline]
                    pub fn #id(&self) -> #ret {
                        self.0.#id()
                    }
                }
            }
        }
    }

    /// Return type of the checked accessor for the fields the view delegates.
    fn reader_ret(&self, f: &F<'_>) -> TokenStream {
        let req = matches!(f.presence, Presence::Required);
        let res = |ty: TokenStream| quote!(::core::result::Result<#ty, ::graven::Error>);
        let opt = |ty: TokenStream| quote!(::core::option::Option<#ty>);
        match f.ty {
            Ty::Bool => match f.presence {
                Presence::Optional => opt(quote!(bool)),
                _ => quote!(bool),
            },
            Ty::Scalar(_) | Ty::Enum(_) | Ty::Struct(_) => {
                let rty = self.inline_ty(f.ty);
                match f.presence {
                    Presence::Optional => opt(rty),
                    _ => rty,
                }
            }
            Ty::Bytes if req => res(quote!(&'a [u8])),
            Ty::Bytes => res(opt(quote!(&'a [u8]))),
            Ty::List(elem) => {
                let ty = match &**elem {
                    Ty::Bytes => quote!(::graven::StrList<'a>),
                    inline => {
                        let ety = self.inline_ty(inline);
                        quote!(::graven::ScalarList<'a, #ety>)
                    }
                };
                if req { res(ty) } else { res(opt(ty)) }
            }
            _ => unreachable!("reader_ret: field without delegation"),
        }
    }
}

fn unsafe_accessor(id: &Ident, ret: TokenStream, body: TokenStream) -> TokenStream {
    quote! {
        #[allow(unsafe_code)]
        #[inline]
        pub fn #id(&self) -> #ret {
            #body
        }
    }
}
