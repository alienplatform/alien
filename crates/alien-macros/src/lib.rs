use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, Expr, ItemFn};

/// A procedural macro that wraps a function with an AlienEvent scope.
///
/// This macro is similar to tracing's `#[instrument]` but for the Alien events system.
/// It automatically wraps the function body with `AlienEvent::in_scope()`.
///
/// # Usage
///
/// ```rust
/// use alien_macros::alien_event;
/// use alien_core::{AlienEvent, Result};
///
/// #[alien_event(AlienEvent::BuildingStack { stack: "my-stack".to_string() })]
/// async fn build_stack() -> Result<()> {
///     // Function body - all events emitted here will be children
///     // of the BuildingStack event
///     Ok(())
/// }
/// ```
///
/// The macro supports any AlienEvent variant:
///
/// ```rust
/// #[alien_event(AlienEvent::BuildingImage { image: "api:latest".to_string() })]
/// async fn build_image() -> Result<()> {
///     Ok(())
/// }
/// ```
///
/// You can also use expressions for dynamic values:
///
/// ```rust
/// #[alien_event(AlienEvent::BuildingStack { stack: format!("stack-{}", id) })]
/// async fn build_dynamic_stack(id: u32) -> Result<()> {
///     Ok(())
/// }
/// ```
///
/// The macro only works with async functions since it uses `AlienEvent::in_scope()` internally.
#[proc_macro_attribute]
pub fn alien_event(args: TokenStream, input: TokenStream) -> TokenStream {
    let event_expr = parse_macro_input!(args as Expr);
    let input_fn = parse_macro_input!(input as ItemFn);

    let fn_vis = &input_fn.vis;
    let fn_sig = &input_fn.sig;
    let fn_block = &input_fn.block;
    let fn_attrs = &input_fn.attrs;

    // Check if function is async
    let is_async = fn_sig.asyncness.is_some();

    if is_async {
        // For async functions, use in_scope for automatic success/failure tracking
        let expanded = quote! {
            #(#fn_attrs)*
            #fn_vis #fn_sig {
                (#event_expr).in_scope(|_event_handle| async move #fn_block).await
            }
        };
        TokenStream::from(expanded)
    } else {
        // For sync functions, we can't use in_scope since it's async
        // Instead, we'll just note that this macro is designed for async functions
        panic!("alien_event macro currently only supports async functions. Use AlienEvent::emit() manually for sync functions.");
    }
}

mod controller;

use controller::{controller_impl, controller_struct};

/// Macro for simplifying resource controller implementations.
///
/// When applied to a struct, it adds internal state management fields.
/// When applied to an impl block, it generates the ResourceController trait implementation.
///
/// # Usage on struct:
/// ```rust
/// #[controller]
/// struct AwsFunctionController {
///     pub arn: Option<String>,
///     pub url: Option<String>,
/// }
/// ```
///
/// # Usage on impl:
/// ```rust
/// #[controller]
/// impl AwsFunctionController {
///     #[flow_entry(Create)]
///     #[handler(state = CreateStart, on_failure = CreateFailed, status = ResourceStatus::Provisioning)]
///     async fn create_start(&mut self, ctx: &ResourceControllerContext) -> Result<HandlerAction> {
///         // ...
///     }
/// }
/// ```
#[proc_macro_attribute]
pub fn controller(args: TokenStream, input: TokenStream) -> TokenStream {
    // Check if this is a struct or impl block
    if syn::parse::<syn::ItemStruct>(input.clone()).is_ok() {
        return controller_struct(args, input);
    }

    if syn::parse::<syn::ItemImpl>(input.clone()).is_ok() {
        return controller_impl(args, input);
    }

    panic!("controller macro can only be applied to structs or impl blocks");
}

/// Helper attributes for the controller macro
#[proc_macro_attribute]
pub fn flow_entry(_args: TokenStream, input: TokenStream) -> TokenStream {
    // This is just a marker attribute, return input unchanged
    input
}

#[proc_macro_attribute]
pub fn handler(_args: TokenStream, input: TokenStream) -> TokenStream {
    // This is just a marker attribute, return input unchanged
    input
}

/// Macro for defining terminal states in a controller
#[proc_macro]
pub fn terminal_state(_input: TokenStream) -> TokenStream {
    // This will be used inside the controller impl
    // For now, just return an empty token stream as it will be handled by the controller macro
    quote::quote! {}.into()
}
