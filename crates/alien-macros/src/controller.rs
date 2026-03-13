use proc_macro::TokenStream;
use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::{format_ident, quote};
use std::collections::HashMap;
use syn::{
    parse::{Parse, ParseStream},
    parse_macro_input, parse_quote,
    punctuated::Punctuated,
    spanned::Spanned,
    Error, Expr, ExprAssign, ExprPath, Fields, Ident, ImplItem, ImplItemFn, ItemImpl, ItemStruct,
    Result, Token, Type,
};

// Helper struct to parse handler attributes
#[derive(Debug)]
struct HandlerAttr {
    state: Ident,
    on_failure: Ident,
    status: ExprPath,
}

impl Parse for HandlerAttr {
    fn parse(input: ParseStream) -> Result<Self> {
        let mut state = None;
        let mut on_failure = None;
        let mut status = None;

        let args = Punctuated::<ExprAssign, Token![,]>::parse_terminated(input)?;

        for arg in args {
            if let (Expr::Path(ExprPath { path, .. }), Expr::Path(value_path)) =
                (&*arg.left, &*arg.right)
            {
                if path.is_ident("state") {
                    state = Some(value_path.path.get_ident().unwrap().clone());
                } else if path.is_ident("on_failure") {
                    on_failure = Some(value_path.path.get_ident().unwrap().clone());
                } else if path.is_ident("status") {
                    status = Some(value_path.clone());
                }
            }
        }

        Ok(HandlerAttr {
            state: state
                .ok_or_else(|| Error::new(input.span(), "missing 'state' in handler attribute"))?,
            on_failure: on_failure.ok_or_else(|| {
                Error::new(input.span(), "missing 'on_failure' in handler attribute")
            })?,
            status: status
                .ok_or_else(|| Error::new(input.span(), "missing 'status' in handler attribute"))?,
        })
    }
}

// Helper struct to parse flow_entry attributes
#[derive(Debug)]
struct FlowEntryAttr {
    flow_type: Ident,
    from_states: Vec<Ident>,
}

impl Parse for FlowEntryAttr {
    fn parse(input: ParseStream) -> Result<Self> {
        let flow_type: Ident = input.parse()?;
        let mut from_states = Vec::new();

        if input.peek(Token![,]) {
            input.parse::<Token![,]>()?;
            // Look for "from = [...]"
            let from_kw: Ident = input.parse()?;
            input.parse::<Token![=]>()?;
            let content;
            syn::bracketed!(content in input);

            // Delete transitions are unconditional -- the executor decides when to delete,
            // not the controller state machine. A `from` list here is always wrong.
            if flow_type == "Delete" {
                return Err(Error::new(
                    from_kw.span(),
                    "#[flow_entry(Delete)] must not have a `from` list -- delete transitions \
                     are unconditional. Remove the `from = [...]` clause.",
                ));
            }

            from_states = Punctuated::<Ident, Token![,]>::parse_terminated(&content)?
                .into_iter()
                .collect();
        }

        Ok(FlowEntryAttr {
            flow_type,
            from_states,
        })
    }
}

// Process struct with #[controller] attribute
pub fn controller_struct(_args: TokenStream, input: TokenStream) -> TokenStream {
    let mut item_struct = parse_macro_input!(input as ItemStruct);

    // Extract the base name (remove "Controller" suffix)
    let struct_name = &item_struct.ident;
    let state_enum_name = if struct_name.to_string().ends_with("Controller") {
        format_ident!(
            "{}State",
            struct_name.to_string().trim_end_matches("Controller")
        )
    } else {
        format_ident!("{}State", struct_name)
    };

    // Add the state and stay count fields
    if let Fields::Named(ref mut fields) = item_struct.fields {
        fields.named.push(parse_quote! {
            pub(crate) state: #state_enum_name
        });
        fields.named.push(parse_quote! {
            pub(crate) _internal_stay_count: Option<u32>
        });
    } else {
        return Error::new(
            item_struct.span(),
            "controller macro only supports structs with named fields",
        )
        .to_compile_error()
        .into();
    }

    // Add derives to the struct
    item_struct.attrs.push(parse_quote! {
        #[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
    });
    item_struct.attrs.push(parse_quote! {
        #[serde(rename_all = "camelCase")]
    });

    quote! { #item_struct }.into()
}

// Process impl block with #[controller] attribute
pub fn controller_impl(_args: TokenStream, input: TokenStream) -> TokenStream {
    let item_impl = parse_macro_input!(input as ItemImpl);
    let struct_name = match &*item_impl.self_ty {
        Type::Path(type_path) => &type_path.path.segments.last().unwrap().ident,
        _ => {
            return Error::new(item_impl.self_ty.span(), "expected a simple type")
                .to_compile_error()
                .into();
        }
    };

    // Extract the state enum name
    let state_enum_name = if struct_name.to_string().ends_with("Controller") {
        format_ident!(
            "{}State",
            struct_name.to_string().trim_end_matches("Controller")
        )
    } else {
        format_ident!("{}State", struct_name)
    };

    // Generate handler action enum name
    let struct_name_str = struct_name.to_string();
    let handler_action_name = format_ident!(
        "{}HandlerAction",
        if struct_name_str.ends_with("Controller") {
            struct_name_str.trim_end_matches("Controller")
        } else {
            &struct_name_str
        }
    );

    // Parse handlers and collect state information
    let mut handlers = HashMap::new();
    let mut terminal_states = Vec::new();
    let mut flow_entries = HashMap::new();
    let mut default_state = None;
    let mut all_states = Vec::new();
    let mut get_binding_params_method = None;

    for item in &item_impl.items {
        match item {
            ImplItem::Fn(method) => {
                // Check for get_binding_params method
                if method.sig.ident == "get_binding_params" {
                    get_binding_params_method = Some(method.clone());
                    continue;
                }

                // Check for handler attribute
                match parse_handler_method(method) {
                    Ok(Some((handler_attr, flow_entry))) => {
                        let state_name = handler_attr.state.clone();

                        if handlers.contains_key(&state_name.to_string()) {
                            return Error::new(
                                state_name.span(),
                                format!("duplicate handler for state '{}'", state_name),
                            )
                            .to_compile_error()
                            .into();
                        }

                        handlers.insert(state_name.to_string(), (method, handler_attr));
                        all_states.push(state_name.clone());

                        if let Some(flow_entry) = flow_entry {
                            if flow_entry.flow_type == "Create" && default_state.is_none() {
                                default_state = Some(state_name.clone());
                            }
                            flow_entries
                                .insert(flow_entry.flow_type.to_string(), (state_name, flow_entry));
                        }
                    }
                    Ok(None) => {}
                    Err(e) => return e.to_compile_error().into(),
                }
            }
            ImplItem::Macro(item_macro) => {
                // Check for terminal_state! macro
                if item_macro.mac.path.is_ident("terminal_state") {
                    if let Ok(terminal_attr) = parse_terminal_state(&item_macro.mac.tokens) {
                        terminal_states.push(terminal_attr.clone());
                        all_states.push(terminal_attr.0.clone());
                    }
                }
            }
            _ => {}
        }
    }

    if default_state.is_none() {
        return Error::new(
            Span::call_site(),
            "controller must have at least one handler with #[flow_entry(Create)]",
        )
        .to_compile_error()
        .into();
    }

    // Validate that failure states don't appear in flow_entry from lists
    if let Err(validation_error) =
        validate_failure_states_not_in_flow_entries(&handlers, &flow_entries)
    {
        return validation_error.to_compile_error().into();
    }

    // Generate the state enum
    let state_enum = generate_state_enum(&state_enum_name, &all_states, &default_state.unwrap());

    // Generate the controller-specific HandlerAction enum
    let handler_action_enum = generate_handler_action_enum(&handler_action_name, &state_enum_name);

    // Generate the ResourceController implementation
    let controller_impl = generate_controller_impl(
        struct_name,
        &state_enum_name,
        &handler_action_name,
        &handlers,
        &terminal_states,
        &flow_entries,
        get_binding_params_method.as_ref(),
    );

    // Generate handler methods
    let handler_methods =
        generate_handler_methods(&item_impl, &state_enum_name, &handler_action_name);

    quote! {
        #state_enum

        #handler_action_enum

        #handler_methods

        #controller_impl
    }
    .into()
}

fn parse_handler_method(
    method: &ImplItemFn,
) -> Result<Option<(HandlerAttr, Option<FlowEntryAttr>)>> {
    let mut handler_attr = None;
    let mut flow_entry_attr = None;

    for attr in &method.attrs {
        if attr.path().is_ident("handler") {
            handler_attr = Some(attr.parse_args::<HandlerAttr>()?);
        } else if attr.path().is_ident("flow_entry") {
            flow_entry_attr = Some(attr.parse_args::<FlowEntryAttr>()?);
        }
    }

    Ok(handler_attr.map(|h| (h, flow_entry_attr)))
}

fn parse_terminal_state(tokens: &TokenStream2) -> Result<(Ident, ExprPath)> {
    // Parse terminal_state!(state = SomeState, status = ResourceStatus::SomeStatus)
    let content = tokens.to_string();
    let parts: Vec<&str> = content.split(',').collect();

    if parts.len() != 2 {
        return Err(Error::new(
            tokens.span(),
            "terminal_state! expects exactly 2 arguments",
        ));
    }

    let state_part = parts[0].trim();
    let status_part = parts[1].trim();

    // Extract state name
    let state_name = state_part
        .strip_prefix("state")
        .and_then(|s| s.trim().strip_prefix("="))
        .map(|s| s.trim())
        .ok_or_else(|| Error::new(tokens.span(), "expected 'state = StateName'"))?;

    // Extract status
    let status = status_part
        .strip_prefix("status")
        .and_then(|s| s.trim().strip_prefix("="))
        .map(|s| s.trim())
        .ok_or_else(|| Error::new(tokens.span(), "expected 'status = ResourceStatus::...'"))?;

    let state_ident = syn::parse_str::<Ident>(state_name)?;
    let status_path = syn::parse_str::<ExprPath>(status)?;

    Ok((state_ident, status_path))
}

fn validate_failure_states_not_in_flow_entries(
    handlers: &HashMap<String, (&ImplItemFn, HandlerAttr)>,
    flow_entries: &HashMap<String, (Ident, FlowEntryAttr)>,
) -> Result<()> {
    // Check each flow entry for violations
    for (flow_type, (start_state, flow_entry)) in flow_entries {
        // Find the handler that corresponds to this flow's start state
        if let Some((_, start_handler)) = handlers.get(&start_state.to_string()) {
            let on_failure_state = start_handler.on_failure.to_string();

            // Check if the on_failure state appears in the from list
            for from_state in &flow_entry.from_states {
                let from_state_name = from_state.to_string();
                if from_state_name == on_failure_state {
                    return Err(Error::new(
                        from_state.span(),
                        format!(
                            "Failure state '{}' cannot appear in flow_entry({}) 'from' list. \
                             The on_failure state '{}' requires manual intervention via retry_failed(). \
                             Remove '{}' from the 'from' list.",
                            from_state_name, flow_type, on_failure_state, from_state_name
                        )
                    ));
                }
            }
        }
    }

    Ok(())
}

fn generate_state_enum(name: &Ident, states: &[Ident], default_state: &Ident) -> TokenStream2 {
    let variants = states.iter().map(|state| {
        if state == default_state {
            quote! { #[default] #state }
        } else {
            quote! { #state }
        }
    });

    quote! {
        #[derive(Debug, serde::Serialize, serde::Deserialize, Clone, PartialEq, Default)]
        #[serde(rename_all = "camelCase")]
        pub enum #name {
            #(#variants,)*
        }
    }
}

fn generate_handler_action_enum(name: &Ident, state_enum_name: &Ident) -> TokenStream2 {
    quote! {
        /// Represents the outcome of a handler method in the resource controller state machine.
        #[derive(Debug)]
        pub enum #name {
            /// Continue to the next state with an optional delay
            Continue {
                /// The next state to transition to
                state: #state_enum_name,
                /// Optional delay before the next step
                suggested_delay: Option<std::time::Duration>,
            },
            /// Stay in the current handler with a delay
            Stay {
                /// Maximum number of iterations
                max_times: u32,
                /// Optional delay before running the handler again
                suggested_delay: Option<std::time::Duration>,
            },
        }
    }
}

fn generate_controller_impl(
    struct_name: &Ident,
    state_enum_name: &Ident,
    handler_action_name: &Ident,
    handlers: &HashMap<String, (&ImplItemFn, HandlerAttr)>,
    terminal_states: &[(Ident, ExprPath)],
    flow_entries: &HashMap<String, (Ident, FlowEntryAttr)>,
    get_binding_params_method: Option<&ImplItemFn>,
) -> TokenStream2 {
    let step_match_arms = generate_step_match_arms(state_enum_name, handler_action_name, handlers);
    let get_status_match_arms =
        generate_get_status_match_arms(state_enum_name, handlers, terminal_states);
    let transition_to_failure_body = generate_transition_to_failure(state_enum_name, handlers);
    let transition_to_delete_body = generate_transition_to_delete(state_enum_name, flow_entries);
    let transition_to_update_body = generate_transition_to_update(state_enum_name, flow_entries);

    let get_binding_params_impl = if let Some(method) = get_binding_params_method {
        // Include the user's implementation directly in the trait
        let method_block = &method.block;
        quote! {
            fn get_binding_params(&self) -> Option<serde_json::Value> {
                #method_block
            }
        }
    } else {
        quote! {}
    };

    quote! {
        #[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
        #[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
        #[typetag::serde]
        impl crate::core::ResourceController for #struct_name {
            async fn step(
                &mut self,
                ctx: &crate::core::ResourceControllerContext,
            ) -> crate::Result<crate::core::ResourceControllerStepResult> {
                use #state_enum_name::*;

                let delay = match &self.state {
                    #step_match_arms

                    // Terminal states
                    _ => return Ok(crate::core::ResourceControllerStepResult {
                        suggested_delay: None,
                    }),
                };

                Ok(crate::core::ResourceControllerStepResult {
                    suggested_delay: delay,
                })
            }

            fn transition_to_failure(&mut self) {
                use #state_enum_name::*;
                #transition_to_failure_body
            }

            fn transition_to_delete_start(&mut self) -> crate::Result<()> {
                use #state_enum_name::*;
                #transition_to_delete_body
            }

            fn transition_to_update(&mut self) -> crate::Result<()> {
                use #state_enum_name::*;
                #transition_to_update_body
            }

            fn get_status(&self) -> alien_core::ResourceStatus {
                use #state_enum_name::*;
                match &self.state {
                    #get_status_match_arms
                }
            }

            fn get_outputs(&self) -> Option<alien_core::ResourceOutputs> {
                use #state_enum_name::*;
                // Don't return outputs for deleted resources
                match &self.state {
                    Deleted => None,
                    _ => self.build_outputs(),
                }
            }

            #get_binding_params_impl

            fn reset_stay_count(&mut self) {
                self._internal_stay_count = None;
            }

            fn box_clone(&self) -> Box<dyn crate::core::ResourceController> {
                Box::new(self.clone())
            }

            fn as_any(&self) -> &dyn std::any::Any {
                self
            }
        }
    }
}

fn generate_step_match_arms(
    _state_enum_name: &Ident,
    handler_action_name: &Ident,
    handlers: &HashMap<String, (&ImplItemFn, HandlerAttr)>,
) -> TokenStream2 {
    let arms = handlers.iter().map(|(_state_name, (method, attr))| {
        let state_ident = &attr.state;
        let method_ident = &method.sig.ident;
        let _on_failure = &attr.on_failure;

        quote! {
            #state_ident => {
                match self.#method_ident(ctx).await {
                    Ok(#handler_action_name::Continue { state, suggested_delay }) => {
                        // Directly use the enum variant
                        self.state = state;
                        self._internal_stay_count = None;
                        suggested_delay
                    }
                    Ok(#handler_action_name::Stay { max_times, suggested_delay }) => {
                        let count = self._internal_stay_count.get_or_insert(0);
                        *count += 1;
                        if *count >= max_times {
                            return Err(alien_error::AlienError::new(
                                crate::error::ErrorData::PollingTimeout {
                                    state: format!("{:?}", self.state),
                                    max_times,
                                },
                            ));
                        } else {
                            suggested_delay
                        }
                    }
                    Err(e) => return Err(e),
                }
            }
        }
    });

    quote! { #(#arms)* }
}

fn generate_get_status_match_arms(
    _state_enum_name: &Ident,
    handlers: &HashMap<String, (&ImplItemFn, HandlerAttr)>,
    terminal_states: &[(Ident, ExprPath)],
) -> TokenStream2 {
    let handler_arms = handlers.iter().map(|(_, (_, attr))| {
        let state = &attr.state;
        let status = &attr.status;
        quote! { #state => #status }
    });

    let terminal_arms = terminal_states.iter().map(|(state, status)| {
        quote! { #state => #status }
    });

    quote! {
        #(#handler_arms,)*
        #(#terminal_arms,)*
    }
}

fn generate_transition_to_failure(
    _state_enum_name: &Ident,
    handlers: &HashMap<String, (&ImplItemFn, HandlerAttr)>,
) -> TokenStream2 {
    let match_arms = handlers.values().map(|(_, attr)| {
        let state = &attr.state;
        let on_failure = &attr.on_failure;
        quote! { #state => #on_failure }
    });

    quote! {
        self.state = match &self.state {
            #(#match_arms,)*
            // Keep terminal states as-is
            state => state.clone(),
        };
    }
}

fn generate_transition_to_delete(
    state_enum_name: &Ident,
    flow_entries: &HashMap<String, (Ident, FlowEntryAttr)>,
) -> TokenStream2 {
    if let Some((delete_start_state, _delete_flow)) = flow_entries.get("Delete") {
        // Unconditional -- the executor's plan() decides *when* to delete.
        // The controller just transitions to the delete start state from wherever it is.
        // This mirrors how transition_to_failure() works.
        quote! {
            self.state = #state_enum_name::#delete_start_state;
            Ok(())
        }
    } else {
        quote! {
            Err(alien_error::AlienError::new(crate::error::ErrorData::ResourceConfigInvalid {
                message: "No delete flow defined".to_string(),
                // TODO (CRITICAL): Implement
                resource_id: None,
            }))
        }
    }
}

fn generate_transition_to_update(
    state_enum_name: &Ident,
    flow_entries: &HashMap<String, (Ident, FlowEntryAttr)>,
) -> TokenStream2 {
    if let Some((update_start_state, update_flow)) = flow_entries.get("Update") {
        let allowed_states = &update_flow.from_states;
        let allowed_states_str = allowed_states
            .iter()
            .map(|s| quote!(#s).to_string())
            .collect::<Vec<_>>()
            .join(", ");
        quote! {
            match &self.state {
                #(#allowed_states)|* => {
                    self.state = #state_enum_name::#update_start_state;
                    Ok(())
                }
                _ => Err(alien_error::AlienError::new(crate::error::ErrorData::ResourceConfigInvalid {
                    message: format!("Cannot transition to update from state: {:?}. Allowed states: {}", self.state, #allowed_states_str),
                    // TODO (CRITICAL): Implement
                    resource_id: None,
                }))
            }
        }
    } else {
        quote! {
            Err(alien_error::AlienError::new(crate::error::ErrorData::ResourceConfigInvalid {
                message: "No update flow defined".to_string(),
                // TODO (CRITICAL): Implement
                resource_id: None,
            }))
        }
    }
}

fn generate_handler_methods(
    item_impl: &ItemImpl,
    state_enum_name: &Ident,
    handler_action_name: &Ident,
) -> TokenStream2 {
    let struct_name = match &*item_impl.self_ty {
        Type::Path(type_path) => &type_path.path.segments.last().unwrap().ident,
        _ => panic!("expected a simple type"),
    };

    let mut has_build_outputs = false;
    let methods: Vec<_> = item_impl
        .items
        .iter()
        .filter_map(|item| match item {
            ImplItem::Fn(method) => {
                // Check if build_outputs is already implemented
                if method.sig.ident == "build_outputs" {
                    has_build_outputs = true;
                    return Some(method.clone()); // Include the build_outputs method
                }

                // Exclude get_binding_params as it's handled in the trait implementation
                if method.sig.ident == "get_binding_params" {
                    return None;
                }

                // Only include methods with handler attribute
                let has_handler = method
                    .attrs
                    .iter()
                    .any(|attr| attr.path().is_ident("handler"));
                if has_handler {
                    // Remove handler and flow_entry attributes and update return type
                    let mut clean_method = method.clone();
                    clean_method.attrs.retain(|attr| {
                        !attr.path().is_ident("handler") && !attr.path().is_ident("flow_entry")
                    });

                    // Update the return type to use the controller-specific HandlerAction
                    if let syn::ReturnType::Type(_, ref mut return_type) =
                        &mut clean_method.sig.output
                    {
                        if let syn::Type::Path(type_path) = return_type.as_mut() {
                            // Look for Result<HandlerAction> and replace with Result<ControllerHandlerAction>
                            if let Some(last_segment) = type_path.path.segments.last_mut() {
                                if last_segment.ident == "Result" {
                                    if let syn::PathArguments::AngleBracketed(ref mut args) =
                                        &mut last_segment.arguments
                                    {
                                        if let Some(syn::GenericArgument::Type(syn::Type::Path(
                                            ref mut inner_type_path,
                                        ))) = args.args.first_mut()
                                        {
                                            if let Some(inner_segment) =
                                                inner_type_path.path.segments.last_mut()
                                            {
                                                if inner_segment.ident == "HandlerAction" {
                                                    inner_segment.ident =
                                                        handler_action_name.clone();
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }

                    // Add the use statements at the beginning of the method body
                    let handler_action_use_stmt: syn::Stmt = syn::parse_quote! {
                        use #handler_action_name as HandlerAction;
                    };
                    let state_enum_use_stmt: syn::Stmt = syn::parse_quote! {
                        use #state_enum_name::*;
                    };
                    clean_method.block.stmts.insert(0, state_enum_use_stmt);
                    clean_method.block.stmts.insert(0, handler_action_use_stmt);

                    Some(clean_method)
                } else {
                    None
                }
            }
            _ => None,
        })
        .collect();

    // Generate default build_outputs if not provided
    let default_build_outputs = if !has_build_outputs {
        quote! {
            fn build_outputs(&self) -> Option<alien_core::ResourceOutputs> {
                None
            }
        }
    } else {
        quote! {}
    };

    quote! {
        impl #struct_name {
            #(#methods)*

            #default_build_outputs
        }
    }
}
