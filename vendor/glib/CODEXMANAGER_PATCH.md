# GLib 0.18 compatibility security backport

This directory contains the unmodified crates.io `glib` 0.18.5 package, except
for the `VariantStrIter::impl_get` security fix and its regression test in
`src/variant_iter.rs`, plus this provenance file. The upstream MIT license and
copyright notices are retained.

- Source: <https://crates.io/crates/glib/0.18.5>
- Original crate SHA-256:
  `233daaf6e83ae6a12a52055f568f9d7cf4671dabb78ff9560ab6da230ce00ee5`
- Advisory: <https://rustsec.org/advisories/RUSTSEC-2024-0429.html>
  (`GHSA-wrw7-89jp-8q8g`)
- Upstream fix: <https://github.com/gtk-rs/gtk-rs-core/pull/1343>
- Fixed upstream release used to verify the implementation:
  <https://crates.io/crates/glib/0.20.0>

The backport makes the C output pointer mutable and passes `&mut p` to
`g_variant_get_child`. It preserves the GLib 0.18 public API and GTK dependency
compatibility required by Tauri's Linux stack. Upgrading only the application's
direct GLib dependency would leave the vulnerable transitive GLib 0.18 copy.

The package version intentionally stays 0.18.5. Version-only vulnerability
scanners may continue to report this advisory; the code is locally backported,
and the advisory must not be silently ignored or dismissed as a version upgrade.
Remove this override and directory when Tauri's complete Linux GTK dependency
chain uses a compatible fixed upstream GLib release.

Run the focused regression with a recent Rust compiler and native GLib development
libraries (on an isolated copy if the repository workspace owns this directory):

```sh
cargo test --release --lib variant_iter::tests::test_variant_str_iter
```

The release profile is intentional: the original invalid shared-reference write
can be optimized away. The regression covers forward/reverse iteration, an empty
string, Unicode, and ASCII; existing upstream string-iterator tests are retained.
