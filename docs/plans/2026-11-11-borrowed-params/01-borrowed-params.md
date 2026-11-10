# Appendix 01: borrowed parameters (design, reviewed and revised)

English design deliverable for the plan `docs/plans/2026-11-11-borrowed-params.md`. Where it disagrees with the plan's section 5, the plan wins. Tags: [V] verified, [I] inferred. Base f918f5fb.


Base: /Users/arakaki/Projects/eml at f918f5fb. Tags: [V] verified (how), [I] inferred.
Terms:
- *block param*: a variable in `Block.params` (entry block params are the function params).
- *root(v)*: v itself if v is a block param; root(parent) if v is a field bound by a `switch` case, `unpack`, or struct `unbox`; undefined otherwise.
- *entry-borrowed(v)*: root(v) is defined and is marked borrowed. Its reference is held by a frame below the current one.
- *pinned function*: `split.is_some() || clause.is_some() || name ends with "$uniform" || name ends with "$owned" || eml_lir::ep_helper(name) || name is a RuntimeEntry::ALL name`.
- *copy*: a `<f>$owned` function. *original*: any other function present after contract (including `$uniform` variants).
- *tail graph*: one edge f -> g per `Term::TailCall(Call::Direct(g, _))` in f, over the originals after contract. *tail-SCC*: SCC of that graph. *open(g)*: some function reachable from g over tail edges (g included) has a `tail apply` or `tail clause`.

## 1. Conclusion

1. **Representation.** `eml_lir::Block` gains `borrowed: Vec<VarId>` (a subsequence of `params`, empty by default). The function signature is the entry block's `borrowed`. Calls, `jump`s and `VarInfo` carry nothing. There is exactly one lender, the function entry (the caller), so no owner field exists. This is final for the stage: merge-block params can be borrowed (rule B2 produces them from source programs), a block param lent by another param of the same block cannot. Text form: `borrowed` before the binder in `fn` parameter lists and block labels: `fn skip(borrowed s.0: obj, i.1: int) -> int {`, `b4(borrowed u.7: obj):`.
2. **ABI meaning.** A borrowed parameter is a plain word without a reference. The caller keeps the object alive and unmodified until the call returns. The callee may read it and lend it on; it must `dup` before any consuming use and may never `decref`, `release`, tokenise, or update it in place. Only `unr` RC (`obj`/`tobj`) params can be borrowed. Pinned functions have none.
3. **Name rule.** A function's marks are the least fixpoint of B0-B4 (5.2) over the code reachable from it by direct calls. Its code additionally depends on the marks of functions it references as values (rule A1). Both are fixed by names. Inference never reads callers.
4. **Adaptation.** `<f>$owned` is a copy of f (post-contract body) with every mark cleared. Value references to a function with a borrowed param go to `<f>$owned`. A tail call that would lend a value its own frame keeps alive is demoted in place to `let r = call g(..)` / `return r` when `!open(g)`, else retargeted to `tail call g$owned(..)`. This rule (A2) applies to every function. In an original it never fires on an edge inside a tail-SCC, because B4 (Lean `ownParamsUsingArgs`) made the fed param owned; in a copy it fires on those edges too.
5. **Pass order.** Boxing, Flatten, Contract, **Borrow** (`eml_lir::borrow`, `Pass::Borrow`), Perceus, Reuse. Perceus does not edit terminators.
6. **Perceus/verifier** are correct for any marking that passes the static rules; inference is policy. New verifier rule: per instruction, apply consumptions first, then require each lent argument to be valid.
7. **Extern rows: none change in this design's units.** (S3's FFI lending work is separate and goes through the `eml_rt` gate on its own.)
8. **VM: no op changes.** The entry unwind row lists owned params only, from the existing verifier walk.
9. **Measurement.** `bench/words_loop.em`: today `rc_increments: 60000, rc_decrements: 60016` [V]; after the stage `0` and `16` [I, derivation in 5.6]. No bench `rc_increments` rises and no `reuses` changes; both are scripted checks.

## 2. Rationale

- **Marks on the block.** `VarInfo` is shared with Core IR [V: `grep -rln VarInfo crates/*/src`]; `eml_lir::Block` is built in 4 places [V: grep]. Ownership on call sites would duplicate the callee's signature.
- **"Borrowed iff only read" (B1 with roots).** A consuming use of a param or of a field unpacked from it makes the param owned. So inferred code never `dup`s an entry-borrowed value: borrowing removes caller `dup`s and callee `decref`s and adds none. This is Lean's rule.
- **Closures and continuations.** Closure captures are consuming positions [V: lir.md:63]. Everything reached through `apply`, `clause` or the runtime is pinned: functions born as values have `split` [V: lir.md:214], other value-referenced functions are reached through `$uniform` or `$owned`. So no heap object holds a borrowed word, by mechanism (G-5), independent of inference.
- **Reuse (B3).** `release`/`decref` of an entry-borrowed variable fails the verifier (count 0), so the Beans hazard is unrepresentable. B3 only preserves today's reuse where B1 is silent (`map` over an `Int` list).
- **Why demotion needs `open(g)`.** `lambda -> tail g(s, k)` with `g` ending in `tail apply k()` is a constant-stack tail cycle today; demoting the edge into `g` would add a frame per round. Whether an `apply` closes a cycle is not decidable from names.
- **Frame bound.** For a demoted edge c -> g: `!open(g)`, so everything tail-reachable from g is an original reached by direct tail calls, with no retarget (retargets only go to open callees) and no unknown callee. c is not in that set: if c is an original it is in another tail-SCC (else B4 applied); if c is a copy, originals never tail-call a copy except by retarget. Each demotion along a chain strictly shrinks the tail-reachable set, so no cycle of tail-position calls contains a demoted call and extra frames per chain are bounded by the number of functions.
- **Copies on a tail-SCC.** In `f$owned`, f's params are owned, so an in-SCC edge `tail h(p)` now lends a frame-owned value. A2 handles it: `!open(h)`: demote, one frame, then h runs the original cycle; `open(h)`: `tail h$owned(p)`, and the copies form their own constant-stack cycle.
- **Demotion and yields.** A demoted call has no yield check, like the thin `$uniform` call [V: lir.md:227, :426]. After it come only `decref`/`drop` and the `return`. Dummies are inert under `dup`/`decref`/`drop` [V: lir.md:421-423]; for `let (r1..rn) = call g` the leaves are g's dummy leaves returned unchanged, no box is allocated (unlike thin `$uniform`, nothing is boxed).
- **Extern rows stay** [I, classification from row names in R1 section 7, not audited against `Ctx` methods]. Three classes: (a) retains the argument: closures and cells of `Std.Async.*`, `Std.Chan.push_raw`, buffers of `Std.Net`/`Std.Stream` `submit_*`, `Std.Gui.*_raw`, `fail_io`'s message; (b) ends a handle: `close*`, `Proc.wait`, `Tls.push_eof`/`send_close_notify`; (c) reads a value that is typically a temporary at its last use: `println`, `eprintln`, `FileIn.open`, `FileOut.create`/`append`, `Env.has`/`lookup`, `spawn_raw`. (a) and (b) must consume. For (c) a borrow saves an RC pair only when the caller keeps the value, next to a syscall, and costs an `eml_rt` method change per row. Consequence, accepted: by B1 a param that is printed or opened is owned, so the stage benefits pure readers only.
- **Unchanged VM.** `call`, `enter_in_place` and `TailSelf` copy words and check nothing about ownership in either mode [V: vm.rs:1145-1153, 1549-1559, 431-437; the `CHECKED` sites in vm.rs are counters and a result-count check].

## 3. Alternatives considered and rejected

| Alternative | Why rejected |
|---|---|
| Ownership per argument on `Call`/`Jump` | Two sources of truth |
| Flag in `VarInfo` | Shared with Core IR |
| Usage decides (value-used functions get owned params) | ABI depends on the rest of the program |
| Demote every tail call off a direct-call cycle | Grows the stack on tail cycles through `apply`/`clause` |
| Never demote; always `tail g$owned` | Copies every reader that gets a fresh value in tail position |
| `open(g)` implies all-owned params | Loses borrowing for any function ending in a callback |
| Thin `$owned` wrapper only | Leaves a frame under tail cycles through `apply` |
| In-place demotion inside Perceus | Borrow rewrites the terminator first; Perceus stays an RC inserter |
| Borrow the parent when only a field is consumed | No "never adds a dup" guarantee |
| Block param lent by a same-block param, with an owner field | Releasing the owner needs one (ctor, slot) per field, which differs per edge, so Perceus could only `dup`; no producer. Dropped from the stage |
| No borrowed merge-block params at all | `if c then s else t` would force both params owned; B2 is one rule and one per-edge check |
| Pin every `internal` function | Lambdas with value references already have `split` and are pinned; the rest are called directly and should borrow |
| Borrow some of the 86 Effectful rows | See Rationale |

## 4. Assumptions made

- A2 [I]: `Atom::Fn`, tags and unit dummies in RC positions are immediates and need no owner.
- A3 [I]: every function the runtime calls other than through `apply`/`clause` is the program entry (no params [V: lib.rs:41-42]) or a `RuntimeEntry` name [V: eml_extern/src/lib.rs:2362-2388].
- A4: the baea5793 LIR is unknown; the "per-byte dup" shape is the call-per-iteration shape.
- (r1's A1 and A5 are now verified, see section 7.)

## 5. Implications for implementation

### 5.1 IR, text, classification

- `Block { params, borrowed, stmts, term }`; `LirFn::lends() -> Vec<bool>` per param position.
- `for_each_consumed` / `for_each_lent` (lib.rs:263-475) stay the single definition of read vs consume and get the context they need (callee masks per `FnIdx`, target-block masks, `c_functions`): a direct-call argument at a borrowed position and a `jump` argument into a borrowed block param are lent. Exact signature is the implementer's.
- `borrowed` is accepted only by the LIR reader; Core IR text rejects it.

### 5.2 Borrow pass (`borrow.rs`)

Marks: start with every block param borrowed; apply until stable (owned only grows; the result is unique).

- **B0** Owned if the Repr is not RC, the mark is not `unr`, or it is an entry param of a pinned function.
- **B1** For every consuming use of v (current marks decide which call/jump positions consume): root(v), if defined, is owned.
- **B2** For a `jump` into block param q with an RC argument variable a that is not entry-borrowed: q is owned.
- **B3** If v is taken apart as a reuse source and a `con` with the same word count occurs in a block reachable from there, root(v) is owned. "Taken apart": scrutinee of a tag `switch` case, value of an `unpack`, or box of a struct `unbox`, whose constructor satisfies `reuse::source_words(shape).is_some()`; "same word count": `reuse::source_words` of both constructors equal; "reachable": the case target (for `unpack`/`unbox`: the rest of the block) and every block reachable from it. `source_words` is the one predicate shared with the reuse pass (unit 5). B3 deliberately ignores liveness and `may_yield`: it must be a superset of what reuse pairs.
- **B4** For a tail call f -> g with g in f's tail-SCC and an RC argument variable at position i that is not entry-borrowed: param i of g is owned.

Schedule: SCCs of the direct-call graph callee-first, iterating inside an SCC; iterative code only.

Adaptation, after the fixpoint, on every original and every copy:

- **A1** `Atom::Fn(f)` anywhere and `MakeClosure(f, _)`: if f has a borrowed param, use `<f>$owned`.
- **A2** `tail call g(args)` where some borrowed position of g holds an RC variable that is not entry-borrowed: if `!open(g)`, replace by `let r = call g(args)` + `return r` (multi-value: `let (r1..rn)` + `return (r1..rn)`; new vars named `r` with g's `rets`); else retarget to `<g>$owned`. `debug_assert` that in an original the edge is not inside a tail-SCC.
- **A3** `<f>$owned` = clone of f taken before A1/A2, every `borrowed` list cleared (entry and merge blocks), renamed; `yields`, `internal`, marks, `Loc`, `rets` copied; `open(f$owned) = open(f)`; then A1/A2. Created on demand, appended after the originals. Debug builds rebuild each copy from its source and compare (as `variants::check`).
- **`$uniform` variants** are originals with pinned entry params; A1/A2 apply to their bodies like any function. Thin form: `call f` lends to f's borrowed positions and Perceus places the `decref` after the call (it was never a tail call). Copy form: a tail call with an owned argument at a borrowed position follows A2. `variants::check` runs inside Boxing, before Borrow, and is unaffected. `needs_copy` and `open` are independent predicates; neither is derived from the other.

### 5.3 Perceus

- P1 Entry and merge blocks start owning live RC vars and RC params except entry-borrowed ones. The owner table gets an `Entry` owner; fields of an entry-borrowed parent get it too. Entry-borrowed vars never enter `owned` or `State.lent`.
- P2 A consuming use of an entry-borrowed var emits one `dup` before the instruction per consuming occurrence; no state change.
- P3 Lent call/jump arguments are reads; an owned var whose last use is a lend is `decref`ed right after.
- P4 A lent borrowed var whose owner the same instruction consumes for the last time is `dup`ed before and `decref`ed after.
- P5 `tail`/`jump` lending a non-entry-borrowed RC var: `debug_assert`.
- With no marks, output is unchanged: P1-P3, P5 are no-ops without marks, and the P4 shape does not occur in any current pipeline output or test [V: probe, section 7].

### 5.4 Verifier

New level `Borrowed` (`verify_borrowed`) after Borrow = flattened checks + static rules; earlier levels reject marks. Static rules (also at the ownership level):

- S1 `borrowed` is a subsequence of `params`; each is RC and `unr`.
- S2 A pinned function has no borrowed param; no value reference targets a function with one.
- S3 Per edge: a `jump` argument into a borrowed block param is an entry-borrowed variable or a non-variable atom.
- S4 A tail call's argument at a borrowed position is an entry-borrowed variable or a non-variable atom.

Ownership rules:

- O1 A borrowed block param starts with count 0 and is entry-backed; a field bound from an entry-backed value with count 0 is entry-backed. Entry-backed variables are always valid to read.
- O2 Consuming, `decref`, `release` need count >= 1.
- O3 For an instruction with lent arguments (extern, prim, direct call): apply all consumptions left to right, then each lent variable must be valid (count >= 1, or owner count >= 1, or entry-backed). Replaces `read_borrowed` + `lent_apart_from_consumed`.
- O4 Merge rule and "nothing owned at `return`/`tail`" unchanged. Entry unwind row = owned params.

New messages (existing ones keep their wording):
- `` `s.0` is borrowed before the borrow pass ``
- `` `n.1` (int) is a borrowed parameter, but only obj and tobj can be `` / `` `h.0` (lin) is a borrowed parameter, but only unr can be ``
- `` `s.0` of `main$lambda3` is borrowed, but the function is not called directly ``
- `` `f` is used as a value, but its parameter `s.0` is borrowed ``
- `` `x.3` is passed to the borrowed parameter `u.7` of b4, but it is not borrowed from the caller ``
- `` `x.3` is lent to `g` by a tail call, but it is not borrowed from the caller ``
- `` `s.0` is {used|released} but is only borrowed from the caller ``
- O3 failure: existing `` `x.1` is lent to `Std.Array.get` and consumed by the same instruction `` when the variable is also consumed there, else existing `` `x.1` is lent to `g` after its owner `d.0` was given up ``.

### 5.5 VM, REPL, modes

No new op, no operand change. Entry unwind row = owned params; a demoted call is a new unwind point. REPL: the debug `Mismatch` comparison (vm.md 「追記」) checks the name rule on every transcript.

### 5.6 Bench program

`bench/words_loop.em` :

```
-- 3 万語の文字列の語を数える。語ごとに、呼んだ側が持ち続ける文字列を、展開されない再帰の関数 2 つへ渡す。
-- 呼ばれる側が文字列を読むだけなら、渡すたびの RC が `rc_increments` に出ない。
import Byte
import String

grow : Int -> String -> <Panic> String
grow n acc = if n == 0 then acc else grow (n - 1) (acc ++ " ab")

skip_spaces : String -> Int -> <Panic> Int
skip_spaces s i =
  if i >= String.byte_length s then i
  else if String.byte_at i s == ' 'u8 then skip_spaces s (i + 1) else i

skip_word : String -> Int -> <Panic> Int
skip_word s i =
  if i >= String.byte_length s then i
  else if String.byte_at i s == ' 'u8 then i else skip_word s (i + 1)

count : String -> Int -> Int -> <Panic> Int
count s i acc =
  if i >= String.byte_length s then acc
  else count s (skip_word s (skip_spaces s i)) (acc + 1)

main : Unit -> <IO, Panic> Unit
main () =
  let s = grow 30000 ""
  println (show (count s 0 0))
```

Output `30000\n`. Today [V]: `rc_increments=60000 rc_decrements=60016 allocations=17 peak_objects=3 peak_stack=21 string_bytes_copied=90005`.

Derivation of the expected values [I]: `count` runs its else-branch 30000 times; each round has `dup s.0` before `call skip_spaces` and before `call skip_word` (60000 increments) and each callee exit has `decref s.0` (60000 decrements) [V: dump]. The other 16 decrements are outside the loop: 15 from building and printing (the same program prefix as `bench/byte_loop.em`, which pins `rc_decrements: 16` with `allocations: 17` and a single reader exit [V: bench.rs:140-145]) and 1 final release of `s` (today at `count`'s exit). After the stage the three readers have `borrowed s.0` and no RC instruction, and the final release moves to `entry$main` (`decref t.2` after `call count`). So `rc_increments: 0`, `rc_decrements: 16`, all other fields unchanged. If `rc_increments` is not 0, a reader still owns `s`: dump the LIR and look at the marks. If it is 0 and `rc_decrements` is not 16, the release of `s` is misplaced; checked mode reports the leak or the bad release.

### 5.7 Units (commit order; each leaves the workspace green)

Gate for every unit: `cargo nextest run` all pass; `cargo clippy --all-targets -- -D warnings` clean; `cargo fmt --check` clean. Each unit touches no file under `crates/eml_rt` or `crates/eml_extern`: `git show --stat HEAD -- crates/eml_rt crates/eml_extern` prints no file.

| # | Unit | Files | Spec in the same commit | Unit-specific check |
|---|---|---|---|---|
| 1 | Baseline bench | `bench/words_loop.em`; `eml_vm/tests/bench.rs`: `PROGRAMS` 41 -> 42 (after `upsert_by_hand`), test `words_loop`, new `unwind_points` line | benchmarks.md row description | `cargo nextest run -p eml_vm --test integration -E 'test(bench::words_loop)'` passes with `rc_increments: 60000`, `rc_decrements: 60016` |
| 2 | Marks in IR and text | `eml_lir/src/{lib,syntax,text,pretty}.rs`, `Block` literals, `eml_core_ir/src/lowering.rs`; earlier levels reject marks | lir.md 「ブロックの列」; testing.md 「LIR のテキストの形」 | new `text::` round-trip and Core-IR-rejects tests; `git show HEAD | grep -c '^-.*@"'` is 0 (no existing inline snapshot edited) |
| 3 | Classification, verifier ownership, unwind | `lib.rs`, `verify.rs`, `unwind.rs` | lir.md 「verifier」 (S1-S4, O1-O4, messages), 「extern の借用」 (O3), 「巻き戻しの地点」; vm.md 「巻き戻しの表」 entry row | one new `verify::` test per message in 5.4 plus "lent field, consumed owner"; `unwind::` test: entry row omits a borrowed param; no existing test changes (O3 is silent on all current code [V]) |
| 4 | Perceus with marks | `perceus.rs` | lir.md 「Perceus」 (P1-P5) | new hand-written tests: reader has no RC; a consumed borrowed param is `dup`ed once per use; field of a borrowed param; borrowed merge param; P4; a lend of an owned var is followed by `decref`. `git show HEAD -- crates/eml_lir/tests/perceus.rs | grep -c '^-'` is 1 (only the `---` header: additions only) |
| 5 | Shared reuse predicate | `reuse.rs`: `pub(crate) fn source_words(&Shape) -> Option<usize>` used by the pass for sources and `con`s | none (no rule changes) | kind 3: `git show --stat HEAD` lists only `crates/eml_lir/src/reuse.rs`; all tests pass unchanged |
| 6 | Borrow pass, not in the pipeline | `eml_lir/src/borrow.rs`, `pub fn borrow`; `eml_lir/tests/borrow.rs` (declared in `tests/main.rs`) | lir.md new 「借用の推論」 (B0-B4, A1-A3, pinned, `open`, `$uniform` bodies), 「関数の ABI」 (`$owned`, borrowed ABI) | tests on hand-written LIR, snapshot of `pretty` after `borrow` and `verify_borrowed` passing: one per B0-B4; A1; A2 demote; A2 retarget; `$owned` of a member of a 2-function tail-SCC with `!open` (copy demotes into the original) and with `open` (copy retargets, copies form a cycle); thin and copy-form `$uniform` over a borrowing callee; multi-value demotion; two programs sharing `f`, one calling it, one also referencing `&f` and a caller `h`: `f` prints identically, `h` differs only in `&f$owned`; a 2000-function call chain gets the expected marks (no time bound). Plus `#[ignore = "timing: borrow on a 20000-function chain, release only"]` listed in testing.md 「よく使うコマンド」 |
| 7 | Switch on | `pipeline.rs` (`Pass::Borrow`, `verify_borrowed`); every kind-2 expected value (5.8), regenerated with `cargo insta test --accept` and reviewed | lir.md 「パスの順番」 (6 passes; delete "借用パラメータの最適化はしない"), 「末尾の位置の呼び出し」 (demotion exception and bound), lir.md:64 and :608; core-ir.md 「変換の規則」 projection note; vm.md 「呼び出しと戻り」; native.md 「呼び出し規約とエントリ」; object-model.md:336; status.md:62 | C1-C4 below |
| 8 | Source-level LIR tests | `eml_core_ir/tests/{perceus,c_functions}.rs` | none | new tests pass (list below) |
| 9 | VM tests | `eml_vm/tests/scaling.rs`, `eml_vm/tests/bench.rs` (`owned_variants`) | benchmarks.md 「回数のテスト」 | new tests pass (list below) |
| 10 | UI and REPL tests | `tests/ui/run/...`, `tests/ui/run-fail/...`, `crates/eml_cli/tests/repl/` | none | `cargo nextest run -p eml_cli --test integration -E 'test(ui::) | test(repl::)'` passes; Miri: `nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_vm --test integration miri::` passes, again with `-Zmiri-tree-borrows` |
| 11 | Close (shared with the stage) | benchmarks.md three tables; roadmap.md: remove the stage, and state in 「その後の項目」 "同じブロックの引数から借りるブロック引数" and "薄い形の `$owned`" with reason and revisit condition; delete `docs/plans/` | — | `grep -rn "借用パラメータの最適化はしない" docs/` prints nothing |

The plan document must state, from the first commit, the two refinements of the roadmap bullets: one lender (no owner declaration), and `$owned` as a copy instead of a wrapper.

Unit 7 checks (BASE = the unit-6 commit):
- C1 `cargo nextest run -p eml_vm --test integration -E 'test(bench::words_loop)'` passes with `rc_increments: 0`, `rc_decrements: 16`.
- C2 `paste <(git show BASE:crates/eml_vm/tests/bench.rs | grep -o 'rc_increments: [0-9]*' | cut -d' ' -f2) <(grep -o 'rc_increments: [0-9]*' crates/eml_vm/tests/bench.rs | cut -d' ' -f2) | awk '$2 > $1 {bad++; print} END {print NR " rows"; exit bad != 0}'` prints exactly `45 rows`, exit 0. (Today the same command against HEAD prints `44 rows` [V]; unit 1 adds one.)
- C3 the same command with `reuses` and `$2 != $1` prints exactly `45 rows`, exit 0.
- C4 the feature-combo clippy lines of CLAUDE.md are clean.

Unit 8 tests: readers of the `words_loop` shape have `borrowed` and no RC; `f p.name` then a use of `p`: no `dup` when `f` reads, one when `f` consumes; `let u = if c then s else t` with only reads of `u`: `s`, `t` and the merge param are `borrowed`; a param live across a yield point is owned and moved into the continuation closure; `map` over an `Int` list keeps `reuse`; `c_functions::a_blocking_call_argument_is_owned_by_the_waiting_continuation` (G-6): a function `wrap s a = <blocking C call>(s, a)` with a `String` and a `MutUnboxedArray` param, at `Pass::Reuse`: neither param of `wrap` is `borrowed`; the `closure wrap$kN(..)` after `call Std.Async.io_wait` lists both; `wrap$kN` has no `borrowed` param; no `dup` of the array variable appears.

Unit 9 tests: (a) a tail cycle `lambda -> g -> tail apply` passing a fresh String into a reader each round: `peak_stack` equal at 1 000 and 100 000 rounds; (b) `words_loop` shape: `rc_increments` equal at two sizes; (c) `bench::owned_variants`: inline snapshot of the names of `$owned` functions per bench program and per script (`wc`, `grep`, `logstat`, `csv2jsonl`, `deploy`), next to `unwind_points`. The list is the measure of copy growth; put its total in the commit message.

Unit 10 tests: a reader passed as a function value and partially applied; a reader under a handler whose operation resumes twice, the borrowed value read after the resume; a function returning a record of two floats, called in tail position with a fresh String it only reads (demoted multi-value call), whose callee performs a resumable operation; `run-fail`: a panic inside a reader holding a borrowed argument (checked mode proves the unwind rows); REPL: define a reader, then pass it as a value and tail-call it with a fresh value in a later input.

Left to the implementer: Rust signatures, worklist layout, Japanese wording (PLaMo), where the SCC helper lives.

### 5.8 Test changes by kind

- **Kind 1: none.**
- **Kind 2, all in unit 7**, reason "borrowed params move or remove RC instructions, add `$owned` functions and demoted calls":
  - snapshots under `crates/eml_core_ir/tests/` that print LIR at `Pass::Perceus` or `Pass::Reuse`;
  - tests in `crates/eml_lir/tests/` that go through `passes_until(.., Pass::Perceus | Pass::Reuse)` (not those calling `perceus()`/`reuse()` directly [V: perceus.rs:10-21]);
  - `eml_vm/tests/bench.rs` RunStats snapshots and `unwind_points`;
  - `eml_vm/tests/codegen.rs` disassembly of programs compiled from source;
  - RunStats counts asserted from source programs elsewhere in `eml_vm/tests`.
- **Kind 3:** `borrowed: Vec::new()` in `Block` literals; call sites of the changed `for_each_consumed`/`for_each_lent`; `PROGRAMS` length; unit 5.
- Everything else is new tests.

## 6. Unresolved items (with defaults)

- U2 Thin `$owned`. Default: copy only. Revisit if `owned_variants` (unit 9) lists more than a handful per program or `bench/run.sh` load time moves.
- U3 native.md/benchmarks.md cite 3.7-4.0x for a shape the current `wc` LIR lacks [V per R1]. Default: keep the numbers, reword the shape as "a callee that owns the string and is called per iteration", point at `words_loop`.
- U4 B3 over-approximates. Default: keep; tighten on a measured loss.
- U5 Peak memory: a borrowed value is freed by the caller after the call; a local owner now spans calls that only lend its fields. Default: accept; a `peak_objects` rise in bench.rs is a kind-2 change with this reason.

## 7. Additional checks

- `words_loop.em` built and run with `--stats`; LIR dumped after Perceus [V].
- **O3/P4 on existing code.** Added the missing half of O3 (a lent borrowed field whose owner the same extern/prim consumes with no spare count) to `verify.rs` in the scratch clone and ran `cargo nextest run --no-fail-fast`: 4794 passed, 21 skipped, 0 hits of the probe message [V:  The debug pipeline runs this verifier after Perceus and Reuse for every UI, bench and core_ir test, so no current Perceus output and no hand-written test has the shape. Units 3 and 4 are green in that order and change no existing expected value.
- **Checked mode.** `Op::Call`, `Op::TailCall`, `Op::TailSelf` copy argument words and check nothing about ownership; the `CHECKED` branches in vm.rs count boxes, unwound frames and a result count [V: read vm.rs:1145-1198, 1285-1315, 1549-1559, 1625, 1717 and `enter_in_place`].
- The comparison command of C2 runs and prints `44 rows` on the current tree [V].
- Existing linear-time tests use a wall-clock bound [V: eml_lir/tests/perceus.rs:1046-1093]; the new one is `#[ignore]` instead.
- Read: `perceus.rs`, `variants.rs`, `pipeline.rs`, `unwind.rs` in full; parts of `verify.rs`, `lib.rs`, `syntax.rs`, lir.md, vm.md (「追記」, 「巻き戻し」, 「`blocking` の呼び出し」) [V].
- Not done: a prototype of the pass.

## 8. Conformance criteria

**G-1 (representation)** Borrowedness MUST be stored only as `Block.borrowed`; `Call`, `Term::Jump`, `Atom`, `VarInfo` MUST NOT carry ownership, and there MUST be no owner field: the lender is always the function entry.
Why: one source of truth. Applies to: `eml_lir` types. Check: review of `lib.rs`; text tests.

**G-2 (eligibility)** A borrowed block param MUST be RC and `unr`.
Why: consuming it needs `dup`; a `lin` value has exactly one reference. Applies to: B0, S1. Check: verify tests.

**G-3 (pinned)** Pinned functions MUST have no borrowed param; no `Atom::Fn`/`MakeClosure` may target a function with one.
Why: `apply`, clause entries and the runtime pass owned words. Applies to: A1, S2. Check: verify tests; UI test.

**G-4 (ABI)** For a borrowed param the caller MUST keep the object alive and unmodified until the call returns, including through the callee's tail calls; the callee MUST NOT `decref`, `release`, tokenise or update it in place, and MUST `dup` before a consuming use.
Why: native code may hoist content pointer and length for the activation. Applies to: Perceus, VM, jit-core. Check: O1-O3; checked-mode UI runs.

**G-5 (where borrows may exist)** Borrowed positions MUST exist only at direct-call params, block params and extern/prim rows. `con`, closure captures, `apply`/`clause`/`par` arguments and callee, `return`, `box`, `drop` and non-borrowed row arguments MUST consume.
Why: a borrowed word must not outlive the frames that own it. Applies to: `for_each_consumed`/`for_each_lent`. Check: unit-3 tests.

**G-6 (suspension; S3's R1 is checked against this)**
(a) A continuation closure MUST hold a counted reference of its own for every RC capture: a capture is a consuming position (G-5) and continuation functions are pinned (G-3). This holds for any marking, not only inferred ones.
(b) Under inferred marks no entry-borrowed value is ever captured: a captured param, or the param a captured field was unpacked from, is owned by B1, and the reference moves (or is `dup`ed from an owned value) into the closure. Under any other valid marking a captured entry-borrowed value is `dup`ed (P2). Either way the closure's reference is independent of every frame.
(c) Therefore, for each object argument of a `blocking` call: the lowering keeps it an operand of `take` (vm.md 「`blocking` の呼び出し」), so it is live across `call Std.Async.io_wait` and is a capture of the caller's continuation. From `FfiSubmit` until `FfiTake` returns, the continuation stored by `await_io` reaches, through counted references only, a closure holding one counted reference to the object; after resume the reference is an owned param of the continuation function until after `take`. No borrowed reference stands in for it at any point after the capture. Between `FfiSubmit` and the capture nothing runs but the yield path of the same task, and the value is valid in the frame.
(d) Wrapper case: if the argument is a param of the enclosing function (or a field of one), that param is owned by B1 because of the capture; the function's callers pass a counted reference. The enclosing function is never a borrower of it.
(e) `MutUnboxedArray` is `lin`: it is never borrowed (G-2) and never `dup`ed (V1), so the closure's reference is the only one.
(f) This design MUST NOT add, remove or move yield points, change the `submit`/`io_wait`/`take` lowering, or change that C rows borrow `obj` arguments.
Not guaranteed here: what happens when the continuation is dropped (cancellation, unwind, quiesce); that is S3's R2.
Why: R1 of S3. Applies to: Borrow, Perceus; EP and translate unchanged. Check: `c_functions::a_blocking_call_argument_is_owned_by_the_waiting_continuation` (unit 8); the commits of units 1-10 change no file under `crates/eml_core_ir/src/evidence` or `crates/eml_core_ir/src/translate`.

**G-7 (name rule)** A function's marks MUST depend only on code reachable from it by direct calls. Its code MAY additionally depend on the marks of functions it references as values or calls. Inference MUST NOT read callers, value-reference sites of the function itself, or which copies exist.
Why: "same name, same code". Applies to: `borrow.rs`. Check: unit-6 two-program test (direct and value-reference cases); copy rebuild assert; REPL transcript.

**G-8 (inference result)** Marks MUST be the least fixpoint of B0-B4.
Why: borrowing must never add a `dup`. Applies to: `borrow.rs`. Check: one test per rule; C2.

**G-9 (reuse)** A value taken apart where a same-word-count `con` is reachable MUST have an owned root; Borrow and Reuse MUST use the same `source_words`.
Why: a borrowed box is never a reuse source; two predicates would drift. Applies to: B3, unit 5. Check: C3; core_ir `map` test.

**G-10 (tail calls)** After Borrow every `tail call` argument at a borrowed position MUST be entry-borrowed or a non-variable atom. A2 MUST demote only when `!open(g)` and otherwise retarget to `g$owned`. In an original, a tail call inside a tail-SCC MUST NOT need A2 (B4 guarantees it); in a copy it follows A2 like any other edge.
Why: the caller's frame is gone at a tail call; tail cycles, also through `apply`, must not grow the stack. Applies to: B4, A2, S4. Check: S4 verify test; unit-6 SCC copy tests; scaling test (a).

**G-11 (lend after consume)** The verifier MUST apply consumptions first and then require each lent variable to be valid. Perceus MUST `dup` a lent variable whose owner the same instruction consumes for the last time.
Why: the callee may free a consumed owner before reading the lent field. Applies to: O3, P4. Check: verify and perceus tests.

**G-12 (per edge)** Every `jump` argument into a borrowed block param MUST be entry-borrowed or a non-variable atom.
Why: a merge block cannot tell its incoming edge. Applies to: B2, S3. Check: verify test; unit-8 merge test.

**G-13 (mechanism vs policy)** Perceus and the verifier MUST be correct for any marking satisfying S1-S4.
Why: inference will be tuned. Applies to: `perceus.rs`, `verify.rs`. Check: unit-4 tests with a consumed borrowed param.

**G-14 (rows; scope: the borrow work only)** The units of this design MUST NOT change a `FunctionRow`/`PrimRow` `borrowed` mask or any file under `crates/eml_rt`. FFI lending changes belong to S3 and its `eml_rt` gate. Rule for later rows: a row borrows an argument only if it only reads it and call sites typically keep the value; rows that retain the argument, end a handle, or typically receive a temporary consume.
Why: no measurable gain; gate cost. Applies to: units 1-10. Check: the per-unit `git show --stat HEAD -- crates/eml_rt crates/eml_extern` prints no file.

**G-15 (VM)** The units of this design MUST NOT add or change VM ops. The entry unwind row MUST list exactly the owned RC params.
Why: ownership is explicit in LIR. Applies to: `eml_vm`. Check: `unwind::` test; review of unit-7 diff under `crates/eml_vm/src` (expected: none).

**G-16 (completion)** C1, C2 and C3 of unit 7 MUST hold.
Why: the user's completion condition. Applies to: unit 7.

---

