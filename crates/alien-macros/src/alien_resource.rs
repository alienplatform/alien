//! `#[alien_resource]`: makes a struct an extension's resource type, writing what Alien's own
//! resource types write by hand.

use proc_macro2::TokenStream;
use quote::quote;
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::{Ident, ItemStruct, LitBool, LitStr, Path, Token};

/// The macro's arguments: `type = "acme/dns-zone", outputs = DnsZoneOutputs,
/// lifecycles = [Frozen, Live], holds_data = false`.
struct Args {
    resource_type: LitStr,
    outputs: Path,
    lifecycles: Vec<Ident>,
    holds_data: LitBool,
}

impl Parse for Args {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let (mut resource_type, mut outputs, mut lifecycles, mut holds_data) =
            (None, None, None, None);
        while !input.is_empty() {
            let key: Ident = if input.peek(Token![type]) {
                input.parse::<Token![type]>()?;
                Ident::new("type", proc_macro2::Span::call_site())
            } else {
                input.parse()?
            };
            input.parse::<Token![=]>()?;
            match key.to_string().as_str() {
                "type" => resource_type = Some(input.parse::<LitStr>()?),
                "outputs" => outputs = Some(input.parse::<Path>()?),
                "lifecycles" => {
                    let content;
                    syn::bracketed!(content in input);
                    let list = Punctuated::<Ident, Token![,]>::parse_terminated(&content)?;
                    lifecycles = Some(list.into_iter().collect::<Vec<_>>());
                }
                "holds_data" => holds_data = Some(input.parse::<LitBool>()?),
                other => {
                    return Err(syn::Error::new(
                        key.span(),
                        format!(
                            "unknown argument `{other}`; #[alien_resource] takes type, outputs, lifecycles and holds_data"
                        ),
                    ))
                }
            }
            if !input.is_empty() {
                input.parse::<Token![,]>()?;
            }
        }
        let missing = |name: &str| {
            syn::Error::new(
                input.span(),
                format!("#[alien_resource] needs `{name} = ...`"),
            )
        };
        let resource_type = resource_type.ok_or_else(|| missing("type"))?;
        let (extension, name) = resource_type
            .value()
            .split_once('/')
            .map(|(extension, name)| (extension.to_string(), name.to_string()))
            .unwrap_or_default();
        if extension.is_empty() || name.is_empty() || name.contains('/') {
            return Err(syn::Error::new(
                resource_type.span(),
                "a custom resource type is `<extension>/<type>`, such as \"acme/dns-zone\"",
            ));
        }
        let lifecycles = lifecycles.ok_or_else(|| missing("lifecycles"))?;
        if lifecycles.is_empty() {
            return Err(missing("lifecycles"));
        }
        for lifecycle in &lifecycles {
            if lifecycle != "Frozen" && lifecycle != "Live" {
                return Err(syn::Error::new(
                    lifecycle.span(),
                    "a lifecycle is `Frozen` or `Live`",
                ));
            }
        }
        Ok(Self {
            resource_type,
            outputs: outputs.ok_or_else(|| missing("outputs"))?,
            lifecycles,
            holds_data: holds_data.ok_or_else(|| missing("holds_data"))?,
        })
    }
}

pub fn expand(args: TokenStream, item: TokenStream) -> syn::Result<TokenStream> {
    let Args {
        resource_type,
        outputs,
        lifecycles,
        holds_data,
    } = syn::parse2(args)?;
    let item: ItemStruct = syn::parse2(item)?;
    let name = &item.ident;
    let sdk = quote!(::alien_extension_sdk::__private);
    let core = quote!(#sdk::alien_core);
    Ok(quote! {
        #item

        impl #name {
            pub const RESOURCE_TYPE: ::alien_extension_sdk::ResourceTypeOf<Self> =
                ::alien_extension_sdk::ResourceTypeOf::new(
                    #core::ResourceType::from_static(#resource_type),
                );
        }

        impl ::alien_extension_sdk::AlienResource for #name {
            const RESOURCE_TYPE: ::alien_extension_sdk::ResourceTypeOf<Self> = #name::RESOURCE_TYPE;
            const LIFECYCLES: &'static [#core::ResourceLifecycle] =
                &[#(#core::ResourceLifecycle::#lifecycles),*];
            const HOLDS_DATA: bool = #holds_data;
            type Outputs = #outputs;
        }

        impl #core::ResourceDefinition for #name {
            fn get_resource_type(&self) -> #core::ResourceType {
                #name::RESOURCE_TYPE.resource_type()
            }

            fn id(&self) -> &str {
                &self.id
            }

            fn get_dependencies(&self) -> ::std::vec::Vec<#core::ResourceRef> {
                ::std::vec::Vec::new()
            }

            // Alien can't tell what a failed create of an extension's resource left behind, so it
            // never deletes it to create it again.
            fn replace_after_failed_create_is_safe(&self) -> bool {
                false
            }

            fn validate_update(
                &self,
                new_config: &dyn #core::ResourceDefinition,
            ) -> #core::Result<()> {
                let new = new_config.as_any().downcast_ref::<#name>().ok_or_else(|| {
                    #sdk::alien_error::AlienError::new(#core::ErrorData::UnexpectedResourceType {
                        resource_id: self.id.clone(),
                        expected: #name::RESOURCE_TYPE.resource_type(),
                        actual: new_config.get_resource_type(),
                    })
                })?;
                if self.id != new.id {
                    return Err(#sdk::alien_error::AlienError::new(
                        #core::ErrorData::InvalidResourceUpdate {
                            resource_id: self.id.clone(),
                            reason: "the 'id' field is immutable".to_string(),
                        },
                    ));
                }
                Ok(())
            }

            fn as_any(&self) -> &dyn ::std::any::Any {
                self
            }

            fn as_any_mut(&mut self) -> &mut dyn ::std::any::Any {
                self
            }

            fn box_clone(&self) -> ::std::boxed::Box<dyn #core::ResourceDefinition> {
                ::std::boxed::Box::new(::std::clone::Clone::clone(self))
            }

            fn resource_eq(&self, other: &dyn #core::ResourceDefinition) -> bool {
                other.as_any().downcast_ref::<#name>() == ::std::option::Option::Some(self)
            }

            fn to_json_value(&self) -> #sdk::serde_json::Result<#sdk::serde_json::Value> {
                #sdk::serde_json::to_value(self)
            }
        }

        impl #core::ResourceOutputsDefinition for #outputs {
            fn get_resource_type(&self) -> #core::ResourceType {
                #name::RESOURCE_TYPE.resource_type()
            }

            fn as_any(&self) -> &dyn ::std::any::Any {
                self
            }

            fn box_clone(&self) -> ::std::boxed::Box<dyn #core::ResourceOutputsDefinition> {
                ::std::boxed::Box::new(::std::clone::Clone::clone(self))
            }

            fn outputs_eq(&self, other: &dyn #core::ResourceOutputsDefinition) -> bool {
                other.as_any().downcast_ref::<#outputs>() == ::std::option::Option::Some(self)
            }

            fn to_json_value(&self) -> #sdk::serde_json::Result<#sdk::serde_json::Value> {
                #sdk::serde_json::to_value(self)
            }
        }
    })
}
