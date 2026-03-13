//! Procedural macro that powers `alien-error`.
//!
//! Usage:
//! ```rust
//! use alien_error::AlienErrorData;
//! #[derive(Debug, AlienErrorData)]
//! enum MyError {
//!     #[error(code = "SOMETHING_WRONG", message = "something went wrong", retryable = "false", internal = "false", http_status_code = 420)]
//!     Oops,
//! }
//! ```
//! The `error(...)` attribute supplies compile-time metadata:
//! • `code`              – short machine friendly identifier (defaults to variant name).
//! • `message`           – human-readable error message with field interpolation.
//! • `retryable`         – flag set to `true` if the operation can be retried.
//! • `internal`          – flag set to `true` if this error should not be exposed.
//! • `http_status_code`  – HTTP status code for this error (defaults to 500).
//!
//! The macro also auto-implements `AlienErrorData` including a `context()` method
//! that serialises variant fields into a JSON map for diagnostic payloads.

use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, Attribute, Data, DeriveInput};

#[proc_macro_derive(AlienErrorData, attributes(error))]
pub fn derive_alien_error(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = input.ident;

    let (
        code_match_arms,
        retryable_match_arms,
        internal_match_arms,
        http_status_code_match_arms,
        context_match_arms,
        message_match_arms,
        retryable_inherit_match_arms,
        internal_inherit_match_arms,
        http_status_code_inherit_match_arms,
    ) = match input.data {
        Data::Enum(ref data_enum) => {
            let mut code_arms = Vec::new();
            let mut retryable_arms = Vec::new();
            let mut internal_arms = Vec::new();
            let mut http_status_code_arms = Vec::new();
            let mut context_arms = Vec::new();
            let mut message_arms = Vec::new();
            let mut retryable_inherit_arms = Vec::new();
            let mut internal_inherit_arms = Vec::new();
            let mut http_status_code_inherit_arms = Vec::new();

            for variant in &data_enum.variants {
                let ident = &variant.ident;

                let (
                    code_val,
                    retryable_val,
                    internal_val,
                    http_status_code_val,
                    message_val,
                    retryable_inherit,
                    internal_inherit,
                    http_status_code_inherit,
                ) = parse_error_attrs(&variant.attrs, ident.to_string());

                let matcher = if variant.fields.is_empty() {
                    quote! { #name::#ident }
                } else {
                    quote! { #name::#ident { .. } }
                };

                let code_lit = code_val;
                let retry_bool = retryable_val;
                let internal_bool = internal_val;
                let http_status_code_u16 = http_status_code_val;

                code_arms.push(quote! { #matcher => #code_lit });
                retryable_arms.push(quote! { #matcher => #retry_bool });
                internal_arms.push(quote! { #matcher => #internal_bool });
                http_status_code_arms.push(quote! { #matcher => #http_status_code_u16 });
                retryable_inherit_arms.push(quote! { #matcher => #retryable_inherit });
                internal_inherit_arms.push(quote! { #matcher => #internal_inherit });
                http_status_code_inherit_arms
                    .push(quote! { #matcher => #http_status_code_inherit });

                // Generate message arm with field interpolation
                match &variant.fields {
                    syn::Fields::Named(fields_named) if !fields_named.named.is_empty() => {
                        // SAFETY: Named fields are guaranteed to have identifiers.
                        // The Option exists only for compatibility with tuple struct fields.
                        let field_idents: Vec<_> = fields_named
                            .named
                            .iter()
                            .map(|f| f.ident.as_ref().unwrap())
                            .collect();
                        let matcher = quote! { #name::#ident { #( ref #field_idents ),* } };

                        // Generate message with field interpolation
                        let interpolated_message =
                            generate_message_interpolation(&message_val, &field_idents);
                        message_arms.push(quote! { #matcher => #interpolated_message });

                        context_arms.push(quote! { #matcher => {
                            let mut map = serde_json::Map::new();
                            #( map.insert(
                                stringify!(#field_idents).to_string(), 
                                serde_json::to_value(#field_idents)
                                    .expect(&format!("Failed to serialize field '{}' to JSON. This field must implement Serialize correctly.", stringify!(#field_idents)))
                            ); )*
                            Some(serde_json::Value::Object(map))
                        } });
                    }
                    _ => {
                        let matcher = if variant.fields.is_empty() {
                            quote! { #name::#ident }
                        } else {
                            quote! { #name::#ident { .. } }
                        };
                        message_arms.push(quote! { #matcher => #message_val.to_string() });
                        context_arms.push(quote! { #matcher => None });
                    }
                }
            }
            (
                code_arms,
                retryable_arms,
                internal_arms,
                http_status_code_arms,
                context_arms,
                message_arms,
                retryable_inherit_arms,
                internal_inherit_arms,
                http_status_code_inherit_arms,
            )
        }
        _ => {
            return quote! { compile_error!("AlienErrorData can only be derived for enums"); }
                .into();
        }
    };

    let expanded = quote! {
        impl alien_error::AlienErrorData for #name {
            fn code(&self) -> &'static str {
                match self {
                    #(#code_match_arms),*
                }
            }
            fn retryable(&self) -> bool {
                match self {
                    #(#retryable_match_arms),*
                }
            }
            fn internal(&self) -> bool {
                match self {
                    #(#internal_match_arms),*
                }
            }
            fn http_status_code(&self) -> u16 {
                match self {
                    #(#http_status_code_match_arms),*
                }
            }
            fn message(&self) -> String {
                match self {
                    #(#message_match_arms),*
                }
            }
            fn context(&self) -> Option<serde_json::Value> {
                match self {
                    #(#context_match_arms),*
                }
            }
            fn retryable_inherit(&self) -> Option<bool> {
                match self {
                    #(#retryable_inherit_match_arms),*
                }
            }
            fn internal_inherit(&self) -> Option<bool> {
                match self {
                    #(#internal_inherit_match_arms),*
                }
            }
            fn http_status_code_inherit(&self) -> Option<u16> {
                match self {
                    #(#http_status_code_inherit_match_arms),*
                }
            }
        }

        impl std::fmt::Display for #name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", self.message())
            }
        }
    };

    TokenStream::from(expanded)
}

fn parse_error_attrs(
    attrs: &[Attribute],
    default_code: String,
) -> (
    proc_macro2::TokenStream,
    proc_macro2::TokenStream,
    proc_macro2::TokenStream,
    proc_macro2::TokenStream,
    String,
    proc_macro2::TokenStream,
    proc_macro2::TokenStream,
    proc_macro2::TokenStream,
) {
    let mut code = default_code;
    let mut retryable: Option<String> = None;
    let mut internal: Option<String> = None;
    let mut http_status_code: Option<String> = None;
    let mut message: Option<String> = None;

    for attr in attrs {
        if !attr.path().is_ident("error") {
            continue;
        }
        if let Err(e) = attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("code") {
                let lit: syn::LitStr = meta.value()?.parse()?;
                code = lit.value();
                Ok(())
            } else if meta.path.is_ident("retryable") {
                let lit: syn::LitStr = meta.value()?.parse()?;
                retryable = Some(lit.value());
                Ok(())
            } else if meta.path.is_ident("internal") {
                let lit: syn::LitStr = meta.value()?.parse()?;
                internal = Some(lit.value());
                Ok(())
            } else if meta.path.is_ident("http_status_code") {
                // Parse the value as a literal (either string or int)
                let value = meta.value()?;

                // Try to parse as a literal expression
                let lit: syn::Lit = value.parse()?;

                match lit {
                    syn::Lit::Str(lit_str) => {
                        // String literal like "inherit" or "404"
                        http_status_code = Some(lit_str.value());
                    }
                    syn::Lit::Int(lit_int) => {
                        // Integer literal like 404
                        let parsed_value = lit_int.base10_parse::<u16>()?;
                        http_status_code = Some(parsed_value.to_string());
                    }
                    _ => {
                        return Err(
                            meta.error("http_status_code must be a string or integer literal")
                        );
                    }
                }
                Ok(())
            } else if meta.path.is_ident("message") {
                let lit: syn::LitStr = meta.value()?.parse()?;
                message = Some(lit.value());
                Ok(())
            } else {
                Err(meta.error("unsupported error attribute key"))
            }
        }) {
            // Re-emit the actual syn error instead of a generic message
            return (
                syn::Error::new(e.span(), e.to_string()).to_compile_error(),
                syn::Error::new(e.span(), e.to_string()).to_compile_error(),
                syn::Error::new(e.span(), e.to_string()).to_compile_error(),
                syn::Error::new(e.span(), e.to_string()).to_compile_error(),
                String::new(),
                syn::Error::new(e.span(), e.to_string()).to_compile_error(),
                syn::Error::new(e.span(), e.to_string()).to_compile_error(),
                syn::Error::new(e.span(), e.to_string()).to_compile_error(),
            );
        }
    }

    // ensure all required fields are specified
    macro_rules! parse_flag {
        ($val:expr,$name:expr) => {
            match $val {
                Some(ref s) if s == "true" => quote! { true },
                Some(ref s) if s == "false" => quote! { false },
                Some(ref s) if s == "inherit" => quote! { false }, // For backward compatibility in the main method
                Some(ref _other) => syn::Error::new(proc_macro2::Span::call_site(), format!("{} must be \"true\", \"false\" or \"inherit\"", $name)).to_compile_error(),
                None => syn::Error::new(proc_macro2::Span::call_site(), format!("{}=\"...\" is required in #[error(...)]", $name)).to_compile_error(),
            }
        };
    }

    // Parse inheritance flags from the same values
    macro_rules! parse_inherit_flag {
        ($val:expr) => {
            match $val {
                Some(ref s) if s == "inherit" => quote! { None },
                Some(ref s) if s == "true" => quote! { Some(true) },
                Some(ref s) if s == "false" => quote! { Some(false) },
                Some(_) => quote! { Some(false) }, // fallback for any other value
                None => syn::Error::new(proc_macro2::Span::call_site(), "flag is required")
                    .to_compile_error(),
            }
        };
    }

    let retry_ts = parse_flag!(retryable.clone(), "retryable");
    let internal_ts = parse_flag!(internal.clone(), "internal");
    let retryable_inherit_ts = parse_inherit_flag!(retryable);
    let internal_inherit_ts = parse_inherit_flag!(internal);

    let code_ts = {
        let lit = syn::LitStr::new(&code, proc_macro2::Span::call_site());
        quote! { #lit }
    };

    // Parse HTTP status code with inheritance support
    let (http_status_code_ts, http_status_code_inherit_ts) = match http_status_code {
        Some(ref s) if s == "inherit" => {
            // When inherit is specified, use 500 as the default but return None for inherit
            (quote! { 500 }, quote! { None })
        }
        Some(ref s) => {
            // Parse as number
            match s.parse::<u16>() {
                Ok(status_code) => (quote! { #status_code }, quote! { Some(#status_code) }),
                Err(_) => (
                    syn::Error::new(
                        proc_macro2::Span::call_site(),
                        "http_status_code must be a number or \"inherit\"",
                    )
                    .to_compile_error(),
                    syn::Error::new(
                        proc_macro2::Span::call_site(),
                        "http_status_code must be a number or \"inherit\"",
                    )
                    .to_compile_error(),
                ),
            }
        }
        None => {
            // Default to 500
            (quote! { 500 }, quote! { Some(500) })
        }
    };

    let message_str = message.unwrap_or_else(|| code.clone());

    (
        code_ts,
        retry_ts,
        internal_ts,
        http_status_code_ts,
        message_str,
        retryable_inherit_ts,
        internal_inherit_ts,
        http_status_code_inherit_ts,
    )
}

fn generate_message_interpolation(
    message_template: &str,
    field_idents: &[&syn::Ident],
) -> proc_macro2::TokenStream {
    // Let Rust's format! macro handle the parsing - just pass the template and fields directly
    // This leverages Rust's built-in format string parsing which handles all cases correctly

    if field_idents.is_empty() {
        quote! { #message_template.to_string() }
    } else {
        // Find which fields are actually used in the message template
        let used_fields: Vec<&syn::Ident> = field_idents
            .iter()
            .filter(|field| {
                let field_name = field.to_string();
                message_template.contains(&format!("{{{}", field_name))
            })
            .cloned()
            .collect();

        if used_fields.is_empty() {
            // No fields are referenced in the template
            quote! { #message_template.to_string() }
        } else {
            // Use named parameters - only pass fields that are actually used
            quote! { format!(#message_template, #(#used_fields = #used_fields),*) }
        }
    }
}
