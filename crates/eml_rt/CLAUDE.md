# eml_rt

The runtime core under the VM, verified with Verus. Its specification is docs/spec/object-model.md; how it is checked is docs/implementation/testing.md の「`eml_rt` の検証」.

- A change to `eml_rt` must pass `cargo verus verify -p eml_rt` and `verusfmt --verus-only --check $(git ls-files 'crates/eml_rt/src/*.rs')` besides `cargo test`, `cargo clippy --all-targets` and `cargo fmt`. `verusfmt` formats inside `verus!`, `cargo fmt` the rest.
- All code is inside `verus!` except the safe glue that docs/spec/object-model.md の「`unsafe` の核」 lists. What Verus does not check is the named trust list T0-T13 in the same section, pinned by `src/trust.rs`: a new FFI boundary adds a row there and a Miri test in `src/heap/tests/trusted.rs`.
- Unit tests take the mode as a type parameter and `both_modes!` runs them in fast and checked mode; internal functions get `Tracked::assume_new()` for their permissions. Miri runs only the `trusted` module, and tests do not shrink under `cfg!(miri)` (docs/implementation/testing.md の「Miri」).
- `src/heap/tests/expected.txt` pins the externs' results; an `#[ignore]` test rewrites it from `eml_rt`'s own results (docs/implementation/testing.md の「期待値の表」).
- `eml_rt` installs no signal handler; `eml_cli` owns SIGINT (docs/spec/object-model.md の「実行の制御」).
