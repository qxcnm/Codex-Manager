# Rust GLib and GObject bindings

__Rust__ bindings and wrappers for [GLib](https://docs.gtk.org/glib/), part of [gtk-rs-core](https://github.com/gtk-rs/gtk-rs-core).

GLib __2.56__ is the lowest supported version for the underlying library.

This library contains bindings to GLib and GObject types and APIs as well as
common building blocks used in both handmade and machine generated
bindings to GTK and other GLib-based libraries.

It is the foundation for higher level libraries with uniform Rusty (safe and
strongly typed) APIs. It avoids exposing GLib-specific data types where
possible and is not meant to provide comprehensive GLib bindings, which
would often amount to duplicating the Rust Standard Library or other utility
crates.

## Minimum supported Rust version

Currently, the minimum supported Rust version is `1.70.0`.

## Dynamic typing

Most types in the GLib family have [`Type`] identifiers.
Their corresponding Rust types implement the [`StaticType`] trait.

A dynamically typed [`Value`] can carry values of any [`StaticType`].
[`Variant`](struct@Variant)s can carry values of [`StaticVariantType`].

## Errors

Errors are represented by [`Error`], which can
carry values from various [error domains](error::ErrorDomain) such as
[`FileError`].

## Objects

Each class and interface has a corresponding smart pointer struct
representing an instance of that type (e.g. [`Object`] for `GObject` or
`gtk::Widget` for `GtkWidget`). They are reference counted and feature
interior mutability similarly to Rust's `Rc<RefCell<T>>` idiom.
Consequently, cloning objects is cheap and their methods never require
mutable borrows. Two smart pointers are equal if they point to the same
object.

The root of the object hierarchy is [`Object`].
Inheritance and subtyping is denoted with the [`IsA`]
marker trait. The [`Cast`] trait enables upcasting
and downcasting.

Interfaces and non-leaf classes also have corresponding traits (e.g.
[`ObjectExt`] or `gtk::WidgetExt`), which are blanketly implemented for all
their subtypes.

You can create new subclasses of [`Object`] or other object types. Look at
the module's documentation for further details and a code example.

## Under the hood

GLib-based libraries largely operate on pointers to various boxed or
reference counted structures so the bindings have to implement corresponding
smart pointers (wrappers), which encapsulate resource management and safety
checks. Such wrappers are defined via the
[`wrapper`][`macro@wrapper`] macro, which uses abstractions
defined in the [`wrapper`][`mod@wrapper`], [`boxed`][`mod@boxed`],
[`shared`][`mod@shared`] and [`object`][`mod@object`] modules.

The [`translate`][`mod@translate`] module defines and partly implements
conversions between high level Rust types (including the aforementioned
wrappers) and their FFI counterparts.

## Documentation

 * [Rust API - Stable](https://gtk-rs.org/gtk-rs-core/stable/latest/docs/glib/)
 * [Rust API - Development](https://gtk-rs.org/gtk-rs-core/git/docs/glib)
 * [C API](https://docs.gtk.org/glib/)
 * [GTK Installation instructions](https://www.gtk.org/docs/installations/)

## Using

We recommend using [crates from crates.io](https://crates.io/keywords/gtk-rs),
as [demonstrated here](https://gtk-rs.org/#using).

If you want to track the bleeding edge, use the git dependency instead:

```toml
[dependencies]
glib = { git = "https://github.com/gtk-rs/gtk-rs-core.git", package = "glib" }
```

Avoid mixing versioned and git crates like this:

```toml
# This will not compile
[dependencies]
glib = "0.13"
glib = { git = "https://github.com/gtk-rs/gtk-rs-core.git", package = "glib" }
```

## License

__glib__ is available under the MIT License, please refer to it.
