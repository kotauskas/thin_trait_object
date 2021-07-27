//! Generates the representation struct.

use crate::{
    attr::StageStash,
    vtable::{VtableFnArg, VtableItem},
};
use proc_macro2::{Ident, TokenStream};
use quote::{format_ident, quote, ToTokens};
use syn::{token::Colon, Abi, BareFnArg, Path, Signature};
use crate::util::IdentOrPath;

pub fn generate_repr(
    stash: &mut StageStash,
    inline_vtable: bool,
    path_to_box: Path,
    drop_abi: Option<&Abi>,
    store_layout: bool,
) -> TokenStream {
    let StageStash {
        repr_name,
        vtable_name,
        trait_name,
        vtable_items,
        ..
    } = stash;
    let (vtable_contents, thunk_methods) = generate_vtable_and_thunks(
        &trait_name,
        &repr_name,
        vtable_items.iter().cloned(),
        |_| true, // TODO
    );

    // Perform necessary branching depending on vtable style in advance.
    let (vtable_field_type, ctor_val) = if inline_vtable {
        // The type of the vtable field is the vtable type's name itself,
        // so just get a token stream of it.
        let vtable_field_type = vtable_name.to_token_stream();
        // The constructor will memcpy the vtable into the repr struct.
        let ctor_val = quote! {
            Self {
                __thintraitobjectmacro_repr_vtable: Self::__THINTRAITOBJECTMACRO_VTABLE,
                __thintraitobjectmacro_repr_value: __thintraitobjectmacro_arg0,
            }
        };
        (vtable_field_type, ctor_val)
    } else {
        // Here, we need to construct a reference-to-static type with the vtable typename.
        let vtable_field_type = quote! {
            &'static #vtable_name
        };
        // The constructor will borrow the static vtable.
        let ctor_val = quote! {
            Self {
                __thintraitobjectmacro_repr_vtable: &Self::__THINTRAITOBJECTMACRO_VTABLE,
                __thintraitobjectmacro_repr_value: __thintraitobjectmacro_arg0,
            }
        };
        (vtable_field_type, ctor_val)
    };
    let size_and_align = if store_layout {
        quote! {
            size: ::core::mem::size_of::<__ThinTraitObjectMacro_ReprGeneric0>(),
            align: ::core::mem::align_of::<__ThinTraitObjectMacro_ReprGeneric0>(),
        }
    } else {
        quote! {}
    };
    let init_super_type = if let Some(ref super_trait) = stash.super_trait {
        let super_repr_name = super_trait.clone()
            .with_simple_name(repr_name_from_trait_name(super_trait.simple_name().clone()));
        quote! {
            super_trait_vtable: #super_repr_name::<__ThinTraitObjectMacro_ReprGeneric0>::__THINTRAITOBJECTMACRO_VTABLE,
        }
    } else {
        quote!()
    };
    let init_drop = if stash.super_trait.is_none() {
        quote! {
            drop: Self :: __thintraitobjectmacro_repr_drop,
        }
    } else {
        quote!() // not needed
    };
    // Here comes the cluttered part: heavily prefixed names.
    let repr = quote! {
        #[repr(C)]
        struct #repr_name <__ThinTraitObjectMacro_ReprGeneric0: #trait_name> {
            __thintraitobjectmacro_repr_vtable: #vtable_field_type,
            __thintraitobjectmacro_repr_value: __ThinTraitObjectMacro_ReprGeneric0,
        }
        impl<
            __ThinTraitObjectMacro_ReprGeneric0: #trait_name
        > #repr_name<__ThinTraitObjectMacro_ReprGeneric0> {
            const __THINTRAITOBJECTMACRO_VTABLE: #vtable_name = #vtable_name {
                #init_super_type
                #size_and_align
                #vtable_contents
                #init_drop
            };

            fn __thintraitobjectmacro_repr_create(
                __thintraitobjectmacro_arg0: __ThinTraitObjectMacro_ReprGeneric0,
            ) -> *mut #vtable_name {
                #path_to_box::into_raw(#path_to_box::new(#ctor_val)) as *mut _
            }
            // Simple destructor which uses Box's internals to deallocate and
            // drop the value as necessary.
            unsafe #drop_abi fn __thintraitobjectmacro_repr_drop(
                __thintraitobjectmacro_arg0: *mut ::core::ffi::c_void,
            ) {
                let _ = #path_to_box::from_raw(
                    __thintraitobjectmacro_arg0
                        as *mut #repr_name<__ThinTraitObjectMacro_ReprGeneric0>
                );
            }
            #thunk_methods
        }
    };
    repr
}

#[inline]
pub fn repr_name_from_trait_name(trait_name: Ident) -> Ident {
    format_ident!("__ThinTraitObjectMacro_ReprFor{}", trait_name)
}

fn generate_vtable_and_thunks(
    trait_name: &Ident,
    repr_name: &Ident,
    vtable_entries: impl IntoIterator<Item = VtableItem>,
    mut double_hop_predicate: impl FnMut(&VtableItem) -> bool,
) -> (TokenStream, TokenStream) {
    let mut vtable_contents = TokenStream::new();
    let mut thunk_methods = TokenStream::new();
    for mut entry in vtable_entries {
        let double_hop = double_hop_predicate(&entry);

        let has_receiver = entry.make_raw();
        if has_receiver {
            entry.make_unsafe();
        }
        // Create the list of arguments decorated with the collision-avoiding
        // names. Using mixed-site hygeine could be a better solution.
        let mut argument_counter = 1_u32;
        let thunk_call_args = entry.inputs.clone().into_iter().skip(1).map(|x| {
            let arg = to_nth_thunk_arg(x, argument_counter);
            argument_counter += 1;
            arg
        });

        if double_hop {
            // Clone this out before handing them over to into_signature().
            let name = entry.name.clone();

            let thunk_name = format_ident!("__thintraitobjectmacro_thunk_{}", &entry.name);
            let thunk_signature = {
                let mut signature = entry.into_signature(nth_arg);
                signature.ident = thunk_name.clone();
                signature
            };

            // Remember that this gets called in a loop, so we add one vtable
            // constructor entry for every vtable entry.
            write_vtable_thunk_entry(&name, &thunk_name, &mut vtable_contents);

            // Generate the thunks, again, one for every vtable entry. Those are
            // pretty simple, actually: just unsafely convert the pointer to a
            // reference to the repr struct and call the appropriate method,
            // offsetting into the actual value.
            write_thunk(
                &name,
                &repr_name,
                thunk_signature,
                thunk_call_args,
                &mut thunk_methods,
            );
        } else {
            write_vtable_single_hop_entry(&entry.name, &trait_name, &mut vtable_contents);
        }
    }
    (vtable_contents, thunk_methods)
}

fn write_vtable_thunk_entry(name: &Ident, val: &Ident, out: &mut TokenStream) {
    (quote! {
        #name: Self :: #val,
    })
    .to_tokens(out);
}
fn write_vtable_single_hop_entry(name: &Ident, trait_name: &Ident, out: &mut TokenStream) {
    (quote! {
        #name: <__ThinTraitObjectMacro_ReprGeneric0 as #trait_name> :: #name,
    })
    .to_tokens(out);
}
fn write_thunk(
    name: &Ident,
    repr_name: &Ident,
    signature: Signature,
    args: impl IntoIterator<Item = BareFnArg>,
    out: &mut TokenStream,
) {
    let args = args.into_iter().map(|arg| arg.name.unwrap().0);
    (quote! {
        #signature {
            (
                *(__thintraitobjectmacro_arg0
                    as *mut #repr_name<__ThinTraitObjectMacro_ReprGeneric0>
                )
            ).__thintraitobjectmacro_repr_value.#name(#(#args)*)
        }
    })
    .to_tokens(out);
}

fn nth_arg(n: u32) -> Ident {
    format_ident!("__thintraitobjectmacro_arg{}", n)
}
/// Transforms a VtableFnArg to an argument to a thunk.
fn to_nth_thunk_arg(arg: VtableFnArg, n: u32) -> BareFnArg {
    let mut arg = arg.into_bare_arg_with_ptr_receiver();
    arg.name = Some(arg.name.unwrap_or_else(|| (nth_arg(n), Colon::default())));
    arg
}
