//! Handling of marker supertraits of traits annotated with `#[thin_trait_object]`.

use once_cell::unsync::Lazy;
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use std::{borrow::Borrow, iter};
use syn::{
    parse::{Parse, ParseStream},
    punctuated::Punctuated,
    token,
    Lifetime,
    Path,
    PathSegment,
    TraitBound,
    TypeParamBound,
};

#[derive(Clone, PartialEq, Eq)]
pub struct MarkerTrait {
    pub unsafety: Option<token::Unsafe>,
    pub path: Path,
}
impl MarkerTrait {
    pub fn as_impl_for(&self, implementor: &Path) -> TokenStream {
        let marker_unsafety = self.unsafety.as_ref();
        let marker_path = &self.path;
        quote! {
            #marker_unsafety impl #marker_path for #implementor {}
        }
    }
}
impl Parse for MarkerTrait {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let unsafety = if input.peek(token::Unsafe) {
            Some(input.parse()?)
        } else {
            None
        };
        Ok(Self {
            unsafety,
            path: input.parse()?,
        })
    }
}

/// Takes a supertrait list and splits it into `Vec<MarkerTrait>` and `Vec<Lifetime>`, using the given predicate function to determine whether an element should be unsafe or not and whether it should even be added to the marker list. `None` ignores the element, `Some(false)` includes it as a safe marker and `Some(true)` includes it as a safe one.
pub fn supertraits_to_markers_and_lifetimes(
    supertraits: Punctuated<TypeParamBound, token::Add>,
    mut marker_filter: impl FnMut(TraitBound) -> Option<(TraitBound, bool)>,
) -> (Vec<MarkerTrait>, Vec<Lifetime>) {
    // Both have excess capacity to make sure that reallocation never happens, which
    // wins in performance much more than it loses by asking for more than it needs.
    let mut markers = Vec::with_capacity(supertraits.len());
    let mut lifetimes = Vec::with_capacity(supertraits.len());
    for supertrait in supertraits {
        match supertrait {
            TypeParamBound::Trait(trait_bound) => {
                if let Some((bound, is_unsafe)) = marker_filter(trait_bound) {
                    markers.push(MarkerTrait {
                        unsafety: if is_unsafe {
                            Some(token::Unsafe {
                                span: Span::call_site(),
                            })
                        } else {
                            None
                        },
                        path: bound.path,
                    })
                }
            }
            TypeParamBound::Lifetime(lifetime) => lifetimes.push(lifetime),
        }
    }
    (markers, lifetimes)
}

macro_rules! make_path {
    ($segment:expr) => {
        Path {
            leading_colon: None,
            segments: iter::once($segment).collect(),
        }
    };
    [$first:expr, $($rest:expr),+] => {
        Path {
            leading_colon: Some(Default::default()),
            segments: iter::once($first)
                $(.chain(iter::once($rest)))*
                .collect(),
        }
    };
}
fn mkseg(string: &str) -> PathSegment {
    PathSegment::from(Ident::new(string, Span::call_site()))
}

pub fn default_marker_filter(bound: TraitBound) -> Option<(TraitBound, bool)> {
    LOOKUP_TABLE.with(|lookup_table| {
        for (short_name, full_path, is_unsafe) in lookup_table.borrow().iter().cloned() {
            if bound.path == make_path!(mkseg(short_name)) || bound.path == full_path {
                return Some((bound, is_unsafe));
            }
        }
        None
    })
}

thread_local! {
    static LOOKUP_TABLE: Lazy<[(&'static str, Path, bool); 5]> = Lazy::new(|| {
        [
            (
                "Send",
                make_path![mkseg("core"), mkseg("marker"), mkseg("Send")],
                true,
            ),
            (
                "Sync",
                make_path![mkseg("core"), mkseg("marker"), mkseg("Sync")],
                true,
            ),
            (
                "Unpin",
                make_path![mkseg("core"), mkseg("marker"), mkseg("Unpin")],
                false,
            ),
            (
                "UnwindSafe",
                make_path![mkseg("std"), mkseg("panic"), mkseg("UnwindSafe")],
                false,
            ),
            (
                "RefUnwindSafe",
                make_path![mkseg("std"), mkseg("panic"), mkseg("RefUnwindSafe")],
                false,
            ),
        ]
    });
}
