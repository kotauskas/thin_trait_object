//! Generates the owned trait object struct. Not to be confused with the representation struct.

use proc_macro2::{Ident, Punct, Spacing, TokenStream, TokenTree};
use quote::{format_ident, quote, ToTokens};
use syn::{punctuated::Punctuated, token, Attribute, FnArg, Lifetime, Path, Visibility};

use crate::{attr::StageStash, marker_traits::MarkerTrait, vtable::VtableItem};

pub fn generate_trait_object<
    'a,
    M: Iterator<Item = &'a MarkerTrait> + ExactSizeIterator,
    L: Iterator<Item = &'a Lifetime> + ExactSizeIterator,
>(
    stash: &mut StageStash,
    visibility: Visibility,
    inline_vtable: bool,
    attributes: impl IntoIterator<Item = &'a Attribute> + Clone,
    markers: impl IntoIterator<IntoIter = M> + Clone,
    lifetime_bounds: impl IntoIterator<IntoIter = L> + Clone,
) -> syn::Result<TokenStream> {
    let StageStash {
        trait_name,
        repr_name,
        vtable_name,
        trait_object_name,
        vtable_items,
        ..
    } = stash;
    let trait_object_name_as_path = trait_object_name.clone().into();
    let num_markers = markers.clone().into_iter().len();
    let num_lifetimes = lifetime_bounds.clone().into_iter().len();
    #[derive(Copy, Clone)]
    struct MarkerToImpl<'a>(&'a MarkerTrait, &'a Path);
    impl<'a> ToTokens for MarkerToImpl<'a> {
        fn to_tokens(&self, token_stream: &mut TokenStream) {
            token_stream.extend((*self).into_token_stream());
        }
        fn into_token_stream(self) -> TokenStream {
            self.0.as_impl_for(self.1)
        }
    }
    struct VtableItemToImplThunk<'a> {
        item: VtableItem,
        vtable_name: &'a Ident,
        inline_vtable: bool,
    }
    impl<'a> ToTokens for VtableItemToImplThunk<'a> {
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
                    FnArg::Receiver(..) => quote! {
                        self.0.as_ptr() as *mut _
                    },
                })
                .collect::<Punctuated<_, token::Comma>>();
            let call_name = signature.ident.clone();
            let vtable_name = self.vtable_name;
            let vtable_pointer_cast = if self.inline_vtable {
                quote! { as *mut }
            } else {
                quote! { as *mut &'static }
            };
            (quote! {
                #signature {
                    unsafe {
                        (
                            (&*(self.0.as_ptr() #vtable_pointer_cast #vtable_name)).#call_name
                        )(#call_args)
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
    let marker_impls = markers
        .clone()
        .into_iter()
        .map(|x| MarkerToImpl(x, &trait_object_name_as_path));

    let maybe_plus = |yes| {
        if yes {
            TokenStream::from(TokenTree::Punct(Punct::new('+', Spacing::Alone)))
        } else {
            TokenStream::new()
        }
    };
    let marker_bounds =
        markers
            .into_iter()
            .fold(maybe_plus(num_markers != 0), |mut token_stream, marker| {
                marker.path.to_tokens(&mut token_stream);
                token::Add::default().to_tokens(&mut token_stream);
                token_stream
            });
    let mut has_static_lifetime = false;
    let lifetime_bounds = lifetime_bounds.into_iter().fold(
        maybe_plus(num_lifetimes != 0),
        |mut token_stream, lifetime| {
            if lifetime.ident == "static" {
                has_static_lifetime = true;
            }
            lifetime.to_tokens(&mut token_stream);
            token::Add::default().to_tokens(&mut token_stream);
            token_stream
        },
    );
    let impl_thunks = vtable_items
        .iter()
        .cloned()
        .map(|item| VtableItemToImplThunk {
            item,
            vtable_name: &vtable_name,
            inline_vtable,
        });
    let (phantomdata, generics, creation_bound, impl_elided_lifetime) = if has_static_lifetime {
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
        let creation_bound = quote! { + 'inner };
        let impl_elided_lifetime = quote! { <'_> };
        (phantomdata, generics, creation_bound, impl_elided_lifetime)
    };
    let result = quote! {
        #(#attributes)*
        #[repr(transparent)]
        #visibility struct #trait_object_name #generics (
            ::core::ptr::NonNull<#vtable_name>,
            #phantomdata,
        );
        impl #generics #trait_object_name #generics {
            /// Constructs a boxed thin trait object from a type implementing the trait.
            #[inline]
            pub fn new<
                T: #trait_name + Sized #marker_bounds #lifetime_bounds #creation_bound
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
        }
        #[allow(clippy::ref_in_deref)] // see https://github.com/rust-lang/rust-clippy/issues/6658
        impl #trait_name for #trait_object_name #impl_elided_lifetime {
            #(#impl_thunks)*
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
