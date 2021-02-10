use thin_trait_object::*;

#[thin_trait_object(
    /*marker_traits(
        SafeMarker,
        // `unsafe` keyword here ensures that "unsafe code" is required
        // to produce UB by implementing the trait
        unsafe UnsafeMarker,
    )*/
    marker_traits()
)]
trait Foo: SafeMarker + UnsafeMarker {
    fn fooify(&self);
}

trait SafeMarker {}
unsafe trait UnsafeMarker {}

impl Foo for String {
    fn fooify(&self) {
        println!("Fooified a string: {}", self);
    }
}
impl SafeMarker for String {}
unsafe impl UnsafeMarker for String {}
impl SafeMarker for BoxedFoo<'_> {}
unsafe impl UnsafeMarker for BoxedFoo<'_> {}

fn main() {
    BoxedFoo::new("Hello World!".to_string()).fooify();
}
