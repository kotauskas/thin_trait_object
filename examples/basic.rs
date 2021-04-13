use thin_trait_object::*;

#[thin_trait_object]
trait Foo {
    fn fooify(&self, extra_data: &str);
}
impl Foo for String {
    fn fooify(&self, extra_data: &str) {
        println!(
            "Fooified a string: \"{}\" with extra data: \"{}\"",
            self, extra_data
        );
    }
}

fn main() {
    BoxedFoo::new("Hello World!".to_string()).fooify("Another string!");
}
