//! Everything related to parsing the options of the attribute macro.

use proc_macro2::Ident;
use std::borrow::Borrow;
use syn::{parenthesized, parse::{Parse, ParseStream}, punctuated::Punctuated, token, Attribute, LitBool, LitStr, Token, Visibility, Path};

use crate::marker_traits::MarkerTrait;

pub type AttrOptions = Punctuated<AttrOption, Token![,]>;
pub type InheritanceOptions = Punctuated<InheritanceOption, Token![,]>;

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
        val: LitBool,
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
    /// Sets whether the vtable will contain the size and alignment of the implementation it corresponds to.
    ///
    /// # Example
    /// ```rust
    /// # /*
    /// #[thin_trait_object(
    ///     store_layout = true,
    /// )]
    /// # */
    /// ```
    StoreLayout {
        name: custom_token::StoreLayout,
        eq: Token![=],
        val: LitBool,
    },
    /// Specifies options for inheritance.
    ///
    /// ## Example
    /// ```rust
    /// # /*
    /// #[thin_trait_object(
    ///     inheritance(
    ///         extends(SuperTrait),
    ///         possible_supertrait = true
    ///     )
    /// )]
    /// # */
    /// ```
    Inheritance {
        name: custom_token::Inheritance,
        paren: token::Paren,
        options: InheritanceOptions
    }
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
                    name: custom_token::TraitObject(ident.span()),
                    paren: parenthesized!(inside_parens in input),
                    additions: inside_parens.parse()?,
                }
            }
            "drop_abi" => Self::DropAbi {
                name: custom_token::DropAbi(ident.span()),
                eq: input.parse()?,
                abi: input.parse()?,
            },
            "marker_traits" => {
                let inside_parens;
                Self::MarkerTraits {
                    name: custom_token::MarkerTraits(ident.span()),
                    paren: parenthesized!(inside_parens in input),
                    marker_traits: inside_parens.call(Punctuated::parse_terminated)?,
                }
            }
            "store_layout" => Self::StoreLayout {
                name: custom_token::StoreLayout(ident.span()),
                eq: input.parse()?,
                val: input.parse()?,
            },
            "inheritance" => {
                let inside_parens;
                Self::Inheritance {
                    name: custom_token::Inheritance(ident.span()),
                    paren: parenthesized!(inside_parens in input),
                    options: inside_parens.call(Punctuated::parse_terminated)?
                }
            }
            _ => {
                return Err(syn::Error::new_spanned(
                    ident,
                    "\
expected `vtable`, `inline_vtable`, `trait_object`, `drop_abi`, `inheritance`, or `marker_traits`",
                ));
            }
        };
        Ok(option)
    }
}

pub enum InheritanceOption {
    /// Specifies the supertrait
    Extends {
        name: custom_token::Extends,
        paren: token::Paren,
        super_type: Path
    },
    /// Specifies whether this type is a possible supertrait
    PossibleSuperTrait {
        name: custom_token::PossibleSuperTrait,
        eq: Token![=],
        val: LitBool
    }
}
impl Parse for InheritanceOption {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let ident = input.parse::<Ident>()?;
        // see above
        #[allow(clippy::eval_order_dependence)]
        let option = match ident.to_string().borrow() {
            "extends" => {
                let inside_parens;
                Self::Extends {
                    name: custom_token::Extends(ident.span()),
                    paren: parenthesized!(inside_parens in input),
                    super_type: inside_parens.parse()?
                }
            },
            "possible_super_trait" => {
                Self::PossibleSuperTrait {
                    name: custom_token::PossibleSuperTrait(ident.span()),
                    eq: input.parse()?,
                    val: input.parse()?,
                }
            }
            _ => {
                return Err(syn::Error::new_spanned(
                    ident,
                    "\
expected `extends`, `possible_super_trait`"
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
    use proc_macro2::Span;
    use syn::{
        parse::{Parse, ParseStream},
        Ident,
    };
    use syn::spanned::Spanned;

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
            impl Spanned for $name {
                #[inline]
                fn span(&self) -> Span {
                    self.0
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
        (StoreLayout, "store_layout"),
        (Inheritance, "inheritance"),
        (Extends, "extends"),
        (PossibleSuperTrait, "possible_super_trait")
    }
}
