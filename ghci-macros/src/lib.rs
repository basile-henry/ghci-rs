use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::parse::{Parse, ParseStream};
use syn::{parse_macro_input, Expr, Ident, Token};

struct Binding {
    haskell_name: Ident,
    rust_expr: Expr,
}

impl Parse for Binding {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let haskell_name: Ident = input.parse()?;
        let rust_expr = if input.peek(Token![=]) {
            input.parse::<Token![=]>()?;
            // Use Expr::parse_without_eager_brace to avoid consuming past commas in
            // brackets. However, Expr parsing is still greedy, so wrap non-trivial
            // expressions in parentheses: [z = (1 + 2)]
            Expr::parse_without_eager_brace(input)?
        } else {
            syn::parse_quote!(#haskell_name)
        };
        Ok(Binding {
            haskell_name,
            rust_expr,
        })
    }
}

struct GhciMacroInput {
    ghci_expr: Expr,
    bindings: Vec<Binding>,
    body: TokenStream2,
}

impl Parse for GhciMacroInput {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let ghci_expr: Expr = input.parse()?;
        // Normalize: wrap with &mut if not already a mutable reference, so both
        // `ghci` and `&mut ghci` are accepted in addition to `&mut *ghci`.
        let ghci_expr = match &ghci_expr {
            Expr::Reference(r) if r.mutability.is_some() => ghci_expr,
            _ => syn::parse_quote!(&mut #ghci_expr),
        };
        input.parse::<Token![,]>()?;

        let bindings = if input.peek(syn::token::Bracket) {
            let content;
            syn::bracketed!(content in input);
            let bindings = content.parse_terminated(Binding::parse, Token![,])?;
            bindings.into_iter().collect()
        } else {
            Vec::new()
        };

        let body_content;
        syn::braced!(body_content in input);
        let body: TokenStream2 = body_content.parse()?;

        Ok(GhciMacroInput {
            ghci_expr,
            bindings,
            body,
        })
    }
}

fn expand_ghci(input: GhciMacroInput) -> TokenStream2 {
    let ghci_expr = &input.ghci_expr;
    let body_str = input.body.to_string();

    if input.bindings.is_empty() {
        quote! {
            {
                ::ghci::Ghci::eval_as(#ghci_expr, #body_str)
            }
        }
    } else {
        let mut stmts = Vec::new();
        stmts.push(quote! {
            let mut __ghci_expr = ::std::string::String::new();
        });

        for (i, binding) in input.bindings.iter().enumerate() {
            let name = binding.haskell_name.to_string();
            let rust_expr = &binding.rust_expr;
            if i > 0 {
                stmts.push(quote! {
                    __ghci_expr.push_str(" in ");
                });
            }
            stmts.push(quote! {
                __ghci_expr.push_str(concat!("let ", #name, " = "));
                __ghci_expr.push_str(&::ghci::ToHaskell::to_haskell(&#rust_expr));
            });
        }

        stmts.push(quote! {
            __ghci_expr.push_str(concat!(" in ", #body_str));
        });

        quote! {
            {
                #(#stmts)*
                ::ghci::Ghci::eval_as(#ghci_expr, &__ghci_expr)
            }
        }
    }
}

/// Evaluate an inline Haskell expression via a GHCi session.
///
/// # Syntax
///
/// ```rust,ignore
/// // No bindings:
/// ghci!(ghci, { 1 + 2 })
///
/// // With bindings (Rust vars injected as Haskell let-bindings):
/// ghci!(ghci, [x, y] { x ++ " " ++ y })
///
/// // Expression bindings:
/// ghci!(ghci, [z = some_expr] { z * 10 })
/// ```
///
/// The first argument is always taken by mutable reference internally. Passing
/// `ghci`, `&mut ghci`, or `&mut *ghci` (for a dereffed smart pointer) are all
/// accepted.
///
/// Returns `Result<T>` where `T` is inferred from context (via `FromHaskell`).
///
/// # Known limitations
///
/// - Backtick application (`` x `div` y ``) is not supported — use `div x y` instead
/// - Primed identifiers (`x'`, `f'`) are not supported — `'` starts a lifetime/char token in Rust
/// - Haskell-specific escape sequences in string literals that differ from Rust
#[proc_macro]
pub fn ghci(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as GhciMacroInput);
    expand_ghci(input).into()
}
