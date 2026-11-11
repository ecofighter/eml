# Part 1: revised deliverable

## 1. Conclusion

### 1.1 Principle

- **P1 ownership.** A fact is *eml_rt-owned* iff eml_rt's own code (allocator, traversal, free, recovery, accessors, `Ctx` operations) interprets the same bits, so that a disagreeing code generator breaks memory safety. A fact eml_rt never interprets is a code-generator convention (owner: `eml_lir` if more than one consumer shares it, else the consumer). P1 decides who owns a fact, not whether it is exported.
- **P2 single spelling.** An eml_rt-owned fact has one spec definition and one exec definition, both in eml_rt, and eml_rt's own code uses them.
- **P3 owner by subject.** Three verified surfaces: *geometry* (where words are; a new dedicated layout module), *meta word bits* (`Meta`, `Kind`), *word tagging* (`Word`). Later surfaces of the same kind: `rc` (count encoding), `Ctx` (evidence roles). A fact is defined on one surface and never mirrored onto another.
- **P4 timing.** (a) `rt-layout` covers the geometry surface completely for `Kind::Data` and the header: every existing spelling, inside or outside eml_rt, moves onto the module. (b) Any other eml_rt-owned fact is exported in the stage whose emitted code first reads those bits itself; until then it is not exported at all. (c) The one existing duplicate outside the geometry surface, evidence field roles (eml_vm `vm.rs`/`forks.rs` write what eml_rt `Ctx` reads), is a named exception assigned to jit-effects (1.2).
- **P5 import, never write numbers.** Code generators obtain every eml_rt-owned value from eml_rt `const fn`s / `const`s. No layout literal outside eml_rt.

### 1.2 Contract scope (question 1)

| Item | Owner (P1) | Surface | Exported in |
|---|---|---|---|
| `Data` payload start: count word present (`array` forced, or scanned >= 255), `scan` byte, first scanned word index, first raw word index (= first + scanned), payload word count | eml_rt | layout module | **rt-layout** |
| Array/MutArray: count at word 0, element i at word 1+i | eml_rt | layout module; it is the row above with `array = true`, no separate rule | **rt-layout** |
| Header geometry: header size, rc byte offset 0, meta byte offset 4, word size 8, byte offset of payload word i | eml_rt | layout module | **rt-layout** |
| Closure as an object: it is an ordinary `Data` with `scanned = 1 + refs`, `raw = other captures`; physical index of logical scanned word j is `first(1 + refs, false) + j`, of raw capture j is `first + scanned + j` | eml_rt (geometry) | layout module (same encoder, no closure-specific item) | **rt-layout** (`apply.rs` build and read sites) |
| Closure contents: `f \| k << 32` immediate, which capture goes to which logical slot | code generator | eml_vm today; if jit-core shares capture placement with the VM the function goes to `eml_lir` next to `shape` | - |
| Constructor field order (scanned in declared order, then raw): `LayoutCtor::shape`, logical positions | code generator | `eml_lir` | - |
| Meta bit packing (shifts/masks, raw `u32`), `EXTENDED_SCAN` value | eml_rt | `Meta` (already single-spelled, verified) | jit-core raises visibility; `EXTENDED_SCAN` itself stops being `pub` in rt-layout (1.3) |
| Word tagging (low bit, shift 1, `()` = 1, `MIN_INT`/`MAX_INT`) | eml_rt | `Word` (already single-spelled, verified) | jit-core raises visibility |
| rc encoding read by inline `dup`/`decref` | eml_rt | `rc` | jit-core |
| BigInt/Float box: value at payload word 0 | eml_rt | layout module | jit-core |
| Raw: count at word 0, element i at word 1+i; String/Bytes: byte length at word 0, content from word 1 | eml_rt | layout module, as two kind-keyed word-index consts shared by the three kinds (length word = 0, content first word = 1); additive, does not pass through the `Data` encode/decode | native-inline, which reroutes eml_rt's internal `slot_ptr(h, 1)` / `1 + i` sites in the same stage |
| Evidence field roles (`[evidence, next]`, `[entry table, state, outer evv]`) | eml_rt | `Ctx` | jit-effects (P4c): its effects design chooses between `Ctx` constructors and role consts; until then eml_vm's writers keep the role numbers and take only geometry from the module |
| Kind numbering | eml_rt | `Kind` | not exported; no stage plans a reader |
| VM-private packings (frame word, cell ids) | eml_vm | - | - |
| `Ctx` `#[repr(C)]` field offsets | eml_rt, **separate mechanism** | `offset_of!`-derived `pub const`s next to `Ctx`, outside the layout module | jit-core |

`Ctx` offsets are separate: they are rustc struct-layout facts, not functions of an object's meta that the heap invariant speaks about; Verus proves nothing about them and they change with every field. They share only P5.

### 1.3 Form (question 2)

- One dedicated `pub` module in eml_rt, inside `verus!`. In rt-layout it holds: the `Data` encode and decode, header byte consts, the word-index-to-byte-offset function, and the inline-scan bound const (largest-plus-one scanned count that never has a count word without `array`; value 255).
- **Spec is the definition.** `pub open spec fn`s in the module define the rules. `p_extended`, `p_first`, `slot_offset`, every `scan == 255` / `scan != 255` predicate in specs (preconditions of `alloc`/`alloc_immortal`/`con_at`/`reuse_in_place`, array shape specs in `heap/ctx/arrays.rs`, `heap/ctx/proof.rs`, `states.rs`), the `scan_of`/raw-range clauses of `alloc_data*` and `lemma_data_perm` are written with them. The direction is forced: a `pub` fn's `ensures` cannot name a `pub(crate)` spec fn [V].
- **Exec twins.** `#[inline] pub const fn` with `ensures r == <spec>`. Scalar and small `Copy` struct results both work in const contexts, including from eml_vm [V].
- **`Data`-only, two directions.** *Encode* `(scanned, array)` -> (count word?, `scan` byte, first, first raw). *Decode* `scan` byte -> (extended?, first). Both are named and documented as `Kind::Data` rules; the decode is meaningful only for `Data`. No "is array" predicate: array-ness is not readable from meta (a tag-0 `Data` with >= 255 scanned words, e.g. a closure with >= 254 reference captures, has the same meta) [I from object-model.md 「クロージャ」/「配列」]; array checks use the decode's `extended` predicate plus kind and tag as today.
- **Unit.** Word indices are primary. One function converts a payload word index to a byte offset from the object pointer; header fields are byte consts. No rule in both units.
- **eml_rt goes through it.** The four exec decoders (`heap/core.rs` `scan_get`, `heap/graph.rs:918`, `remote.rs:297`, `heap/ctx.rs:2492`), `heap/ctx/arrays.rs:106`, `alloc_data_rc`, `meta_ptr`, `slot_ptr`.
- **`Meta::EXTENDED_SCAN` is no longer `pub`** (`pub(crate)`, defined in `meta.rs`, read by the module). Crates outside eml_rt then cannot name it; they use the encoder and the bound const.
- **Verus proves:** (a) exec == spec for each fn; (b) decode(encode) agreement, and `!count_word ==> scan byte == scanned`; (c) `alloc_data`, `alloc_data_ext`, `alloc_array_immortal` postconditions stated with the module's spec fns; (d) `slot_ptr`/`meta_ptr` bodies use the module's consts under their existing `ensures`.
- **Plain tests / const asserts pin:** threshold 255, header 8, rc 0, meta 4, word 8, encode samples at 0, 254, 255, 300 with and without `array`; `offset_of!(Header, rc)`, `offset_of!(Header, meta)`, `size_of::<Header>()` against the consts (compile-time, outside `verus!`). These go in ordinary unit tests, not `trusted::`, so the Miri budgets are unaffected [I: Miri runs only `trusted::`].

### 1.4 Ownership vs eml_lir (question 3)

- `eml_lir` does not depend on eml_rt. No new shared leaf crate.
- `LayoutCtor::shape` stays in `eml_lir`; `Shape.positions` are logical. Physical word index = eml_rt `first(shape.scanned, false)` + position.
- `eml_lir` gets one named constant, its own policy: reuse pairs only constructors with fewer than that many scanned words (value 255). `reuse_words` and the verifier message use it; the message text stays byte-identical ("255 or more scanned words", formatted from the constant).
- **One tie:** a compile-time assert `eml_lir limit <= eml_rt inline-scan bound` in each crate that lowers LIR reuse onto eml_rt (eml_vm now, eml_native in jit-core). `<=` is the soundness condition; the two numbers are different facts and may diverge downward.
- **Reuse path:** eml_rt offers a `const fn` that builds the meta of a `Data` without a count word, with the Verus precondition `scanned < bound`. The VM reuse path (`data.rs:372`) calls it instead of `Meta::new(.., scanned as u8, Kind::Data, ..)`. No new run-time or compile-time check is added: the precondition holds because the LIR verifier pairs only shapes under the eml_lir limit (the existing `debug_assert!(shape.reuse_words().is_some())` at `compile/emit.rs:564` stays as the check of that LIR guarantee) and the const assert relates the limit to the bound.
- Other VM sites: `forks.rs` (`table`, the 3- and 2-word shapes) and `vm.rs:1932-1938, 2396-2400` take meta and `first` from the encoder (const items where the count is constant); `forks.rs`'s `!= EXTENDED_SCAN` filter becomes a check on the encoder's count-word result.

### 1.5 Staging and AOT (question 4)

- **rt-layout:** everything marked rt-layout in 1.2, the eml_rt reroute of 1.3, the VM and eml_lir changes of 1.4, pins, spec text, the native.md fix, roadmap text.
- **jit-core:** visibility of `Meta` shifts/masks/raw bits, `Word` consts, rc consts; box value word in the module (with eml_rt's own sites rerouted); the eml_native const assert; `Ctx` offsets as `offset_of!` consts.
- **jit-effects:** evidence roles on the `Ctx` surface, removing the eml_vm/eml_rt duplicate.
- **native-inline:** the two body consts, with eml_rt's own sites rerouted.
- **AOT stability.** `rt-layout` adds no version or fingerprint. For the JIT, code generator and runtime are the same eml_rt crate instance linked into one executable, so they cannot disagree. For AOT, native-aot MUST add a link-time check that emitted objects and `libeml_rt.a` come from the same eml_rt-native ABI (object layout, `Ctx` offsets, entry symbols, extern word ABI); how the id is derived is native-aot's decision. This deliverable only guarantees the input: every eml_rt-owned value is a `const`/`const fn` (G-4) and `Ctx` offsets are `offset_of!` consts (G-12), so a const-computed id can cover actual rustc layout as well as declared numbers. Decision #10's "形と配置の中身" includes that id.

### 1.6 Doc inconsistency (question 5)

Contract: count at `base + 8`, Raw element i at `base + 16 + 8*i` (object-model.md 「配列」). Code agrees: `unboxed_payload` returns `slot_ptr(h, 1)`, `slot_ptr(h, i)` = `addr + 8 * (HEADER_WORDS + i)`, `HEADER_WORDS = 1` [V: read `heap/unboxed.rs:196-202`, `meta.rs:486-520`]. native.md L120 (`base + 8 + 8*i`) is wrong [V: read]. `rt-layout` fixes it by replacing the number with a reference to object-model.md 「配列」.

## 2. Rationale

- P1 draws the line where a mismatch is unsafe. eml_rt never decodes the closure immediate [V: `grep -rnE '<< 32|>> 32' crates/eml_rt/src` outside tests gives only `control.rs:732`, unrelated]; it does allocate and traverse the closure object, so the closure's geometry is eml_rt's while its contents are not.
- The count-word rule is the fact with many spellings: 43 non-test, non-comment lines outside `externs.rs` in eml_rt, 3 in eml_vm, 2 in eml_lir, 0 in eml_core_ir/eml_native/eml_cabi [V: grep, list in 7]. All 43 are scan-related; `externs.rs`'s 40 are `Byte` ranges.
- Moving all of them costs no proof work (7), so the criterion can be "none left" instead of a judgment call.
- Header geometry enters now because `meta_ptr`/`slot_ptr` already consume it and the change verified unchanged; `Meta` and `Word` are already single-spelled and reached only through their API from eml_vm [I from R1 A1], so they need visibility only.
- eml_lir -> eml_rt would pull vstd and the runtime into the `core`-feature build (`core = ["types", "dep:eml_core_ir", "dep:eml_lir"]`) [V: read `crates/eml_cli/Cargo.toml`] and break architecture.md's leaf rule.
- Evidence roles are deferred although duplicated today because removing the duplicate is a `Ctx` API choice (constructors vs role consts) that the effects design of native code determines; rt-layout would guess it.

## 3. Alternatives considered and rejected

- **Minimum only (`count_word` moved to eml_rt).** eml_rt's decoders, allocator and specs keep their own spellings; nothing proves the export is the rule the heap uses.
- **Everything now (Meta masks, Word consts, body kinds, boxes, evidence roles).** Exports without a reader; evidence needs a design rt-layout does not have.
- **Byte-offset table per kind.** `Heap` accessors and VM operands are word indices; two units for one rule.
- **eml_lir depends on eml_rt / shared leaf crate / threshold as a pass parameter.** Leaf rule and `core` build; premise (eml_rt depends only on vstd); plumbing a runtime fact through `eml_core_ir`.
- **Equality between the eml_lir limit and the eml_rt bound.** Couples two facts; `<=` is sufficient.
- **Per-reuse check against the encoder in the bytecode compiler.** A second statement of what the const assert already gives for all programs.
- **`LayoutCtor::shape` in eml_rt.** eml_rt does not interpret field order.
- **"Is array" predicate in the module.** Not decodable from meta.
- **`Ctx` offsets in the layout module; a fingerprint in rt-layout.** Different mechanism; no consumer before AOT.

## 4. Assumptions made

- A1 [V, was I]: the reroute does not change eml_rt fast-mode instruction counts beyond noise (7). S2 re-measures on the real change.
- A2 [I]: native code is fast-mode, little-endian, 64-bit (`usize::BITS == 64` is asserted today).
- A3 [I]: AOT constants are made by `freeze` at startup by default (native.md), so emitted objects carry no object images.
- A4 [I]: `Meta::EXTENDED_SCAN` has no user outside eml_rt after the VM changes (today: `eml_vm/src/compile/forks.rs:82` only [V: grep]).

## 5. Implications for implementation

### rt-layout delivers
1. The layout module (1.3), added to `src/trust.rs`'s source-file list (otherwise `trust::every_source_file_is_listed` fails [V]) and verusfmt-formatted.
2. eml_rt reroute: all 43 lines of 7's list rewritten onto the module's spec/exec items; `EXTENDED_SCAN` made `pub(crate)`.
3. eml_vm: `data::count_word` deleted; `compile/emit.rs` `field_word`, `apply.rs:234` and `:295-305`, `data.rs:372`, `compile/forks.rs:79-108`, `vm.rs:1932-1938` and `2396-2400` per 1.4.
4. eml_lir: the named limit; eml_vm: the `<=` const assert.
5. Pins and const asserts (1.3).
6. Gates: `cargo verus verify -p eml_rt` (0 errors, no warnings), verusfmt, nextest, clippy, fmt, Miri `trusted::` (no new trusted tests), `bench/rt.sh` (U5).
7. Test changes: none of kind 1 or 2. No test names `count_word`, `reuse_words` or the verifier message [V: grep over `crates/*/tests` and `tests/`]; the message text and `RunStats` are unchanged by design. eml_rt tests that name `Meta::EXTENDED_SCAN` keep compiling (same crate). New pin tests are additions.

### Doc changes in rt-layout (same commits as the code)
- object-model.md: a section stating the contract (P1-P5 as rules, the rt-layout rows of 1.2, the module); 「ペイロード」: positions are logical, physical = eml_rt first + position, eml_rt owns the count word, eml_lir owns field order and its reuse limit, tied by the `<=` const assert (replaces "結び付けのテストは要らない" for the limit); 「クロージャ」: the closure's geometry is the `Data` rule with `scanned = 1 + refs`.
- vm.md 「データとクロージャ」 L193-194, L215; lir.md 「reuse の元」 (names the eml_lir limit).
- native.md L120 (1.6); L98 (`Ctx` offsets are `offset_of!` consts exported by eml_rt).
- roadmap.md: jit-core, jit-effects, native-inline, native-aot texts gain their rows of 1.5; `rt-layout` deleted at close.
- testing.md 「`eml_rt` の検証」: what Verus proves about the module and what the pins cover.
- architecture.md and the CI step `dependency edges of eml_native`: unchanged in rt-layout [V: read ci.yml L97-102].

## 6. Unresolved items (with defaults)

- U1: struct-returning encoder vs separate fns. Default: S2's choice; both verify and const-evaluate [V].
- U3: constant-count hand-built shapes. Default: const items computed from the encoder; no new `Heap` API.
- U4: `compile/constants.rs:94,113` BigInt/Float boxes (`alloc_immortal(1, Meta::new(0, 0, Kind::BigInt|Float, false))`, `set_raw(w, 0, ..)`). Default: unchanged in rt-layout and listed as allowed residue in G-5; jit-core, which exports the box value word, removes them.
- U5: `bench/rt.sh` acceptance. `/usr/bin/time -l` counts are not exact. Default: no compute row (rows above `read-all` in the table) more than +0.1% over a same-session baseline; observed spread in 7 was -0.47%..+0.04%.
- U7: enforcing G-1 by a source-scan unit test beside `trust.rs`'s file scan instead of by grep. Default: add it if it is a few lines on the existing scanner; otherwise the grep in G-1 is the check.

(U2 and U6 of r1 are decided in 1.4 and 1.3.)

## 7. Additional checks

The experiments ran on a scratch copy of the base commit; the final design is in 02-stage-design.md and its reference diff 03-reference.diff.

- [V] Experiment 1: `crates/eml_rt/src/layout.rs` with pub open spec fns, `pub const fn` twins (one returning a `Copy` struct, one calling `Meta::new`), pub consts; `slot_offset`, `p_extended`, `p_first` defined through it; `scan_get` routed. `cargo verus verify -p eml_rt`: 1211 verified, 0 errors.
- [V] Experiment 2 (full reroute): additionally rewrote, by script, every scan-255 spelling in eml_rt onto the module: the `alloc_data` / `alloc_data_ext` / `alloc_data_rc` ensures (scan byte and raw range), all `scan_of(..) == 255` / `scan_spec() != 255` / `bits_scan(..) != 255` specs in `heap/mod.rs`, `reuse.rs`, `states.rs`, `ctx/arrays.rs`, `ctx/proof.rs`, the exec decoders in `graph.rs`, `remote.rs`, `ctx.rs`, `ctx/arrays.rs:106`, `alloc_data_rc`'s body (via the struct encoder), `meta_ptr` and `slot_ptr` (via consts). Result: **1213 verified, 0 errors, 27 s wall, no proof body edited**. Remaining hits of `grep -rnE '\b255(u8|usize)?\b|EXTENDED_SCAN'` outside tests, comments, `externs.rs` and the module: `meta.rs:154` (definition) and two lines of `lemma_data_perm`'s `requires` (not attempted; same form).
- [V] Pub fn `ensures` naming a `pub(crate)` spec fn is rejected by Verus ("cannot refer to private function").
- [V] Cross-crate const use: `const` items in `crates/eml_vm/src/data.rs` calling the encoder, the byte-offset fn and the meta builder compile with `cargo check -p eml_vm`; a deliberately false const assert fails with E0080, so they are evaluated.
- [V] `bench/rt.sh` on base vs experiment 2 (one session, 41 rows): compute rows between -0.47% and +0.04%; `free` +0.005%, `list` -0.008%, `map-reuse` -0.13%, `mask` -0.25%. I/O rows moved -1.3%..-4.8% (noise; untouched code).
- [V] Hit list used for the counts: eml_rt 43 lines in `heap/mod.rs` (20), `ctx/arrays.rs` (5), `ctx/proof.rs` (4), `states.rs` (2), and one each in `core.rs`, `ctx.rs`, `graph.rs`, `inv.rs`, `reuse.rs`, `meta.rs`, `remote.rs`; eml_vm `data.rs:13`, `compile/forks.rs:82,83`; eml_lir `lib.rs:792`, `verify.rs:985`.
- [V] eml_vm sites: `Meta::new(.., Kind::Data, ..)` at `data.rs:372`, `forks.rs:86,101,106`; literal payload indices at `vm.rs:1935-1938, 2399-2400`, `constants.rs:95,114`.
- [V] `trust::every_source_file_is_listed` fails until the new file is listed; unformatted file fails `verusfmt --check`.
- Not checked: full nextest; Miri; `lemma_data_perm` restated; the eml_lir constant and VM edits themselves.

## 8. Conformance criteria

- **G-1** MUST: in `crates/eml_rt/src`, the scan-255 rule is spelled only in the layout module. Why: P2. Applies to: eml_rt. Check: `grep -rnE '\b255(u8|usize)?\b|EXTENDED_SCAN' crates/eml_rt/src | grep -v '/tests/' | grep -vE ':[0-9]+:\s*//'` returns lines only from (a) the layout module file, (b) `externs.rs`, (c) the single definition line of `EXTENDED_SCAN` in `meta.rs`. Any other line fails. (Baseline today: 43 failing lines.)
- **G-2** MUST: `p_extended`, `p_first`, `slot_offset` are defined by the module's `pub open spec fn`s; each exported exec fn has `ensures r == <module spec>`. Why: the export is provably the heap's rule. Applies to: eml_rt. Check: read the three definitions; `cargo verus verify -p eml_rt` 0 errors, no warnings.
- **G-3** MUST: `alloc_data`, `alloc_data_ext`, `alloc_array_immortal` state `scan` byte, first index and raw range with the module's spec fns. Why: index validity as a caller-visible theorem. Check: read the `ensures` (also implied by G-1).
- **G-4** MUST: exported contract items are `pub const fn` / `pub const`. Why: codegen-time evaluation; AOT id input. Check: eml_vm's `<=` const assert and at least one const item built from the encoder compile.
- **G-5** MUST NOT: a crate other than eml_rt spell a geometry fact. Applies to: eml_vm, eml_lir, eml_core_ir, eml_native, eml_cabi. Checks, all on `src/` with comment lines removed:
  - `EXTENDED_SCAN` is not `pub` (compile-enforced); `grep -rn 'count_word' crates/eml_vm/src` is empty.
  - `grep -rnE '\b255\b'`: eml_vm 0 lines; eml_lir exactly 1 (the limit's definition); eml_core_ir, eml_native, eml_cabi 0.
  - `grep -rnE 'Meta::new\([^;]*Kind::Data' crates/eml_vm/src` is empty (today 4).
  - `grep -rnE 'heap\.(set_field|field|raw|set_raw)\([^,()]+,\s*[0-9]' crates/eml_vm/src` returns only `compile/constants.rs` (2 lines, U4).
  - The design lists a replacement for each of: `emit.rs` `field_word`, `apply.rs:234`, `apply.rs:295-305`, `data.rs:372`, `forks.rs:79-108`, `vm.rs:1932-1938`, `vm.rs:2396-2400`.
  - From jit-core on, in eml_native: no integer literal as a load/store offset or mask of object memory.
- **G-6** MUST NOT: `eml_lir` depend on eml_rt; eml_rt depend on anything but vstd. Check: `cargo tree -p eml_lir -e normal`, `cargo tree -p eml_rt -e normal`.
- **G-7** MUST: eml_lir's reuse limit is one named constant; each crate lowering LIR reuse onto eml_rt has `const _: () = assert!(limit <= bound)`. MUST NOT: add any other check relating the two. Why: one tie. Applies to: eml_lir, eml_vm, eml_native (jit-core). Check: grep the assert; G-5's eml_lir count.
- **G-8** MUST: `Shape.positions` stay logical and field order stays in `eml_lir`. Check: `crates/eml_lir/tests/shape.rs` unchanged; object-model.md 「ペイロード」.
- **G-9** MUST: an eml_rt-owned fact is defined on exactly one surface; later stages raise visibility or add to that surface. The body-kind items are two kind-keyed word-index consts and leave the `Data` encode/decode signatures unchanged. Applies to: jit-core and later. Check: stage design names the surface per item.
- **G-10** MUST: a stage that exports a fact eml_rt already reads internally routes eml_rt's own sites through it in that stage. Applies to: jit-core (box word), jit-effects (evidence roles, also removing eml_vm's role literals), native-inline (body consts). Check: grep for the old spelling (`slot_ptr(h, 1)`, `1 + i`, role literals) is empty for that fact.
- **G-11** MUST: unit tests or const asserts pin each numeric value of the module and `offset_of!`/`size_of` of `Header`. Applies to: eml_rt, each stage adding items. Check: the test names 255, 8, 0, 4, 8 and the four encode samples.
- **G-12** MUST: `Ctx` field offsets are `offset_of!`-derived consts exported by eml_rt outside the layout module, delivered by jit-core; MUST NOT appear as numbers in eml_native. Check: jit-core design; grep.
- **G-13** MUST NOT: `rt-layout` add a version or fingerprint. MUST: native-aot add a link-time ABI check between emitted objects and `libeml_rt.a`. Check: stage designs.
- **G-14** MUST: each offset is stated once in docs (object-model.md); native.md refers to it. Check: `grep -rn 'base + 8 + 8' docs` is empty after rt-layout.
- **G-15** MUST: `rt-layout` change no `RunStats` snapshot and meet U5 on `bench/rt.sh`; a deviation is reported before acceptance. Check: `crates/eml_vm/tests/bench.rs` unchanged; before/after table.
- **G-16** MUST NOT: eml_vm gain evidence-role or closure-content abstractions in rt-layout; those sites change geometry only. Why: P4c, scope. Check: diff of `vm.rs`/`forks.rs`/`apply.rs`.

---

