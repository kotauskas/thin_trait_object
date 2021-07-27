//! Generates the vtable struct itself.

use crate::attr::StageStash;
use proc_macro2::{Ident, Span, TokenStream};
use quote::{quote, ToTokens};
use replace_with::replace_with_or_abort;
use std::convert::TryFrom;
use syn::{
    punctuated::Punctuated,
    spanned::Spanned,
    token::{Colon, Paren, Unsafe},
    Abi,
    AttrStyle,
    Attribute,
    BareFnArg,
    BoundLifetimes,
    FnArg,
    GenericParam,
    Generics,
    LitStr,
    Pat,
    PatIdent,
    PatType,
    Path,
    PathArguments,
    PathSegment,
    Receiver,
    ReturnType,
    Signature,
    Token,
    TraitItem,
    TraitItemMethod,
    Type,
    TypePath,
    TypePtr,
    Variadic,
    Visibility,
};
use crate::inheritance::super_vtable_type;

pub fn generate_vtable(
    stash: &mut StageStash,
    visibility: Visibility,
    attributes: impl IntoIterator<Item = Attribute>,
    drop_abi: Option<&Abi>,
    store_layout: bool,
) -> TokenStream {
    let StageStash {
        vtable_items: items,
        vtable_name: name,
        ref super_trait,
        ..
    } = stash;
    let all_attributes = {
        let mut token_stream = TokenStream::new();
        let mut had_repr = false;
        for attr in attributes {
            if attr.path.get_ident().map(|x| x == "repr") == Some(true) {
                had_repr = true;
            }
            attr.to_tokens(&mut token_stream);
        }
        if !had_repr {
            repr_attribute().to_tokens(&mut token_stream);
        }
        token_stream
    };
    struct VtableItemToFnPtr(VtableItem);
    impl<'a> ToTokens for VtableItemToFnPtr {
        fn to_tokens(&self, out: &mut TokenStream) {
            out.extend(self.to_token_stream());
        }
        fn to_token_stream(&self) -> TokenStream {
            let name = self.0.name.clone();
            let ty = {
                let mut owned = self.0.clone();
                owned.make_unsafe();
                owned.make_raw();
                owned.to_function_pointer()
            };
            quote! { #name : #ty }
        }
    }
    struct VtableItemToDebugImplLine(VtableItem);
    impl<'a> ToTokens for VtableItemToDebugImplLine {
        fn to_tokens(&self, out: &mut TokenStream) {
            out.extend(self.to_token_stream());
        }
        fn to_token_stream(&self) -> TokenStream {
            let name = self.0.name.clone();
            let namelit = LitStr::new(&name.to_string(), Span::call_site());
            quote! { .field(#namelit, &(self.#name as *mut ())) }
        }
    }
    struct VtableItemToHashImplLine(VtableItem);
    impl<'a> ToTokens for VtableItemToHashImplLine {
        fn to_tokens(&self, out: &mut TokenStream) {
            out.extend(self.to_token_stream());
        }
        fn to_token_stream(&self) -> TokenStream {
            let name = self.0.name.clone();
            quote! { (self.#name as *mut ()).hash(state) }
        }
    }
    let vtable_entries = items.iter().cloned().map(VtableItemToFnPtr);
    let debug_impl_lines = items.iter().cloned().map(VtableItemToDebugImplLine);
    let hash_impl_lines = items.iter().cloned().map(VtableItemToHashImplLine);
    let name_strlit = LitStr::new(&name.to_string(), Span::call_site());
    let super_trait_decl = if let Some(ref super_trait) = super_trait {
        let super_vtable_type = super_vtable_type(super_trait);
        quote!(pub super_trait_vtable: #super_vtable_type,)
    } else {
        quote!()
    };
    let size_and_align = if store_layout {
        quote! {
            pub size: usize,
            pub align: usize,
        }
    } else {
        quote! {}
    };
    let drop_func = if super_trait.is_none() {
        quote! { pub drop: unsafe #drop_abi fn(*mut ::core::ffi::c_void), }
    } else {
        // only super-trait has the drop func, saving space
        quote! {}
    };
    let drop_impl = if super_trait.is_some() {
        quote!(self.super_trait_vtable.invoke_drop(ptr))
    } else {
        quote!((self.drop)(ptr))
    };
    quote! {
        #[derive(Copy, Clone)]
        #all_attributes
        #visibility struct #name {
            #super_trait_decl
            #size_and_align
            #(pub #vtable_entries,)*
            #drop_func
        }
        impl #name {
            #[inline]
            pub unsafe fn invoke_drop(&self, ptr: *mut core::ffi::c_void) {
                #drop_impl
            }
        }
        impl ::core::fmt::Debug for #name {
            fn fmt(&self, f: &mut ::core::fmt::Formatter) -> ::core::fmt::Result {
                f.debug_struct(#name_strlit)
                    #(#debug_impl_lines)*
                    .finish()
            }
        }
        impl ::core::hash::Hash for #name {
            fn hash<H: ::core::hash::Hasher>(&self, state: &mut H) {
                #(#hash_impl_lines;)*
            }
        }
    }
}

fn repr_attribute() -> Attribute {
    let path = {
        let mut segments = Punctuated::new();
        segments.push(PathSegment {
            ident: Ident::new("repr", Span::call_site()),
            arguments: PathArguments::None,
        });
        Path {
            leading_colon: None,
            segments,
        }
    };
    let tokens = {
        let mut token_stream = TokenStream::new();
        Paren::default().surround(&mut token_stream, |token_stream| {
            Ident::new("C", Span::call_site()).to_tokens(token_stream);
        });
        token_stream
    };
    Attribute {
        pound_token: Default::default(),
        style: AttrStyle::Outer,
        bracket_token: Default::default(),
        path,
        tokens,
    }
}

#[derive(Clone)]
pub enum VtableFnArg {
    Normal(BareFnArg),
    Receiver(Receiver),
}
impl VtableFnArg {
    pub fn into_bare_arg_with_ptr_receiver(self) -> BareFnArg {
        match self {
            VtableFnArg::Normal(arg) => arg,
            VtableFnArg::Receiver(arg) => BareFnArg {
                attrs: arg.attrs,
                name: None, // Fill this out later.
                ty: Type::Ptr(TypePtr {
                    star_token: Default::default(),
                    const_token: None,
                    mutability: Some(Default::default()),
                    elem: Type::Path(TypePath {
                        qself: None,
                        path: define_path![::, "core", "ffi", "c_void"],
                    })
                    .into(),
                }),
            },
        }
    }
}
impl ToTokens for VtableFnArg {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        match self {
            VtableFnArg::Normal(x) => x.to_tokens(tokens),
            VtableFnArg::Receiver(x) => x.to_tokens(tokens),
        }
    }
}
impl TryFrom<FnArg> for VtableFnArg {
    type Error = syn::Error;
    fn try_from(value: FnArg) -> Result<Self, Self::Error> {
        let success = match value {
            FnArg::Typed(ty) => Self::Normal(BareFnArg {
                attrs: ty.attrs,
                name: match *ty.pat {
                    Pat::Ident(x) => Some((x.ident, Default::default())),
                    _ => None,
                },
                ty: *ty.ty,
            }),
            FnArg::Receiver(receiver) => {
                if receiver.reference.is_none() {
                    // Pass-by-value, cannot have that just yet
                    return Err(syn::Error::new_spanned(
                        receiver.self_token,
                        "`#[thin_trait_object]` does not support pass-by-value just yet",
                    ));
                }
                Self::Receiver(receiver)
            }
        };
        Ok(success)
    }
}
impl From<BareFnArg> for VtableFnArg {
    #[inline]
    fn from(arg: BareFnArg) -> Self {
        Self::Normal(arg)
    }
}
impl From<Receiver> for VtableFnArg {
    #[inline]
    fn from(rec: Receiver) -> Self {
        Self::Receiver(rec)
    }
}

#[derive(Clone)]
pub struct VtableItem {
    pub lifetimes: BoundLifetimes,
    pub unsafety: Option<Unsafe>,
    pub abi: Option<Abi>,
    pub name: Ident,
    pub inputs: Punctuated<VtableFnArg, Token![,]>,
    pub variadic: Option<Variadic>,
    pub output: ReturnType,
}
impl VtableItem {
    #[inline]
    pub fn make_unsafe(&mut self) {
        if self.unsafety.is_none() {
            self.unsafety = Some(Default::default())
        }
    }
    pub fn to_function_pointer(&self) -> TokenStream {
        let inputs = self.inputs.iter();
        let lifetimes = &self.lifetimes;
        let unsafety = &self.unsafety;
        let abi = &self.abi;
        let variadic = &self.variadic;
        let output = &self.output;
        quote! {
            #lifetimes #unsafety #abi fn ( #(#inputs,)* #variadic ) #output
        }
    }
    pub fn into_signature(self, mut default_argname: impl FnMut(u32) -> Ident) -> Signature {
        let mut current_arg = 0;
        let mut inner_default_argname = || {
            let argname = default_argname(current_arg);
            current_arg += 1;
            argname
        };
        Signature {
            constness: None,
            asyncness: None,
            unsafety: self.unsafety,
            abi: self.abi,
            fn_token: Default::default(),
            ident: self.name,
            generics: lifetimes_to_generics(self.lifetimes),
            paren_token: Default::default(),
            inputs: self
                .inputs
                .into_iter()
                .map(|x| match x {
                    VtableFnArg::Normal(arg) => {
                        bare_fn_arg_to_fn_arg(arg, &mut inner_default_argname)
                    }
                    VtableFnArg::Receiver(rec) => FnArg::Receiver(rec),
                })
                .collect(),
            variadic: self.variadic,
            output: self.output,
        }
    }
    pub fn make_raw(&mut self) -> bool {
        let mut replaced = false;
        for input in &mut self.inputs {
            replace_with_or_abort(input, |x| {
                if matches!(x, VtableFnArg::Receiver(..)) {
                    replaced = true;
                }
                x.into_bare_arg_with_ptr_receiver().into()
            });
        }
        replaced
    }
}
impl TryFrom<TraitItemMethod> for VtableItem {
    type Error = syn::Error;
    fn try_from(method: TraitItemMethod) -> Result<Self, Self::Error> {
        let signature = method.sig;
        if signature.receiver().is_none() {
            return Err(syn::Error::new(
                signature.span(),
                "traits with associated functions cannot be made into trait objects",
            ));
        }
        if let Some(asyncness) = signature.asyncness {
            return Err(syn::Error::new(
                asyncness.span,
                "traits with async methods cannot be made into trait objects",
            ));
        }
        Ok(Self {
            lifetimes: generics_to_lifetimes(signature.generics)?,
            // The function pointer will be made unsafe later,
            // don't touch its unsafety just yet.
            unsafety: signature.unsafety,
            abi: signature.abi,
            name: signature.ident,
            inputs: signature
                .inputs
                .into_iter()
                .map(VtableFnArg::try_from)
                .collect::<Result<_, _>>()?,
            variadic: signature.variadic,
            output: signature.output,
        })
    }
}
fn bare_fn_arg_to_fn_arg(argument: BareFnArg, default_argname: impl FnOnce() -> Ident) -> FnArg {
    let pat = {
        let pat = PatIdent {
            attrs: Vec::new(),
            by_ref: None,
            mutability: None,
            ident: argument
                .name
                .map(|(x, _)| x)
                .unwrap_or_else(default_argname),
            subpat: None,
        };
        Box::new(Pat::Ident(pat))
    };
    FnArg::Typed(PatType {
        attrs: argument.attrs,
        pat,
        colon_token: Colon {
            spans: [Span::call_site()],
        },
        ty: Box::new(argument.ty),
    })
}
/// Checks through the generics of the function to see if it has any non-lifetime generics — if it doesn't, convert the generics to an HRTB for a function pointer, if it does, return an error stating that generics are not allowed in function pointers.
fn generics_to_lifetimes(generics: Generics) -> Result<BoundLifetimes, syn::Error> {
    if let Some(where_clause) = generics.where_clause {
        return Err(syn::Error::new_spanned(
            where_clause,
            "trait methods with `where` clauses are not object-safe",
        ));
    }
    let lifetimes = {
        let mut lifetimes = Punctuated::new();
        for param in generics.params {
            match param {
                GenericParam::Lifetime(lifetime) => lifetimes.push(lifetime),
                GenericParam::Type(ty) => {
                    return Err(syn::Error::new_spanned(
                        ty,
                        "generic type parameters are not object-safe",
                    ))
                }
                GenericParam::Const(constant) => {
                    return Err(syn::Error::new_spanned(
                        constant,
                        "generic constant parameters are not object-safe",
                    ))
                }
            }
        }
        lifetimes
    };
    Ok(BoundLifetimes {
        for_token: Default::default(),
        lt_token: generics.lt_token.unwrap_or_else(Default::default),
        lifetimes,
        gt_token: generics.gt_token.unwrap_or_else(Default::default),
    })
}
fn lifetimes_to_generics(lifetimes: BoundLifetimes) -> Generics {
    let params = lifetimes
        .lifetimes
        .into_iter()
        .map(GenericParam::Lifetime)
        .collect();
    Generics {
        lt_token: Some(lifetimes.lt_token),
        params,
        gt_token: Some(lifetimes.gt_token),
        where_clause: None,
    }
}
impl TryFrom<TraitItem> for VtableItem {
    type Error = syn::Error;
    fn try_from(item: TraitItem) -> Result<Self, Self::Error> {
        let span = item.span();
        match item {
            TraitItem::Method(method) => Self::try_from(method),
            TraitItem::Const(constant) => Err(syn::Error::new(
                constant.span(),
                "traits with associated constants cannot be made into trait objects",
            )),
            TraitItem::Type(..) => Err(syn::Error::new(
                span,
                "traits with associated types cannot be made into trait objects",
            )),
            TraitItem::Macro(..) => Err(syn::Error::new(
                span,
                "\
`#[thin_trait_object]` cannot expand macros, please type out the trait items directly",
            )),
            _ => Err(syn::Error::new(
                span,
                "\
traits with this kind of item cannot be made into trait objects (item type not recognized)",
            )),
        }
    }
}
