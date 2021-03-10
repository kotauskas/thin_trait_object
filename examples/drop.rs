use thin_trait_object::*;

#[thin_trait_object]
trait Foo {
    fn touch_bytes(&self, f: fn(&u8));
}
impl Foo for Vec<u8> {
    fn touch_bytes(&self, f: fn(&u8)) {
        self.iter().for_each(f)
    }
}

fn main() {
    for _ in 0..512 {
        // Allocates 1 MiB per iteration, using up 512 MiB if not freed.
        let tto = BoxedFoo::new(vec![0; 1024 * 1024]);
        tto.touch_bytes(touch);
    }
}

#[inline(never)]
fn touch(_val: &u8) {}
