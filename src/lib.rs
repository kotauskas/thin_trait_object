//! One pointer wide trait objects which are also FFI safe, allowing traits to be passed to/from and implemented by C ABI code.
//!
//! # Overview
//! Trait objects in Rust suffer from several fundamental limitations:
//! - **Pointers have twice the size** because trait objects are constructed with a pointer coercion rather than a value transformation — this means that the [virtual dispatch table] or a pointer to one cannot be stored inside the object and has to accompany pointers to that object, increasing size overhead for the pointers, especially for collections like `Vec<Box<dyn ...>>`;
//! - **Passing trait objects over an FFI boundary is impossible** because they do not have a defined memory layout and implementation;
//! - **No way to manually construct a trait object given only a dispatch table and a value**, i.e. to create a custom implementation which does not correspond to any type's implementation of the trait.
//!
//! For most purposes, those limitations are relatively easy to work around or are not applicable at all. However, in several scenarios, there is no possible solution and that is inherent to the nature of how trait objects work in Rust. Examples include:
//! - **Implementing a plugin system** where plugins residing inside dynamically loaded libraries (`.dll`/`.so`/`.dylib`) can be loaded by Rust code and then be used to extend the functionality of the base program using a defined interface;
//! - **Decreasing storage overhead for references/boxes/pointers** to trait objects, as in the `Vec<Box<dyn ...>>` example;
//! - **Implementing traits via JIT compilation of a different language**, though this is a very niche scenario.
//!
//! All those workloads fit the *pattern* of trait objects but don't fit the *implementation*. This crate serves as an alternate *implementation* of trait objects which serves the *pattern* while overcoming *limitations* of the compiler's built-in implementation. The functionality is provided in the form of an easy-to-use atttribute macro.
//!
//! The macro was **heavily** inspired by the design and implementation of an FFI-safe trait object described in the [*FFI-Safe Polymorphism: Thin Trait Objects*] article by Michael-F-Bryan. The article is a walkthrough for writing such a trait object manually, and this crate serves as the macro to perform the same task in an automated fashion.
//!
//! # Usage
//! The most basic use case:
//! ```rust
//! use thin_trait_object::*;
//!
//! #[thin_trait_object]
//! trait Foo {
//!     fn fooify(&self);
//! }
//! impl Foo for String {
//!     fn fooify(&self) {
//!         println!("Fooified a string: {}", self);
//!     }
//! }
//! BoxedFoo::new("Hello World!".to_string()).fooify();
//! ```
//! The macro will generate two structures (there's a third one but that's an implementation detail):
//! - **`FooVtable`**, the dispatch table (vtable) — a `#[repr(C)]` structure containing type-erased function pointer equivalents to all methods in the trait, as well as an additional `drop` function pointer called by `BoxedFoo` when it gets dropped (another attribute, `#[derive(Copy, Clone, Debug, Hash)]`, is added by default);
//! - **`BoxedFoo`**, analogous to `Box<dyn Foo>` in that it acts as a valid implementation of the `Foo` trait and has exclusive ownership of the contained value, which has the same memory layout as a [`core::ptr::NonNull`] to a type which implements `Sized`.
//!
//! Both of those will have the same visibility modifier as the trait on which the `#[thin_trait_object]` attribute is placed, unless you override it — the section up ahead is there to explain how.
//!
//! ## Configuring the macro
//! The basic invocation form, `#[thin_trait_object]`, will use the reasonable defaults for all possible configuration values. To override those configuration parameters, the following syntax is used:
//! ```rust
//! # /*
//! #[thin_trait_object(
//!     parameter1(value_for_the_parameter),
//!     parameter2(another_value),
//!     // Certain parameters require a slightly different syntax, like this:
//!     parameter3 = value,
//! )]
//! trait Foo {
//!     ...
//! }
//! # */
//! ```
//! The following options are supported:
//! - `vtable(<attributes> <visibility> <name>)` — specifies the visibility and name of the generated vtable structure and optionally attaches attributes to it *(that includes documentation comments)*.
//!   
//!   By default, `#[repr(C)]` and `#[derive(Copy, Clone, Debug, Hash)]` are attached, the visibility is taken from the trait definition, and the name is of form `<trait_name>Vtable`, as in `MyTraitVtable`.
//!
//!   `#[repr(C)]` will be overriden, while the `#[derive(...)]` will not be, meaning that specifying `#[derive(PartialEq)]`, for example, will add `PartialEq` to the list of traits being derived without overriding it.
//!   
//!   Example:
//!   ```no_run
//!   #[thin_trait_object(
//!       vtable(
//!           /// Documentation for my generated vtable.
//!   # /*
//!           #[repr(custom_repr)] // Will override the default #[repr(C)]
//!           #[another_fancy_attribute]
//!   # */
//!           pub MyVtableName // No semicolon allowed!
//!       )
//!   )]
//!   # trait MyTrait {}
//!   ```
//!   
//! - `trait_object(<attributes> <visibility> <name>)` — same as `vtable(...)`, but applies its effects to the generated boxed trait object structure.
//!   
//!   **Cannot attach a `#[derive(...)]` attribute for soundness reasons** (so that a `#[derive(Copy)]` wouldn't lead to undefined behavior without any usage of the `unsafe` keyword on the macro usage site.)
//!   
//!   By default, `#[repr(transparent)]` is attached (cannot be overriden), the visibility is taken from the trait definition, and the name is of form `Boxed<trait_name>`, as in `BoxedMyTrait`.
//!   
//! - `inline_vtable = <true/false>` — specifies whether the vtable should be stored directly in the trait object (`true`) or be stored as a `&'static` reference to the vtable. Set to `false` by default, and **overriding this is not recommended** unless the trait has very few (one or two) methods, or it is absolutely necessary to override this in order to be compatible with certain third-party code.
//!   
//!   Example:
//!   ```rust
//!   #[thin_trait_object(
//!       inline_vtable = true
//!   )]
//!   # trait MyTrait {}
//!   ```
//!   
//! - `drop_abi = "..."` — specifies the ABI (the `"C"` in `extern "C"`) for the `drop` function pointer in the vtable. The ABI for all other methods in the vtable can be specified in the trait definition directly.
//!   
//!   Example:
//!   ```rust
//!   #[thin_trait_object(
//!       drop_abi = "C" // Equivalent to extern "C" on a function/method
//!   )]
//!   # trait MyTrait {}
//!   ```
//! - `marker_traits(...)` — specifies a comma-separated list of traits which are to be considered marker traits, i.e. be implemented via an empty `impl` block on the generated thin trait object structure if the trait definition lists them as supertraits. Unsafe traits in the list need to be prefixed with the `unsafe` keyword.
//!   
//!   By default, the list is `marker_traits(unsafe Send, unsafe Sync, unsafe Unpin, UnwindSafe, RefUnwindSafe)`.
//!   
//!   See the [Supertraits](#supertraits) section for more on how the macro interacts with supertraits.
//!   
//!   Example:
//!   ```rust
//!   trait SafeTrait {}
//!   unsafe trait UnsafeTrait {}
//!   
//!   #[thin_trait_object(
//!       marker_traits(
//!           SafeTrait,
//!           // `unsafe` keyword here ensures that "unsafe code" is required
//!           // to produce UB by implementing the trait
//!           unsafe UnsafeTrait,
//!       )
//!   )]
//!   trait MyTrait: SafeTrait + UnsafeTrait {}
//!   ```
//!
//! ## Use with FFI
//! One of the main focuses of the macro is FFI, which is why usage of the macro with FFI is simple and natural:
//! ```no_run
//! use thin_trait_object::*;
//! use std::ffi::c_void;
//!
//! #[thin_trait_object(drop_abi = "C")]
//! trait Foo {
//!     extern "C" fn say_hello(&self);
//! }
//!
//! impl Foo for String {
//!     extern "C" fn say_hello(&self) {
//!          println!("Hello from \"{}\"", self);
//!     }
//! }
//!
//! # /*
//! extern "C" {
//!     fn eater_of_foo(foo: *mut c_void);
//!     fn creator_of_foo() -> *mut c_void;
//! }
//! # */
//! # extern "C" unsafe fn eater_of_foo(_foo: *mut c_void) {}
//! # extern "C" unsafe fn creator_of_foo() -> *mut c_void {
//! #     BoxedFoo::new("Rust pretending to be C".to_string()).into_raw() as *mut _
//! # }
//!
//! let foo = BoxedFoo::new("Hello World!".to_string());
//!
//! unsafe {
//!     // Will transfer ownership to the C side.
//!     eater_of_foo(foo.into_raw() as *mut c_void);
//! }
//! // Acquire ownership of a different implementation from the C side.
//! let foo = unsafe { BoxedFoo::from_raw(creator_of_foo()) };
//! foo.say_hello();
//! ```
//! The C side would do:
//! ```c
//! #include <stdio.h>
//!
//! typedef void (*vtable_say_hello)(void*);
//! typedef void (*vtable_drop)(void*);
//! typedef struct foo_vtable {
//!    vtable_say_hello say_hello;
//!    vtable_drop drop;
//! } foo_vtable;
//!
//! void eater_of_foo(void* foo) {
//!     // The first field is a pointer to the vtable, so we have to first
//!     // extract that pointer and then dereference the function pointers.
//!     foo_vtable* vtable = *((foo_vtable**)foo);
//!
//!     // Have to provide the pointer twice, firstly for
//!     // lookup and then to provide the &self reference.
//!     vtable.say_hello(foo);
//!     // Don't forget about manual memory management — the C side owns the trait object now.
//!     vtable.drop(foo);
//! }
//! void* creator_of_foo(void) {
//!     // Allocate space for one pointer, the pointer to the vtable.
//!     void* allocation = malloc(sizeof(foo_vtable*));
//!     void* vtable_pointer = &custom_vtable;
//!     // Put the pointer into the allocation.
//!     memcpy(allocation, &vtable_pointer, sizeof(foo_vtable*));
//!     return allocation;
//! }
//!
//! static foo_vtable custom_vtable {
//!     // Using C11 designated initializers, consult your local C expert for
//!     // ways to do this on an old compiler.
//!     .say_hello = &impl_say_hello,
//!     .drop = &impl_drop
//! };
//! void impl_say_hello(void* self) {
//!     puts("Hello from C!");
//! }
//! void impl_drop(void* self) {
//!     free(self);
//! }
//! ```
//!
//! ## Supertraits
//! Consider this situation:
//! ```compile_fail
//! use thin_trait_object::*;
//!
//! trait A {
//!     fn a(&self);
//! }
//! #[thin_trait_object]
//! trait B: A {
//!     fn b(&self);
//! }
//! ```
//! This will fail to compile because the macro will try to implement `B` for `BoxedB`, the generated thin trait object structure, which will fail because `BoxedB` doesn't implement `A`. To fix this, that must be done manually:
//! ```no_run
//! # use thin_trait_object::*;
//! # trait A {
//! #     fn a(&self);
//! # }
//! #[thin_trait_object]
//! trait B: A {
//!     fn b(&self);
//!     #[doc(hidden)]
//!     fn _thunk_a(&self) {
//!         self.a(); // Redirect to the method from the A trait implementation
//!     }
//! }
//! impl A for BoxedB {
//!     fn a(&self) {
//!         // Redirect to the hidden thunk, which will use the actual implementation of the method
//!         self._thunk_a();
//!     }
//! }
//! ```
//! This is necessary because the macro has no access to `A` and thus doesn't know that it needs to add its methods to the vtable.
//! A little hacky, but there is no cleaner way of doing this using only procedural macros. If you have any suggestions for improving this pattern, raise an issue explaining your proposed solution or create a PR.
//!
//! [*FFI-Safe Polymorphism: Thin Trait Objects*]: https://adventures.michaelfbryan.com/posts/ffi-safe-polymorphism-in-rust/ " "
//! [virtual dispatch table]: https://en.wikipedia.org/wiki/Virtual_method_table " "
//! [`core::ptr::NonNull`]: https://doc.rust-lang.org/std/ptr/struct.NonNull.html " "

#![deny(rust_2018_idioms)]
#![warn(missing_docs, clippy::cargo)]

use proc_macro::TokenStream;

mod attr;
use attr::*;
pub(crate) mod marker_traits;
pub(crate) mod options;
pub(crate) mod repr;
pub(crate) mod trait_object;
pub(crate) mod vtable;

#[cfg(test)]
mod tests;

/// Creates a thin trait object interface for a trait.
#[proc_macro_attribute]
pub fn thin_trait_object(attr: TokenStream, mut item: TokenStream) -> TokenStream {
    let output: TokenStream = attribute_main(attr.into(), item.clone().into())
        .unwrap_or_else(|error| error.to_compile_error())
        .into();
    // Concatenate the original trait definition and the generated additions
    // only here, for three reasons:
    // • This allows us to have both the trait definition and the custom
    //   compile error we produce, otherwise rustc itself will generate a
    //   massive amount of name resolution errors because the trait itself
    //   wouldn't exist if the macro fails
    // • This looks neater, since we separate the logic for generating the FFI
    //   support for the trait from the logic of interfacing with the compiler,
    //   and not discarding input is a part of the compiler interface logic.
    // • We can write concise unit tests which check the output of
    //   attribute_main (since only proc_macro2 is available in tests) and
    //   don't have to skip over the trait definition.
    item.extend(Some(output));
    item
}

/// Convinces [`cargo geiger`] that the crate has unsafe code.
///
/// Since we only generate unsafe code rather than call it directly, `cargo geiger` won't spot that and will report that it couldn't find any unsafe code (though not display the green `#![forbid(unsafe_code)]` smiley because we don't have `#![forbid(unsafe_code)]`).
///
/// To make it clear for users that the crate uses unsafe code in some form and thus is subject to security auditing, we have this unsafe function which is never called and will not do anything. This will immediately be unconditionally found by `cargo geiger` and reported as a form of unsafe code being used.
///
/// [`cargo geiger`]: https://crates.io/crates/cargo-geiger " "
unsafe fn _dummy_unsafe() {}
