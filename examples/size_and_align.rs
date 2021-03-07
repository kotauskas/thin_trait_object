use std::mem::{align_of, size_of};
use thin_trait_object::*;

#[thin_trait_object(store_layout = true)]
trait Foo {
    fn fooify(&self);
}
impl Foo for String {
    fn fooify(&self) {
        println!("Fooified a string: {}", self);
    }
}

fn main() {
    let boxed = BoxedFoo::new("Hello World!".to_string());
    boxed.fooify();
    assert_eq!(boxed.vtable().size, size_of::<String>());
    assert_eq!(boxed.vtable().align, align_of::<String>());
}
