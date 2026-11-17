// Take a look at the license at the top of the repository in the LICENSE file.

#![allow(clippy::needless_doctest_main)]
// rustdoc-stripper-ignore-next
//! Module containing infrastructure for subclassing `GObject`s and registering boxed types.
//!
//! # Example for registering a `glib::Object` subclass
//!
//! The following code implements a subclass of `glib::Object` with a
//! string-typed "name" property.
//!
//! ```rust
//! use glib::prelude::*;
//! use glib::subclass;
//! use glib::subclass::prelude::*;
//! use glib::{Variant, VariantType};
//!
//! use std::cell::{Cell, RefCell};
//!
//! #[derive(Debug, Eq, PartialEq, Clone, Copy, glib::Enum)]
//! #[repr(u32)]
//! // type_name: GType name of the enum (mandatory)
//! #[enum_type(name = "SimpleObjectAnimal")]
//! enum Animal {
//!     Goat = 0,
//!     #[enum_value(name = "The Dog")]
//!     Dog = 1,
//!     // name: the name of the GEnumValue (optional), default to the enum name in CamelCase
//!     // nick: the nick of the GEnumValue (optional), default to the enum name in kebab-case
//!     #[enum_value(name = "The Cat", nick = "chat")]
//!     Cat = 2,
//! }
//!
//! impl Default for Animal {
//!     fn default() -> Self {
//!         Animal::Goat
//!     }
//! }
//!
//! #[glib::flags(name = "MyFlags")]
//! enum MyFlags {
//!     #[flags_value(name = "Flag A", nick = "nick-a")]
//!     A = 0b00000001,
//!     #[flags_value(name = "Flag B")]
//!     B = 0b00000010,
//!     #[flags_value(skip)]
//!     AB = Self::A.bits() | Self::B.bits(),
//!     C = 0b00000100,
//! }
//!
//! impl Default for MyFlags {
//!     fn default() -> Self {
//!         MyFlags::A
//!     }
//! }
//!
//! mod imp {
//!     use super::*;
//!
//!     // This is the struct containing all state carried with
//!     // the new type. Generally this has to make use of
//!     // interior mutability.
//!     // If it implements the `Default` trait, then `Self::default()`
//!     // will be called every time a new instance is created.
//!     #[derive(Default)]
//!     pub struct SimpleObject {
//!         name: RefCell<Option<String>>,
//!         animal: Cell<Animal>,
//!         flags: Cell<MyFlags>,
//!         variant: RefCell<Option<Variant>>,
//!     }
//!
//!     // ObjectSubclass is the trait that defines the new type and
//!     // contains all information needed by the GObject type system,
//!     // including the new type's name, parent type, etc.
//!     // If you do not want to implement `Default`, you can provide
//!     // a `new()` method.
//!     #[glib::object_subclass]
//!     impl ObjectSubclass for SimpleObject {
//!         // This type name must be unique per process.
//!         const NAME: &'static str = "SimpleObject";
//!
//!         type Type = super::SimpleObject;
//!
//!         // The parent type this one is inheriting from.
//!         // Optional, if not specified it defaults to `glib::Object`
//!         type ParentType = glib::Object;
//!
//!         // Interfaces this type implements.
//!         // Optional, if not specified it defaults to `()`
//!         type Interfaces = ();
//!     }
//!
//!     // Trait that is used to override virtual methods of glib::Object.
//!     impl ObjectImpl for SimpleObject {
//!         // Called once in the very beginning to list all properties of this class.
//!         fn properties() -> &'static [glib::ParamSpec] {
//!             use once_cell::sync::Lazy;
//!             static PROPERTIES: Lazy<Vec<glib::ParamSpec>> = Lazy::new(|| {
//!                 vec![
//!                     glib::ParamSpecString::builder("name")
//!                         .build(),
//!                     glib::ParamSpecEnum::builder::<Animal>("animal")
//!                         .build(),
//!                     glib::ParamSpecFlags::builder::<MyFlags>("flags")
//!                         .build(),
//!                     glib::ParamSpecVariant::builder("variant", glib::VariantTy::ANY)
//!                         .build(),
//!                 ]
//!             });
//!
//!             PROPERTIES.as_ref()
//!         }
//!
//!         // Called whenever a property is set on this instance. The id
//!         // is the same as the index of the property in the PROPERTIES array.
//!         fn set_property(&self, _id: usize, value: &glib::Value, pspec: &glib::ParamSpec) {
//!             match pspec.name() {
//!                 "name" => {
//!                     let name = value
//!                         .get()
//!                         .expect("type conformity checked by `Object::set_property`");
//!                     self.name.replace(name);
//!                 },
//!                 "animal" => {
//!                     let animal = value
//!                         .get()
//!                         .expect("type conformity checked by `Object::set_property`");
//!                     self.animal.replace(animal);
//!                 },
//!                 "flags" => {
//!                     let flags = value
//!                         .get()
//!                         .expect("type conformity checked by `Object::set_property`");
//!                     self.flags.replace(flags);
//!                 },
//!                 "variant" => {
//!                     let variant = value
//!                         .get()
//!                         .expect("type conformity checked by `Object::set_property`");
//!                     self.variant.replace(variant);
//!                 },
//!                 _ => unimplemented!(),
//!             }
//!         }
//!
//!         // Called whenever a property is retrieved from this instance. The id
//!         // is the same as the index of the property in the PROPERTIES array.
//!         fn property(&self, _id: usize, pspec: &glib::ParamSpec) -> glib::Value {
//!             match pspec.name() {
//!                 "name" => self.name.borrow().to_value(),
//!                 "animal" => self.animal.get().to_value(),
//!                 "flags" => self.flags.get().to_value(),
//!                 "variant" => self.variant.borrow().to_value(),
//!                 _ => unimplemented!(),
//!             }
//!         }
//!
//!         // Called right after construction of the instance.
//!         fn constructed(&self) {
//!             // Chain up to the parent type's implementation of this virtual
//!             // method.
//!             self.parent_constructed();
//!
//!             // And here we could do our own initialization.
//!         }
//!     }
//! }
//!
//! // Optionally, define a wrapper type to make it more ergonomic to use from Rust
//! glib::wrapper! {
//!     pub struct SimpleObject(ObjectSubclass<imp::SimpleObject>);
//! }
//!
//! impl SimpleObject {
//!     // Create an object instance of the new type.
//!     pub fn new() -> Self {
//!         glib::Object::new()
//!     }
//! }
//!
//! pub fn main() {
//!     let obj = SimpleObject::new();
//!
//!     // Get the name property and change its value.
//!     assert_eq!(obj.property::<Option<String>>("name"), None);
//!     obj.set_property("name", "test");
//!     assert_eq!(&obj.property::<String>("name"), "test");
//!
//!     assert_eq!(obj.property::<Animal>("animal"), Animal::Goat);
//!     obj.set_property("animal", Animal::Cat);
//!     assert_eq!(obj.property::<Animal>("animal"), Animal::Cat);
//!
//!     assert_eq!(obj.property::<MyFlags>("flags"), MyFlags::A);
//!     obj.set_property("flags", MyFlags::B);
//!     assert_eq!(obj.property::<MyFlags>("flags"), MyFlags::B);
//! }
//! ```
//!
//! # Example for registering a boxed type for a Rust struct
//!
//! The following code boxed type for a tuple struct around `String` and uses it in combination
//! with `glib::Value`.
//!
//! ```rust
//! use glib::prelude::*;
//! use glib::subclass;
//! use glib::subclass::prelude::*;
//!
//! #[derive(Clone, Debug, PartialEq, Eq, glib::Boxed)]
//! #[boxed_type(name = "MyBoxed")]
//! struct MyBoxed(String);
//!
//! pub fn main() {
//!     assert!(MyBoxed::static_type().is_valid());
//!
//!     let b = MyBoxed(String::from("abc"));
//!     let v = b.to_value();
//!     let b2 = v.get::<&MyBoxed>().unwrap();
//!     assert_eq!(&b, b2);
//! }
//! ```

pub mod basic;
#[macro_use]
pub mod types;

#[macro_use]
pub mod interface;

#[macro_use]
pub mod object;

#[macro_use]
pub mod boxed;

pub mod shared;

pub mod signal;

mod object_impl_ref;
pub use object_impl_ref::{ObjectImplRef, ObjectImplWeakRef};

pub mod prelude {
    // rustdoc-stripper-ignore-next
    //! Prelude that re-exports all important traits from this crate.
    pub use super::{
        boxed::BoxedType,
        interface::{ObjectInterface, ObjectInterfaceExt, ObjectInterfaceType},
        object::{DerivedObjectProperties, ObjectClassSubclassExt, ObjectImpl, ObjectImplExt},
        shared::{RefCounted, SharedType},
        types::{
            ClassStruct, InstanceStruct, InstanceStructExt, IsImplementable, IsSubclassable,
            IsSubclassableExt, ObjectSubclass, ObjectSubclassExt, ObjectSubclassIsExt,
            ObjectSubclassType,
        },
    };
}

pub use self::{
    boxed::register_boxed_type,
    interface::register_interface,
    signal::{
        Signal, SignalClassHandlerToken, SignalId, SignalInvocationHint, SignalQuery, SignalType,
    },
    types::{register_type, InitializingObject, InitializingType, TypeData},
};
