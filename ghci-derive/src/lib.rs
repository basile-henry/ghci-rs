use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::{parse_macro_input, Data, DeriveInput, Fields, GenericParam, LitStr};

#[derive(Clone, Copy, PartialEq)]
enum Style {
    App,
    Record,
}

struct HaskellAttrs {
    name: Option<String>,
    transparent: bool,
    style: Option<Style>,
    skip: bool,
    bound_to: Option<String>,
    bound_from: Option<String>,
}

fn parse_haskell_attrs(attrs: &[syn::Attribute]) -> HaskellAttrs {
    let mut name = None;
    let mut transparent = false;
    let mut style = None;
    let mut skip = false;
    let mut bound_to = None;
    let mut bound_from = None;
    for attr in attrs {
        if attr.path().is_ident("haskell") {
            let _ = attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("name") {
                    let value = meta.value()?;
                    let s: LitStr = value.parse()?;
                    name = Some(s.value());
                } else if meta.path.is_ident("transparent") {
                    transparent = true;
                } else if meta.path.is_ident("style") {
                    let value = meta.value()?;
                    let s: LitStr = value.parse()?;
                    match s.value().as_str() {
                        "app" => style = Some(Style::App),
                        "record" => style = Some(Style::Record),
                        _ => {
                            return Err(meta.error("expected \"app\" or \"record\""));
                        }
                    }
                } else if meta.path.is_ident("skip") {
                    skip = true;
                } else if meta.path.is_ident("bound") {
                    meta.parse_nested_meta(|inner| {
                        if inner.path.is_ident("ToHaskell") {
                            let value = inner.value()?;
                            let s: LitStr = value.parse()?;
                            bound_to = Some(s.value());
                        } else if inner.path.is_ident("FromHaskell") {
                            let value = inner.value()?;
                            let s: LitStr = value.parse()?;
                            bound_from = Some(s.value());
                        } else {
                            return Err(inner.error("expected `ToHaskell` or `FromHaskell`"));
                        }
                        Ok(())
                    })?;
                }
                Ok(())
            });
        }
    }
    HaskellAttrs {
        name,
        transparent,
        style,
        skip,
        bound_to,
        bound_from,
    }
}

fn add_trait_bounds(mut generics: syn::Generics, trait_path: &TokenStream2) -> syn::Generics {
    for param in &mut generics.params {
        if let GenericParam::Type(type_param) = param {
            type_param.bounds.push(syn::parse_quote!(#trait_path));
        }
    }
    generics
}

fn apply_custom_bounds(
    generics: &syn::Generics,
    bound_str: &str,
) -> syn::Result<(TokenStream2, TokenStream2, TokenStream2)> {
    let (impl_generics, ty_generics, _) = generics.split_for_impl();
    let impl_generics = quote! { #impl_generics };
    let ty_generics = quote! { #ty_generics };
    let predicates: TokenStream2 = bound_str.parse().map_err(|e| {
        syn::Error::new(
            proc_macro2::Span::call_site(),
            format!("failed to parse bound: {e}"),
        )
    })?;
    let where_clause = quote! { where #predicates };
    Ok((impl_generics, ty_generics, where_clause))
}

/// Resolve the effective style for named fields: explicit style wins, otherwise Record.
fn resolve_style(style: Option<Style>) -> Style {
    style.unwrap_or(Style::Record)
}

// ── ToHaskell ────────────────────────────────────────────────────────

/// Derive `ToHaskell` for a struct or enum.
///
/// # Container attributes (`#[haskell(...)]`)
///
/// - `name = "HaskellName"` — override the Haskell constructor/type name
/// - `transparent` — single-field struct: delegate to the inner field's impl
/// - `style = "app"` / `style = "record"` — force app or record syntax for
///   named-field structs; sets the default for all variants of an enum
/// - `bound(ToHaskell = "...")` — override auto-generated trait bounds
///
/// # Variant attributes (`#[haskell(...)]`)
///
/// - `name = "haskell_name"` — override the Haskell constructor name
/// - `style = "app"` / `style = "record"` — override container default
///
/// # Field attributes (`#[haskell(...)]`)
///
/// - `name = "haskell_name"` — override the Haskell field name
/// - `skip` — omit this field from serialization
#[proc_macro_derive(ToHaskell, attributes(haskell))]
pub fn derive_to_haskell(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match expand_to_haskell(input) {
        Ok(ts) => ts.into(),
        Err(e) => e.to_compile_error().into(),
    }
}

fn expand_to_haskell(input: DeriveInput) -> syn::Result<TokenStream2> {
    let attrs = parse_haskell_attrs(&input.attrs);
    let ident = &input.ident;
    let haskell_name = attrs.name.unwrap_or_else(|| ident.to_string());
    let container_style = attrs.style;

    let (impl_generics, ty_generics, where_clause) = if let Some(ref bound) = attrs.bound_to {
        let r = apply_custom_bounds(&input.generics, bound)?;
        (r.0, r.1, r.2)
    } else {
        let trait_path: TokenStream2 = quote!(::ghci::ToHaskell);
        let generics = add_trait_bounds(input.generics.clone(), &trait_path);
        let (ig, tg, wc) = generics.split_for_impl();
        (quote! { #ig }, quote! { #tg }, quote! { #wc })
    };

    let body = match &input.data {
        Data::Struct(s) => {
            if attrs.transparent {
                expand_to_haskell_transparent_struct(&s.fields)?
            } else {
                expand_to_haskell_struct(&s.fields, &haskell_name, container_style)?
            }
        }
        Data::Enum(e) => {
            if attrs.transparent {
                return Err(syn::Error::new_spanned(
                    ident,
                    "`#[haskell(transparent)]` is not supported on enums",
                ));
            }
            let arms = e
                .variants
                .iter()
                .map(|v| {
                    let variant_ident = &v.ident;
                    let variant_attrs = parse_haskell_attrs(&v.attrs);
                    let variant_name = variant_attrs
                        .name
                        .unwrap_or_else(|| variant_ident.to_string());
                    let variant_style = variant_attrs.style.or(container_style);
                    expand_to_haskell_variant(
                        ident,
                        variant_ident,
                        &v.fields,
                        &variant_name,
                        variant_style,
                    )
                })
                .collect::<syn::Result<Vec<_>>>()?;
            quote! {
                fn write_haskell(&self, buf: &mut impl ::std::fmt::Write) -> ::std::fmt::Result {
                    match self {
                        #(#arms)*
                    }
                }
            }
        }
        Data::Union(u) => {
            return Err(syn::Error::new_spanned(
                u.union_token,
                "`ToHaskell` cannot be derived for unions",
            ));
        }
    };

    Ok(quote! {
        #[automatically_derived]
        impl #impl_generics ::ghci::ToHaskell for #ident #ty_generics #where_clause {
            #body
        }
    })
}

fn expand_to_haskell_struct(
    fields: &Fields,
    haskell_name: &str,
    style: Option<Style>,
) -> syn::Result<TokenStream2> {
    match fields {
        Fields::Named(named) => {
            let effective_style = resolve_style(style);
            if effective_style == Style::App {
                let arg_calls = named
                    .named
                    .iter()
                    .filter(|f| !parse_haskell_attrs(&f.attrs).skip)
                    .map(|f| {
                        let field_ident = f.ident.as_ref().unwrap();
                        quote! { .arg(&self.#field_ident) }
                    });
                Ok(quote! {
                    fn write_haskell(&self, buf: &mut impl ::std::fmt::Write) -> ::std::fmt::Result {
                        ::ghci::haskell::app(buf, #haskell_name)
                            #(#arg_calls)*
                            .finish()
                    }
                })
            } else {
                let field_calls = named
                    .named
                    .iter()
                    .filter(|f| !parse_haskell_attrs(&f.attrs).skip)
                    .map(|f| {
                        let field_ident = f.ident.as_ref().unwrap();
                        let fattrs = parse_haskell_attrs(&f.attrs);
                        let field_name = fattrs.name.unwrap_or_else(|| field_ident.to_string());
                        quote! { .field(#field_name, &self.#field_ident) }
                    });
                Ok(quote! {
                    fn write_haskell(&self, buf: &mut impl ::std::fmt::Write) -> ::std::fmt::Result {
                        ::ghci::haskell::record(buf, #haskell_name)
                            #(#field_calls)*
                            .finish()
                    }
                })
            }
        }
        Fields::Unnamed(unnamed) => {
            let arg_calls = unnamed.unnamed.iter().enumerate().map(|(i, _)| {
                let index = syn::Index::from(i);
                quote! { .arg(&self.#index) }
            });
            Ok(quote! {
                fn write_haskell(&self, buf: &mut impl ::std::fmt::Write) -> ::std::fmt::Result {
                    ::ghci::haskell::app(buf, #haskell_name)
                        #(#arg_calls)*
                        .finish()
                }
            })
        }
        Fields::Unit => Ok(quote! {
            fn write_haskell(&self, buf: &mut impl ::std::fmt::Write) -> ::std::fmt::Result {
                buf.write_str(#haskell_name)
            }
        }),
    }
}

fn expand_to_haskell_transparent_struct(fields: &Fields) -> syn::Result<TokenStream2> {
    match fields {
        Fields::Named(named) if named.named.len() == 1 => {
            let field_ident = named.named[0].ident.as_ref().unwrap();
            Ok(quote! {
                fn write_haskell(&self, buf: &mut impl ::std::fmt::Write) -> ::std::fmt::Result {
                    ::ghci::ToHaskell::write_haskell(&self.#field_ident, buf)
                }
            })
        }
        Fields::Unnamed(unnamed) if unnamed.unnamed.len() == 1 => Ok(quote! {
            fn write_haskell(&self, buf: &mut impl ::std::fmt::Write) -> ::std::fmt::Result {
                ::ghci::ToHaskell::write_haskell(&self.0, buf)
            }
        }),
        _ => Err(syn::Error::new(
            proc_macro2::Span::call_site(),
            "`#[haskell(transparent)]` requires exactly one field",
        )),
    }
}

fn expand_to_haskell_variant(
    enum_ident: &syn::Ident,
    variant_ident: &syn::Ident,
    fields: &Fields,
    haskell_name: &str,
    style: Option<Style>,
) -> syn::Result<TokenStream2> {
    match fields {
        Fields::Named(named) => {
            let effective_style = resolve_style(style);
            let all_field_idents: Vec<_> = named
                .named
                .iter()
                .map(|f| f.ident.as_ref().unwrap())
                .collect();
            if effective_style == Style::App {
                let non_skipped: Vec<_> = named
                    .named
                    .iter()
                    .filter(|f| !parse_haskell_attrs(&f.attrs).skip)
                    .map(|f| f.ident.as_ref().unwrap())
                    .collect();
                Ok(quote! {
                    #enum_ident::#variant_ident { #(#all_field_idents),* } => {
                        ::ghci::haskell::app(buf, #haskell_name)
                            #(.arg(#non_skipped))*
                            .finish()
                    }
                })
            } else {
                let field_stmts: Vec<_> = named
                    .named
                    .iter()
                    .filter(|f| !parse_haskell_attrs(&f.attrs).skip)
                    .map(|f| {
                        let fattrs = parse_haskell_attrs(&f.attrs);
                        let field_ident = f.ident.as_ref().unwrap();
                        let field_name = fattrs.name.unwrap_or_else(|| field_ident.to_string());
                        (field_name, field_ident)
                    })
                    .collect();
                let field_names: Vec<_> = field_stmts.iter().map(|(n, _)| n.clone()).collect();
                let field_idents: Vec<_> = field_stmts.iter().map(|(_, i)| *i).collect();
                Ok(quote! {
                    #enum_ident::#variant_ident { #(#all_field_idents),* } => {
                        ::ghci::haskell::record(buf, #haskell_name)
                            #(.field(#field_names, #field_idents))*
                            .finish()
                    }
                })
            }
        }
        Fields::Unnamed(unnamed) => {
            let vars: Vec<_> = (0..unnamed.unnamed.len())
                .map(|i| format_ident!("__f{}", i))
                .collect();
            Ok(quote! {
                #enum_ident::#variant_ident(#(#vars),*) => {
                    ::ghci::haskell::app(buf, #haskell_name)
                        #(.arg(#vars))*
                        .finish()
                }
            })
        }
        Fields::Unit => Ok(quote! {
            #enum_ident::#variant_ident => buf.write_str(#haskell_name),
        }),
    }
}

// ── FromHaskell ──────────────────────────────────────────────────────

/// Derive `FromHaskell` for a struct or enum.
///
/// # Container attributes (`#[haskell(...)]`)
///
/// - `name = "HaskellName"` — override the Haskell constructor/type name
/// - `transparent` — single-field struct: delegate to the inner field's impl
/// - `style = "app"` / `style = "record"` — force app or record syntax for
///   named-field structs; sets the default for all variants of an enum
/// - `bound(FromHaskell = "...")` — override auto-generated trait bounds
///
/// # Variant attributes (`#[haskell(...)]`)
///
/// - `name = "haskell_name"` — override the Haskell constructor name
/// - `style = "app"` / `style = "record"` — override container default
///
/// # Field attributes (`#[haskell(...)]`)
///
/// - `name = "haskell_name"` — override the Haskell field name
/// - `skip` — initialize with `Default::default()` instead of parsing
///
/// For enums, each variant is tried in declaration order; the first successful
/// parse wins.
#[proc_macro_derive(FromHaskell, attributes(haskell))]
pub fn derive_from_haskell(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match expand_from_haskell(input) {
        Ok(ts) => ts.into(),
        Err(e) => e.to_compile_error().into(),
    }
}

fn expand_from_haskell(input: DeriveInput) -> syn::Result<TokenStream2> {
    let attrs = parse_haskell_attrs(&input.attrs);
    let ident = &input.ident;
    let haskell_name = attrs.name.unwrap_or_else(|| ident.to_string());
    let container_style = attrs.style;

    let (impl_generics, ty_generics, where_clause) = if let Some(ref bound) = attrs.bound_from {
        let r = apply_custom_bounds(&input.generics, bound)?;
        (r.0, r.1, r.2)
    } else {
        let trait_path: TokenStream2 = quote!(::ghci::FromHaskell);
        let generics = add_trait_bounds(input.generics.clone(), &trait_path);
        let (ig, tg, wc) = generics.split_for_impl();
        (quote! { #ig }, quote! { #tg }, quote! { #wc })
    };

    let body = match &input.data {
        Data::Struct(s) => {
            if attrs.transparent {
                expand_from_haskell_transparent_struct(&s.fields)?
            } else {
                expand_from_haskell_struct(&s.fields, &haskell_name, container_style)?
            }
        }
        Data::Enum(e) => {
            if attrs.transparent {
                return Err(syn::Error::new_spanned(
                    ident,
                    "`#[haskell(transparent)]` is not supported on enums",
                ));
            }
            let type_name = ident.to_string();
            let tries = e
                .variants
                .iter()
                .map(|v| {
                    let variant_ident = &v.ident;
                    let variant_attrs = parse_haskell_attrs(&v.attrs);
                    let variant_name = variant_attrs
                        .name
                        .unwrap_or_else(|| variant_ident.to_string());
                    let variant_style = variant_attrs.style.or(container_style);
                    expand_from_haskell_variant(
                        ident,
                        variant_ident,
                        &v.fields,
                        &variant_name,
                        variant_style,
                    )
                })
                .collect::<syn::Result<Vec<_>>>()?;
            quote! {
                fn parse_haskell(input: &str) -> ::core::result::Result<(Self, &str), ::ghci::HaskellParseError> {
                    #(#tries)*
                    ::core::result::Result::Err(::ghci::HaskellParseError::ParseError {
                        message: ::std::format!("failed to parse {} from {:?}", #type_name, input),
                    })
                }
            }
        }
        Data::Union(u) => {
            return Err(syn::Error::new_spanned(
                u.union_token,
                "`FromHaskell` cannot be derived for unions",
            ));
        }
    };

    Ok(quote! {
        #[automatically_derived]
        impl #impl_generics ::ghci::FromHaskell for #ident #ty_generics #where_clause {
            #body
        }
    })
}

fn expand_from_haskell_struct(
    fields: &Fields,
    haskell_name: &str,
    style: Option<Style>,
) -> syn::Result<TokenStream2> {
    match fields {
        Fields::Named(named) => {
            let effective_style = resolve_style(style);
            if effective_style == Style::App {
                let field_inits: Vec<_> = named
                    .named
                    .iter()
                    .map(|f| {
                        let field_ident = f.ident.as_ref().unwrap();
                        let fattrs = parse_haskell_attrs(&f.attrs);
                        if fattrs.skip {
                            quote! { #field_ident: ::core::default::Default::default() }
                        } else {
                            quote! { #field_ident: __p.arg()? }
                        }
                    })
                    .collect();
                Ok(quote! {
                    fn parse_haskell(input: &str) -> ::core::result::Result<(Self, &str), ::ghci::HaskellParseError> {
                        let mut __p = ::ghci::haskell::parse_app(#haskell_name, input)?;
                        let __val = Self { #(#field_inits),* };
                        let rest = __p.finish()?;
                        ::core::result::Result::Ok((__val, rest))
                    }
                })
            } else {
                let field_inits = named.named.iter().map(|f| {
                    let field_ident = f.ident.as_ref().unwrap();
                    let fattrs = parse_haskell_attrs(&f.attrs);
                    if fattrs.skip {
                        quote! { #field_ident: ::core::default::Default::default() }
                    } else {
                        let field_name = fattrs.name.unwrap_or_else(|| field_ident.to_string());
                        quote! { #field_ident: rec.field(#field_name)? }
                    }
                });
                Ok(quote! {
                    fn parse_haskell(input: &str) -> ::core::result::Result<(Self, &str), ::ghci::HaskellParseError> {
                        let (rec, rest) = ::ghci::haskell::parse_record(#haskell_name, input)?;
                        ::core::result::Result::Ok((Self { #(#field_inits),* }, rest))
                    }
                })
            }
        }
        Fields::Unnamed(unnamed) => {
            let vars: Vec<_> = (0..unnamed.unnamed.len())
                .map(|i| format_ident!("__f{}", i))
                .collect();
            Ok(quote! {
                fn parse_haskell(input: &str) -> ::core::result::Result<(Self, &str), ::ghci::HaskellParseError> {
                    let mut __p = ::ghci::haskell::parse_app(#haskell_name, input)?;
                    #(let #vars = __p.arg()?;)*
                    let rest = __p.finish()?;
                    ::core::result::Result::Ok((Self(#(#vars),*), rest))
                }
            })
        }
        Fields::Unit => Ok(quote! {
            fn parse_haskell(input: &str) -> ::core::result::Result<(Self, &str), ::ghci::HaskellParseError> {
                let mut __p = ::ghci::haskell::parse_app(#haskell_name, input)?;
                let rest = __p.finish()?;
                ::core::result::Result::Ok((Self, rest))
            }
        }),
    }
}

fn expand_from_haskell_transparent_struct(fields: &Fields) -> syn::Result<TokenStream2> {
    match fields {
        Fields::Named(named) if named.named.len() == 1 => {
            let field_ident = named.named[0].ident.as_ref().unwrap();
            Ok(quote! {
                fn parse_haskell(input: &str) -> ::core::result::Result<(Self, &str), ::ghci::HaskellParseError> {
                    let (val, rest) = ::ghci::FromHaskell::parse_haskell(input)?;
                    ::core::result::Result::Ok((Self { #field_ident: val }, rest))
                }
            })
        }
        Fields::Unnamed(unnamed) if unnamed.unnamed.len() == 1 => Ok(quote! {
            fn parse_haskell(input: &str) -> ::core::result::Result<(Self, &str), ::ghci::HaskellParseError> {
                let (val, rest) = ::ghci::FromHaskell::parse_haskell(input)?;
                ::core::result::Result::Ok((Self(val), rest))
            }
        }),
        _ => Err(syn::Error::new(
            proc_macro2::Span::call_site(),
            "`#[haskell(transparent)]` requires exactly one field",
        )),
    }
}

fn expand_from_haskell_variant(
    enum_ident: &syn::Ident,
    variant_ident: &syn::Ident,
    fields: &Fields,
    haskell_name: &str,
    style: Option<Style>,
) -> syn::Result<TokenStream2> {
    match fields {
        Fields::Named(named) => {
            let effective_style = resolve_style(style);
            if effective_style == Style::App {
                let field_inits: Vec<_> = named
                    .named
                    .iter()
                    .map(|f| {
                        let field_ident = f.ident.as_ref().unwrap();
                        let fattrs = parse_haskell_attrs(&f.attrs);
                        if fattrs.skip {
                            quote! { #field_ident: ::core::default::Default::default() }
                        } else {
                            quote! { #field_ident: __p.arg()? }
                        }
                    })
                    .collect();
                Ok(quote! {
                    if let ::core::result::Result::Ok(mut __p) = ::ghci::haskell::parse_app(#haskell_name, input) {
                        let __val = #enum_ident::#variant_ident { #(#field_inits),* };
                        let rest = __p.finish()?;
                        return ::core::result::Result::Ok((__val, rest));
                    }
                })
            } else {
                let field_inits: Vec<_> = named
                    .named
                    .iter()
                    .map(|f| {
                        let field_ident = f.ident.as_ref().unwrap();
                        let fattrs = parse_haskell_attrs(&f.attrs);
                        if fattrs.skip {
                            quote! { #field_ident: ::core::default::Default::default() }
                        } else {
                            let field_name = fattrs
                                .name
                                .unwrap_or_else(|| f.ident.as_ref().unwrap().to_string());
                            quote! { #field_ident: rec.field(#field_name)? }
                        }
                    })
                    .collect();
                Ok(quote! {
                    if let ::core::result::Result::Ok((rec, rest)) = ::ghci::haskell::parse_record(#haskell_name, input) {
                        return ::core::result::Result::Ok((
                            #enum_ident::#variant_ident { #(#field_inits),* },
                            rest,
                        ));
                    }
                })
            }
        }
        Fields::Unnamed(unnamed) => {
            let vars: Vec<_> = (0..unnamed.unnamed.len())
                .map(|i| format_ident!("__f{}", i))
                .collect();
            Ok(quote! {
                if let ::core::result::Result::Ok(mut __p) = ::ghci::haskell::parse_app(#haskell_name, input) {
                    #(let #vars = __p.arg()?;)*
                    let rest = __p.finish()?;
                    return ::core::result::Result::Ok((#enum_ident::#variant_ident(#(#vars),*), rest));
                }
            })
        }
        Fields::Unit => Ok(quote! {
            if let ::core::result::Result::Ok(mut __p) = ::ghci::haskell::parse_app(#haskell_name, input) {
                let rest = __p.finish()?;
                return ::core::result::Result::Ok((#enum_ident::#variant_ident, rest));
            }
        }),
    }
}
