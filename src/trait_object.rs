//! Generates the owned trait object struct. Not to be confused with the representation struct.

use proc_macro2::{Ident, TokenStream};
use quote::{format_ident, quote, ToTokens};
use syn::{punctuated::Punctuated, token, Attribute, FnArg, Visibility};

use crate::util::IdentOrPath;
use crate::{
    attr::{StageStash, TargetImpl},
    marker_traits::MarkerTrait,
    vtable::VtableItem,
};

#[derive(Clone, Debug)]
pub struct TraitObjectName {
    /// The primary name of the type (ie. BoxedFoo)
    pub primary_name: Ident,
    /// The 'elided lifetime' of the type (if any)
    ///
    /// This is needed to give a complete name of the type
    pub elided_lifetime: Option<TokenStream>,
}
impl ToTokens for TraitObjectName {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        self.primary_name.to_tokens(tokens);
        if let Some(ref lt) = self.elided_lifetime {
            lt.to_tokens(tokens);
        }
    }
}
pub fn generate_trait_object<'a>(
    stash: &mut StageStash,
    visibility: Visibility,
    inline_vtable: bool,
    has_static_bound: bool,
    attributes: impl IntoIterator<Item = &'a Attribute> + Clone,
    markers: impl IntoIterator<Item = &'a MarkerTrait>,
) -> syn::Result<TokenStream> {
    let StageStash {
        trait_name,
        repr_name,
        target_impl,
        vtable_name,
        trait_object_name,
        vtable_items,
        super_trait,
        ..
    } = stash;
    #[derive(Copy, Clone)]
    struct MarkerToImpl<'a> {
        marker_trait: &'a MarkerTrait,
        implementor: &'a TraitObjectName,
    }
    impl<'a> ToTokens for MarkerToImpl<'a> {
        fn to_tokens(&self, token_stream: &mut TokenStream) {
            token_stream.extend((*self).into_token_stream());
        }
        fn into_token_stream(self) -> TokenStream {
            let implementor = self.implementor;
            let implementor = quote!(#implementor);
            self.marker_trait.as_impl_for(&implementor)
        }
    }
    struct VtableItemToImplThunk<'a> {
        item: VtableItem,
        vtable_method_name: &'a Ident,
        data_ptr_method_name: &'a Ident,
    }
    impl ToTokens for VtableItemToImplThunk<'_> {
        fn to_tokens(&self, token_stream: &mut TokenStream) {
            let signature = self
                .item
                .clone()
                .into_signature(|x| format_ident!("__arg{}", x));
            let call_args = signature
                .inputs
                .clone()
                .into_iter()
                .map(|param| match param {
                    FnArg::Typed(param) => param.pat.into_token_stream(),
                    FnArg::Receiver(..) => {
                        let name = self.data_ptr_method_name;
                        quote!(self.#name() as *mut _)
                    }
                })
                .collect::<Punctuated<_, token::Comma>>();
            let call_name = signature.ident.clone();
            let vtable_method_name = self.vtable_method_name;
            (quote! {
                #signature {
                    unsafe {
                        ((self.#vtable_method_name()).#call_name)(#call_args)
                    }
                }
            })
            .to_tokens(token_stream);
        }
    }

    attributes
        .clone()
        .into_iter()
        .try_for_each(check_attribute)?;
    let attributes = attributes.into_iter();
    let marker_impls = markers.into_iter().map(|marker_trait| MarkerToImpl {
        marker_trait,
        implementor: trait_object_name,
    });

    let vtable_method_name = target_impl.vtable_method_name();
    let data_ptr_method_name = target_impl.data_ptr_method_name();
    let impl_thunks = vtable_items
        .iter()
        .cloned()
        .map(|item| VtableItemToImplThunk {
            item,
            vtable_method_name: &vtable_method_name,
            data_ptr_method_name: &data_ptr_method_name,
        });
    let (phantomdata, generics, creation_bound, impl_elided_lifetime) = if has_static_bound {
        let phantomdata = quote! {
            ::core::marker::PhantomData<&'static ()>
        };
        // Those three are empty, so use the tuple Default impl to write this concisely
        let (generics, creation_bound, impl_elided_lifetime) = Default::default();
        (phantomdata, generics, creation_bound, impl_elided_lifetime)
    } else {
        let phantomdata = quote! {
            ::core::marker::PhantomData<&'inner ()>
        };
        let generics = quote! { <'inner> };
        let creation_bound = quote! { 'inner };
        let impl_elided_lifetime = quote! { <'_> };
        (phantomdata, generics, creation_bound, impl_elided_lifetime)
    };
    let vtable_getter_impl = {
        let vtable_pointer_cast = if inline_vtable {
            quote! { as *mut }
        } else {
            quote! { as *mut &'static }
        };
        quote! {
            unsafe { &*(self.0.as_ptr() #vtable_pointer_cast #vtable_name) }
        }
    };
    let cast_funcs = match super_trait {
        Some(ref super_trait) => {
            use heck::SnakeCase;
            let super_trait_object = super_trait
                .clone()
                .with_simple_name(format_ident!("Boxed{}", super_trait.simple_name()));
            let simple_name = super_trait.simple_name();
            let snake_case =
                Ident::new(&simple_name.to_string().to_snake_case(), simple_name.span());
            let cast_ref_func_name = format_ident!("as_{}", snake_case);
            let cast_val_func_name = format_ident!("into_{}", snake_case);
            // TODO: What if our super-trait has no lifetime bound but we do?
            quote! {
                /// Cast a reference to this type into a reference to its super trait
                #[inline]
                pub fn #cast_ref_func_name(&self) -> &#super_trait_object #generics {
                    unsafe { core::mem::transmute(self) }
                }
                /// Cast a boxed reference to this type into a reference to its super trait
                #[inline]
                pub fn #cast_val_func_name(self) -> #super_trait_object #generics {
                    unsafe { core::mem::transmute(self) }
                }
            }
        }
        None => quote!(),
    };
    let impl_declaration = match *target_impl {
        TargetImpl::SpecificTraitObject {
            ref trait_object_name,
        } => {
            quote!(impl #trait_name for #trait_object_name)
        }
        TargetImpl::BlanketTrait {
            trait_name: ref blanket_trait_name,
            vtable_method: _,
        } => {
            quote! {
                impl<Target: #blanket_trait_name> #trait_name for Target
            }
        }
    };
    let trait_object_name = &trait_object_name.primary_name;
    let result = quote! {
        #(#attributes)*
        #[repr(transparent)]
        #visibility struct #trait_object_name #generics (
            ::core::ptr::NonNull<#vtable_name>,
            #phantomdata,
        );
        impl #generics #trait_object_name #generics {
            #cast_funcs
            /// Constructs a boxed thin trait object from a type implementing the trait.
            #[inline]
            pub fn new<
                T: #trait_name + Sized + #creation_bound
                >(val: T) -> Self {
                    unsafe { Self::from_raw(#repr_name::__thintraitobjectmacro_repr_create(val) as *mut _) }
            }
            /// Creates a thin trait object directly from a raw pointer to its vtable.
            ///
            /// # Safety
            /// This constructor, by its nature, is hugely unsafe and should be avoided when possible. The following invariants must be upheld:
            /// - The pointer must not be null and must point to a valid thin trait object as expected by its vtable which is not uninitialized;
            /// - The function pointers in the vtable must not be null and must point to valid functions with correct ABI and signature;
            /// - The function pointers must have the same safety contract as implied and not a stronger one: only cause UB if the vtable pointer passed to them is invalid or, if those are unsafe in the trait itself, cause UB if the safety contract in their declarations is violated;
            /// - If the trait is unsafe, the function pointers must follow the trait's contract for valid implementations;
            /// - The pointer was not returned by [`as_raw`] which was called on an object which was not put into [`ManuallyDrop`] or consumed by [`mem::forget`], otherwise undefined behavior will be invoked when both are dropped.
            ///
            /// [`as_raw`]: #method.as_raw " "
            /// [`ManuallyDrop`]: https://doc.rust-lang.org/std/mem/struct.ManuallyDrop.html " "
            /// [`mem::forget`]: https://doc.rust-lang.org/std/mem/fn.forget.html " "
            #[inline]
            pub const unsafe fn from_raw(ptr: *mut ()) -> Self {
                // `new_unchecked` is fine because that's part of the safety
                // contract and the entire thing is horribly unsafe anyway.
                Self(::core::ptr::NonNull::new_unchecked(ptr as *mut _), ::core::marker::PhantomData)
            }
            /// Extracts the contained pointer to the trait object.
            ///
            /// Unlike [`into_raw`], ownership of the pointer is not released, and as such will be dropped normally. Unless the original copy is removed via [`mem::forget`] or [`ManuallyDrop`], calling [`from_raw`] and then dropping will cause undefined behavior.
            ///
            /// [`into_raw`]: #method.into_raw " "
            /// [`from_raw`]: #method.from_raw " "
            /// [`ManuallyDrop`]: https://doc.rust-lang.org/std/mem/struct.ManuallyDrop.html " "
            /// [`mem::forget`]: https://doc.rust-lang.org/std/mem/fn.forget.html " "
            #[inline]
            pub const fn as_raw(&self) -> *mut () {
                self.0.as_ptr() as *mut ()
            }
            /// Releases ownership of the trait object, returning the contained pointer. It is the caller's responsibility to drop the trait object at a later time using [`from_raw`].
            ///
            /// For a version which does not release ownership, see [`as_raw`].
            ///
            /// [`from_raw`]: #method.from_raw " "
            /// [`as_raw`]: #method.as_raw " "
            #[inline]
            pub fn into_raw(self) -> *mut () {
                let pointer = self.as_raw();
                ::core::mem::forget(self);
                pointer
            }
            /// Retrieves the raw vtable of the contained trait object.
            #[inline]
            pub fn vtable(&self) -> &#vtable_name {
                #vtable_getter_impl
            }
        }
        #[allow(clippy::ref_in_deref)] // see https://github.com/rust-lang/rust-clippy/issues/6658
        #impl_declaration {
            #(#impl_thunks)*
        }
        impl ::core::ops::Drop for #trait_object_name #impl_elided_lifetime {
            fn drop(&mut self) {
                unsafe { self.vtable().invoke_drop(self.as_raw() as *mut _) }
            }
        }
        #(#marker_impls)*
    };
    Ok(result)
}

fn check_attribute(attribute: &Attribute) -> syn::Result<()> {
    let name = &attribute.path;
    let ident = &name.segments[0].ident;
    let span = ident.span();
    let error = match ident.to_string().as_str() {
        "derive" => syn::Error::new(
            span,
            "\
cannot use derive macros on the thin trait object structure because some \
derived traits may cause undefined behavior when derived on it, such as `Copy`",
        ),
        "repr" => syn::Error::new(
            span,
            "\
the trait object structure already has a `#[repr(transparent)]` annotation",
        ),
        _ => return Ok(()),
    };
    Err(error)
}
