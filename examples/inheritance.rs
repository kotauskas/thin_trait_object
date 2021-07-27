use thin_trait_object::*;

#[thin_trait_object(
    inheritance(
        possible_super_trait = true
    )
)]
pub trait Bar {
    fn baring(&self, extra: i32);
}

#[thin_trait_object(
    inheritance(
        extends(Bar)
    )
)]
pub trait Foo: Bar {
    fn fooify(&self, extra_data: &str);
}

impl Bar for String {
    fn baring(&self, extra: i32) {
        println!("Barred a string: {:?} with {}", self, extra)
    }
}
impl Foo for String {
    fn fooify(&self, extra_data: &str) {
        println!(
            "Fooified a string: \"{}\" with extra data: \"{}\"",
            self, extra_data
        );
    }
}

fn accept_foo(f: &BoxedFoo) {
    f.fooify("tacoz");
    accept_bar(f.as_bar());
}
fn accept_bar(b: &BoxedBar) {
    b.baring(5);
}

fn main() {
    accept_foo(&BoxedFoo::new("Horray".to_string()))
}