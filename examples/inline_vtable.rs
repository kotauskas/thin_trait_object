extern crate thin_trait_object;
use thin_trait_object::*;

#[thin_trait_object(inline_vtable = true)]
pub trait Foo {
    fn fooify(&self);
}
impl Foo for String {
    fn fooify(&self) {
        println!("Fooified a string: {}", self);
    }
}

fn main() {
    BoxedFoo::new("Hello World!".to_string()).fooify();
}
