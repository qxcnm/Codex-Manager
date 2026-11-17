# PHF generator compatibility dependency update

This is the crates.io `phf_generator` 0.8.0 package with its `rand` dependency
updated from 0.7 to `^0.8.6` in both manifests, a focused round-trip regression
test, and this provenance file. The published library's obsolete standalone
`Cargo.lock` is omitted because the desktop application's lockfile controls this
dependency. The upstream MIT license is included from the source revision below.

- Package: <https://crates.io/crates/phf_generator/0.8.0>
- Original crate SHA-256:
  `17367f0cc86f2d25802b2c26ee58a7b23faeccf78a396094c13dced0d0182526`
- Source revision: `81c7cc5b48649108428671d3b8ad151f6fbdb359`
- License source:
  <https://github.com/sfackler/rust-phf/blob/81c7cc5b48649108428671d3b8ad151f6fbdb359/LICENSE>
- Removed dependency advisory:
  <https://github.com/advisories/GHSA-cq8v-f236-94qc>

Tauri's legacy HTML parser uses `selectors` 0.24, whose PHF build dependency
otherwise retains `rand` 0.7.3. Updating the HTML parser would also change its
`html5ever` types and break the current Tauri public API. Updating this build-only
generator's compatible random-number API removes the obsolete Rand branch while
retaining its PHF 0.8 hash/output format and public API.

`FIXED_SEED` and generation logic are unchanged. Rand's `SmallRng` algorithm can
change between versions, so generated keys and displacements need not match old
builds; they remain deterministic within the locked version and are emitted
together with the matching table. The regression checks generation plus lookup
for empty, singleton, Unicode/ASCII, and 1024-key tables, and repeated generation.

Run on an isolated copy when the repository workspace owns this directory:

```sh
cargo test --lib codexmanager_tests
```

Remove this patch when Tauri's complete build dependency chain no longer needs
the legacy PHF generator.
