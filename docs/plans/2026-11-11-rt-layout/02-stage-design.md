# Part 1: revised deliverable

## 1. Conclusion

- **D-1 Module.** New file `crates/eml_rt/src/layout.rs`.
  - Declare it as `pub mod layout;` in `lib.rs` after `mod host;`. Do not re-export it at the crate root.
  - List it in `src/trust.rs` `FILES` after `host.rs`.
  - Everything is inside `verus!` except the one const assert and the test module.
- **D-2 U1 decided: separate scalar `const fn`s, no struct.**
  - Each exec fn has one spec twin (`ensures r == <spec>`), which is G-2 taken literally.
  - No aggregate type becomes part of the native ABI.
  - The only site that needs several outputs is `alloc_data_rc`. Its calls are inlined, and its bench delta is noise.
- **D-3 255 is spelled once.**
  - `layout::INLINE_SCAN_LIMIT: usize = 255` holds the literal.
  - `Meta::EXTENDED_SCAN` stays in `meta.rs` as `pub(crate) const EXTENDED_SCAN: u8 = crate::layout::INLINE_SCAN_LIMIT as u8;`.
  - A const assert in `layout.rs`, outside `verus!`, guards the cast: `const _: () = assert!(Meta::EXTENDED_SCAN as usize == INLINE_SCAN_LIMIT);`.
  - The module doc states that the marker and the limit are one number by design, because `scan` bytes below the limit are literal counts.
  - Why the literal lives in the module: Verus rejects a `pub` const or a `pub open spec fn` whose body refers to a `pub(crate)` const. I ran it and got "in pub open spec function, cannot refer to private const item" [V].
- **D-4 Header geometry moves into the module.**
  - `HEADER_WORDS` moves out of `meta.rs` and becomes `pub`. New items: `WORD_BYTES`, `RC_BYTE`, `META_BYTE`, `slot_byte_offset`. There is no `HEADER_BYTES`; the header size in bytes is `slot_byte_offset(0)`.
  - `meta::slot_offset` becomes an alias of `layout::slot_offset_spec`.
  - `rc_ptr`, `meta_ptr` and `slot_ptr` take their offsets from the module.
  - `BlockPerm::from_raw`, `into_raw`, `slots_from_raw`, `slots_into_raw` and `block_size` split permissions with `RC_BYTE`, `META_BYTE`, `slot_offset(..)` and `WORD_BYTES`.
  - Every `8 * (HEADER_WORDS + x)` and `8 * (1 + x)` bound (13 lines, `heap/unboxed.rs` included) becomes `crate::layout::slot_offset_spec(x as int)` [V].
  - The remaining literal `8` and `4` spellings are named exceptions (E1-E6 in 5.2), each with a P1 or P4 reason and an exact count.
- **D-5 Count-word rule in eml_rt.** All 38 lines that G-1 flags at HEAD move onto the module. The decoders branch on `scan_extended(scan)` and take `first` from `scan_first(scan)`.
- **D-6 Count-word position (C-1).** New items `COUNT_WORD: usize = 0` and `EXTENDED_FIRST: usize = COUNT_WORD + 1`. Both specs define `first` as `EXTENDED_FIRST` when there is a count word. The following are rerouted [V]:
  - `p_count`;
  - the 8 invariants of the form `p_extended(p) ==> … p_init(p, 0)`;
  - the exec count-word reads in `scan_get`, `push_children` and `foreign_children`;
  - the count-word write and loop invariant in `alloc_data_rc`;
  - the `i != 0` proof steps in `field.rs` and `safe.rs`;
  - `recover.rs`'s `f.first == 1`;
  - all Array element and count positions in `heap/ctx/arrays.rs`, exec and proof (40 sites).
  
  No proof body needed new reasoning.
- **D-7 `lemma_data_perm`.** Gains a `force: bool` parameter; its three scan `requires` are restated with module specs [V].
- **D-8 Payload words.** `data_words(scanned, raw, array) -> Option<usize>` (checked arithmetic) replaces `alloc_data_rc`'s nested `checked_add` [V].
- **D-9 eml_vm.**
  - `data::count_word` is deleted, and every eml_vm site takes geometry from the module (5.3).
  - Fixed shapes use function-local `const` items computed from the encoder (U3 default).
  - No new `Heap` API.
  - G-16 holds: role numbers and closure packing are unchanged.
  - Run-time guarding: `data_meta` (requires no count word) and `data_raw_first` (requires `scanned < usize::MAX`) are called from plain Rust, so their Verus preconditions are not checked at run time. The guards are:
    - the const assert of D-11;
    - the LIR verifier's reuse pairing;
    - the debug assert at `compile/emit.rs:564`;
    - the run-time `assert!` in `compile/forks.rs` `table`.
    
    The plan states this explicitly.
- **D-10 eml_lir.** `Shape::REUSE_SCANNED_LIMIT: u32 = 255`, an associated const. `reuse_words` uses it, and the verifier formats its message from it; the message stays byte-identical ("255 or more scanned words").
- **D-11 The one tie.** In `crates/eml_vm/src/data.rs`: `const _: () = assert!(Shape::REUSE_SCANNED_LIMIT as usize <= INLINE_SCAN_LIMIT);`. Comments may name the constant; the G-7 grep filters comment lines.
- **D-12 Tests.**
  - New in `layout.rs`: 2 pin tests and 2 source-scan tests (U7 adopted). The source-scan tests use `trust.rs`'s scanner, so `FILES`, `prepare` and `word_at` become `pub(crate)`.
  - New const asserts: 4 in `meta.rs`, 1 in `layout.rs`, 1 in eml_vm.
  - In TDD order inside T1: the module and its tests come first. The scan test then fails until the reroute is done.
  - Test-change kinds: none of kind 1 or 2. The `trust.rs` edits are kind 3.
- **D-13 U5 gate (replaces S1's default; G-15 kept).**
  - Base is the T0 tree and after is the T3 tree. Both are built and run in one worktree path, alternating by checkout. 5 pairs.
  - Gate: per row, the median delta of `instructions retired` must be <= +0.1%. This applies to every row except `empty`, `println-*`, `read-*` and `stdout-*`.
  - Why this replaces S1's "rows above `read-all`": `empty` (startup only) and `println-line` lie above `read-all`, and identical builds differ on them by up to 0.54% and 0.24% (bimodal: 11.358e9 or 11.386e9 for either build) [V: 9 pairs and 6 direct runs].
  - The gate runs as the first step of T4 (macOS), not in T1.
  - A failure is reported before acceptance (G-15).
- **D-14 Spec section.** `### 語の位置` in `docs/spec/object-model.md`, under `## 物体`, between `### ペイロード` and `### 種類`. Code cites `docs/spec/object-model.md の「語の位置」`.
- **D-15 Commits.**
  - T0: plan.
  - T1: eml_rt module and reroute, pins, spec section (eml_rt half), testing.md, eml_rt CLAUDE.md.
  - T2: eml_lir limit, eml_vm reroute, the tie, `EXTENDED_SCAN` made `pub(crate)`, and the VM, LIR and payload docs, including 「語の位置」's consumer rule.
  - T3: native.md and roadmap text.
  - T4: bench gate, then `Close rt-layout`.

## 2. Rationale

- With specs as the definition and every eml_rt predicate written through them, Verus proves that the exported exec fns compute the heap's own rule. The count-word position, the Array element positions and the slot offsets are routed the same way. Verification went from 1217 to 1220 with no new proof reasoning [V].
- `EXTENDED_FIRST = COUNT_WORD + 1` makes "the scanned words follow the count word" a definition, not a coincidence. Array element i is at `EXTENDED_FIRST + i` (S1 1.2 row 2: "the row above with `array = true`").
- With the count-word index exported, native-inline reads the Array length at `slot_byte_offset(COUNT_WORD)` with no new item. native-inline's G-9 body consts stay limited to Raw/String/Bytes.
- The gate moves to T4 so that T1 and T2 can be completed on any OS. Running both sides in one path removes the build-path difference as a noise source.

## 3. Alternatives considered and rejected

- **Struct encoder.** It needs one `ensures` per field and creates a new ABI type. One site needs several outputs.
- **255 literal in both `meta.rs` and `layout.rs`.** That is two spellings; Verus visibility forces the cross-reference (D-3).
- **The const assert of D-3 in `meta.rs`.** It would be a second `EXTENDED_SCAN` line in `meta.rs`, which fails G-1 [V: the scan test failed].
- **A `HEADER_BYTES` const.** That is a second unit for one fact.
- **Names containing `count_word`.** G-5's grep `grep -rn 'count_word' crates/eml_vm/src` would match them [V].
- **A logical-index `Heap::set_scanned` for the VM.** It is new `Heap` API.
- **Deferring the count-word index (r1 D-13).** It is inside S1's rt-layout row, and rerouting it verified without proof work [V].
- **`ARRAY_FIRST` as a separate export.** That is a second name for one rule; `EXTENDED_FIRST` serves arrays and Data with 255 or more scanned words alike.

## 4. Assumptions made

- A1 [V]: no test names `count_word`, `reuse_words`, the verifier message, or the fork-table panic text (grep over `crates/*/tests`, `crates/*/src`, `tests/`).
- A2 [V]: `cargo tree -p eml_lir -e normal` shows only `eml_extern` and `rustc-hash`. `cargo tree -p eml_rt -e normal` shows only `vstd`.
- A3 [I]: Miri `trusted::` is unaffected, because no trusted test is added.
- A4 [V]: when nothing changed since the last run, `cargo verus verify -p eml_rt` prints no `verification results` line (cached) and exits 0. The checks below therefore rely on the exit code plus the absence of `error`/`warning`/`note:` lines.

## 5. Implications for implementation

### 5.1 API (`crates/eml_rt/src/layout.rs`)

Module doc (intent):
- This module is the only definition of the object positions that eml_rt reads itself: the header geometry and the `Kind::Data` count word.
- Code generators take every value from here.
- The `scan` marker equals `INLINE_SCAN_LIMIT` by design.
- Cite object-model 「語の位置」.

| Item (exact) | Intent | Verus / pin |
|---|---|---|
| `pub const WORD_BYTES: usize = 8;` | bytes per word | const assert `== size_of::<Word>()` (meta.rs) |
| `pub const HEADER_WORDS: usize = 1;` | header size in words | const assert `HEADER_WORDS * WORD_BYTES == size_of::<Header>()` |
| `pub const RC_BYTE: usize = 0;` | byte offset of `rc` | const assert `== offset_of!(Header, rc)` |
| `pub const META_BYTE: usize = 4;` | byte offset of `meta` | const assert `== offset_of!(Header, meta)` |
| `pub const COUNT_WORD: usize = 0;` | payload word of the count word when present | pin |
| `pub const EXTENDED_FIRST: usize = COUNT_WORD + 1;` | first scanned word when the count word is present (arrays always) | pin |
| `pub const INLINE_SCAN_LIMIT: usize = 255;` | smallest non-array scanned count with a count word; the `scan` marker | pin; const assert `Meta::EXTENDED_SCAN as usize == INLINE_SCAN_LIMIT` |
| `pub open spec fn slot_offset_spec(i: int) -> int { WORD_BYTES * (HEADER_WORDS + i) }` | byte offset of payload word i | defines `meta::slot_offset` |
| `pub open spec fn data_extended_spec(scanned: nat, array: bool) -> bool { array \|\| scanned >= INLINE_SCAN_LIMIT }` | count word present | |
| `pub open spec fn data_scan_spec(scanned: nat, array: bool) -> u8` | `INLINE_SCAN_LIMIT as u8` if extended, else `scanned as u8` | |
| `pub open spec fn data_first_spec(scanned: nat, array: bool) -> int` | `EXTENDED_FIRST as int` if extended, else 0 | |
| `pub open spec fn data_words_spec(scanned: nat, raw: nat, array: bool) -> int` | `data_first_spec + scanned + raw` | |
| `pub open spec fn scan_extended_spec(scan: u8) -> bool { scan as int == INLINE_SCAN_LIMIT }` | decode (Data only) | defines `p_extended` |
| `pub open spec fn scan_first_spec(scan: u8) -> int` | `EXTENDED_FIRST as int` if `scan_extended_spec`, else 0 | defines `p_first` |
| `pub proof fn lemma_data_scan(scanned: nat, array: bool)`, ensures: `scan_extended_spec(data_scan_spec(s,a)) == data_extended_spec(s,a)`, `scan_first_spec(data_scan_spec(s,a)) == data_first_spec(s,a)`, `!data_extended_spec(s,a) ==> data_scan_spec(s,a) as nat == s` | decode inverts encode | empty body |
| `#[inline] pub const fn data_extended(scanned: usize, array: bool) -> (r: bool)` | | `ensures r == data_extended_spec(scanned as nat, array)` |
| `#[inline] pub const fn data_scan(scanned: usize, array: bool) -> (r: u8)` | returns `Meta::EXTENDED_SCAN` when extended | `ensures r == data_scan_spec(..)` |
| `#[inline] pub const fn data_first(scanned: usize, array: bool) -> (r: usize)` | physical index of logical scanned word 0 | `ensures r == data_first_spec(..)` |
| `#[inline] pub const fn data_raw_first(scanned: usize, array: bool) -> (r: usize)` | physical index of raw word 0 | `requires scanned < usize::MAX`, `ensures r == data_first_spec(..) + scanned` |
| `#[inline] pub const fn data_words(scanned: usize, raw: usize, array: bool) -> (r: Option<usize>)` | payload words; `None` on overflow | `ensures r matches Some(n) ==> n == data_words_spec(..)`, `r is None ==> data_words_spec(..) > usize::MAX` |
| `#[inline] pub const fn data_meta(tag: u16, scanned: usize, lin: bool) -> (r: Meta)` | meta of a Data without a count word | `requires !data_extended_spec(scanned as nat, false)`, `ensures` tag, `scan == data_scan_spec(scanned, false)`, `scan == scanned`, kind `Data`, `lin` |
| `#[inline] pub const fn scan_extended(scan: u8) -> (r: bool)` | | `ensures r == scan_extended_spec(scan)` |
| `#[inline] pub const fn scan_first(scan: u8) -> (r: usize)` | | `ensures r == scan_first_spec(scan)` |
| `#[inline] pub const fn slot_byte_offset(i: usize) -> (r: usize)` | byte offset of payload word i from the object pointer | `requires slot_offset_spec(i as int) <= usize::MAX`, `ensures r == slot_offset_spec(i as int)` |

The exact bodies are in `crates/eml_rt/src/layout.rs` of `03-reference.diff`. Doc comments must also say that decoding applies only to `Kind::Data`, and that there is no "is array" predicate because a tag-0 Data with 255 or more scanned words has the same meta as an array.

### 5.2 eml_rt sites (HEAD line numbers)

Reconciliation: S1's 43 lines are the 38 eml_rt lines that G-1 flags at HEAD [V], plus 3 eml_vm lines (`data.rs:13`, `forks.rs:82,83`), plus 2 eml_lir lines (`lib.rs:792`, `verify.rs:985`). The last 5 are covered in 5.3.

**Count-word rule (the G-1 lines) and header items:**

| File:line | Replacement |
|---|---|
| `lib.rs:21` | add `pub mod layout;` |
| `trust.rs` | add the `("layout.rs", include_str!("layout.rs")),` row; make `FILES`, `prepare` and `word_at` `pub(crate)` |
| `meta.rs:154` | `EXTENDED_SCAN = crate::layout::INLINE_SCAN_LIMIT as u8` (stays `pub` in T1; becomes `pub(crate)` in T2) |
| `meta.rs:485-486` | delete `HEADER_WORDS`; add `use crate::layout::{HEADER_WORDS, META_BYTE, RC_BYTE, WORD_BYTES, slot_byte_offset};` |
| `meta.rs:496-498` | `slot_offset` body becomes `crate::layout::slot_offset_spec(i)` |
| `meta.rs:503` (`rc_ptr` ensures) | `field_ptr::<i32>(h, RC_BYTE as int)` |
| `meta.rs:510-515` (`meta_ptr`) | requires `h@.addr + META_BYTE <= usize::MAX`; ensures `field_ptr::<u32>(h, META_BYTE as int)`; body `h.addr() + META_BYTE` |
| `meta.rs:526` (`slot_ptr`) | `h.addr() + slot_byte_offset(i)` |
| `meta.rs:868` | `HEADER_WORDS * WORD_BYTES == size_of::<Header>()`; add `offset_of!(Header, rc) == RC_BYTE`, `offset_of!(Header, meta) == META_BYTE`, `WORD_BYTES == size_of::<Word>()` |
| `HEADER_WORDS` imports: `heap/mod.rs:48`, `heap/proof.rs:16`, `alloc.rs:22`, `frozen.rs:29`, `remote.rs:19-21` | import from `crate::layout`. In `frozen.rs`, keep `#[cfg(verus_only)]` on both resulting lines [V: clippy flags it otherwise] |
| `heap/core.rs:187-192` (`scan_get`) | `let scan = m.scan(); let first = crate::layout::scan_first(scan); if crate::layout::scan_extended(scan) { (first, self.slot_get(h, crate::layout::COUNT_WORD).bits()) } else { (first, scan as usize) }` |
| `heap/graph.rs:918-923` | same shape; `ptr_ref(slot_ptr(h, crate::layout::COUNT_WORD), Tracked(p.slots.tracked_borrow(crate::layout::COUNT_WORD as int)))` |
| `remote.rs:297-301` | same shape; `let w: Word = unsafe { slot_ptr(h, COUNT_WORD).read() }; w.bits()`. Bind to a `let`: verusfmt cannot parse `unsafe {…}.bits()` as a tail expression [V] |
| `heap/ctx.rs:2492-2496` | `let i = crate::layout::scan_first(tm.scan()) + op;` |
| `heap/inv.rs:33` | `crate::layout::scan_extended_spec(bits_scan(p.meta.value()))` |
| `heap/inv.rs:95-101` (`p_first`) | `crate::layout::scan_first_spec(bits_scan(p.meta.value()))` |
| `heap/mod.rs:1167,1186,1203`; `heap/reuse.rs:489` | `!crate::layout::scan_extended_spec(meta.scan_spec()),` |
| `heap/mod.rs:1338-1353`, `1384-1399`, `1452-1467` (ensures of `alloc_data` / `alloc_data_ext` / `alloc_data_rc`) | `final(self).scan_of(w) == crate::layout::data_scan_spec(scanned as nat, false\|force)`; raw range `crate::layout::data_first_spec(scanned as nat, false\|force) + scanned <= i < crate::layout::data_first_spec(..) + scanned + raw ==> #[trigger] final(self).raw_at(w, i)` |
| `heap/mod.rs:1423` (`alloc_array_immortal`) | `final(self).scan_of(w) == crate::layout::data_scan_spec(0, true),` [V] |
| `heap/mod.rs:1476-1493` (`alloc_data_rc` body) | `let extended = crate::layout::data_extended(scanned, force); let scan = crate::layout::data_scan(scanned, force); let first = crate::layout::data_first(scanned, force); let payload_words = match crate::layout::data_words(scanned, raw, force) { Some(n) => n, None => too_large() };` |
| `heap/mod.rs:1667-1677` (`lemma_data_perm`; new param `force: bool` after `extended`; the call passes `force`) | `extended == crate::layout::data_extended_spec(scanned as nat, force), meta.scan_spec() == crate::layout::data_scan_spec(scanned as nat, force), first == crate::layout::data_first_spec(scanned as nat, force),` |
| `heap/states.rs:467, 663` | `!crate::layout::scan_extended_spec(m.scan_spec())` / `(bits_scan(m.bits_spec()))` |
| `heap/ctx/proof.rs:860-864, 912-916` | `i == crate::layout::scan_first_spec(s0.heap.scan_of(s0.ev_table(ev))) + op,` |
| `heap/ctx/proof.rs:1554, 1589` | `!crate::layout::scan_extended_spec(h.scan_of(w)),` / `(h0.scan_of(w))` |
| `heap/ctx/arrays.rs:26, 275, 333, 377` | `crate::layout::scan_extended_spec(h.scan_of(a\|t))` |
| `heap/ctx/arrays.rs:106` | `\|\| !crate::layout::scan_extended(m.scan())` |

**Count-word position and Array positions (D-6).** `CW` = `crate::layout::COUNT_WORD`; `EF` = `crate::layout::EXTENDED_FIRST`.

| File:line (HEAD) | Replacement |
|---|---|
| `heap/inv.rs:102` (`p_count`) | `p_word(p, CW as int)` |
| invariants `p_extended(..) ==> … p_init(.., 0)`: `heap/inv.rs:453`, `heap/safe.rs:131, 543, 1002, 1102`, `heap/graph.rs:362`, `frozen.rs:133`, `heap/field.rs:104` | `p_init(.., CW as int)` on those lines only. The `boxes_value()` lines stay (box word, jit-core) |
| `heap/mod.rs:1460-1463` (count-word write) | `tracked_remove(CW as int)`, `slot_ptr(h, CW)`, `tracked_insert(CW as int, slot)` |
| `heap/mod.rs:1480, 1632` | `extended ==> p_init(perm, CW as int) && p_word(perm, CW as int).bits_spec() == scanned,` |
| `heap/mod.rs:1509, 1529` | `assert(perm.slots[CW as int] == before.slots[CW as int])` / `scanned_perm` |
| `heap/field.rs:176, 296`; `heap/safe.rs:628-629` | `assert(i != CW);`, `assert(p.slots[CW as int] == q.slots[CW as int]);`. The `safe.rs:655-659` `boxes_value` block stays |
| `heap/recover.rs:1380` | `…, f.first == crate::layout::EXTENDED_FIRST);` |
| `heap/ctx/arrays.rs:337, 551, 557, 604, 610, 1371` | `1 + j` becomes `EF as int + j` |
| `heap/ctx/arrays.rs:338` | `EF <= j < EF + h.count_of(t)` |
| `heap/ctx/arrays.rs:339, 547, 600, 1365` | `word_at(t\|a, CW as int)` |
| `heap/ctx/arrays.rs:449, 450, 483, 520, 537, 543, 582, 591, 597` | `1 + i` becomes `EF as int + i` |
| `heap/ctx/arrays.rs:772` | `heap.set_field(t, EF + i, w)` |
| `heap/ctx/arrays.rs:1075` | `set_field(t, EF + last, x)` |
| `heap/ctx/arrays.rs:1199, 1249, 1307` | `p == EF + idx,` |
| `heap/ctx/arrays.rs:1625, 1646, 1664` | `(EF + idx) as int` |
| `heap/ctx/arrays.rs:1628, 1637, 1732` | `field(a, EF + idx)` / `set_field(a\|t, EF + idx, x)` |
| `heap/ctx/arrays.rs:1754` (`fill(.., idx + 1, ..)`) | unchanged: a logical element index |

**Header-geometry spellings (C-2):**

| File:line (HEAD) | Replacement |
|---|---|
| `8 * (HEADER_WORDS + x)`: `meta.rs:638`, `frozen.rs:123`, `alloc.rs:772`, `heap/mod.rs:274, 298, 1107, 1513`, `heap/unboxed.rs:397` | `crate::layout::slot_offset_spec(x as int)` |
| `8 * (1 + x)`: `heap/core.rs:47`, `heap/safe.rs:1127, 1304, 1340`, `heap/graph.rs:354` | `crate::layout::slot_offset_spec(x as int)` |
| `meta.rs` `from_raw`: `Set::range(hs, hs + 4)`, `(hs + 4, hs + 8)`, `(hs + 8, hs + 8 + 8 * cap)`, `into_typed::<u32>((hs + 4) as usize)` | `(hs + RC_BYTE, hs + META_BYTE)`, `(hs + META_BYTE, hs + slot_offset(0))`, `(hs + slot_offset(0), hs + slot_offset(cap as int))`, `(hs + META_BYTE) as usize` |
| `meta.rs` `into_raw`: `Set::range(hs, hs + 8 + 8 * cap)` | `Set::range(hs, hs + slot_offset(cap as int))` |
| `meta.rs` `slots_from_raw` / `slots_into_raw`: `8 * (cap - from)`, `8 * (cap - from - 1)`, `a + 8` | `WORD_BYTES * (cap - from)`, `WORD_BYTES * (cap - from - 1)`, `a + WORD_BYTES` [V] |

**Named exceptions** (left as written; none is a deferred item; owners per S1). Each is an eml_rt-owned fact that no code generator reads, or is assigned to a later stage by S1:

| Id | Lines (HEAD-relative description) | Reason |
|---|---|---|
| E1 | checked-mode prefix: `meta.rs` `prefix_first_ptr` (`- 8`), `prefix_state_ptr` (`- 4`), `from_raw`'s prefix split (`start + 4`, `start + 8`, 3 lines), `block_start` / `block_size` prefix `8int` | P1: owned by eml_rt, but it lies before the header and exists only in checked mode. Native code is fast mode only (native.md #3), so P4(b) never requires an export. Each value is defined once, in its `prefix_*_ptr` / `block_*` fn |
| E2 | `meta.rs` alignment lemma (`% 8`, `% 4` arithmetic, 6 lines) | arithmetic facts about alignment, not positions |
| E3 | allocator page layout: `alloc.rs` (24 lines: `block_words * 8`, `CLASS_WORDS[c] * 8`, `prefix_words * 8`, `FIRST_BLOCK + …`), `remote.rs:206` | P1: object-model 「ページ」 facts that only eml_rt's allocator reads. Native code allocates through eml_rt (P4b) |
| E4 | `String` / `Bytes` capacity `8 * (cap - 1)`: `text.rs` (14), `heap/string.rs` (26), `heap/inv.rs:121` | body-kind geometry (length word plus bytes). Assigned to native-inline (S1 1.2, G-9, G-10) |
| E5 | memory-budget estimates `(k + 4) * 8`: `heap/ctx/arrays.rs` `array_make`, `heap/ctx/unboxed.rs:590` | a byte estimate for `Limits::max_bytes`, not a position |
| E6 | other slot-0 or slot-1 uses that are not the Data count word: box value word (`heap/mod.rs` `dummy`, `alloc_int`, `alloc_float`, `recover.rs:812,831`, `heap/mod.rs:4430,4474`, the `boxes_value` invariants); `Raw` / `String` / `Bytes` length and content (`heap/unboxed.rs`, `heap/string.rs`, `text.rs`, `recover.rs:798`); `Foreign` vtable (`foreign.rs`); free-list next (`alloc.rs`); fixed-shape evidence lemmas (`heap/ctx/proof.rs:526, 3159`) | box word: jit-core. Body consts: native-inline (G-10). Evidence roles: jit-effects (P4c). `Foreign` payload and free list: P1, read only by eml_rt (the finalizer and the allocator go through eml_rt) |

### 5.3 eml_vm and eml_lir sites

| Site (HEAD) | Replacement |
|---|---|
| `eml_vm/src/data.rs:11-14` (`count_word`) | delete; in its place `const _: () = assert!(Shape::REUSE_SCANNED_LIMIT as usize <= INLINE_SCAN_LIMIT);` with a why-comment (cite object-model 「ペイロード」). Imports: `eml_lir::Shape`, `eml_rt::layout::{INLINE_SCAN_LIMIT, data_meta}`. `Meta` leaves the `eml_rt` import; `Kind` stays |
| `data.rs:370-372` | `let meta = data_meta(tag as u16, scanned as usize, lin);`. Comment: the shapes are below eml_lir's limit, which the assert keeps <= eml_rt's limit, so `data_meta`'s precondition holds. This precondition is not checked at run time |
| `data.rs:377` `con_at(token, (scanned + raw) as usize, meta)` | unchanged (`reuse_words` gives that word count) |
| `compile/emit.rs:1057-1060` (`field_word`) | `(eml_rt::layout::data_first(shape.scanned as usize, false) as u32 + position) \| raw` |
| `compile/emit.rs:564` | unchanged |
| `apply.rs:8` | drop `use crate::data::count_word;`; add `use eml_rt::layout::{data_first, data_raw_first};` |
| `apply.rs:234` | `let mut raw = data_raw_first(closure.scanned, false);` |
| `apply.rs:295-301` | `let first = data_first(scanned, false);` … `(first + 1, data_raw_first(scanned, false))` … `heap.set_field(w, first, Word::imm(i64::from(f) \| (k as i64) << 32))?;` |
| `compile/forks.rs:9` | `use eml_rt::layout::{INLINE_SCAN_LIMIT, data_extended, data_first, data_meta}; use eml_rt::{Control, Frozen, Heap, Mode, Word};` |
| `compile/forks.rs:79-91` (`table`) | `let n = entries.len(); assert!(!data_extended(n, false), "an effect with a fork table has fewer than {INLINE_SCAN_LIMIT} operations"); let first = data_first(n, false);` with `alloc_immortal(n, data_meta(0, n, false))` and `set_field(w, first + i, …)` |
| `compile/forks.rs:97-113` (`panic_tail`) | `const EV: usize = data_first(3, false); const NODE: usize = data_first(2, false);` with `alloc_immortal(3, data_meta(tag, 3, false))`, `set_field(ev, EV + i, w)`, `alloc_immortal(2, data_meta(tag, 2, false))`, `set_field(node, NODE + i, w)` |
| `vm.rs:11` | add `use eml_rt::layout::data_first;` |
| `vm.rs:1928-1938` | `const EV: usize = data_first(3, false); const NODE: usize = data_first(2, false);` with `set_field(ev, EV, …)`, `(ev, EV + 1, …)`, `(node, NODE, …)`, `(node, NODE + 1, …)` |
| `vm.rs:2396-2400` | `const PAIR: usize = data_first(2, false);` with `set_field(pair, PAIR, l)`, `(pair, PAIR + 1, r)` |
| `compile/constants.rs:94-95, 113-114` | unchanged (U4; jit-core) |
| `eml_lir/src/lib.rs:789-793` | in `impl Shape`: `pub const REUSE_SCANNED_LIMIT: u32 = 255;` (doc: eml_lir's reuse policy; a crate lowering reuse onto eml_rt asserts it <= eml_rt's inline-scan limit); `reuse_words` uses `Self::REUSE_SCANNED_LIMIT` |
| `eml_lir/src/lib.rs:777-779, 789-790`, `reuse.rs:482-483` (doc comments) | reword without the number; name `Shape::REUSE_SCANNED_LIMIT`; cite object-model 「ペイロード」 |
| `eml_lir/src/verify.rs:985` | `format!("{} or more scanned words", crate::Shape::REUSE_SCANNED_LIMIT)` |

### 5.4 Tests

| Where | Name | Asserts |
|---|---|---|
| `layout.rs` tests | `the_header_and_word_values_are_pinned` | `INLINE_SCAN_LIMIT == 255`; `(WORD_BYTES, HEADER_WORDS, RC_BYTE, META_BYTE) == (8, 1, 0, 4)`; `(COUNT_WORD, EXTENDED_FIRST) == (0, 1)`; `slot_byte_offset(0) == 8`; `slot_byte_offset(1) == 16` |
| same | `data_shapes_have_a_count_word_from_255_scanned_words_or_as_arrays` | for scanned in {0, 254, 255, 300} × array in {false, true}: `(data_extended, data_scan, data_first, data_raw_first)` equal the table in the reference `layout.rs` tests (`03-reference.diff`); decoding the scan byte gives `(extended, first)`; `data_words` samples `(254,2,false)=Some(256)`, `(255,2,false)=Some(258)`, `(0,0,true)=Some(1)`, `(usize::MAX,0,true)=None`; `data_meta(7, 254, true)` fields |
| same | `only_layout_spells_the_count_word_rule` | a regression guard for the two spellings `255` and `EXTENDED_SCAN` (other spellings such as `u8::MAX` are not detected). For each `trust::FILES` entry except `layout.rs` and `externs.rs`, the count in `trust::prepare(text)` is 1 for `meta.rs` and 0 otherwise |
| same | `count_word_spellings_are_found` | the count on `"a == 255\nb != 255u8\nMeta::EXTENDED_SCAN\n// 255\nc == 2550\nx0255\n"` is 3 |
| `meta.rs` tail | 4 const asserts | D-4 |
| `layout.rs` tail | 1 const assert | D-3 |
| `eml_vm/src/data.rs` | 1 const assert | D-11 |

- Existing tests: no change. `crates/eml_vm/tests/bench.rs` and `crates/eml_lir/tests/shape.rs` stay byte-identical.
- Kinds: no kind (1), no kind (2); the `trust.rs` edits are kind (3).
- Nothing is added to `trusted::`.

### 5.5 Docs (English intent; translate with PLaMo, plain form, no spaces around Latin)

Single-statement rule for G-14:
- The `scan` semantics stay stated only in 「ヘッダ」 (the L58 bullet).
- Byte offsets are stated only in 「語の位置」, as items and the `slot_byte_offset` formula.
- 「配列」 and native.md give word indices and refer to 「語の位置」.

**object-model.md `### 語の位置` (T1, items 1, 2 and 4-8; item 3 is added in T2):**
1. eml_rt owns the positions of the object words that its own code reads: the header fields, the `Kind::Data` count word and the first scanned word. A code generator that disagreed would break memory safety.
2. eml_rt states each of these once, in `eml_rt::layout`. Each is a Verus `pub open spec fn` with a `pub const fn` twin whose `ensures` is that spec, or a `pub const`. eml_rt's decoders, allocator, pointer functions and heap predicates use them.
3. (T2) The code generators (the VM's compiler and runtime) take every such value from the module and write no layout number. eml_lir's reuse limit is tied to `INLINE_SCAN_LIMIT` by a compile-time assert in each crate that lowers reuse onto eml_rt (see 「ペイロード」).
4. Items:
   - header: `WORD_BYTES`, `HEADER_WORDS`, `RC_BYTE`, `META_BYTE`, and `slot_byte_offset(i)` = `WORD_BYTES * (HEADER_WORDS + i)` (the byte offset of payload word i from the object pointer);
   - `Data`: `INLINE_SCAN_LIMIT`, `COUNT_WORD`, `EXTENDED_FIRST`;
   - encode from `(scanned, array)`: `data_extended`, `data_scan`, `data_first`, `data_raw_first`, `data_words`, `data_meta`;
   - decode from `scan`: `scan_extended`, `scan_first`.
   
   They implement the rule of 「ヘッダ」 (do not restate it): with a count word, it is at payload word `COUNT_WORD` and the scanned words start at `EXTENDED_FIRST`; raw words follow the scanned words. Indices are in words; only `slot_byte_offset` converts to bytes.
5. Decoding is meaningful only for `Data`. Array-ness cannot be read from `meta`.
6. Verus proves:
   - each exec item equals its spec;
   - decode inverts encode (`lemma_data_scan`);
   - `alloc_data`, `alloc_data_ext` and `alloc_data_rc` state their `scan` byte and raw range with these specs;
   - `alloc_array_immortal` states its `scan` byte as `data_scan_spec(0, true)`, with count 0 and no raw words;
   - the heap invariant reads the count at `COUNT_WORD`.
7. Unit tests pin every value. Const asserts tie the header items to `size_of::<Header>()`, `offset_of!` and `size_of::<Word>()`, and `Meta::EXTENDED_SCAN` to `INLINE_SCAN_LIMIT`. A unit test guards the spellings `255` and `EXTENDED_SCAN` in other eml_rt sources.
8. Not in this module: the `Meta` bit packing (`Meta`), `Word` tagging (`Word`), field order (eml_lir), closure contents (「クロージャ」), `Ctx` field offsets, the checked-mode prefix and page layout (eml_rt only), and the body layout of `String`/`Bytes`/`Raw` (「種類」).

**object-model.md, other places:**
- (T1) 「ヘッダ」: replace the `size_of::<Header>()` bullet with "The header size and field offsets are the consts of 「語の位置」, tied by const asserts to `size_of::<Header>()` and `offset_of!`". The `scan` bullet ends with "positions: 「語の位置」".
- (T1) L404 (`field` row): replace "VM は… 添字を直す必要がない" with "the VM computes physical indices at code generation from the shape and `data_first` (「語の位置」), so it never adjusts them at run time".
- (T1) L972: replace "書くときは位置を1つずらす" with "writes at `scan_first(scan) + op` (「語の位置」)".
- Unchanged, because they state contracts in terms of `scan` semantics: L346, L358, L388, L393-L395, L83 (the kind table).
- (T2) 「ペイロード」:
  - The bullet "規則を出す関数は `eml_lir` に置く… 結び付けのテストは要らない" becomes:
    - eml_lir owns field order (`LayoutCtor::shape`), and eml_rt receives only tag, scanned count and raw count.
    - Positions are logical; the physical index is `layout::data_first(scanned, false) + position`.
    - eml_lir's reuse pairs only shapes with fewer than `Shape::REUSE_SCANNED_LIMIT` scanned words.
    - Each crate lowering reuse onto eml_rt asserts `REUSE_SCANNED_LIMIT <= layout::INLINE_SCAN_LIMIT` at compile time. That assert is the one tie, and the limit may be lowered on its own.
  - L73: the positions do not count the count word (「語の位置」).
- (T2) 「クロージャ」 L159, reason sentence:
  - A closure is a `Data` with `scanned = 1 + reference captures`, positioned by 「語の位置」.
  - The function word is at `data_first(scanned, false)`, and raw capture j at `data_raw_first(scanned, false) + j`.
  - The VM reads the function and the reference captures with `scanned(w, j)`.
- (T2) 「配列」:
  - L534: replace the `base + 8` / `base + 16 + 8*i` sentence with "the count is at payload word `COUNT_WORD` and element i at `EXTENDED_FIRST + i`, so native code reads both at fixed offsets (`slot_byte_offset`, 「語の位置」) without reading the length first".
  - L554: "Like arrays, word 0 holds n and the elements start at word 1".
  - L570 (`unboxed_ptr`): replace "`base + 16` (`slot_ptr(h, 1)`)" with "`slot_ptr(h, 1)`, right after the count word". The element-0 offset of `Raw` is native-inline's body const (E6).

**vm.md (T2):**
- L193: "Shape positions are logical; for `field`, `set_field`, `raw` and `set_raw` the VM adds `eml_rt::layout::data_first(scanned, false)` at code generation ([オブジェクトモデル] 「語の位置」)".
- L194: positions come from `data_first` and `data_raw_first` of `1 + reference captures`.
- L214: the meta is `eml_rt::layout::data_meta(tag, scanned, lin)`.
- L215:
  - The shapes have fewer than `Shape::REUSE_SCANNED_LIMIT` scanned words.
  - `eml_vm`'s compile-time assert keeps that limit at or below `layout::INLINE_SCAN_LIMIT`, so there is no count word and `data_meta`'s precondition holds.
  - The precondition is not checked at run time; the guards are the verifier, the debug assert in code generation, and the compile-time assert.

**lir.md L679 (T2):** "Constructors with `Shape::REUSE_SCANNED_LIMIT` (255) or more scanned words are neither a source nor a `con` partner. eml_rt may give them a count word, and the limit is eml_lir's policy ([オブジェクトモデル] 「ペイロード」)".

**testing.md (T1):**
- In 「Verus」, item 7 gains: "object positions: each `eml_rt::layout` item equals its spec, decode inverts encode, and the allocator's postconditions and heap predicates are written with them".
- In 「単体テスト」, a new bullet: the layout pins, and the guard on `255`/`EXTENDED_SCAN` in other eml_rt sources, which reuses the `trust.rs` scanner (`externs.rs` is excluded; its 255s are `Byte` ranges).

**crates/eml_rt/CLAUDE.md (T1):** add the rule "Object positions (header offsets, the `Data` count word and first scanned word) are written only in `src/layout.rs` and used through it (docs/spec/object-model.md の「語の位置」)".

**native.md (T3):**
- L98, last sentence: "eml_rt exports the offsets as `offset_of!`-derived `pub const`s next to `Ctx`, outside `eml_rt::layout`; native code uses only those".
- L120: replace "`base + 8 + 8*i` の位置" with "the element position of [オブジェクトモデル] 「配列」".

**roadmap.md 「段の列」 (T3), appended to each bullet:**
- `jit-core`:
  - "Export from eml_rt what emitted code reads, each on its one surface:
    - the `Meta` bit positions and masks as `pub const`s on `Meta`;
    - the `Word` tagging consts on `Word`;
    - the rc encoding consts in `rc`;
    - the BigInt/Float box value word in `eml_rt::layout`, rerouting eml_rt's box sites (`dummy`, `alloc_int`, `alloc_float`, their readers and recovery, the `boxes_value` invariants) and `eml_vm`'s `compile/constants.rs` box writes;
    - the `Ctx` field offsets as `offset_of!`-derived `pub const`s next to `Ctx`, not in `eml_rt::layout`."
  - "`eml_native` gains its `eml_rt` and `eml_lir` edges (update the CI step `dependency edges of eml_native`) and `const _: () = assert!(Shape::REUSE_SCANNED_LIMIT as usize <= layout::INLINE_SCAN_LIMIT)`."
  - "Emitted code uses no integer literal as a load/store offset or mask of object memory."
- `jit-effects`: "Move the evidence field roles (`evv` node `[evidence, next]`, evidence `[entry table, state, outer evv]`) onto the `Ctx` surface (constructors or role consts, as this stage's effects design decides). In the same stage, remove the role numbers in `eml_vm`'s `vm.rs` and `compile/forks.rs` and in eml_rt's evidence proof lemmas."
- `native-inline`: "Add to `eml_rt::layout` two kind-keyed word-index consts for the `Raw`/`String`/`Bytes` bodies (length word 0, content first word 1), leaving the `Data` items unchanged. Reroute eml_rt's body sites (`slot_ptr(h, 1)`, `1 + i`, the length-word reads, `8 * (cap - 1)`) through them. `Array` length and element reads use `COUNT_WORD` and `EXTENDED_FIRST`."
- `native-aot`: "Add a link-time check that emitted objects and `libeml_rt.a` come from the same eml_rt native ABI (object layout, `Ctx` offsets, entry symbols, extern word ABI). This stage chooses how the id is derived from eml_rt's consts."
- At Close: delete the `rt-layout` bullet, and remove `rt-layout` from `jit-core`'s 前提.

### 5.6 Task list (one commit each)

**G (common gate).** Run from the repo root after `git add -A`:
1. `cargo verus verify -p eml_rt > "$TMPDIR/verus.log" 2>&1; echo $?` prints `0`, and `grep -v 'verification results::' "$TMPDIR/verus.log" | grep -cE 'error|warning|note:'` prints `0`. When it does verify, the log shows `verification results:: N verified, 0 errors`.
2. `verusfmt --verus-only --check $(git ls-files 'crates/eml_rt/src/*.rs')` exits 0.
3. `cargo fmt --check` exits 0.
4. `cargo clippy --all-targets -- -D warnings` exits 0.
5. `cargo nextest run` ends with `0 failed` (summary shows all tests passed).

**T0 Plan.**
- Files: `docs/plans/YYYY-MM-DD-rt-layout.md`. It holds D-1..D-15, the tables of 5.2-5.4, the exceptions E1-E6, the test kinds (none of 1 or 2; `trust.rs` is kind 3), and the bench script of T4.
- Verify: `git diff --name-only HEAD~1 HEAD` prints exactly `docs/plans/YYYY-MM-DD-rt-layout.md`. Nothing checks the plan's content mechanically (the citation test skips `docs/plans`).

**T1 eml_rt.** Linked: G-1, G-2, G-3, G-4 (part), G-6, G-11; D-1..D-8, D-12, D-14.
- Order (TDD):
  1. Write `layout.rs` with its items and the 4 tests, plus the `lib.rs` and `trust.rs` rows. Run `cargo nextest run -p eml_rt layout::`. Expected: 3 pass, and `only_layout_spells_the_count_word_rule` fails, listing the files.
  2. Do the reroute tables of 5.2. The test then passes.
  3. Write the docs (T1 parts of 5.5).
- Files:
  - `crates/eml_rt/src/{layout.rs, lib.rs, trust.rs, meta.rs, alloc.rs, frozen.rs, remote.rs}`;
  - `crates/eml_rt/src/heap/{mod.rs, proof.rs, core.rs, graph.rs, ctx.rs, inv.rs, reuse.rs, states.rs, safe.rs, field.rs, recover.rs, unboxed.rs}`;
  - `crates/eml_rt/src/heap/ctx/{proof.rs, arrays.rs}`;
  - `docs/spec/object-model.md`, `docs/implementation/testing.md`, `crates/eml_rt/CLAUDE.md`.
  - `Meta::EXTENDED_SCAN` stays `pub` in this task.
- Verify (all from the repo root):
  - G.
  - `cargo nextest run -p eml_rt layout:: trust::` reports `0 failed`, with 4 `layout::` tests among those run.
  - G-1: `grep -rnE '\b255(u8|usize)?\b|EXTENDED_SCAN' crates/eml_rt/src | grep -v '/tests/' | grep -vE ':[0-9]+:\s*//' | grep -v 'src/layout.rs\|src/externs.rs'` prints exactly one line, `crates/eml_rt/src/meta.rs:…: pub const EXTENDED_SCAN: u8 = crate::layout::INLINE_SCAN_LIMIT as u8;`.
  - G-2:
    - `grep -c 'crate::layout::slot_offset_spec(i)' crates/eml_rt/src/meta.rs` prints `1`.
    - `grep -cE 'crate::layout::scan_(extended|first)_spec\(bits_scan\(p\.meta\.value\(\)\)\)' crates/eml_rt/src/heap/inv.rs` prints `2`.
    - `grep -c '^pub const fn' crates/eml_rt/src/layout.rs` and `awk '/^pub const fn/{f=1} f&&/^    ensures/{n++;f=0} END{print n}' crates/eml_rt/src/layout.rs` both print `9`.
  - G-3:
    - `grep -c 'crate::layout::data_scan_spec(scanned as nat' crates/eml_rt/src/heap/mod.rs` prints `4`.
    - `grep -c 'crate::layout::data_first_spec(scanned as nat' crates/eml_rt/src/heap/mod.rs` prints `7`.
    - `grep -c 'crate::layout::data_scan_spec(0, true)' crates/eml_rt/src/heap/mod.rs` prints `1`.
  - C-1:
    - `grep -rnE 'p_extended\(.*p_init\([^,]+, 0\)' crates/eml_rt/src` prints nothing.
    - `grep -nE 'slot_get\(h, 0\)|slot_ptr\(h, 0\)|tracked_borrow\(0\)' crates/eml_rt/src/heap/core.rs crates/eml_rt/src/heap/graph.rs crates/eml_rt/src/remote.rs` prints nothing.
    - `grep -nE '1 \+ [ij]\b|idx \+ 1|last \+ 1|word_at\([a-z]+, 0\)' crates/eml_rt/src/heap/ctx/arrays.rs` prints exactly one line, `idx + 1,` (`fill`'s logical start).
    - `grep -nE 'i != 0' crates/eml_rt/src/heap/field.rs crates/eml_rt/src/heap/safe.rs` prints exactly one line, in `safe.rs` (the `boxes_value` block).
    - `grep -c 'f.first == 1' crates/eml_rt/src/heap/recover.rs` prints `0`.
  - C-2:
    - `grep -rnE '8 \* \((HEADER_WORDS|1) \+|hs \+ [48]\b' crates/eml_rt/src | grep -v /tests/` prints nothing.
    - `grep -rnE '\b8 \*|\* 8\b|\+ 8\b|- 8\b|\b8 \+|\+ 4\b|- 4\b' crates/eml_rt/src | grep -v '/tests/' | grep -vE ':[0-9]+:\s*//' | grep -v 'src/externs.rs\|src/trust.rs\|src/layout.rs' | cut -d: -f1 | sort | uniq -c` prints exactly:
      - 24 `alloc.rs`
      - 1 `heap/ctx/arrays.rs`
      - 1 `heap/ctx/unboxed.rs`
      - 1 `heap/inv.rs`
      - 26 `heap/string.rs`
      - 11 `meta.rs`
      - 1 `remote.rs`
      - 14 `text.rs`
      
      These are E1-E5 [V on scratch].
  - Miri: `nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_rt trusted::` passes, and passes again with `-Zmiri-strict-provenance -Zmiri-tree-borrows`.
- Left to the implementer: Japanese comments and the exact section wording.

**T2 eml_lir, eml_vm, the tie.** Linked: G-4, G-5, G-6, G-7, G-8, G-16; D-3 (visibility), D-9..D-11.
- Files:
  - `crates/eml_lir/src/{lib.rs, verify.rs, reuse.rs}`;
  - `crates/eml_vm/src/{data.rs, apply.rs, vm.rs, compile/emit.rs, compile/forks.rs}`;
  - `crates/eml_rt/src/meta.rs` (`pub(crate)`);
  - `docs/spec/object-model.md` (「語の位置」 item 3, 「ペイロード」, 「クロージャ」, 「配列」), `docs/spec/vm.md`, `docs/spec/lir.md`.
- Verify:
  - G.
  - `cargo clippy -p eml_cli --no-default-features --features core -- -D warnings` exits 0, and the same with `--features run` exits 0.
  - G-5:
    - `grep -rn 'count_word' crates/eml_vm/src` prints nothing.
    - `grep -rnE '\b255\b' crates/eml_vm/src crates/eml_core_ir/src crates/eml_native/src crates/eml_cabi/src | grep -vE ':[0-9]+:\s*//'` prints nothing.
    - `grep -rnE '\b255\b' crates/eml_lir/src | grep -vE ':[0-9]+:\s*//'` prints exactly the `pub const REUSE_SCANNED_LIMIT: u32 = 255;` line.
    - `grep -rnE 'Meta::new\([^;]*Kind::Data' crates/eml_vm/src` prints nothing.
    - `grep -rnE 'heap\.(set_field|field|raw|set_raw)\([^,()]+,\s*[0-9]' crates/eml_vm/src` prints exactly 2 lines, `compile/constants.rs:95` and `:114`.
  - `grep -rn 'EXTENDED_SCAN' crates/*/src | grep -v '^crates/eml_rt/'` prints nothing.
  - G-6: `cargo tree -p eml_lir -e normal --depth 1` lists only `eml_extern` and `rustc-hash`; `cargo tree -p eml_rt -e normal --depth 1` lists only `vstd`.
  - G-7: `grep -rn 'REUSE_SCANNED_LIMIT' crates/eml_vm/src | grep -vE ':[0-9]+:\s*//'` prints exactly 1 line (the assert).
  - G-8: `git diff --stat <T0> -- crates/eml_lir/tests/shape.rs crates/eml_vm/tests/bench.rs` prints nothing.
  - G-16: `git diff <T0> -- crates/eml_vm/src/apply.rs | grep -c '<< 32'` prints `2` (the `set_field` line removed and re-added with only the index changed).

**T3 native and roadmap docs.** Linked: G-9, G-10, G-12, G-13, G-14.
- Files: `docs/future/native.md`, `docs/future/roadmap.md`.
- Verify:
  - G-14: `grep -rnE 'base \+ (8|16)\b' docs | grep -v '^docs/reports/' | grep -v '^docs/plans/'` prints nothing. Before the stage it lists object-model.md:534, 554, 570 and native.md:120. The broader pattern `base \+ [0-9]` also matches unrelated frame and stack text (vm.md:103, native.md:27/95/145).
  - `cargo nextest run -p eml_cli --test integration citations::` reports `0 failed`.

**T4 Bench gate, then `Close rt-layout` (macOS).** Linked: G-15, D-13.
1. Gate. From the repo root, with `B` = the T0 commit and `A` = HEAD (T3):
   ```sh
   OUT="$TMPDIR/rt-layout-bench"; mkdir -p "$OUT"
   git worktree add --detach ../eml-rt-layout-bench "$B"
   for i in 1 2 3 4 5; do
     git -C ../eml-rt-layout-bench checkout -q --detach "$B" && ../eml-rt-layout-bench/bench/rt.sh > "$OUT/base-$i.md"
     git -C ../eml-rt-layout-bench checkout -q --detach "$A" && ../eml-rt-layout-bench/bench/rt.sh > "$OUT/after-$i.md"
   done
   python3 -I "$OUT/rt-compare.py" "$OUT" 5 | tee "$OUT/gate.md"
   git worktree remove ../eml-rt-layout-bench
   ```
   `$OUT/rt-compare.py` holds the script below, copied from the plan. Expected: the last line of `$OUT/gate.md` is `GATE PASS`. Put the table into the Close commit message. If it prints `GATE FAIL`, stop and report the table before going on (G-15).
   ```python
   import re, statistics, sys
   d = sys.argv[1]; n = int(sys.argv[2])
   EXCLUDED = re.compile(r'^(empty|println-.*|read-.*|stdout-.*)$')
   def rows(f):
       out = {}
       for l in open(f):
           m = re.match(r'\| `([^`]+)` \| (\d+) \|', l)
           if m: out[m.group(1)] = int(m.group(2))
       return out
   pairs = [(rows(f'{d}/base-{i}.md'), rows(f'{d}/after-{i}.md')) for i in range(1, n + 1)]
   fail = False
   print('| row | median delta % | gated |'); print('|---|---:|---|')
   for k in pairs[0][0]:
       m = statistics.median(100 * (a[k] - b[k]) / b[k] for b, a in pairs)
       gated = not EXCLUDED.match(k); bad = gated and m > 0.1; fail |= bad
       print(f'| `{k}` | {m:+.3f} | {"yes" if gated else "no"}{" FAIL" if bad else ""} |')
   print('GATE', 'FAIL' if fail else 'PASS')
   ```
2. Spec: no stage names or "later" notices in the touched sections.
3. On a clean tree at A, run `bench/scripts.sh`, then `bench/run.sh`, `bench/rt.sh` and `bench/scripts.sh`. Replace the three tables in `docs/implementation/benchmarks.md` 「今の数値」 with that output.
4. `status.md`, `architecture.md`, `overview.md` and the README map: unchanged.
5. roadmap: delete the `rt-layout` bullet and remove `rt-layout` from `jit-core`'s 前提.
6. Root `CLAUDE.md`: unchanged.
7. Delete the plan file.
8. Commit message must include:
   - test-change kinds (none of 1 or 2; `trust.rs` is kind 3);
   - the gate table;
   - the benchmark differences;
   - the exceptions E1-E6 with their owning stages.
   
   No item goes to 「その後の項目」: E1-E3 and E5 are not deferred work, and E4/E6 sit in later stages' text per S1. So there are no deferral dates.

Verify:
- `tail -1 "$TMPDIR/rt-layout-bench/gate.md"` prints `GATE PASS`.
- G.
- `cargo nextest run -p eml_cli --test integration citations::` reports `0 failed`.
- `ls docs/plans | grep -c rt-layout` prints `0`.
- `grep -c 'rt-layout' docs/future/roadmap.md` prints `0`.

## 6. Risks and fallbacks

- **Verus.** Everything verifies [V]. If a later Verus fails on `lemma_data_perm`, add `crate::layout::lemma_data_scan(scanned as nat, force)` as its first proof step. If it fails on an arrays proof, assert `crate::layout::EXTENDED_FIRST == 1` at the top of that proof.
- **Verus visibility.** A `pub` const or `pub open spec fn` that refers to `EXTENDED_SCAN` fails [V]. Use `INLINE_SCAN_LIMIT`.
- **Verus cache.** A rerun with nothing changed prints no results line (A4). G uses the exit code.
- **Bench gate fails.**
  1. Rerun with 5 more pairs (`n = 10`).
  2. If it still fails, compare `scan_get` with the S1 form that branches on `first == 1` (S1 measured -0.47%..+0.04%).
  3. Report before acceptance (G-15).
- **New file and `git ls-files`.** The verusfmt check skips `layout.rs` until it is `git add`ed.
- **verusfmt.** It cannot parse `unsafe {…}.bits()` as a tail expression [V]; bind it to a `let`.
- **G-5 `count_word` grep.** It matches any eml_vm name containing `count_word` [V].
- **G-1 / scan test.** A second `EXTENDED_SCAN` line in `meta.rs` (for example a const assert) fails both [V].
- **`frozen.rs` imports.** They need `#[cfg(verus_only)]` [V].
- **Run-time preconditions.** `data_meta` and `data_raw_first` are unchecked in eml_vm (D-9). A wrong LIR pairing in a release build would write a truncated `scan`. The guards are the const assert, the LIR verifier, `emit.rs:564` (debug) and the `forks.rs` `assert!`.

## 7. Unresolved items (with defaults)

- U-a: the heading text. Default: `語の位置` (code cites it).
- U-b: `alloc_array_immortal` states only the `scan` byte equality, with count 0 and no raw words. Its first index and raw range are therefore vacuous. Default: no further ensures.

## 8. Additional checks

All on a scratch copy of the base with `03-reference.diff` applied, final state:
- [V] Verus: `1220 verified, 0 errors`, no warnings or notes (forced re-verification by editing `layout.rs`). verusfmt, fmt and clippy `--all-targets -D warnings` are clean. nextest: 4878 passed.
- [V] Every grep in T1 and T2 gives the stated output, including the per-file counts for C-2.
- [V] `bench/rt.sh`, 5 interleaved pairs (script: the one in T4): `GATE PASS`.
  - Gated medians range from -0.033% to +0.067%.
  - Ungated: `empty` +0.066%, `println-line` -0.231%, `read-all-bytes-std` -0.345%.
  - These ran from two different paths. T4 uses one path.
- [V] Rerouting the count word (D-6) and the header geometry (C-2) needed no new proof reasoning. One revert was needed: the `boxes_value` block in `safe.rs` is the box word, not the count word.
- Not checked:
  - Miri;
  - the `--no-default-features` clippy combos;
  - the citations test with the new docs;
  - `bench/run.sh` and `bench/scripts.sh`;
  - the T4 single-path script as written (it uses `git`, which the scratch copy lacks).

---

