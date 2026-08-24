//! The `#[pharo_primitive]` attribute. See the `pharo-vm-plugin` crate docs.

use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{parse_macro_input, FnArg, ItemFn, LitInt, LitStr};

/// Everything the attribute accepts.
struct Args {
    /// Symbol the VM will look up. Defaults to the function's own name.
    name: Option<LitStr>,
    /// Value of the exported `<name>AccessorDepth` byte.
    accessor_depth: Option<LitInt>,
}

impl Args {
    fn parse(attr: TokenStream) -> syn::Result<Self> {
        let mut args = Args {
            name: None,
            accessor_depth: None,
        };
        if attr.is_empty() {
            return Ok(args);
        }
        let parser = syn::meta::parser(|meta| {
            if meta.path.is_ident("name") {
                args.name = Some(meta.value()?.parse()?);
                Ok(())
            } else if meta.path.is_ident("accessor_depth") {
                args.accessor_depth = Some(meta.value()?.parse()?);
                Ok(())
            } else {
                Err(meta.error("expected `name` or `accessor_depth`"))
            }
        });
        syn::parse::Parser::parse(parser, attr)?;
        Ok(args)
    }
}

/// Exports a Rust function as a Pharo named primitive.
///
/// The annotated function takes `&Interp` and returns `PrimResult<T>` for any
/// `T: IntoReturn`. It may also declare its Smalltalk arguments as further
/// typed parameters (any `StackArg` types, up to eight):
///
/// ```ignore
/// #[pharo_primitive]
/// fn primitiveScale(vm: &Interp, form: Oop, factor: sqInt) -> PrimResult<()> { .. }
/// ```
///
/// The generated wrapper then checks the argument count and extracts each
/// argument -- in declaration order, first Smalltalk argument first -- before
/// the body runs, exactly as `vm.args::<(Oop, sqInt)>()` would.
///
/// The macro emits, alongside the function:
///
/// * `extern "C" fn <name>()` -- the symbol the VM looks up, which fetches the
///   stored proxy, runs the body inside `catch_unwind`, and translates the
///   outcome into an answer or a `primitiveFailFor` call;
/// * `static <name>AccessorDepth: c_schar` -- the byte the VM reads to decide
///   how deep to follow forwarding pointers in the arguments when retrying a
///   failed primitive.
///
/// # Naming
///
/// The exported symbol defaults to the function's name, so a primitive
/// declared in the image as `<primitive: 'primitiveFoo' module: 'M'>` can be
/// written as `fn primitiveFoo(..)`. To keep an idiomatic Rust name, give the
/// export explicitly:
///
/// ```ignore
/// #[pharo_primitive(name = "primitiveMakeUUID")]
/// fn make_uuid(vm: &Interp) -> PrimResult<()> { .. }
/// ```
///
/// # Accessor depth
///
/// Spur uses lazy forwarding: `become:` leaves a forwarder behind rather than
/// scanning the heap, and primitives are expected to fail when they meet one
/// so the VM can resolve it and retry. To do that the VM needs to know how far
/// into the arguments' object graph the primitive reads -- that is the
/// accessor depth. Slang computes it statically for in-image plugins; for a
/// hand-written one it is declared.
///
/// The default here is `1`: enough to cover a primitive that reads or writes
/// the contents of its arguments, which is the overwhelmingly common case. A
/// value that is too high only costs a little work on the failure path, while
/// one that is too low risks a spurious failure against a forwarded argument,
/// so the default errs high. Set it explicitly when you know better:
///
/// ```ignore
/// #[pharo_primitive(accessor_depth = 0)]   // touches only the oops themselves
/// #[pharo_primitive(accessor_depth = -1)]  // does not traverse at all
/// ```
#[proc_macro_attribute]
pub fn pharo_primitive(attr: TokenStream, item: TokenStream) -> TokenStream {
    let args = match Args::parse(attr) {
        Ok(args) => args,
        Err(e) => return e.to_compile_error().into(),
    };
    let func = parse_macro_input!(item as ItemFn);

    if func.sig.inputs.is_empty() {
        return syn::Error::new_spanned(
            &func.sig,
            "a primitive takes the `&Interp` first, then any typed arguments",
        )
        .to_compile_error()
        .into();
    }
    let mut extra_types = Vec::new();
    for input in func.sig.inputs.iter().skip(1) {
        match input {
            FnArg::Typed(pat) => extra_types.push((*pat.ty).clone()),
            FnArg::Receiver(receiver) => {
                return syn::Error::new_spanned(
                    receiver,
                    "a primitive is a free function, not a method",
                )
                .to_compile_error()
                .into();
            }
        }
    }
    if let Some(asyncness) = func.sig.asyncness {
        return syn::Error::new_spanned(
            asyncness,
            "a primitive cannot be async: the interpreter calls it synchronously",
        )
        .to_compile_error()
        .into();
    }

    let export_name = args
        .name
        .map_or_else(|| func.sig.ident.to_string(), |lit| lit.value());
    let export_ident = format_ident!("{}", export_name);
    let depth_ident = format_ident!("{}AccessorDepth", export_name);

    // Rename the body so the exported `extern "C"` wrapper can take the
    // primitive's name. Without this, the common case of writing
    // `fn primitiveFoo(..)` would collide with its own export.
    let mut func = func;
    let inner_ident = format_ident!("__pharo_primitive_impl_{}", export_name);
    func.sig.ident = inner_ident.clone();

    // The C machinery reads this as a `signed char`; -1 means "no traversal".
    let depth = args
        .accessor_depth
        .map_or_else(|| quote!(1), |lit| quote!(#lit));

    let doc_export = format!(
        "VM entry point for the `{export_name}` named primitive. Generated by `#[pharo_primitive]`."
    );
    let doc_depth = format!(
        "Accessor depth for `{export_name}`, read by the VM when retrying a failed primitive."
    );

    // With typed extra parameters, wrap the body in a closure that extracts
    // them through `StackArgs` first; a bare `(&Interp)` body is passed as is.
    let body = if extra_types.is_empty() {
        quote!(#inner_ident)
    } else {
        let bindings: Vec<_> = (0..extra_types.len())
            .map(|i| format_ident!("arg{i}"))
            .collect();
        quote! {
            |vm: &::pharo_vm_plugin::Interp| {
                let (#(#bindings,)*): (#(#extra_types,)*) =
                    ::pharo_vm_plugin::StackArgs::from_stack_args(vm)?;
                #inner_ident(vm, #(#bindings),*)
            }
        }
    };

    quote! {
        #func

        #[doc = #doc_export]
        #[no_mangle]
        pub extern "C" fn #export_ident() -> ::pharo_vm_plugin::sqInt {
            ::pharo_vm_plugin::__private::run_primitive(#body)
        }

        #[doc = #doc_depth]
        // `#[used]` so the linker keeps it: nothing in Rust references this
        // symbol, only the VM does, by name, at load time.
        #[used]
        #[no_mangle]
        pub static #depth_ident: ::core::ffi::c_schar = #depth;
    }
    .into()
}
