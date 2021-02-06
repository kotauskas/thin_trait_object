//! Everything related to parsing the options of the attribute macro.

use std::borrow::Borrow;
use proc_macro2::Ident;
use syn::{
    Attribute,
    LitStr,
    Token,
    Visibility,
    parenthesized,
    parse::{Parse, ParseStream},
    punctuated::Punctuated,
    token,
};

use crate::marker_traits::MarkerTrait;

pub type AttrOptions = Punctuated<AttrOption, Token![,]>;

pub enum AttrOption {
    /// Overrides the visibility modifier, name and optionally adds attributes to the generated vtable struct.
    ///
    /// # Example
    /// ```rust
    /// # /*
    /// #[thin_trait_object(
    ///     vtable(
    ///         /// Documentation for my vtable!
    ///         #[fancy_attribute]
    ///         pub MyVtableName
    ///     ),
    /// )]
    /// # */
    /// ```
    Vtable {
        name: custom_token::Vtable,
        paren: token::Paren,
        additions: OutputAdditions,
    },
    /// Sets whether the vtable will be stored inline within the thin trait object or as a pointer.
    ///
    /// # Example
    /// ```rust
    /// # /*
    /// #[thin_trait_object(
    ///     inline_vtable = false,
    /// )]
    /// # */
    /// ```
    InlineVtable {
        name: custom_token::InlineVtable,
        eq: Token![=],
        val: Ident, // Boolean literals are parsed as identifiers
    },
    /// Overrides the visibility modifier, name and optionally adds attributes to the generated thin trait object struct.
    ///
    /// # Example
    /// ```rust
    /// # /*
    /// #[thin_trait_object(
    ///     trait_object(
    ///         /// Documentation for my thin trait object type!
    ///         #[fancy_attribute]
    ///         pub MyTraitObjectName
    ///     )
    /// )]
    /// # */
    /// ```
    TraitObject {
        name: custom_token::TraitObject,
        paren: token::Paren,
        additions: OutputAdditions,
    },
    /// Specifies the ABI of the drop handler in the vtable. (ABI for all other methods can be specified directly in the trait definition.)
    ///
    /// # Example
    /// ```rust
    /// # /*
    /// #[thin_trait_object(
    ///     drop_abi = "C",
    /// )]
    /// # */
    /// ```
    DropAbi {
        name: custom_token::DropAbi,
        eq: Token![=],
        abi: LitStr,
    },
    /// Specifies the supertraits which are to be considered marker traits and be automatically implemented on the trait object struct, as well as the safety/unsafety for every single one of them.
    ///
    /// # Example
    /// ```rust
    /// # /*
    /// #[thin_trait_object(
    ///     marker_traits(
    ///         MySafeTrait,
    ///         unsafe MyUnsafeTrait,
    ///     ),
    /// )]
    /// trait SomeTrait: MySafeTrait + MyUnsafeTrait {
    ///     ...
    /// }
    /// # */
    /// ```
    MarkerTraits {
        name: custom_token::MarkerTraits,
        paren: token::Paren,
        marker_traits: Punctuated<MarkerTrait, Token![,]>,
    },
}
impl Parse for AttrOption {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let ident = input.parse::<Ident>()?;
        // see https://github.com/rust-lang/rust-clippy/issues/4637
        #[allow(clippy::eval_order_dependence)]
        let option = match ident.to_string().borrow() {
            "vtable" => {
                let inside_parens;
                Self::Vtable {
                    name: custom_token::Vtable(ident.span()),
                    paren: parenthesized!(inside_parens in input),
                    additions: inside_parens.parse()?,
                }
            }
            "inline_vtable" => Self::InlineVtable {
                name: custom_token::InlineVtable(ident.span()),
                eq: input.parse()?,
                val: input.parse()?,
            },
            "trait_object" => {
                let inside_parens;
                Self::TraitObject {
                    name: input.parse()?,
                    paren: parenthesized!(inside_parens in input),
                    additions: inside_parens.parse()?,
                }
            }
            "drop_abi" => Self::DropAbi {
                name: input.parse()?,
                eq: input.parse()?,
                abi: input.parse()?,
            },
            "marker_traits" => {
                let inside_parens;
                Self::MarkerTraits {
                    name: input.parse()?,
                    paren: parenthesized!(inside_parens in input),
                    marker_traits: inside_parens.call(Punctuated::parse_terminated)?,
                }
            }
            _ => {
                return Err(syn::Error::new_spanned(
                    ident,
                    "\
expected `vtable`, `inline_vtable`, `trait_object`, `drop_abi` or `marker_traits`",
                ));
            }
        };
        Ok(option)
    }
}

pub struct OutputAdditions {
    pub attributes: Vec<Attribute>,
    pub visibility: Visibility,
    pub name: Ident,
}
impl Parse for OutputAdditions {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        Ok(Self {
            attributes: input.call(Attribute::parse_outer)?,
            visibility: input.parse()?,
            name: input.parse()?,
        })
    }
}

pub mod custom_token {
    use syn::{
        Ident,
        parse::{Parse, ParseStream},
    };
    use proc_macro2::Span;

    macro_rules! custom_tokens {
        ($name:ident, $string:literal) => (
            pub struct $name (pub Span);
            impl Parse for $name {
                #[inline]
                fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
                    let ident = input.parse::<Ident>()?;
                    if ident == $string {
                        Ok(
                            Self(ident.span())
                        )
                    } else {
                        Err(
                            syn::Error::new(ident.span(), concat!("expected `", $string, "`"))
                        )
                    }
                }
            }
        );
        ($(($name:ident, $string:literal)),+ $(,)?) => (
            $(custom_tokens!($name, $string);)*
        );
    }

    custom_tokens! {
        (Vtable, "vtable"),
        (InlineVtable, "inline_vtable"),
        (TraitObject, "trait_object"),
        (DropAbi, "drop_abi"),
        (MarkerTraits, "marker_traits"),
    }
}
