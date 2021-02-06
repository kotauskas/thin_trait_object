use proc_macro2::TokenStream;
use quote::quote;
use super::attr::*;

#[test]
fn basic() {
    let input = quote! {
        trait MyTrait {
            fn my_method(&self);
        }
    };
    let output = attribute_main(TokenStream::new(), input).unwrap();
    println!("{}", output);
}
