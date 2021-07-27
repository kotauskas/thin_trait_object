//! Handling of inheritance

use crate::attr::{StageStash, TargetImpl};
use crate::options::{InheritanceOption, InheritanceOptions};
use crate::util::IdentOrPath;
use proc_macro2::{Ident, TokenStream};
use quote::{format_ident, quote, ToTokens};
use syn::{Path, Visibility};

fn blanket_trait_name<P: IdentOrPath>(target_trait: P) -> P {
    let simple_name = format_ident!("ThinTraitObject_Implements_{}", target_trait.simple_name());
    target_trait.with_simple_name(simple_name)
}
fn vtable_method_name(target_trait: &impl IdentOrPath) -> Ident {
    format_ident!("vtable_{}", target_trait.simple_name())
}
pub struct PossibleSuperTrait {
    target_trait: Ident,
    vtable_type: Ident,
    vis: Visibility,
    blanket_impl: Option<TokenStream>,
}
impl PossibleSuperTrait {
    pub fn blanket_trait_name(&self) -> Ident {
        blanket_trait_name(self.target_trait.clone())
    }
    fn generate_trait_decl(&self) -> TokenStream {
        let vtable_type = &self.vtable_type;
        let vis = &self.vis;
        let trait_name = self.blanket_trait_name();
        let vtable_method = vtable_method_name(&self.target_trait);
        quote! {
            #[allow(non_camel_case_types)]
            #vis unsafe trait #trait_name {
                fn data_ptr(&self) -> *mut core::ffi::c_void;
                #[allow(non_snake_case)]
                fn #vtable_method (&self) -> &'_ #vtable_type;
            }
        }
    }
}
impl ToTokens for PossibleSuperTrait {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        tokens.extend(self.to_token_stream());
    }
    fn to_token_stream(&self) -> TokenStream {
        let decl = self.generate_trait_decl();
        let blanket_impl = &self.blanket_impl;
        quote! {
            #decl
            #blanket_impl
        }
    }
}

pub fn handle_possible_super_trait(
    stash: &mut StageStash,
    vis: Visibility,
    config: &InheritanceConfig,
) -> syn::Result<Option<PossibleSuperTrait>> {
    let trait_object_name = &stash.trait_object_name;
    if config.possible_super_trait {
        if &stash.vtable_name != super_vtable_type(&stash.trait_name).simple_name() {
            // TODO: Lift this restriction
            return Err(syn::Error::new(
                stash.vtable_name.span(),
                "When a type is a possible super-trait, vtable names can't currently be customized",
            ));
        }
        let mut res = PossibleSuperTrait {
            vtable_type: stash.vtable_name.clone(),
            vis,
            target_trait: stash.trait_name.clone(),
            blanket_impl: None,
        };
        let blanket_trait_name = res.blanket_trait_name();
        let vtable_method_name = vtable_method_name(&res.target_trait);
        let vtable_name = &stash.vtable_name;
        res.blanket_impl = Some(quote! {
            unsafe impl #blanket_trait_name for #trait_object_name {
                #[inline]
                fn data_ptr(&self) -> *mut core::ffi::c_void {
                    self.as_raw() as *mut _
                }
                #[inline]
                fn #vtable_method_name(&self) -> &'_ #vtable_name {
                    self.vtable() // inherent impl
                }
            }
        });
        stash.target_impl = TargetImpl::BlanketTrait {
            trait_name: blanket_trait_name,
            vtable_method: vtable_method_name,
        };
        Ok(Some(res))
    } else {
        Ok(None)
    }
}

pub struct ExtendsSuperTrait {
    our_target: TargetImpl,
    super_trait: Path,
    super_trait_blanket_impl: Path,
    super_vtable_type: Path,
}
impl ExtendsSuperTrait {
    fn generate_blanket_impl(&self) -> TokenStream {
        let our_target = &self.our_target;
        let super_trait_blanket_impl = &self.super_trait_blanket_impl;
        let super_trait_vtable_method_name = vtable_method_name(&self.super_trait);
        let super_vtable_type = &self.super_vtable_type;
        let our_vtable_method = our_target.vtable_method_name();
        let our_data_ptr_method = our_target.data_ptr_method_name();
        let actual_impl = quote! {
            #[inline]
            fn data_ptr(&self) -> *mut core::ffi::c_void {
                self.#our_data_ptr_method() as *mut _
            }
            #[inline]
            fn #super_trait_vtable_method_name(&self) -> &'_ #super_vtable_type {
                &self.#our_vtable_method().super_trait_vtable
            }
        };
        match *our_target {
            TargetImpl::SpecificTraitObject {
                ref trait_object_name,
            } => {
                quote! {
                    unsafe impl #super_trait_blanket_impl for #trait_object_name {
                        #actual_impl
                    }
                }
            }
            TargetImpl::BlanketTrait {
                ref trait_name,
                vtable_method: _,
            } => {
                quote! {
                    unsafe impl<Target: #trait_name> #super_trait_blanket_impl for Target {
                        #actual_impl
                    }
                }
            }
        }
    }
}
impl ToTokens for ExtendsSuperTrait {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        tokens.extend(self.to_token_stream())
    }

    fn to_token_stream(&self) -> TokenStream {
        self.generate_blanket_impl()
    }
}

pub fn super_vtable_type(super_trait: &impl IdentOrPath) -> Path {
    // TODO: Support for custom super-type vtable names
    // When this is fixed, remove the check above
    super_trait
        .clone()
        .into_path()
        .with_simple_name(format_ident!("{}Vtable", super_trait.simple_name()))
}
pub fn handle_extends(
    stash: &mut StageStash,
    config: &InheritanceConfig,
) -> syn::Result<Option<ExtendsSuperTrait>> {
    if let Some(ref super_trait) = config.extends {
        let super_vtable_type = super_vtable_type(super_trait);
        let super_trait_blanket_impl = blanket_trait_name(super_trait.clone());
        Ok(Some(ExtendsSuperTrait {
            our_target: stash.target_impl.clone(),
            super_trait_blanket_impl,
            super_vtable_type: super_vtable_type.into(),
            super_trait: super_trait.clone(),
        }))
    } else {
        Ok(None)
    }
}

pub struct InheritanceConfig {
    pub extends: Option<Path>,
    possible_super_trait: bool,
}

impl From<InheritanceOptions> for InheritanceConfig {
    fn from(opts: InheritanceOptions) -> Self {
        let mut res = InheritanceConfig::default();
        for opt in opts {
            // TODO: Detect duplicates?
            // NOTE: Regular `Config` doesn't do this either....
            match opt {
                InheritanceOption::Extends { super_type, .. } => {
                    res.extends = Some(super_type);
                }
                InheritanceOption::PossibleSuperTrait { val, .. } => {
                    res.possible_super_trait = val.value;
                }
            }
        }
        res
    }
}
impl Default for InheritanceConfig {
    fn default() -> Self {
        InheritanceConfig {
            extends: None,
            possible_super_trait: false,
        }
    }
}
