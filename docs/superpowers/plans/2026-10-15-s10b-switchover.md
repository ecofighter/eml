# S10b VM への切り替え Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `eml run` を VM だけで動かし、CEK (`eml_interp`)、`eml_runtime`、LIR の `saved` と R7 を消し、SIGINT のハンドラと既定の上限を入れ、S10a の残りの整理を終える。

**Architecture:** CEK と比べる UI テストの仕組みを使える間に VM を整理し (Task 1-5)、最後の比較を記録してから (Task 6)、CEK のテストを VM へ移し (Task 7)、VM の API を固め (Task 8)、期待値の表の書き手を `eml_rt` に移し (Task 9)、切り替えと2つの crate の削除を1つの段で行う (Task 10)。その後に上限と SIGINT (Task 11)、`saved` と R7 と Perceus の 2' (Task 12)、`run.sh` (Task 13)、文書 (Task 14) を進める。

**Tech Stack:** Rust (edition 2024、stable 1.98.1)、`insta`、`ctrlc`、`libc`、Verus (`eml_rt`)、Miri (`.#miri` の nightly)、macOS の `/usr/bin/time -l` (`bench/run.sh`)。

**Spec:** `docs/superpowers/specs/2026-10-15-s10b-switchover-design.md`

## Global Constraints

- 各タスクの終わりに `cargo test` (ワークスペース全体)、`cargo clippy --all-targets` (警告 0)、`cargo fmt --check` を通す。`crates/eml_vm/src` を変えたタスクは、Miri の2つの設定 (`-Zmiri-strict-provenance`、`-Zmiri-strict-provenance -Zmiri-tree-borrows`) で `cargo miri test -p eml_vm --test integration miri::` を通す。`crates/eml_rt` を変えたタスクは、`cargo verus verify -p eml_rt` と `verusfmt --verus-only --check $(git ls-files 'crates/eml_rt/src/*.rs')` を通す
- コードのコメントと `docs/` は日本語で書き、`yomiyasu:yomiyasu` の規則に従う。コメントは理由を書き、規則は `docs/…/x.md の「見出し」` の形で引く。`CLAUDE.md` は英語である
- 引用の検査 (`cargo test -p eml_cli --test integration citations`) は、各タスクの終わりに通る
- テストの変更は spec の「テストの変更」の3つの種類のどれかで、コミットの本文に種類を書く。spec にない pass/fail の変更をしない
- 上限の既定の値: `max_depth` = 2^27 語、`max_bytes` = 物理メモリの 80% (`sysconf(_SC_PHYS_PAGES) * sysconf(_SC_PAGESIZE)`、読めなければ `usize::MAX`)、`max_evv` = `usize::MAX`
- `Task::reserve` の容量: `max(要る長さ, min(2 × 今の容量, max_depth + u16::MAX + 2))` 語 (`reserve_exact`)。余った引数の適用は安全点の前に2つの枠を積みうるので、要る長さより小さくはしない (spec の「1つの枠」の見積もりを Task 5 で直す)
- 割り込みで止まったときの終了コードは 130。2回目の Ctrl-C は `std::process::exit(130)`。ほかの実行時エラーは 1、使い方の誤りは 2
- `RunStats::peak_stack` は実行の終わりの `task.stack.len()` である
- 新しい依存は `ctrlc` と `libc` だけで、どちらも `eml_cli` の `run` の feature の依存にする
- コミットの末尾に次の2行を付ける:
  `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`
  `Claude-Session: https://claude.ai/code/session_01MK52fNp5uA711Q2D4kWEtR`
- `git diff` は `--no-ext-diff` を付ける (difftastic が外部の diff に設定されている)
- Task 10 から Task 13 までの間、`bench/run.sh` は消えた `--vm` を渡すので動かない。計測は Task 2、Task 6 (切り替えの前) と Task 13 (切り替えた後) だけで行う

## Review Focus

spec が求めるが、どのタスクのテストも直接は確かめない入力のうち、利用者に効きやすいものを5つ挙げる。それぞれのテストを、持ち主のタスクに足してある。

1. コンパイルの間に Ctrl-C を押したとき: ビットは実行の前に立つので、実行の最初の安全点で止まり、終了コード 130 になるはずである。Task 8 に「割り込み済みの `Control` で実行すると、最初の安全点で `interrupted` になり、`RuntimeError::interrupted` が真になる」テストを足す
2. `Fs.read_all` で大きなファイルを読んでいる間の Ctrl-C: extern の中の `Stopped(Interrupted)` も 130 になるはずである。Task 8 の `interrupted` のテストで、extern の道 (`VmFault::Extern(Fault::Stopped(Stop::Interrupted))`) も真になることを確かめる
3. 深さの上限に当たる深い再帰: abort でなく `the call depth limit is exceeded` で止まり、スタックの容量が上限を大きく超えないはずである。深さの上限で止まった実行は `RunStats` を返さず、`peak_stack` は容量でなく長さなので、上限は `RunStats` からは見えない。そのため Task 5 は、`Task::reserve` の容量の上限を `src/` の単体テストで確かめる (`the_stack_limit_is_the_depth_limit_and_one_frame`、`reserve_doubles_the_capacity_up_to_the_stack_limit`、`reserve_beyond_the_stack_limit_takes_exactly_the_length`)
4. 1つの `Control` を2回の実行に使い回したとき: 前の実行で立ったビットが残ると、次の実行がすぐ止まる。Task 8 の `RunConfig` のテストで「`clone` した設定は `Control` を共有する」ことを確かめ、UI テストの仕組みと `eml_test_support` が実行ごとに新しい `RunConfig` を作ることを Task 10 のコードで守る
5. 速いモードの `eml run` (利用者の既定): ほかの CLI のテストはどれも `--debug-heap` を付ける。Task 10 で `--vm` を外した2本が、既定の速いモードの成功と実行時エラーの終了コード 1 を確かめる

---

### Task 1: VM execution cleanups (spec 「S10a の残りの整理」 items 1-3)

Behaviour-preserving refactor of the VM loop. Every code fragment below was compiled and run against the current tree in a scratch clone: `cargo test` (workspace), `cargo clippy --all-targets` (0 warnings) and Miri (strict provenance) all passed.

**Files:**
- Modify: `crates/eml_vm/src/vm.rs`. Item 1: add `Task::load_scratch` and `Task::store_scratch`, and have `enter_in_place` (lines 83-98) use `store_scratch`. Item 2: split the `Apply | Clause` and `TailApply | TailClause` arms (lines 206-218) and add `apply_call`. Item 3: replace `step` (lines 149-315) with one exhaustive `match` that holds the primitive arms, and rewrite `load_args` (lines 317-328). Drop `use crate::prims;` (line 14)
- Modify: `crates/eml_vm/src/apply.rs` (`apply_rest`, lines 162-169: the scratch loop becomes `load_scratch`)
- Modify: `crates/eml_vm/src/lib.rs` (line 18: delete `mod prims;`)
- Delete: `crates/eml_vm/src/prims.rs`
- Test: no new tests. These existing tests pin the behaviour:
  - the UI tests: `crates/eml_cli/tests/ui.rs`, `run` and `run_fail`. Each runs the CEK, then the VM in checked mode (output, error text and the six counts), then the VM in fast mode
  - every `eml_vm` integration test, in particular `calls.rs` (apply, clause, over- and under-application, `ApplyRest`, self tail calls), `evidence.rs` (all 19 primitives) and `bench.rs` (the `RunStats` snapshots)
  - Miri `miri::` with both settings

**Interfaces:**
- Consumes: nothing new.
- Produces (crate-private, used by Tasks 2 and 5): `Task::load_scratch(&mut self, slots: impl IntoIterator<Item = Slot>)`, `Task::store_scratch(&mut self)`, `fn slots(list: &[u32]) -> impl Iterator<Item = Slot>` and `fn apply_call<M: Mode>(code: &CodeBase<M>, ctx: &mut Ctx<M>, task: &mut Task, at: u32, target: apply::Target, clause: bool) -> Result<(), VmFault>`, all in `vm.rs`. `prims::step` no longer exists. The primitive arms are now inside `vm::step` (`#[inline(always)]`).

- [ ] **Step 1: Run the pinning tests before the change**

```bash
cargo test -p eml_vm
cargo test -p eml_cli --test integration ui::
```

Expected: everything passes (eml_vm: 5 unit tests and 89 integration tests; `ui::`: 9 passed).

- [ ] **Step 2: Add `load_scratch` and `store_scratch`, and use them in `enter_in_place`**

In `crates/eml_vm/src/vm.rs`, replace `enter_in_place` (lines 83-98) with:

```rust
    /// `slots` の語を scratch に読む。全部読んでから書くので、書き先が読んだスロットと重なってもよい。
    pub(crate) fn load_scratch(&mut self, slots: impl IntoIterator<Item = Slot>) {
        self.scratch.clear();
        for slot in slots {
            let w = self.get(slot);
            self.scratch.push(w);
        }
    }

    /// scratch の語を今の base のスロット 0.. に書き、scratch を空にする。
    pub(crate) fn store_scratch(&mut self) {
        let Task {
            stack,
            scratch,
            base,
            ..
        } = self;
        for (i, w) in scratch.drain(..).enumerate() {
            stack[*base + i] = w;
        }
    }

    /// scratch の引数を今の base のスロット 0.. に書き、`f` の先頭へ進む。見出しはそのまま使う。
    pub(crate) fn enter_in_place<M: Mode>(&mut self, code: &CodeBase<M>, f: u32) {
        let entry = &code.functions[f as usize];
        self.reserve(self.base + usize::from(entry.frame_size));
        self.store_scratch();
        self.frame_size = usize::from(entry.frame_size);
        self.pc = entry.start;
    }
```

- [ ] **Step 3: Replace `step` and `load_args` with one exhaustive match**

In `crates/eml_vm/src/vm.rs`, delete line 14 (`use crate::prims;`). Then replace everything from the doc comment `/// 命令を1つ実行する。` (line 149) through the end of `load_args` (line 328) with the code below. The primitive arms are copied from `prims.rs` with the `unsafe` narrowed to each call. `yielding`, `yield_status` and `yield_cont_take` are safe methods of `Ctx`, so they get no `unsafe`. The comment on argument order moves from `prims::step` to the doc comment of `step`.

```rust
/// 命令を1つ実行する。`Finish` に着いたら `false` を返す。
///
/// 結果のスロットは引数のスロットと同じ色でありうるので (docs/spec/vm.md の「スロットの割り当て」)、どの腕も引数を
/// 読み終えてから結果を書く。
#[inline(always)]
fn step<M: Mode>(
    code: &CodeBase<M>,
    ctx: &mut Ctx<M>,
    task: &mut Task,
    counts: &mut RunCounts,
    pc: Pc,
) -> Result<bool, VmFault> {
    task.pc = pc + 1;
    let ops = |at: u32| &code.operands[at as usize..];
    match code.code[pc as usize] {
        Op::Move { dst, src } => task.set(dst, task.get(src)),
        Op::LoadRaw { dst, value } => task.set_raw(dst, value.into()),
        Op::LoadImm { dst, value } => task.set(dst, Word::imm(value.into())),
        Op::LoadConst { dst, index } => task.set(dst, code.constants[index as usize]),
        Op::Goto { target } => task.pc = target,
        Op::Ret { src } => {
            let value = task.get(src);
            task.last_ret = pc;
            task.return_value(value);
            ctx.poll(task.depth())?;
        }
        Op::Call { dst, at } => {
            call(code, task, dst, at);
            ctx.poll(task.depth())?;
        }
        Op::TailCall { at } => {
            let o = ops(at);
            task.load_scratch(slots(&o[2..2 + o[1] as usize]));
            task.enter_in_place(code, o[0]);
            ctx.poll(task.depth())?;
        }
        Op::Finish => {
            finish(code, ctx, task)?;
            return Ok(false);
        }
        Op::ApplyRest => {
            apply::apply_rest(code, ctx, task)?;
            ctx.poll(task.depth())?;
        }
        Op::Closure { dst, at } => {
            let o = ops(at);
            let (f, lin, n) = (o[0], o[1] != 0, o[2] as usize);
            task.load_scratch(slots(&o[3..3 + n]));
            let w = apply::make_closure(ctx, f, lin, &task.scratch)?;
            task.scratch.clear();
            task.set(dst, w);
        }
        Op::Apply { dst, at } => {
            let target = apply::Target::Frame { ret: task.pc, dst };
            apply_call(code, ctx, task, at, target, false)?;
        }
        Op::Clause { dst, at } => {
            let target = apply::Target::Frame { ret: task.pc, dst };
            apply_call(code, ctx, task, at, target, true)?;
        }
        Op::TailApply { at } => apply_call(code, ctx, task, at, apply::Target::Tail, false)?,
        Op::TailClause { at } => apply_call(code, ctx, task, at, apply::Target::Tail, true)?,
        Op::TailSelf { at } => {
            let o = ops(at);
            task.load_scratch(slots(&o[2..2 + o[1] as usize]));
            task.store_scratch();
            task.pc = o[0];
            // バイトコードに後ろ向きの辺ができるので、ここも安全点にする (docs/spec/vm.md の「呼び出しと戻り」)
            ctx.poll(task.depth())?;
        }
        Op::IntAdd { dst, a, b } => task.set_raw(dst, externs::int_add(task.raw(a), task.raw(b))?),
        Op::IntSub { dst, a, b } => task.set_raw(dst, externs::int_sub(task.raw(a), task.raw(b))?),
        Op::IntMul { dst, a, b } => task.set_raw(dst, externs::int_mul(task.raw(a), task.raw(b))?),
        Op::IntDiv { dst, a, b } => task.set_raw(dst, externs::int_div(task.raw(a), task.raw(b))?),
        Op::IntRem { dst, a, b } => task.set_raw(dst, externs::int_rem(task.raw(a), task.raw(b))?),
        Op::IntNeg { dst, a } => task.set_raw(dst, externs::int_negate(task.raw(a))?),
        Op::IntEq { dst, a, b } => task.set_raw(dst, externs::int_eq(task.raw(a), task.raw(b))),
        Op::IntNe { dst, a, b } => task.set_raw(dst, externs::int_ne(task.raw(a), task.raw(b))),
        Op::IntLt { dst, a, b } => task.set_raw(dst, externs::int_lt(task.raw(a), task.raw(b))),
        Op::IntLe { dst, a, b } => task.set_raw(dst, externs::int_le(task.raw(a), task.raw(b))),
        Op::IntGt { dst, a, b } => task.set_raw(dst, externs::int_gt(task.raw(a), task.raw(b))),
        Op::IntGe { dst, a, b } => task.set_raw(dst, externs::int_ge(task.raw(a), task.raw(b))),
        Op::IntCompare { dst, a, b } => {
            task.set_raw(dst, externs::int_compare(task.raw(a), task.raw(b)))
        }
        Op::BoolEq { dst, a, b } => task.set_raw(dst, externs::bool_eq(task.raw(a), task.raw(b))),
        Op::BoolNe { dst, a, b } => task.set_raw(dst, externs::bool_ne(task.raw(a), task.raw(b))),
        Op::SwitchEnum { src, at } => {
            data::switch_tag(code, ctx, task, task.get(src), task.raw(src), at)?
        }
        Op::SwitchObj { src, at } => {
            let w = task.get(src);
            // scrutinee の Repr は `obj` なので、語はポインタである (docs/spec/vm.md の「switch」)
            let tag = unsafe { ctx.heap().meta(w) }?.tag();
            data::switch_tag(code, ctx, task, w, tag.into(), at)?
        }
        Op::SwitchTObj { src, at } => {
            let w = task.get(src);
            let tag = if w.is_imm() {
                w.imm_value()
            } else {
                // `is_imm()` の枝を先に取ったので、ここでは語はポインタである (docs/spec/vm.md の「switch」)
                unsafe { ctx.heap().meta(w) }?.tag().into()
            };
            data::switch_tag(code, ctx, task, w, tag, at)?
        }
        Op::SwitchInt { src, at } => data::switch_int(code, task, task.raw(src), at)?,
        Op::SwitchString { src, at } => data::switch_string(code, ctx, task, task.get(src), at)?,
        Op::Unpack { src, at } => data::unpack(code, ctx, task, src, at)?,
        // RC の命令は Repr が `obj` か `tobj` の変数にだけ出す。VM はその参照を持つか借りている
        Op::Dup { src } => unsafe { ctx.heap_mut().dup(task.get(src)) }?,
        Op::Decref { src } => unsafe { ctx.heap_mut().decref(task.get(src)) }?,
        Op::Release { src, at } => data::release(code, ctx, task, src, at)?,
        Op::Con { dst, at } => data::con(code, ctx, task, dst, at)?,
        Op::BoxInt { dst, src } => {
            if M::CHECKED {
                counts.boxes += 1;
            }
            let w = unsafe { ctx.heap_mut() }.alloc_int(task.raw(src));
            task.set(dst, w);
        }
        Op::BoxEnum { dst, src } => {
            if M::CHECKED {
                counts.boxes += 1;
            }
            task.set(dst, Word::imm(task.raw(src)));
        }
        Op::BoxConst { dst, index } => {
            if M::CHECKED {
                counts.boxes += 1;
            }
            task.set(dst, code.constants[index as usize]);
        }
        Op::Unbox { dst, src } => {
            let n = data::unbox(ctx, task.get(src), counts)?;
            task.set_raw(dst, n);
        }
        Op::Extern { dst, at } => extern_call(code, ctx, task, dst, at)?,
        // 基本操作 (docs/spec/lir.md の「基本操作」) は `Ctx` の同じ名前のメソッドを呼ぶ。引数の語は、LIR の所有の規則に
        // より VM が持つ参照か借りた語で、基本操作の契約 (docs/spec/object-model.md の「基本操作の契約」) のとおりに
        // 渡す。借用の引数は `Ctx` が手放さない。VM は基本操作の合間に `Ctx` の持つ語に触れない
        Op::EvvGet { dst } => {
            let w = unsafe { ctx.evv_get() }?;
            task.set(dst, w);
        }
        Op::EvvSet { dst, l } => {
            unsafe { ctx.evv_set(task.get(l)) }?;
            task.set(dst, Word::UNIT);
        }
        Op::EvvPush { dst, l, e } => {
            let w = unsafe { ctx.evv_push(task.get(l), task.get(e)) }?;
            task.set(dst, w);
        }
        Op::EvvMask { dst, at } => {
            let o = ops(at);
            let list = &code.effect_lists[o[0] as usize..(o[0] + o[1]) as usize];
            let w = unsafe { ctx.evv_mask(list, task.get(o[2] as Slot)) }?;
            task.set(dst, w);
        }
        Op::EvNew { dst, at } => {
            let o = ops(at);
            let (state, outer) = (task.get(o[2] as Slot), task.get(o[3] as Slot));
            let w = unsafe { ctx.ev_new(o[0] as u16, o[1] as usize, state, outer) }?;
            task.set(dst, w);
        }
        Op::EvSetClause { dst, at } => {
            let o = ops(at);
            let (ev, entry) = (task.get(o[0] as Slot), task.get(o[2] as Slot));
            unsafe { ctx.ev_set_clause(ev, o[1] as usize, entry) }?;
            task.set(dst, Word::UNIT);
        }
        Op::EvRenew { dst, ev, state } => {
            let w = unsafe { ctx.ev_renew(task.get(ev), task.get(state)) }?;
            task.set(dst, w);
        }
        Op::EvLookup { dst, effect } => {
            let w = unsafe { ctx.ev_lookup(effect) }?;
            task.set(dst, w);
        }
        Op::EvClause { dst, ev, op } => {
            let w = unsafe { ctx.ev_clause(task.get(ev), op.into()) }?;
            task.set(dst, w);
        }
        Op::EvEvv { dst, ev } => {
            let w = unsafe { ctx.ev_evv(task.get(ev)) }?;
            task.set(dst, w);
        }
        Op::EvTakeState { dst, ev } => {
            let w = unsafe { ctx.ev_take_state(task.get(ev)) }?;
            task.set(dst, w);
        }
        Op::EvSetState { dst, ev, s } => {
            unsafe { ctx.ev_set_state(task.get(ev), task.get(s)) }?;
            task.set(dst, Word::UNIT);
        }
        Op::Yielding { dst } => {
            let r = ctx.yielding();
            task.set_raw(dst, r);
        }
        Op::YieldStatus { dst, ev } => {
            let r = ctx.yield_status(task.get(ev));
            task.set_raw(dst, r);
        }
        Op::YieldTo { dst, at } => {
            let o = ops(at);
            let (ev, clause, k) = (
                task.get(o[0] as Slot),
                task.get(o[1] as Slot),
                task.get(o[2] as Slot),
            );
            unsafe { ctx.yield_to(ev, clause, k) }?;
            task.set(dst, Word::UNIT);
        }
        Op::YieldAbort { dst, at } => {
            let o = ops(at);
            let (ev, clause, k) = (
                task.get(o[0] as Slot),
                task.get(o[1] as Slot),
                task.get(o[2] as Slot),
            );
            unsafe { ctx.yield_abort(ev, clause, k) }?;
            task.set(dst, Word::UNIT);
        }
        Op::YieldContTake { dst } => {
            let w = ctx.yield_cont_take()?;
            task.set(dst, w);
        }
        Op::YieldContSet { dst, k } => {
            unsafe { ctx.yield_cont_set(task.get(k)) }?;
            task.set(dst, Word::UNIT);
        }
        Op::YieldFinish { dst } => {
            let w = unsafe { ctx.yield_finish() }?;
            task.set(dst, w);
        }
        Op::Dummy { dst } => {
            let w = ctx.heap().dummy();
            task.set(dst, w);
        }
    }
    Ok(true)
}

/// オペランドの表のスロットの並び。
fn slots(list: &[u32]) -> impl Iterator<Item = Slot> {
    list.iter().map(|&slot| slot as Slot)
}

/// `apply` と `clause` の命令。`clause` は照合した命令の種類から受ける。
fn apply_call<M: Mode>(
    code: &CodeBase<M>,
    ctx: &mut Ctx<M>,
    task: &mut Task,
    at: u32,
    target: apply::Target,
    clause: bool,
) -> Result<(), VmFault> {
    let callee = load_args(code, task, at);
    apply::apply(code, ctx, task, callee, target, clause)?;
    ctx.poll(task.depth())?;
    Ok(())
}

/// `[呼ばれる値, n, 引数…]` から引数を scratch に移し、呼ばれる値を返す。全部読んでから書くので、結果のスロットが
/// オペランドのスロットと同じ色でもよい (docs/spec/vm.md の「スロットの割り当て」)。
fn load_args<M: Mode>(code: &CodeBase<M>, task: &mut Task, at: u32) -> Word {
    let o = &code.operands[at as usize..];
    task.load_scratch(slots(&o[2..2 + o[1] as usize]));
    task.get(o[0] as Slot)
}
```

- [ ] **Step 4: Use `load_scratch` in `apply_rest` and delete `prims.rs`**

In `crates/eml_vm/src/apply.rs`, inside `apply_rest`, replace

```rust
    task.scratch.clear();
    for i in 0..n {
        let w = task.get((2 + i) as Slot);
        task.scratch.push(w);
    }
```

with

```rust
    task.load_scratch((2..2 + n).map(|i| i as Slot));
```

Then:

```bash
git rm crates/eml_vm/src/prims.rs
sed -i '/^mod prims;$/d' crates/eml_vm/src/lib.rs
grep -rn "prims" crates/eml_vm/src
```

Expected: `grep` prints nothing. Exactly one loop now reads into the scratch area: `grep -n "scratch.push" crates/eml_vm/src/*.rs` prints only the line in `Task::load_scratch`.

- [ ] **Step 5: Run the pinning tests again**

```bash
cargo fmt
cargo test -p eml_vm
cargo test -p eml_cli --test integration ui::
```

Expected: the same counts as Step 1, all passing. `git diff --no-ext-diff crates/eml_vm/tests` is empty, so no snapshot changed.

- [ ] **Step 6: Full gate**

```bash
cargo test
cargo clippy --all-targets
cargo fmt --check
nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_vm --test integration miri::
nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance -Zmiri-tree-borrows" cargo miri test -p eml_vm --test integration miri::
```

Expected: all pass, clippy prints no warning, and Miri reports `4 passed` for both settings.

- [ ] **Step 7: Commit**

```bash
git add crates/eml_vm/src/vm.rs crates/eml_vm/src/apply.rs crates/eml_vm/src/lib.rs crates/eml_vm/src/prims.rs
git commit -m "Merge the VM's scratch loops and primitive dispatch into one exhaustive match" -m "Five loops that read argument slots into the scratch area become Task::load_scratch, and the two write-back loops become Task::store_scratch. Apply and Clause (and their tail forms) get separate arms that pass the clause flag instead of re-reading the instruction. The primitive instructions move from prims::step, which ended in a wildcard arm, into vm::step, so the instruction match is exhaustive again. Behaviour-preserving: no test changes; the UI tests (CEK vs VM), the eml_vm tests and Miri pin it." -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01MK52fNp5uA711Q2D4kWEtR"
```

### Task 2: Inlining of `eml_vm` functions (spec item 4)

Task 1 removes `prims::step`, so item 4 now covers the `eml_vm` functions that the VM loop still calls without inlining them. The task measures, decides by the rule below, and records the decision. Measured on an Apple M4 in a scratch clone (numbers in Step 6), Task 1 already cut `state`'s execution share by about 17%. `#[inline]` on nine functions then cut every program by another 1.2-5.3%.

Decision rule (written into vm.md in Step 7). An `eml_vm` function gets `#[inline]` only when both of these hold:
- (a) the `-C remark=inline` remarks of a release build show that it is not inlined into the VM loop (`eml_vm::vm::run`, or a function inlined into it such as `vm::apply_call`) because "its definition is unavailable"
- (b) `bench/run.sh` shows that the VM's execution share (program − `empty`) drops: at least one program by 1% or more, and no program rises by more than 0.5% (the run-to-run noise recorded for S10a is at most 0.22%)

**Files:**
- Modify: `crates/eml_vm/src/data.rs` (`#[inline]` on `con` line 66, `switch_tag` 91, `switch_int` 134, `switch_string` 153, `unpack` 176, `release` 198 and `unbox` 236, if Step 5 confirms it)
- Modify: `crates/eml_vm/src/apply.rs` (`#[inline]` on `make_closure` line 136 and `apply_rest` line 157, if Step 5 confirms it)
- Modify: `docs/spec/vm.md` (「置き場所」: one new bullet after the `eml_rt` `#[inline]` bullet, line 17)
- Modify: `docs/implementation/benchmarks.md` (a new `### S10b` heading after the S10a section, at the end of the file. Task 6 adds the final CEK-vs-VM comparison under it)
- Test: no test changes. `cargo test` must stay green, and the `RunStats` snapshots (`crates/eml_vm/tests/bench.rs`) must not change.

**Interfaces:**
- Consumes: Task 1's `vm::step` without `prims::step`.
- Produces: `docs/implementation/benchmarks.md` has the heading `### S10b`, which Task 6 extends. Function signatures do not change.

- [ ] **Step 1: Measure the tree before Task 1**

Run in the devShell (bash 4.3+; macOS's `/bin/bash` 3.2 is refused by the script). `run.sh` builds and measures its own checkout, so the baseline gets its own worktree:

```bash
before="$TMPDIR/eml-bench-before"
git worktree add --detach "$before" HEAD~1   # the commit before Task 1
bash "$before/bench/run.sh" > "$TMPDIR/bench-before.md"
bash bench/run.sh > "$TMPDIR/bench-task1.md"
git worktree remove "$before"
tail -7 "$TMPDIR/bench-before.md"; tail -7 "$TMPDIR/bench-task1.md"
```

Expected: the last table of each file (`| プログラム | CEK の実行の分 | VM の実行の分 | VM / CEK |`) has five rows. In the scratch measurement, the VM's execution share went from 85164176 / 269310576 / 172423839 / 209327314 / 235632891 (fib / list / loop / state / tree) to 81950984 / 265997949 / 165893227 / 173513779 / 230029183. Keep both files for Step 6.

- [ ] **Step 2: Collect the inlining remarks of `eml_vm`**

`vm::run::<M>` is instantiated inside `eml_vm` (by `execute_with`), so remarks for the `eml_vm` crate alone are enough:

```bash
cargo rustc --release -p eml_vm --lib -- -C remark=inline > "$TMPDIR/remark-vm.txt" 2>&1
grep -E 'inline \((missed|success)\)' "$TMPDIR/remark-vm.txt" \
  | sed -E "s/^.*inline \(missed\): ([^ ]+) will not be inlined into ([^ ]+) because (.*)$/missed \1 \2 \3/; s/^.*inline \(success\): '([^']+)' inlined into '([^']+)' (.*)$/success \1 \2 \3/" \
  | c++filt \
  | sed -E 's/::<eml_rt::mode::(Fast|Checked)>//g' \
  | grep -E '^missed eml_vm::[^ ]+ eml_vm::vm::(run|apply_call) its definition is unavailable' \
  | sort -u
```

How to read it: each line is `missed <callee> <caller> <reason>`. `c++filt` (from the devShell's clang wrapper) demangles the Rust v0 symbols, and the `sed` merges the `Fast` and `Checked` instances. `-C remark` warns that it "requires -C debuginfo=n to show source locations". That warning is harmless, because the symbol names are enough.

Expected (scratch measurement), nine `run` lines and one `apply_call` line:

```
missed eml_vm::apply::apply eml_vm::vm::apply_call its definition is unavailable
missed eml_vm::apply::apply_rest eml_vm::vm::run its definition is unavailable
missed eml_vm::apply::make_closure eml_vm::vm::run its definition is unavailable
missed eml_vm::data::con eml_vm::vm::run its definition is unavailable
missed eml_vm::data::release eml_vm::vm::run its definition is unavailable
missed eml_vm::data::switch_int eml_vm::vm::run its definition is unavailable
missed eml_vm::data::switch_string eml_vm::vm::run its definition is unavailable
missed eml_vm::data::switch_tag eml_vm::vm::run its definition is unavailable
missed eml_vm::data::unbox eml_vm::vm::run its definition is unavailable
missed eml_vm::data::unpack eml_vm::vm::run its definition is unavailable
```

(The `apply::apply` line may show up only after Step 3. In the scratch run it appeared in both.) The `Ctx` primitive methods of `eml_rt` (`ev_new`, `evv_mask`, …) also show "definition is unavailable" into `run`. They are large generic `eml_rt` functions, and vm.md's `eml_rt` rule (line 17, measured with `bench/rt.sh`) governs them, so this task leaves them alone.

- [ ] **Step 3: Add `#[inline]` to the nine functions called from `run`**

```bash
perl -0pi -e 's/\npub\(crate\) fn (con|switch_tag|switch_int|switch_string|unpack|release|unbox)</\n#[inline]\npub(crate) fn $1</g' crates/eml_vm/src/data.rs
perl -0pi -e 's/\npub\(crate\) fn (make_closure|apply_rest)</\n#[inline]\npub(crate) fn $1</g' crates/eml_vm/src/apply.rs
grep -c '^#\[inline\]$' crates/eml_vm/src/data.rs crates/eml_vm/src/apply.rs
```

Expected: `data.rs:7` and `apply.rs:2`. Rerun Step 2's commands. None of the nine names may appear any more. A `success eml_vm::data::con eml_vm::vm::run with (cost=…)` line (drop the final `grep` for "missed" to see it) confirms each one.

- [ ] **Step 4: Measure with the nine `#[inline]`**

```bash
bash bench/run.sh > "$TMPDIR/bench-inline.md"
tail -7 "$TMPDIR/bench-inline.md"
```

Expected (scratch measurement): VM execution share 80976137 / 256009575 / 163912180 / 168790757 / 217722935. That is a drop against `bench-task1.md` of 1.2% / 3.8% / 1.2% / 2.7% / 5.3%.

- [ ] **Step 5: Decide, and try `apply::apply` the same way**

Apply rule (b) to `bench-task1.md` against `bench-inline.md`:
- If it holds, keep the nine `#[inline]`. If it does not, revert them (`git checkout -- crates/eml_vm/src/data.rs crates/eml_vm/src/apply.rs`) and use the alternative bullet in Step 6.

Then try the one `apply_call` candidate:

```bash
perl -0pi -e 's/\npub\(crate\) fn apply</\n#[inline]\npub(crate) fn apply</' crates/eml_vm/src/apply.rs
bash bench/run.sh > "$TMPDIR/bench-apply.md"
tail -7 "$TMPDIR/bench-apply.md"
```

Expected (scratch measurement): 81094621 / 257463457 / 163767713 / 168813085 / 217930482. That is within 0.6% of `bench-inline.md` and fails rule (b), so revert this one function:

```bash
perl -0pi -e 's/\n#\[inline\]\npub\(crate\) fn apply</\npub(crate) fn apply</' crates/eml_vm/src/apply.rs
grep -c '^#\[inline\]$' crates/eml_vm/src/apply.rs
```

Expected: `2`. If your run of `bench-apply.md` does satisfy rule (b), keep it instead and add `apply::apply` to the lists in Steps 6 and 7.

- [ ] **Step 6: Record the measurement in `docs/implementation/benchmarks.md`**

Append at the end of the file (after the S10a section). Replace the numbers with the VM execution shares from your three files, in the same positions: column 2 from `bench-before.md`, column 3 from `bench-task1.md`, column 4 from `bench-inline.md`. Replace the percentages with the ones computed from them, and the commit with the `コミット:` line of `bench-before.md`. Copy the machine lines from the head of `bench-inline.md`.

```markdown
### S10b

`bench/run.sh` の記録である。切り替えの前は、同じコミットの CEK と VM を並べて測る。

#### `eml_vm` の関数のインライン化

VM の整理の間に、`eml_vm` の関数のインライン化を測った。表の値は VM の実行の分 (プログラム − `empty`) である。「S10a の終わり」は S10b の作業の前のコミット (7d1bf44)、「分岐の統合」は基本操作の命令を `vm::step` の `match` に移した後、「`#[inline]`」は下の9つの関数に `#[inline]` を付けた後である。

- 機種: Mac16,1 (Apple M4)
- OS: macOS 27.0.1 (26A434)
- rustc: rustc 1.98.1 (48a229cea 2026-09-01)

| プログラム | S10a の終わり | 分岐の統合 | `#[inline]` |
|---|---:|---:|---:|
| `fib` | 85164176 | 81950984 | 80976137 |
| `list` | 269310576 | 265997949 | 256009575 |
| `loop` | 172423839 | 165893227 | 163912180 |
| `state` | 209327314 | 173513779 | 168790757 |
| `tree` | 235632891 | 230029183 | 217722935 |

- `prims::step`: S10a では、基本操作の命令を `vm::step` の外の関数 `prims::step` で実行していた。`-C remark=inline` は、`prims::step` を `vm::run` に展開しなかった理由を「definition is unavailable」と出していた。基本操作の分岐を `vm::step` の網羅的な `match` に移すと、この関数はなくなった。`state` の実行の分は約17% 減り、ほかのプログラムも 1〜4% 減った
- 9つの関数: 分岐を移した後の注釈で、`vm::run` に「definition is unavailable」で展開されなかった `eml_vm` の関数は、`data` の `con`、`switch_tag`、`switch_int`、`switch_string`、`unpack`、`release`、`unbox` と、`apply` の `make_closure`、`apply_rest` の9つだった。9つに `#[inline]` を付けると、注釈からこの理由が消え、実行の分はどのプログラムでも減った (`tree` で約5.3%、`list` で約3.8%、`state` で約2.7%、`fib` と `loop` で約1.2%)。そのため付けた
- `apply::apply`: `vm::apply_call` にも同じ理由で展開されていなかった。付けて測ると、実行の分は 0.6% 以内でしか変わらなかったので、付けていない
```

If Step 5 reverted the nine `#[inline]`, drop the `#[inline]` column and replace the second bullet with:

```markdown
- 9つの関数: 分岐を移した後の注釈で、`vm::run` に「definition is unavailable」で展開されなかった `eml_vm` の関数は、`data` の `con`、`switch_tag`、`switch_int`、`switch_string`、`unpack`、`release`、`unbox` と、`apply` の `make_closure`、`apply_rest` の9つだった。9つに `#[inline]` を付けて測ると、<増えたプログラムと割合> だったので、付けていない
```

with `<増えたプログラムと割合>` filled in from your measurement (for example 「`list` が約0.8% 増えた」).

- [ ] **Step 7: Write the rule into vm.md 「置き場所」**

In `docs/spec/vm.md`, insert after the bullet that starts with `` - `eml_rt` の関数には、VM のために `#[inline]` を足していない。`` (line 17):

```markdown
- `eml_vm` の中の関数には、2つが分かったときだけ `#[inline]` を付ける。1つは、インライン化の注釈 (`-C remark=inline`) で、VM の繰り返し (`vm::run` と、そこに展開される関数) に「definition is unavailable」で展開されていないことである。もう1つは、付けると `bench/run.sh` で命令の数が減ることである。総称な関数の実体は、コード生成単位の1つにしかできず、ほかの単位からは展開できないためである。今は `data` の命令の関数 (`con`、`switch_tag`、`switch_int`、`switch_string`、`unpack`、`release`、`unbox`) と、`apply` の `make_closure` と `apply_rest` に付けている。測った数は [計測の基準](../implementation/benchmarks.md) の「S10b」にある
```

If Step 5 reverted the nine, end the bullet after 「…展開できないためである。」 with 「今はどの関数にも付けていない。測った数は [計測の基準](../implementation/benchmarks.md) の「S10b」にある」.

- [ ] **Step 8: Full gate**

```bash
cargo test
cargo clippy --all-targets
cargo fmt --check
nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_vm --test integration miri::
nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance -Zmiri-tree-borrows" cargo miri test -p eml_vm --test integration miri::
git diff --no-ext-diff --stat
```

Expected: all pass. The diff touches only `data.rs`, `apply.rs`, `vm.md` and `benchmarks.md`. The citations test (`cargo test -p eml_cli --test integration citations`) passes with the new 「S10b」 heading.

- [ ] **Step 9: Commit**

```bash
git add crates/eml_vm/src/data.rs crates/eml_vm/src/apply.rs docs/spec/vm.md docs/implementation/benchmarks.md
git commit -m "Inline the VM's data and closure helpers where the remarks and bench/run.sh show a gain" -m "With prims::step gone, -C remark=inline still reported nine eml_vm functions not inlined into vm::run because their definition was unavailable in that codegen unit. With #[inline] on them, bench/run.sh shows a lower VM execution share for every program; #[inline] on apply::apply made no difference and is not added. The rule and the numbers go into vm.md (置き場所) and benchmarks.md (S10b). No test changes." -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01MK52fNp5uA711Q2D4kWEtR"
```

### Task 3: Code generation cleanups (spec items 5-9)

All fragments were compiled and tested in a scratch clone. Under items 5-9, **no existing snapshot in `crates/eml_vm/tests/codegen.rs` changes**. The spec expected kind-2 changes there, but none of the existing tests has a function whose only temporary slot was the jump scratch slot, a repeated wide constant, or an empty `switch`. Each item is pinned by a new test instead.

**Files:**
- Modify: `crates/eml_vm/src/compile/moves.rs` (`sequentialize` takes `impl FnMut() -> Slot` and calls it only at the first cycle. Tests: signature follow-up plus two new unit tests)
- Modify: `crates/eml_vm/src/compile/emit.rs`. Item 5: `jump` (lines 587-606). Item 7: the `params` clone (line 588). Item 8: new `layout_ctor`, with `field_operands` (lines 273-290) taking the looked-up layout, plus the `Unpack`, `Release`, `Con` and `switch` call sites. Item 9: `switch` (lines 608-702). Also the `impl` header (line 145)
- Modify: `crates/eml_vm/src/compile/constants.rs` (`raws: HashMap<i64, u32>` and `raw` deduplicating, lines 7-31)
- Modify: `crates/eml_vm/src/compile/slots.rs` (line 62: no clone of `live_in[index]`)
- Test: `crates/eml_vm/src/compile/moves.rs` (unit tests), `crates/eml_vm/tests/codegen.rs` (two new snapshot tests)

**Interfaces:**
- Consumes: nothing from earlier tasks.
- Produces: `pub(crate) fn sequentialize(moves: &[(Slot, Slot)], scratch: impl FnMut() -> Slot) -> Vec<(Slot, Slot)>`. `Emitter::layout_ctor(&self, ctor: Ctor) -> &'a LayoutCtor`. `Emitter::field_operands(&self, layout_ctor: &LayoutCtor, shape: &Shape, fields: &[VarId]) -> Vec<u32>`.

- [ ] **Step 1: Write the failing tests for item 5 (scratch slot only for a cycle)**

In `crates/eml_vm/src/compile/moves.rs`, the existing tests pass the scratch slot as a closure (mechanical follow-up, kind 3). Run:

```bash
sed -i 's/sequentialize(moves, scratch)/sequentialize(moves, || scratch)/; s/\], 7)\.len()/], || 7).len()/; s/sequentialize(&\[(3, 3)\], 7)/sequentialize(\&[(3, 3)], || 7)/' crates/eml_vm/src/compile/moves.rs
grep -n "sequentialize(" crates/eml_vm/src/compile/moves.rs
```

Expected: the four test call sites read `|| scratch` or `|| 7`. Then add before `fn a_self_move_disappears`:

```rust
    #[test]
    fn moves_without_a_cycle_take_no_scratch_slot() {
        let order = sequentialize(&[(1, 0), (2, 1), (3, 2)], || {
            panic!("moves without a cycle asked for a scratch slot")
        });
        assert_eq!(order.len(), 3);
    }

    #[test]
    fn two_cycles_share_one_scratch_slot() {
        let moves = [(0, 1), (1, 0), (2, 3), (3, 2)];
        simulate(&moves, 7);
        let mut asked = 0;
        let order = sequentialize(&moves, || {
            asked += 1;
            7
        });
        assert_eq!((asked, order.len()), (1, 6));
    }
```

```bash
cargo test -p eml_vm --lib moves
```

Expected: compile error `expected u16, found closure` (E0308) at the test call sites, because `sequentialize` still takes a `Slot`.

- [ ] **Step 2: Implement item 5**

In `crates/eml_vm/src/compile/moves.rs`, replace the head of `sequentialize` and the cycle arm:

```rust
/// 並列な代入 `dst := src` の組を、順に実行してよい `(dst, src)` の列にする。どの代入も、まだ読まれていない値を
/// 上書きしない順に並べる。輪は作業用のスロットを通して切る。作業用のスロットは最初の輪で `scratch` から1つだけ
/// 取り、どの輪にも使う。輪を切った後に残る代入は、ほかの輪の前にすべて並べられるためである。
pub(crate) fn sequentialize(
    moves: &[(Slot, Slot)],
    mut scratch: impl FnMut() -> Slot,
) -> Vec<(Slot, Slot)> {
    let mut pending: Vec<(Slot, Slot)> = moves.iter().copied().filter(|(d, s)| d != s).collect();
    let mut out = Vec::with_capacity(pending.len() + 1);
    let mut taken = None;
    while !pending.is_empty() {
        let ready = pending
            .iter()
            .position(|&(dst, _)| !pending.iter().any(|&(_, src)| src == dst));
        match ready {
            Some(index) => out.push(pending.swap_remove(index)),
            None => {
                // 残りはどれも、ほかの代入がまだ読む値を上書きする。1つの値を退避して輪を切る
                let slot = *taken.get_or_insert_with(&mut scratch);
                let (_, src) = pending[0];
                out.push((slot, src));
                for (_, s) in &mut pending {
                    if *s == src {
                        *s = slot;
                    }
                }
            }
        }
    }
    out
}
```

In `crates/eml_vm/src/compile/emit.rs`, `jump` (lines 587-606) becomes (this also does item 7's `params` clone):

```rust
    pub(crate) fn jump(&mut self, from: BlockId, target: BlockId, args: &[Atom]) {
        let function = self.function;
        let params = &function.block(target).params;
        let mut moves = Vec::new();
        let mut constants = Vec::new();
        for (&param, &arg) in params.iter().zip(args) {
            let dst = self.slot(param);
            match arg {
                Atom::Var(var) => moves.push((dst, self.slot(var))),
                constant => constants.push((dst, constant, self.repr(param))),
            }
        }
        // 作業用のスロットは、代入が輪になるときだけ取る
        let ordered = super::moves::sequentialize(&moves, || self.temp());
        for (dst, src) in ordered {
            self.out.push(Op::Move { dst, src });
        }
        for (dst, constant, repr) in constants {
            self.load(dst, constant, repr);
        }
        self.goto(from, target);
    }
```

(The result is bound to `ordered` first: a closure that borrows `self` inside a `for` header would live through the loop body, which pushes to `self.out`.)

```bash
cargo test -p eml_vm
```

Expected: 7 unit tests and 89 integration tests pass. `codegen.rs` is unchanged: `swapped_block_arguments_go_through_the_scratch_slot` still uses `s2` with frame 4, because the swap is a cycle. `a_jump_loads_constants_after_moving_variables` keeps frame 4, because its `+(1, 2)` already takes two temporaries.

- [ ] **Step 3: Write the failing test for item 6 (one table entry per wide constant)**

Append to `crates/eml_vm/tests/codegen.rs`:

```rust
/// `i32` に収まらない同じ値の定数は、定数の表に1つだけ置く。
#[test]
fn a_wide_constant_is_stored_once() {
    insta::assert_snapshot!(disasm("\
fn main() -> unit {
  let a.0: int = extern \"Prelude.+\"(5000000000, 1)
  let b.1: int = extern \"Prelude.+\"(a.0, 5000000000)
  let s.2: obj = extern \"Prelude.Show Int.show\"(b.1)
  let u.3: unit = extern Prelude.println(s.2)
  return u.3
}
"), @"
    fn main (arity 0, frame 3)
         2  load_const s1, c0
         3  load_raw s2, 1
         4  int_add s0, s1, s2
         5  load_const s1, c0
         6  int_add s0, s0, s1
         7  extern s0 = Prelude.Show Int.show(s0)
         8  extern s0 = Prelude.println(s0)
         9  ret s0
    ");
}
```

```bash
cargo test -p eml_vm --test integration codegen::a_wide_constant_is_stored_once
```

Expected: FAIL. The actual line 5 is `load_const s1, c1`.

- [ ] **Step 4: Implement item 6**

In `crates/eml_vm/src/compile/constants.rs`, add `use std::collections::HashMap;` above `use std::sync::Arc;`, a field, its initializer, and the lookup:

```rust
pub(crate) struct Constants<M: Mode> {
    heap: Heap<M>,
    words: Vec<Word>,
    /// 生の整数の値ごとの番号。同じ値を表に2度置かない。
    raws: HashMap<i64, u32>,
}
```

```rust
        Constants {
            heap: Heap::with_control(control),
            words: Vec::new(),
            raws: HashMap::new(),
        }
```

```rust
    /// 生の整数。`LoadRaw` の `i32` に収まらない `Int` の定数に使う。
    pub(crate) fn raw(&mut self, n: i64) -> u32 {
        if let Some(&index) = self.raws.get(&n) {
            return index;
        }
        let index = self.push(Word::from_bits(n as usize));
        self.raws.insert(n, index);
        index
    }
```

```bash
cargo test -p eml_vm --test integration codegen::
```

Expected: all codegen tests pass.

- [ ] **Step 5: Items 7 and 8 (no needless clones; one layout lookup per instruction)**

Item 7, `crates/eml_vm/src/compile/slots.rs` lines 62-64: replace

```rust
        let entry: Vec<VarId> = live_in[index].clone();
        for &param in &block.params {
            for &other in block.params.iter().chain(&entry) {
```

with

```rust
        for &param in &block.params {
            for &other in block.params.iter().chain(&live_in[index]) {
```

(The `params` clone in `emit.rs` went away in Step 2. `grep -n "clone()" crates/eml_vm/src/compile/*.rs` now shows only the `function.name.clone()` calls for the error and the `FnEntry`, `program.files.clone()` for the `CodeBase`, and the test's `before.clone()`. All of them are owned results.)

Item 8, `crates/eml_vm/src/compile/emit.rs`:
- the `use` line becomes

```rust
use eml_lir::{
    Atom, BlockId, Call, Ctor, FnIdx, LayoutCtor, LirFn, Program, Repr, Rhs, Shape, Stmt, Term,
    VarId,
};
```

- line 145 `impl<M: Mode> Emitter<'_, M> {` becomes `impl<'a, M: Mode> Emitter<'a, M> {`
- replace `field_operands` (lines 273-290) with

```rust
    /// コンストラクタの配置。命令ごとに1回引き、形 (`LayoutCtor::shape`) とともに使い回す。
    fn layout_ctor(&self, ctor: Ctor) -> &'a LayoutCtor {
        &self.program.layouts[ctor.layout.0 as usize].constructors[ctor.tag as usize]
    }

    /// 使うフィールドの [位置 | RAW, スロット] の並び。死んでいるフィールドは読まない。
    fn field_operands(
        &self,
        layout_ctor: &LayoutCtor,
        shape: &Shape,
        fields: &[VarId],
    ) -> Vec<u32> {
        let offset = crate::data::count_word(shape.scanned);
        let mut items = Vec::new();
        for ((&field, &position), &repr) in
            fields.iter().zip(&shape.positions).zip(&layout_ctor.fields)
        {
            if self.used[field.0 as usize] {
                let raw = if repr.is_rc() { 0 } else { RAW };
                items.push((position + offset) | raw);
                items.push(u32::from(self.slot(field)));
            }
        }
        items
    }
```

- `Stmt::Unpack` (lines 232-235) becomes

```rust
                let layout_ctor = self.layout_ctor(*ctor);
                let shape = layout_ctor.shape();
                let bound = self.field_operands(layout_ctor, &shape, fields);
```

- in `Stmt::Release` (lines 249-250) and `Rhs::Con` (lines 327-328), replace

```rust
                let layout_ctor =
                    &self.program.layouts[ctor.layout.0 as usize].constructors[ctor.tag as usize];
```

with `let layout_ctor = self.layout_ctor(*ctor);`
- in `switch`, the tag case's fields (lines 671-679) become

```rust
                    let (check, fields) = if case.fields.is_empty() {
                        (NONE, Vec::new())
                    } else {
                        let layout_ctor = &ctors[tag as usize];
                        let shape = layout_ctor.shape();
                        let fields = self.field_operands(layout_ctor, &shape, &case.fields);
                        (shape.scanned, fields)
                    };
```

```bash
cargo test -p eml_vm
grep -n "\.shape()" crates/eml_vm/src/compile/emit.rs
```

Expected: all pass with `codegen.rs` byte-identical (`git diff --no-ext-diff crates/eml_vm/tests/codegen.rs` shows only Step 3's test). `grep` shows one `.shape()` per instruction: in `Unpack`, `Release`, `Con` and the tag case of `switch`.

- [ ] **Step 6: Write the failing test for item 9 (an empty switch is a `goto`)**

Append to `crates/eml_vm/tests/codegen.rs`:

```rust
/// case のない `switch` は、値を比べずに `default` へ進むので、`goto` になる。
#[test]
fn a_switch_without_cases_is_a_goto() {
    insta::assert_snapshot!(disasm("\
layout Bool { False, True }
fn main() -> unit {
  let n.0: int = extern \"Prelude.+\"(1, 2)
  let d.1: enum = extern \"Prelude.Eq Int.==\"(n.0, 3)
  switch d.1 Bool { #0 -> b2, #1 -> b1 }
b1:
  switch n.0 { _ -> b3 }
b2:
  return ()
b3:
  let s.2: obj = extern \"Prelude.Show Int.show\"(n.0)
  let u.3: unit = extern Prelude.println(s.2)
  return u.3
}
"), @"
    fn main (arity 0, frame 4)
         2  load_raw s2, 1
         3  load_raw s3, 2
         4  int_add s0, s2, s3
         5  load_raw s2, 3
         6  int_eq s1, s0, s2
         7  switch_enum s1 { #0 -> 9 [], #1 -> 8 [] }
         8  goto 11
         9  load_imm s2, 0
        10  ret s2
        11  extern s0 = Prelude.Show Int.show(s0)
        12  extern s0 = Prelude.println(s0)
        13  ret s0
    ");
}
```

```bash
cargo test -p eml_vm --test integration codegen::a_switch_without_cases_is_a_goto
```

Expected: FAIL. The actual line 8 is `switch_int s0 { _ -> 11 }`.

- [ ] **Step 7: Implement item 9**

The verifier rejects a layout without tag cases (`check.rs` `switch_layout`, "a switch without tag cases has the layout") and a switch with neither cases nor a default ("a switch has no targets"). So a switch without cases has no layout and does have a default. In `switch` in `crates/eml_vm/src/compile/emit.rs`, insert after the constant-scrutinee `let … else { … };` block:

```rust
        // case のない `switch` は値を比べずに `default` へ進むので、キーのない表を作らない
        let Some(first) = cases.first().map(|case| case.pattern) else {
            let target = default.expect("a switch without cases has a default");
            self.goto(block, target);
            return;
        };
```

and change the match on the first pattern:

```rust
        let (items, op) = match (first, layout) {
            (CasePattern::Int(_) | CasePattern::String(_), _) => {
```

```rust
                let op = if matches!(first, CasePattern::String(_)) {
```

```rust
            (CasePattern::Tag(_), Some(layout)) => {
```

```rust
            (CasePattern::Tag(_), None) => {
                unreachable!("a switch with tag cases names its layout (docs/spec/lir.md)")
            }
```

(These replace, in order, `match (cases.first().map(|case| case.pattern), layout) {` with its first arm `(Some(CasePattern::Int(_) | CasePattern::String(_)), _) | (None, None) => {`, the `matches!(cases.first().map(|c| c.pattern), Some(CasePattern::String(_)))` test, `(_, Some(layout)) => {`, and `(Some(CasePattern::Tag(_)), None) => {`.)

```bash
cargo fmt
cargo test -p eml_vm
```

Expected: all pass (7 unit tests in the lib, 91 integration tests).

- [ ] **Step 8: Full gate**

```bash
cargo test
cargo clippy --all-targets
cargo fmt --check
nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_vm --test integration miri::
nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance -Zmiri-tree-borrows" cargo miri test -p eml_vm --test integration miri::
git diff --no-ext-diff crates/eml_vm/tests/codegen.rs | grep '^-' | grep -v '^---'
```

Expected: all pass. The last command prints nothing: no existing snapshot line was removed, and the file only gained two tests.

- [ ] **Step 9: Commit**

```bash
git add crates/eml_vm/src/compile crates/eml_vm/tests/codegen.rs
git commit -m "Tidy the VM code generator: scratch slot only for cycles, shared wide constants, one layout lookup, goto for empty switches" -m "The jump scratch slot is taken only when the parallel move has a cycle; Constants::raw keeps one table entry per value; slots.rs and emit.rs borrow instead of cloning; each instruction looks up its constructor layout and shape once; a switch without cases emits a goto to its default instead of a SwitchInt without keys. Test changes: new unit tests moves_without_a_cycle_take_no_scratch_slot and two_cycles_share_one_scratch_slot, new codegen snapshots a_wide_constant_is_stored_once and a_switch_without_cases_is_a_goto; the existing sequentialize unit tests pass the scratch slot as a closure (kind 3, mechanical). No existing codegen snapshot changes." -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01MK52fNp5uA711Q2D4kWEtR"
```

### Task 4: Switch snapshots and the effect-number unit test (spec items 10-11)

**Files:**
- Modify: `crates/eml_vm/src/compile/emit.rs`: the closure `effect` inside `Emitter::prim` (lines 428-435 before Task 3; find it with `grep -n "let effect = |e: u32|"`) becomes a free function `effect_number`, and a `#[cfg(test)] mod tests` goes at the end of the file
- Modify: `crates/eml_vm/tests/codegen.rs` (delete `ev_new_of_effect_65535_is_rejected_by_the_code_generator` and its doc comment, lines 268-288; add three snapshot tests)
- Modify: `docs/implementation/testing.md` (line 165, the `codegen.rs` bullet of 「`eml_vm` のテスト」)
- Test: `crates/eml_vm/tests/codegen.rs`, `crates/eml_vm/src/compile/emit.rs`

**Interfaces:**
- Consumes: Task 3's `switch` (the snapshots below were taken after Task 3; Task 3 does not change their output).
- Produces: `fn effect_number(e: u32) -> u16` (private to `emit.rs`).

- [ ] **Step 1: Add the three switch snapshots with empty values**

Append to `crates/eml_vm/tests/codegen.rs`:

```rust
/// `enum` の scrutinee の `switch` は、生の値をタグとして読む `switch_enum` になる。フィールドのない case だけなので、
/// どの行き先にも束縛はない。
#[test]
fn a_switch_on_an_enum_is_switch_enum() {
    insta::assert_snapshot!(disasm("\
layout Bool { False, True }
fn main() -> unit {
  tail call pick(3)
}
fn pick(n.0: int) -> unit {
  let d.1: enum = extern \"Prelude.Eq Int.==\"(n.0, 3)
  switch d.1 Bool { #0 -> b1, #1 -> b2 }
b1:
  return ()
b2:
  let s.2: obj = const \"three\"
  let u.3: unit = extern Prelude.println(s.2)
  return u.3
}
"), @"");
}

/// `Int` の `switch` は、キーと行き先を表に並べる `switch_int` になる。`i32` に収まらないキーも表に入る。
#[test]
fn a_switch_on_an_int_is_switch_int() {
    insta::assert_snapshot!(disasm("\
fn main() -> unit {
  tail call pick(2)
}
fn pick(n.0: int) -> unit {
  switch n.0 { 1 -> b1, 5000000000 -> b2, _ -> b3 }
b1:
  let s.1: obj = const \"one\"
  let u.2: unit = extern Prelude.println(s.1)
  return u.2
b2:
  let s.3: obj = const \"big\"
  let u.4: unit = extern Prelude.println(s.3)
  return u.4
b3:
  return ()
}
"), @"");
}

/// `String` の `switch` は、キーを定数の表の文字列で指す `switch_string` になる。
#[test]
fn a_switch_on_a_string_is_switch_string() {
    insta::assert_snapshot!(disasm("\
fn main() -> unit {
  let s.0: obj = const \"a\"
  let n.1: int = call pick(s.0)
  return ()
}
fn pick(s.0: obj) -> int {
  switch s.0 { \"a\" -> b1, \"b\" -> b2, _ -> b3 }
b1:
  decref s.0
  return 1
b2:
  decref s.0
  return 2
b3:
  decref s.0
  return 3
}
"), @"");
}
```

- [ ] **Step 2: Fill in the snapshots and check them**

```bash
cargo insta test -p eml_vm --accept -- codegen::
git diff --no-ext-diff crates/eml_vm/tests/codegen.rs
```

Expected: insta accepts exactly the three new tests, and the diff only adds them. The accepted values must equal these (measured in the scratch clone):

`a_switch_on_an_enum_is_switch_enum`:

```
    fn main (arity 0, frame 1)
         2  load_raw s0, 3
         3  tail_call pick(s0)
    fn pick (arity 1, frame 2)
         4  load_raw s1, 3
         5  int_eq s0, s0, s1
         6  switch_enum s0 { #0 -> 7 [], #1 -> 9 [] }
         7  load_imm s1, 0
         8  ret s1
         9  load_const s0, c0
        10  extern s0 = Prelude.println(s0)
        11  ret s0
```

`a_switch_on_an_int_is_switch_int`:

```
    fn main (arity 0, frame 1)
         2  load_raw s0, 2
         3  tail_call pick(s0)
    fn pick (arity 1, frame 2)
         4  switch_int s0 { 1 -> 5, 5000000000 -> 8, _ -> 11 }
         5  load_const s0, c0
         6  extern s0 = Prelude.println(s0)
         7  ret s0
         8  load_const s0, c1
         9  extern s0 = Prelude.println(s0)
        10  ret s0
        11  load_imm s1, 0
        12  ret s1
```

`a_switch_on_a_string_is_switch_string`:

```
    fn main (arity 0, frame 2)
         2  load_const s0, c0
         3  call s0 = pick(s0)
         4  load_imm s1, 0
         5  ret s1
    fn pick (arity 1, frame 2)
         6  switch_string s0 { 0 -> 7, 1 -> 10, _ -> 13 }
         7  decref s0
         8  load_raw s1, 1
         9  ret s1
        10  decref s0
        11  load_raw s1, 2
        12  ret s1
        13  decref s0
        14  load_raw s1, 3
        15  ret s1
```

In `switch_int`, the wide key `5000000000` is printed whole, which confirms that the table stores both 32-bit halves. In `switch_string`, the keys `0` and `1` are constant indices: `"a"` is `c0` (the same constant `main` loads at 2) and `"b"` is `c1`.

What the reviewer verifies: each switch is the instruction its name says; the case targets are the first instruction of the right block; the default appears as `_ -> N`; nothing outside the three new tests changed.

- [ ] **Step 3: Write the unit test for the effect-number check**

At the end of `crates/eml_vm/src/compile/emit.rs` add:

```rust
#[cfg(test)]
mod tests {
    use super::effect_number;

    /// 番号 65534 までは変換し、65535 はコード生成で断る。番号 65535 のエフェクトを持つプログラムを作る統合テストは
    /// 遅いので、変換の関数を直に確かめる。
    #[test]
    #[should_panic(expected = "effect numbers are below 65535")]
    fn effect_65535_is_rejected_by_the_code_generator() {
        assert_eq!(effect_number(65534), 65534);
        effect_number(65535);
    }
}
```

```bash
cargo test -p eml_vm --lib effect_65535
```

Expected: compile error E0432 `unresolved import super::effect_number`.

- [ ] **Step 4: Extract `effect_number`**

In `Emitter::prim`, delete the closure and its comment:

```rust
        // evidence の tag はエフェクトの番号 + 1 なので、番号は 65535 より小さい (docs/spec/object-model.md の
        // 「evidence の3つの物体」)。速いモードの `ev_new` はこれを確かめない
        let effect = |e: u32| {
            u16::try_from(e)
                .ok()
                .filter(|&e| e < u16::MAX)
                .expect("effect numbers are below 65535 (docs/spec/object-model.md)")
        };
```

and rename its three call sites:

```bash
sed -i 's/\beffect(\*e)/effect_number(*e)/g; s/|&e| effect(e)/|\&e| effect_number(e)/' crates/eml_vm/src/compile/emit.rs
grep -n "effect(" crates/eml_vm/src/compile/emit.rs
```

Expected: `grep` prints nothing (the remaining names are `effect_number(`). Then add, just above `/// 算術と比較の extern の専用の命令`:

```rust
/// エフェクトの番号を命令の `u16` にする。evidence の tag はエフェクトの番号 + 1 なので、番号は 65535 より小さい
/// (docs/spec/object-model.md の「evidence の3つの物体」)。速いモードの `ev_new` はこれを確かめないので、コード生成が
/// panic で確かめる。
fn effect_number(e: u32) -> u16 {
    u16::try_from(e)
        .ok()
        .filter(|&e| e < u16::MAX)
        .expect("effect numbers are below 65535 (docs/spec/object-model.md)")
}
```

```bash
cargo fmt
cargo test -p eml_vm --lib effect_65535
```

Expected: `test compile::emit::tests::effect_65535_is_rejected_by_the_code_generator - should panic ... ok`.

- [ ] **Step 5: Delete the slow integration test (kind 1)**

In `crates/eml_vm/tests/codegen.rs`, delete the whole block that starts with the doc comment `/// evidence の tag はエフェクトの番号 + 1 なので、番号 65535 のエフェクトの evidence は作れない` and ends with the closing `}` of `fn ev_new_of_effect_65535_is_rejected_by_the_code_generator` (lines 268-288, including the blank line after it).

```bash
grep -n "65535" crates/eml_vm/tests/codegen.rs
cargo test -p eml_vm
```

Expected: `grep` prints nothing. Tests: 8 unit tests in the lib, 93 integration tests (three added, one deleted).

- [ ] **Step 6: Update testing.md**

In `docs/implementation/testing.md`, replace the bullet at line 165 (it begins with `` - コード生成 (`codegen.rs`): ``) with:

```markdown
- コード生成 (`codegen.rs`): 手書きの LIR を `CodeBase` に変換し、逆アセンブラの出力をインラインスナップショットで固定する。スロットの割り当て (重ならない値のスロットの共有、使われない呼び出しの結果、scrutinee とフィールドの干渉)、入れ替えになるブロック引数の作業用のスロット、`jump` の定数の読み込みの順、`#N` の行き先の `decref` を出さないこと、`release` の展開、同じ広い定数を表に1つだけ置くこと、3つの `switch` (`switch_enum`、`switch_int`、`switch_string`) の表、case のない `switch` を `goto` にすることを確かめる。番号 65535 のエフェクトをコード生成が panic で断ることは、`src/compile/emit.rs` の単体テストで確かめる。そのエフェクトを持つプログラムは 65536 個のエフェクトを宣言するので、統合テストでは遅いためである
```

- [ ] **Step 7: Full gate**

```bash
cargo test
cargo clippy --all-targets
cargo fmt --check
nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_vm --test integration miri::
nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance -Zmiri-tree-borrows" cargo miri test -p eml_vm --test integration miri::
```

Expected: all pass, 0 clippy warnings, citations test green.

- [ ] **Step 8: Commit**

```bash
git add crates/eml_vm/src/compile/emit.rs crates/eml_vm/tests/codegen.rs docs/implementation/testing.md
git commit -m "Pin the three switch encodings and check effect numbers with a unit test" -m "Add codegen snapshots a_switch_on_an_enum_is_switch_enum, a_switch_on_an_int_is_switch_int and a_switch_on_a_string_is_switch_string. The effect-number check becomes the private function effect_number with the unit test effect_65535_is_rejected_by_the_code_generator; the integration test ev_new_of_effect_65535_is_rejected_by_the_code_generator, which built 65536 effects, is deleted (kind 1, pass/fail change, as the S10b design lists)." -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01MK52fNp5uA711Q2D4kWEtR"
```

### Task 5: `peak_stack`, the stack capacity cap, and the memory-limit test

How the cap knows `max_depth`: `eml_vm::run` already holds the `Arc<Control>` before it moves it into `Ctx::new`. It reads `control.limits().max_depth` there and passes the value to `Task::new`, which stores `stack_limit = max_depth.saturating_add(u16::MAX + 2)`. `Ctx` needs no new accessor.

The cap is on `Vec` capacity, not on length. `peak_stack` reads the length, so no `RunStats` can show the cap, and a run stopped by the depth limit returns no `RunStats` at all. That is why the cap test the coordinator asked for ("a run stopped by the depth limit keeps peak_stack within max_depth + u16::MAX + 2") is a unit test of `Task::reserve` in `vm.rs`. It drives the same growth that a deep recursion drives, against a `Task` whose `stack_limit` comes from a `max_depth`.

Note for the spec: one case pushes two frames before a safepoint. An over-application through `Target::Frame` pushes the `ApplyRest` frame and then the callee frame (`apply.rs` lines 86-102, then `enter`), and only `apply_call` polls after that. The length can then reach `max_depth + 2 × (u16::MAX + 2)`. `reserve` therefore never caps below the requested length (`.max(end)`). The memory bound stays within about one more frame of the spec's figure. The code comment states the one-frame reason, and the test `reserve_beyond_the_stack_limit_takes_exactly_the_length` covers the overshoot.

Memory limit and the constants region: `compile` builds the constants on a working heap with the same `Control` (`Constants::new` → `Heap::with_control`). Pages are 64 KiB (`eml_rt::alloc::PAGE_BYTES = 0x1_0000`, crate-private). Measured in the scratch clone, a 100-cons run under `max_bytes` of 1 or 2 pages already fails, and 3 pages pass. The baseline is the constants region page plus the execution heap's first pages. The test uses 4 pages (`4 * 0x1_0000`) for margin and asserts that the small run succeeds under the same limit. That assertion is the check that the limit sits above the region.

**Files:**
- Modify: `crates/eml_vm/src/vm.rs`. Line 16-26: new field `stack_limit` on `Task`. Line 39: `Task::new` takes `max_depth`. Lines 77-81: `reserve` with the cap. New free function `stack_limit` after `impl Task`. New `#[cfg(test)] mod tests` at the end of the file
- Modify: `crates/eml_vm/src/lib.rs`. Lines 30-42: `RunStats` gains `peak_stack`. Lines 83-86: read `max_depth` and pass it to `Task::new`. Lines 112-122: fill `peak_stack`
- Modify: `crates/eml_vm/tests/bench.rs` (six snapshots gain `peak_stack`, kind 2)
- Modify: `crates/eml_vm/tests/calls.rs` (new helper `cons_loop` and test `a_growing_heap_hits_the_memory_limit` at the end)
- Modify: `docs/spec/vm.md`: 「スロットとスタック」 bullet at line 52, and 「`RunStats`」 (line 185 sentence plus a new table row)
- Test: `crates/eml_vm/src/vm.rs` (unit), `crates/eml_vm/tests/calls.rs`, `crates/eml_vm/tests/bench.rs`

**Interfaces:**
- Consumes: Task 1's `Task` (`load_scratch`, `store_scratch`).
- Produces: `pub peak_stack: u64` as the last field of `eml_vm::RunStats` (`#[non_exhaustive]`, checked mode only). `pub(crate) fn Task::new<M: Mode>(code: &CodeBase<M>, max_depth: usize) -> Task`. `pub(crate) stack_limit: usize` on `Task`. `fn stack_limit(max_depth: usize) -> usize` in `vm.rs`. Task 7's `scaling.rs` reads `peak_stack` through `eml_test_support::run_stats_vm`, which returns `eml_vm::RunStats` unchanged.

- [ ] **Step 1: Write the failing unit tests for the cap**

At the end of `crates/eml_vm/src/vm.rs` add:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn empty_task(max_depth: usize) -> Task {
        Task {
            stack: Vec::new(),
            pc: FINISH,
            base: 0,
            frame_size: 0,
            last_ret: FINISH,
            scratch: Vec::new(),
            stack_limit: stack_limit(max_depth),
        }
    }

    #[test]
    fn the_stack_limit_is_the_depth_limit_and_one_frame() {
        assert_eq!(stack_limit(1000), 1000 + 65535 + 2);
        assert_eq!(stack_limit(usize::MAX), usize::MAX);
    }

    /// 深さの上限で止まる実行でも、スタックの容量は上限と1つの枠の分を超えない。
    #[test]
    fn reserve_doubles_the_capacity_up_to_the_stack_limit() {
        let mut task = empty_task(0);
        let limit = task.stack_limit;
        task.reserve(30_000);
        assert_eq!(task.stack.capacity(), 30_000);
        task.reserve(30_001);
        assert_eq!(task.stack.capacity(), 60_000);
        task.reserve(60_001);
        assert_eq!((task.stack.len(), task.stack.capacity()), (60_001, limit));
        task.reserve(limit);
        assert_eq!((task.stack.len(), task.stack.capacity()), (limit, limit));
    }

    /// 上限を超える長さを求められたら、容量をその長さちょうどにする。`ApplyRest` の枠と呼ばれる側の枠を、同じ安全点の
    /// 前に積むときに起きる。
    #[test]
    fn reserve_beyond_the_stack_limit_takes_exactly_the_length() {
        let mut task = empty_task(0);
        let end = task.stack_limit + 100;
        task.reserve(end);
        assert_eq!((task.stack.len(), task.stack.capacity()), (end, end));
    }
}
```

```bash
cargo test -p eml_vm --lib vm::tests
```

Expected: compile errors: no field `stack_limit` on `Task` (E0560) and cannot find function `stack_limit` (E0425).

- [ ] **Step 2: Implement the cap**

In `crates/eml_vm/src/vm.rs`:

Add the field at the end of `struct Task`, after `scratch`:

```rust
    /// `reserve` がスタックの容量を伸ばす上限の語の数 (`stack_limit`)。
    pub(crate) stack_limit: usize,
```

Change `Task::new`'s signature to `pub(crate) fn new<M: Mode>(code: &CodeBase<M>, max_depth: usize) -> Task {` and add `stack_limit: stack_limit(max_depth),` after `scratch: Vec::new(),` in its struct literal.

Replace `reserve` (lines 77-81) with:

```rust
    /// スタックの長さを `end` 語まで伸ばす。容量は倍に伸ばすが、`stack_limit` で止める。`end` が上限を超えるときは、
    /// 容量を `end` ちょうどにする。
    pub(crate) fn reserve(&mut self, end: usize) {
        if self.stack.len() >= end {
            return;
        }
        if end > self.stack.capacity() {
            let capacity = (2 * self.stack.capacity()).min(self.stack_limit).max(end);
            self.stack.reserve_exact(capacity - self.stack.len());
        }
        self.stack.resize(end, Word::UNIT);
    }
```

Add before `/// 見出しの1語目。`:

```rust
/// スタックの容量の上限。深さが `max_depth` を超えると次の安全点で止まるので、安全点の前に積みうる1つの枠と見出し
/// (`u16::MAX + 2` 語) を足した分より大きい容量は要らない (docs/spec/vm.md の「スロットとスタック」)。
fn stack_limit(max_depth: usize) -> usize {
    max_depth.saturating_add(usize::from(u16::MAX) + 2)
}
```

In `crates/eml_vm/src/lib.rs` `run`, replace

```rust
    let mut ctx = Ctx::<M>::new(control, host);
```

with

```rust
    let max_depth = control.limits().max_depth;
    let mut ctx = Ctx::<M>::new(control, host);
```

and `let mut task = Task::new(&code);` with `let mut task = Task::new(&code, max_depth);`.

```bash
cargo test -p eml_vm --lib vm::tests
```

Expected: 3 passed (the lib now has 11 unit tests).

- [ ] **Step 3: Add `peak_stack` to `RunStats`**

In `crates/eml_vm/src/lib.rs`, add after `pub peak_objects: u64,` in `struct RunStats`:

```rust
    /// タスクのスタックで同時に使った語の数の最大。
    pub peak_stack: u64,
```

and after `peak_objects: h.peak_live,` in `run`'s result:

```rust
        // スタックの長さは縮めないので、長さがそのまま使った語の数の最大である
        peak_stack: task.stack.len() as u64,
```

```bash
cargo test -p eml_vm --test integration bench::
```

Expected: FAIL on the 6 snapshot tests `bench::{empty,fib,list,loop_,state,tree}`. The new line `peak_stack: …` is missing from each stored snapshot. `every_program_in_bench_has_a_test` passes.

- [ ] **Step 4: Accept the bench snapshots (kind 2) and check the diff**

```bash
cargo insta test -p eml_vm --accept -- bench::
git diff --no-ext-diff crates/eml_vm/tests/bench.rs
```

Expected: the only changes are six added lines, one per snapshot, after `peak_objects`. No other line changes. The values measured in the scratch clone (after Task 3) were:

| test | added line |
|---|---|
| `empty` | `peak_stack: 7,` |
| `fib` | `peak_stack: 132,` |
| `list` | `peak_stack: 700015,` |
| `loop_` | `peak_stack: 14,` |
| `state` | `peak_stack: 27,` |
| `tree` | `peak_stack: 392,` |

What the reviewer checks: `list` (a non-tail recursion over 100000 elements) is in the hundreds of thousands, and `loop_` (a tail loop) is a handful of words. If any `rc_*`, `peak_objects` or other value changed, stop: this task must not change counts.

- [ ] **Step 5: Write the memory-limit test**

Append to `crates/eml_vm/tests/calls.rs` (`Arc`, `Control`, `Limits` and `parse` are already imported at the top):

```rust
/// `n` 個の cons を作ってから捨てる。cons は自己末尾呼び出しのループで作るので、深さは伸びず、ヒープだけが伸びる。
fn cons_loop(n: u32) -> String {
    format!(
        "\
layout Bool {{ False, True }}
layout List {{ Nil, Cons(tobj, tobj) }}
fn main() -> unit {{
  let r.0: unit = call build({n}, #0)
  return r.0
}}
fn build(n.0: int, acc.1: tobj) -> unit {{
  let d.2: enum = extern \"Prelude.Eq Int.==\"(n.0, 0)
  switch d.2 Bool {{ #0 -> b1, #1 -> b2 }}
b1:
  let m.3: int = extern \"Prelude.-\"(n.0, 1)
  let c.4: tobj = con List #1(#0, acc.1)
  tail call build(m.3, c.4)
b2:
  decref acc.1
  return ()
}}
"
    )
}

/// 確保量の上限で、次の安全点で止まる。コード生成が作る定数の塊と、実行のヒープの最初のページも同じ `Control` に
/// 数えるので、上限はその分より大きくし、小さいループは同じ上限の中で終わることも確かめる
/// (docs/spec/object-model.md の「実行の制御」)。
#[test]
fn a_growing_heap_hits_the_memory_limit() {
    for debug_heap in [true, false] {
        let run = |n: u32| {
            let control = Arc::new(Control::new(Limits {
                max_bytes: 4 * 0x1_0000,
                ..Limits::default()
            }));
            let (host, _) = eml_vm::capture_host(Default::default());
            eml_vm::execute_with(&parse(&cons_loop(n)), debug_heap, host, control)
        };
        assert!(run(100).is_ok(), "debug_heap = {debug_heap}");
        assert_eq!(
            run(1_000_000).unwrap_err().to_string(),
            "the memory limit is exceeded in `build`",
            "debug_heap = {debug_heap}"
        );
    }
}
```

```bash
cargo test -p eml_vm --test integration calls::a_growing_heap_hits_the_memory_limit
```

Expected: passes. The VM already stops on `MEMORY`, so this test pins existing behaviour; there is no red phase for it. To see that it checks something, temporarily change `max_bytes: 4 * 0x1_0000` to `max_bytes: 2 * 0x1_0000` and rerun. Expected: FAIL at `assert!(run(100).is_ok(), …)`, because the constants region and the execution heap's first page already exceed two pages. Then restore `4 * 0x1_0000`.

- [ ] **Step 6: vm.md**

In `docs/spec/vm.md` 「スロットとスタック」, the bullet at line 52 ends with 「…そのときは Miri のテストで覆う ([テスト戦略](../implementation/testing.md) の「Miri」)」 and no period, like the other bullets. Append the following text to the end of that same line. It starts with 「。」, which closes the previous sentence, and the bullet stays one item:

```markdown
。`Vec` の容量は `Task::reserve` が倍に伸ばすが、`min(2 × 今の容量, max_depth + u16::MAX + 2)` 語で止める (`reserve_exact`)。深さが `max_depth` を超えると次の安全点で止まり、安全点の前に積むのは1つの枠と見出し (`u16::MAX + 2` 語) までなので、それより大きい容量は要らない。余る引数の適用は `ApplyRest` の枠と呼ばれる側の枠を同じ安全点の前に積むので、求める長さが上限を超えうる。そのときは容量をその長さちょうどにする
```

In 「`RunStats`」, replace the sentence (line 185)

```markdown
VM の `RunStats` は、`eml_rt` の数え方に従い、検査モードだけで数える。欄は `eml_interp::RunStats` と同じである。
```

with

```markdown
VM の `RunStats` は、`eml_rt` の数え方に従い、検査モードだけで数える。欄は `eml_interp::RunStats` の欄に `peak_stack` を足したものである。
```

and add a row after the `boxes`、`unboxes` row (line 192):

```markdown
| `peak_stack` | 実行の終わりのタスクのスタックの長さ (`Task::stack.len()`)。長さは縮めないので、同時に使った語の数の最大になる。VM のフレームはスタックにあって `peak_objects` に表れないので、末尾呼び出しを失ったことはこの数で見つける |
```

- [ ] **Step 7: Full gate**

```bash
cargo fmt
cargo test
cargo clippy --all-targets
cargo fmt --check
nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_vm --test integration miri::
nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance -Zmiri-tree-borrows" cargo miri test -p eml_vm --test integration miri::
```

Expected: all pass. The UI tests stay green: `assert_same_counts` in `crates/eml_cli/tests/ui.rs` compares six named fields and ignores `peak_stack`. The citations test passes (「スロットとスタック」 and 「実行の制御」 exist). Miri reports 4 passed for both settings.

- [ ] **Step 8: Commit**

```bash
git add crates/eml_vm/src/vm.rs crates/eml_vm/src/lib.rs crates/eml_vm/tests/bench.rs crates/eml_vm/tests/calls.rs docs/spec/vm.md
git commit -m "Report peak_stack, cap the VM stack's capacity near max_depth, and test the memory limit on the VM" -m "RunStats gains peak_stack, the task stack's length at the end, which never shrinks and so is the peak number of words in use; frames on the VM stack do not show in peak_objects. Task::reserve doubles the capacity but stops at max_depth + u16::MAX + 2 words (reserve_exact), and still reaches any longer requested length. Test changes: crates/eml_vm/tests/bench.rs snapshots gain the peak_stack line only (kind 2, expected-value change); new tests a_growing_heap_hits_the_memory_limit (calls.rs) and the vm.rs unit tests the_stack_limit_is_the_depth_limit_and_one_frame, reserve_doubles_the_capacity_up_to_the_stack_limit, reserve_beyond_the_stack_limit_takes_exactly_the_length." -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01MK52fNp5uA711Q2D4kWEtR"
```

### Task 6: The last CEK-vs-VM comparison and the `tree` decrement

Spec: 「作業の順」 step 3 and 「S10a の残りの整理」 item 13. No source or test changes are committed in this task; only `docs/implementation/benchmarks.md` changes. Any instrumentation below is temporary and must be gone (`git status --short` shows only `docs/implementation/benchmarks.md`) before the commit.

**Files:**
- Modify: `docs/implementation/benchmarks.md` (append to the 「### S10b」 section that Task 2 created: the `bench/run.sh` output of the CEK and the VM, the `RunStats` comparison, and the result of the `tree` investigation)
- Test: none committed (temporary probe `crates/eml_vm/tests/probe.rs`, deleted before the commit)

**Interfaces:**
- Consumes: `eml_test_support::{run_stats(text) -> (String, Result<eml_interp::RunStats, eml_interp::RuntimeError>), run_stats_vm(text) -> (String, Result<eml_vm::RunStats, eml_vm::VmError>), lir(text) -> eml_lir::Program}`, `eml_lir::pretty(&Program) -> String`, `bench/run.sh` as of S10a (still compares `eml run` and `eml run --vm`), `eml_vm::RunStats::peak_stack` (Task 5).
- Produces: the 「S10b」 record before the switch (Task 13 appends the after-switch numbers to the same section).

Known state before this task (from `docs/implementation/benchmarks.md` 「S10a (VM)」 and the two `bench.rs` snapshots): the six work counts are equal on both engines; `rc_decrements` is equal for all six programs; the differences are `peak_objects` (fib 26/2, list 100002/100001, state 7/4, tree 14935/14900) and `tree`'s `rc_increments` (14899/14898, the `" "` literal, reason 2). For `tree`'s `rc_decrements` (266465 on both), S10a found that a literal-free `main` gives CEK 266463 and VM 266464, so the VM has one decrement it could not place.

- [ ] **Step 1: Check that the tree is at the state Task 5 left**

```bash
git status --short          # expected: empty
grep -n "pub peak_stack" crates/eml_vm/src/lib.rs   # expected: one line (Task 5)
grep -n "^### S10b" docs/implementation/benchmarks.md  # expected: one line (Task 2)
```

- [ ] **Step 2: Measure both engines with the current `bench/run.sh`**

Run inside the devShell (direnv, or prefix with `nix develop -c`), because the script needs bash 4.3+ and the fenix toolchain. It builds `eml_cli` in release mode and runs each program 3 times on `eml run` (CEK) and `eml run --vm` (VM fast mode).

```bash
nix develop -c bench/run.sh | tee "${TMPDIR:-/tmp}/s10b-before.md"
```

Expected: the environment lines (機種, OS, rustc, コミット without `-dirty`), the CEK table, the VM table and the 「VM / CEK」 table, all six programs present, and exit status 0. If the commit line ends in `-dirty`, stop: commit or stash first, the record must name a clean commit.

Sanity check before recording (not a gate, but report it in the review): every `VM / CEK` value should be at or below the S10a value (fib 0.102, list 0.171, loop 0.130, state 0.151, tree 0.169) give or take 0.01, because Tasks 1-4 only removed work. A larger rise means one of Tasks 1-4 slowed the VM; report it before going on.

- [ ] **Step 3: Collect the `RunStats` differences**

```bash
cargo test -p eml_interp --test integration bench::
cargo test -p eml_vm --test integration bench::
```

Expected: both pass. Then compare the snapshots field by field (`crates/eml_interp/tests/bench.rs` and `crates/eml_vm/tests/bench.rs`, ignoring `peak_stack`, which only the VM has). Expected result: the same differences as the S10a table (above). If any other field differs, the difference was introduced by Tasks 1-5 and must be explained in Step 7's text (or treated as a VM bug as in Step 6b).

- [ ] **Step 4: Reduce the `tree` decrement to one statement (temporary probe)**

Create `crates/eml_vm/tests/probe.rs` (temporary, not committed) and add `mod probe;` at the end of `crates/eml_vm/tests/main.rs`:

```rust
const TREE: &str = include_str!("../../../bench/tree.em");

/// `bench/tree.em` の `main` を `main` に替え、CEK と VM の RC の回数を並べて出す。
fn both(main: &str) {
    let head = TREE.split("main : Unit").next().unwrap();
    both_text(&format!("{head}main : Unit -> <IO> Unit\nmain () =\n{main}\n"));
}

fn both_text(text: &str) {
    let (o1, c) = eml_test_support::run_stats(text);
    let (o2, v) = eml_test_support::run_stats_vm(text);
    let (c, v) = (c.unwrap(), v.unwrap());
    assert_eq!(o1, o2);
    println!(
        "{text:?}\n  CEK inc {} dec {} | VM inc {} dec {}",
        c.rc_increments, c.rc_decrements, v.rc_increments, v.rc_decrements
    );
}

#[test]
fn probe() {
    let t = "  let t = build 15000 42 Leaf\n";
    let fold = "show (fold (fn acc x -> acc + x) 0 t)";
    both(&format!("{t}  println (show (size t))\n  println ({fold})"));
    both(&format!("{t}  println (show (size t) ++ {fold})"));
    both(&format!("{t}  println (show (size t) ++ \" \" ++ {fold})"));
    let main = "main : Unit -> <IO> Unit\nmain () = ";
    both_text(&format!("{main}println (show 12345 ++ show 123)\n"));
    both_text(&format!("{main}println (show 12345 ++ show 1234)\n"));
    println!(
        "{}",
        eml_lir::pretty(&eml_test_support::lir(&format!(
            "{main}println (show 12345 ++ show 1234)\n"
        )))
    );
}
```

```bash
cargo test -p eml_vm --test integration probe -- --nocapture
```

Expected output (measured on 7d1bf441 with a stand-in `peak_stack`; Tasks 1-5 must not change it):

| `main` | CEK inc / dec | VM inc / dec |
|---|---|---|
| two `println`s, no `++` | 14898 / 266463 | 14898 / 266463 |
| `show (size t) ++ show (fold …)` | 14898 / 266463 | 14898 / 266464 |
| `show (size t) ++ " " ++ show (fold …)` (the bench) | 14899 / 266465 | 14898 / 266465 |
| `show 12345 ++ show 123` (8 bytes) | 0 / 2 | 0 / 2 |
| `show 12345 ++ show 1234` (9 bytes) | 0 / 2 | 0 / 3 |

and the LIR of the last program:

```text
fn main(p.0: unit) -> unit {
  let t.1: obj = extern "Prelude.Show Int.show"(12345)
  let t.2: obj = extern "Prelude.Show Int.show"(1234)
  let t.3: obj = extern Prelude.++(t.1, t.2)
  let t.4: unit = extern Prelude.println(t.3)
  return t.4
}
```

So the extra VM decrement is the `extern Prelude.++` whose unique left does not have room for the right side. `docs/spec/object-model.md` 「文字列の操作」 says `string_push` then allocates `max(必要量, 2 × 容量)`, copies, and frees the left box with `free_block`; `eml_rt` counts that `free_block` as one `rc_decrements` (reason 4 in `docs/spec/vm.md` 「`RunStats`」). The CEK's `eml_runtime::Heap::append_str` grows the Rust `String` in place and writes no count. In the bench `main`, `" " ++ show (fold …)` has an immortal left, so it builds a fresh string; the outer `"14898" ++ " 7454478318"` has a unique 5-byte left and needs a rebuild (+1 on the VM), while the CEK counts one `decref` of the `" "` literal that the VM does not count (reason 2). The two cancel.

If the table does not come out as above (Tasks 1-5 changed something), locate the site with per-function counts instead (temporary patch, both engines, not committed):

- VM, `crates/eml_vm/src/vm.rs` `run`: around `step(code, ctx, task, counts, pc)`, read `ctx.heap().stats().rc_decrements` before and after, and add the difference to a `std::collections::BTreeMap<String, u64>` keyed by `code.function_name(pc)`; `eprintln!("{map:?}")` before each `return`.
- CEK, `crates/eml_interp/src/machine.rs` `Machine::run`: around `self.step()`, read `self.rt.heap.rc_decrements()` before and after, and key by `self.program.function(self.function).name.clone()` read *before* the step.
- Run the reduced program on both (`cargo test -p eml_vm --test integration probe -- --nocapture`), find the function whose totals differ, and print its LIR with `eml_lir::pretty` to find the statement. Then apply Step 6a or 6b.

- [ ] **Step 5: Remove the probe and any instrumentation**

```bash
rm crates/eml_vm/tests/probe.rs
sed -i '/^mod probe;$/d' crates/eml_vm/tests/main.rs
git checkout -- crates/eml_vm/src/vm.rs crates/eml_interp/src/machine.rs
git status --short   # expected: empty
```

- [ ] **Step 6a (expected outcome): a counting difference**

The rebuild is the specified behaviour (`object-model.md` 「文字列の操作」), so nothing in the VM changes. Go to Step 7.

- [ ] **Step 6b (only if Step 4 shows a decrement the spec does not explain): a VM bug**

Stop the documentation work. Write a failing test first in the `eml_vm` file that owns the instruction (`crates/eml_vm/tests/run.rs` for externs and calls, `data.rs` for `release` / `unpack`, `calls.rs` for `apply`), as hand-written LIR run through `crate::common::run_lir` plus `eml_test_support::execute_vm(&program, true)` for the counts, asserting the decrement count derived by hand from the LIR (one per `decref`, one per `release` of the box, one per freed child, one per `free_block`). Run it and see it fail on the extra decrement, fix `crates/eml_vm/src/`, see it pass, re-run `cargo test -p eml_vm --test integration bench::` (the `tree` snapshot changes: an expected-value change, kind 2, to name in that commit) and the Miri gate. Commit the fix separately (`Fix the VM's extra decrement in <site>`) before Step 7, and in Step 7 write the fix instead of the explanation below.

- [ ] **Step 7: Append the record to 「### S10b」 in `docs/implementation/benchmarks.md`**

Append after whatever Task 2 wrote in the section. No new heading (Task 13 appends the after-switch record to the same section). First the paragraph, then the `bench/run.sh` output from `${TMPDIR:-/tmp}/s10b-before.md` pasted verbatim (environment lines and the three tables), then the paragraphs that follow it:

```markdown
切り替えの前の記録である。VM の整理と `peak_stack` を入れた後、CEK を消す前に、同じコミットの CEK と VM を `bench/run.sh` で測った。

<the output of bench/run.sh, verbatim>

`empty` を除くどのプログラムでも、VM の実行の分は CEK の約 <min>〜<max> 倍である。
```

Fill `<min>` and `<max>` with the smallest and largest `VM / CEK` value of the third table, rounded to 2 places (S10a wrote 「約 0.10〜0.17 倍」). Then:

```markdown
`RunStats` は、`crates/eml_vm/tests/bench.rs` (VM の検査モード) と `crates/eml_interp/tests/bench.rs` (CEK) のスナップショットで比べた。`peak_stack` は VM だけが持つ。ほかの欄の違いは「S10a (VM)」の表と同じである。

`tree` の `rc_decrements` が等しい理由を確かめた。「S10a (VM)」で原因の分からなかった VM の1回は、`++` で左の文字列を作り直したときの `free_block` である。`main` は `show (size t) ++ (" " ++ show (fold …))` を作る。内側の `++` は左が凍結した領域のリテラルなので、両辺を写した新しい文字列を作る。外側の `++` の左 `"14898"` は一意だが、右の 11 バイトを足す容量がない。そのため `string_push` は大きい文字列を確保して写し、左の箱を `free_block` で空ける ([オブジェクトモデル](../spec/object-model.md) の「文字列の操作」)。`eml_rt` はこの `free_block` を `rc_decrements` に1回数える (「S10a (VM)」の4つ目の理由)。CEK の `append_str` は Rust の `String` をその場で伸ばし、数を書かない。VM の誤りではなく、数え方の違いである。CEK の側で1回多いのは、「S10a (VM)」のとおり、リテラル `" "` の `decref` である (2つ目の理由)。

小さいプログラムでも確かめた。`println (show 12345 ++ show 1234)` は結果が 9 バイトで、左の文字列の容量に収まらない。`rc_decrements` は CEK が 2、VM が 3 である。結果が 8 バイトの `println (show 12345 ++ show 123)` はその場で足せるので、どちらも 2 になる。
```

If Step 3 found other differing fields, replace 「ほかの欄の違いは「S10a (VM)」の表と同じである。」 with the S10a-style table of the differing values and one sentence per new difference naming its reason (1-4 of 「S10a (VM)」, or the Task that caused it). If Step 6b applied, replace the two `tree` paragraphs with one paragraph naming the site, the fix commit, and the new `tree` counts.

- [ ] **Step 8: Gate**

```bash
cargo test -p eml_cli --test integration citations   # the new 「…」 citations resolve
cargo test
cargo clippy --all-targets   # 0 warnings
cargo fmt --check
git status --short           # expected: only docs/implementation/benchmarks.md
```

- [ ] **Step 9: Commit**

```bash
git add docs/implementation/benchmarks.md
git commit -m "Record the last CEK and VM comparison before the switch" -m "Measures bench/ on both engines with the current bench/run.sh and records it under S10b, with the RunStats differences. The one tree decrement S10a could not place is the free_block of a unique left string that ++ rebuilds for lack of room; eml_rt counts it and the CEK's in-place append does not, so it is a counting difference, not a VM bug. No test changes." -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01MK52fNp5uA711Q2D4kWEtR"
```

### Task 7a: Move `scaling.rs` to `eml_vm`

Spec: 「作業の順」 step 4, 「`scaling.rs`」, and 「LIR の `saved` と R7」 (the new `a_retained_owner_does_not_cross_a_call`, which must pass on the current Perceus because rule 2's resolve branch is still there). The 27 CEK tests move under the same names except the renames the spec lists; the 4 source-level tests of `crates/eml_interp/tests/run.rs` that the spec moves to `scaling.rs` come along. The CEK `run.rs` keeps its copies of those 4 until Task 7c deletes the file.

**Files:**
- Create: `crates/eml_vm/tests/scaling.rs` (32 tests: 27 moved, 4 moved from `eml_interp/tests/run.rs`, 1 new)
- Modify: `crates/eml_vm/tests/main.rs` (add `mod scaling;` between `mod run;` and `mod tags;`)
- Delete: `crates/eml_interp/tests/scaling.rs`
- Modify: `crates/eml_interp/tests/main.rs` (remove `mod scaling;`)
- Test: `crates/eml_vm/tests/scaling.rs`

**Interfaces:**
- Consumes: `eml_test_support::{run_stats_vm(text: &str) -> (String, Result<eml_vm::RunStats, eml_vm::VmError>), run_vm(text: &str) -> (String, Result<(), eml_vm::VmError>), execute_vm(program: &eml_lir::Program, debug_heap: bool) -> (String, Result<eml_vm::RunStats, eml_vm::VmError>), lir(text: &str) -> eml_lir::Program}`; `eml_vm::RunStats { …, peak_objects, peak_stack }` (Task 5).
- Produces: `crates/eml_vm/tests/scaling.rs`. Task 8 must rewrite its one `execute_vm(&program, true)` call (in `long_sequence_of_if_statements_does_not_overflow_the_stack`) to the `&RunConfig` form, and Task 10 renames `run_stats_vm` / `run_vm` / `execute_vm` here to `run_stats` / `run` / `execute`.

What changes from the CEK file, test by test (all other test bodies are byte-identical apart from `run_stats` -> `run_stats_vm`):

| Test | Change | Kind |
|---|---|---|
| the 7 `…_keeps_the_heap_flat` | renamed `…_keeps_the_stack_and_heap_flat`; `assert_eq!(short, long)` on `peak_objects` becomes `assert_flat`, which also requires equal `peak_stack` | 3 (move, rename) + 2 (new check) |
| `a_tail_apply_from_lambda_to_lambda_grows_the_heap_only_by_its_closures` | adds `peak_stack` equal at both n; comment rewritten (the CEK reasoning about frames reusing freed slots does not apply to stack frames) | 2 |
| `derived_comparisons_walk_a_long_list_without_growing_the_heap` | renamed `…_without_growing_the_stack_or_heap`; adds `peak_stack <= baseline + 16` | 3 + 2 |
| `immediate_operations_under_deep_recursion_keep_the_peak_linear` | `peak_stack` is measured at n = 1000, 2000, 3000 and must grow by the same multiple of the n step (constant words per level, > 0); `peak_objects` must stay within `short + 16` (VM frames are not heap objects; a yielding clause would add closures per level) | 2 |
| `a_resuming_operation_builds_a_frame_per_level` | keeps `yield_frames >= shallow + 990`; adds `peak_objects >= shallow + 990` (the continuation is heap closures per level on the VM) | 2 |
| `a_program_without_operations_or_strings_does_no_counted_work` | `peak_objects` stays 1, but the reason is the heap's dummy object, not `Frame::Root` (comment only) | 3 |
| `evaluating_a_literal_copies_nothing`, `appending_to_a_unique_string_copies_only_the_right_side` | use the `stats` helper instead of calling `run_stats` directly; comments cite `object-model.md` 「凍結した領域」 instead of `runtime.md` (deleted in Task 14) | 3 |
| `immediate_operations_cost_the_same_at_any_depth`, `a_resuming_operation_builds_a_frame_per_level` | cite `docs/spec/vm.md の「`RunStats`」` instead of `runtime.md の「実行の API」` | 3 |
| `long_statement_sequence_does_not_overflow_the_stack`, `long_sequence_of_if_statements_does_not_overflow_the_stack`, `a_match_with_a_thousand_literals_runs_in_a_debug_build`, `a_match_with_a_thousand_shared_arms_runs_in_a_debug_build` | moved from `eml_interp/tests/run.rs`; run on the VM (`run_vm`, `execute_vm(&program, true)`) | new tests here (the spec lists the table's moved tests among the added ones) |
| `a_retained_owner_does_not_cross_a_call` | new | new test |

- [ ] **Step 1: Check Task 5 is in**

```bash
grep -n "pub peak_stack" crates/eml_vm/src/lib.rs   # expected: one line
```

- [ ] **Step 2: Create `crates/eml_vm/tests/scaling.rs`**

```rust
//! 実行の仕事の回数と、同時に使ったヒープの物体とスタックの語の数の最大 (`RunStats`) を比べるテスト。
//! 時間ではなく数を比べるので、`#[ignore]` を付けずにふだんの `cargo test` で流す。ソースはテストの中で作る。
//! 数は VM の検査モードで取る (docs/spec/vm.md の「`RunStats`」)。

use std::fmt::Write;

use eml_test_support::{execute_vm, lir, run_stats_vm, run_vm};
use eml_vm::RunStats;

/// 正常に終わった実行の回数。生成したソースが意図どおりに走ったことを、出力でも確かめる。
fn stats(text: &str, expected: &str) -> RunStats {
    let (out, result) = run_stats_vm(text);
    assert_eq!(out, expected);
    result.unwrap_or_else(|error| panic!("{error}"))
}

#[test]
fn a_program_without_operations_or_strings_does_no_counted_work() {
    let stats = stats("main : Unit -> <IO> Unit\nmain () = ()", "");
    assert_eq!(
        (
            stats.evidence_steps,
            stats.yields,
            stats.yield_frames,
            stats.string_bytes_copied,
            stats.rc_increments,
            stats.rc_decrements
        ),
        (0, 0, 0, 0, 0, 0),
        "{stats:?}"
    );
    // `peak_objects` は仕事の回数でなく物体の数の最大なので、ヒープごとのダミーの物体の分だけ 0 にならない
    // (docs/spec/object-model.md の「ダミーの値」)
    assert_eq!(stats.peak_objects, 1, "{stats:?}");
}

/// どの `perform` も、少なくとも見つけた evidence の節点を1つたどる。
#[test]
fn every_operation_looks_up_its_evidence() {
    let text = [
        "effect Ask where",
        "  ask : Unit -> Int",
        "",
        "main : Unit -> <IO> Unit",
        "main () =",
        "  let n = handle ask () + ask () + ask () with",
        "            | ask () k -> k 1",
        "  println (show n)",
    ]
    .join("\n");
    let stats = stats(&text, "3\n");
    assert!(stats.evidence_steps >= 3, "{stats:?}");
}

/// 連結の結果の文字列には、両辺の中身を書く。
#[test]
fn concatenation_writes_the_bytes_of_both_sides() {
    let stats = stats(
        "main : Unit -> <IO> Unit\nmain () = println (\"ab\" ++ \"cd\")",
        "abcd\n",
    );
    assert!(stats.string_bytes_copied >= 4, "{stats:?}");
}

#[test]
fn evaluating_a_literal_copies_nothing() {
    // 文字列のリテラルは凍結した領域の物体で、評価は中身を写さずに参照を1つ作る (docs/spec/object-model.md の
    // 「凍結した領域」)。64 バイトのリテラルを n 回評価して文字列の `switch` で比べるので、1回でも写すと上限を
    // 超える。残る数は `show` の結果の分だけである
    let literal = "x".repeat(64);
    let n = 2000;
    let source = format!(
        "same : String -> Int\n\
         same s = match s with\n  | \"{literal}\" -> 1\n  | _ -> 0\n\n\
         count : Int -> Int -> Int\n\
         count n acc = if n == 0 then acc else count (n - 1) (acc + same \"{literal}\")\n\n\
         main : Unit -> <IO> Unit\n\
         main () = println (show (count {n} 0))\n"
    );
    let stats = stats(&source, &format!("{n}\n"));
    assert!(
        stats.string_bytes_copied < literal.len() as u64,
        "{stats:?}"
    );
}

/// リテラルから始めて、一意な左辺に `"x"` を n 回つなぐプログラム。`grow` の `acc` は各枝で1回だけ使うので、
/// Perceus は `dup` を入れず、2回目からの `++` には RC が 1 の左辺が届く。
fn growing_string(n: usize) -> String {
    format!(
        "grow : Int -> String -> String\n\
         grow n acc = if n == 0 then acc else grow (n - 1) (acc ++ \"x\")\n\
         \n\
         main : Unit -> <IO> Unit\n\
         main () = println (grow {n} \"s\")\n"
    )
}

#[test]
fn appending_to_a_unique_string_copies_only_the_right_side() {
    // 最初の `++` だけが凍結した領域のリテラル "s" を写し、残りは一意な左辺の後に 1 バイトずつ足す。左辺を毎回
    // 写すと、2 + 3 + … + (n + 1) バイトになる
    let n = 2000;
    let stats = stats(&growing_string(n), &format!("s{}\n", "x".repeat(n)));
    assert!(stats.string_bytes_copied <= 4 * n as u64, "{stats:?}");
}

/// handler の下の非末尾の再帰。深さごとに1回 `ask` するので、`count n` は n 回 `ask` する。evidence は `evv` の
/// 先頭から探すので、1回の `ask` がたどる節点の数は今有効な handler の数で決まり、深さによらない
/// (docs/spec/lir.md の「evidence を探す」)。
const ASK_RECURSION: &str = "effect Ask where
  ask : Unit -> Int

count : Int -> <Ask> Int
count n = if n == 0 then 0 else ask () + count (n - 1)
";

/// `ASK_RECURSION` の後に `rest` (`main` とその前の宣言) を置いて実行し、`evidence_steps` を返す。どの形でも
/// `ask` は 1 を返す handler に届くので、出力は n である。
fn evidence_steps(n: u64, rest: &str) -> u64 {
    stats(&format!("{ASK_RECURSION}\n{rest}"), &format!("{n}\n")).evidence_steps
}

#[test]
fn a_perform_under_one_handler_looks_at_one_evidence() {
    // `evv` は `Ask` の evidence だけなので、`ask` ごとに1つたどる
    let n = 2000;
    let rest = format!(
        "main : Unit -> <IO> Unit
main () =
  let r = handle count {n} with
            | ask () k -> k 1
  println (show r)
"
    );
    let steps = evidence_steps(n, &rest);
    assert!(steps <= n, "evidence_steps = {steps}");
}

#[test]
fn a_perform_passes_an_inner_evidence_of_another_effect_once() {
    // `evv` は `Tell` と `Ask` の evidence なので、`ask` ごとに2つたどる
    let n = 2000;
    let rest = format!(
        "effect Tell where
  tell : Int -> Unit

main : Unit -> <IO> Unit
main () =
  let r = handle (handle count {n} with
                    | tell _ k -> k ()) with
            | ask () k -> k 1
  println (show r)
"
    );
    let steps = evidence_steps(n, &rest);
    assert!(steps <= 2 * n, "evidence_steps = {steps}");
}

#[test]
fn a_perform_in_a_masked_callback_skips_the_inner_evidence_once() {
    // tests/ui/run/effects/mask_callback.em と同じ形。`run` は `cb` を `mask` 付きで呼ぶので、`count` の `ask` は
    // 内側の `Ask` の evidence を飛ばして外側に届く (内側に届くと出力は 2n になる)。`mask` は内側を除いた `evv` を
    // 1回だけ写し、各 `ask` はその先頭で外側の evidence を見つける。`ask` のたびに内側を飛ばす形なら 2n ほどになり、
    // 上限を超える
    let n = 2000;
    let rest = format!(
        "run : (Unit -> <e> a) -> <Ask | e> a
run cb = cb ()

go : Unit -> <Ask> Int
go () = count {n}

main : Unit -> <IO> Unit
main () =
  let r = handle (handle run go with
                    | ask () k -> k 2) with
            | ask () k -> k 1
  println (show r)
"
    );
    let steps = evidence_steps(n, &rest);
    assert!(steps <= n + 8, "evidence_steps = {steps}");
}

/// すぐに再開する操作は、その場で節を呼ぶので、yield せず、継続も組み立てない。探す数は今有効な handler の数で
/// 決まり、深さによらない (docs/spec/vm.md の「`RunStats`」)。
#[test]
fn immediate_operations_cost_the_same_at_any_depth() {
    let at = |depth: u64| {
        let text = format!(
            "effect Counter where
  get : Unit -> Int
  put : Int -> Unit

down : Int -> <Counter> Int
down n = if n == 0 then
    put (get () + 1)
    put (get () + 1)
    get ()
  else down (n - 1) + 0

main : Unit -> <IO> Unit
main () =
  let (r, _) =
    handle down {depth} from 0 with
      | get () k st -> k st st
      | put n k _ -> k () n
      | return x st -> (x, st)
  println (show r)
"
        );
        stats(&text, "2\n")
    };
    let (shallow, deep) = (at(10), at(1000));
    assert_eq!(shallow.evidence_steps, deep.evidence_steps);
    assert_eq!(
        (shallow.yields, shallow.yield_frames),
        (0, 0),
        "{shallow:?}"
    );
    assert_eq!((deep.yields, deep.yield_frames), (0, 0), "{deep:?}");
}

/// すぐに再開しない操作は handler まで yield し、戻る途中の各段が自分の続きを継続に足す。VM のフレームはスタックに
/// あるので、継続に足した続きはヒープのクロージャになり、`peak_objects` が段の数に比例して伸びる。これは方式の性質で
/// あり、上限ではない (docs/spec/vm.md の「`RunStats`」)。
#[test]
fn a_resuming_operation_builds_a_frame_per_level() {
    let at = |depth: u64| {
        let text = format!(
            "effect Ask where
  ask : Unit -> Int

down : Int -> <Ask> Int
down n = if n == 0 then ask () else down (n - 1) + 0

main : Unit -> <IO> Unit
main () =
  let r =
    handle down {depth} with
      | ask () k -> k 1 + 0
  println (show r)
"
        );
        stats(&text, "1\n")
    };
    let (shallow, deep) = (at(10), at(1000));
    assert!(
        deep.yield_frames >= shallow.yield_frames + 990,
        "{shallow:?} {deep:?}"
    );
    assert!(
        deep.peak_objects >= shallow.peak_objects + 990,
        "{shallow:?} {deep:?}"
    );
}

/// リストの関数と、本体が `traverse` の `main` を並べたプログラム。`traverse` の中の `{n}` を n に置き換える。
fn list_traversal(n: u64, traverse: &str) -> String {
    format!(
        "data List a =
  | Nil
  | Cons a (List a)

range : Int -> Int -> List Int
range lo hi = if lo > hi then Nil else Cons lo (range (lo + 1) hi)

sum : List Int -> Int
sum xs = match xs with
  | Nil -> 0
  | Cons x rest -> x + sum rest

length : List a -> Int
length xs = match xs with
  | Nil -> 0
  | Cons _ rest -> 1 + length rest

main : Unit -> <IO> Unit
main () =
{traverse}
"
    )
    .replace("{n}", &n.to_string())
}

/// `list_traversal` を実行して `rc_increments` を返す。
fn rc_increments(n: u64, traverse: &str, expected: &str) -> u64 {
    stats(&list_traversal(n, traverse), expected).rc_increments
}

#[test]
fn traversing_a_unique_list_does_not_dup_per_cell() {
    // 一意なリストのセルは、`release` が箱だけを解放し、フィールドが箱の参照をそのまま受け取るので、セルごとの
    // `dup` は要らない。長さを2倍にしても `rc_increments` は変わらない。`release` の代わりにフィールドを `dup` して
    // 箱を `decref` する形に戻ると落ちる
    let traverse = "  println (show (sum (range 1 {n})))";
    let n = 1000;
    let short = rc_increments(n, traverse, &format!("{}\n", n * (n + 1) / 2));
    let long = rc_increments(2 * n, traverse, &format!("{}\n", n * (2 * n + 1)));
    assert_eq!(short, long);
}

#[test]
fn traversing_a_shared_list_dups_each_cell_at_most_once() {
    // `sum` の後で `length` が同じリストを使うので、`sum` がたどるセルは共有されている。`sum` に渡す前に `xs` を
    // 1回、`sum` が各セルで残りのリストを1回 `dup` するので、数は n になる。上限は n に小さな余裕を足したもので、
    // セルを2回以上 `dup` する形になれば超える
    let traverse =
        "  let xs = range 1 {n}\n  println (show (sum xs))\n  println (show (length xs))";
    let n = 1000;
    let increments = rc_increments(n, traverse, &format!("{}\n{n}\n", n * (n + 1) / 2));
    assert!(increments <= n + 2, "rc_increments = {increments}");
}

/// `peaks` が実行する2つの n。上限を n の差で書くテストは `PEAK_LONG - PEAK_SHORT` を使う。
const PEAK_SHORT: u64 = 1000;
const PEAK_LONG: u64 = 2 * PEAK_SHORT;

/// n = `PEAK_SHORT` と n = `PEAK_LONG` で `program` を実行し、`RunStats` の組を返す。`program` は n からソースと
/// 期待する出力を作る。末尾呼び出しを失った形では、反復の数に比例してフレームが残るので、n を2倍にすると
/// `peak_stack` も増える。
fn peaks(program: impl Fn(u64) -> (String, String)) -> (RunStats, RunStats) {
    let run = |n| {
        let (source, expected) = program(n);
        stats(&source, &expected)
    };
    (run(PEAK_SHORT), run(PEAK_LONG))
}

/// 反復の数を2倍にしても、スタックの語の数の最大もヒープの物体の数の最大も変わらないことを確かめる。末尾呼び出しを
/// 失えばスタックが、反復ごとに物体を残せばヒープが伸びる。
fn assert_flat((short, long): (RunStats, RunStats)) {
    assert_eq!(
        (short.peak_stack, short.peak_objects),
        (long.peak_stack, long.peak_objects),
        "{short:?} {long:?}"
    );
}

#[test]
fn a_loop_through_a_function_value_that_returns_int_keeps_the_stack_and_heap_flat() {
    assert_flat(peaks(|n| {
        (
            format!(
                "loop : (Int -> Int) -> Int -> Int
loop f n = if n == 0 then 0 else f (n - 1)

go : Int -> Int
go n = loop go n

main : Unit -> <IO> Unit
main () = println (show (go {n}))
"
            ),
            "0\n".to_string(),
        )
    }));
}

#[test]
fn a_loop_through_a_function_value_that_returns_bool_keeps_the_stack_and_heap_flat() {
    // n が偶数なので、`is_odd n` は偽になる
    assert_flat(peaks(|n| {
        (
            format!(
                "is_even : (Int -> Bool) -> Int -> Bool
is_even odd n = if n == 0 then True else odd (n - 1)

is_odd : Int -> Bool
is_odd n = if n == 0 then False else is_even is_odd (n - 1)

main : Unit -> <IO> Unit
main () = if is_odd {n} then println \"odd\" else println \"even\"
"
            ),
            "even\n".to_string(),
        )
    }));
}

#[test]
fn a_loop_through_a_function_value_that_returns_unit_keeps_the_stack_and_heap_flat() {
    assert_flat(peaks(|n| {
        (
            format!(
                "tick : (Int -> <IO> Unit) -> Int -> <IO> Unit
tick f n = if n == 0 then println \"done\" else f (n - 1)

run : Int -> <IO> Unit
run n = tick run n

main : Unit -> <IO> Unit
main () = run {n}
"
            ),
            "done\n".to_string(),
        )
    }));
}

#[test]
fn a_loop_with_an_unused_let_between_the_call_and_its_result_keeps_the_stack_and_heap_flat() {
    // translate は `let s` を呼び出しと `return r` の間に置く。使われない `let` を飛ばして末尾の位置を見なければ、
    // 反復ごとにフレームが残る
    assert_flat(peaks(|n| {
        (
            format!(
                "loop : (Int -> Int) -> Int -> Int
loop f n = if n == 0 then 0 else let r = f (n - 1) in let s = \"unused\" in r

go : Int -> Int
go n = loop go n

main : Unit -> <IO> Unit
main () = println (show (go {n}))
"
            ),
            "0\n".to_string(),
        )
    }));
}

#[test]
fn a_loop_whose_clause_resumes_in_tail_position_keeps_the_stack_and_heap_flat() {
    assert_flat(peaks(|n| {
        (
            format!(
                "effect Ask where
  ask : Unit -> Int

sum_asks : Int -> Int -> <Ask> Int
sum_asks n acc = if n == 0 then acc else sum_asks (n - 1) (acc + ask ())

main : Unit -> <IO> Unit
main () =
  let r = handle sum_asks {n} 0 with
            | ask () k -> k 2
  println (show r)
"
            ),
            format!("{}\n", 2 * n),
        )
    }));
}

/// 深さ n の非末尾の再帰の各段で、すぐに再開する操作を行う。節はその場で走るので、段ごとに残るのはスタックの
/// フレームだけである。`peak_stack` は n の差に比例して伸び、1段あたりの語の数は n によらない。ヒープの物体は
/// 段の数で増えない。節が yield すると、戻る途中の各段が継続のクロージャを足すので、`peak_objects` が段の数に
/// 比例して増える。
#[test]
fn immediate_operations_under_deep_recursion_keep_the_peak_linear() {
    let run = |n: u64| {
        stats(
            &format!(
                "{ASK_RECURSION}
main : Unit -> <IO> Unit
main () =
  let r = handle count {n} with
            | ask () k -> k 1
  println (show r)
"
            ),
            &format!("{n}\n"),
        )
    };
    let (short, long, longer) = (
        run(PEAK_SHORT),
        run(PEAK_LONG),
        run(PEAK_LONG + (PEAK_LONG - PEAK_SHORT)),
    );
    let per_level = (long.peak_stack - short.peak_stack) / (PEAK_LONG - PEAK_SHORT);
    assert!(per_level > 0, "{short:?} {long:?}");
    assert_eq!(
        (
            long.peak_stack - short.peak_stack,
            longer.peak_stack - long.peak_stack
        ),
        (
            per_level * (PEAK_LONG - PEAK_SHORT),
            per_level * (PEAK_LONG - PEAK_SHORT)
        ),
        "{short:?} {long:?} {longer:?}"
    );
    assert!(
        long.peak_objects <= short.peak_objects + 16,
        "{short:?} {long:?}"
    );
}

/// yield しうる関数の値を通るループ。関数の値への末尾の `apply` は yield の地点にならず、末尾呼び出しのまま残る
/// (docs/spec/core-ir.md の「yield の地点」)。
#[test]
fn a_function_value_loop_under_a_handler_keeps_the_stack_and_heap_flat() {
    assert_flat(peaks(|n| {
        (
            format!(
                "effect Ask where
  ask : Unit -> Int

loop : (Int -> <Ask> Int) -> Int -> <Ask> Int
loop f n = if n == 0 then ask () else f (n - 1)

go : Int -> <Ask> Int
go n = loop go n

main : Unit -> <IO> Unit
main () =
  let r = handle go {n} with
            | ask () k -> k 0
  println (show r)
"
            ),
            "0\n".to_string(),
        )
    }));
}

#[test]
fn a_direct_self_tail_call_keeps_the_stack_and_heap_flat() {
    assert_flat(peaks(|n| {
        (
            format!(
                "loop : Int -> Int -> Int
loop n acc = if n == 0 then acc else loop (n - 1) (acc + 1)

main : Unit -> <IO> Unit
main () = println (show (loop {n} 0))
"
            ),
            format!("{n}\n"),
        )
    }));
}

#[test]
fn a_tail_apply_from_lambda_to_lambda_grows_the_heap_only_by_its_closures() {
    // 反復ごとにクロージャが1つ生き残るので、物体の数は n に比例して増える。`k` を2回使うのは、戻る間も鎖を共有に
    // しておくためである。末尾の `apply` を失うと、反復ごとにスタックのフレームが1つ増えて `peak_stack` が伸びる
    let (short, long) = peaks(|n| {
        (
            format!(
                "count_down : Int -> (Int -> Int) -> Int
count_down n k = if n == 0 then k 0 + k 0 else count_down (n - 1) (fn m -> k (m + 1))

main : Unit -> <IO> Unit
main () = println (show (count_down {n} (fn m -> m)))
"
            ),
            format!("{}\n", 2 * n),
        )
    });
    assert!(
        long.peak_objects <= short.peak_objects + (PEAK_LONG - PEAK_SHORT),
        "{short:?} {long:?}"
    );
    assert_eq!(short.peak_stack, long.peak_stack, "{short:?} {long:?}");
}

/// 非末尾の再帰の各段で、データ `P` を分解してフィールド `s` だけを呼び出しの後に使う。持ち主の `P` は呼び出しで
/// 要らないので、Perceus は呼び出しの前に `release` で解き、使わない兄弟のリストを空ける。持ち主を呼び出しをまたいで
/// 残すと、段ごとにリストの 50 個のセルが残り、`peak_objects` が n の差の約 50 倍伸びる
/// (docs/spec/lir.md の「所有を取る時点」)。
#[test]
fn a_retained_owner_does_not_cross_a_call() {
    let (short, long) = peaks(|n| {
        (
            format!(
                "data P =
  | P (List Int) String

range : Int -> List Int
range k = if k == 0 then [] else k :: range (k - 1)

make : Int -> P
make n = P (range 50) (show n)

walk : Int -> String
walk n = if n == 0 then \"\" else match make n with
  | P _ s ->
    let r = walk (n - 1)
    r ++ s

main : Unit -> <IO> Unit
main () = println (walk {n})
"
            ),
            format!("{}\n", (1..=n).map(|k| k.to_string()).collect::<String>()),
        )
    });
    assert!(
        long.peak_objects <= short.peak_objects + 2 * (PEAK_LONG - PEAK_SHORT),
        "{short:?} {long:?}"
    );
}

/// 型変数のフィールドは `tobj` なので、`Int` を入れると `box`、取り出して `Int` として使うと `unbox` を通る
/// (docs/spec/lir.md の「位置の規則」)。データの配置は単相化の後も一様なので、この形の変換は残る。
#[test]
fn an_int_in_a_type_variable_field_is_boxed_and_unboxed() {
    let text = [
        "data Wrap a =",
        "  | Wrap a",
        "",
        "unwrap : Wrap a -> a",
        "unwrap w = match w with",
        "  | Wrap x -> x",
        "",
        "main : Unit -> <IO> Unit",
        "main () = println (show (unwrap (Wrap 1) + 1))",
    ]
    .join("\n");
    let stats = stats(&text, "2\n");
    assert!(stats.boxes >= 1, "{stats:?}");
    assert!(stats.unboxes >= 1, "{stats:?}");
}

#[test]
fn a_program_with_only_scalar_positions_boxes_nothing() {
    let stats = stats(
        "main : Unit -> <IO> Unit\nmain () = println (show (1 + 2))",
        "3\n",
    );
    assert_eq!((stats.boxes, stats.unboxes), (0, 0), "{stats:?}");
}

/// `range n acc` は `0 :: 1 :: … :: acc` を作る。末尾の再帰で、後ろの要素から積む。
fn list_program(n: u64, body: &str) -> String {
    [
        "range : Int -> List Int -> List Int",
        "range n acc = if n == 0 then acc else range (n - 1) (n - 1 :: acc)",
        "",
        "main : Unit -> <IO> Unit",
        &format!("main () =\n  let xs = range {n} Nil\n  {body}"),
    ]
    .join("\n")
}

/// `show` は左辺を一意な文字列として伸ばすので、写すバイトの数はリストの長さに比例する
/// (docs/spec/declarations.md の「Prelude のクラス」)。出力は長いので確かめない。
#[test]
fn showing_a_list_copies_bytes_in_proportion_to_its_length() {
    let copied = |n: u64| {
        let (_, result) = run_stats_vm(&list_program(n, "println (show xs)"));
        result
            .unwrap_or_else(|error| panic!("{error}"))
            .string_bytes_copied
    };
    let (small, large) = (copied(1000), copied(2000));
    assert!(small >= 1000, "{small}");
    assert!(large as f64 <= small as f64 * 2.5, "{small} {large}");
}

/// 導出した `==` と `compare` は末尾呼び出しで進むので、長いリストを比べても、スタックの語と生きている物体は、
/// リストを作って持つだけのプログラムより定数個しか増えない。
#[test]
fn derived_comparisons_walk_a_long_list_without_growing_the_stack_or_heap() {
    let compared = stats(
        &list_program(
            10000,
            "println (show (xs == xs))\n  println (show (compare xs xs))",
        ),
        "True\nEQ\n",
    );
    let baseline = stats(
        &list_program(10000, "println \"True\"\n  println \"EQ\"\n  drop xs"),
        "True\nEQ\n",
    );
    assert!(
        compared.peak_objects <= baseline.peak_objects + 16,
        "{compared:?} {baseline:?}"
    );
    assert!(
        compared.peak_stack <= baseline.peak_stack + 16,
        "{compared:?} {baseline:?}"
    );
}

/// 式のリストは平らな節点なので、要素が多くてもどの段階のスタックも深くならない。
#[test]
fn a_long_list_literal_runs() {
    let items: Vec<String> = (0..10000).map(|i| (i % 10).to_string()).collect();
    let text = format!(
        "sum : List Int -> Int -> Int\nsum xs acc = match xs with\n  | [] -> acc\n  | x :: rest -> sum rest (acc + x)\n\nmain : Unit -> <IO> Unit\nmain () = println (show (sum [{}] 0))",
        items.join(", ")
    );
    stats(&text, "45000\n");
}

#[test]
fn interpolation_copies_bytes_in_proportion_to_its_length() {
    let copied = |n: usize| {
        let holes = "\\{x}-".repeat(n);
        let program =
            format!("main : Unit -> <IO> Unit\nmain () =\n  let x = 12345\n  println \"{holes}\"");
        stats(&program, &format!("{}\n", "12345-".repeat(n))).string_bytes_copied
    };
    let (small, large) = (copied(500), copied(1000));
    assert!(large as f64 <= small as f64 * 2.5, "{small} {large}");
}

fn main_with(body: &str) -> String {
    format!("main : Unit -> <IO> Unit\nmain () =\n{body}")
}

#[test]
fn long_statement_sequence_does_not_overflow_the_stack() {
    // 逐次の文は入れ子ではないので、後段とコード生成は長い文の列を再帰せずに処理しなければならない
    let mut body = "  let s = \"x\"\n".repeat(5000);
    body.push_str("  println s");
    assert_eq!(run_vm(&main_with(&body)), ("x\n".to_string(), Ok(())));
}

#[test]
fn long_sequence_of_if_statements_does_not_overflow_the_stack() {
    // 文の `if` はそれぞれ `switch` と合流するブロックになり、ブロックの列が文の数だけ長くなる。後段とコード生成は、
    // この列を再帰せずに処理しなければならない。条件は引数にする。`if True` は translate がその場で枝を選び、
    // `switch` を出さないためである
    const COUNT: usize = 5000;
    let mut text = String::from("step : Bool -> <IO> Unit\nstep b =\n");
    text.push_str(&"  if b then println \"x\"\n".repeat(COUNT));
    text.push_str("  println \"done\"\n\nmain : Unit -> <IO> Unit\nmain () = step True");
    let program = lir(&text);
    let step = program
        .functions
        .iter()
        .find(|function| function.name == "step")
        .expect("step");
    let switches = step
        .blocks
        .iter()
        .filter(|block| matches!(block.term, eml_lir::Term::Switch { .. }))
        .count();
    assert_eq!(switches, COUNT);
    let (out, result) = execute_vm(&program, true);
    assert_eq!(out, format!("{}done\n", "x\n".repeat(COUNT)));
    assert_eq!(result.map(|_| ()), Ok(()));
}

#[test]
fn a_match_with_a_thousand_literals_runs_in_a_debug_build() {
    // 平らな `switch` は、リテラルの数だけ入れ子を深くしない (docs/spec/core-ir.md)
    let mut text = String::from("pick : Int -> Int\npick n =\n  match n with\n");
    for i in 0..1000 {
        writeln!(text, "    | {i} -> {}", i + 1).unwrap();
    }
    text.push_str(
        "    | _ -> 0\n\nmain : Unit -> <IO> Unit\nmain () = println (show (pick 999))\n",
    );
    assert_eq!(run_vm(&text), ("1000\n".to_string(), Ok(())));
}

#[test]
fn a_match_with_a_thousand_shared_arms_runs_in_a_debug_build() {
    // `(_, i)` の枝には、`A` の行き先と `default` の2つの葉が向かう。合流するブロックは入れ子にならないので、枝の数
    // だけ後段のスタックを使わない。scrutinee は引数にする。リテラルのタプルを渡すと、translate がその場で枝を選ぶ
    // ためである
    let mut text = String::from(
        "data AB =\n  | A\n  | B\n\npick : AB -> Int -> Int\npick t n = match (t, n) with\n  | (A, 0) -> 0\n",
    );
    for i in 1..=1000 {
        writeln!(text, "  | (_, {i}) -> {i}").unwrap();
    }
    text.push_str(
        "  | _ -> 0\n\nmain : Unit -> <IO> Unit\nmain () = println (show (pick B 999))\n",
    );
    assert_eq!(run_vm(&text), ("999\n".to_string(), Ok(())));
}
```

- [ ] **Step 3: Register the file**

In `crates/eml_vm/tests/main.rs`, the module list becomes:

```rust
mod api;
mod bench;
mod calls;
mod codegen;
mod data;
mod evidence;
mod miri;
mod run;
mod scaling;
mod tags;
```

(keep any module Tasks 1-5 added, in alphabetical order).

- [ ] **Step 4: Run the moved tests on the VM**

```bash
cargo test -p eml_vm --test integration scaling::
```

Expected: `32 passed`. These are ports of passing CEK tests onto an engine that the UI suite already checks against the CEK, plus one new test that passes on the current Perceus, so there is no red phase; a failure here is a VM regression or a wrong relation, and must be debugged (superpowers:systematic-debugging), not loosened.

Expected values (kind 2): the numbers the relations compare are not pinned, only relations. Reference values measured on 7d1bf441 with `peak_stack = task.stack.len()` (Tasks 1-4 may shift the absolute `peak_stack` by a few words per frame; the relations must still hold):

| Test | n = 1000 | n = 2000 | Relation asserted |
|---|---|---|---|
| `a_direct_self_tail_call_…` | stack 14, objects 2 | 14, 2 | equal |
| `a_loop_through_a_function_value_that_returns_int_…` | 14, 2 | 14, 2 | equal |
| `…_returns_bool_…` | 14, 1 | 14, 1 | equal |
| `…_returns_unit_…` | 9, 1 | 9, 1 | equal |
| `a_loop_with_an_unused_let_…` | 14, 2 | 14, 2 | equal |
| `a_loop_whose_clause_resumes_in_tail_position_…` | 27, 4 | 27, 4 | equal |
| `a_function_value_loop_under_a_handler_…` | 21, 4 | 21, 4 | equal |
| `a_tail_apply_from_lambda_to_lambda_…` | stack 20, objects 1001 | 20, 2001 | stack equal, objects grow <= 1000 |
| `immediate_operations_under_deep_recursion_…` | stack 6020, objects 4 (n = 3000: 18020, 4) | 12020, 4 | stack steps equal (6 words per level), objects <= short + 16 |
| `a_retained_owner_does_not_cross_a_call` | objects 1052 | 2052 | objects grow <= 2 × 1000 (the review measured ~51 per level without the resolve branch) |
| `a_resuming_operation_builds_a_frame_per_level` (depth 10 / 1000) | yield_frames 10, objects 15 | 1000, 1005 | both grow >= 990 |
| `derived_comparisons_…` (compared / baseline) | stack 18 / 15, objects 10001 / 10001 | | stack <= baseline + 16, objects <= baseline + 16 |
| `traversing_a_unique_list_does_not_dup_per_cell` | rc_increments 0 | 0 | equal |
| `traversing_a_shared_list_dups_each_cell_at_most_once` | rc_increments 1000 | | <= n + 2 |
| `a_perform_under_one_handler_…` / `…another_effect_once` / `…masked_callback…` | evidence_steps 2000 / 4000 / 2001 | | <= n / 2n / n + 8 |
| `appending_to_a_unique_string_…` | string_bytes_copied 2001 | | <= 4n |
| `showing_a_list_…` | 12667 | 28667 | ratio <= 2.5 |
| `interpolation_…` (500 / 1000 holes) | 5495 | 10995 | ratio <= 2.5 |

To see the actual values when a relation fails, every assertion message prints the `RunStats` (`{short:?} {long:?}`); to see them on a pass, add a temporary `eprintln!("{short:?} {long:?}");` and run with `-- --nocapture`, then remove it. Each relation keeps the CEK test's intent: equal peaks mean no lost tail call and no per-iteration object; linear `peak_stack` with a constant step means the immediate clause adds a fixed frame per level and nothing on the heap; the resuming test shows the continuation is built per level.

- [ ] **Step 5: Delete the CEK file**

```bash
git rm crates/eml_interp/tests/scaling.rs
sed -i '/^mod scaling;$/d' crates/eml_interp/tests/main.rs
grep -n "scaling" crates/eml_interp/tests/main.rs   # expected: no output
cargo test -p eml_interp --test integration          # expected: passes (bench, closures, data, evidence, rt_externs, run)
```

- [ ] **Step 6: Gate**

```bash
cargo test -p eml_cli --test integration citations   # the new 「…」 citations resolve
cargo test
cargo clippy --all-targets   # 0 warnings
cargo fmt --check
```

No `src/` changes, so no Miri or Verus.

- [ ] **Step 7: Commit**

```bash
git add -A crates/eml_vm/tests crates/eml_interp/tests
git commit -m "Move scaling.rs to the VM" -m "Moves the 27 RunStats scaling tests from eml_interp to eml_vm and runs them in checked mode (mechanical, kind 3). The seven tail-call tests and the derived-comparison test are renamed after what they now check, and they, the lambda tail-apply test, the deep immediate-operation test and the resuming-operation test also check peak_stack or peak_objects the VM way (expected-value changes, kind 2): VM frames live on the stack, so lost tail calls show in peak_stack, not peak_objects. Moves the two thousand-arm match tests and the two long-sequence tests from eml_interp's run.rs, and adds a_retained_owner_does_not_cross_a_call, which pins that Perceus resolves a retained owner before a call (new tests)." -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01MK52fNp5uA711Q2D4kWEtR"
```

### Task 7b: Port the CEK's LIR-level run tests to `eml_vm`

Spec: the 「crate `eml_interp`」 table rows for `a_call_resumes_at_the_next_statement_of_its_block`, `a_dup_of_a_file_is_a_runtime_error`, `a_fault_outside_an_extern_has_no_position`, `a_heap_fault_inside_an_extern_with_a_position_carries_it`, `debug_heap_reports_leaks` (fast-mode half), and `src/error.rs`'s `leaks_are_displayed_with_the_object_counts`. The LIR texts are the CEK ones; `save [...]` stays (Task 12 removes it from every text). Assertions use the VM's error type: `VmError::Fault { fault: VmFault::Extern(Fault::Heap(HeapError::…)), function, at }`, and the `File` dup is `HeapError::LinDup` ("a Lin object is shared"), which only checked mode detects. Unverified IR runs through `crate::common::run_unverified` (checked mode only).

**Files:**
- Modify: `crates/eml_vm/tests/run.rs` (header comment, imports, the fast-mode half of `a_forgotten_object_is_a_leak`, five tests appended)
- Test: `crates/eml_vm/tests/run.rs`

**Interfaces:**
- Consumes: `crate::common::{parse(&str) -> Program, run_lir(&str) -> (String, Result<(), VmError>), run_unverified(&str) -> (String, Result<(), VmError>)}` (existing), `eml_test_support::execute_vm(&Program, bool)`, `eml_rt::HeapError::{UseAfterFree, LinDup}`, `eml_vm::{VmError, VmFault, SourceLocation}`.
- Produces: the five tests below. Task 8 rewrites the `execute_vm(&parse(text), false)` call in `a_forgotten_object_is_a_leak` to `execute_vm(&parse(text), &RunConfig::default())` and renames `VmError` here to `RuntimeError`; Task 12 deletes `save [n.0]` from `a_call_resumes_at_the_next_statement_of_its_block`.

- [ ] **Step 1: Header and imports**

In `crates/eml_vm/tests/run.rs`, replace lines 1-7:

```rust
//! 分岐のない LIR の実行: 定数、extern、直接の呼び出しと戻り、末尾呼び出し、誤りと位置、終わりの処理
//! (docs/spec/vm.md の「呼び出しと戻り」と「誤り、止めること、終わり」)。

use eml_rt::{ArithFault, Fault};
use eml_vm::{SourceLocation, VmError, VmFault};

use crate::common::{run_lir, run_unverified};
```

with

```rust
//! LIR の実行: 定数、extern、直接の呼び出しと戻り (入口でないブロックからの戻りを含む)、末尾呼び出し、誤りと位置、
//! 終わりの処理 (docs/spec/vm.md の「呼び出しと戻り」と「誤り、止めること、終わり」)。

use eml_rt::{ArithFault, Fault, HeapError};
use eml_test_support::execute_vm;
use eml_vm::{SourceLocation, VmError, VmFault};

use crate::common::{parse, run_lir, run_unverified};
```

- [ ] **Step 2: Add the fast-mode half of `a_forgotten_object_is_a_leak`**

At the end of `a_forgotten_object_is_a_leak` (after the `"memory leak: objects were not freed: 1 String"` assertion), add:

```rust
    // 速いモードはリークを確かめないので、同じ IR が成功で終わる
    let (_, fast) = execute_vm(&parse(text), false);
    assert_eq!(fast.map(|_| ()), Ok(()));
```

- [ ] **Step 3: Append the five ported tests at the end of the file**

```rust
#[test]
fn a_call_resumes_at_the_next_statement_of_its_block() {
    // 入口でないブロックの2つの呼び出しから、それぞれ次の文に戻り、呼ぶ側のスロットに残る `n.0` を合流のブロック
    // まで持ち越す (docs/spec/vm.md の「呼び出しと戻り」)
    let text = "\
fn main() -> unit {
  let r.0: int = call pick(1)
  let s.1: obj = extern \"Prelude.Show Int.show\"(r.0)
  let t.2: unit = extern Prelude.println(s.1)
  return t.2
}
fn pick(n.0: int) -> int {
  switch n.0 { 1 -> b1, _ -> b2 }
b1:
  let a.1: int = call double(n.0) save [n.0]
  let b.2: int = call double(a.1) save [n.0]
  jump b3(b.2)
b2:
  jump b3(0)
b3(m.3: int):
  let c.4: int = extern Prelude.+(m.3, n.0)
  return c.4
}
fn double(x.0: int) -> int {
  let y.1: int = extern Prelude.+(x.0, x.0)
  return y.1
}
";
    assert_eq!(run_lir(text), ("5\n".to_string(), Ok(())));
}

#[test]
fn a_fault_outside_an_extern_has_no_position() {
    // 位置を付けるのは extern の呼び出しが起こした誤りだけである。同じ関数に位置付きの extern があっても、`decref` の
    // 誤りには付けない
    let text = "\
layout (,) { (,)(tobj, tobj) }
fn main() -> unit {
  let n.0: int = extern Prelude.+(1, 2) @\"main.em\":1:1
  let d.1: obj = con (,) #0(n.0, n.0)
  decref d.1
  decref d.1
  return ()
}
";
    let (_, result) = run_unverified(text);
    assert_eq!(
        result,
        Err(VmError::Fault {
            fault: VmFault::Extern(Fault::Heap(HeapError::UseAfterFree)),
            function: "main".to_string(),
            at: None,
        })
    );
}

#[test]
fn a_heap_fault_inside_an_extern_with_a_position_carries_it() {
    // 位置は extern の呼び出しが起こした誤りすべてに付き、ヒープの誤りも含む (docs/spec/vm.md の「誤り、止めること、
    // 終わり」)
    let text = "\
fn main() -> unit {
  let s.0: obj = extern \"Prelude.Show Int.show\"(1)
  decref s.0
  let t.1: unit = extern Prelude.println(s.0) @\"main.em\":2:3
  return t.1
}
";
    let (_, result) = run_unverified(text);
    let error = result.unwrap_err();
    assert_eq!(
        error,
        VmError::Fault {
            fault: VmFault::Extern(Fault::Heap(HeapError::UseAfterFree)),
            function: "main".to_string(),
            at: Some(SourceLocation {
                path: "main.em".to_string(),
                line: 2,
                column: 3,
            }),
        }
    );
    assert_eq!(error.to_string(), "use of a freed object\n  at main.em:2:3");
}

#[test]
fn a_dup_of_a_file_is_a_runtime_error() {
    // verifier は `lin` の変数の `dup` を V1 で静的に拒む。印を誤った IR でも、検査モードのヒープが実行時に止める
    // ことを、verifier を通さずに確かめる。速いモードはこれを見つけない (docs/spec/object-model.md の「`lin` ビット」と
    // 「受け入れる制約」)
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml");
    let text = format!(
        "\
fn main() -> unit {{
  let p.0: obj = const \"{path}\"
  let u.1: unit = call f(p.0)
  return u.1
}}
fn f(p.0: obj) -> unit {{
  let h.1: obj lin = extern Std.Fs.open(p.0)
  dup h.1
  let u.2: unit = extern Std.Fs.close(h.1)
  let u.3: unit = extern Std.Fs.close(h.1)
  return u.3
}}
"
    );
    assert_eq!(
        eml_lir::verify(&parse(&text)).map_err(|error| error.to_string()),
        Err("`h.1` (lin) is duplicated, but only unr can be in `f`".to_string())
    );
    let (_, result) = run_unverified(&text);
    let error = result.unwrap_err();
    assert_eq!(
        error,
        VmError::Fault {
            fault: VmFault::Extern(Fault::Heap(HeapError::LinDup)),
            function: "f".to_string(),
            at: None,
        }
    );
    assert_eq!(error.to_string(), "a Lin object is shared in `f`");
}

/// 種類が2つ以上のリークは、種類ごとの数を `, ` でつないで表示する。
#[test]
fn leaks_are_displayed_with_the_object_counts() {
    let leak = VmError::Leak(vec![("Closure".to_string(), 2), ("String".to_string(), 1)]);
    assert_eq!(
        leak.to_string(),
        "memory leak: objects were not freed: 2 Closure, 1 String"
    );
}
```

- [ ] **Step 4: Run them**

```bash
cargo test -p eml_vm --test integration run::
```

Expected: `16 passed` (11 existing + 5 new; more if Tasks 1-5 added `run::` tests). The behaviour already exists in the VM, so there is no red phase. If `a_dup_of_a_file_is_a_runtime_error` gives another `HeapError`, read `crates/eml_vm/src/vm.rs`'s `Dup` arm and `eml_rt`'s `Heap::dup` (`heap/check.rs`, `LinDup`): a different variant is a VM bug to fix with this test red, not an assertion to change.

- [ ] **Step 5: Gate**

```bash
cargo test -p eml_cli --test integration citations
cargo test
cargo clippy --all-targets   # 0 warnings
cargo fmt --check
```

- [ ] **Step 6: Commit**

```bash
git add crates/eml_vm/tests/run.rs
git commit -m "Port the CEK's LIR-level run tests to the VM" -m "Adds to eml_vm's run.rs the four CEK tests the spec moves (a call in a non-entry block resuming at its next statement, a File dup caught by checked mode as LinDup, a decref fault without a position next to a positioned extern, a heap fault inside a positioned extern) with their LIR unchanged and the VM's error type, and the multi-kind leak display test from eml_interp's error.rs (new tests, moved per the spec's table). a_forgotten_object_is_a_leak also checks that fast mode ends Ok on the same IR, the half of debug_heap_reports_leaks the VM did not have (expected-value change, kind 2)." -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01MK52fNp5uA711Q2D4kWEtR"
```

### Task 7c: Delete `eml_interp`'s tests

Spec: 「crate `eml_interp`」 table and the list of 30 + 12 deleted tests (kind 1). Every test of `tests/bench.rs`, `closures.rs`, `data.rs`, `evidence.rs`, `run.rs` is either deleted by the spec or now has its VM counterpart (Task 7a, 7b). `tests/rt_externs.rs` stays (Task 9 moves its table writer to `eml_rt`), and so does all of `src/` with its unit tests (Task 10 deletes the crate). The UI suite keeps comparing the CEK and the VM until Task 10.

**Files:**
- Delete: `crates/eml_interp/tests/bench.rs`, `crates/eml_interp/tests/closures.rs`, `crates/eml_interp/tests/data.rs`, `crates/eml_interp/tests/evidence.rs`, `crates/eml_interp/tests/run.rs`, `crates/eml_interp/tests/common/mod.rs`, `crates/eml_interp/tests/ir/mask.lir`, `crates/eml_interp/tests/ir/multi_resume.lir`
- Modify: `crates/eml_interp/tests/main.rs` (only `mod rt_externs;` remains)
- Modify: `crates/eml_interp/Cargo.toml` (drop the dev-dependencies `eml_test_support` and `insta`, which only the deleted files used; keep `eml_rt`, which `rt_externs.rs` uses)
- Modify: `Cargo.lock` (cargo drops `eml_test_support` and `insta` from `eml_interp`'s dependency list)
- Test: `cargo test -p eml_interp`

**Interfaces:**
- Consumes: Tasks 7a and 7b (the moved tests exist in `eml_vm`).
- Produces: `eml_interp` with `src/` intact and one integration test file, `tests/rt_externs.rs`, which Task 9 and Task 10 consume.

- [ ] **Step 1: Confirm the moved tests exist on the VM side**

```bash
for t in a_call_resumes_at_the_next_statement_of_its_block a_dup_of_a_file_is_a_runtime_error \
  a_fault_outside_an_extern_has_no_position a_heap_fault_inside_an_extern_with_a_position_carries_it \
  leaks_are_displayed_with_the_object_counts a_match_with_a_thousand_literals_runs_in_a_debug_build \
  a_match_with_a_thousand_shared_arms_runs_in_a_debug_build long_sequence_of_if_statements_does_not_overflow_the_stack \
  long_statement_sequence_does_not_overflow_the_stack; do
  grep -q "fn $t()" crates/eml_vm/tests/*.rs && echo "ok $t" || echo "MISSING $t"; done
```

Expected: nine `ok` lines. Also check the 30 names of the spec's 「同じ名前で同じ LIR を流すテストが `eml_vm` にある」 list (one command, expected output `0`):

```bash
for t in a_box_and_an_unbox_pass_the_value_through a_case_with_a_different_number_of_fields_is_an_internal_error a_dummy_passes_through_box_unbox_dup_and_decref a_function_value_is_applied_like_a_closure_without_arguments a_function_value_needs_no_reference_counting a_mask_skips_outer_handlers_only a_partial_application_waits_for_the_rest_of_the_arguments a_release_keeps_one_value_in_two_fields_twice a_release_of_a_shared_box_dups_its_field a_release_of_a_unique_box_hands_its_field_over a_release_of_a_value_that_is_not_an_object_is_an_internal_error a_release_of_another_layout_is_an_internal_error a_renewed_evidence_has_its_own_state_and_the_current_evv_outside a_returned_function_can_still_wait_for_more_arguments a_shared_closure_keeps_its_captured_values a_string_switch_leaves_the_string_to_its_targets a_tobj_variable_holding_a_tag_takes_its_case a_value_read_with_another_layout_passes_the_verifier_and_faults a_value_with_fields_that_goes_to_the_default_is_released a_yield_does_not_start_during_another_yield a_yield_reaches_the_status_of_its_target an_immediate_clause_reads_and_writes_the_state_of_its_evidence an_inner_handle_returns_through_each_resumption_of_an_outer_multi_operation an_int_switch_goes_to_the_matching_case an_unpack_of_a_value_that_is_not_an_object_is_an_internal_error an_unpack_of_an_object_that_is_not_data_is_an_internal_error an_unpack_reads_the_fields_without_taking_the_box an_unpack_with_a_different_tag_is_an_internal_error evv_mask_skips_the_innermost_evidence_of_an_effect extra_arguments_are_applied_to_the_returned_function; do
  grep -q "fn $t()" crates/eml_vm/tests/*.rs || echo "MISSING $t"; done | wc -l
```

- [ ] **Step 2: Delete the files**

```bash
git rm crates/eml_interp/tests/bench.rs crates/eml_interp/tests/closures.rs \
  crates/eml_interp/tests/data.rs crates/eml_interp/tests/evidence.rs crates/eml_interp/tests/run.rs
git rm -r crates/eml_interp/tests/common crates/eml_interp/tests/ir
ls crates/eml_interp/tests   # expected: main.rs  rt_externs.rs
```

- [ ] **Step 3: `crates/eml_interp/tests/main.rs`**

The whole file becomes:

```rust
//! 結合テストは、リンクと初回の起動の時間を抑えるため1つのバイナリにまとめる。ここで宣言しないファイルは流れない
//! (docs/implementation/testing.md の「crate の中の置き方」)。

mod rt_externs;
```

- [ ] **Step 4: `crates/eml_interp/Cargo.toml`**

Replace

```toml
[dev-dependencies]
eml_rt.workspace = true
eml_test_support = { workspace = true, features = ["run"] }
insta.workspace = true
```

with

```toml
[dev-dependencies]
eml_rt.workspace = true
```

`[lib]`, `autotests = false` and the `[[test]] name = "integration" path = "tests/main.rs"` entry stay. `src/` has no `insta` or `eml_test_support` use (`grep -rn "insta\|eml_test_support" crates/eml_interp/src` prints nothing).

- [ ] **Step 5: Build and run what is left**

```bash
cargo test -p eml_interp
git diff --no-ext-diff Cargo.lock
```

Expected: the lib's unit tests pass (12 on 7d1bf441), and the integration binary runs `rt_externs` with `3 passed; 1 ignored` (the ignored one is the table writer). The `Cargo.lock` diff removes exactly the two lines `"eml_test_support",` and `"insta",` from the `eml_interp` package's `dependencies`.

- [ ] **Step 6: Gate**

```bash
cargo test
cargo clippy --all-targets   # 0 warnings
cargo fmt --check
```

The UI suite (`cargo test -p eml_cli --test integration ui::`) still compares the CEK and the VM; it must pass unchanged.

- [ ] **Step 7: Commit**

```bash
git add -A crates/eml_interp Cargo.lock
git commit -m "Delete eml_interp's tests except rt_externs" -m "Deletes bench.rs, closures.rs, data.rs, evidence.rs, run.rs, common and ir from eml_interp's tests: the VM runs the same LIR under the same names, the spec drops the rest, and the tests it moves are already in eml_vm (pass/fail changes, kind 1, as listed in the S10b spec's eml_interp table). rt_externs.rs stays until its table writer moves to eml_rt, and src/ stays until the crate goes. The dev-dependencies only the deleted files used are dropped." -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01MK52fNp5uA711Q2D4kWEtR"
```

### Task 8: eml_vm の実行の API (`RunConfig`、`RuntimeError`、`interrupted`)

All code in this task was compiled and run against HEAD 7d1bf441 in a scratch copy (`cargo test`, `cargo clippy --all-targets`, `cargo fmt --check` green). Line numbers below are at HEAD 7d1bf441; Tasks 1-7 shift some of them, so locate each block by its quoted text.

**Files:**
- Modify: `crates/eml_vm/src/lib.rs` (line 9 `use eml_rt::…`, lines 24-25 `pub use`, lines 51-73 `execute` / `execute_with` replaced by `RunConfig` + one `execute`; every `VmError` → `RuntimeError`)
- Modify: `crates/eml_vm/src/error.rs` (lines 59-96: `VmError` → `RuntimeError`, new `RuntimeError::interrupted`)
- Modify: `crates/eml_vm/src/code.rs` (lines 8, 80, 92: `VmError` → `RuntimeError`)
- Modify: `crates/eml_test_support/src/lib.rs` (lines 313-335: `execute_vm`, `run_vm`, `run_stats_vm` take/build a `RunConfig`)
- Modify: `crates/eml_cli/src/lib.rs` (lines 228-242: `execute_vm(program, &VmRunConfig, HostRef)`, interim re-export names)
- Modify: `crates/eml_cli/src/main.rs` (lines 62-69: the `--vm` branch builds a `VmRunConfig`)
- Modify: `crates/eml_cli/tests/ui.rs` (lines 9-12 imports, lines 150-153 `check_on_vm`'s closure)
- Modify: `crates/eml_cli/tests/api.rs` (lines 275-279 `execute_vm_runs_a_compiled_program_and_counts_its_work`)
- Modify: `crates/eml_vm/tests/calls.rs` (line 7 import; lines 251, 261, 295 `execute_with`; the memory-limit test Task 5 added, also an `execute_with` call)
- Modify: `crates/eml_vm/tests/common/mod.rs` (lines 6, 21-36: `VmError`, `execute_vm(…, bool)`)
- Modify: `crates/eml_vm/tests/miri.rs` (lines 7, 14: `execute(&program, debug_heap, host)`)
- Modify: `crates/eml_vm/tests/scaling.rs` and `crates/eml_vm/tests/run.rs` (Task 7 put one `execute_vm(&program, bool)` call in each)
- Modify: `crates/eml_vm/tests/{data.rs,evidence.rs,run.rs}` (`VmError` → `RuntimeError` only)
- Test: `crates/eml_vm/tests/api.rs` (new `RunConfig` and `interrupted` tests)

**Interfaces:**
- Consumes: Task 5's `pub peak_stack: u64` in `eml_vm::RunStats` (this task does not touch `run`'s body, so the field stays as Task 5 left it); the current `eml_vm::{execute(program, debug_heap, host), execute_with(…, control), VmError}`; `eml_test_support::{execute_vm(&Program, bool), run_vm, run_stats_vm}`.
- Produces:
  - `#[derive(Clone)] #[non_exhaustive] pub struct eml_vm::RunConfig { pub debug_heap: bool, pub control: Arc<Control> }`, `impl Default` (fast, `Arc::new(Control::new(Limits::default()))`), `with_debug_heap(self, bool) -> Self`, `with_control(self, Arc<Control>) -> Self`
  - `pub fn eml_vm::execute(program: &eml_lir::Program, config: &RunConfig, host: HostRef) -> Result<RunStats, RuntimeError>` (`execute_with` is gone)
  - `pub enum eml_vm::RuntimeError { Fault { fault: VmFault, function: String, at: Option<SourceLocation> }, Leak(Vec<(String, u64)>) }`, `pub fn RuntimeError::interrupted(&self) -> bool`
  - `pub use eml_rt::{Control, HostRef, Limits, Stop}` from `eml_vm`
  - `eml_test_support::execute_vm(program: &eml_lir::Program, config: &eml_vm::RunConfig) -> (String, Result<eml_vm::RunStats, eml_vm::RuntimeError>)`, `run_vm(text) -> (String, Result<(), eml_vm::RuntimeError>)`, `run_stats_vm(text) -> (String, Result<eml_vm::RunStats, eml_vm::RuntimeError>)` (checked mode, fresh unbounded `Control`)
  - Interim, removed in Task 10: `eml_cli::execute_vm(program: &eml_lir::Program, config: &VmRunConfig, host: HostRef) -> Result<VmRunStats, VmRuntimeError>` and `pub use eml_vm::{Capture, HostRef, RunConfig as VmRunConfig, RunStats as VmRunStats, RuntimeError as VmRuntimeError, VmFault, capture_host, stdio_host}`. The aliases exist only because `eml_cli` still re-exports the CEK's `RunConfig`, `RunStats` and `RuntimeError`.

- [ ] **Step 1: Write the failing API tests**

Replace the whole of `crates/eml_vm/tests/api.rs` with:

```rust
//! 公開の API の性質。後の並列 VM のための型の性質 (docs/future/multicore.md の「VM の並列化の備え」の制約1と2) と、
//! 実行の設定 `RunConfig` と、実行時エラーの `interrupted` である。

use std::sync::Arc;

use eml_rt::{Checked, Fast, Fault};
use eml_vm::{CodeBase, Control, Limits, RunConfig, RuntimeError, Stop, Task, VmFault};

use crate::common::parse;

fn sync<T: Sync>() {}

fn send<T: Send>() {}

#[test]
fn a_code_base_is_shared_and_a_task_moves() {
    sync::<CodeBase<Fast>>();
    sync::<CodeBase<Checked>>();
    send::<Task>();
}

#[test]
fn the_default_config_is_fast_with_a_fresh_unbounded_control() {
    let config = RunConfig::default();
    assert!(!config.debug_heap);
    assert_eq!(config.control.limits(), Limits::default());
    assert_eq!(config.control.stop_bits(), 0);
    assert_eq!(config.control.bytes(), 0);
    assert!(!Arc::ptr_eq(&config.control, &RunConfig::default().control));
}

#[test]
fn with_debug_heap_sets_the_flag() {
    assert!(RunConfig::default().with_debug_heap(true).debug_heap);
    assert!(
        !RunConfig::default()
            .with_debug_heap(true)
            .with_debug_heap(false)
            .debug_heap
    );
}

#[test]
fn with_control_replaces_the_control() {
    let limits = Limits {
        max_depth: 1000,
        ..Limits::default()
    };
    let control = Arc::new(Control::new(limits));
    let config = RunConfig::default().with_control(Arc::clone(&control));
    assert!(Arc::ptr_eq(&config.control, &control));
    assert_eq!(config.control.limits(), limits);
}

/// 止める理由のビットは `Control::clear` でしか下りないので、`clone` した設定で続けて流すと前の実行の割り込みで止まる。
/// 実行ごとに新しい設定を作るのはこのためである。
#[test]
fn a_cloned_config_shares_its_control() {
    let config = RunConfig::default().with_debug_heap(true);
    let copy = config.clone();
    assert!(Arc::ptr_eq(&config.control, &copy.control));
    assert!(copy.debug_heap);
    config.control.interrupt();
    assert_eq!(
        copy.control.stop_bits() & Control::INTERRUPT,
        Control::INTERRUPT
    );
}

const COUNT: &str = "\
layout Bool { False, True }
fn main() -> unit {
  tail call count(0)
}
fn count(n.0: int) -> unit {
  let d.1: enum = extern \"Prelude.Eq Int.==\"(n.0, 100000)
  switch d.1 Bool { #0 -> b1, #1 -> b2 }
b1:
  let m.2: int = extern \"Prelude.+\"(n.0, 1)
  tail call count(m.2)
b2:
  return ()
}
";

/// 実行の前に割り込まれた `Control` では、最初の安全点で止まる。
#[test]
fn an_interrupted_control_stops_at_the_first_safepoint() {
    for debug_heap in [true, false] {
        let control = Arc::new(Control::new(Limits::default()));
        control.interrupt();
        let config = RunConfig::default()
            .with_debug_heap(debug_heap)
            .with_control(control);
        let (host, _) = eml_vm::capture_host(Default::default());
        let error = eml_vm::execute(&parse(COUNT), &config, host).unwrap_err();
        assert_eq!(error.to_string(), "interrupted in `main`");
        assert!(error.interrupted());
    }
}

/// 安全点を通らずに `Fs.read_all` に着くプログラムは、読みの中で止まる。止めたのは extern なので `Extern` の誤りになる。
#[test]
fn an_interrupt_inside_read_all_is_an_interrupt() {
    let root = std::env::temp_dir().join(format!("eml-vm-read-interrupt-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("a.txt"), "abc").unwrap();
    let text = "\
layout (,) { (,)(tobj, tobj) }
fn main() -> unit {
  let p.0: obj = const \"a.txt\"
  let h.1: obj lin = extern \"Std.Fs.open\"(p.0)
  let t.2: obj lin = extern \"Std.Fs.read_all\"(h.1)
  unpack t.2 (,) #0(f.3: obj lin, s.4: obj)
  release t.2 (,) #0(f.3, s.4)
  decref s.4
  let u.5: unit = extern \"Std.Fs.close\"(f.3)
  return u.5
}
";
    let program = parse(text);
    eml_lir::verify(&program).unwrap_or_else(|error| panic!("{error}"));
    let control = Arc::new(Control::new(Limits::default()));
    control.interrupt();
    let config = RunConfig::default()
        .with_debug_heap(true)
        .with_control(control);
    let (host, _) = eml_vm::capture_host(root.clone());
    let result = eml_vm::execute(&program, &config, host);
    std::fs::remove_dir_all(&root).unwrap();
    let error = result.unwrap_err();
    assert!(
        matches!(
            error,
            RuntimeError::Fault {
                fault: VmFault::Extern(Fault::Stopped(Stop::Interrupted)),
                ..
            }
        ),
        "{error:?}"
    );
    assert!(error.interrupted());
}

fn fault(fault: VmFault) -> RuntimeError {
    RuntimeError::Fault {
        fault,
        function: "f".to_string(),
        at: None,
    }
}

/// 割り込みだけが真になる。上限で止めたことも、ほかの誤りも、リークも割り込みではない。
#[test]
fn only_an_interrupt_is_interrupted() {
    assert!(fault(VmFault::Stopped(Stop::Interrupted)).interrupted());
    assert!(fault(VmFault::Extern(Fault::Stopped(Stop::Interrupted))).interrupted());
    for stop in [Stop::DepthLimit, Stop::MemoryLimit] {
        assert!(!fault(VmFault::Stopped(stop)).interrupted());
        assert!(!fault(VmFault::Extern(Fault::Stopped(stop))).interrupted());
    }
    assert!(!fault(VmFault::Extern(eml_rt::ArithFault::DivisionByZero.into())).interrupted());
    assert!(!fault(VmFault::Internal("x")).interrupted());
    assert!(!RuntimeError::Leak(vec![("String".to_string(), 1)]).interrupted());
}
```

The read-all test works because `main` makes no call before `Std.Fs.read_all`, so no safepoint polls first, and `read_text` (`crates/eml_rt/src/host.rs`) checks the stop bits before its first chunk. Hand-written LIR externs carry no position, so the error has `at: None`.

- [ ] **Step 2: Run the new tests and see them fail to compile**

Run: `cargo test -p eml_vm --test integration api::`
Expected: compile errors `unresolved imports eml_vm::Control, eml_vm::Limits, eml_vm::RunConfig, eml_vm::RuntimeError, eml_vm::Stop`.

- [ ] **Step 3: Add `RunConfig` and the single `execute` in `crates/eml_vm/src/lib.rs`**

Change the `eml_rt` import (line 9) to drop the names that are now re-exported:

```rust
use eml_rt::{Buffering, Checked, Ctx, Fast, Mode, StdHost};
```

Replace `pub use eml_rt::HostRef;` and `pub use error::{SourceLocation, VmError, VmFault};` (lines 24-25) with:

```rust
pub use eml_rt::{Control, HostRef, Limits, Stop};
pub use error::{RuntimeError, SourceLocation, VmFault};
```

Replace the block from `` /// 上限のない `Control` で実行する。上限の指定は S10b で `RunConfig` から渡す。 `` through the closing `}` of `execute_with` (lines 51-73) with:

```rust
/// 実行の設定。上限は `Control` が持つので、ここに上限の欄は置かない。`file_root` は Host が持つ
/// (docs/spec/vm.md の「置き場所」)。
#[derive(Clone)]
#[non_exhaustive]
pub struct RunConfig {
    pub debug_heap: bool,
    /// 止める理由のビットは `Control::clear` でしか下りないので、1つの `Control` は1回の実行にだけ使う。`clone` した
    /// 設定は同じ `Control` を指す。
    pub control: Arc<Control>,
}

// `Control` は `Default` を持たないので、手で書く
impl Default for RunConfig {
    fn default() -> RunConfig {
        RunConfig {
            debug_heap: false,
            control: Arc::new(Control::new(Limits::default())),
        }
    }
}

impl RunConfig {
    pub fn with_debug_heap(mut self, debug_heap: bool) -> RunConfig {
        self.debug_heap = debug_heap;
        self
    }

    pub fn with_control(mut self, control: Arc<Control>) -> RunConfig {
        self.control = control;
        self
    }
}

pub fn execute(
    program: &Program,
    config: &RunConfig,
    host: HostRef,
) -> Result<RunStats, RuntimeError> {
    let control = Arc::clone(&config.control);
    if config.debug_heap {
        run::<Checked>(program, host, control, check_heap)
    } else {
        run::<Fast>(program, host, control, |_, _| Ok(()))
    }
}
```

`run`, `check_heap` and `compile_error` keep their bodies; only the type name changes in the next step.

- [ ] **Step 4: Rename `VmError` to `RuntimeError` everywhere except `eml_cli`**

```bash
grep -rlw VmError crates/eml_vm crates/eml_test_support | xargs perl -pi -e 's/\bVmError\b/RuntimeError/g'
```

This covers `crates/eml_vm/src/{lib.rs,error.rs,code.rs}`, `crates/eml_vm/tests/{calls.rs,common/mod.rs,data.rs,evidence.rs,run.rs}` and any file Task 7 added, and the qualified `eml_vm::VmError` in `crates/eml_test_support/src/lib.rs`. `eml_cli` re-exports `VmError` next to the CEK's `RuntimeError`, so it is edited by hand in Step 7.

- [ ] **Step 5: Add `RuntimeError::interrupted` in `crates/eml_vm/src/error.rs`**

Insert before `impl fmt::Display for RuntimeError {` (the file already imports `Fault` and `Stop` from `eml_rt`):

```rust
impl RuntimeError {
    /// 割り込みで止まったか。安全点で止めたときと、`Fs.read_all` の中で止めたときの両方で真になる。CLI が終了コードを
    /// 決めるために使う。
    pub fn interrupted(&self) -> bool {
        matches!(
            self,
            RuntimeError::Fault {
                fault: VmFault::Stopped(Stop::Interrupted)
                    | VmFault::Extern(Fault::Stopped(Stop::Interrupted)),
                ..
            }
        )
    }
}
```

- [ ] **Step 6: Move the VM test callers and `eml_test_support` onto `RunConfig`**

Rewrite every `eml_vm::execute_with(program, bool, host, control)` (the three in `calls.rs` and Task 5's memory-limit test) and every `execute_vm(program, bool)` in the eml_vm tests (`common/mod.rs`, and the one call each in Task 7's `scaling.rs` and `run.rs`):

```bash
perl -0pi -e 's/eml_vm::execute_with\(\s*([^;]*?),\s*(true|false),\s*host,\s*(\w+),?\s*\)/eml_vm::execute(\n        $1,\n        &eml_vm::RunConfig::default()\n            .with_debug_heap($2)\n            .with_control($3),\n        host,\n    )/gs' crates/eml_vm/tests/*.rs
perl -0pi -e 's/\bexecute_vm\(([^;]*?),\s*(true|false)\)/execute_vm($1, &eml_vm::RunConfig::default().with_debug_heap($2))/gs' crates/eml_vm/tests/*.rs crates/eml_vm/tests/*/*.rs
```

In `crates/eml_vm/tests/miri.rs`, change `use eml_vm::{capture_host, execute};` to `use eml_vm::{RunConfig, capture_host, execute};` and the loop body line `execute(&program, debug_heap, host).unwrap_or_else(|error| panic!("{error}"));` to:

```rust
        let config = RunConfig::default().with_debug_heap(debug_heap);
        execute(&program, &config, host).unwrap_or_else(|error| panic!("{error}"));
```

In `crates/eml_test_support/src/lib.rs`, replace the three VM functions (from `/// 手で書いた LIR を VM で実行する。出力はメモリに捕まえる。` to the end of `run_stats_vm`) with:

```rust
/// 手で書いた LIR を VM で実行する。出力はメモリに捕まえる。
#[cfg(feature = "run")]
pub fn execute_vm(
    program: &eml_lir::Program,
    config: &eml_vm::RunConfig,
) -> (String, Result<eml_vm::RunStats, eml_vm::RuntimeError>) {
    let (host, out) = eml_vm::capture_host(std::path::PathBuf::new());
    let result = eml_vm::execute(program, config, host);
    (out.contents(), result)
}

/// ソースを VM の検査モードで実行する。
#[cfg(feature = "run")]
pub fn run_vm(text: &str) -> (String, Result<(), eml_vm::RuntimeError>) {
    let (out, result) = run_stats_vm(text);
    (out, result.map(|_| ()))
}

/// `RunStats` も返す。VM の数を固定するテストのために使う。止める理由のビットは前の実行から持ち越されるので、実行
/// ごとに新しい `Control` を持つ設定を作る。
#[cfg(feature = "run")]
pub fn run_stats_vm(text: &str) -> (String, Result<eml_vm::RunStats, eml_vm::RuntimeError>) {
    execute_vm(
        &lir(text),
        &eml_vm::RunConfig::default().with_debug_heap(true),
    )
}
```

Check: `grep -rn -e execute_with -e VmError crates/eml_vm crates/eml_test_support` prints nothing, and `grep -rnE 'execute_vm\([^)]*\b(true|false)\)' crates` prints nothing.

- [ ] **Step 7: Adapt `eml_cli` to the interim names**

In `crates/eml_cli/src/lib.rs`, replace from `pub fn execute_vm(` (line 231) to the end of the file with:

```rust
pub fn execute_vm(
    program: &eml_lir::Program,
    config: &VmRunConfig,
    host: HostRef,
) -> Result<VmRunStats, VmRuntimeError> {
    eml_vm::execute(program, config, host)
}

// CEK の `RunConfig`、`RunStats`、`RuntimeError` と名前がぶつかるので、切り替えまで別の名前で公開する
#[cfg(feature = "run")]
pub use eml_vm::{
    Capture, HostRef, RunConfig as VmRunConfig, RunStats as VmRunStats,
    RuntimeError as VmRuntimeError, VmFault, capture_host, stdio_host,
};
```

In `crates/eml_cli/src/main.rs`, replace `eml_cli::execute_vm(&program, debug_heap, host)` (line 66) with:

```rust
                let config = eml_cli::VmRunConfig::default().with_debug_heap(debug_heap);
                eml_cli::execute_vm(&program, &config, host)
```

In `crates/eml_cli/tests/ui.rs`, change the import block (lines 9-12) to:

```rust
use eml_cli::{
    FsProvider, OutputSink, RunConfig, RunStats, RuntimeError, Session, VmRunConfig, VmRunStats,
    capture_host, execute_vm,
};
```

and in `check_on_vm`'s closure replace `let result = execute_vm(program, debug_heap, host);` with:

```rust
        let config = VmRunConfig::default().with_debug_heap(debug_heap);
        let result = execute_vm(program, &config, host);
```

The closure is called once per mode, so each run gets a fresh `Control`.

In `crates/eml_cli/tests/api.rs`, in `execute_vm_runs_a_compiled_program_and_counts_its_work`, replace `let stats: eml_cli::VmRunStats = eml_cli::execute_vm(&program, true, host).unwrap();` with:

```rust
    let config = eml_cli::VmRunConfig::default().with_debug_heap(true);
    let stats: eml_cli::VmRunStats = eml_cli::execute_vm(&program, &config, host).unwrap();
```

- [ ] **Step 8: Format and run the new tests**

Run: `cargo fmt && cargo test -p eml_vm --test integration api::`
Expected: `test result: ok. 8 passed` (`a_code_base_is_shared_and_a_task_moves` plus the 7 new tests). `an_interrupted_control_stops_at_the_first_safepoint` reads ``interrupted in `main` ``: the first safepoint is the tail call in `main`, and its error takes the function of the failing instruction.

- [ ] **Step 9: Run the full gate**

```bash
cargo test
cargo clippy --all-targets
cargo fmt --check
cargo clippy -p eml_cli --no-default-features --features core
nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_vm --test integration miri::
nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance -Zmiri-tree-borrows" cargo miri test -p eml_vm --test integration miri::
```

Expected: all green, 0 clippy warnings. No snapshot changes (the UI harness still compares the VM with the CEK).

- [ ] **Step 10: Commit**

```bash
git add crates/eml_vm crates/eml_test_support/src/lib.rs crates/eml_cli/src/lib.rs crates/eml_cli/src/main.rs crates/eml_cli/tests/ui.rs crates/eml_cli/tests/api.rs
git commit -m "Give eml_vm one execute with RunConfig and rename VmError to RuntimeError" -m "eml_vm::execute now takes a RunConfig { debug_heap, control } and replaces execute_with. The config holds the Control so that the CLI can hand the same Control to the SIGINT handler (spec, 実行の API). VmError becomes RuntimeError because it will be the only runtime error type, and RuntimeError::interrupted covers both the safepoint stop and the stop inside Fs.read_all. eml_vm re-exports Control, Limits and Stop. eml_cli exposes the VM types under Vm* aliases until the switch, because it still re-exports the CEK's types.

Test changes: new tests in eml_vm/tests/api.rs for RunConfig (default, with_debug_heap, with_control, a cloned config shares its Control) and for interrupted (an interrupted Control stops at the first safepoint in both modes, a stop inside Fs.read_all, other faults and leaks are not interrupts). Mechanical (kind 3): the VmError rename in tests, and the execute_with / execute_vm(…, bool) callers in eml_vm, eml_cli and eml_test_support moved onto RunConfig with unchanged expected values." -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01MK52fNp5uA711Q2D4kWEtR"
```

### Task 9: 期待値の表の書き手を eml_rt に移す

Decision: this task deletes all of `crates/eml_interp/tests/rt_externs.rs`. Its writer moves to `eml_rt`, and the spec deletes the other three tests (`the_interpreter_matches_the_table`, `a_failed_write_reads_the_same_on_both_sides`, `tags_agree_with_eml_lir`). Leaving them for Task 10 would keep a second writer of `expected.txt` alive for one more commit. The stale-table check of `the_interpreter_matches_the_table` (the table's inputs equal `table::inputs()`) moves into `eml_rt`'s `the_externs_match_the_table`. Otherwise no check would catch a table that was not rewritten after its inputs changed.

Verified in a scratch copy at HEAD 7d1bf441: the writer regenerates `expected.txt` with all 4025 rows byte-identical; only the comment on line 1 changes. `cargo verus verify -p eml_rt` and `verusfmt --verus-only --check` pass, and so do `cargo clippy --all-targets` (0 warnings after removing `#[allow(dead_code)]`) and `cargo test -p eml_rt`.

**Files:**
- Modify: `crates/eml_rt/src/heap/tests/externs.rs` (module doc line 1; lines 75-77, 188-189, 270-271, 331, 639 comments and two test names; lines 690-811 `call` takes `name, inputs`, new `outcome`, `the_externs_match_the_table` checks the inputs, new `#[ignore]` writer `the_table_can_be_written`; line 816 `both_modes!` entry)
- Modify: `crates/eml_rt/src/heap/tests/table.rs` (lines 1-2 module doc, line 331 doc of `inputs`)
- Modify: `crates/eml_rt/src/heap/tests/mod.rs` (lines 32-33: drop the comment and `#[allow(dead_code)]`)
- Modify: `crates/eml_rt/src/heap/tests/ctx.rs` (lines 62-64, 319 comments)
- Modify: `crates/eml_rt/src/heap/tests/host.rs` (line 261 comment, "今の実装と同じ")
- Modify: `crates/eml_rt/src/error.rs` (lines 46, 70: delete two comments)
- Modify: `crates/eml_rt/src/externs.rs` (line 342 comment)
- Modify: `crates/eml_rt/src/heap/ctx.rs` (lines 46-47 doc of `string_bytes_copied`)
- Modify: `crates/eml_rt/src/heap/ctx/strings.rs` (lines 87-88 doc of `escape_byte`, lines 913-914 comment)
- Modify: `crates/eml_rt/src/heap/tests/expected.txt` (line 1 only, rewritten by the writer)
- Modify: `crates/eml_interp/tests/main.rs` (delete `mod rt_externs;`)
- Delete: `crates/eml_interp/tests/rt_externs.rs`
- Modify: `docs/implementation/testing.md` (line 80 phrase, line 128 one sentence, lines 132-146 「期待値の表」)
- Test: `crates/eml_rt/src/heap/tests/externs.rs`

**Interfaces:**
- Consumes: `table::{inputs, cases, line, digest, fixtures, Scratch, Case, Arg, Outcome}` in `crates/eml_rt/src/heap/tests/table.rs` (unchanged).
- Produces: `#[test] #[ignore] fn the_table_can_be_written()` in `crates/eml_rt/src/heap/tests/externs.rs`, run as `cargo test -p eml_rt the_table_can_be_written -- --ignored`; `fn outcome<M: Mode>(name: &str, inputs: &[Arg], root: &Path) -> Outcome` (private to the test module). Nothing in `crates/eml_rt/src` outside `heap/tests/` changes except comments.

- [ ] **Step 1: Pin the current behaviour**

Run: `cargo test -p eml_rt externs` and `cargo test -p eml_interp --test integration rt_externs`
Expected: both green. They pin the table this task must keep byte-identical.

- [ ] **Step 2: Refactor `call` and the table test, and add the writer**

In `crates/eml_rt/src/heap/tests/externs.rs`, change `call`'s doc and signature (lines 690-697) to:

```rust
/// 表の1つの場合の extern を呼び、結果を表の形に戻す
unsafe fn call<M: Mode>(
    ctx: &mut crate::Ctx<M>,
    name: &str,
    inputs: &[Arg],
    args: &mut Args,
    root: &Path,
    out: &Shared,
) -> Result<Outcome, Fault> {
```

and inside `call` make these four replacements: `Args::int(&case.args[i])` → `Args::int(&inputs[i])`; `args.string(ctx, &case.args[0], root)` → `args.string(ctx, &inputs[0], root)` (two sites); `args.string(ctx, &case.args[1], root)` → `args.string(ctx, &inputs[1], root)`; `Ok(match case.name.as_str() {` → `Ok(match name {`; `if case.name == "Std.Fs.read_all2" {` → `if name == "Std.Fs.read_all2" {`.

Replace the whole of `the_externs_match_the_table` (from its doc comment `` /// `eml_rt` の extern が、期待値の表のどの場合も同じ結果を返し、物体を残さない。… `` to its closing `}`, lines 772-811) with:

```rust
/// 表の1つの場合を新しい `Ctx` で動かし、結果を表の形に戻す。引数の定数の不死の物体のほかに、物体を残さないことも
/// 確かめる
fn outcome<M: Mode>(name: &str, inputs: &[Arg], root: &Path) -> Outcome {
    let out = Shared::default();
    let host = StdHost::new(
        Box::new(out.clone()),
        Box::new(io::sink()),
        root.to_path_buf(),
        Buffering::Line,
    );
    let mut ctx = crate::Ctx::<M>::new(
        Arc::new(Control::new(Limits::default())),
        HostRef::new(Arc::new(host)),
    );
    let mut args = Args::default();
    let got = unsafe { call(&mut ctx, name, inputs, &mut args, root, &out) }
        .unwrap_or_else(|fault| Outcome::Err(fault.to_string()));
    for w in args.release {
        unsafe { ctx.heap_mut().decref(w) }.unwrap();
    }
    assert_eq!(mortal(ctx.heap()), args.immortals, "{name} {inputs:?}");
    got
}

/// `eml_rt` の extern が、期待値の表のどの場合も同じ結果を返し、物体を残さない。表の入力は `table::inputs` と同じで、
/// 入力を変えて表を書き直していなければ落ちる
fn the_externs_match_the_table<M: Mode>() {
    let scratch = Scratch::new(&format!("eml_rt-table-{}", M::CHECKED));
    let root = scratch.path();
    let locked = table::fixtures(root);
    let cases = table::cases();
    let written: Vec<(String, Vec<Arg>)> = cases
        .iter()
        .map(|c| (c.name.clone(), c.args.clone()))
        .collect();
    let inputs = table::inputs();
    assert!(
        written == inputs,
        "expected.txt is not written from the current inputs ({} cases, {} inputs)",
        written.len(),
        inputs.len()
    );
    for case in cases {
        if !locked
            && case
                .args
                .iter()
                .any(|a| matches!(a, Arg::Rel(p) if p == "locked.txt"))
        {
            continue;
        }
        assert_eq!(
            table::digest(outcome::<M>(&case.name, &case.args, root)),
            case.outcome,
            "{}",
            table::line(&case)
        );
    }
}

/// 表を書く。行を足すか入力を変えたときだけ、`cargo test -p eml_rt the_table_can_be_written -- --ignored` で流し、
/// 差分の結果の欄を人が確かめる (docs/implementation/testing.md の「期待値の表」)。結果は検査モードで作る
#[test]
#[ignore = "writes expected.txt"]
fn the_table_can_be_written() {
    let scratch = Scratch::new("eml_rt-table-write");
    let root = scratch.path();
    assert!(
        table::fixtures(root),
        "cannot make an unreadable locked.txt (running as root?), so its row would be wrong"
    );
    let mut text =
        String::from("# eml_rt の extern の期待値の表。eml_rt の検査モードの結果から作った\n");
    for (name, args) in table::inputs() {
        let outcome = outcome::<crate::Checked>(&name, &args, root);
        text.push_str(&table::line(&Case {
            name,
            args,
            outcome,
        }));
        text.push('\n');
    }
    std::fs::write(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("src/heap/tests/expected.txt"),
        text,
    )
    .unwrap();
}
```

- [ ] **Step 3: Regenerate the table and check that only line 1 changes**

```bash
cargo test -p eml_rt the_table_can_be_written -- --ignored
git diff --no-ext-diff -U0 -- crates/eml_rt/src/heap/tests/expected.txt
```

Expected: `1 passed`, and the diff is exactly:

```
@@ -1 +1 @@
-# eml_rt の extern の期待値の表。今の実装 (eml_interp) の出力から作った
+# eml_rt の extern の期待値の表。eml_rt の検査モードの結果から作った
```

If any other line changes, stop. A changed result column means `eml_rt` and the CEK disagree, and both modes of `the_externs_match_the_table` would already have failed on it.

- [ ] **Step 4: Rewrite the eml_rt comments that name the CEK as the live reference**

`crates/eml_rt/src/heap/tests/externs.rs`:
- Line 1: `//! extern の単体テスト。結果と誤りの文言が spec のとおりであることを確かめる。`
- Lines 75-77: replace the doc and the name with

```rust
/// 文言は docs/spec/lir.md の「実行時エラー」の一覧のとおりである。
#[test]
fn faults_print_their_listed_texts() {
```

- Lines 188-189: `/// 写すバイトを数える。一意な左なら、その場で足しても作り直しても右の長さを、共有か不死の左なら両辺の長さを数える。` (one line; drop `/// (今のインタプリタと同じ)。`)
- Lines 270-271:

```rust
/// エスケープの規則は docs/spec/declarations.md の「Prelude のクラス」のとおりである。C1 の制御文字 (U+0080〜U+009F)
/// はそのまま写す。
fn strings_show_with_their_escapes<M: Mode>() {
```

  and in `both_modes!` replace `strings_show_with_the_interpreters_escapes,` with `strings_show_with_their_escapes,`
- Line 331: `/// 書けなくても文字列は手放す。`
- Line 639: ``     /// 文字列の引数の参照を1つ作る。定数は VM の `const` と同じく不死の物体にする ``

`crates/eml_rt/src/heap/tests/table.rs`:
- Lines 1-2:

```rust
//! 期待値の表の形と入力。`std` だけを使う。表は `eml_rt` の検査モードの結果から作る
//! (docs/implementation/testing.md の「期待値の表」)。
```

- Line 331: `` /// 表を作るための入力。結果の欄は `externs.rs` の書き手が埋める ``

`crates/eml_rt/src/heap/tests/mod.rs`: replace the two lines

```rust
// `eml_interp` のテストと分け合うファイルなので、表を作るための項目はここでは使わない
#[allow(dead_code)]
mod table;
```

with `mod table;`. Every item of `table.rs` is now used inside `eml_rt`.

`crates/eml_rt/src/heap/tests/ctx.rs`:
- Lines 62-64:

```rust
/// yield して、継続を2回組み立て直し、終える。最初の κ は関数の値 (`Word::imm(1)`) で、関数 0 の値は `()` と同じ語に
/// なるので使わない。
```

- Line 319: `` /// `ev_lookup` と `evv_mask` は、たどった `evv` の節点を `evidence_steps` に数える。 ``

`crates/eml_rt/src/heap/tests/host.rs` line 261: `    /// ディレクトリも開ける (読むときに誤りになる)。長さの見込みは返さない。ディレクトリを開けるのは` (only `。今の実装と同じ` is removed; line 262 stays).

`crates/eml_rt/src/error.rs`: delete the comment lines ``             // `eml_runtime` の `File` の検査と同じ文言にする `` (line 46) and ``             // `eml_runtime` の `release_fields` と同じ文言にする `` (line 70).

`crates/eml_rt/src/externs.rs` line 342: `// 文言は docs/spec/lir.md の「実行時エラー」の一覧と1文字も違わないようにする`

`crates/eml_rt/src/heap/ctx.rs` lines 46-47 (inside `verus!`):

```rust
    /// 同じである。左が共有か不死なら両辺の長さを数える。VM の `RunStats::string_bytes_copied` はこの数である。
```

`crates/eml_rt/src/heap/ctx/strings.rs`:
- Lines 87-88 (inside `verus!`):

```rust
/// `String.show` が1バイトを書く形。規則は文字の単位で決めてある (docs/spec/declarations.md の「Prelude のクラス」)。
/// UTF-8 の先頭と続きのバイトはどれも 0x80 以上なので、正しい UTF-8 ではバイトの単位で書いても同じ結果になる。
```

- Lines 913-914 (inside `verus!`):

```rust
            // 一意な左には、その場で足すか作り直すかによらず右の長さだけを数え、共有か不死の左には両辺を数える
            // (docs/spec/object-model.md の「仕事の回数」)
```

Check: `grep -rn -i -e eml_interp -e eml_runtime -e インタプリタ -e interpreter -e 今の実装 crates/eml_rt/src` prints nothing.

- [ ] **Step 5: Delete the CEK side of the table**

```bash
git rm crates/eml_interp/tests/rt_externs.rs
sed -i '/^mod rt_externs;$/d' crates/eml_interp/tests/main.rs
```

Check: `grep -rn rt_externs crates` prints nothing. Whatever Task 7 left in `crates/eml_interp/tests/main.rs` still compiles (`cargo test -p eml_interp`); Task 10 deletes the crate.

- [ ] **Step 6: Rewrite 「期待値の表」 in `docs/implementation/testing.md`**

Line 80: replace `期待値の表は extern が今の実装と同じ結果になることを、` with `期待値の表は extern の結果が変わらないことを、`.

Line 128: delete the sentence `今のインタプリタは誤りの後に漏れを確かめないので、期待値の表では比べられないためである。` The rest of the bullet stays.

Replace the section body (lines 134-146, from `` extern が今の実装 (`eml_interp` と `eml_runtime`) と同じ結果になることを、 `` to the bullet ending `「S10 VM と切り替え」)`) with:

```markdown
extern の結果を1つの期待値の表に固定し、`eml_rt` の回帰を防ぐ。表は `eml_rt` 自身の結果から作る。比べる独立した実装はないので、行を足すか入力を変えたときは、差分の結果の欄を人が確かめる。

- 表は `crates/eml_rt/src/heap/tests/expected.txt` で、1行が1つの場合である。タブで区切った extern の行の名前、引数、`=>` の後の期待する結果 (文字にした値か、位置を除いた誤りの文言) を持つ。今は 4025 の場合がある。4096 バイトを超える文字列の結果は、`h:<バイト長>:<FNV-1a 64 の16進>` と書く。表を小さく保つためである
- 表の形と入力は `crates/eml_rt/src/heap/tests/table.rs` に置く。`std` だけを使う
- 入力
  - 整数: 0、±1、±2、`MIN`、`MIN+1`、`MAX`、`MAX-1`、±2³¹、±2⁶² のすべての組み合わせと、種を固定した疑似乱数の組
  - 文字列: 空、ASCII、複数バイトの文字 (é、日本、絵文字)、制御文字、`"`、`\`、0x7F、U+0080、U+0085、U+009F、U+2028、NUL、互いに前置きになる組と、種を固定した疑似乱数の列
  - `++` の左: 一意なもの、`dup` で共有したもの、不死の定数
  - ファイル: 一時ディレクトリに作り、`file_root` からの相対パスで開く。ふつうのファイル、空のファイル、UTF-8 でないもの、ないもの、読めないもの (Unix の 000。root なら飛ばす)、ディレクトリ、NUL を含むパス、64 KiB の区切りをまたぐもの (1 MiB)、絶対パス、2回目の `Fs.read_all`
- テスト `the_externs_match_the_table` (`heap/tests/externs.rs`) は、両方のモードで表のどの場合も同じ結果になり、引数の定数のほかに物体が残らないことを確かめる。表の入力が `table.rs` の入力と違えば落ちるので、古い表は残らない
- 表は、同じファイルの `#[ignore]` のテスト `the_table_can_be_written` が、検査モードの結果で書く。入力を変えたときは `cargo test -p eml_rt the_table_can_be_written -- --ignored` で書き直し、`git diff --no-ext-diff` で結果の欄を確かめる
```

- [ ] **Step 7: Run the full gate including Verus**

```bash
cargo test
cargo clippy --all-targets
cargo fmt --check
cargo verus verify -p eml_rt
verusfmt --verus-only --check $(git ls-files 'crates/eml_rt/src/*.rs')
cargo test -p eml_cli --test integration citations
```

Expected: all green (the citations test checks `docs/spec/lir.md の「実行時エラー」`, `docs/spec/declarations.md の「Prelude のクラス」`, `docs/spec/object-model.md の「仕事の回数」` and `docs/implementation/testing.md の「期待値の表」`, which all exist). `cargo verus verify -p eml_rt` takes about 35 s; only comments inside `verus!` changed.

- [ ] **Step 8: Commit**

```bash
git add crates/eml_rt crates/eml_interp/tests docs/implementation/testing.md
git commit -m "Write the extern expected-value table from eml_rt" -m "The CEK goes away in this stage, so expected.txt can no longer be written from eml_interp. The #[ignore] writer moves into eml_rt (the_table_can_be_written) and fills the result column from eml_rt's own externs in checked mode. The table becomes eml_rt's regression table, and a person checks the result column of the diff when rows or inputs change (spec, 期待値の表). Regenerating it changes only the comment on line 1; all 4025 rows are byte-identical. the_externs_match_the_table now also checks that the table was written from the current inputs, the check the deleted CEK test used to make. eml_rt comments that named the CEK as the live reference now cite the spec instead.

Test changes: pass/fail (kind 1): deleted crates/eml_interp/tests/rt_externs.rs with the_interpreter_matches_the_table, a_failed_write_reads_the_same_on_both_sides, tags_agree_with_eml_lir and the writer the_table_can_be_written_from_the_interpreter (replaced by eml_rt's the_table_can_be_written), as the spec lists. Mechanical (kind 3): renamed faults_print_as_the_interpreter_does to faults_print_their_listed_texts and strings_show_with_the_interpreters_escapes to strings_show_with_their_escapes, refactored call/outcome, and rewrote line 1 of expected.txt; no expected value changes." -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01MK52fNp5uA711Q2D4kWEtR"
```

### Task 10: 切り替え (`eml run` を VM に、`eml_interp` と `eml_runtime` を消す)

One commit. The steps must land together: `eml_test_support` names `eml_interp` types until it is switched, and the CEK crates cannot be deleted while anything names them. Verified in a scratch copy at HEAD 7d1bf441 with Tasks 8-9 applied: `cargo test` (every crate), `cargo clippy --all-targets` (0 warnings), `cargo fmt --check` and the feature combinations all pass. No snapshot changed: the S10a harness already required the VM's output and error text to equal the CEK's for every UI test. Line numbers are at HEAD 7d1bf441.

Boundary with Task 14: this task removes every reference to `eml_interp`, `eml_runtime`, `OutputSink`, `execute_vm`, `run_vm`, `run_stats_vm` and `--vm` that the compiler or the binary sees: manifests, `use`, items, tests and the CLI flag. Comments that only mention the CEK (`crates/eml_vm/src/{error.rs,lib.rs}`, `crates/eml_vm/tests/{bench.rs,evidence.rs}`, `crates/eml_core_ir/src/translate/expr.rs`, `crates/eml_core_ir/tests/translate.rs`, and the others in the spec's 「文書」 table) and all of `docs/`, `CLAUDE.md`, `README.md` stay for Task 14. `bench/run.sh` still passes `--vm` and stops working at this commit; Task 13 rewrites it, and it is not part of the gate.

**Files:**
- Delete: `crates/eml_interp/` (whole crate), `crates/eml_runtime/` (whole crate)
- Modify: `Cargo.toml` (lines 18 and 20: drop `eml_runtime` and `eml_interp` from `[workspace.dependencies]`; `members = ["crates/*"]` needs no change)
- Modify: `Cargo.lock` (cargo drops the two packages and their edges when it next builds)
- Modify: `crates/eml_cli/Cargo.toml` (line 26 `run` feature; lines 33, 35 dependencies)
- Modify: `crates/eml_test_support/Cargo.toml` (line 24 `run` feature; lines 31, 33 dependencies)
- Modify: `crates/eml_cli/src/lib.rs` (lines 14-17 re-exports; lines 218-242 `execute`, `execute_vm`, the interim aliases)
- Modify: `crates/eml_cli/src/main.rs` (line 6 import; lines 25-27 the `vm` flag; lines 45-82 the `Run` arm)
- Modify: `crates/eml_test_support/src/lib.rs` (lines 15-18 imports; lines 270-335 the run functions)
- Modify: `crates/eml_cli/tests/ui.rs` (lines 9-12 imports; lines 117-200 harness; lines 210-212 and 225-235 the two glob tests)
- Modify: `crates/eml_cli/tests/api.rs` (lines 1-3 imports; lines 90-101 `execute_runs_a_compiled_program`; lines 274-281 the renamed test)
- Modify: `crates/eml_cli/tests/cli.rs` (lines 184-205 the two `--vm` tests)
- Modify: every file under `crates/eml_vm/tests/` that calls `execute_vm`, `run_vm` or `run_stats_vm` (at HEAD `common/mod.rs` and `bench.rs`; after Task 7 also `scaling.rs` and `run.rs`)
- Test: `crates/eml_cli/tests/{ui.rs,api.rs,cli.rs}`, `crates/eml_test_support/tests/support.rs` (unchanged; it now runs on the VM)

**Interfaces:**
- Consumes: Task 8's `eml_vm::{RunConfig, RuntimeError, execute, Control, Limits, Stop}`, the interim `eml_test_support::{execute_vm, run_vm, run_stats_vm}` and `eml_cli::{execute_vm, VmRunConfig, VmRunStats, VmRuntimeError}`.
- Produces (final API):
  - `eml_cli`: `pub use eml_vm::{Capture, Control, HostRef, Limits, RunConfig, RunStats, RuntimeError, SourceLocation, Stop, VmFault, capture_host, stdio_host}` and `pub fn execute(program: &eml_lir::Program, config: &RunConfig, host: HostRef) -> Result<RunStats, RuntimeError>`. `OutputSink`, `Captured`, `execute_vm`, `VmRunStats`, `VmRunConfig` and `VmRuntimeError` are gone.
  - `eml_test_support`: `run(text) -> (String, Result<(), RuntimeError>)`, `run_files(entry, modules) -> (String, Result<(), RuntimeError>)`, `run_stats(text) -> (String, Result<RunStats, RuntimeError>)` (all checked mode, fresh unbounded `Control`, `file_root` empty) and `execute(program: &eml_lir::Program, config: &RunConfig) -> (String, Result<RunStats, RuntimeError>)`, with the `eml_vm` types. The `*_vm` names are gone.
  - `eml run [--debug-heap] <file>` runs on the VM; `--vm` is a usage error (exit 2).

- [ ] **Step 1: Rename the tests in `crates/eml_cli/tests/cli.rs` to the final behaviour (failing first)**

Replace the two tests at lines 184-205 with:

```rust
#[test]
fn run_in_fast_mode_prints_the_program_output() {
    let output = eml(&["run", "run/basics/hello.em"]);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert_eq!(String::from_utf8_lossy(&output.stdout), "Hello, world!\n");
}

#[test]
fn a_runtime_error_in_fast_mode_exits_with_one() {
    let output = eml(&["run", "run-fail/basics/division_by_zero.em"]);
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.starts_with("runtime error: division by zero\n  at "),
        "{stderr}"
    );
}
```

In `crates/eml_cli/tests/api.rs`, replace `execute_runs_a_compiled_program` (lines 90-101) with:

```rust
#[test]
fn execute_runs_a_compiled_program() {
    let program = single("main : Unit -> <IO> Unit\nmain () = ()")
        .compile()
        .program
        .unwrap();
    let (host, _) = capture_host(std::path::PathBuf::new());
    assert_eq!(
        execute(&program, &RunConfig::default(), host).map(|_| ()),
        Ok(())
    );
}
```

and the last test (lines 274-281) with:

```rust
#[test]
fn execute_counts_its_work() {
    let program = single(TWICE).compile().program.unwrap();
    let (host, output) = capture_host(std::path::PathBuf::new());
    let config = RunConfig::default().with_debug_heap(true);
    let stats: eml_cli::RunStats = execute(&program, &config, host).unwrap();
    assert_eq!(output.contents(), "xx\n");
    assert!(stats.string_bytes_copied > 0);
}
```

Change the import at lines 1-3 to:

```rust
use eml_cli::{
    FsProvider, ModulePath, ModuleSource, ReadError, RunConfig, Session, capture_host, execute,
};
```

Run: `cargo test -p eml_cli --test integration api::execute`
Expected: compile error (`execute` still takes an `OutputSink`, `capture_host` returns a `HostRef`). This pins the new `eml_cli::execute` signature.

- [ ] **Step 2: Switch `eml_cli`'s lib and binary to the VM**

`crates/eml_cli/Cargo.toml`: replace `run = ["core", "dep:eml_interp", "dep:eml_runtime", "dep:eml_vm"]` with `run = ["core", "dep:eml_vm"]`, and delete the lines `eml_interp = { workspace = true, optional = true }` and `eml_runtime = { workspace = true, optional = true }`.

`crates/eml_cli/src/lib.rs`: replace lines 14-17

```rust
#[cfg(feature = "run")]
pub use eml_interp::{RunConfig, RunStats, RuntimeError};
#[cfg(feature = "run")]
pub use eml_runtime::{Captured, OutputSink};
```

with

```rust
#[cfg(feature = "run")]
pub use eml_vm::{
    Capture, Control, HostRef, Limits, RunConfig, RunStats, RuntimeError, SourceLocation, Stop,
    VmFault, capture_host, stdio_host,
};
```

and replace everything from `` /// 仕事の回数 (`RunStats`) はテストが比べるためのもので、CLI は使わない。 `` (line 218) to the end of the file with:

```rust
/// 仕事の回数 (`RunStats`) はテストが比べるためのもので、CLI は使わない。
#[cfg(feature = "run")]
pub fn execute(
    program: &eml_lir::Program,
    config: &RunConfig,
    host: HostRef,
) -> Result<RunStats, RuntimeError> {
    eml_vm::execute(program, config, host)
}
```

`crates/eml_cli/src/main.rs`: change line 6 to `use eml_cli::{FsProvider, RunConfig, Session, stdio_host};`; delete the three lines of the `vm` field (lines 25-27: the doc `/// Run on the VM instead of the CEK interpreter (S10a only)`, `#[arg(long, hide = true)]`, `vm: bool,`); replace the head of the `Run` arm `Command::Run { debug_heap, vm, file, } => {` with `Command::Run { debug_heap, file } => {`; and replace the block from `let result = if vm {` through `Ok(()) => ExitCode::SUCCESS,` with:

```rust
            // 相対パスはカレントディレクトリを基準にする
            match eml_cli::execute(&program, &config, stdio_host(PathBuf::new())) {
                Ok(_) => ExitCode::SUCCESS,
```

The `Err(error)` arm (`eprintln!("runtime error: {error}"); ExitCode::from(1)`) stays as it is; Task 11 adds exit code 130. The arm now reads:

```rust
        Command::Run { debug_heap, file } => {
            let Some(session) = load(&file) else {
                return ExitCode::from(2);
            };
            let config = RunConfig::default().with_debug_heap(debug_heap);
            // 警告がプログラムの出力の後に出ないように、実行の前に表示する。
            let compiled = session.compile();
            eprint!("{}", render(&compiled.diagnostics, session.files()));
            let Some(program) = compiled.program else {
                return ExitCode::from(1);
            };
            // 相対パスはカレントディレクトリを基準にする
            match eml_cli::execute(&program, &config, stdio_host(PathBuf::new())) {
                Ok(_) => ExitCode::SUCCESS,
                Err(error) => {
                    eprintln!("runtime error: {error}");
                    ExitCode::from(1)
                }
            }
        }
```

- [ ] **Step 3: Unify `eml_test_support` on the VM**

`crates/eml_test_support/Cargo.toml`: replace `run = ["core", "dep:eml_interp", "dep:eml_runtime", "dep:eml_vm", "eml_cli/run"]` with `run = ["core", "dep:eml_vm", "eml_cli/run"]`, and delete the lines `eml_interp = { workspace = true, optional = true }` and `eml_runtime = { workspace = true, optional = true }`.

`crates/eml_test_support/src/lib.rs`: replace lines 15-18

```rust
#[cfg(feature = "run")]
use eml_interp::{RunConfig, RunStats, RuntimeError};
#[cfg(feature = "run")]
use eml_runtime::OutputSink;
```

with

```rust
#[cfg(feature = "run")]
use eml_vm::{RunConfig, RunStats, RuntimeError};
```

and replace everything from `` /// 実行のテストでは、つねに `debug_heap` を有効にする (docs/implementation/testing.md)。 `` (line 270) up to, not including, `` /// 1件を `E0001 1:2 message` の1行にする。 `` with:

```rust
/// 実行のテストは、つねに検査モードで流す (docs/implementation/testing.md)。
#[cfg(feature = "run")]
pub fn run(text: &str) -> (String, Result<(), RuntimeError>) {
    run_files(text, &[])
}

#[cfg(feature = "run")]
pub fn run_files(entry: &str, modules: &[(&str, &str)]) -> (String, Result<(), RuntimeError>) {
    let (out, result) = execute(&lir_files(entry, modules), &checked());
    (out, result.map(|_| ()))
}

/// `RunStats` (仕事の回数と、ヒープの物体とスタックの語の数の最大) も返す。数を比べるテスト (`eml_vm` の
/// `scaling.rs` と `bench.rs`) のため。
#[cfg(feature = "run")]
pub fn run_stats(text: &str) -> (String, Result<RunStats, RuntimeError>) {
    execute(&lir(text), &checked())
}

/// 上限のない新しい `Control` を持つ、検査モードの設定。止める理由のビットは `Control::clear` でしか下りないので、
/// 設定を使い回すと前の実行で立ったビットで次の実行が止まる。そのため実行ごとに作る。
#[cfg(feature = "run")]
fn checked() -> RunConfig {
    RunConfig::default().with_debug_heap(true)
}

/// LIR を実行する。出力はメモリの Host に捕まえ、相対パスはカレントディレクトリを基準にする。上限の値に頼るテストは、
/// `RunConfig` を自分で作って渡す。
#[cfg(feature = "run")]
pub fn execute(
    program: &eml_lir::Program,
    config: &RunConfig,
) -> (String, Result<RunStats, RuntimeError>) {
    let (host, out) = eml_cli::capture_host(std::path::PathBuf::new());
    let result = eml_cli::execute(program, config, host);
    (out.contents(), result)
}
```

`run_stats` reads `peak_stack`, which Task 5 added, so the doc mentions the stack.

- [ ] **Step 4: Move every test caller onto the unified names**

```bash
grep -rlE '\b(execute_vm|run_vm|run_stats_vm)\b' crates/eml_vm/tests | xargs perl -pi -e 's/\bexecute_vm\b/execute/g; s/\brun_stats_vm\b/run_stats/g; s/\brun_vm\b/run/g'
```

At HEAD this rewrites `crates/eml_vm/tests/common/mod.rs` (`use eml_test_support::execute;` and its three calls) and `crates/eml_vm/tests/bench.rs` (`use eml_test_support::run_stats;`); after Task 7 it also rewrites `scaling.rs` and `run.rs`. None of these files declares its own `execute`, `run` or `run_stats`, and `miri.rs` (which imports `eml_vm::execute`) has no `*_vm` call, so no name collides. `cargo build --tests` in Step 6 catches a collision if Task 7 introduced one.

- [ ] **Step 5: Make the UI harness run checked mode against the snapshot, then fast mode**

`crates/eml_cli/tests/ui.rs`: change the imports (lines 7-12) to

```rust
use std::sync::{Mutex, MutexGuard, PoisonError};

use eml_cli::{FsProvider, RunConfig, RuntimeError, Session, capture_host, execute};
```

Replace `compile_and_execute`, `check_on_vm` and `assert_same_counts` (from `` /// 診断のエラーなしでコンパイルし、`debug_heap` を有効にして CEK で実行する。 `` to the closing `}` of `assert_same_counts`) with:

```rust
/// 診断のエラーなしでコンパイルし、LIR と、入力のファイルを置くディレクトリと、診断の表示を返す。入力のファイルは
/// テストの隣に置く (docs/implementation/testing.md の「UI テスト」)。
fn compile(path: &Path, suffix: &str) -> (eml_lir::Program, PathBuf, String) {
    let session = load(path, suffix);
    let compiled = session.compile();
    let stderr = render(&compiled.diagnostics, session.files());
    let program = compiled
        .program
        .unwrap_or_else(|| panic!("unexpected errors:\n{stderr}"));
    let root = path
        .parent()
        .expect("a test file has a directory")
        .to_path_buf();
    (program, root, stderr)
}

/// 止める理由のビットは `Control::clear` でしか下りないので、実行ごとに新しい `Control` を持つ設定を作る。使い回すと、
/// 前の実行で立ったビットで次の実行がすぐ止まる。
fn execute_in(
    program: &eml_lir::Program,
    root: &Path,
    debug_heap: bool,
) -> (String, Result<(), RuntimeError>) {
    let (host, out) = capture_host(root.to_path_buf());
    let config = RunConfig::default().with_debug_heap(debug_heap);
    let result = execute(program, &config, host).map(drop);
    (out.contents(), result)
}

/// 検査モードの結果をスナップショットと比べた後で、速いモードでも流し、同じ出力と同じ結果になることを確かめる。
/// 速いモードは前提を確かめないので、VM の誤りが未定義の動作になって glob のテスト全体を壊しうる。そのため検査モードの
/// 後に流す (docs/implementation/testing.md の「VM でも流す」)。
fn assert_fast_mode_agrees(
    suffix: &str,
    program: &eml_lir::Program,
    root: &Path,
    stdout: &str,
    result: &Result<(), RuntimeError>,
) {
    let (fast_stdout, fast) = execute_in(program, root, false);
    assert_eq!(
        fast_stdout, stdout,
        "{suffix}: the fast mode printed something else"
    );
    assert_eq!(
        fast.map_err(|error| error.to_string()),
        result.as_ref().map(|_| ()).map_err(ToString::to_string),
        "{suffix}: the fast mode ended otherwise"
    );
}
```

In `fn run()`, replace the glob body after the `classify` guard with:

```rust
        let (program, root, stderr) = compile(path, &suffix);
        let (stdout, result) = execute_in(&program, &root, true);
        assert_eq!(result, Ok(()), "{stderr}");
        snapshot_settings(&suffix).bind(|| {
            insta::assert_snapshot!(format!("--- stdout ---\n{stdout}--- stderr ---\n{stderr}"));
        });
        assert_fast_mode_agrees(&suffix, &program, &root, &stdout, &result);
```

In `fn run_fail()`, replace it with:

```rust
        let (program, root, _) = compile(path, &suffix);
        let (stdout, result) = execute_in(&program, &root, true);
        let Err(error) = &result else {
            panic!("expected a runtime error");
        };
        snapshot_settings(&suffix).bind(|| {
            insta::assert_snapshot!(format!(
                "--- stdout ---\n{stdout}--- runtime error ---\n{error}\n"
            ));
        });
        assert_fast_mode_agrees(&suffix, &program, &root, &stdout, &result);
```

`RunConfig::default()` here has an unbounded `Control`; Task 11 switches `execute_in` to `Control::new(default_limits())`.

- [ ] **Step 6: Delete the two crates**

```bash
git rm -r -q crates/eml_interp crates/eml_runtime
sed -i '/^eml_runtime = { path = "crates\/eml_runtime" }$/d; /^eml_interp = { path = "crates\/eml_interp" }$/d' Cargo.toml
cargo fmt
cargo build --tests
```

Expected: builds with no warnings. `Cargo.lock` loses the `eml_interp` and `eml_runtime` packages and their entries in the dependency lists of `eml_cli` and `eml_test_support` (20 lines). Check:

```bash
git grep -n -e eml_interp -e eml_runtime -e OutputSink -e execute_vm -e run_vm -e run_stats_vm -e VmRunStats -e VmRunConfig -e VmRuntimeError -- Cargo.toml Cargo.lock crates | grep -v '^\S*:[0-9]*:\s*//'
```

Expected: no output (checked in the scratch copy). Lines that remain only in comments belong to Task 14 (see the boundary note above).

- [ ] **Step 7: Run the tests and accept only the allowed expected-value changes**

Run: `cargo test`
Expected: green with no snapshot diff. In S10a every UI test already had to match the CEK's output and error text on the VM, and `eml_test_support::run` callers outside `eml_interp` compare only `Ok(())` or output.

If a snapshot does fail, it is an expected-value change (kind 2) only within the spec's scope: the wording of `Heap(_)` (now `eml_rt::HeapError`'s Display) and of internal errors. Procedure:

```bash
cargo insta test --workspace --accept
git diff --no-ext-diff --stat
git diff --no-ext-diff -- crates/eml_cli/tests/snapshots 'crates/*/tests/*.rs'
```

Accept a change only if all of these hold. It is in an `integration__ui__run_fail@*.snap` file or in an inline snapshot of an `eml_test_support::run*` caller. Only the line after `--- runtime error ---` (or the error text of an inline snapshot) changed. The new text is a `HeapError` message from `crates/eml_rt/src/error.rs` or starts with `internal error:`. No `--- stdout ---` section changed, and no `run/` or `check-fail/` snapshot changed. Any other diff, and any `the fast mode printed something else` / `the fast mode ended otherwise` failure, is a bug: revert with `git checkout -- <file>` and fix the VM instead. List each accepted file in the commit message.

Run: `cargo test -p eml_cli --test integration cli::`
Expected: the two renamed tests pass. `eml run --vm x.em` now exits 2 with clap's `unexpected argument '--vm' found` (`missing_arguments_are_a_usage_error` already pins exit 2 for unknown arguments, so no new test is needed).

- [ ] **Step 8: Run the full gate**

```bash
cargo test
cargo clippy --all-targets
cargo fmt --check
cargo clippy -p eml_cli --no-default-features
cargo clippy -p eml_cli --no-default-features --features types
cargo clippy -p eml_cli --no-default-features --features core
cargo clippy -p eml_test_support --no-default-features --features hir
cargo clippy -p eml_test_support --no-default-features --features core
nix build
```

Expected: all green, 0 warnings. `eml_vm/src` does not change in this task, so Miri is not needed.

- [ ] **Step 9: Commit**

```bash
git add -A Cargo.toml Cargo.lock crates
git commit -m "Switch eml run to the VM and delete eml_interp and eml_runtime" -m "eml run [--debug-heap] now always runs on the VM, and --vm is gone. eml_cli re-exports RunConfig, RunStats, RuntimeError, VmFault, SourceLocation, Control, Limits, Stop, HostRef, Capture, capture_host and stdio_host from eml_vm, and execute takes a HostRef. eml_test_support's run, run_files, run_stats and execute run on the VM (run* in checked mode with a fresh unbounded Control) and the *_vm names are gone. The UI harness runs checked mode against the snapshot and then fast mode, each with a fresh RunConfig because a Control's stop bits persist until Control::clear. The crates eml_interp and eml_runtime are deleted (spec, 作業の順 5).

Test changes: pass/fail (kind 1): every test left in crates/eml_interp and all 43 unit tests of crates/eml_runtime are deleted with their crates, as the spec lists them (Task 7 already moved the ones that survive). Mechanical (kind 3): cli.rs run_on_the_vm_prints_the_program_output -> run_in_fast_mode_prints_the_program_output and a_runtime_error_on_the_vm_exits_with_one -> a_runtime_error_in_fast_mode_exits_with_one without --vm (and without --debug-heap, so both test fast mode); api.rs execute_runs_a_compiled_program rewritten on capture_host and RunConfig, execute_vm_runs_a_compiled_program_and_counts_its_work renamed execute_counts_its_work; eml_vm tests renamed from execute_vm/run_vm/run_stats_vm to execute/run/run_stats; the UI harness no longer compares with the CEK (check_on_vm, assert_same_counts removed). Expected values (kind 2): none changed." -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01MK52fNp5uA711Q2D4kWEtR"
```

If Step 7 accepted any snapshot, replace `Expected values (kind 2): none changed.` with `Expected values (kind 2): <files>, Heap(_) / internal error wording now from eml_rt (spec, UI テスト).`

### Task 11: 上限の既定の値と SIGINT

Verified in a scratch copy at HEAD 7d1bf441 with Tasks 8-10 applied: `cargo test`, `cargo clippy --all-targets` (0 warnings), `cargo fmt --check` and `nix build` pass. The two SIGINT tests passed 15 runs in a row (about 0.15 s each).

**Files:**
- Modify: `Cargo.toml` (`[workspace.dependencies]`: add `ctrlc = "3.5"` after `clap`, `libc = "0.2"` after `la-arena`)
- Modify: `Cargo.lock` (adds `ctrlc` 3.5.x and its dependencies; `libc` 0.2.190 is already there)
- Modify: `crates/eml_cli/Cargo.toml` (`run` feature adds `dep:ctrlc`, `dep:libc`; two optional dependencies)
- Modify: `crates/eml_cli/src/lib.rs` (append `default_limits` and `physical_memory`)
- Modify: `crates/eml_cli/src/main.rs` (imports; the `Run` arm builds the `Control` and installs the handler before `load`; exit code 130; new `interrupt_on_sigint`)
- Modify: `crates/eml_cli/tests/ui.rs` (`execute_in` uses `Control::new(default_limits())`)
- Test: `crates/eml_cli/tests/api.rs` (`default_limits_bound_the_depth_and_the_memory`), `crates/eml_cli/tests/cli.rs` (`an_interrupt_stops_a_running_program_with_130`, `repeated_interrupts_end_a_program_waiting_in_a_read_with_130`)

**Interfaces:**
- Consumes: Task 10's `eml_cli::{Control, Limits, RunConfig, RuntimeError, execute, stdio_host}`, Task 8's `RuntimeError::interrupted`, `eml_rt`'s `Control::{new, interrupt, stop_bits, INTERRUPT}` and `Limits { max_bytes, max_depth, max_evv }`; Task 5's `Task::reserve` cap (`min(2 × capacity, max_depth + u16::MAX + 2)` words), which makes `max_depth` bound the stack's memory.
- Produces: `#[cfg(feature = "run")] pub fn eml_cli::default_limits() -> Limits` (`max_depth = 1 << 27`, `max_bytes` = 80% of physical memory or `usize::MAX`, `max_evv = usize::MAX`); `eml run` exits 130 when `RuntimeError::interrupted()` holds.

- [ ] **Step 1: Write the failing `default_limits` test**

Append to `crates/eml_cli/tests/api.rs`:

```rust
/// 深さの上限は 2^27 語である。確保量の上限は物理メモリの 80% で、物理メモリを読める macOS と Linux では有限である。
/// 既定の上限まで実際に再帰するテストは 1 GiB を使うので置かない。
#[test]
fn default_limits_bound_the_depth_and_the_memory() {
    let limits = eml_cli::default_limits();
    assert_eq!(limits.max_depth, 1 << 27);
    assert_eq!(limits.max_evv, usize::MAX);
    if cfg!(any(target_os = "macos", target_os = "linux")) {
        assert!(limits.max_bytes > 0);
        assert!(limits.max_bytes < usize::MAX);
    }
}
```

Run: `cargo test -p eml_cli --test integration api::default_limits`
Expected: compile error `cannot find function default_limits in crate eml_cli`.

- [ ] **Step 2: Add the dependencies**

In the root `Cargo.toml`, `[workspace.dependencies]`, add the line `ctrlc = "3.5"` after `clap = { version = "4.6", features = ["derive"] }` and `libc = "0.2"` after `la-arena = "0.3"`.

In `crates/eml_cli/Cargo.toml`, set `run = ["core", "dep:eml_vm", "dep:ctrlc", "dep:libc"]`, add `ctrlc = { workspace = true, optional = true }` after `clap.workspace = true`, and add `libc = { workspace = true, optional = true }` after `eml_vm = { workspace = true, optional = true }`.

Run: `cargo build -p eml_cli`
Expected: cargo resolves `ctrlc v3.5.x` and builds; `git diff --no-ext-diff --stat Cargo.lock` shows only added lines (ctrlc and its dependencies, and `ctrlc`/`libc` in `eml_cli`'s list).

- [ ] **Step 3: Implement `default_limits`**

Append to `crates/eml_cli/src/lib.rs`:

```rust
/// `eml run` の既定の上限。深さは 2^27 語 (1 GiB 分のスタック) で、OCaml 5 のスタックの既定の上限と同じ量である。
/// 確保量は物理メモリの 80% で、機械を使い切る前に分かる誤りで止める。物理メモリを読めない OS では上限を置かない。
/// `evv` はヒープの物体なので、確保量の上限で抑えられる (docs/spec/object-model.md の「実行の制御」)。
#[cfg(feature = "run")]
pub fn default_limits() -> Limits {
    Limits {
        max_depth: 1 << 27,
        max_bytes: physical_memory().map_or(usize::MAX, |bytes| bytes / 5 * 4),
        ..Limits::default()
    }
}

/// `eml_rt` は vstd だけに依存したままにするため、OS に問い合わせるのはここである。`src/` の `unsafe` の呼び出しは
/// この関数の中だけに置く。
#[cfg(all(feature = "run", unix))]
fn physical_memory() -> Option<usize> {
    // sysconf は名前のほかに前提を持たず、読めなければ -1 を返す
    let (pages, page_size) = unsafe {
        (
            libc::sysconf(libc::_SC_PHYS_PAGES),
            libc::sysconf(libc::_SC_PAGESIZE),
        )
    };
    usize::try_from(pages)
        .ok()?
        .checked_mul(usize::try_from(page_size).ok()?)
}

#[cfg(all(feature = "run", not(unix)))]
fn physical_memory() -> Option<usize> {
    None
}
```

`bytes / 5 * 4` cannot overflow; `Limits::default()` leaves `max_evv` at `usize::MAX`.

Run: `cargo test -p eml_cli --test integration api::default_limits`
Expected: `1 passed`.

- [ ] **Step 4: Write the two failing SIGINT tests**

Append to `crates/eml_cli/tests/cli.rs` (the file already imports `std::path::Path`, `std::process::Command` and `crate::common::temp_project`):

```rust
/// 終わらない自己末尾呼び出しのループで、行を出力し続ける。
#[cfg(unix)]
const SPIN: &str = "\
spin : Int -> <IO> Unit
spin n =
  println (show n)
  spin (n + 1)

main : Unit -> <IO> Unit
main () = spin 0
";

/// 名前付きパイプ `fifo` を開いて全部読む。書き手が何も書かずに開いたままなら、読みの中で待ち続ける。
#[cfg(unix)]
const READ_FIFO: &str = "\
main : Unit -> <IO> Unit
main () =
  let f = Fs.open \"fifo\"
  let (f, text) = Fs.read_all f
  Fs.close f
  println text
";

/// テストが止まらずに CI を待たせないための締め切り。
#[cfg(unix)]
const DEADLINE: std::time::Duration = std::time::Duration::from_secs(10);

#[cfg(unix)]
fn spawn_run(dir: &Path, stdout: std::process::Stdio) -> std::process::Child {
    Command::new(env!("CARGO_BIN_EXE_eml"))
        .args(["run", "main.em"])
        .current_dir(dir)
        .stdout(stdout)
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap()
}

#[cfg(unix)]
fn sigint(child: &std::process::Child) {
    let pid = libc::pid_t::try_from(child.id()).unwrap();
    // 子はまだ待っていないので、終わっていても pid はほかのプロセスに使い回されていない
    assert_eq!(unsafe { libc::kill(pid, libc::SIGINT) }, 0);
}

/// 締め切りを過ぎたら子を殺して、テストを失敗にする。
#[cfg(unix)]
fn kill_after_deadline(child: &mut std::process::Child, what: &str) -> ! {
    let _ = child.kill();
    let _ = child.wait();
    panic!("{what} within {DEADLINE:?}");
}

#[cfg(unix)]
fn stderr_of(child: &mut std::process::Child) -> String {
    use std::io::Read;
    let mut stderr = String::new();
    child
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut stderr)
        .unwrap();
    stderr
}

/// 1回目の SIGINT で、VM は次の安全点で止まり、終了コード 130 で終わる。標準出力はパイプなので 64 KiB ごとにしか
/// 書かれない。パイプが詰まると子は `write` の中で止まって安全点に来ないので、別のスレッドで読み続ける。
#[cfg(unix)]
#[test]
fn an_interrupt_stops_a_running_program_with_130() {
    use std::io::Read;
    use std::time::Instant;

    let dir = temp_project("cli-sigint-loop", &[("main.em", SPIN)]);
    let deadline = Instant::now() + DEADLINE;
    let mut child = spawn_run(&dir, std::process::Stdio::piped());
    let mut stdout = child.stdout.take().unwrap();
    let (first_output, arrived) = std::sync::mpsc::channel();
    let reader = std::thread::spawn(move || {
        let mut buffer = vec![0; 64 * 1024];
        while let Ok(n @ 1..) = stdout.read(&mut buffer) {
            let _ = first_output.send(n);
        }
    });
    if arrived
        .recv_timeout(deadline.saturating_duration_since(Instant::now()))
        .is_err()
    {
        kill_after_deadline(&mut child, "no output arrived");
    }
    sigint(&child);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            kill_after_deadline(&mut child, "the program did not stop");
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    };
    reader.join().unwrap();
    let stderr = stderr_of(&mut child);
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(status.code(), Some(130), "{stderr}");
    assert!(stderr.contains("interrupted"), "{stderr}");
}

/// 読みの中で待つプログラムは安全点に来ない。2回目の SIGINT でハンドラがその場で終わらせ、終了コードは 130 である。
/// 続けて届いた2つの SIGINT は1つにまとまることがあるので、終わるまで 100 ms ごとに送り続ける。
#[cfg(unix)]
#[test]
fn repeated_interrupts_end_a_program_waiting_in_a_read_with_130() {
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::OpenOptionsExt;
    use std::time::{Duration, Instant};

    let dir = temp_project("cli-sigint-fifo", &[("main.em", READ_FIFO)]);
    let fifo = dir.join("fifo");
    let path = std::ffi::CString::new(fifo.as_os_str().as_bytes()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
    let deadline = Instant::now() + DEADLINE;
    let mut child = spawn_run(&dir, std::process::Stdio::null());
    // 子の `Fs.open` は書き手が開くまで待つ。読み手のいないパイプを書き込み用に非同期で開くと ENXIO なので、開けた
    // 時点で子は実行を始めている。何も書かずに開いたままにして、子を読みの中で待たせる
    let writer = loop {
        match std::fs::OpenOptions::new()
            .write(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(&fifo)
        {
            Ok(writer) => break writer,
            Err(error) if error.raw_os_error() == Some(libc::ENXIO) => {
                if let Some(status) = child.try_wait().unwrap() {
                    panic!("the program ended before it opened the pipe: {status}");
                }
                if Instant::now() >= deadline {
                    kill_after_deadline(&mut child, "the program did not open the pipe");
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(error) => panic!("{error}"),
        }
    };
    let status = loop {
        sigint(&child);
        std::thread::sleep(Duration::from_millis(100));
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            kill_after_deadline(&mut child, "the program did not end");
        }
    };
    drop(writer);
    let stderr = stderr_of(&mut child);
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(status.code(), Some(130), "{stderr}");
}
```

Notes for the reviewer:
- The test opens the writer with `O_NONBLOCK` instead of a blocking `open`. A blocking open would hang the test past its deadline if the program failed before reaching `Fs.open`. A non-blocking write-only open fails with `ENXIO` until a reader exists, so the loop can check the deadline.
- If the first SIGINT lands between the child's `Fs.open` and `Fs.read_all`, `read_text` sees the bit before its first chunk and stops with `Extern(Fault::Stopped(Interrupted))`. That also exits 130, so the test does not depend on the timing.
- `libc` is a dependency of `eml_cli` under the `run` feature, and the integration test binary has `required-features = ["run"]`, so the tests can name it without a dev-dependency.

Run: `cargo test -p eml_cli --test integration interrupt`
Expected: both fail. Without a handler, SIGINT kills the child: `status.code()` is `None` (the first test fails with `left: None, right: Some(130)`). The second test fails the same way on its first signal.

- [ ] **Step 5: Install the handler and return 130**

`crates/eml_cli/src/main.rs`: change the imports to

```rust
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use clap::{Parser, Subcommand};
use eml_cli::{Control, FsProvider, RunConfig, Session, default_limits, stdio_host};
use eml_diagnostics::{has_errors, render};
```

Replace the `Command::Run { debug_heap, file } => { … }` arm with:

```rust
        Command::Run { debug_heap, file } => {
            // 読み込みとコンパイルの間の Ctrl-C も、シグナルで殺されずにこのハンドラが受ける。そこで立てたビットは、
            // 実行の最初の安全点で読まれる
            let control = Arc::new(Control::new(default_limits()));
            interrupt_on_sigint(Arc::clone(&control));
            let Some(session) = load(&file) else {
                return ExitCode::from(2);
            };
            let config = RunConfig::default()
                .with_debug_heap(debug_heap)
                .with_control(control);
            // 警告がプログラムの出力の後に出ないように、実行の前に表示する。
            let compiled = session.compile();
            eprint!("{}", render(&compiled.diagnostics, session.files()));
            let Some(program) = compiled.program else {
                return ExitCode::from(1);
            };
            // 相対パスはカレントディレクトリを基準にする
            match eml_cli::execute(&program, &config, stdio_host(PathBuf::new())) {
                Ok(_) => ExitCode::SUCCESS,
                Err(error) => {
                    eprintln!("runtime error: {error}");
                    // 130 は、シェルが SIGINT で終わった子に報告する値である
                    ExitCode::from(if error.interrupted() { 130 } else { 1 })
                }
            }
        }
```

Add before `/// 入口が読めないことは、読み込みの段に入る前に使い方の誤りとして報告する。…`:

```rust
/// 1回目の SIGINT は `INTERRUPT` を立て、VM を次の安全点で止める。2回目はその場で終わり、まだ書いていない出力は
/// 捨てる。安全点に来ないプログラム (入力を待つ `Fs.read_all` など) を止めるための道である。`ctrlc` はハンドラを専用の
/// スレッドで順に呼ぶので、2つの SIGINT が同時にビットを読み書きすることはない (docs/spec/object-model.md の
/// 「実行の制御」)。
fn interrupt_on_sigint(control: Arc<Control>) {
    ctrlc::set_handler(move || {
        if control.stop_bits() & Control::INTERRUPT == 0 {
            control.interrupt();
        } else {
            std::process::exit(130);
        }
    })
    .expect("the SIGINT handler is installed once, before anything else");
}
```

The depth and memory limits still exit 1: only `interrupted()` selects 130.

Run: `cargo test -p eml_cli --test integration interrupt`
Expected: `2 passed`. Run it repeatedly to check for flakiness: `for i in $(seq 10); do cargo test -q -p eml_cli --test integration interrupt 2>&1 | grep 'test result'; done` prints `ok. 2 passed` ten times.

- [ ] **Step 6: Run the UI tests under the default limits**

In `crates/eml_cli/tests/ui.rs`, change `use std::sync::{Mutex, MutexGuard, PoisonError};` to `use std::sync::{Arc, Mutex, MutexGuard, PoisonError};`, the `eml_cli` import to

```rust
use eml_cli::{
    Control, FsProvider, RunConfig, RuntimeError, Session, capture_host, default_limits, execute,
};
```

and `execute_in`'s doc and config to

```rust
/// `eml run` と同じ既定の上限で流す。止める理由のビットは `Control::clear` でしか下りないので、実行ごとに新しい
/// `Control` を持つ設定を作る。使い回すと、前の実行で立ったビットで次の実行がすぐ止まる。
fn execute_in(
    program: &eml_lir::Program,
    root: &Path,
    debug_heap: bool,
) -> (String, Result<(), RuntimeError>) {
    let (host, out) = capture_host(root.to_path_buf());
    let config = RunConfig::default()
        .with_debug_heap(debug_heap)
        .with_control(Arc::new(Control::new(default_limits())));
    let result = execute(program, &config, host).map(drop);
    (out.contents(), result)
}
```

Run: `cargo test -p eml_cli --test integration ui::`
Expected: green with no snapshot change. No UI test comes near 2^27 words of stack or 80% of memory.

- [ ] **Step 7: Run the full gate**

```bash
cargo test
cargo clippy --all-targets
cargo fmt --check
cargo clippy -p eml_cli --no-default-features --features core
nix build
```

Expected: all green, 0 warnings. `nix build` fetches `ctrlc` through `cargoLock.lockFile` in `flake.nix` (no hash to update). `eml_vm/src` and `eml_rt` do not change here, so Miri and Verus are not needed.

- [ ] **Step 8: Commit**

```bash
git add Cargo.toml Cargo.lock crates/eml_cli
git commit -m "Add default limits and a SIGINT handler to eml run" -m "eml_cli::default_limits() gives eml run a depth limit of 2^27 words and a memory limit of 80% of physical memory, read with sysconf through libc and left unbounded where it cannot be read (spec, 上限の既定の値). eml run creates its Control before loading the file and installs a ctrlc handler on it: the first Ctrl-C sets INTERRUPT so that the VM stops at its next safepoint, and the second exits 130 at once for programs that never reach a safepoint. A run stopped by an interrupt (RuntimeError::interrupted) exits with 130; other runtime errors, including the limits, still exit with 1 (spec, CLI). The UI harness now runs both modes under default_limits. ctrlc and libc are optional dependencies of the run feature.

Test changes: new tests api.rs default_limits_bound_the_depth_and_the_memory and cli.rs an_interrupt_stops_a_running_program_with_130 and repeated_interrupts_end_a_program_waiting_in_a_read_with_130 (both cfg(unix), with a 10 s deadline). Mechanical (kind 3): the UI harness's Control now carries default_limits; no expected value changes." -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01MK52fNp5uA711Q2D4kWEtR"
```

### Task 12a: Remove `saved` and rule R7

Spec: 「LIR の `saved` と R7」 (all items except the Perceus rule 2', which is Task 12b), 「テストの変更」 (the `saved` / R7 rows). Precondition: Task 10 is done, so `crates/eml_interp` and `crates/eml_runtime` no longer exist (the CEK restored only the `saved` variables after a call, so this change must come after it).

Task 12 is split in two so each part keeps `cargo test` green and stays reviewable: 12a removes `saved`, R7 and the `save [...]` text form and renumbers R8/R9; 12b changes Perceus. The order matters: with R7 still in the verifier, rule 2' would be rejected (a borrowed variable that crosses a call is live but not in the owned multiset), while the old rule 2 output passes a verifier without R7.

Documents: 12a and 12b edit only what describes `saved`, R7 and Perceus rule 2 (plus the R-number renumbering everywhere). Left for Task 14, because Task 14 rewrites or deletes those sections anyway: `docs/spec/lir.md` 「インタプリタ (CEK 機械)」 and 「実行時の規約」 (their `saved` / 退避 sentences), the CEK paragraph of 「実行の文脈」, `docs/implementation/architecture.md`'s `Frame::Return` / `saved` paragraph, `docs/future/roadmap.md`'s S10b row and S10b item, and CLAUDE.md's CEK sentences. Task 12a already does what the spec's doc table assigns to testing.md's 「LIR のテキストの形」 (`save`), so Task 14 must not redo it.

**Files:**
- Modify: `crates/eml_lir/src/lib.rs` (262-266 `Rhs::Call { call, saved }` -> `Rhs::Call(Call)`; 297-299 and 330-332 the two match arms)
- Modify: `crates/eml_lir/src/boxing.rs` (57-60, 133, 322-325, 505: patterns and the `f$boxed` body)
- Modify: `crates/eml_lir/src/contract.rs` (36-41 doc comment drops the `saved` sentence; 51-54, 82-85, 152 patterns)
- Modify: `crates/eml_lir/src/pretty.rs` (140-148: no `save [..]`)
- Modify: `crates/eml_lir/src/text.rs` (118-127 `tail` arm, 169-173 call rhs, 218-230 delete `fn saved`)
- Modify: `crates/eml_lir/src/perceus.rs` (1-3 module doc; 227-229 comment; 234 pattern; 262-270 delete the `saved` filling; 220 `mut stmt` -> `stmt`)
- Modify: `crates/eml_lir/src/verify.rs` (1-10 module doc; 169-178 delete `Entry` and the 区間 doc; 187-192 fields; 216-218 init; 234-240 block entry; 306-346 delete `enter` and `visible_at_end`; 359/465 `check_term` loses `index`; 367-371 call arm; 537-553 `pending` inserts; 559-585 `merge`; 622-658 delete `after_call`; 667 `def_epochs`; 674-693 `visible`; 895 pattern)
- Modify: `crates/eml_lir/src/liveness.rs` (9-11 doc comment: the reason is now the VM's slot interference)
- Modify: `crates/eml_lir/src/pipeline.rs` (26-27 doc comment drops `saved`)
- Modify: `crates/eml_core_ir/src/lowering.rs` (99-107: `eml_lir::Rhs::Call(call(value))`)
- Modify: `crates/eml_vm/src/compile/emit.rs` (the two `Rhs::Call { call: …, saved: _ }` patterns, at 295-298 and 401-404 before Tasks 1-4; matched by pattern below, not by line)
- Modify: `crates/eml_vm/src/vm.rs` (330-331 doc comment of `fn call`)
- Modify (R8 -> R7, R9 -> R8 in comments only): `crates/eml_lir/src/{check.rs,lib.rs,verify.rs}`, `crates/eml_core_ir/src/{lib.rs,verify.rs,translate/pattern.rs}`, `crates/eml_vm/src/data.rs`, `crates/eml_vm/tests/data.rs`, `crates/eml_core_ir/tests/verify.rs`, plus any file Task 7 ported that cites R8/R9 (the perl command finds them)
- Modify: `docs/spec/lir.md`, `docs/spec/core-ir.md`, `docs/spec/vm.md`, `docs/implementation/testing.md`, `docs/implementation/architecture.md`, `docs/implementation/status.md`, `docs/future/roadmap.md`, `docs/future/verification.md`, `CLAUDE.md`
- Test: `crates/eml_lir/tests/{verify.rs,text.rs,perceus.rs}`, `crates/eml_core_ir/tests/{perceus.rs,text.rs,translate.rs}`, every hand-written LIR under `crates/*/tests` (`crates/eml_vm/tests/*.rs`, `crates/eml_vm/tests/ir/*.lir`, including what Task 7 ported, e.g. `a_call_resumes_at_the_next_statement_of_its_block` in `crates/eml_vm/tests/run.rs`)

**Interfaces:**
- Consumes: the post-Task-10 tree (no `eml_interp`, no `eml_runtime`); `eml_test_support::lir(text: &str) -> eml_lir::Program`; `eml_lir::liveness::{live_in, live_after_term, step_back, insert_var}` (unchanged).
- Produces: `eml_lir::Rhs::Call(eml_lir::Call)` (tuple variant; `saved` is gone); the LIR text form has no `save [...]`, and a leftover `save` is a parse error like any other unknown word (`line 2: expected a statement, found `save``); the verifier's structure rules are R1-R8 (old R8 Repr is R7, old R9 layout is R8); the ownership level no longer limits what is visible after a call. Task 12b relies on all of this.

- [ ] **Step 1: Delete the R7 / `save` tests and adjust the helpers (test side first)**

Run from the repository root. The script deletes the kind-1 tests the spec lists, removes the `save` half of `the_lowered_level_rejects_what_perceus_inserts`, drops the `save` parameter (and the now-meaningless `call_first` parameter, which only steered the deleted 区間 logic) of `call_on_one_path`, drops the `save` parameter of `chain_of_switches` (the linear-time test now verifies one program at both levels), renames the two Perceus tests the spec renames, retitles the old `// R7` section (only `scopes_keep_variables_in_scope_after_a_call` remains there) as `// R6: 呼び出しの後`, and fixes the comments that cite `saved` / R7.

```bash
python3 - <<'PY'
import pathlib


def delete_tests(path, names):
    """`#[test] fn name()` を、直前の `///` のコメントと後ろの空行ごと消す。"""
    p = pathlib.Path(path)
    s = p.read_text()
    for name in names:
        i = s.index(f"\nfn {name}()")
        k = s.rindex("\n#[test]\n", 0, i + 1) + 1
        lines = s[:k].split("\n")
        while len(lines) >= 2 and lines[-2].startswith("///"):
            lines.pop(-2)
        start = len("\n".join(lines))
        j = s.index("\n}\n", i)
        while s[j + 3 : j + 4] not in ("\n", ""):
            j = s.index("\n}\n", j + 1)
        end = j + 3
        if s[end : end + 1] == "\n":
            end += 1
        s = s[:start] + s[end:]
    p.write_text(s)


def edit(path, pairs):
    p = pathlib.Path(path)
    s = p.read_text()
    for old, new in pairs:
        assert s.count(old) == 1, (path, old[:60], s.count(old))
        s = s.replace(old, new)
    p.write_text(s)


delete_tests("crates/eml_lir/tests/verify.rs", [
    "a_call_that_does_not_save_an_owned_variable_is_rejected",
    "a_call_that_saves_a_variable_it_does_not_own_is_rejected",
    "a_call_that_saves_a_variable_twice_is_rejected",
    "a_call_that_saves_an_invisible_variable_is_rejected",
    "a_non_rc_variable_not_saved_by_a_call_is_not_visible_after_it",
    "a_merge_block_does_not_see_a_variable_that_one_path_did_not_save",
    "scopes_reject_a_saved_list",
    "a_borrowed_field_cannot_be_saved",
])
delete_tests("crates/eml_lir/tests/text.rs", [
    "a_call_with_saved_variables_round_trips",
    "a_tail_call_with_save_is_an_error",
    "an_empty_save_is_an_error",
])
delete_tests("crates/eml_core_ir/tests/perceus.rs", [
    "calls_save_the_variables_used_after_them",
])

edit("crates/eml_lir/tests/verify.rs", [
    ("""/// `n` を使う合流のブロックの前で、片方の経路だけが呼び出しをする。`saved` はその呼び出しの `save` の部分である。
/// `call_first` が偽なら、呼び出しのない経路を先のブロックに置く。合流のブロックの区間が、先に着いた `jump` の区間
/// で決まらないことを確かめるためである。
fn call_on_one_path(saved: &str, call_first: bool) -> String {
    let call = format!("  let t.2: int = call k(1){saved}\\n  jump b3(t.2)\\n");
    let other = "  jump b3(1)\\n";
    let (first, second) = if call_first {
        (call.as_str(), other)
    } else {
        (other, call.as_str())
    };
    format!(
        "{BOOL}{K}fn f(n.0: int, c.1: enum) -> int {{
  switch c.1 Prelude.Bool {{ #0 -> b1, #1 -> b2 }}
b1:
{first}b2:
{second}b3(r.3: int):""", """/// `n` を使う合流のブロックの前で、片方の経路だけが呼び出しをする。
fn call_on_one_path() -> String {
    format!(
        "{BOOL}{K}fn f(n.0: int, c.1: enum) -> int {{
  switch c.1 Prelude.Bool {{ #0 -> b1, #1 -> b2 }}
b1:
  let t.2: int = call k(1)
  jump b3(t.2)
b2:
  jump b3(1)
b3(r.3: int):"""),
    ("""// R7

#[test]
fn scopes_keep_variables_in_scope_after_a_call() {
    // `saved` は Perceus が決めるので、その前は呼び出しの後で見える変数を区切り直さない
""", """// R6: 呼び出しの後

#[test]
fn scopes_keep_variables_in_scope_after_a_call() {
"""),
    ('assert_eq!(check_scopes(&call_on_one_path("", true)), Ok(()));',
     'assert_eq!(check_scopes(&call_on_one_path()), Ok(()));'),
    ("""    let saved = format!(
        "{IDENTITY}fn f(s.0: obj) -> obj {{\\n  let t.1: obj = call g(s.0) save [s.0]\\n  return t.1\\n}}\\n"
    );
    assert_eq!(
        check_lowered(&saved),
        Err("a call saves [s.0] before Perceus in `f`".to_string())
    );
""", ""),
    ("""/// `n` 個の `switch` が続き、片方の枝が呼び出しをして合流する関数。所有の検査の段では、合流のブロックの数だけ、
/// 見える変数の集合を決め直す。`save` は呼び出しの `save` の部分で、Perceus より前の形では空にする。
fn chain_of_switches(n: u32, save: &str) -> String {""", """/// `n` 個の `switch` が続き、片方の枝が呼び出しをして合流する関数。所有の検査の段では、合流のブロックの数だけ、
/// 入口の所有を比べる。
fn chain_of_switches(n: u32) -> String {"""),
    ('"b{call}:\\n  let t.{}: int = call k(n.1){save}\\n  jump b{merge}()\\nb{other}:\\n  jump b{merge}()\\nb{merge}:\\n",',
     '"b{call}:\\n  let t.{}: int = call k(n.1)\\n  jump b{merge}()\\nb{other}:\\n  jump b{merge}()\\nb{merge}:\\n",'),
    ("""    // ブロックごとに所有や見える変数を関数の先頭から数え直すと、2乗の時間がかかる
    let before = read(&chain_of_switches(20000, ""));
    let after = read(&chain_of_switches(20000, " save [s.0, n.1, c.2]"));
    let start = std::time::Instant::now();
    assert_eq!(verify_scopes(&before), Ok(()));
    assert_eq!(verify(&after), Ok(()));""", """    // ブロックごとに所有を関数の先頭から数え直すと、2乗の時間がかかる
    let program = read(&chain_of_switches(20000));
    let start = std::time::Instant::now();
    assert_eq!(verify_scopes(&program), Ok(()));
    assert_eq!(verify(&program), Ok(()));"""),
])

edit("crates/eml_lir/tests/perceus.rs", [
    ("//! 生存解析と Perceus の `dup` / `decref` / `release` の位置、呼び出しの `saved` (docs/spec/lir.md の「Perceus」)。",
     "//! 生存解析と Perceus の `dup` / `decref` / `release` の位置 (docs/spec/lir.md の「Perceus」)。"),
    ("// RC の対象でない変数も入る。`saved` が使うためである。",
     "// RC の対象でない変数も入る。VM のスロットの割り当てが使うためである。"),
    ("    // 借りたフィールドは `save` に入れられないので、呼び出しの前の `release` で所有にしてから退避する\n",
     "    // 借りたフィールドは呼び出しの前の `release` で所有にする\n"),
    ("""fn an_unused_call_result_is_released_and_calls_save_what_is_used_later() {
    // `saved` は呼び出しの後で生きている変数から結果の変数を除いたもので、RC の対象でない `n.1` も入る
""", """fn an_unused_call_result_is_released() {
"""),
    ("fn an_argument_used_after_the_call_is_dupped_and_saved() {",
     "fn an_argument_used_after_the_call_is_dupped() {"),
    ("    // 持ち主が死んでいれば解き、生きていれば `dup` する。フレームは借りた値を持てない (R7)\n",
     "    // 持ち主が死んでいれば解き、生きていれば `dup` する\n"),
])

edit("crates/eml_core_ir/tests/text.rs", [
    ("""        (
            "fn g(x.0: int) -> int {\\n  let y.1: int = call g(1) save [x.0]\\n  return y.1\\n}\\n",
            2,
            "expected a statement, found `save`",
        ),
""", ""),
])

edit("crates/eml_core_ir/tests/translate.rs", [
    ("// contract は使われない純粋な `let` を消し、Perceus は RC の命令と `save` を足す",
     "// contract は使われない純粋な `let` を消し、Perceus は RC の命令を足す"),
])
PY
```

- [ ] **Step 2: Remove `save [...]` from every hand-written LIR in the tests and watch them fail**

```bash
grep -rl --include='*.rs' --include='*.lir' ' save \[' crates/*/tests | xargs perl -pi -e 's/ save \[[^\]]*\]//g'
grep -rn 'save \[' crates/*/tests
```

Expected: the second grep prints only `crates/eml_core_ir/tests/perceus.rs:…: let Some((_, saved)) = line.split_once(" save [") else {` (the interpolation test, rewritten in Step 5). The perl runs only on `crates/*/tests`: `crates/eml_lir/src/pretty.rs` and `text.rs` also contain ` save [` and are changed by hand in Step 3.

```bash
cargo test -p eml_lir --test integration 2>&1 | grep -E '^test .*FAILED|test result'
```

Expected: 6 failures, all because the code still prints and checks `save`:
`perceus::an_argument_used_after_the_call_is_dupped`, `perceus::a_field_live_after_a_call_is_owned_before_it`, `perceus::a_field_used_after_a_call_is_owned_before_the_call_saves_it`, `perceus::an_unused_call_result_is_released` (actual output still has ` save [..]`), `verify::the_result_of_a_call_is_bound_to_a_compatible_variable` and `verify::a_long_chain_of_switches_is_verified_in_linear_time` (`… is used after a call that does not save it`). `cargo test -p eml_vm --test integration` also fails in the tests whose hand-written LIR lost its `save` (ownership-level verification rejects them); that is expected until Step 4.

- [ ] **Step 3: Change `Rhs::Call` and delete `saved`, R7 and the `save` text form**

```bash
python3 - <<'PY'
import pathlib


def edit(path, pairs):
    p = pathlib.Path(path)
    s = p.read_text()
    for old, new in pairs:
        assert s.count(old) == 1, (path, old[:60], s.count(old))
        s = s.replace(old, new)
    p.write_text(s)


def cut(path, start_marker, end_marker, replacement=""):
    """`start_marker` から `end_marker` の直前までを `replacement` に替える。"""
    p = pathlib.Path(path)
    s = p.read_text()
    assert s.count(start_marker) == 1 and s.count(end_marker) == 1, (path, start_marker[:40])
    i = s.index(start_marker)
    j = s.index(end_marker)
    assert i < j
    p.write_text(s[:i] + replacement + s[j:])


edit("crates/eml_lir/src/lib.rs", [
    ("""    /// `saved` は、呼び出しの後で生きている変数から結果の変数を除いたもので、Perceus が埋める
    /// (docs/spec/lir.md)。
    Call { call: Call, saved: Vec<VarId> },
""", """    Call(Call),
"""),
    ("Rhs::Call { call, saved: _ } => call.for_each_atom(f),", "Rhs::Call(call) => call.for_each_atom(f),"),
    ("Rhs::Call { call, saved: _ } => call.for_each_atom_mut(f),", "Rhs::Call(call) => call.for_each_atom_mut(f),"),
])

edit("crates/eml_lir/src/boxing.rs", [
    ("""                        Rhs::Call {
                            call: Call::Direct(target, _),
                            saved: _,
                        } => direct[target.0 as usize] = true,""",
     "                        Rhs::Call(Call::Direct(target, _)) => direct[target.0 as usize] = true,"),
    ("Rhs::Call { call, saved: _ } => Some((index, call)),", "Rhs::Call(call) => Some((index, call)),"),
    ("""                rhs: Rhs::Call {
                    call: Call::Direct(target, params.into_iter().map(Atom::Var).collect()),
                    saved: Vec::new(),
                },""", """                rhs: Rhs::Call(Call::Direct(
                    target,
                    params.into_iter().map(Atom::Var).collect(),
                )),"""),
    ("Rhs::Call { call, saved: _ } => {\n                        let given", "Rhs::Call(call) => {\n                        let given"),
])

edit("crates/eml_lir/src/contract.rs", [
    ("""/// `poly`) と関数の戻り値の印も同じく比べる (docs/spec/lir.md の「縮約」と「Kind の印」)。`saved` は Perceus が
/// 決めるので、この時点では空である。
""", """/// `poly`) と関数の戻り値の印も同じく比べる (docs/spec/lir.md の「縮約」と「Kind の印」)。
"""),
    ("""    let Some(Stmt::Let {
        var,
        rhs: Rhs::Call { call, saved: _ },
    }) = block.stmts.last()""", """    let Some(Stmt::Let {
        var,
        rhs: Rhs::Call(call),
    }) = block.stmts.last()"""),
    ("""    let Some(Stmt::Let {
        var: _,
        rhs: Rhs::Call { call, saved: _ },
    }) = block.stmts.pop()""", """    let Some(Stmt::Let {
        var: _,
        rhs: Rhs::Call(call),
    }) = block.stmts.pop()"""),
    ("Rhs::Call { call: _, saved: _ } | Rhs::Drop(_) => false,", "Rhs::Call(_) | Rhs::Drop(_) => false,"),
])

edit("crates/eml_lir/src/pretty.rs", [
    ("""            Rhs::Call { call, saved } => {
                let text = self.call(function, call);
                if saved.is_empty() {
                    text
                } else {
                    let names: Vec<String> = saved.iter().map(|&v| w.var(vars, v)).collect();
                    format!("{text} save [{}]", names.join(", "))
                }
            }
""", """            Rhs::Call(call) => self.call(function, call),
"""),
])

edit("crates/eml_lir/src/text.rs", [
    ("""                "tail" => {
                    let call = self.call(state)?;
                    if self.reader.at_word("save") {
                        return Err(error(
                            line,
                            "a tail call saves nothing; `save` is only on `let`",
                        ));
                    }
                    break Term::TailCall(call);
                }
""", """                "tail" => break Term::TailCall(self.call(state)?),
"""),
    ("""            let call = self.call(state)?;
            let saved = self.saved(state)?;
            return Ok(Rhs::Call { call, saved });
""", """            return Ok(Rhs::Call(self.call(state)?));
"""),
])
cut("crates/eml_lir/src/text.rs",
    "    /// `save [..]`。`pretty` は空の `save` を書かないので、`save []` は読まない。\n",
    "    /// `let` の右辺と `tail` の後の呼び出し。")

edit("crates/eml_lir/src/perceus.rs", [
    ("""//! Perceus の `dup` / `decref` / `release` の挿入と、呼び出しの `saved` (docs/spec/lir.md の「Perceus」)。値を消費する
//! 使いを所有権の移動として扱い、後でも使う変数を複製し、使わなくなった変数をできるだけ早く捨てる。対象は RC の対象
//! (`Repr::is_rc`) の変数だけである。""", """//! Perceus の `dup` / `decref` / `release` の挿入 (docs/spec/lir.md の「Perceus」)。値を消費する使いを所有権の移動
//! として扱い、後でも使う変数を複製し、使わなくなった変数をできるだけ早く捨てる。対象は RC の対象 (`Repr::is_rc`) の
//! 変数だけである。"""),
    ("""        // 消費する変数のほかに、所有を取るのは次の変数である。フレームは借りた値を持てない (R7) ので、呼び出しの後で
        // 生きている借りた変数。持ち主の最後の参照をこの文が消費した後も生きている、その持ち主に属する変数
        // (docs/spec/lir.md の「所有を取る時点」)""", """        // 消費する変数のほかに、所有を取るのは次の変数である。呼び出しの後で生きている借りた変数。持ち主の最後の参照を
        // この文が消費した後も生きている、その持ち主に属する変数 (docs/spec/lir.md の「所有を取る時点」)"""),
    ("                rhs: Rhs::Call { .. },\n", "                rhs: Rhs::Call(_),\n"),
    ("""        // 呼び出しは、その後で生きている変数のうち結果の変数以外を退避する。借りた変数は上で所有にしてある
        if let Stmt::Let {
            var,
            rhs: Rhs::Call { call: _, saved },
        } = &mut stmt
        {
            *saved = live.iter().copied().filter(|v| v != var).collect();
        }
""", ""),
    ("        mut stmt: Stmt,\n", "        stmt: Stmt,\n"),
])

edit("crates/eml_lir/src/verify.rs", [
    ("""//! Perceus の後は、RC の対象の所有の多重集合と、呼び出しの後に見える変数 (R6、R7) も確かめる (`verify`)。`switch`、
//! `unpack`、`unbox` は値を読むだけである。`switch` と `unpack` のフィールドは値から借りて始まり、自分か持ち主が
//! 所有を持つ間だけ有効である。段の分け方は docs/spec/lir.md の「verifier」
//! による。Core IR の verifier と共有する部品は `check` にある。""", """//! Perceus の後は、RC の対象の所有の多重集合 (R6) も確かめる (`verify`)。`switch`、`unpack`、`unbox` は値を読む
//! だけである。`switch` と `unpack` のフィールドは値から借りて始まり、自分か持ち主が所有を持つ間だけ有効である。
//! 段の分け方は docs/spec/lir.md の「verifier」による。Core IR の verifier と共有する部品は `check` にある。"""),
    ("""    defs: Vec<Option<Site>>,
    def_epochs: Vec<u32>,
    /// 区間ごとの、入れ直した変数 (昇順)。呼び出しの区間は `save` の変数、合流の区間は入るどの辺でも見える変数である。
    epochs: Vec<Vec<VarId>>,
    epoch: u32,
    at: Site,
    pending: HashMap<u32, Entry>,
""", """    defs: Vec<Option<Site>>,
    at: Site,
    /// まだ着いていないブロックの入口の所有。`switch` の行き先は1本の辺から、合流するブロックは最初の `jump` から
    /// 作る。
    pending: HashMap<u32, Owned>,
"""),
    ("""            defs: vec![None; function.vars.len()],
            def_epochs: vec![0; function.vars.len()],
            epochs: vec![Vec::new()],
            epoch: 0,
""", """            defs: vec![None; function.vars.len()],
"""),
    ("""            } else {
                self.enter(index)
            };""", """            } else {
                self.pending
                    .remove(&index)
                    .expect("R3 and R4 give every block an edge from an earlier block")
            };"""),
    ("""                    Rhs::Call { call, saved } => {
                        let never = self.never_returns(call);
                        self.cx.note_never(*var, never);
                        self.after_call(owned, saved)?;
                    }""", """                    Rhs::Call(call) => {
                        let never = self.never_returns(call);
                        self.cx.note_never(*var, never);
                    }"""),
    ("        self.check_term(index, owned, &block.term)", "        self.check_term(owned, &block.term)"),
    ("    fn check_term(&mut self, index: u32, mut owned: Owned, term: &Term)", "    fn check_term(&mut self, mut owned: Owned, term: &Term)"),
    ("self.merge(index, *target, owned)", "self.merge(*target, owned)"),
    ("""                    self.pending.insert(
                        case.target.0,
                        Entry {
                            owned: entry,
                            incoming: vec![(index, self.epoch)],
                        },
                    );""", "                    self.pending.insert(case.target.0, entry);"),
    ("""                    self.pending.insert(
                        default.0,
                        Entry {
                            owned,
                            incoming: vec![(index, self.epoch)],
                        },
                    );""", "                    self.pending.insert(default.0, owned);"),
    ("""        *slot = Some(site);
        self.def_epochs[var.0 as usize] = self.epoch;
""", """        *slot = Some(site);
"""),
    ("    /// 定義の位置が今の位置を支配し (R6)、所有の検査の段では、間の呼び出しがすべて退避している (R7)。",
     "    /// 定義の位置が今の位置を支配する (R6)。"),
    ("""        if self.def_epochs[var.0 as usize] != self.epoch
            && self.epochs[self.epoch as usize]
                .binary_search(&var)
                .is_err()
        {
            return Err(format!(
                "`{}` is used after a call that does not save it",
                self.cx.name(var)
            ));
        }
""", ""),
    ("Rhs::Call { call, saved: _ } => {\n                return self.check_call", "Rhs::Call(call) => {\n                return self.check_call"),
])
cut("crates/eml_lir/src/verify.rs",
    "/// まだ着いていないブロックの入口の状態。`switch` の行き先は1本の辺から、合流するブロックは最初の `jump` から作る。\n",
    "struct Checker<'a> {")
cut("crates/eml_lir/src/verify.rs",
    "    /// ブロックの入口の状態を取り出し、区間を決める。",
    "    fn check_block(")
cut("crates/eml_lir/src/verify.rs",
    "    /// 合流するブロックに入る `jump` は、渡す値を除いて",
    "    /// `jump` の実引数は、行き先の引数と Repr が互換で",
    """    /// 合流するブロックに入る `jump` は、渡す値を除いて、どれも同じ多重集合を所有する。その集合が入口の所有になる (R6)。
    fn merge(&mut self, target: BlockId, owned: Owned) -> Result<(), String> {
        let Some(first) = self.pending.get(&target.0) else {
            self.pending.insert(target.0, owned);
            return Ok(());
        };
        if *first != owned {
            return Err(format!(
                "a jump to b{} owns {} but an earlier jump to it owns {}",
                target.0,
                self.owned_names(&owned),
                self.owned_names(first)
            ));
        }
        Ok(())
    }

""")
cut("crates/eml_lir/src/verify.rs",
    "    /// 呼び出しの後に見える変数は、退避した変数と結果だけである。",
    "    /// 変数を多くとも1回定義し (R5)")

edit("crates/eml_lir/src/liveness.rs", [
    ("""/// Perceus が、呼び出しの後で生きている RC でない変数も `saved` に入れるためである。""",
     """/// VM のスロットの割り当てが、文の単位の干渉を作るのに RC でない変数も数えるためである
/// (docs/spec/vm.md の「スロットの割り当て」)。"""),
])

edit("crates/eml_lir/src/pipeline.rs", [
    ("""/// 変換の直後の LIR に、box の挿入から `last` までをかける。入力の約束 (`tail`、`box`、`unbox`、RC の命令、`saved`
/// がないこと) を入口で確かめるので、手で書いた LIR を渡すテストもこの検査を通る""",
     """/// 変換の直後の LIR に、box の挿入から `last` までをかける。入力の約束 (`tail`、`box`、`unbox`、RC の命令が
/// ないこと) を入口で確かめるので、手で書いた LIR を渡すテストもこの検査を通る"""),
])

edit("crates/eml_core_ir/src/lowering.rs", [
    ("""            eml_lir::Rhs::Call {
                call: call(value),
                saved: Vec::new(),
            }""", "            eml_lir::Rhs::Call(call(value))"),
])

edit("crates/eml_vm/src/vm.rs", [
    ("""/// 呼ぶ側の上に呼ばれる側のフレームを作り、引数を写して先頭へ進む。呼ぶ側のスロットはスタックに残るので、`saved` は
/// 読まない。""", """/// 呼ぶ側の上に呼ばれる側のフレームを作り、引数を写して先頭へ進む。呼ぶ側のスロットはスタックに残る。"""),
])

edit("crates/eml_lir/tests/text.rs", [
    ("""            rhs: Rhs::Call {
                call: Call::Apply {
                    callee: v(0),
                    args: vec![Atom::Int(1), Atom::Int(2)],
                    yields: false,
                },
                saved: Vec::new(),
            },""", """            rhs: Rhs::Call(Call::Apply {
                callee: v(0),
                args: vec![Atom::Int(1), Atom::Int(2)],
                yields: false,
            }),"""),
    ("""            rhs: Rhs::Call {
                call: Call::Clause {
                    effect: 0,
                    op: 0,
                    callee: v(5),
                    args: vec![v(2), Atom::Unit],
                },
                saved: vec![],
            },""", """            rhs: Rhs::Call(Call::Clause {
                effect: 0,
                op: 0,
                callee: v(5),
                args: vec![v(2), Atom::Unit],
            }),"""),
])
PY
perl -0pi -e 's/Rhs::Call \{\n\s*call: ([^\n]*?),\n\s*saved: _,\n\s*\}/Rhs::Call($1)/g' crates/eml_vm/src/compile/emit.rs
grep -n 'Rhs::Call' crates/eml_vm/src/compile/emit.rs
```

Expected from the grep: exactly two lines, `Rhs::Call(Call::Direct(f, args)) => {` and `Rhs::Call(call @ (Call::Apply { .. } | Call::Clause { .. })) => {` (if Tasks 1-4 reshaped the Apply/Clause arm, the perl still matches any `Rhs::Call { call: <pattern>, saved: _ }`; check that no `saved` remains: `grep -n saved crates/eml_vm/src/compile/emit.rs` prints nothing).

What the verifier change amounts to (for the reviewer): `Entry` becomes a plain `Owned` in `pending` (its `incoming` list existed only to compute 区間), the fields `def_epochs` / `epochs` / `epoch`, the functions `enter` / `visible_at_end` / `after_call`, the epoch half of `visible` (`… is used after a call that does not save it`), and the before-Perceus check `a call saves [...] before Perceus` are deleted; `check_term` and `merge` lose their unused `from` / `index` argument. `visible` keeps only the dominance check (R6), so after a call every variable in scope stays visible and ownership simply continues.

- [ ] **Step 4: Run the LIR and VM tests**

```bash
cargo build --all-targets 2>&1 | grep -E '^(error|warning)'
cargo test -p eml_lir -p eml_vm 2>&1 | grep -E '^test .*FAILED|test result'
```

Expected: the build prints nothing, and all `eml_lir` and `eml_vm` tests pass. No snapshot changes beyond the ` save [..]` removed in Step 2: Perceus emits the same instructions as before, without `saved`. `eml_core_ir`'s `interpolation_saves_only_the_accumulator_across_holes` now fails (it counts `save [` lines and finds 0). Step 5 replaces it.

- [ ] **Step 5: Rewrite the interpolation test on statement-level liveness**

`interpolation_saves_only_the_accumulator_across_holes` read the `save [...]` lists. The rewrite (`interpolation_keeps_only_the_accumulator_live_across_holes`, kind 2) walks each block of `f` backwards with the same public API the VM's slot allocator uses (`eml_lir::liveness::{live_in, live_after_term, step_back}`, see `crates/eml_vm/src/compile/slots.rs`) and checks that after each of the 6 calls the only live variable besides the call's result is the left argument of the next `Prelude.++` (the accumulator).

```bash
python3 - <<'PY'
import pathlib

p = pathlib.Path("crates/eml_core_ir/tests/perceus.rs")
s = p.read_text()
old_head = """//! Perceus の `dup` / `decref` / `release` の位置と、呼び出しの `saved` (docs/spec/lir.md の「Perceus」)。ソースから
//! パイプラインを通した結果を見る。"""
assert s.count(old_head) == 1
s = s.replace(old_head, """//! Perceus の `dup` / `decref` / `release` の位置 (docs/spec/lir.md の「Perceus」)。ソースからパイプラインを通した
//! 結果を見る。""")
old_use = "use crate::common::{function, lir_text};"
assert s.count(old_use) == 1
s = s.replace(old_use, """use eml_extern::Extern;
use eml_lir::liveness::{insert_var, live_after_term, live_in, step_back};
use eml_lir::{Atom, Rhs, Stmt, VarId};

use crate::common::{function, lir_text};""")
i = s.index("/// 補間は穴を評価するたびにつなぐので")
assert "fn interpolation_saves_only_the_accumulator_across_holes()" in s[i:]
s = s[:i] + r'''/// 補間は穴を評価するたびにつなぐので、穴の呼び出しをまたいで生きているのは組み立て中の文字列だけである
/// (docs/spec/core-ir.md の「変換の規則」)。`n` を最初の穴でだけ使い、ほかの穴の引数を定数にして、組み立て中の
/// 文字列のほかに生きている変数をなくす。生存は、VM のスロットの割り当てと同じく、Perceus の後の LIR で文の単位に
/// 求める。
#[test]
fn interpolation_keeps_only_the_accumulator_live_across_holes() {
    let text = "g : Int -> <IO> Int\ng n =\n  println \"g\"\n  n\n\nf : Int -> <IO> String\nf n = \"a\\{g n}b\\{g 2}c\\{g 3}\"\n\nmain : Unit -> <IO> Unit\nmain () = println (f 1)";
    let program = eml_test_support::lir(text);
    let shown = function(&eml_lir::pretty(&program), "f");
    let f = program
        .functions
        .iter()
        .find(|function| function.name == "f")
        .expect("a function `f`");
    let live_in = live_in(f);
    let mut calls = 0;
    for block in &f.blocks {
        let mut live = live_after_term(f, &block.term, &live_in);
        block.term.for_each_atom(|atom| insert_var(&mut live, atom));
        // 後ろからたどるので、呼び出しに着いたときの `accumulator` は、その後で穴の値をつなぐ `++` の左の引数
        // (組み立て中の文字列) である
        let mut accumulator = None;
        for stmt in block.stmts.iter().rev() {
            match stmt {
                Stmt::Let {
                    var: _,
                    rhs:
                        Rhs::Extern {
                            ext: Extern::StrConcat,
                            args,
                            at: _,
                        },
                } => {
                    accumulator = match args[0] {
                        Atom::Var(left) => Some(left),
                        _ => None,
                    }
                }
                Stmt::Let {
                    var,
                    rhs: Rhs::Call(_),
                } => {
                    calls += 1;
                    let others: Vec<VarId> = live.iter().copied().filter(|v| v != var).collect();
                    assert_eq!(others, Vec::from_iter(accumulator), "{shown}");
                }
                _ => {}
            }
            step_back(stmt, &mut live);
        }
    }
    assert_eq!(calls, 6, "two calls per hole: {shown}");
}
'''
p.write_text(s)
PY
cargo test -p eml_core_ir 2>&1 | grep -E '^test .*FAILED|test result'
```

Expected: all pass. `lower_until_stops_after_the_named_pass` (translate.rs) lost ` save [s.2]` in Step 2 and its comment in Step 1; `interpolation_keeps_only_the_accumulator_live_across_holes` passes (6 calls, each with exactly the accumulator live). If it fails, the message prints `f`'s LIR.

- [ ] **Step 6: Update the documents for `saved` and R7, then renumber R8/R9 everywhere**

The script edits the sentences that describe `saved`, `save [...]` and R7 (lir.md 「ブロックの列」「構造の規則」「パスの順番」「生存解析」「Perceus」「verifier」, core-ir.md, vm.md 「スロットの割り当て」, testing.md 「LIR のテキストの形」 and 「IR のテスト」, architecture.md, status.md's list-literal note, roadmap.md's 借用パラメータ item, CLAUDE.md). In lir.md it only drops the `(R7)` reason from Perceus rule 2; Task 12b rewrites that rule. The status.md note is rewritten, not deleted, because a list literal of calls is still quadratic, now in the VM's interference graph: each call's result interferes with every element evaluated so far (`crates/eml_vm/src/compile/slots.rs`, `interference`).

```bash
python3 - <<'PY'
import pathlib

def edit(path, pairs):
    p = pathlib.Path(path)
    s = p.read_text()
    for old, new in pairs:
        assert s.count(old) == 1, (path, old[:60], s.count(old))
        s = s.replace(old, new)
    p.write_text(s)

edit("docs/spec/lir.md", [
    ("末尾呼び出し `tail`、呼び出しの退避 (`saved`) を持つ。どれも LIR のパス",
     "末尾呼び出し `tail` を持つ。どれも LIR のパス"),
    ("- 呼び出しの退避 (`saved`) は、CEK だけが使う (下の「インタプリタ (CEK 機械)」)。S10b で CEK を消すまで LIR に残す\n", ""),
    ("- `let x = <呼び出し> save S` の S は、呼び出しの後で使う変数の並び (`saved`) である。Perceus が入れ、CEK がフレームに退避する (下の「Perceus」と「インタプリタ (CEK 機械)」)。`tail` は退避を持たない\n", ""),
    ("- R7 [LIR だけ]: 所有の検査の段では、`let x = <呼び出し> save S` の後に見える変数は S と x だけである。S は呼び出しの前に見えていなければならず、S のうち RC の対象の部分は、所有の多重集合と一致する。呼び出しの後に定義した変数は、次の呼び出しまで見える。合流するブロックでは、入るすべての辺で見えている変数だけが見える\n", ""),
    ("末尾呼び出し、`box` と `unbox`、RC の命令、`saved` のない LIR", "末尾呼び出し、`box` と `unbox`、RC の命令のない LIR"),
    ("`dup` / `decref` / `release` と `saved` が入った LIR", "`dup` / `decref` / `release` が入った LIR"),
    ("- 生存集合はすべての変数を持つ。Perceus は RC の対象だけを見るが、`saved` は RC の対象でない変数も退避するためである",
     "- 生存集合はすべての変数を持つ。Perceus は RC の対象だけを見るが、VM のスロットの割り当ては RC の対象でない変数も干渉に数えるためである ([VM](vm.md) の「スロットの割り当て」)"),
    ("`clause` は呼び出しとして退避 (`saved`) を持つ", "`clause` は呼び出しとして扱う"),
    ("の後で生きている。フレームは借りた値を持てないためである (R7)", "の後で生きている"),
    ("- `saved` は、呼び出しの後で生きている変数から、結果の変数を除いたものである。借りた変数は呼び出しの前に所有になっているので、`saved` に入らない。そのために末尾呼び出しを降格することもない\n", ""),
    ("`dup`、`decref`、`release`、空でない `saved` がないことを確かめる", "`dup`、`decref`、`release` がないことを確かめる"),
    ("RC の対象の所有の多重集合 (R6、R7) と、呼び出しの後に見える変数 (R7) と、V1 を確かめる",
     "RC の対象の所有の多重集合 (R6) と、V1 を確かめる"),
    ("呼び出しでは、退避する RC の対象の変数が、その時点で所有している変数とちょうど一致する (同じ変数を2回退避しない)。", ""),
    ("  - 借りた変数 (所有の数0) は消費できず、`save` の所有の多重集合とも合わないので、呼び出しをまたいで保存されることも、`jump` で渡されることもない。定義が支配する合流のブロックでは見えたままで、持ち主がどの辺でも所有されていれば有効である",
     "  - 借りた変数 (所有の数0) は消費できないので、`jump` で渡されることはない。呼び出しの後も見えたままで、持ち主が所有されていれば有効である。持ち主を呼び出しの引数として消費すると、その後の借りた変数の使いは拒まれる。定義が支配する合流のブロックでも見えたままで、持ち主がどの辺でも所有されていれば有効である"),
    ("Perceus より前の RC の命令と `saved` を拒む", "Perceus より前の RC の命令を拒む"),
    ("  - ``a call saves [s.0] before Perceus``\n", ""),
    ("範囲と所有の検査 (R5、R6、R7) を最後に行う", "範囲と所有の検査 (R5、R6) を最後に行う"),
])

edit("docs/spec/core-ir.md", [
    ("- 評価の途中でつなぐので、穴の呼び出しをまたいで退避するのは組み立て中の文字列だけである。リストのリテラルのように、すべての部分を評価してから組むと、退避の大きさの合計が部分の数の2乗になる",
     "- 評価の途中でつなぐので、穴の呼び出しをまたいで生きているのは組み立て中の文字列だけである。リストのリテラルのように、すべての部分を評価してから組むと、呼び出しをまたいで生きている変数の数の合計が部分の数の2乗になる"),
    ("末尾呼び出し `tail`、呼び出しの退避 (`saved`) を持たないこと", "末尾呼び出し `tail` を持たないこと"),
    ("RC の命令、`box`、`unbox`、`tail`、`saved` の項目は Core IR に当たらない", "RC の命令、`box`、`unbox`、`tail` の項目は Core IR に当たらない"),
    ("変換の直後の LIR は、RC の命令、`box`、`unbox`、`tail`、`saved` を持たない", "変換の直後の LIR は、RC の命令、`box`、`unbox`、`tail` を持たない"),
    ("- RC の命令、`box`、`unbox`、`tail`、`saved` は Core IR の型にないので", "- RC の命令、`box`、`unbox`、`tail` は Core IR の型にないので"),
])

edit("docs/spec/vm.md", [
    ("- LIR の `saved` は読まない。呼ぶ側のスロットは呼び出しの間もスタックに残るためである",
     "- 呼ぶ側のスロットは、呼び出しの間もスタックに残る。そのため、呼び出しの後で使う変数を退避しない"),
])

edit("docs/implementation/testing.md", [
    ("LIR だけの命令 (`tail`、`box`、`unbox`、RC の命令、`save`) のテスト", "LIR だけの命令 (`tail`、`box`、`unbox`、RC の命令) のテスト"),
    ("`release` の値と残すフィールド、`save`) には付けない", "`release` の値と残すフィールド) には付けない"),
    ("- Perceus の後の呼び出しは、後ろに `save [..]` を付ける (`let t.2: int = call f(c.0) save [c.0]`)。`saved` が空なら何も書かない。\n", ""),
    ("空の `evv_mask[]`、`save []`、末尾のカンマ", "空の `evv_mask[]`、末尾のカンマ"),
    ("どれも、読み直して表示すると表示が変わる形だからである。`tail` の後の `save` も誤りにする。", "どれも、読み直して表示すると表示が変わる形だからである。"),
    ("`unbox`、`tail`、`save [..]` はない。", "`unbox`、`tail` はない。"),
    ("``expected a right-hand side, found `box` ``、``expected a statement, found `save` `` など", "``expected a right-hand side, found `box` `` など"),
])

edit("docs/implementation/architecture.md", [
    ("Core IR の型は RC の命令、`box`、`unbox`、`tail`、`saved` を持たず", "Core IR の型は RC の命令、`box`、`unbox`、`tail` を持たず"),
])

edit("docs/implementation/status.md", [
    ("- 呼び出しを要素に持つリストのリテラル (`[f 1, f 2, …]`) では、末尾でない呼び出しのたびに、それまでに評価した要素を `save` に並べる。そのため、IR の `save` の大きさと、インタプリタのフレームの退避が、要素の数の2乗になる。タプルも同じである。",
     "- 呼び出しを要素に持つリストのリテラル (`[f 1, f 2, …]`) では、末尾でない呼び出しのたびに、それまでに評価した要素がすべて生きている。そのため、VM のスロットの割り当ての干渉グラフの辺の数が、要素の数の2乗になる。タプルも同じである。"),
])

edit("docs/future/roadmap.md", [
    ("所有の引数を消費した後で借用の引数を確かめる。R7 は所有の部分だけを所有の多重集合と比べ、継続のクロージャが捕まえる値に借用の部分を含めない。",
     "所有の引数を消費した後で借用の引数を確かめる。継続のクロージャが捕まえる値に借用の部分を含めない。"),
])

edit("CLAUDE.md", [
    ("but `dup` / `decref` / `release`, `box` / `unbox`, terminator `tail` and call `saved` (kept until the CEK is removed in S10).",
     "but `dup` / `decref` / `release`, `box` / `unbox` and terminator `tail`."),
    ("rejects `tail`, `box`, `unbox`, RC instructions and `save`;", "rejects `tail`, `box`, `unbox` and RC instructions;"),
])
PY
grep -rlE '\bR[89]\b' crates docs/spec docs/implementation docs/future docs/overview.md CLAUDE.md | xargs perl -pi -e 's/\bR8\b/R7/g; s/\bR9\b/R8/g'
cargo fmt
```

The perl renames old R8 to R7 first and old R9 to R8 second, so no number is renamed twice. It must run after Steps 1, 3 and this script have removed every reference to the old R7. It rewrites "R1 から R9" (both verify.rs test files), "R1-R9" (CLAUDE.md), the test heading comments (`// R8` -> `// R7` in both verify.rs test files, `// R9: データの配置` -> `// R8: データの配置`), and every code comment and document that cites R8 or R9. `docs/superpowers/` (this design, deleted at the end of S10b) and `docs/reports/` (dated records) are deliberately not touched. `cargo fmt` reflows `Rhs` in `lib.rs` (the `Con { ctor, args }` variant goes multi-line), which is expected.

Verification greps (all must print nothing):

```bash
grep -rn 'save \[' crates
grep -rnw 'saved' crates --include='*.rs' | grep -v 'saved_row\|saved_source'
grep -rnE '\bR9\b' crates docs/spec docs/implementation docs/future CLAUDE.md
grep -rn 'does not save it\|a call saves\|after_call\|def_epochs' crates docs/spec
```

(`saved_row` / `saved_source` are unrelated locals in `crates/eml_types/src/check/body.rs`.) And this one must print only the Task-14 sites listed at the top of this task (lir.md's CEK sections, architecture.md's `Frame::Return`, roadmap.md's S10b row and item):

```bash
grep -rn 'saved' docs/spec docs/implementation docs/future CLAUDE.md
```

If an `edit` assertion in the docs script fails because an earlier task already reworded that sentence, apply the same change by hand to the current wording.

- [ ] **Step 7: Full gate**

```bash
cargo test
cargo clippy --all-targets
cargo fmt --check
nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_vm --test integration miri::
nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance -Zmiri-tree-borrows" cargo miri test -p eml_vm --test integration miri::
```

Expected: everything passes, clippy prints no warning, fmt prints nothing. The citations test (`cargo test -p eml_cli --test integration citations`) is part of `cargo test`; the only new citation is `docs/spec/vm.md の「スロットの割り当て」` in `liveness.rs`, which exists. The UI tests and `crates/eml_vm/tests/bench.rs` do not change: Perceus emits the same instructions as before, minus `saved`. Miri runs because `crates/eml_vm/src/compile/emit.rs` changed.

- [ ] **Step 8: Commit**

```bash
git add -u crates docs CLAUDE.md
git commit -m "Remove LIR's saved and verifier rule R7" -m "The VM keeps the caller's slots on the stack across a call, so the list of variables to restore after a call (saved), which only the CEK read, and R7, which tied that list to the owned multiset, have no reader left. Rhs::Call becomes a tuple variant, the text form loses save [...] (a leftover save is a parse error like any unknown word), and the verifier's 区間 machinery goes away: after a call every variable in scope stays visible. R8 and R9 become R7 and R8 in the docs, CLAUDE.md, code comments and test headings.

Test changes (docs/superpowers/specs/2026-10-15-s10b-switchover-design.md, 「テストの変更」):
- pass/fail (1): deleted the 8 R7 tests in eml_lir verify.rs (a_call_that_does_not_save_an_owned_variable_is_rejected, a_call_that_saves_a_variable_it_does_not_own_is_rejected, a_call_that_saves_a_variable_twice_is_rejected, a_call_that_saves_an_invisible_variable_is_rejected, a_non_rc_variable_not_saved_by_a_call_is_not_visible_after_it, a_merge_block_does_not_see_a_variable_that_one_path_did_not_save, scopes_reject_a_saved_list, a_borrowed_field_cannot_be_saved), the save half of the_lowered_level_rejects_what_perceus_inserts, the 3 save syntax tests in eml_lir text.rs (a_call_with_saved_variables_round_trips, a_tail_call_with_save_is_an_error, an_empty_save_is_an_error), the save row of eml_core_ir text.rs lir_instructions_are_not_core_ir, and eml_core_ir perceus.rs calls_save_the_variables_used_after_them.
- expected value (2): Perceus snapshots lose save [...] (eml_lir perceus.rs, eml_core_ir translate.rs); interpolation_saves_only_the_accumulator_across_holes is rewritten on statement-level liveness as interpolation_keeps_only_the_accumulator_live_across_holes; an_unused_call_result_is_released_and_calls_save_what_is_used_later and an_argument_used_after_the_call_is_dupped_and_saved are renamed an_unused_call_result_is_released and an_argument_used_after_the_call_is_dupped.
- mechanical (3): save [...] removed from hand-written LIR in crates/eml_vm/tests (including ir/*.lir) and crates/eml_lir/tests; the save parameter dropped from call_on_one_path (with call_first, which only steered the deleted 区間 logic) and chain_of_switches; test heading comments renumbered." -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01MK52fNp5uA711Q2D4kWEtR"
```

### Task 12b: Perceus rule 2' (resolve a retained owner before a call)

Spec: 「LIR の `saved` と R7」 items on rule 2' and rule 4, 「テストの変更」 (the two new Perceus tests, the Perceus snapshot moves and renames).

Today rule 2 has two branches at a call: if a borrowed variable `v` is live after the call and its owner `o` is still needed, Perceus `dup`s `v` (the frame could not hold a borrowed value, R7); if `o` is retained, it resolves `o`. 2' keeps only the second branch. Rule 2 also stood in for rule 4 at calls (a call that consumes `o`'s last reference while `v` is live after it), so the rule-4 loop, which today runs only in the `else` branch for non-call statements, must run for every statement. Forgetting that passes every existing test and is a use-after-free in release builds (the reviewer's probe); the two new tests catch it.

**Files:**
- Modify: `crates/eml_lir/src/perceus.rs` (module doc 5-9; `Fields::stmt`, the `take` computation at 227-253 of the pre-12a file)
- Modify: `docs/spec/lir.md` (「値の表現」 guard note; 「所有を取る時点」 item 2 and the resolve bullet; 「持ち主を手放す時点」 「呼び出しの前」)
- Modify: `docs/spec/vm.md` (「スロットの割り当て」 borrowed variables across a call; 「並列化への備え」 owned-slot table)
- Modify: `docs/future/roadmap.md` (「言語機能と構文」 guard item; 借用パラメータ's 降格 sub-item)
- Modify: `CLAUDE.md` (the Perceus sentence on when a field is owned)
- Test: `crates/eml_lir/tests/perceus.rs`

**Interfaces:**
- Consumes: Task 12a (`Rhs::Call(Call)`, no R7); Task 7's `a_retained_owner_does_not_cross_a_call` in `crates/eml_vm/tests/scaling.rs`.
- Produces: Perceus output where a borrowed variable whose owner is needed at a call stays borrowed across the call and is `dup`ped where it is consumed; a retained owner never crosses a call. No API change.

- [ ] **Step 1: Add the two rule-4-at-a-call tests (they pass on the current code)**

Append to `crates/eml_lir/tests/perceus.rs`:

```rust
#[test]
fn a_field_live_after_a_call_that_consumes_its_owner_is_dupped_before_it() {
    // 呼び出しが持ち主の最後の参照を消費し、その後でフィールドを使う形 (docs/spec/lir.md の「所有を取る時点」の 4)
    let shown = perceus_text(
        "\
layout Pair { Pair(obj, obj) }
fn g(s.0: obj) -> obj {
  return s.0
}
fn f(p.0: obj) -> obj {
  unpack p.0 Pair #0(a.1: obj, b.2: obj)
  let r.3: obj = call g(p.0)
  let q.4: obj = con Pair #0(r.3, b.2)
  return q.4
}
",
    );
    insta::assert_snapshot!(shown, @"
    layout Pair { Pair(obj, obj) }
    fn g(s.0: obj) -> obj {
      return s.0
    }
    fn f(p.0: obj) -> obj {
      unpack p.0 Pair #0(a.1: obj, b.2: obj)
      dup b.2
      let r.3: obj = call g(p.0)
      let q.4: obj = con Pair #0(r.3, b.2)
      return q.4
    }
    ");
}

#[test]
fn a_field_live_after_a_call_that_consumes_its_parent_is_owned_by_resolving_the_owner() {
    // 呼び出しが途中の親 `b.2` を消費し、その下のフィールド `d.4` を後で使う形。`d.4` の持ち主は `p.0` である
    let shown = perceus_text(
        "\
layout Pair { Pair(obj, obj) }
fn g(s.0: obj) -> obj {
  return s.0
}
fn h(p.0: obj) -> obj {
  unpack p.0 Pair #0(a.1: obj, b.2: obj)
  unpack b.2 Pair #0(c.3: obj, d.4: obj)
  let r.5: obj = call g(b.2)
  let q.6: obj = con Pair #0(r.5, d.4)
  return q.6
}
",
    );
    insta::assert_snapshot!(shown, @"
    layout Pair { Pair(obj, obj) }
    fn g(s.0: obj) -> obj {
      return s.0
    }
    fn h(p.0: obj) -> obj {
      unpack p.0 Pair #0(a.1: obj, b.2: obj)
      unpack b.2 Pair #0(c.3: obj, d.4: obj)
      dup d.4
      release p.0 Pair #0(_, b.2)
      let r.5: obj = call g(b.2)
      let q.6: obj = con Pair #0(r.5, d.4)
      return q.6
    }
    ");
}
```

```bash
cargo test -p eml_lir --test integration perceus::a_field_live_after_a_call_that 2>&1 | grep -E '^test |test result'
```

Expected: both pass. With the current rule 2, both shapes already go through the call branch (`p.0` is needed at the call in `f`, so `b.2` is `dup`ped; `p.0` is retained at the call in `h`, so it is resolved). These are the regression guards for the trap below; their expected text is the output after 12b too.

- [ ] **Step 2: Write the new expected value of the moved `dup` and the renames (fails)**

In `crates/eml_lir/tests/perceus.rs`:
- rename `a_field_used_after_a_call_is_owned_before_the_call_saves_it` to `a_retained_owner_is_resolved_before_a_call` and replace its comment `// 借りたフィールドは呼び出しの前の `release` で所有にする` with `// `o.0` は呼び出しの後で使わず、`s.1` だけが生きているので、呼び出しの前に `o.0` を解いて `s.1` を受け取る` (its snapshot does not change: `release o.0 Option #1(s.1)` stays before `call k(1)`);
- rename `a_field_live_after_a_call_is_owned_before_it` to `a_field_stays_borrowed_across_a_call_while_its_owner_is_needed`, replace its comment `// 持ち主が死んでいれば解き、生きていれば `dup` する` with the two lines

```rust
    // 持ち主が死んでいれば呼び出しの前に解く。生きていれば、フィールドは借りたまま呼び出しをまたぎ、消費する所で
    // `dup` する
```

  and change `live_owner` in its snapshot from

```
    fn live_owner(p.0: obj) -> obj {
      unpack p.0 Pair #0(a.1: obj, b.2: obj)
      dup a.1
      dup b.2
      let r.3: obj = call g(a.1)
      decref r.3
      let q.4: obj = con Pair #0(p.0, b.2)
      return q.4
    }
```

  to

```
    fn live_owner(p.0: obj) -> obj {
      unpack p.0 Pair #0(a.1: obj, b.2: obj)
      dup a.1
      let r.3: obj = call g(a.1)
      decref r.3
      dup b.2
      let q.4: obj = con Pair #0(p.0, b.2)
      return q.4
    }
```

  (`dead_owner` keeps `release p.0 Pair #0(a.1, b.2)` before the call: rule 1 resolves the dead owner when the call consumes `a.1`.)

```bash
cargo test -p eml_lir --test integration perceus::a_field_stays_borrowed_across_a_call_while_its_owner_is_needed 2>&1 | grep -E '^test |test result'
```

Expected: FAILED; the actual output still has `dup b.2` before `let r.3: obj = call g(a.1)`.

- [ ] **Step 3: Show the trap: delete the `dup` branch without running rule 4 at calls**

Temporarily change only the call branch in `Fields::stmt` (`crates/eml_lir/src/perceus.rs`) to take just the variables of retained owners, leaving the rule-4 loop in the `else` branch:

```rust
            take.extend(
                state
                    .lent
                    .iter()
                    .filter(|(owner, _)| !need.contains(**owner))
                    .flat_map(|(_, vars)| vars.iter().copied())
                    .filter(|var| live.contains(var)),
            );
```

(in place of `.lent.values().flatten().filter(|var| live.contains(var))`).

```bash
cargo test -p eml_lir --test integration perceus::a_field_live_after_a_call_that 2>&1 | grep -E '^test |panicked|owner of a live field'
```

Expected: `a_field_live_after_a_call_that_consumes_its_owner_is_dupped_before_it ... FAILED` with the panic `the owner of a live field is owned` (Perceus's `debug_assert` in `take`: after `call g(p.0)` consumed `p.0`, the con consumes the still-borrowed `b.2`). In a release build this would have emitted a read of `b.2` after its owner was given up. The other new test passes. This is why rule 4 must run for every statement.

- [ ] **Step 4: Implement 2' with the rule-4 loop for every statement**

Replace the whole `take` computation in `Fields::stmt` (from the comment `// 消費する変数のほかに、所有を取るのは次の変数である。` through the end of the `if … else { … }`) with:

```rust
        // 消費する変数のほかに、所有を取るのは次の変数である。持ち主の最後の参照をこの文が消費した後も生きている、
        // その持ち主に属する変数。呼び出しの後で生きている、保持されている持ち主に属する変数。後者は持ち主を呼び出しの
        // 前に解く。保持されている持ち主が呼び出しをまたぐと、死んだ兄弟のフィールドも残り、末尾でない再帰で段ごとに
        // 溜まるためである (docs/spec/lir.md の「所有を取る時点」)
        let mut take = consumed.clone();
        for var in &consumed {
            if state.owned.contains(var)
                && !live.contains(var)
                && let Some(vars) = state.lent.get(var)
            {
                take.extend(vars.iter().filter(|var| live.contains(var)));
            }
        }
        if matches!(
            stmt,
            Stmt::Let {
                rhs: Rhs::Call(_),
                ..
            }
        ) {
            take.extend(
                state
                    .lent
                    .iter()
                    .filter(|(owner, _)| !need.contains(**owner))
                    .flat_map(|(_, vars)| vars.iter().copied())
                    .filter(|var| live.contains(var)),
            );
        }
```

`take` keeps its `dup` branch: rules 1, 3 and 4 still `dup` when the owner is needed. Only the call branch never reaches it now, because it passes only variables whose owner is not needed.

Replace the module doc's second sentence (lines 5-9 of `perceus.rs`):

```rust
//! `switch` と `unpack` のフィールドは値から借りて始まり、読むだけなら借りたまま使う。所有を取るのは、消費するとき、
//! 合流するブロックの入口で生きているとき、持ち主の最後の参照を消費した後も生きているとき、保持されている持ち主に
//! 属して呼び出しの後で生きているときだけである。持ち主がその命令で要るならフィールドを `dup` し、要らないなら
//! 持ち主を `release` で解いてフィールドを受け取る (docs/spec/lir.md の「Perceus」)。こうすると、分解した後に使わない
//! 持ち主のフィールドは `dup` されず、`Lin` の物体の参照は1つのまま保たれる。
```

(it replaces the five lines from `//! \`switch\` と \`unpack\` のフィールドは値から借りて始まり` to `//! 持ち主のフィールドは \`dup\` されず、\`Lin\` の物体の参照は1つのまま保たれる。`).

```bash
cargo fmt
cargo test -p eml_lir -p eml_core_ir 2>&1 | grep -E '^test .*FAILED|test result'
cargo test -p eml_vm --test integration scaling::a_retained_owner_does_not_cross_a_call 2>&1 | grep -E '^test |test result'
```

Expected: all pass, including Step 2's test and the two new tests. In `eml_lir` and `eml_core_ir`, `live_owner` is the only Perceus snapshot that changes (checked on a scratch copy). `a_retained_owner_does_not_cross_a_call` still passes: the resolve branch is what it guards. The reviewer measured the probe it is built on at 152 / 252 `peak_objects` (n = 100 / 200) with 2', against 5201 / 10401 without the resolve branch.

Should any other Perceus snapshot change (for example from a test Task 7 ported), accept it with `cargo insta test -p <crate> --accept` and check with `git diff --no-ext-diff` that the only change is a `dup` of a borrowed variable moving from before a call to the point where it is consumed. No `release` or `decref` may appear, disappear or move.

- [ ] **Step 5: Update the documents for 2'**

```bash
python3 - <<'PY'
import pathlib

def edit(path, pairs):
    p = pathlib.Path(path)
    s = p.read_text()
    for old, new in pairs:
        assert s.count(old) == 1, (path, old[:60], s.count(old))
        s = s.replace(old, new)
    p.write_text(s)

edit("docs/spec/lir.md", [
    ("  - この議論は、決定木の途中のブロックに呼び出しがないことに依る。今の決定木の途中は `switch`、`unpack`、`unbox` だけでできている (ガードと as パターンはない)。ガードを入れると、ガードの呼び出しの後で、後の行が使う分解した値と `Lin` のフィールドが両方生きる形ができる。呼び出しの後で生きる借りた変数の所有を取る規則が、そこで `File` を `dup` する ([ロードマップ](../future/roadmap.md) の「言語機能と構文」)",
     "  - この議論は、決定木の途中で、後の行が持ち主を使うフィールドを消費しないことに依る。今の決定木の途中は `switch`、`unpack`、`unbox` だけでできていて、どれも値を読むだけである (ガードと as パターンはない)。ガードを入れると、ガードが呼び出しの引数としてフィールドを渡し、後の行が持ち主を使う形ができる。フィールドを消費する時点で持ち主が必要なので、Perceus はそこで `File` を `dup` する ([ロードマップ](../future/roadmap.md) の「言語機能と構文」)"),
    ("2. v が呼び出し (`call`、`apply`、`clause`) の後で生きている\n",
     "2. v が呼び出し (`call`、`apply`、`clause`) の後で生きていて、v の持ち主 o がその呼び出しで必要でない (o が保持されている)。o が必要なら、v は借りたまま呼び出しをまたぐ。o は呼び出しの後でも使われるのでそこまで生きていて、v も有効なままである\n"),
    ("- o が必要でない (保持されている) なら、命令の前で o を解く (下の「持ち主を解く」)。o に属する生きている変数は、まとめて所有になる\n",
     "- o が必要でない (保持されている) なら、命令の前で o を解く (下の「持ち主を解く」)。o に属する生きている変数は、まとめて所有になる。2. はいつもこの場合である\n"),
    ("- 呼び出しの前: 上の「所有を取る時点」の 2. のとおり、呼び出しの後で生きている借りた変数をすべて所有にするので、保持されている変数が呼び出しをまたぐことはない",
     "- 呼び出しの前: 上の「所有を取る時点」の 2. のとおり、保持されている持ち主を解くので、保持されている変数は呼び出しをまたがない。保持されている持ち主が呼び出しをまたぐと、その死んだ兄弟のフィールドも残り、末尾でない再帰では段ごとに溜まるためである"),
])

edit("docs/spec/vm.md", [
    ("- 呼ぶ側のスロットは、呼び出しの間もスタックに残る。そのため、呼び出しの後で使う変数を退避しない",
     "- 呼ぶ側のスロットは、呼び出しの間もスタックに残る。そのため、呼び出しの後で使う変数を退避しない。借りた変数が呼び出しをまたぐときは、その持ち主もスロットに残るので、借りた変数は有効なままである ([LIR とインタプリタ](lir.md) の「所有を取る時点」)"),
    ("a2-cancel で走っている子を安全点で取り消すときの後始末に使えるが、スロットの割り当てが静的なので、後から生存解析で作れる。",
     "a2-cancel で走っている子を安全点で取り消すときの後始末に使える。スロットの割り当てが静的なので、後から作れる。ただし、生きているスロットには借りた語もあるので、表は生存解析だけでは決まらない。Perceus の所有の状態 (verifier の所有の段と同じ計算) から作る。"),
])

edit("docs/future/roadmap.md", [
    ("今の Perceus が `Lin` のフィールドを `dup` しないのは、決定木の途中のブロックに呼び出しがないことに依る ([LIR とインタプリタ](../spec/lir.md) の「値の表現」)。ガードを入れると、ガードの呼び出しの後で、後の行が使う分解した値と `Lin` のフィールドが両方生きる形ができる。呼び出しの後で生きる借りた変数は所有を取るので ([LIR とインタプリタ](../spec/lir.md) の「所有を取る時点」)、`File` が `dup` され、ヒープの検査が止める。ガードを入れるときは、呼び出しをまたいで借用を保つ形 (フレームが借りた値と所有した値を区別して持ち、中断でフレームを捨てるときの解放の規則も変える。借用パラメータと一緒に考える) か、別の変換が要る",
     "今の Perceus が `Lin` のフィールドを `dup` しないのは、決定木の途中で、後の行が持ち主を使うフィールドを消費しないことに依る ([LIR とインタプリタ](../spec/lir.md) の「値の表現」)。ガードを入れると、ガードが呼び出しの引数として `Lin` のフィールドを渡し、後の行が持ち主を使う形ができる。フィールドを消費する時点で持ち主が必要なので、Perceus はそのフィールドを `dup` し ([LIR とインタプリタ](../spec/lir.md) の「所有を取る時点」)、`File` の `dup` をヒープの検査が止める。ガードを入れるときは、ガードが `Lin` のフィールドを消費しないという型の規則か、別の変換が要る"),
    ("呼び出しの後で生きる借りたフィールドは呼び出しの前に所有になる ([LIR とインタプリタ](../spec/lir.md) の「所有を取る時点」) ので、借用パラメータを入れるまで降格は起きない。",
     "今は `tail` の前に実引数をすべて所有にし、保持している変数をすべて手放す ([LIR とインタプリタ](../spec/lir.md) の「持ち主を手放す時点」) ので、借用パラメータを入れるまで降格は起きない。"),
])

edit("CLAUDE.md", [
    ("(consumed, live after a call, live at a jump target, or live after its owner's last reference is consumed)",
     "(consumed, live at a jump target, live after its owner's last reference is consumed, or live after a call while its owner is retained, so a retained owner never crosses a call)"),
])
PY
```

This rewrites, per the spec: lir.md 「所有を取る時点」 item 2 as 2' (its numbering stays "2."), with "2. はいつもこの場合である" on the resolve bullet; the 「呼び出しの前」 item, now giving space as the reason; the 「値の表現」 guard note, whose premise changes from "no call in the middle of a decision tree" to "the middle of a decision tree consumes no field whose owner a later row uses"; vm.md's statement that a borrowed variable stays valid across a call because its owner stays in a slot, and the owned-slot table sentence in 「並列化への備え」 (built from Perceus's ownership state, not from liveness alone); roadmap.md's guard item, and the 降格 sub-item, whose reason no longer relies on rule 2; and CLAUDE.md's Perceus sentence.

- [ ] **Step 6: Full gate**

```bash
cargo test
cargo clippy --all-targets
cargo fmt --check
```

Expected: everything passes, clippy prints no warning, fmt prints nothing. UI tests (VM, checked and fast mode) are unchanged; the reviewer measured all 199 `run` / `run-fail` programs byte-identical, so no UI snapshot changes. `crates/eml_vm/tests/bench.rs` is expected to be unchanged too (the reviewer measured identical `rc_*` and `peak_objects` for all six programs; the spec calls this "may change"). If a count does change, accept it as kind 2 and name it in the commit message. `scaling.rs` must pass unchanged. Miri is not needed: `crates/eml_vm/src` is untouched.

- [ ] **Step 7: Commit**

```bash
git add -u crates/eml_lir docs CLAUDE.md
git commit -m "Perceus: resolve a retained owner before a call, keep needed owners' fields borrowed" -m "Rule 2 of 所有を取る時点 dupped a borrowed variable live after a call when its owner was still needed, because a CEK frame could not hold a borrowed value. The VM keeps the owner in the caller's slot, and native code keeps it alive through its later decref or release, so that branch goes (rule 2'). The resolve branch stays for space: a retained owner crossing a call would keep its dead sibling fields alive at every level of a non-tail recursion. Rule 2 also stood in for rule 4 at calls, so the rule-4 loop now runs for every statement; without it, a call that consumes the owner of a field live after it is a use-after-free in release builds.

Test changes (docs/superpowers/specs/2026-10-15-s10b-switchover-design.md, 「テストの変更」):
- new: a_field_live_after_a_call_that_consumes_its_owner_is_dupped_before_it and a_field_live_after_a_call_that_consumes_its_parent_is_owned_by_resolving_the_owner (rule 4 at a call).
- expected value (2): in a_field_stays_borrowed_across_a_call_while_its_owner_is_needed (was a_field_live_after_a_call_is_owned_before_it), live_owner's dup b.2 moves from before the call to the con that consumes it; a_field_used_after_a_call_is_owned_before_the_call_saves_it is renamed a_retained_owner_is_resolved_before_a_call (output unchanged)." -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01MK52fNp5uA711Q2D4kWEtR"
```

### Task 13: `bench/run.sh` measures only the VM, and the after-switch record

Spec items: 「S10a の残りの整理」 12, 「作業の順」 7, 「文書」 (the `benchmarks.md` row: 「回数のテスト」, 「命令の数の測り方」, the after-switch numbers in 「S10b」).

**Files:**
- Modify: `bench/run.sh` (whole file replaced: one engine, the execution-part column, the ≤ 0 guard, no bash-4 features)
- Modify: `docs/implementation/benchmarks.md` (sections 「回数のテスト」 and 「命令の数の測り方」 rewritten; the `bench/rt.sh` paragraph's stale reason removed; the after-switch record appended to 「### S10b …」 that Tasks 2/6 created)
- Test: none (a script and a document). `cargo test -p eml_cli --test integration citations` pins the document's citations.

**Interfaces:**
- Consumes: the release `eml` binary after Task 10/11 (`eml run [--debug-heap] <file>` runs on the VM; `--vm` is a usage error). The 「### S10b …」 heading and the before-switch VM table that Task 6 wrote in `docs/implementation/benchmarks.md`.
- Produces: `bench/run.sh` prints the environment lines and one table `| プログラム | instructions retired | 実行の分 | 実時間 (s) |`. Task 14 cites `docs/implementation/benchmarks.md` 「命令の数の測り方」 and 「回数のテスト」 (both headings kept).

- [ ] **Step 1: Check the starting point**

```bash
grep -n '^### S10b' docs/implementation/benchmarks.md
eml_run_vm_flag=$(cargo run -q -p eml_cli -- run --vm bench/empty.em >/dev/null 2>&1; echo $?)
echo "$eml_run_vm_flag"
```

Expected: one line `NNN:### S10b …` (the heading Tasks 2/6 created; note its exact text, Task 14 cites it), and `2` (the `--vm` flag is gone since Task 10, so clap reports a usage error). If the flag still exits 0, Task 10 is not done: stop.

- [ ] **Step 2: Replace `bench/run.sh`**

Write the whole file:

```bash
#!/usr/bin/env bash
# 基準のプログラムを release ビルドの `eml run` (VM の速いモード) で3回ずつ走らせ、実行した命令の数と実時間の中央値と、
# 実行の分 (命令の数から `empty` の命令の数を引いた値) を Markdown の表で出す
# (docs/implementation/benchmarks.md の「命令の数の測り方」)。命令の数は macOS の `/usr/bin/time -l` の
# `instructions retired` で測る。valgrind が arm64 の macOS で動かないためである。
set -euo pipefail

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "bench/run.sh: macOS の /usr/bin/time -l の instructions retired を使うので、macOS でだけ動く" >&2
  exit 1
fi

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# 中央値を1つの値に決めるため、奇数にする
runs=3

cargo build --release -p eml_cli --manifest-path "$root/Cargo.toml" >&2
eml="$root/target/release/eml"

# `runs` 個の値の中央値
median() {
  printf '%s\n' "$@" | sort -n | sed -n "$(( (runs + 1) / 2 ))p"
}

# プログラム `$1` を `runs` 回走らせ、命令の数の中央値と実時間の中央値を空白で区切って出す
measure() {
  local file="$1"
  local name report count elapsed
  local instructions=()
  local seconds=()
  name="$(basename "$file" .em)"
  for _ in $(seq "$runs"); do
    # 実行時エラーで止まったら、捕まえた出力を見せて止める。表の値が欠けたまま記録しないためである
    if ! report="$( { /usr/bin/time -l "$eml" run "$file" >/dev/null; } 2>&1 )"; then
      echo "bench/run.sh: $name が失敗した" >&2
      echo "$report" >&2
      exit 1
    fi
    count="$(awk '/instructions retired/ { print $1 }' <<<"$report")"
    elapsed="$(awk '/ real / { print $1 }' <<<"$report")"
    # macOS の版や実行の環境によって `time -l` が数を出さないと、表に空の欄が混ざるので止める
    if [[ ! "$count" =~ ^[0-9]+$ || ! "$elapsed" =~ ^[0-9]+\.[0-9]+$ ]]; then
      echo "bench/run.sh: $name の /usr/bin/time -l の出力から命令の数か実時間を読めなかった" >&2
      echo "$report" >&2
      exit 1
    fi
    instructions+=("$count")
    seconds+=("$elapsed")
  done
  echo "$(median "${instructions[@]}") $(median "${seconds[@]}")"
}

echo "- 機種: $(sysctl -n hw.model) ($(sysctl -n machdep.cpu.brand_string))"
echo "- OS: macOS $(sw_vers -productVersion) ($(sw_vers -buildVersion))"
echo "- rustc: $(rustc -V)"
echo "- コミット: $(git -C "$root" describe --always --dirty --abbrev=7)"
echo
echo "| プログラム | instructions retired | 実行の分 | 実時間 (s) |"
echo "|---|---:|---:|---:|"
# 実行の分は `empty` (フロントエンド、コード生成、起動の費用) を引いた値なので、`empty` を先に測る。
# 代入の形で受けるのは、`measure` が止まったときに `set -e` でこのスクリプトも止めるためである
result="$(measure "$root/bench/empty.em")"
read -r base base_seconds <<<"$result"
echo "| \`empty\` | $base | | $base_seconds |"
for file in "$root"/bench/*.em; do
  name="$(basename "$file" .em)"
  [[ "$name" == empty ]] && continue
  result="$(measure "$file")"
  read -r count elapsed <<<"$result"
  part=$(( count - base ))
  # 0 以下の実行の分は、段の前後で比べられる値でない。表に残さずに止める
  if (( part <= 0 )); then
    echo "bench/run.sh: $name の実行の分 ($name の命令の数 $count - empty の命令の数 $base) が $part で、0 以下になった。" >&2
    echo "bench/run.sh: $name の実行が empty の揺れより小さいか、empty の費用が増えている。empty と $name を個別に測って調べる" >&2
    exit 1
  fi
  echo "| \`$name\` | $count | $part | $elapsed |"
done
```

Design notes for the reviewer: one engine needs no associative array or name reference, so the bash-4.3 check is gone (macOS `/bin/bash` 3.2 runs it). `empty` is measured first explicitly instead of relying on glob order. The failure message no longer carries `($flag)` (spec item 12). The guard stops with the reason, as the spec asks.

- [ ] **Step 3: Check the script statically**

```bash
bash -n bench/run.sh && echo syntax-ok
command -v shellcheck >/dev/null && shellcheck bench/run.sh || echo "shellcheck not installed"
grep -nE 'CEK|--vm|declare -A|local -n' bench/run.sh || echo clean
```

Expected: `syntax-ok`; shellcheck prints nothing (or "not installed"); `clean`.

- [ ] **Step 4: Run it and keep the output**

```bash
bench/run.sh | tee "${TMPDIR:-/tmp}/s10b-after.md"
```

Expected (shape; numbers are measured): four environment lines, a blank line, the header row, the separator row, the `empty` row with an empty 実行の分 cell, and five rows `fib`, `list`, `loop`, `state`, `tree` with positive 実行の分. The コミット line must not end in `-dirty`; if it does, commit or stash first and rerun (the record must name a real commit). If the script stops with the ≤ 0 message, do not record anything: report the message to the coordinator.

Run it a second time (`bench/run.sh > "${TMPDIR:-/tmp}/s10b-after-2.md"`) and compute, for each program, `|実行の分₁ - 実行の分₂| / 実行の分₁`. Keep the largest value and its program for Step 6.

- [ ] **Step 5: Rewrite 「回数のテスト」 and 「命令の数の測り方」 in `docs/implementation/benchmarks.md`**

Replace the body of `## 回数のテスト` (the two paragraphs between that heading and `## 命令の数の測り方`) with:

```markdown
`crates/eml_vm/tests/bench.rs` は、プログラムごとに1つのテストで、VM の検査モードで走らせ、出力を確かめ、`RunStats` の全項目をインラインスナップショットで固定する。`bench/` のファイルの一覧と、テストのプログラムの一覧が一致することも確かめる。回数は機械の速さに左右されないので、ふだんの `cargo test` で流す。

段の前後の回数は、このスナップショットの差分で残る。回数を変える変更は、その段の spec に期待値の変更として書く ([テスト戦略](testing.md) の「テストの変更の運用」)。S10a までの記録の回数は S10b で消したインタプリタのもので、`rc_increments`、`rc_decrements`、`peak_objects` は VM と数え方が違う (下の「S10a (VM)」)。`peak_stack` は S10b で足した欄で、それより前の記録にはない。
```

Replace the body of `## 命令の数の測り方` from the code block through the paragraph that ends `…bash 4.3 以上 (devShell の bash) が要る。` with:

````markdown
```sh
bench/run.sh
```

`bench/run.sh` は、release ビルドの `eml run` (VM の速いモード) で各プログラムを3回ずつ走らせ、`instructions retired` と実時間の中央値と、実行の分を Markdown の表で出す。実行の分は、各プログラムの命令の数から `empty` の命令の数を引いた値である。`empty` はフロントエンド、コード生成、起動の費用なので、段の前後は実行の分で比べる。実行の分が 0 以下になったときは、理由を出して止まる。比べられない値を記録しないためである。表の前に、測定の環境 (機種と CPU、macOS の版、`rustc -V`、コミット) を出す。コミットしていない変更があれば、コミットの後に `-dirty` が付く。プログラムが実行時エラーで止まったときと、`time -l` の出力から命令の数か実時間を読めなかったときは、その出力を見せて止まる。macOS でだけ動く。
````

Keep the three paragraphs after it (valgrind, `empty.em`, 段の終わり) unchanged. In the `bench/rt.sh` paragraph, replace the sentence

```
`eml_rt` はまだ `eml run` から使わないので、`bench/` のプログラムでなく、ヒープの操作を直接呼ぶ。
```

with

```
`bench/` のプログラムでなく、ヒープの操作を直接呼ぶ。
```

- [ ] **Step 6: Append the after-switch record to 「### S10b …」**

At the end of the `### S10b …` section (after everything Tasks 2/6 wrote, before the end of the file), append:

````markdown
#### 切り替えた後

切り替えた後の `bench/run.sh` の記録である。`eml run` は VM の速いモードで動く。切り替えの前の記録 (上の表) の VM の実行の分と比べる。

<the full stdout of Step 4, pasted verbatim: the four environment lines, the blank line, and the table>

`bench/run.sh` は2回流した。2回の実行の分の違いは最大 <largest value from Step 4, as a percentage with 2 decimals>% (`<its program>`) だった。

| プログラム | 切り替えの前の VM の実行の分 | 切り替えた後の実行の分 | 後 / 前 |
|---|---:|---:|---:|
| `fib` | <before> | <after> | <after / before, 3 decimals> |
| `list` | … | … | … |
| `loop` | … | … | … |
| `state` | … | … | … |
| `tree` | … | … | … |

<one sentence per program whose 後 / 前 is outside 0.98〜1.02, naming the change in S10b that explains it (Task 1-4 VM cleanups, Task 5 `Task::reserve`, Task 12 Perceus 2'), or "原因は調べていない" when no change explains it. If every ratio is inside 0.98〜1.02, write the single sentence: どのプログラムでも、切り替えの前の VM との差は 2% 以内だった。>

`RunStats` の切り替えの前後の差は、`crates/eml_vm/tests/bench.rs` のスナップショットの差分で残る。
````

The `<…>` slots are measured values and the rules above say exactly how to fill each; nothing else is free text. `before` is the VM 実行の分 that Task 6 recorded in the same `### S10b` section (the `VM の実行の分` column of its comparison table). `after` is the 実行の分 column of Step 4's first run. Write numbers as plain integers like the other records.

Run the yomiyasu linter on the new prose (skill `yomiyasu:yomiyasu`, Step 3 of the skill):

```bash
python3 /Users/arakaki/.claude/plugins/cache/yomiyasu/yomiyasu/1.1.0/skills/yomiyasu/scripts/yomiyasu_lint.py docs/implementation/benchmarks.md
```

Expected: no finding in the lines this task changed (findings in older records are out of scope).

- [ ] **Step 7: Gate**

```bash
cargo test -p eml_cli --test integration citations
cargo test
cargo clippy --all-targets
cargo fmt --check
```

Expected: all pass; clippy reports 0 warnings.

- [ ] **Step 8: Commit**

```bash
git add bench/run.sh docs/implementation/benchmarks.md
git commit -m "Measure only the VM in bench/run.sh and record the switched numbers" -m "bench/run.sh runs eml run (the VM in fast mode) and keeps the execution-part column (program minus empty); it stops with the reason when that part is zero or below. benchmarks.md: the count tests and the measuring method describe the VM only, and the S10b record gets the numbers after the switch. No test changes." -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01MK52fNp5uA711Q2D4kWEtR"
```

---

### Task 14 (overview): documents, split into 14a-14g by document group

Spec section 「文書」, plus the hand-offs from earlier tasks. Every sub-task ends with the citations test green, because a heading that moves breaks every citer in the same commit. The sub-tasks run in this order because each one only cites headings that exist at that point:

| Sub-task | Documents | Why here |
|---|---|---|
| 14a | `docs/spec/vm.md`, `docs/spec/object-model.md` | adds 「上限と割り込み」 to `vm.md`, which 14b and later cite |
| 14b | `docs/spec/lir.md`, `core-ir.md`, `linearity.md` (and the link text everywhere) | renames 「インタプリタ (CEK 機械)」 to 「実行の意味」 |
| 14c | `docs/implementation/testing.md` | renames 「VM でも流す」 to 「2つのモードで流す」 |
| 14d | `docs/implementation/architecture.md`, `status.md` | 「段の終わり」 documents; renames the internals heading |
| 14e | `docs/future/roadmap.md`, `multicore.md`, `docs/overview.md`, `README.md`, `docs/README.md`, `CLAUDE.md` | overview documents and history links |
| 14f | code comments, `std/Prelude.em`, delete `docs/spec/runtime.md`, final grep | `runtime.md` can go only after nothing links to it |
| 14g | delete the S10b spec and plan | project process |

Rule destinations (spec 「文書」, first table), and where this plan writes them:

| Rule (from) | Destination | Sub-task, step |
|---|---|---|
| `runtime.md` 「不死の物体」: LIR keeps `dup`/`decref` balanced for literals; no pass drops them | `lir.md` 「値の表現」 | 14b Step 5 |
| `runtime.md` 「文字列の連結」 | already in `object-model.md` 「文字列の操作」; drop its `runtime.md` citation | 14a Step 7 |
| `runtime.md` 「`File` の参照の検査」 | already in `object-model.md` 「`lin` ビット」, 「報告する誤り」, 「受け入れる制約」; `lir.md` 「実行時エラー」 cites them | 14b Step 8 |
| `runtime.md` 「実行の API」: `RunStats` field meanings, "counts work, not time", `RunConfig` `#[non_exhaustive]` | `vm.md` 「`RunStats`」 and 「置き場所」 | 14a Steps 3, 6 |
| `runtime.md` 「`debug_heap`」: `--debug-heap` is checked mode; UI tests always run checked | `vm.md` 「置き場所」, `testing.md` 「2つのモードで流す」 | 14a Step 3, 14c Step 6 |
| `runtime.md` other sections (heap, header, API, evidence objects) | not moved; `object-model.md` defines `eml_rt`'s versions | 14f deletes the file |
| `lir.md` 「インタプリタ (CEK 機械)」 | same place, renamed 「実行の意味」, engine-independent rules only | 14b Step 7 |
| `lir.md` 「実行時の規約」 | deleted | 14b Step 9 |
| `lir.md` 「実行時エラー」 | rewritten | 14b Step 8 |

Per-document list (spec 「文書」, second table), and where:

| Document | Sub-task |
|---|---|
| `docs/spec/lir.md` (title, link text, R7, Perceus 2', guard note) | 14b (R numbering and most Perceus text belong to Task 12; 14b checks and completes) |
| `docs/spec/linearity.md`, `docs/spec/core-ir.md` | 14b |
| `docs/spec/vm.md`, `docs/spec/object-model.md` | 14a |
| `docs/future/multicore.md`, `docs/overview.md` | 14e |
| `docs/implementation/testing.md` | 14c |
| `docs/implementation/benchmarks.md` | Task 13 (done), history-link rule in 14e Step 2 |
| `docs/future/roadmap.md` | 14e |
| `docs/implementation/architecture.md`, `docs/implementation/status.md` | 14d |
| `CLAUDE.md`, `README.md`, `docs/README.md` | 14e |
| code comments (`eml_vm`, `eml_extern`, `eml_core_ir`, `eml_lir`, `eml_cli` `main.rs`, `std/Prelude.em`, `eml_rt`) | 14f |

All Japanese prose below was written under the `yomiyasu:yomiyasu` rules. The implementer still invokes the skill (Skill tool, `yomiyasu:yomiyasu`) once before 14a and applies it to any sentence they have to adapt, and runs its linter on each changed document:

```bash
python3 /Users/arakaki/.claude/plugins/cache/yomiyasu/yomiyasu/1.1.0/skills/yomiyasu/scripts/yomiyasu_lint.py <changed .md file>
```

Findings in lines this task did not change are out of scope.

Edits are given as "old text → new text". Old texts are quoted from the tree at the start of S10b; when an earlier task already changed a line, the step says what to check. Use the Edit tool for multi-line replacements.

### Task 14a: `vm.md` and `object-model.md`

**Files:**
- Modify: `docs/spec/vm.md` (intro; 「置き場所」; 「スロットとスタック」; 「スロットの割り当て」; 「switch」; 「誤り、止めること、終わり」; new section 「上限と割り込み」; 「`RunStats`」 rewritten)
- Modify: `docs/spec/object-model.md` (intro; 「ヘッダ」; 「文字列の操作」; 「検査モード」; 「モードの選び方」; 「不死の物体の数え方」; 「実行の制御」; 「速いモードと検査モード」; 「語の ABI」; extern 「誤り」; 「仕事の回数」; 「受け入れる制約」)
- Test: `crates/eml_cli/tests/citations.rs` (run, not changed)

**Interfaces:**
- Consumes: Task 5 (`RunStats::peak_stack`, `Task::reserve` cap), Task 8 (`RunConfig`, `RuntimeError`, `RuntimeError::interrupted`, re-exports), Task 11 (`eml_cli::default_limits`, SIGINT handler, exit code 130).
- Produces: headings `vm.md` 「上限と割り込み」 (new), `vm.md` 「`RunStats`」 (kept, rewritten). Later sub-tasks cite both.

- [ ] **Step 1: Check what earlier tasks already wrote**

```bash
grep -n 'execute_with\|VmError\|eml_interp\|CEK\|--vm\|S10a の間\|saved\|peak_stack\|reserve\|eml_vm` の中の関数' docs/spec/vm.md
```

Expected now: hits on lines 5, 13, 17-20, 90-91, 156-157, 170-178, 185, 194-199 (the lines the steps below replace). Lines that a step replaces but that an earlier task already rewrote: compare with the new text below, keep the step's new text (it is the complete version), and note it in the commit body. If `grep -n 'eml_vm` の中の関数' docs/spec/vm.md` already prints a line, Task 2 added the inline rule: skip that bullet in Step 3.

- [ ] **Step 2: Intro of `vm.md`**

Old (line 5):

```
VM は、LIR をレジスタ型のバイトコードに変換し、`eml_rt` の `Ctx` の上で実行する。S10a の間は CEK と並べて置き、既定の `eml run` は CEK のままで、`eml run --vm` のときだけ VM が動く。S10b で `eml run` を VM に切り替える。LIR の命令と規則は [LIR とインタプリタ](lir.md) に、物体の形と `Ctx` の基本操作は [オブジェクトモデル](object-model.md) にある。ここでは、バイトコードの形、フレーム、呼び出しの規約、命令ごとの実行、誤りと終わりの処理、`RunStats` の数え方を定める。テストは [テスト戦略](../implementation/testing.md) の「`eml_vm` のテスト」と「VM でも流す」に、計測は [計測の基準](../implementation/benchmarks.md) の「S10a (VM)」にある。
```

New:

```
VM は、LIR をレジスタ型のバイトコードに変換し、`eml_rt` の `Ctx` の上で実行する。`eml run` は VM で動く。LIR の命令と規則は [LIR](lir.md) に、物体の形と `Ctx` の基本操作は [オブジェクトモデル](object-model.md) にある。ここでは、実行の API、バイトコードの形、フレーム、呼び出しの規約、命令ごとの実行、誤りと終わりの処理、上限と割り込み、`RunStats` の意味と数え方を定める。テストは [テスト戦略](../implementation/testing.md) の「`eml_vm` のテスト」と「VM でも流す」に、計測は [計測の基準](../implementation/benchmarks.md) にある。
```

(「VM でも流す」 stays here on purpose: 14c renames the heading and rewrites every citer with one command.)

- [ ] **Step 3: 「置き場所」**

Old (line 13):

```
- `eml_vm` は、自分の `RunStats` と実行時エラーの型 `VmError` を持ち、`eml_interp` には依存しない。`VmError` の表示は、`eml_interp::RuntimeError` と1文字も違わない形にする
```

New:

```
- `eml_vm` は、自分の `RunStats` と実行時エラーの型 `RuntimeError` を持つ。実行時エラーの型は1つだけなので、`RuntimeError` と呼ぶ。誤りの種類は `VmFault` と呼ぶ。`Fault` にすると、包んでいる `eml_rt::Fault` と名前がぶつかるためである
```

Old (line 17, the `eml_rt` inline bullet) stays. Task 2 added the `eml_vm` inline rule next to it; Step 1's last grep must print it. If it does not (Task 2 incomplete), insert this bullet after the `eml_rt` one and say so in the commit body:

```
- `eml_vm` の中の関数には、インライン化の注釈 (`-C remark=inline`) で展開されていないと分かり、`bench/run.sh` で命令の数が減ると分かったときだけ `#[inline]` を足す。足すかどうかと測った数は [計測の基準](../implementation/benchmarks.md) に書く
```

Old (line 18):

```
- release のプロファイルの `codegen-units` は既定の値のままにする。1 にすると、VM の実行の分の命令の数はプログラムによって減ったり増えたりし (`state` で 14.2% 減、`fib` で 2.5% 増)、CEK の記録との比べ方も変わるためである ([計測の基準](../implementation/benchmarks.md) の「S10a (VM)」)
```

New:

```
- release のプロファイルの `codegen-units` は既定の値のままにする。1 にすると、VM の実行の分の命令の数はプログラムによって減ったり増えたりしたためである (`state` で 14.2% 減、`fib` で 2.5% 増。[計測の基準](../implementation/benchmarks.md) の「S10a (VM)」)
```

Old (lines 19-20, two bullets):

```
- `eml_cli` は、`run` の feature で `eml_vm` に依存し、隠しフラグ `eml run --vm` のときだけ `eml_vm` を呼ぶ。`--vm` は速いモードで動き、`--debug-heap` を付けると検査モードになる。Host は `StdHost::stdio` で、SIGINT のハンドラはまだない。終了コードは CEK と同じである
- 実行の関数は `eml_vm::execute(program: &eml_lir::Program, debug_heap: bool, host: HostRef) -> Result<RunStats, VmError>` である。`file_root` は Host が持つので、`RunConfig` は受け取らない。`Control` は上限のない既定の値で作る。割り込みと上限のテストのために、`Control` を受け取る `execute_with` もある。`eml_cli` は `execute` と同じ形の `execute_vm` を公開する
```

New:

```
- 実行の関数は `eml_vm::execute(program: &eml_lir::Program, config: &RunConfig, host: HostRef) -> Result<RunStats, RuntimeError>` の1つである。`RunConfig` は、検査モードかどうか (`debug_heap`) と、実行を止める情報 (`control: Arc<Control>`) を持つ。`file_root` は Host が持つので、`RunConfig` は持たない
- `RunConfig` は `#[non_exhaustive]` にする。欄を足しても呼び出し側を壊さないためである。`RunConfig::default()` は、速いモードと、上限のない新しい `Control` (`Control::new(Limits::default())`) である。`Control` は `Default` を持たないので、`Default` は手で書く。`with_debug_heap(bool)` と `with_control(Arc<Control>)` で組み立てる
- 1つの `Control` は1回の実行に使う。止める理由のビットは `Control::clear` でしか下りないので、使い回すと、前の実行で立ったビットで次の実行がすぐ止まるためである。`RunConfig` を `clone` すると `Control` を共有するので、UI テストの仕組みと `eml_test_support` は実行ごとに新しい `RunConfig` を作る
- `RunConfig` に `Control` を持たせるのは、CLI が SIGINT のハンドラに同じ `Control` を渡すためである。上限は `Control` が持つので、`RunConfig` に上限の欄は作らない ([オブジェクトモデル](object-model.md) の「実行の制御」)
- `eml_vm` は、`eml_rt` の `Control`、`Limits`、`Stop` を再公開する。`eml_cli` は `eml_rt` に依存せず、これらの名前を `eml_vm` から受け取る
- `eml_cli` は、`run` の feature で `eml_vm` に依存する。`RunConfig`、`RunStats`、`RuntimeError`、`VmFault`、`SourceLocation`、`Control`、`Limits`、`Stop`、`HostRef`、`Capture`、`capture_host`、`stdio_host` を `eml_vm` から再公開し、`execute` を同じ名前で公開する
- `eml run` は速いモードで動き、`--debug-heap` を付けると検査モードになる。UI テストは、いつも検査モードで流してから速いモードでも流す ([テスト戦略](../implementation/testing.md) の「VM でも流す」)。Host は `stdio_host(PathBuf::new())` で、相対パスはカレントディレクトリを基準にする
```

- [ ] **Step 4: 「スロットとスタック」**

After the bullet `- 深さはスタックの使っている語の数 (`base + frame_size`) で、`Ctx::poll` に渡す` (line 53), add (skip if Task 5 already added an equivalent `Task::reserve` bullet; then keep the more complete one):

```
- `Task::reserve` は、スタックの `Vec` の容量を `min(2 × 今の容量, max_depth + u16::MAX + 2)` 語まで伸ばす (`reserve_exact`)。`Vec` に任せると容量が倍に伸びるので、深さの上限を超えた容量を取りうるためである。`u16::MAX + 2` は、安全点の前に積みうる1つの枠と見出しの分である。これで、スタックのメモリは既定の上限で約 1 GiB を超えない
- スタックはヒープの外にあるので、確保量の上限 (`max_bytes`) には数えず、深さの上限 (`max_depth`) で抑える (「上限と割り込み」)
```

- [ ] **Step 5: 「スロットの割り当て」 and 「switch」**

Task 12 owns the `saved` bullet (line 91) and the owned-slot-table sentences (line 88 and 「並列化への備え」 line 216). Check it did them:

```bash
grep -nE 'saved|後から生存解析で作れる|表を作ることもできる' docs/spec/vm.md
```

Expected: no output. If anything prints, Task 12 is incomplete: stop and report to the coordinator.

Old (line 90):

```
- ブロック引数と実引数に同じ色を付けやすくする合併は行わない。移動の命令を減らす工夫だが、合併なしで VM の実行の分の命令の数が CEK の約 0.10〜0.17 倍に収まったので、簡潔さを先にした ([計測の基準](../implementation/benchmarks.md) の「S10a (VM)」)
```

New:

```
- ブロック引数と実引数に同じ色を付けやすくする合併は行わない。移動の命令を減らす工夫だが、合併なしで VM の実行の分の命令の数が、S10a で比べた前のインタプリタの約 0.10〜0.17 倍に収まったので、簡潔さを先にした ([計測の基準](../implementation/benchmarks.md) の「S10a (VM)」)
```

Old (line 156, in 「switch」), the clause:

```
融合なしで VM の実行の分の命令の数が CEK の約 0.10〜0.17 倍に収まったので、簡潔さを先にした
```

New:

```
融合なしで VM の実行の分の命令の数が、S10a で比べた前のインタプリタの約 0.10〜0.17 倍に収まったので、簡潔さを先にした
```

Old (line 157), the tail of the `unpack` bullet:

```
違えば内部の誤りにする (CEK の `unpack` の見張りと同じ)
```

New:

```
違えば内部の誤りにする
```

- [ ] **Step 6: 「誤り、止めること、終わり」, the new 「上限と割り込み」, and 「`RunStats`」**

In 「誤り、止めること、終わり」 (lines 170-181), make these replacements (if Task 8 already renamed `VmError`, only the CEK parts remain):

| Old | New |
|---|---|
| `- 実行時エラー `VmError` は、` | `- 実行時エラー `RuntimeError` は、` |
| `- 表示の形は CEK と同じである。位置があれば` | `- 表示の形は次のとおりである。位置があれば` |
| `末尾の位置の部分適用が作ったクロージャを返すときも、関数の中で起きたなら同じく覚える。CEK も、最後に動いていた関数の名前を付けている` | `末尾の位置の部分適用が作ったクロージャを返すときも、関数の中で起きたなら同じく覚える` |
| `- `Arith`、`Output`、`Io`、`NotUtf8` の文言は `eml_rt` のものを使う。CEK と1文字も違わないことは、`crates/eml_rt/src/heap/tests/expected.txt` の表で確かめてある ([テスト戦略](../implementation/testing.md) の「期待値の表」)。`Heap(_)` の文言は `eml_rt::HeapError` のものになる` | `- `Arith`、`Output`、`Io`、`NotUtf8`、`Heap(_)` の文言は `eml_rt` のものを使う ([オブジェクトモデル](object-model.md) の「誤り」)。extern の結果と文言は、`crates/eml_rt/src/heap/tests/expected.txt` の表が固定する ([テスト戦略](../implementation/testing.md) の「期待値の表」)` |
| `  1. 入口の関数の結果を手放す。Repr が `obj` か `tobj` のときだけ `decref` する。CEK のいちばん下のフレームも、結果を手放してから終わる` | `  1. 入口の関数の結果を手放す。Repr が `obj` か `tobj` のときだけ `decref` する` |

After the bullet that begins `- 安全点の `Ctx::poll` が返す `Stop` と extern の `Fault::Stopped` は、` add:

```
- 割り込みで止めたときの表示も、ほかの実行時エラーと同じ形である。安全点で止めたときは ``interrupted in `f` `` (`f` は止めたときの関数)、`Fs.read_all` の中で止めたときは位置付きの `interrupted` である。`RuntimeError::interrupted()` は、安全点の `Stopped(Stop::Interrupted)` と、`Fs.read_all` の中で止めた `Extern(Fault::Stopped(Stop::Interrupted))` の両方で真を返す。CLI が終了コードを決めるのに使う (「上限と割り込み」)
```

Between the end of 「誤り、止めること、終わり」 (after item 4 of the `Finish` list) and `## `RunStats``, insert the new section:

```markdown
## 上限と割り込み

`eml run` の既定の上限は、`eml_cli::default_limits() -> Limits` が作る。`eml_rt` を vstd だけに依存したままにするため、OS に問い合わせるコードは `eml_cli` に置く。

| 上限 | 既定の値 | 理由 |
|---|---|---|
| `max_depth` | 2^27 語 (1 GiB 分のスタック) | OCaml 5 のスタックの既定の上限 (128M 語) と同じ量である。1段が 8 語なら、約 1600 万段まで入る |
| `max_bytes` | 物理メモリの 80% | GHC がスタックの上限 (`-K`) の既定を物理メモリの 80% にしているのに倣う。ヒープの上限としての前例ではなく、「機械を使い切る前に、分かる誤りで止める」という考え方を借りる。物理メモリを読めない OS では上限なし (`usize::MAX`) にする |
| `max_evv` | 上限なし | `evv` はヒープの物体なので、確保量の上限で抑えられる |

- 物理メモリは `sysconf(_SC_PHYS_PAGES) * sysconf(_SC_PAGESIZE)` で読む。macOS と Linux のどちらもこの2つを持つ。`eml_cli` の `src/` の `unsafe` の呼び出しは、この関数の中だけに置く
- Linux のコンテナでは、物理メモリは cgroup の上限を反映しない。上限が cgroup より大きくなり、分かる誤りの前に OOM で殺されうる。cgroup を読むのは要るときに足す
- VM のスタックはヒープの外にあるので `max_bytes` に数えず、`max_depth` で抑える (「スロットとスタック」)
- 検査モードは解放したブロックを使い回さないので、`--debug-heap` の長い実行は、速いモードより先に `max_bytes` に当たりうる。暴走を止める安全網という上限の役 ([オブジェクトモデル](object-model.md) の「確保と解放」) は変わらないので、受け入れる
- CLI のフラグ (`--max-depth` など) は足さない。要るとき (REPL など) に足す
- 深さか確保量の上限で止まったときは ``the call depth limit is exceeded in `f` `` か ``the memory limit is exceeded in `f` `` と表示し、終了コードは 1 である

`eml run` は、ファイルを読む前に `Control::new(default_limits())` を作り、`ctrlc` crate で SIGINT のハンドラを入れる。読み込みとコンパイルの間の Ctrl-C も、既定の動作 (シグナルで殺される) でなく、このハンドラが受ける。ハンドラは実行の `Control` を持ち、次のように動く。

- `control.stop_bits() & Control::INTERRUPT` が 0 なら、`control.interrupt()` を呼ぶ。VM は次の安全点で止まる。コンパイルの間に立ったビットは、実行の最初の安全点で読まれる
- すでに立っていれば (2回目の Ctrl-C)、`std::process::exit(130)` でその場で終わる。まだ書いていない出力は捨てる。安全点に来ないプログラム (入力を待つ `Fs.read_all` など) を止めるための道だからである
- `ctrlc` はハンドラを専用のスレッドで順に呼ぶので、2つの Ctrl-C が同時に読み書きすることはない。`eml_rt` の `Control` は変えない

割り込みで止まったとき (`RuntimeError::interrupted`) は、1回目でも終了コードを 130 にする。130 は、シェルが SIGINT で終わった子に報告する値と同じである。ほかの実行時エラーは 1 である。
```

Replace the whole 「`RunStats`」 section (from `## `RunStats`` up to, not including, `## 並列化への備え`) with:

```markdown
## `RunStats`

`RunStats` は、実行の仕事の回数と、同時に生きていたヒープの物体の数と、スタックの語の数の最大である。検査モードだけで数え、速いモードの欄は 0 のままである。実行時エラーとリークのときは返さない。欄は次の10で、はじめの8つが仕事の回数である。

- `evidence_steps`: `ev_lookup` と `evv_mask` がたどった `evv` の節点の数。今有効な handler の数で決まり、呼び出しの深さにはよらない。検査モードの `evv_mask` は、合う evidence が足りずに誤りを返したときは数えない ([オブジェクトモデル](object-model.md) の「速いモードと検査モード」)。出どころは `Ctx` の `CtxStats` である
- `yields`: `yield_to` と `yield_abort` を実行した回数。すぐに再開する操作は yield しないので数えない。出どころは `CtxStats` である
- `yield_frames`: `yield_cont_set` を実行した回数。yield が handler まで戻る間に、継続 κ に足したフレームの数である。再開する操作では、操作と handler の間のフレームの数に比例する。これは方式の性質である。出どころは `CtxStats` である
- `string_bytes_copied`: extern がヒープの文字列の物体に写したバイト数 ([オブジェクトモデル](object-model.md) の「仕事の回数」)。`++` では、新しい文字列を作る側が |左辺| + |右辺|、一意な左辺に足す側が |右辺| になる。出力への書き込みと、コード生成が定数を作る書き込みは数えない。出どころは `CtxStats` である
- `rc_increments`: 参照の数を増やした回数。`dup`、共有の側の `release` が残すフィールド、基本操作が残す参照を数える。凍結した領域の物体とダミーは数えない ([オブジェクトモデル](object-model.md) の「検査モードでは数えない」)。出どころは `Heap` の `Stats` である
- `rc_decrements`: 参照の数を減らした回数。`decref`、`drop`、解放の連鎖で子を手放した分を数える。`release` と `free_block` は、持ち主を空けるときにも1つ足す。出どころは `Heap` の `Stats` である
- `boxes`: VM が実行した `box` (`BoxInt`、`BoxEnum`、`BoxConst`) の数。語の値の表現では確保しうる変換なので数える
- `unboxes`: VM が実行した `unbox` の数。数える理由は `boxes` と同じである
- `peak_objects`: 同時に生きていたヒープの物体の数の最大 (`Heap` の `Stats` の `peak_live`)。ヒープごとのダミーの物体を1つ含む。フレームはスタックにあり、凍結した領域の物体も数えないので、どちらも含まない
- `peak_stack`: タスクのスタックで同時に使った語の数の最大。スタックの `Vec` の長さは縮めないので、`Task::reserve` が伸ばした長さがそのまま最大になり、VM は実行の終わりに `task.stack.len()` を読むだけで済む。フレームはスタックにあって `peak_objects` に表れないので、末尾呼び出しを失ったことはこの欄で見つける

どの回数も時間ではなく、仕事か物体か語を数える。そのため、2乗の時間にならないことと、ループがフレームを積まないことを、ふだんのテストで確かめられる ([テスト戦略](../implementation/testing.md) の「性能のテスト」)。

S10a までの記録の `rc_increments`、`rc_decrements`、`peak_objects` は、S10b で消したインタプリタの数え方の値で、VM の値とは違う。違う理由は [計測の基準](../implementation/benchmarks.md) の「S10a (VM)」にある。
```

- [ ] **Step 7: (no step; 「並列化への備え」 is Task 12's)**

Step 5's grep already checked that Task 12 rewrote the owned-slot table paragraph.

- [ ] **Step 8: `object-model.md`**

Make these replacements (each old text occurs once):

| Where | Old | New |
|---|---|---|
| intro (line 5), last three sentences | `` `eml_rt` を使うのは [VM](vm.md) で、S10a の間は `eml run --vm` のときだけ動く。既定の `eml run` の CEK のヒープは [ランタイム](runtime.md) が定める。S10b で `eml run` を VM に切り替えるときに、2つの文書を合わせる。`` | `` `eml_rt` を使うのは [VM](vm.md) で、`eml run` は VM で動く。`` |
| 「ヘッダ」 (line 65), last sentence | `` ので、リークの報告は種類の名前で数える。今の `eml_runtime` が、どの型のデータも `Data` と数えるのと同じである `` | `` ので、リークの報告は種類の名前で数える。どの型のデータも `Data` と数える `` |
| 「文字列の操作」 (line 373) | `` - 容量を倍にするので、一意な左辺に足していくループは、ならすと右辺の長さに比例する時間で済む。今の `eml_runtime` と同じ性質である ([ランタイム](runtime.md) の「文字列の連結」) `` | `` - 容量を倍にするので、一意な左辺に足していくループは、ならすと右辺の長さに比例する時間で済む `` |
| 「検査モード」 (line 474) | `` 検査モードは、今の `eml_runtime` の検査付きヒープ ([ランタイム](runtime.md) の「`debug_heap`」) の役を引き継ぐ。解放した領域を `` | `` 検査モードは、`eml run --debug-heap` と UI テストが使う、検査付きのヒープである。解放した領域を `` |
| 「モードの選び方」 (line 482) | `` `RunStats` の各項目の VM での出どころと、CEK と値が違う理由は、[VM](vm.md) の「`RunStats`」にある。 `` | `` `RunStats` の各項目の意味と VM での出どころは、[VM](vm.md) の「`RunStats`」にある。 `` |
| 「不死の物体の数え方」 (line 506) | `` 今の `eml_runtime` の検査付きヒープは、不死の物体の外に出ている参照を数え、数が 0 の物体へのどの操作も解放の後の使用として断る ([ランタイム](runtime.md) の「不死の物体」)。そこでの不死の物体は子を持たない文字列のリテラルだけなので、この形で足りている。`eml_rt` の不死の物体は、`CodeBase` の定数のように子を持ちうる。親が子への参照を構造として持ち、親を作るときに `set_field` で子を書き、子を親から借りて読むので、同じ規則では正しいプログラムを断ってしまう。そこで、検査モードは次の形で数える。 `` | `` `eml_rt` の不死の物体は、`CodeBase` の定数のように子を持ちうる。親が子への参照を構造として持ち、親を作るときに `set_field` で子を書き、子を親から借りて読む。そのため、不死の物体の外に出ている参照を数え、数が 0 の物体へのどの操作も解放の後の使用として断る形では、正しいプログラムを断ってしまう。そこで、検査モードは次の形で数える。 `` |
| same section (last bullet) | `` - 今の検査より弱くなるのは、数が 0 の不死の物体を読むことを断らない点である。 `` | `` - この形で断らないのは、数が 0 の不死の物体を読むことである。 `` |
| 「実行の制御」 (line 629) | `` - `eml_rt` は止める語だけを持ち、SIGINT のハンドラを持たない。vstd だけに依存したままにするためである。ハンドラは S10b で `eml_cli` が入れ、`Control::interrupt()` を呼ぶ ([ロードマップ](../future/roadmap.md) の「S10 VM と切り替え」) `` | `` - `eml_rt` は止める語だけを持ち、SIGINT のハンドラを持たない。vstd だけに依存したままにするためである。ハンドラは `eml_cli` が `ctrlc` crate で入れ、`Control::interrupt()` を呼ぶ ([VM](vm.md) の「上限と割り込み」)`` + newline + `` - `eml run` の上限の既定の値は、`eml_cli::default_limits()` が決める ([VM](vm.md) の「上限と割り込み」)。上の表の既定の値 (`usize::MAX`) は `Limits::default()` のもので、`RunConfig::default()` とテストが使う `` |
| 「速いモードと検査モード」 (line 680), tail | `` たどった節点の数は、たどりが通ったときだけ `evidence_steps` に足し、`Err` のときは数えない。今のインタプリタは、途中までたどった数も数える `` | `` たどった節点の数は、たどりが通ったときだけ `evidence_steps` に足し、`Err` のときは数えない `` |
| same section (line 682) | `` - 仕事の回数の数え方は、`evv_mask` の `Err` を除いて、今のインタプリタと同じである。インタプリタの数え方のテスト3つを、同じ期待値で `eml_rt` に写してある `` | `` - 仕事の回数の数え方は、`heap/tests/ctx.rs` の3つのテストが固定する。S10b で消したインタプリタの数え方のテストを、同じ期待値で写したものである `` |
| 「語の ABI」 (line 705), tail | `` `eml_lir` の定数と同じであることは、両方に依存する `eml_interp` のテストで確かめる `` | `` `eml_lir` の定数と同じであることは、両方に依存する `eml_vm` のテスト (`tags.rs`) で確かめる `` |
| extern 「誤り」 (line 725) | `` - `Arith`、`Output`、`Io`、`NotUtf8` の表示は、今の `eml_interp::Fault` と1文字も違わない。これは verifier を通るプログラムでの約束である。`Heap(_)` の表示は `eml_rt::HeapError` のもので、今の `eml_runtime::HeapError` とは違う。今の `Fault::Internal` (文字列の操作が文字列でない値を受けたときなど) に対応するものもない。どちらも正しいプログラムでは起きないので、S10 で切り替えるときに UI テストの期待値の変更として扱う `` | `` - `Arith`、`Output`、`Io`、`NotUtf8` の表示は、[LIR](lir.md) の「実行時エラー」の文言と1文字も違わない。これは verifier を通るプログラムでの約束である。`Heap(_)` の表示は `eml_rt::HeapError` のものである `` |
| 「仕事の回数」 (line 832) | `` 検査モードの `CtxStats` の `string_bytes_copied` は、extern がヒープの文字列に写したバイト数である。今のインタプリタの `RunStats::string_bytes_copied` と同じ数え方で、S10 の `RunStats` はこの定義を引き継ぐ。 `` | `` 検査モードの `CtxStats` の `string_bytes_copied` は、extern がヒープの文字列に写したバイト数である。VM の `RunStats` の `string_bytes_copied` はこの数である ([VM](vm.md) の「`RunStats`」)。 `` |
| 「受け入れる制約」 (line 882), the reason cell | `` 速い道に検査を置かないため。`lin` の物体の rc は 1 なので、その `dup` は速い道を通る。検出は verifier と、検査モードで走らせる UI テスト (S10 から) に任せる。今の `eml_runtime` は、解放の後の使用を世代番号で、`File` の重複を `Heap::dup` で、`debug_heap` によらず常に検出しているので、S10 からはどちらも `--debug-heap` のときだけになる。[ランタイム](runtime.md) と [LIR とインタプリタ](lir.md) の「実行時エラー」の該当の文は、S10 で改める `` | `` 速い道に検査を置かないため。`lin` の物体の rc は 1 なので、その `dup` は速い道を通る。検出は verifier と、検査モードで走らせる UI テストに任せる。`eml run` では、どれも `--debug-heap` のときだけ見つかる ([LIR](lir.md) の「実行時エラー」) `` |

- [ ] **Step 9: Verify**

```bash
grep -nE 'eml_interp|eml_runtime|CEK|OutputSink|--vm|execute_with|VmError|runtime\.md|S10a の間|今のインタプリタ|今の `eml_runtime`' docs/spec/vm.md docs/spec/object-model.md
cargo test -p eml_cli --test integration citations
```

Expected: the grep prints nothing (the `[LIR とインタプリタ]` link text left in these two files is replaced in 14b); citations pass.

- [ ] **Step 10: Gate**

```bash
cargo test && cargo clippy --all-targets && cargo fmt --check
```

Expected: all pass, 0 warnings.

- [ ] **Step 11: Commit**

```bash
git add docs/spec/vm.md docs/spec/object-model.md
git commit -m "Write the VM-only run API, limits and SIGINT into vm.md and object-model.md" -m "vm.md: RunConfig and RuntimeError, the re-exports, the eml_vm inline rule, the Task::reserve cap, a new section on the default limits, SIGINT and exit code 130, and RunStats field meanings moved from runtime.md (with peak_stack). object-model.md: no more CEK, eml_runtime or runtime.md; the SIGINT handler and default_limits() live in eml_cli. No test changes." -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01MK52fNp5uA711Q2D4kWEtR"
```

### Task 14b: `lir.md` (title, 「実行の意味」, 「実行時エラー」), `core-ir.md`, `linearity.md`, and the link text `[LIR]`

**Files:**
- Modify: `docs/spec/lir.md` (title and intro; 「構造の規則」 layout-rule tail; 「値の表現」 last bullet and the literal rule; 「データの配置」; 「T3」; 「基本操作と実行の文脈」 incl. the CEK paragraph of 「実行の文脈」; 「節の呼び出し」; 「`evv` の不変条件」; section 「インタプリタ (CEK 機械)」 renamed and rewritten as 「実行の意味」; 「実行時エラー」 rewritten; 「実行時の規約」 deleted)
- Modify: `docs/spec/core-ir.md` (intro and bullets at lines 5-13; 「変換の規則」 line 168; 「パス」 line 350)
- Modify: every file under `docs/` (except `docs/superpowers/` and `docs/reports/`) that has the link text `[LIR とインタプリタ]` (one `sed`), and every citer of 「インタプリタ (CEK 機械)」 (`docs/spec/linearity.md`, `docs/spec/runtime.md`)
- Test: `crates/eml_cli/tests/citations.rs` (run)

**Interfaces:**
- Consumes: Task 12 (R7 deleted, R8/R9 renumbered to R7/R8, `saved` and the CEK-only Perceus branch gone from code and from the non-CEK text of `lir.md` and `core-ir.md`). 14a's `vm.md` 「上限と割り込み」.
- Produces: `lir.md` headings 「実行の意味」 (renamed) and 「実行時エラー」 (kept). The document title is `# LIR` and every link to it reads `[LIR]`.

- [ ] **Step 1: Check Task 12's part**

```bash
grep -rnw 'R9' docs/spec docs/implementation docs/future CLAUDE.md crates --include='*.md' --include='*.rs' | grep -v superpowers
grep -nE 'saved|save \[|save S|フレームは借りた値を持てない|after_call|ブロックに呼び出しがないこと' docs/spec/lir.md docs/spec/core-ir.md
```

Expected for the first grep: no output. If it prints anything, Task 12's renumbering is incomplete: stop and report to the coordinator (this sub-task does not renumber rules).

Expected for the second grep: no output. Task 12 removed `saved`, R7 and the CEK-only `dup` branch from `lir.md` (everything outside the CEK sections) and from `core-ir.md`, including the guard note in 「値の表現」 and the interpolation line in 「変換の規則」. If the second grep prints anything, Task 12 is incomplete: stop and report. This sub-task touches only the CEK-specific text Task 12 left: the title, 「インタプリタ (CEK 機械)」, 「実行時の規約」, the CEK paragraph of 「実行の文脈」, 「実行時エラー」, and other mentions of the interpreter, `eml_runtime` or `runtime.md`.

- [ ] **Step 2: Replace the link text everywhere**

```bash
grep -rl '\[LIR とインタプリタ\]' docs CLAUDE.md README.md | grep -vE '^docs/(superpowers|reports)/' | xargs sed -i 's/\[LIR とインタプリタ\]/[LIR]/g'
grep -rn 'LIR とインタプリタ' docs CLAUDE.md README.md | grep -vE '^docs/(superpowers|reports)/'
```

Expected: the second grep prints only `docs/spec/lir.md:1:# LIR とインタプリタ` (the title, changed in Step 3). Link text is not checked by the citations test, so this command is safe on its own; it also rewrites history rows in `roadmap.md` and `benchmarks.md`, as the spec asks ("すべて").

- [ ] **Step 3: Title and intro of `lir.md`**

Old (lines 1-5):

```
# LIR とインタプリタ

位置づけ: 規範。

EP の後の Core IR から写す LIR と、それを実行する CEK 風のインタプリタを定める。LIR は、box の挿入、縮約、Perceus が書き換える層で、後の VM (S10) とネイティブ化の入力である。2つの IR に共通する規則 (ブロックの列、構造の規則、値の表現、データの配置、位置の規則、末尾の位置の呼び出し、基本操作と実行時のモデル) はこの文書に1回だけ書き、[Core IR](core-ir.md) から引く。型から決める規則と translate の規則は [Core IR](core-ir.md) にある。ヒープと参照カウントは [ランタイム](runtime.md) で扱う。
```

New:

```
# LIR

位置づけ: 規範。

EP の後の Core IR から写す LIR と、その実行の意味を定める。LIR は、box の挿入、縮約、Perceus が書き換える層で、[VM](vm.md) と後のネイティブ化の入力である。2つの IR に共通する規則 (ブロックの列、構造の規則、値の表現、データの配置、位置の規則、末尾の位置の呼び出し、基本操作と実行時のモデル) はこの文書に1回だけ書き、[Core IR](core-ir.md) から引く。型から決める規則と translate の規則は [Core IR](core-ir.md) にある。物体の形、ヒープ、参照カウントは [オブジェクトモデル](object-model.md) で扱う。
```

- [ ] **Step 4: (no step; the `saved` bullets were Task 12's, checked in Step 1)**

- [ ] **Step 5: 「構造の規則」, 「値の表現」, 「データの配置」**

In the configuration rule about layouts (the last sub-bullet of the layout rule, whose number is R8 after Task 12), old tail:

```
タグかフィールドの数が違えば、インタプリタが内部の誤りとして止める。形が同じなら気付かない (下の「インタプリタ (CEK 機械)」)
```

New:

```
タグかフィールドの数が違えば、検査モードの VM が内部の誤りとして止める。形が同じなら気付かない ([VM](vm.md) の「switch」)
```

Last bullet of 「値の表現」. Old:

```
- `unr` でない変数の `dup` と `decref` は、V1 が静的に拒む (下の「verifier」)。`File` を含む `Lin` のデータとクロージャの `dup` は、実行時にはほかの物体と区別できないので、この静的な検査が覆う。`File` の物体そのものの参照を増やす違反は、ヒープも実行時に見つける ([ランタイム](runtime.md) の「`File` の参照の検査」)
```

New (two bullets; the second is the rule moved from `runtime.md` 「不死の物体」):

```
- `unr` でない変数の `dup` と `decref` は、V1 が静的に拒む (下の「verifier」)。`File` を含む `Lin` のデータとクロージャの `dup` は、`lin` ビットが立っていないとほかの物体と区別できないので、この静的な検査が覆う。`lin` ビットの立った物体 (`File` など) の参照を増やす違反は、検査モードの `eml_rt` も実行時に見つける ([オブジェクトモデル](object-model.md) の「`lin` ビット」)
- 文字列のリテラル (`const`) の値は、凍結した領域の不死の物体を指す ([オブジェクトモデル](object-model.md) の「凍結した領域」)。それでも LIR は、リテラルについても `dup` と `decref` の釣り合いを保つ。値がリテラルだからといって `dup` や `decref` を消すパスは作らない
```

Last bullet of 「データの配置」. Old:

```
- 後のバイトコード VM はジャンプ表の大きさとポインタのフィールドを、ネイティブ化は物体の記述子を、reuse は同じ配置かどうかを、この表から知る ([ランタイム](runtime.md) の「オブジェクトのヘッダ」)
```

New:

```
- VM はジャンプ表の大きさとポインタのフィールドを、後のネイティブ化は物体の形を、reuse は同じ配置かどうかを、この表から知る ([VM](vm.md) の「switch」、[オブジェクトモデル](object-model.md) の「物体の形」)
```

- [ ] **Step 6: 「T3」, 「基本操作と実行の文脈」, 「節の呼び出し」, 「`evv` の不変条件」**

| Old | New |
|---|---|
| `` がフレームを積む。インタプリタはフレームをヒープに置くのでメモリが O(n) になるだけだが、ネイティブではスタックがあふれる `` | `` がフレームを積む。VM ではスタックが反復の数に比例して伸び、深さの上限で止まる。ネイティブではスタックがあふれる `` |
| `` この節は、その基本操作と節の呼び出しの意味と、それが読み書きする実行時のモデルを定める。CEK、S9 の `eml_rt`、S10 の VM が実装する。 `` | `` この節は、その基本操作と節の呼び出しの意味と、それが読み書きする実行時のモデルを定める。`eml_rt` の `Ctx` と VM が実装する。 `` |
| `` インタプリタの機械は、次の2つを持つ。後の VM とネイティブでは、これを実行の単位ごとに持つ。 `` | `` 実行の文脈は、次の2つである。VM とネイティブでは、これを実行の単位ごとに持つ。 `` |
| `` - インタプリタでは `apply` と同じに動く。VM とネイティブでは、操作の ABI を知っている間接呼び出しになる `` | `` - `apply` と同じく関数の値を呼ぶ。VM とネイティブでは、操作の ABI を知っている間接呼び出しになる ([VM](vm.md) の「呼び出しと戻り」) `` |
| `` - 実行が正しく終わったとき、`evv` は空で、yield の状態はなしである。インタプリタはこれを `debug_heap` のリークの検査の前に確かめる `` | `` - 実行が正しく終わったとき、`evv` は空で、yield の状態はなしである。VM はこれをモードによらず、検査モードのリークの検査の前に確かめる ([VM](vm.md) の「誤り、止めること、終わり」) `` |

- [ ] **Step 7: Confirm the pass, liveness, Perceus and verifier text is Task 12's final text**

```bash
grep -nE 'saved|save \[|save S|退避|フレームは借りた値を持てない|after_call' docs/spec/lir.md
```

Expected: no output (Task 12 rewrote 「パスの順番」, 「生存解析」, 「Perceus」 and 「verifier」). If anything prints, stop and report; do not rewrite Task 12's sections here.

- [ ] **Step 8: Rewrite 「インタプリタ (CEK 機械)」 as 「実行の意味」, rewrite 「実行時エラー」, delete 「実行時の規約」**

Replace everything from the line `## インタプリタ (CEK 機械)` to the end of the file with:

````markdown
## 実行の意味

LIR を実行する [VM](vm.md) と後のネイティブのコードは、次の意味に従う。

- 実行は入口の関数から始める。始めは、空の `evv` と yield なしである (上の「実行の文脈」)
- `jump` は実引数をすべて読んでから、行き先の引数に書き、行き先の先頭へ進む。同じ関数の中で制御を移すだけで、フレームを積まない
- 末尾でない呼び出しは、戻ったらその文の `let` の変数に結果を入れ、次の文から続ける。末尾呼び出し (`tail`) は、呼び出し元のフレームを積まない (上の「末尾の位置の呼び出し」)
- 変数の消費は所有権の移動 (move) とし、値を複製するのは `dup` 命令だけにする。`switch` の scrutinee、`unpack` の値、`unbox` のオペランド、基本操作の借用の引数 (上の「基本操作」) は読むだけで、所有権を動かさない。Perceus の所有権の規則と実行の動作を1対1に対応させるためである
- `switch` は scrutinee を読むだけで、参照の数を変えない。case を選び、フィールドの値をフィールドの変数に書いてから、行き先の先頭へ進む。ヒープからは何も取り出さず、複製もしない。行き先は `switch` の前の所有をそのまま持って始まり、scrutinee を手放す `decref` か `release` と、フィールドの `dup` は、Perceus が所有の要る所に置く (上の「Perceus」)。scrutinee が即値のタグならその case に入る。物体なら、そのタグの case に入り、合う case がなければ `default` に進む。`Int` と `String` は case の値と比べる。`String` の scrutinee も `switch` では手放さず、行き先が手放す
- `unpack` は、1つの case の `switch` と同じく値を読むだけで、フィールドの値をフィールドの変数に書く
- `release x L #t(..)` は、x の参照を1つ手放し、名前を書いた位置のフィールドの参照を残す。x が一意なら箱を空け、残さないフィールドを手放す。そうでなければ、残すフィールドを `dup` してから x を手放す ([オブジェクトモデル](object-model.md) の「核の操作」)
- `prim` は、上の「基本操作」の表のとおりに文脈と evidence を読み書きする。借用の引数は呼ぶ側が持ったままなので手放さず、ほかの引数は所有した参照として受け取り、結果に残さないものを手放す。結果は所有した参照である。`evv_mask` が残すリストの残りと、`ev_renew` が新しい evidence に入れる入口の表は、基本操作が自分で `dup` する
- `clause` は `apply` と同じく関数の値を呼ぶ。操作の ABI は値の表し方を変えないためである (上の「節の呼び出し」)
- ダミーの値 (`prim dummy()`、`0`、`#0`、`()`) は、上の「ダミーの値」の約束を守る。`dup`、`decref`、`drop` は何も起こさず、`box` と `unbox` は誤りにしない
- `drop k` と `never` の操作による中断は、継続のクロージャが捕まえていた値を、解放の連鎖で1回ずつ解放する。`Lin` の値の破棄処理は物体の解放そのものなので、捕まっていた `File` は、この解放で読み出し口が捨てられて閉じる ([オブジェクトモデル](object-model.md) の「`File`」)
- `extern` 命令は、引数を受け取ってその場で実行し、値を返す。継続の最下部には、プログラムの終わりを表す枠があるだけで、`IO` の handler はない。`println` は Host に書く。`open` は `File` の物体を確保し、`read_all` は受け取った `File` の参照を、読んだ文字列との組に移して返す。`close` は `File` を解放する ([エフェクトと handler](effects.md) の「組み込みの `IO`」)
- 実行が正しく終わったとき、`evv` は空で、yield の状態はなしでなければならない (上の「`evv` の不変条件」)。そうでなければ内部の誤りである。残った evidence がリークに見えないように、この検査はモードによらず、検査モードのリークの検査の前に行う
- 再帰の深さは Rust のスタックで制限されない。VM はフレームをタスクのスタックに置き、深さの上限で止める ([VM](vm.md) の「スロットとスタック」)
- 名前付きのレコードとタプルは、コンストラクタが1つの `data` と同じ物体で、位置で分解する ([直積型とレコード](records.md))

## 実行時エラー

次の場合は実行を止めて実行時エラーにする。CLI の終了コードは 1 で、割り込みで止まったときだけ 130 である ([VM](vm.md) の「上限と割り込み」)。

- `Int` のオーバーフローとゼロ除算 ([宣言](declarations.md) の標準の演算子の表)
- `Fs.open` が開けないとき (``cannot open `<path>`: <reason>``)、`Fs.read_all` が読めないとき (``cannot read `<path>`: <reason>``)、読んだ内容が UTF-8 でないとき (``` `<path>` is not valid UTF-8 ```)。`reason` は `not found`、`permission denied`、`I/O error` のどれかである
- `println` が標準出力に書けないとき (``cannot write the output: <e>``)。`e` は書き込みの誤りの `io::Error` の文言である
- 実行を止めたとき。割り込み (`interrupted`)、確保量の上限 (`the memory limit is exceeded`)、深さの上限 (`the call depth limit is exceeded`) である ([オブジェクトモデル](object-model.md) の「実行の制御」)
- 検査モード (`eml run --debug-heap`) のヒープの誤り。解放済みの物体の使用 (`use of a freed object`)、`lin` ビットの立った物体の `dup` (`a Lin object is shared`)、終わりに残った物体 (リーク) などである ([オブジェクトモデル](object-model.md) の「報告する誤り」)。どれもコンパイラの誤りでしか起きない。速いモードはこれらを確かめない ([オブジェクトモデル](object-model.md) の「受け入れる制約」)
- 内部の誤り (`internal error: …`)。verifier を通った IR では起きないはずの形を、VM の見張りが見つけたときである ([VM](vm.md) の「誤り、止めること、終わり」)

extern の実装は、VM と後のネイティブのコードが使う `eml_rt` にある ([オブジェクトモデル](object-model.md) の「extern」)。誤りの文言は `eml_rt` のもので、上の一覧と1文字も違わない。

`eml_rt` の Host が出力をまとめて書くときは、書き込みの誤りが見つかるのは、後でバッファがあふれた `println` か、プログラムの終わりの flush である。そのため、`println` の誤りに付く位置は、書けなかった行を書いた `println` でなく、誤りを見つけた `println` の位置になる。終わりの flush の誤りは位置を持たない。

実行時エラーは、誤りの種類 (`fault`)、そのとき実行していた LIR の関数の名前 (`function`)、省略できる位置 (`at`) を持つ。位置を持つ `extern` 命令の実行が起こした誤り (その中のヒープの誤りを含む) にだけ、その命令の位置を付ける。位置は Core IR から写したもので ([Core IR](core-ir.md) の「位置」)、ファイルは `Program.files` の添字である。引数の読み出し、`dup`、`decref`、`release`、`switch`、`unpack`、`box`、`unbox` の誤りと、位置を持たない `extern` 命令の誤りには付けない。安全点で止めたときも位置を付けない。`Fs.read_all` の中で止めたときは、extern の誤りなので位置を付ける。

位置があるときは、次のように表示する。`path` は `Program.files` の表示用のパスである。

```
{fault}
  at {path}:{line}:{column}
```

位置がないとき (extern 命令の外で起きた誤り、位置のない手書きの IR) は ``{fault} in `{function}` `` と表示する。リークの表示は位置を持たない。
````

This drops 「実行時の規約」 (CEK's `Rc`/`RefCell` rule and `eml_runtime` objects) entirely, as the spec's first table says.

- [ ] **Step 9: Point the citers of the renamed heading at 「実行の意味」**

```bash
grep -rl 'インタプリタ (CEK 機械)' docs crates std CLAUDE.md | grep -vE '^docs/(superpowers|reports)/' | xargs sed -i 's/「インタプリタ (CEK 機械)」/「実行の意味」/g'
grep -rn 'インタプリタ (CEK 機械)' docs crates std CLAUDE.md | grep -vE '^docs/(superpowers|reports)/'
```

Expected: the second grep prints nothing. The first rewrites `docs/spec/linearity.md` line 28 and `docs/spec/runtime.md` line 10 (runtime.md is deleted in 14f; it must keep valid citations until then), and any comment in `crates/eml_interp` if that crate still existed (it does not after Task 10).

- [ ] **Step 10: `core-ir.md`**

| Old | New |
|---|---|
| `` と、LIR のパスとインタプリタは [LIR](lir.md) にある。 `` | `` と、LIR のパスと実行の意味は [LIR](lir.md) にある。 `` |
| `` - 実行系は、2つの層の IR と CEK 風のインタプリタで構成する。バックエンドは未定である ([ロードマップ](../future/roadmap.md)) `` | `` - 実行系は、2つの層の IR と、LIR を実行する [VM](vm.md) で構成する。ネイティブのバックエンドは未定である ([ロードマップ](../future/roadmap.md)) `` |
| `` インタプリタは LIR を実行し、S10 の VM とネイティブ化も LIR を入力にする `` | `` VM は LIR をバイトコードに変換して実行し、後のネイティブ化も LIR を入力にする `` |
| `` - ブロックの列にしたのは、将来インタプリタをバイトコード VM に組み替えることと、ネイティブ化 (Cranelift か LLVM) の土台にすることを見込んだためである。 `` | `` - ブロックの列にしたのは、バイトコード VM に写すことと、ネイティブ化 (Cranelift か LLVM) の土台にすることを見込んだためである。 `` |
| `` 2つ目からは、一意な左辺をその場で伸ばす ([ランタイム](runtime.md) の「文字列の連結」)。 `` | `` 2つ目からは、一意な左辺をその場で伸ばす ([オブジェクトモデル](object-model.md) の「文字列の操作」)。 `` |
| `` テキストの表示と読み込み、インタプリタ、`Drop`、 `` | `` テキストの表示と読み込み、VM のコード生成と実行、`Drop`、 `` |

- [ ] **Step 11: Verify**

```bash
grep -nE 'eml_interp|eml_runtime|CEK|OutputSink|インタプリタ|runtime\.md|saved|save \[' docs/spec/lir.md docs/spec/core-ir.md docs/spec/linearity.md docs/spec/effects.md
grep -c '^## 実行の意味$' docs/spec/lir.md
cargo test -p eml_cli --test integration citations
```

Expected: the first grep prints nothing; the count is `1`; citations pass.

- [ ] **Step 12: Gate**

```bash
cargo test && cargo clippy --all-targets && cargo fmt --check
```

- [ ] **Step 13: Commit**

```bash
git add docs
git commit -m "Rename lir.md to LIR and keep only the engine-independent execution rules" -m "lir.md: the title and every link text become LIR; the CEK section becomes 「実行の意味」 with only the rules every engine follows; 「実行時エラー」 lists stops, checked-mode heap errors and exit code 130; 「実行時の規約」 is gone; the literal RC rule moves in from runtime.md 「不死の物体」. core-ir.md and linearity.md follow. No test changes." -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01MK52fNp5uA711Q2D4kWEtR"
```


### Task 14c: `testing.md`

**Files:**
- Modify: `docs/implementation/testing.md` (「テストの置き場所」 row 3; 「crate の中の置き方」 `eml_test_support` paragraph; 「層ごとの方法」 table; 「`eml_rt` の検証」 intro and two unit-test bullets; 「期待値の表」; 「`eml_vm` のテスト」 (except the `codegen.rs` bullet, Task 4's); 「UI テスト」; heading 「VM でも流す」 renamed 「2つのモードで流す」 and rewritten; 「CLI のテスト」; 「性能のテスト」; 「よく使うコマンド」)
- Modify: every citer of 「VM でも流す」 (`docs/spec/vm.md`, `docs/future/roadmap.md`, `CLAUDE.md`, and code comments such as `crates/eml_cli/tests/ui.rs`)
- Test: `crates/eml_cli/tests/citations.rs` (run)

**Interfaces:**
- Consumes: Task 7 (`crates/eml_vm/tests/scaling.rs`, moved tests), Task 9 (the `#[ignore]` table writer in `eml_rt`), Task 10 (`eml_test_support::{run, run_files, run_stats, execute}` on the VM; UI harness in two modes), Task 11 (SIGINT tests in `cli.rs`, `default_limits()` test in `api.rs`), Task 4 (rewrote the `codegen.rs` bullet). Task 12 (removed `save` from the text-form sections).
- Produces: heading `testing.md` 「2つのモードで流す」.

- [ ] **Step 1: See what Tasks 7-12 already changed here**

```bash
grep -nE 'eml_interp|eml_runtime|CEK|OutputSink|--vm|execute_vm|run_vm|run_stats_vm|save|VM でも流す|65535' docs/implementation/testing.md
```

Every hit is replaced or checked by a step below. If a line was already rewritten by an earlier task, compare it with this step's text and keep the more complete one.

- [ ] **Step 2: 「テストの置き場所」 and 「crate の中の置き方」**

| Old | New |
|---|---|
| `` `eml_lir/tests/`、`eml_core_ir/tests/verify.rs`、`eml_interp/tests/`、`eml_vm/tests/`。 `` | `` `eml_lir/tests/`、`eml_core_ir/tests/verify.rs`、`eml_vm/tests/`。 `` |
| `` (`parse`、`def_map`、`lower`、`check`、`core_until`、`lir`、`lir_until`、`run`、`run_stats`、`execute`、`run_vm`、`run_stats_vm`、`execute_vm`) `` | `` (`parse`、`def_map`、`lower`、`check`、`core_until`、`lir`、`lir_until`、`run`、`run_stats`、`execute`) `` |

In the long `parse` paragraph, old:

```
`run` は `compile` の結果を `eml_cli::execute` に渡し、手で書いた LIR を受け取る `execute` と、`run_stats` も同じ関数を通す。`run_stats` は、出力と、`eml_interp::RunStats` か実行時エラーを返す。`run`、`run_files`、`execute` は `RunStats` を捨てる。`run_vm`、`run_stats_vm`、`execute_vm` は、同じ入力を VM で実行する。`run_vm` と `run_stats_vm` は検査モードで走らせ、`execute_vm` は手で書いた LIR とモードを受け取る。出力はメモリの Host (`eml_vm::capture_host`) で受ける。`RunConfig` と出力の受け口は、`eml_test_support` の1か所で組み立てる。
```

New:

```
`run` は `compile` の結果を `eml_cli::execute` で VM に渡す。手で書いた LIR と `RunConfig` を受け取る `execute` と、`run_stats` も同じ関数を通す。`run`、`run_files`、`run_stats` は検査モードで、上限のない新しい `Control` を持つ `RunConfig` で走らせる。上限の値に頼るテストは、`RunConfig` を自分で作って `execute` に渡す。`run_stats` と `execute` は、出力と、`RunStats` か実行時エラー (`RuntimeError`) を返す。`run` と `run_files` は `RunStats` を捨てる。出力はメモリの Host (`capture_host`) で受け、`file_root` は空のパス (カレントディレクトリ) である。出力の受け口は、`eml_test_support` の1か所で組み立てる。
```

- [ ] **Step 3: 「層ごとの方法」**

Old (three rows):

```
| ランタイム | ヒープの単体テスト (確保と解放、世代番号による解放済みアクセスの検出、リークの数え方、長い連鎖の解放、不死の物体の数え方と数が 0 のときの誤り、文字列のその場の連結、`release_fields` の一意と共有の側と形の誤り、参照の数の書き込みの数え方、`take_or_copy` がクロージャだけを扱うこと、evidence と `evv` の物体の解放と種類ごとの数え方、evidence をクロージャのように写さないこと) |
```

Delete that row. Old:

```
| RC | すべての実行テストで `debug_heap` を有効にする。違反があればテストを失敗させる |
| ランタイムの核 (`eml_rt`) | 下の「`eml_rt` の検証」 |
```

New:

```
| RC | すべての実行テストを検査モードで流す。違反があればテストを失敗させる |
| ランタイム (`eml_rt`) | 下の「`eml_rt` の検証」 |
| VM | 下の「`eml_vm` のテスト」 |
```

- [ ] **Step 4: 「`eml_rt` の検証」**

Old (intro, line 80):

```
`eml_rt` ([オブジェクトモデル](../spec/object-model.md)) は、Verus、単体テスト、期待値の表、Miri で確かめる。既定の `eml run` はまだ CEK なので、`eml_rt` を実際のプログラムで通すのは VM のテストである。ソースのプログラムを VM で流すのは、UI テスト (下の「VM でも流す」)、`crates/eml_vm/tests/bench.rs`、`crates/eml_cli/tests/cli.rs` の `eml run --vm` のテストである。メモリの安全と核の正しさは Verus の証明が受け持ち、単体テストは動作を、期待値の表は extern が今の実装と同じ結果になることを、Miri は Verus で確かめない部分 (信頼の一覧) を確かめる。
```

New:

```
`eml_rt` ([オブジェクトモデル](../spec/object-model.md)) は、Verus、単体テスト、期待値の表、Miri で確かめる。`eml run` は `eml_rt` の上の VM で動くので、ソースのプログラムで `eml_rt` を通すのは、UI テスト (下の「VM でも流す」)、`crates/eml_vm/tests/bench.rs` と `scaling.rs`、`crates/eml_cli/tests/cli.rs` の `eml run` のテストである。メモリの安全と核の正しさは Verus の証明が受け持ち、単体テストは動作を、期待値の表は extern の結果の回帰を、Miri は Verus で確かめない部分 (信頼の一覧) を確かめる。
```

In the 単体テスト list:

| Old | New |
|---|---|
| `` 入口の欄が 255 個以上の表、`eml_interp` から写した仕事の回数のテスト3つ ( `` | `` 入口の欄が 255 個以上の表、S10b で消したインタプリタから写した仕事の回数のテスト3つ ( `` |
| `` 検査モードの `Heap(_)` でヒープが変わらないことである。今のインタプリタは誤りの後に漏れを確かめないので、期待値の表では比べられないためである。 `` | `` 検査モードの `Heap(_)` でヒープが変わらないことである。期待値の表は結果だけを持つので、誤りのときの所有はここで確かめる。 `` |

- [ ] **Step 5: 「期待値の表」**

Replace the section body (from the paragraph after `### 期待値の表` up to, not including, `### Miri`) with the text below. Keep the 入力 sub-list exactly as it is now (marked here as unchanged lines).

```markdown
extern の結果を、1つの期待値の表で固定する。表は `eml_rt` 自身の結果から書く回帰の表である。S10b までは別の実装 (インタプリタ) の結果と照らしていたが、それを消したので、表の行を足すか入力を変えたときは、差分の結果の欄を人が確かめる。

- 表は `crates/eml_rt/src/heap/tests/expected.txt` で、1行が1つの場合である。タブで区切った extern の行の名前、引数、`=>` の後の期待する結果 (文字にした値か、位置を除いた誤りの文言) を持つ。今は 4025 の場合がある。4096 バイトを超える文字列の結果は、`h:<バイト長>:<FNV-1a 64 の16進>` と書く。表を小さく保つためである
- 表の形と入力は `crates/eml_rt/src/heap/tests/table.rs` に置く。`std` だけを使う
- 入力
  (the five unchanged sub-bullets: 整数, 文字列, `++` の左, ファイル)
- `eml_rt` のテスト `the_externs_match_the_table` (`heap/tests/externs.rs`) は、両方のモードで表のどの場合も同じ結果になり、引数の定数のほかに物体が残らないことを確かめる
- 表は `eml_rt` の `#[ignore]` のテストが、`eml_rt` 自身の結果で書く。入力を変えたときは、`cargo test -p eml_rt -- --ignored` で書き直し、`git diff --no-ext-diff crates/eml_rt/src/heap/tests/expected.txt` で変わった行を確かめる
```

Check that the command is right before committing:

```bash
grep -rn '#\[ignore' crates/eml_rt/src
```

Expected: exactly one hit, Task 9's table writer (so `-- --ignored` runs only it). If there are more, replace the command in the bullet with `cargo test -p eml_rt <that test's name> -- --ignored`.

If "今は 4025 の場合がある" no longer matches (`grep -vc '^#' crates/eml_rt/src/heap/tests/expected.txt` minus blank lines), write the current count.

- [ ] **Step 6: Rename 「VM でも流す」 and rewrite it; 「UI テスト」 bullet; all citers**

First rename every citer and the heading with one command:

```bash
grep -rl 'VM でも流す' docs crates std CLAUDE.md README.md | grep -vE '^docs/(superpowers|reports)/' | xargs sed -i 's/VM でも流す/2つのモードで流す/g'
grep -rn 'VM でも流す' docs crates std CLAUDE.md README.md | grep -vE '^docs/(superpowers|reports)/'
```

Expected: the second grep prints nothing. This rewrites the heading `### VM でも流す` in `testing.md`, the citations in `vm.md` (from 14a), `roadmap.md` (the S10a row), `CLAUDE.md`, `docs/README.md` (the table cell "UI テスト (VM でも流すこと)" becomes "UI テスト (2つのモードで流すこと)", which reads correctly), and comments in `crates/` (for example `crates/eml_cli/tests/ui.rs`).

Then replace the body of `### 2つのモードで流す` (its four bullets) with:

```markdown
- `run` と `run-fail` のテストは、まず VM の検査モードで、`Control::new(default_limits())` を持つ新しい `RunConfig` で実行する。標準出力と誤りの文言は、スナップショットと比べる。`file_root` はテストのディレクトリである
- その後で、新しい `Control` を持つ別の `RunConfig` で、速いモードでも実行する。標準出力と、終わり方 (成功か、誤りの文言) が検査モードと同じことを確かめる。速いモードを後に流すのは、速いモードは前提を確かめないので、VM の誤りがあると未定義の動作になり、1つのテストの失敗が glob のテスト全体を壊しうるためである。検査モードの誤り (リーク、解放の後の使用、`lin` の物体の `dup`) は、検査モードの実行が見つける
- 実行ごとに新しい `RunConfig` を作るのは、1つの `Control` を1回の実行に使うためである ([VM](../spec/vm.md) の「置き場所」)
- 出力は `capture_host` (メモリの書き込み先、そのテストのディレクトリの `file_root`、`Buffering::Block` の `StdHost`) で捕まえる。`run-fail/files` のテストは `file_root` に頼る
```

In 「UI テスト」, old:

```
- テストは `crates/eml_cli/tests/ui.rs` に置き、`insta::glob!` で `tests/ui/` 以下の `.em` を走査する。`eml_cli` の lib API をプロセス内で呼び、`debug_heap` を有効にした `RunConfig` で実行する
```

New:

```
- テストは `crates/eml_cli/tests/ui.rs` に置き、`insta::glob!` で `tests/ui/` 以下の `.em` を走査する。`eml_cli` の lib API をプロセス内で呼び、検査モードと速いモードの2つで実行する (下の「2つのモードで流す」)
```

- [ ] **Step 7: 「`eml_vm` のテスト」**

Old (intro):

```
`eml_vm` ([VM](../spec/vm.md)) の結合テストは、手で書いた LIR を `eml_lir::parse` で読んで確かめる。ソースのプログラムを VM で流すテストは、UI テスト (下の「2つのモードで流す」)、`bench.rs`、`crates/eml_cli/tests/cli.rs` の `eml run --vm` のテストである。
```

New:

```
`eml_vm` ([VM](../spec/vm.md)) の結合テストは、手で書いた LIR を `eml_lir::parse` で読んで確かめる。ソースのプログラムを VM で流すテストは、UI テスト (下の「2つのモードで流す」)、`bench.rs`、`scaling.rs`、`crates/eml_cli/tests/cli.rs` の `eml run` のテストである。
```

The `codegen.rs` bullet (the 65535 test and the new switch snapshots) is Task 4's. Check:

```bash
grep -n '65535' docs/implementation/testing.md
```

Expected: one hit, in Task 4's sentence about the unit test in `src/`. If the old sentence `番号 65535 のエフェクトの `ev_new` をコード生成が panic で断ることもここで確かめる` is still there, Task 4 is incomplete: stop and report.

In the 実行 bullet, old:

```
`run.rs` は定数、extern、直接の呼び出しと末尾呼び出し、誤りの位置と flush、終わりの処理とリークを、`calls.rs` は部分適用、ちょうどの適用、余った引数の `ApplyRest`、末尾の位置の適用、`clause`、自己末尾呼び出しの深さと割り込み、深さの上限を、
```

New:

```
`run.rs` は定数、extern、直接の呼び出しと末尾呼び出し、入口でないブロックの中の呼び出し、誤りの位置 (ヒープの誤りの位置と、位置のない誤りを含む) と flush、`File` の `dup`、終わりの処理とリーク (種類が2つ以上のリークの表示を含む) を、`calls.rs` は部分適用、ちょうどの適用、余った引数の `ApplyRest`、末尾の位置の適用、`clause`、自己末尾呼び出しの深さと割り込み、深さの上限、確保量の上限を、
```

Old:

```
- `api.rs` は、`CodeBase<M>: Sync` と `Task: Send` を型の assert で固定する ([マルチコア対応の設計](../future/multicore.md) の「VM の並列化の備え」の制約1と2)
- `bench.rs` は、基準のプログラムの VM の `RunStats` を固定する (下の「性能のテスト」)
```

New:

```
- `api.rs` は、`CodeBase<M>: Sync` と `Task: Send` を型の assert で固定する ([マルチコア対応の設計](../future/multicore.md) の「VM の並列化の備え」の制約1と2)。`RunConfig` の組み立て (`with_debug_heap`、`with_control`) も確かめる
- `bench.rs` は、基準のプログラムの VM の `RunStats` を固定する。`scaling.rs` は、仕事の回数と物体とスタックの語の数を上限と比べる (どちらも下の「性能のテスト」)
```

Old:

```
- `crates/eml_cli/tests/cli.rs` は、`eml run --vm` の成功と実行時エラーの終了コードを確かめる
```

New:

```
- `crates/eml_cli/tests/cli.rs` は、速いモードの `eml run` の成功と実行時エラーの終了コード、SIGINT で止めたときの終了コード 130 を確かめる (下の「CLI のテスト」)
```

- [ ] **Step 8: Confirm the text-form sections are Task 12's final text**

```bash
grep -nE 'save' docs/implementation/testing.md
```

Expected: no output (Task 12 removed `save` from 「crate の中の置き方」, 「LIR のテキストの形」 and 「Core IR のテキストの形」). If anything prints, stop and report.

- [ ] **Step 9: 「CLI のテスト」**

Old:

```
`crates/eml_cli/tests/cli.rs` は `eml` バイナリを起動し、終了コードが [コンパイラの構成](architecture.md) の「CLI と lib API」の定めに合うことを確認する。
```

New:

```
`crates/eml_cli/tests/cli.rs` は `eml` バイナリを起動し、終了コードが [コンパイラの構成](architecture.md) の「CLI と lib API」の定めに合うことを確認する。`run` のテストのうち、速いモード (`--debug-heap` なし) で走るのは、成功と実行時エラーの2本である。

SIGINT のテストは `cfg(unix)` で2本ある。どちらも `libc::kill` で SIGINT を送り、10 秒の締め切りを持ち、過ぎたら子を殺して失敗にする。テストが止まらずに CI を待たせないためである。

- 止まらない自己末尾呼び出しのループで行を出し続けるプログラムに、最初の行が届いたら SIGINT を1回送り、終了コード 130 と、標準エラーに `interrupted` が含まれることを確かめる。標準出力はパイプなので 64 KiB ごとにしか書かれない。テストは別のスレッドで標準出力を読み続ける。パイプが詰まると子が `write` の中で止まり、安全点に来なくなるためである
- `mkfifo` で作った名前付きのパイプを `Fs.read_all` で読むプログラムに、100 ms ごとに SIGINT を送り続け、終了コード 130 で終わることを確かめる。プログラムは読み込みの中で止まり、安全点に来ないので、2回目以降の SIGINT でハンドラが終わらせる。送り続けるのは、続けて届いた2つの SIGINT が1つにまとまることがあるためである

`default_limits()` のテストは `crates/eml_cli/tests/api.rs` にある。`max_depth` が 2^27 であることと、macOS と Linux では `max_bytes` が 0 より大きく `usize::MAX` より小さいことを確かめる。既定の上限まで実際に再帰するテストは、1 GiB を使うので置かない。
```

- [ ] **Step 10: 「性能のテスト」**

Replace the paragraph that begins `` `crates/eml_interp/tests/scaling.rs` は、時間ではなく、`` with:

```
`crates/eml_vm/tests/scaling.rs` は、時間ではなく、VM の仕事の回数と、同時に生きていたヒープの物体の数とスタックの語の数の最大 (`RunStats`) を上限と比べる。handler の下の非末尾の再帰 (handler が1つ、内側に別のエフェクトの handler が1つ、`mask` 付きのコールバックの中) は `evidence_steps` を、リテラルから始めて `acc ++ "x"` を n 回つなぐ連結は `string_bytes_copied` を、n = 2000 で n の定数倍の上限と比べる。2乗の実装では上限を超える。64 バイトのリテラルを n 回評価するテストは、`string_bytes_copied` が 64 未満であること (1回でも写せば超える) を確かめる。リストをたどるテストは `rc_increments` を確かめる。一意なリストを長さ 1000 と 2000 でたどったときに数が同じであること (セルごとの `dup` がない) と、共有されたリストを長さ n でたどったときに数が n + 2 以下であること (セルごとの `dup` は1回まで) である。ループの形ごとに、`peak_stack` と `peak_objects` を n = 1000 と n = 2000 で比べるテストもある。`Int`、`Bool`、`Unit` を返す関数の値を通るループ、呼び出しと結果の間に使われない `let` があるループ、節が末尾で再開する操作のループ、handler の下で yield しうる関数の値を通るループ、直接の自己末尾呼び出しは、2つの n で同じであることを確かめる。末尾呼び出しを失うと、スタックの語が反復の数に比例して増えるためである。導出した `==` と `compare` が要素が10000個のリストを末尾呼び出しで進むことも、`peak_stack` で確かめる。ラムダからラムダへの末尾の `apply` は、反復ごとにクロージャが1つ残るので、n = 2000 の `peak_objects` が n = 1000 の値 + 1000 以下であることと、`peak_stack` が伸びないことを確かめる。深さ n の非末尾の再帰の各段ですぐに再開する操作を行うテストは、`peak_stack` の伸びが n の差に比例し、1段あたりの語の数が一定であることを確かめる。保持されている持ち主を呼び出しをまたいで残さないこと ([LIR](../spec/lir.md) の「持ち主を手放す時点」) は、分解した値の1つのフィールドだけを使う末尾でない再帰で、n を 1000 から 2000 にしたときの `peak_objects` の伸びが n の差の小さい定数倍に収まることで確かめる (`a_retained_owner_does_not_cross_a_call`)。Prelude のリストには3つのテストがある。長さ 1000 と 2000 のリストの `show` の `string_bytes_copied` の比が 2.5 以下であること、要素が10000個のリストの導出した `==` と `compare` が通ること、要素が10000個のリテラルを持つプログラムが `run` で通ることである。evidence passing の性質を確かめるテストが2つある。末尾でない再帰の深さ 10 と 1000 の底で、すぐに再開する操作 (状態の `get` と `put`) を同じ回数行うと、`evidence_steps` が同じで、`yields` と `yield_frames` が 0 であること (費用が深さによらない)。同じ深さの底で再開する操作を1回行うと、`yield_frames` が深さに比例すること (方式の性質で、上限ではない)。ほかに、数え方そのものを確かめるテストがある (操作も文字列もなければ `evidence_steps`、`yields`、`yield_frames`、`string_bytes_copied`、`rc_increments`、`rc_decrements` は 0 で `peak_objects` はダミーの物体の 1、操作はどれも少なくとも1つの evidence を調べる、`"ab" ++ "cd"` は少なくとも4バイトを書く、型変数のフィールドに `Int` を入れて取り出すと `boxes` と `unboxes` が1以上になり、スカラーの位置だけのプログラムでは 0 になる)。コード生成とパスがブロックの列を再帰でたどらないことも、ここで確かめる (1000 の腕の `switch` の2つの形と、5000 個の `switch` のブロックと 5000 個の文の列)。各テストはソースをテストの中で作り、`eml_test_support::run_stats` で実行する。回数は機械の速さに左右されないので、`#[ignore]` を付けず、ふだんの `cargo test` で流す。
```

Before committing, check this paragraph against the file Task 7 wrote:

```bash
grep -n '^fn \|^#\[test\]' crates/eml_vm/tests/scaling.rs | grep -c '^.*fn '
grep -n 'fn ' crates/eml_vm/tests/scaling.rs
```

Every test named or described in the paragraph must exist, and every test in the file must be covered by the paragraph. If Task 7 asserted a different shape (for example `peak_objects` instead of `peak_stack` for one loop), describe what the test asserts.

Replace the paragraph that begins `` `crates/eml_interp/tests/bench.rs` は、基準のプログラム `` with:

```
`crates/eml_vm/tests/bench.rs` は、基準のプログラム (リポジトリの `bench/`) を VM の検査モードで走らせ、出力を確かめ、`RunStats` の全項目をインラインスナップショットで固定する。プログラムごとに1つのテストにして、並列に走らせる。`bench/` のファイルの一覧とテストのプログラムの一覧が一致することも確かめる。回数は決定的なので、段の前後の回数はこのスナップショットの差分で残る。回数を変える変更は、その段の spec に期待値の変更として書く。プログラムの中身と命令の数の測り方は [計測の基準](benchmarks.md) にある。
```

- [ ] **Step 11: 「よく使うコマンド」**

Old (three lines):

```
cargo test -p eml_interp --test integration scaling::                          # インタプリタの仕事の回数 (ふだんの cargo test にも入る)
cargo test -p eml_interp --test integration bench::                            # 基準のプログラムの回数 (ふだんの cargo test にも入る)
cargo test -p eml_vm --test integration bench::                                # 基準のプログラムの VM での回数 (同上)
```

New (two lines):

```
cargo test -p eml_vm --test integration scaling::                              # VM の仕事の回数とスタックの語の数 (ふだんの cargo test にも入る)
cargo test -p eml_vm --test integration bench::                                # 基準のプログラムの回数 (同上)
```

- [ ] **Step 12: Verify**

```bash
grep -nE 'eml_interp|eml_runtime|CEK|OutputSink|--vm|execute_vm|run_vm|run_stats_vm|save|VM でも流す|今のインタプリタ|debug_heap` を有効' docs/implementation/testing.md
cargo test -p eml_cli --test integration citations
```

Expected: the grep prints nothing; citations pass.

- [ ] **Step 13: Gate and commit**

```bash
cargo test && cargo clippy --all-targets && cargo fmt --check
git add -A docs crates CLAUDE.md
git commit -m "Describe the tests for the VM only in testing.md" -m "testing.md: eml_test_support's run functions are the VM's, the expected-value table is eml_rt's regression table, eml_vm's test files include scaling.rs and the memory-limit test, 「VM でも流す」 becomes 「2つのモードで流す」 (checked mode against the snapshots, then fast mode) with every citer renamed, and the CLI section covers exit code 130 and the SIGINT tests. No test changes." -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01MK52fNp5uA711Q2D4kWEtR"
```

(`git add -A crates` only picks up comment renames from Step 6's `sed`; check `git diff --cached --no-ext-diff --stat` lists only `docs/`, `CLAUDE.md`, and files whose only change is the heading name.)

### Task 14d: `architecture.md` and `status.md` (the end-of-stage documents)

**Files:**
- Modify: `docs/implementation/architecture.md` (「全体の方針」; 「プログラム全体の構成」 two rejected-form bullets; 「リポジトリ」 tree and the `eml_rt` bullet; 「crate の依存関係」 graph and bullets; the stage-entry table; heading 「`eml_core_ir`、`eml_lir`、`eml_runtime`、`eml_interp` の内部」 renamed 「`eml_core_ir`、`eml_lir`、`eml_vm` の内部」 with its CEK bullets replaced by VM bullets; 「継続と evidence」; 「CLI と lib API」; 「外部 crate」)
- Modify: `crates/eml_core_ir/src/translate/expr.rs:32`, `crates/eml_core_ir/tests/translate.rs:895` (they cite the renamed heading; one `sed`)
- Modify: `docs/implementation/status.md` (intro; 「深さと性能」 two bullets (the list-literal `save` bullet is Task 12's); 「verifier」 two bullets)
- Test: `crates/eml_cli/tests/citations.rs` (run)

**Interfaces:**
- Consumes: the final crate set after Task 10 (no `eml_interp`, no `eml_runtime`), Task 11's `ctrlc`/`libc` dependencies and `default_limits()`, 14a's `vm.md` headings.
- Produces: heading `architecture.md` 「`eml_core_ir`、`eml_lir`、`eml_vm` の内部」; 「CLI と lib API」 kept.

- [ ] **Step 1: Rename the internals heading and its citers**

```bash
grep -rl '「`eml_core_ir`、`eml_lir`、`eml_runtime`、`eml_interp` の内部」' docs crates std CLAUDE.md | grep -vE '^docs/(superpowers|reports)/' | xargs sed -i 's/`eml_core_ir`、`eml_lir`、`eml_runtime`、`eml_interp` の内部/`eml_core_ir`、`eml_lir`、`eml_vm` の内部/g'
sed -i 's/^## `eml_core_ir`、`eml_lir`、`eml_runtime`、`eml_interp` の内部$/## `eml_core_ir`、`eml_lir`、`eml_vm` の内部/' docs/implementation/architecture.md
grep -rn 'eml_runtime`、`eml_interp` の内部' docs crates std CLAUDE.md | grep -vE '^docs/(superpowers|reports)/'
```

Expected: the last grep prints nothing. The first command rewrites `crates/eml_core_ir/src/translate/expr.rs` and `crates/eml_core_ir/tests/translate.rs` (and `lir.md` no longer cites it after 14b).

- [ ] **Step 2: 「全体の方針」 and 「プログラム全体の構成」**

| Old | New |
|---|---|
| `` - 実行系は、2つの層の IR と CEK 風のインタプリタからなる。型付き Core IR (前向きの辺だけを持つ基本ブロックの列で、中間値に名前を付け、エフェクトを明示する) は意味の層で、EP の後に LIR (エフェクトの命令を持たず、box、RC、末尾呼び出しを明示する) に写す。インタプリタは LIR を実行し、後の VM とネイティブ化も Perceus の後の LIR を入力にする。バックエンドは未定である。ヒープと RC は `eml_runtime` に分離する `` | `` - 実行系は、2つの層の IR と、レジスタ型のバイトコードの VM からなる。型付き Core IR (前向きの辺だけを持つ基本ブロックの列で、中間値に名前を付け、エフェクトを明示する) は意味の層で、EP の後に LIR (エフェクトの命令を持たず、box、RC、末尾呼び出しを明示する) に写す。VM は Perceus の後の LIR をバイトコードに変換して実行し、後のネイティブ化も同じ LIR を入力にする。ネイティブのバックエンドは未定である。物体の形、ヒープ、RC は、VM とネイティブで共有するランタイム `eml_rt` に置く `` |
| `` - モジュールごとに Core IR を作ってリンクすること。インタプリタでは得るものがない `` | `` - モジュールごとに Core IR を作ってリンクすること。VM では得るものがない `` |
| `` - `Bool` のタグをインタプリタまで運ぶこと。 `` | `` - `Bool` のタグを VM まで運ぶこと。 `` |

- [ ] **Step 3: 「リポジトリ」**

In the tree, old lines:

```
    eml_runtime/
    eml_rt/             # VM とネイティブで共有するランタイムの核。vstd だけに依存し、Verus で検証する。パイプラインの crate からはまだ使わない (`eml_interp` の期待値の表のテストが dev-dependency で使う)
    eml_interp/
```

New lines:

```
    eml_rt/             # VM とネイティブで共有するランタイムの核。vstd だけに依存し、Verus で検証する
    eml_vm/             # LIR のレジスタ型のバイトコード VM。eml_lir、eml_rt、eml_extern だけに依存する
```

Old bullet:

```
- `eml_rt` は vstd に依存するので、`Cargo.lock` に vstd とその依存が入る。`eml_cli` は `eml_rt` に依存しないので `eml` バイナリには入らないが、`nix build` は lock の crate をすべて取る。`eml_rt` の `verus!` の中は verusfmt で、外は `cargo fmt` で整える
```

New:

```
- `eml_rt` は vstd に依存するので、`Cargo.lock` に vstd とその依存が入る。`eml_cli` は `eml_vm` を通して `eml_rt` に依存するので、vstd は `eml` バイナリにも入る。`eml_rt` の `verus!` の中は verusfmt で、外は `cargo fmt` で整える
```

- [ ] **Step 4: 「crate の依存関係」**

Replace the code block with:

```
eml_cli          check / run コマンド。パイプラインを組む唯一の場所 (Session)。テストから呼べる lib API を公開する
eml_vm           LIR → レジスタ型のバイトコード (CodeBase)。eml_rt の Ctx の上で実行する
eml_rt           VM とネイティブで共有するランタイムの核 (語、物体、確保器、RC、検査モード、Ctx、extern)。vstd だけに依存する
eml_core_ir      型付き HIR → Core IR (基本ブロックの列)。evidence passing のパスと、LIR への変換
eml_lir          LIR (box、RC、末尾呼び出しを明示する基本ブロックの列)。box/unbox の挿入、縮約、dup/decref/release の挿入のパス
eml_types        Kind・型・row の推論、線形性・多重度の検査、match の網羅性検査
eml_hir          モジュールの読み込み、CST → HIR の変換、名前解決、脱糖
eml_syntax       SyntaxKind、lexer、レイアウト段、イベント方式のパーサ、rowan、型付き AST ラッパ
eml_diagnostics  Diagnostic 型、FileId と SourceFiles、行と列、ariadne による表示
eml_extern       extern の表 (型、エフェクト、関数)。依存を持たず、HIR、型検査、Core IR、LIR、VM が引く
```

Bullets below the block:

| Old | New |
|---|---|
| `` - 段階の連なりに `eml_runtime` は入らず、`eml_interp` だけが依存する `` | `` - 段階の連なりに `eml_rt` は入らず、`eml_vm` だけが依存する。`eml_rt` は workspace のほかの crate に依存しない `` |
| `` `eml_core_ir` (LIR への変換とパスの順番のため)、`eml_interp`、`eml_cli` が依存する。`eml_interp` が読む IR は `eml_lir` だけなので、フロントエンドなしにビルドできる。後の VM も同じである。 `` | `` `eml_core_ir` (LIR への変換とパスの順番のため)、`eml_vm`、`eml_cli` が依存する。`eml_vm` が読む IR は `eml_lir` だけなので、フロントエンドなしにビルドできる。 `` |
| `` 今は `eml_core_ir`、`eml_lir`、`eml_runtime`、`eml_interp` が診断を出さず、 `` | `` 今は `eml_core_ir`、`eml_lir`、`eml_vm`、`eml_rt` が診断を出さず、 `` |
| `` `eml_hir`、`eml_types`、`eml_core_ir`、`eml_lir`、`eml_interp` が引く。 `` | `` `eml_hir`、`eml_types`、`eml_core_ir`、`eml_lir`、`eml_vm` が引く。 `` |
| `` `core` は `eml_core_ir` と `eml_lir`、`run` は `eml_interp` と `eml_runtime` を足す。 `` | `` `core` は `eml_core_ir` と `eml_lir`、`run` は `eml_vm` と、SIGINT のハンドラの `ctrlc` と、物理メモリを読む `libc` を足す。 `` |

(The fourth bullet's "残りの4つは依存しない" stays correct: `eml_lir`, `eml_vm`, `eml_rt`, `eml_extern`.)

Check the fourth row against Task 11's `crates/eml_cli/Cargo.toml` (`grep -n 'run =' crates/eml_cli/Cargo.toml`): the `run` feature must list `dep:eml_vm`, `dep:ctrlc`, `dep:libc`. If Task 11 made `libc` or `ctrlc` unconditional, write what the file says.

Stage-entry table, old row:

```
| `eml_interp` | `run(&eml_lir::Program, &RunConfig, &OutputSink) -> Result<RunStats, RuntimeError>` |
```

New row:

```
| `eml_vm` | `execute(&eml_lir::Program, &RunConfig, HostRef) -> Result<RunStats, RuntimeError>`。`compile::<M>(&eml_lir::Program, Arc<Control>) -> Result<CodeBase<M>, CompileError>` でバイトコードを作り、`Ctx<M>` の上で実行する |
```

- [ ] **Step 5: The internals section**

First bullet, old:

```
- Core IR の構成と translate、EP の規則は [Core IR](../spec/core-ir.md) が、2つの IR に共通する規則、LIR のパス、評価と所有権の意味、パスの境界の不変条件は [LIR](../spec/lir.md) が、ヒープと RC は [ランタイム](../spec/runtime.md) が定める。translate の組み立てと、継続と evidence の実行時の表し方は、下の「translate の組み立て」と「継続と evidence」に書く。値とフレームに `Rc` と `RefCell` を使わない規約は、[LIR](../spec/lir.md) の「実行時の規約」にある
```

New:

```
- Core IR の構成と translate、EP の規則は [Core IR](../spec/core-ir.md) が、2つの IR に共通する規則、LIR のパス、実行の意味、パスの境界の不変条件は [LIR](../spec/lir.md) が、バイトコードと VM は [VM](../spec/vm.md) が、物体の形とヒープと RC は [オブジェクトモデル](../spec/object-model.md) が定める。translate の組み立てと、継続と evidence の実行時の表し方は、下の「translate の組み立て」と「継続と evidence」に書く
```

| Old | New |
|---|---|
| `` (生存解析、Perceus、verifier、縮約、`pretty`、LIR への変換、インタプリタ) `` | `` (生存解析、Perceus、verifier、縮約、`pretty`、LIR への変換、VM のコード生成) `` |
| `` - パス、verifier、`pretty`、`parse`、インタプリタは、ブロックと文の並びをループでたどり、 `` | `` - パス、verifier、`pretty`、`parse`、VM のコード生成は、ブロックと文の並びをループでたどり、 `` |

Delete the five bullets from `- インタプリタの環境 (`Env`) のスロットは値だけを持ち、` through the bullet that begins `` - `RunStats` の `evidence_steps`、`yields`、`yield_frames` は `prims.rs` の基本操作が、 `` (the bullets about `Env`, `eml_interp` files, `Frame::Return`, the machine's positions, and `RunStats` counting), and put these four bullets in their place:

```
- `eml_vm` は、コード生成 (`compile/`) と実行に分かれる。`compile/` では、`slots.rs` が文の単位の生存から干渉グラフを作ってスロットを塗り、`moves.rs` がブロック引数への並列な代入を `Move` の列に直し、`emit.rs` が関数ごとに命令を出し、`constants.rs` が定数を作業用のヒープに作って凍結する ([VM](../spec/vm.md) の「スロットの割り当て」と「`CodeBase` と `FnEntry`」)。実行は `vm.rs` の、命令を `match` で分ける1つのループである。関数の値の適用は `apply.rs`、データの命令は `data.rs`、基本操作は `prims.rs` にあり、逆アセンブラは `disasm.rs` である
- VM は状態を `Ctx` と `Task` だけに置き、Rust の再帰を使わない。呼び出しのスタックは `Task` のデータである ([マルチコア対応の設計](../future/multicore.md) の「VM の並列化の備え」の制約5)
- 位置を持つ `extern` 命令から作った命令にだけ、`CodeBase::locations` で源の位置 (`SourceLocation`) を付ける ([LIR](../spec/lir.md) の「実行時エラー」)
- `RunStats` の回数は、`Ctx` の `CtxStats` とヒープの `Stats` が検査モードで数える。`boxes` と `unboxes` は VM が数え、`peak_stack` は実行の終わりのスタックの長さである ([VM](../spec/vm.md) の「`RunStats`」)
```

Check the file list against the tree before committing:

```bash
ls crates/eml_vm/src crates/eml_vm/src/compile
```

Expected: `apply.rs code.rs compile data.rs disasm.rs error.rs lib.rs op.rs prims.rs vm.rs` and `constants.rs emit.rs mod.rs moves.rs slots.rs`. If Tasks 1-4 renamed or added a file, name it in the bullet.

- [ ] **Step 6: 「継続と evidence」**

Replace the section body (all bullets under `### 継続と evidence`) with:

```
- 関数の呼び出しの継続は、VM のタスクのスタックの枠の列で表す。枠は見出しに戻り先の命令の位置、結果を書くスロット、呼び出し元の枠の位置と大きさを持つ ([VM](../spec/vm.md) の「フレーム」)。いちばん下の枠は入口の関数の結果を受け、その戻り先は `Finish` である
- エフェクトは EP が普通の呼び出しと基本操作に変えているので ([Core IR](../spec/core-ir.md) の「evidence passing」)、VM は handler を探さず、継続も切り出さない。エフェクトの継続 κ は EP が組み立てるクロージャで、`multi` の再開は共有されたクロージャへの `apply` として捕獲だけを写す。LIR は `handle`、`perform`、`resume`、`mask` を型に持たないので、VM に届くことはない
- 実行の文脈 (`evv` と yield の状態) は `eml_rt` の `Ctx` が持ち、基本操作は `Ctx` のメソッドである ([オブジェクトモデル](../spec/object-model.md) の「実行文脈 `Ctx`」)。借用の引数 (`PrimRow::borrowed`) は手放さず、ほかの引数は所有した参照として受け取り、結果に残さないものは手放す ([LIR](../spec/lir.md) の「基本操作」)
- `Call::Clause` は `Call::Apply` と同じく関数の値を適用する。ただし引数の数は、いつも入口の関数の引数の数と等しい ([VM](../spec/vm.md) の「呼び出しと戻り」)
- EP が足す変数の印は `evidence/` が決める。EP が作るクロージャ (`f$kN`、作り直す `compose$`、`yielding_entry` の `g`) は `captured_mark` で捕まえた値の印の結びにし、補助の関数 (`helpers.rs`) の κ、状態、値は `poly`、evidence は `unr` にする。遅い道と `prompt$` の経路の `drop` は、結果の Repr が RC の対象で印が `unr` でないときだけ出す ([Core IR](../spec/core-ir.md) の「EP の明示の `drop`」)
- `prim dummy()` は、ヒープごとに1つ確保する不死の物体 (値 0 の `BigInt`) を指す語である。`dup`、`decref`、`drop` は何もせず、`unbox` は 0 を読む ([オブジェクトモデル](../spec/object-model.md) の「ダミーの値」)
- 正しく終わった実行では、`Ctx::check_finished` で `evv` が空で yield の状態がなしであることを確かめてから、検査モードのリークを数える。残った evidence がリークに見えないように、この検査はモードによらず行う
- `drop k` と `never` の操作による中断は、継続のクロージャを解放する。物体は捕まえた値だけを所有するので、子をたどる解放が、捕まっていた値を1回ずつ解放する。捕まっていた `File` は `Foreign` の物体で、この解放で読み出し口が捨てられて閉じる ([オブジェクトモデル](../spec/object-model.md) の「`File`」)
- `extern` 命令は、フレームを積まずにその場で実行して値を返す。extern は eml のコードを呼び返さないので、継続を組み立てる必要がないためである
```

(The fifth bullet, on EP's marks, is the current text unchanged; the others replace CEK frames, `Value::Dummy`, `Runtime`, `prims.rs` and `eml_runtime`'s `Payload` with the VM's counterparts.)

- [ ] **Step 7: 「CLI と lib API」 and 「外部 crate」**

| Old | New |
|---|---|
| `` - `eml run --debug-heap` は、RC のリーク検出と解放済みアクセスの検出を有効にする `` | `` - `eml run` は VM の速いモードで実行し、`eml run --debug-heap` は検査モードで実行する。検査モードは、RC のリーク、解放済みの物体の使用、`lin` の物体の `dup` などを見つける ([オブジェクトモデル](../spec/object-model.md) の「報告する誤り」)`` + newline + `` - `eml run` は、ファイルを読む前に SIGINT のハンドラを入れ、`default_limits()` の上限で実行する ([VM](../spec/vm.md) の「上限と割り込み」) `` |
| `` - 終了コードは、0 = 成功、1 = 診断のエラーあり、または実行時エラー、2 = 使い方の誤り (引数の誤り、入口のファイルが読めない) とする。2 は、clap が引数の誤りで返す値に合わせた。 `` | `` - 終了コードは、0 = 成功、1 = 診断のエラーあり、または実行時エラー、2 = 使い方の誤り (引数の誤り、入口のファイルが読めない)、130 = 割り込みで止まった、とする。2 は、clap が引数の誤りで返す値に合わせた。130 は、シェルが SIGINT で終わった子に報告する値と同じである。 `` |
| `` salsa への載せ替えと、フロントエンドの並列のコンパイルに備えるためで、インタプリタとは関係ない。 `` | `` salsa への載せ替えと、フロントエンドの並列のコンパイルに備えるためで、実行とは関係ない。 `` |
| `` - `execute(&eml_lir::Program, &RunConfig, stdout: OutputSink) -> Result<RunStats, RuntimeError>` (feature `run`)。`RunStats` は実行の仕事の回数と、同時に生きていたヒープの物体の数の最大で、CLI は使わない。テストが2乗の時間にならないことと、ループがフレームを積まないことを確かめるのに使う `` | `` - `execute(&eml_lir::Program, &RunConfig, HostRef) -> Result<RunStats, RuntimeError>` (feature `run`)。VM で実行する。`RunStats` は実行の仕事の回数と、同時に生きていたヒープの物体の数とスタックの語の数の最大で、CLI は使わない。テストが2乗の時間にならないことと、ループがフレームを積まないことを確かめるのに使う`` + newline + `` - `default_limits() -> Limits` (feature `run`)。`eml run` の既定の上限を作る ([VM](../spec/vm.md) の「上限と割り込み」) `` |
| `` `RunConfig` と `RunStats` は `eml_interp` で定義し、`eml_cli` が再公開する。`RunConfig` は `#[non_exhaustive]` にしてあり、`RunConfig::default()` から作ってフィールドを代入する。`OutputSink` と `RunStats` の定めは [ランタイム](../spec/runtime.md) の「実行の API」にある。 `` | `` `RunConfig`、`RunStats`、`RuntimeError` などは `eml_vm` で定義し、`eml_cli` が再公開する ([VM](../spec/vm.md) の「置き場所」)。`RunConfig` は `#[non_exhaustive]` にしてあり、`RunConfig::default()` から `with_debug_heap` と `with_control` で作る。`RunStats` の定めは [VM](../spec/vm.md) の「`RunStats`」にある。 `` |

「外部 crate」: if `grep -n '`ctrlc`\|`libc`' docs/implementation/architecture.md` prints nothing, append two rows to the table, with the major.minor of the version that the workspace `Cargo.toml` (or `crates/eml_cli/Cargo.toml`) gives (`grep -nE '^(ctrlc|libc)' Cargo.toml crates/eml_cli/Cargo.toml`):

```
| `ctrlc` | <major.minor from Cargo.toml> | SIGINT のハンドラ (`eml run`) |
| `libc` | <major.minor from Cargo.toml> | 物理メモリの大きさ (`default_limits`) |
```

- [ ] **Step 8: `status.md`**

Old (line 5):

```
M1 (言語の全体を一通り通す vertical slice と、本番の構文の最初の部分) と M2 (モジュールと、参照ごとの具体化の表) は完了した。今は再設計 (S0〜S13) を進めていて、S8 の4つの作業をすべて終えた。S8a は LIR と、box の挿入、縮約、Perceus を LIR に移す作業、S8b は Perceus の所有の形、基本操作の引数の借用、`File` の実行時の検査、S8c は書き出す型の線形性、S8d は Kind の印、EP の明示の `drop`、V1 と V2 である ([ロードマップ](../future/roadmap.md) の「段の列」と「S8 LIR」)。Perceus は、`switch` と `unpack` のフィールドを借りたまま使い、所有が要る所でだけ所有を取る ([LIR](../spec/lir.md) の「Perceus」)。この文書は、今の言語の範囲と既知の制限をまとめる。実装が進んだら更新する。
```

New:

```
M1 (言語の全体を一通り通す vertical slice と、本番の構文の最初の部分) と M2 (モジュールと、参照ごとの具体化の表) は完了した。今は再設計 (S0〜S13) を進めていて、S10 (VM と切り替え) までを終えた ([ロードマップ](../future/roadmap.md) の「段の列」)。`eml run` は、Perceus の後の LIR をレジスタ型のバイトコードに変換し、`eml_rt` の上の VM で実行する ([VM](../spec/vm.md))。Perceus は、`switch` と `unpack` のフィールドを借りたまま使い、所有が要る所でだけ所有を取る ([LIR](../spec/lir.md) の「Perceus」)。この文書は、今の言語の範囲と既知の制限をまとめる。実装が進んだら更新する。
```

| Old | New |
|---|---|
| `` - すぐに再開する操作も、インタプリタでは CEK で handler を探していたときより多くの命令を使う。`bench/state.em` は、S6c の記録より実行の分が約70% 増えた ([計測の基準](benchmarks.md) の「S7 (evidence passing)」)。1回の操作で、 `` | `` - すぐに再開する操作は、1回の操作で `` |
| (same bullet, continuing) `` 1回の操作で、evidence を探す基本操作と入口を読む基本操作、入口の関数の呼び出し、状態の出し入れの基本操作、yield の確認を実行する。S8b で基本操作が evidence を借りるようにしたので、RC の増減は `ev_lookup` が作る参照と入口の終わりの `decref` の1回ずつになり、実行の分は S7 の記録より約11% 減った ([計測の基準](benchmarks.md) の「S8b (所有の形と基本操作の借用)」) `` | `` 1回の操作で、evidence を探す基本操作と入口を読む基本操作、入口の関数の呼び出し、状態の出し入れの基本操作、yield の確認を実行する。evidence passing に変えた S7 では、handler を探していた前の形より `bench/state.em` の実行の分が約70% 増えた ([計測の基準](benchmarks.md) の「S7 (evidence passing)」)。S8b で基本操作が evidence を借りるようにしたので、RC の増減は `ev_lookup` が作る参照と入口の終わりの `decref` の1回ずつになり、実行の分は S7 の記録より約11% 減った ([計測の基準](benchmarks.md) の「S8b (所有の形と基本操作の借用)」) `` |
| `` 全体で2乗の時間がかかる ([ランタイム](../spec/runtime.md) の「文字列の連結」) `` | `` 全体で2乗の時間がかかる ([オブジェクトモデル](../spec/object-model.md) の「文字列の操作」) `` |
| `` タグかフィールドの数が違えば、実行したときにインタプリタが内部の誤りで止まり、形が同じなら気付かない。`tobj` に入ったヒープの物体を `unbox` する IR も verifier を通り、インタプリタが内部の誤りで止まる。 `` | `` タグかフィールドの数が違えば、検査モードの VM が内部の誤りで止まり、形が同じなら気付かない。`tobj` に入ったヒープの物体を `unbox` する IR も verifier を通り、検査モードの VM がヒープの誤り (`WrongKind`) で止まる。 `` |
| `` `File` の物体の参照を増やす違反は、ヒープも実行時に `a Lin object is shared` で止める ([ランタイム](../spec/runtime.md) の「`File` の参照の検査」)。 `` | `` `lin` ビットの立った物体 (`File` など) の参照を増やす違反は、検査モードの `eml_rt` も実行時に `a Lin object is shared` で止める ([オブジェクトモデル](../spec/object-model.md) の「`lin` ビット」)。 `` |
| `` 配置の表がフィールドの Kind を持たないので verifier が確かめず、translate を信用する。S9 のヘッダの `lin` ビットで実行時に覆う `` | `` 配置の表がフィールドの Kind を持たないので verifier が確かめず、translate を信用する。検査モードの `lin` ビットの誤りが実行時に覆う `` |

(The two "same bullet" rows are one Edit on the S7 bullet; they are split here only to show the old text in full. Apply them as a single replacement of that bullet.)

- [ ] **Step 9: Verify**

```bash
grep -nE 'eml_interp|eml_runtime|CEK|OutputSink|--vm|execute_vm|saved|save \[|runtime\.md|インタプリタ|Value::|Frame::' docs/implementation/architecture.md docs/implementation/status.md
cargo test -p eml_cli --test integration citations
```

Expected: no grep output; citations pass.

- [ ] **Step 10: Gate and commit**

```bash
cargo test && cargo clippy --all-targets && cargo fmt --check
git add docs/implementation/architecture.md docs/implementation/status.md crates/eml_core_ir/src/translate/expr.rs crates/eml_core_ir/tests/translate.rs
git commit -m "Bring architecture.md and status.md to the VM at the end of S10" -m "architecture.md: the crate list, tree, feature notes and stage table name eml_vm and eml_rt instead of eml_interp and eml_runtime; the internals section is renamed for eml_vm (two comments citing it follow) and describes the VM's files; continuations, evidence and the dummy are the VM's; the CLI section has exit code 130, SIGINT and default_limits(). status.md: the intro reflects S10, and the CEK and runtime.md statements are gone. No test changes." -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01MK52fNp5uA711Q2D4kWEtR"
```

### Task 14e: `roadmap.md`, `multicore.md`, `overview.md`, `README.md`, `docs/README.md`, `CLAUDE.md`

**Files:**
- Modify: `docs/future/roadmap.md` (history links to `runtime.md`; 「順序の理由」 bullet on CEK; S9 intro sentence; S10b row; S10 「決めたこと」; S11 「決めたこと」 bullet)
- Modify: `docs/implementation/benchmarks.md` (one history link to `runtime.md`)
- Modify: `docs/future/multicore.md` (lines 35, 197, 262)
- Modify: `docs/overview.md` (lines 73, 91)
- Modify: `README.md` (features bullet, exit codes, crate table)
- Modify: `docs/README.md` (spec table rows for `lir.md`, `runtime.md`, `vm.md`; stage table)
- Modify: `CLAUDE.md` (Commands, Architecture crate list and paragraphs, Testing bullets; English)
- Test: `crates/eml_cli/tests/citations.rs` (run)

**Interfaces:**
- Consumes: headings from 14a-14d and Task 13 (`vm.md` 「置き場所」, 「スロットとスタック」, 「上限と割り込み」, 「`RunStats`」, 「並列化への備え」; `object-model.md` 「実行の制御」; `lir.md` 「値の表現」, 「所有を取る時点」, 「持ち主を手放す時点」, 「実行の意味」, 「実行時エラー」; `testing.md` 「期待値の表」, 「2つのモードで流す」, 「性能のテスト」; `benchmarks.md` 「S10b …」).
- Produces: nothing later sub-tasks cite, except that after this sub-task no live document links to `docs/spec/runtime.md` (14f deletes it).

- [ ] **Step 1: Find the exact S10b heading of `benchmarks.md`**

```bash
grep -n '^### S10b' docs/implementation/benchmarks.md
```

Expected: one line. Below, `「S10b (VM への切り替え)」` stands for that heading's text; if the line prints a different name, use the printed text in Step 3 (the citations test fails otherwise).

- [ ] **Step 2: Unlink `runtime.md` in history**

```bash
sed -i 's|\[ランタイム\](\.\./spec/runtime\.md)|`runtime.md`|g' docs/future/roadmap.md docs/implementation/benchmarks.md
grep -n 'runtime\.md' docs/future/roadmap.md docs/implementation/benchmarks.md
```

Expected: the hits are now plain `` `runtime.md` `` in roadmap lines 62, 64, 70 (S8 rows and the S8 note), roadmap line 186 (rewritten in Step 3), and benchmarks line 217 (S7 record). The spec's rule: history keeps the file name as text because it is a record of that time.

- [ ] **Step 3: `roadmap.md`**

| Old | New |
|---|---|
| `` - CEK の確保を除くことと、CEK の上に REPL を作ることは行わない。どちらも、S10 で消す CEK への投資になるためである。 `` | `` - CEK の確保を除くことと、CEK の上に REPL を作ることは行わなかった。どちらも、S10 で消す CEK への投資になるためである。 `` |
| `` CEK と `eml_runtime` は S10 まで変えない。S10 で消す CEK に投資しないためである (上の「段の列」)。 `` | `` CEK と `eml_runtime` は S10 まで変えなかった。S10 で消す CEK に投資しないためである (上の「段の列」)。 `` |
| `` | S10b | `eml run` を VM に切り替え、SIGINT のハンドラと上限を入れ、CEK、`eml_runtime`、`saved` を消し、spec を書き直す | まだ | `` | `` | S10b | `eml run` を VM に切り替え、SIGINT のハンドラと上限を入れ、CEK、`eml_runtime`、`saved` を消し、spec を書き直す | 終えた。決めたことは [VM](../spec/vm.md) の「置き場所」、「スロットとスタック」、「上限と割り込み」、「`RunStats`」、「並列化への備え」と、[オブジェクトモデル](../spec/object-model.md) の「実行の制御」、[LIR](../spec/lir.md) の「値の表現」、「所有を取る時点」、「持ち主を手放す時点」、「実行の意味」、「実行時エラー」、[テスト戦略](../implementation/testing.md) の「期待値の表」、「2つのモードで流す」、「性能のテスト」、「CLI のテスト」に移した。`runtime.md` は消した。<UI sentence> `bench/` の数は [計測の基準](../implementation/benchmarks.md) の「S10b (VM への切り替え)」にある | `` |

`<UI sentence>` is decided by this command (`7d1bf441` is the commit that added the S10b design, the base of the stage):

```bash
git diff --no-ext-diff --stat 7d1bf441..HEAD -- crates/eml_cli/tests/snapshots
```

If it prints nothing, write `UI テストの出力は変わらなかった。`. Otherwise write `UI テストの出力で変わったのは、<the snapshot files' test names, comma-separated> の誤りの文言だけである。` and check in the diff that only `Heap(_)` or internal-error texts changed (the spec's expected-value scope); anything else is a defect to report.

Replace the S10 「決めたこと」 body (from `S10a で決めたことは spec に移した。残りは S10b で行うことである。` through the bullet `- spec を改める。…段の終わりに `status.md` と `architecture.md` を直す`) with:

```markdown
S10a と S10b で決めたことは spec に移した。S10b では、この節で決めていた次の4つを変えた。

- 上限の既定の値と、`RunConfig` での指定を決める、としていた。上限は `Control` が持ち、`RunConfig` は `Control` を持つ形にした。CLI が SIGINT のハンドラと実行に同じ `Control` を渡すためである ([VM](../spec/vm.md) の「置き場所」)
- `crates/eml_interp/tests/bench.rs` のスナップショットを VM のものに替えることを、期待値の変更としていた。VM のスナップショットは S10a で `crates/eml_vm/tests/bench.rs` にあったので、CEK の `bench.rs` は消した (成否の変更)
- `saved` と、Perceus でそれを埋める処理を消す、としていた。`saved` に結びついた規則 R7 も消し、Perceus の「所有を取る時点」の 2 のうち、呼び出しの後で生きる借りた変数を `dup` する枝も消した。どちらも、フレームが借りた値を持てない CEK のための規則だったためである。保持されている持ち主を呼び出しの前に解く枝は、空間のために残した ([LIR](../spec/lir.md) の「持ち主を手放す時点」)
- 期待値の表を `eml_rt` と CEK の両方と照らす形にしていた。CEK を消したので、表は `eml_rt` の回帰の表にした ([テスト戦略](../implementation/testing.md) の「期待値の表」)
```

In S11 「決めたこと」:

| Old | New |
|---|---|
| `` (調査の実験では、CEK で実行時エラーの後に局所変数が残った) `` | `` (調査の実験では、S10b で消したインタプリタで、実行時エラーの後に局所変数が残った) `` |

The guard item in 「言語機能と構文」, the borrow-parameter sentence that cited R7, and the demotion reason in 「処理系」 are Task 12's. Check:

```bash
grep -nE 'ブロックに呼び出しがないこと|呼び出しの前に所有になる|フレームが借りた値と所有した値を区別' docs/future/roadmap.md
```

Expected: no output. If anything prints, Task 12 is incomplete: stop and report.

- [ ] **Step 4: `multicore.md`**

| Old | New |
|---|---|
| `` 別のスレッドからの解放に対応したアロケータは、VM の並列化かネイティブのランタイムのうち先に来るほうで決める。今の `eml_runtime` の検査付きヒープの役は、`eml_rt` の検査モードが引き継ぐ | `` | `` 別のスレッドからの解放に対応したアロケータは、VM の並列化かネイティブのランタイムのうち先に来るほうで決める。実行時の検査は、`eml_rt` の検査モードが受け持つ ([オブジェクトモデル](../spec/object-model.md) の「検査モード」) | `` |
| `` 今のインタプリタでは、破棄処理はオブジェクトの解放そのものである ([ランタイム](../spec/runtime.md) の「オブジェクトのヘッダ」) `` | `` 今の `eml_rt` は型ごとの記述子の表を持たず、破棄処理をヘッダの種類で表す ([オブジェクトモデル](../spec/object-model.md) の「ヘッダ」) `` |
| `` 共有の印方式の RC (上の「スレッドをまたぐ RC」) は、S9 で `eml_rt` に入れ、両方で使う。`eml_rt` の検査モードが、今の検査付きヒープ ([ランタイム](../spec/runtime.md)) の役を引き継ぐ `` | `` 共有の印方式の RC (上の「スレッドをまたぐ RC」) は、S9 で `eml_rt` に入れ、両方で使う。実行時の検査は、`eml_rt` の検査モードが受け持つ `` |

- [ ] **Step 5: `overview.md`**

Old (line 73, from the second sentence to the end):

```
実行系は、型付き Core IR (前向きの辺だけを持つ基本ブロックの列で、エフェクトを明示する) を EP の後に LIR (RC と box を明示し、エフェクトの命令を持たない) に写し、LIR を CEK 風のインタプリタで実行する。メモリは Perceus 方式の参照カウントで管理し、`Lin` の値を2回消費することはない。インタプリタはシングルスレッドである。S10 でインタプリタを置き換える VM もシングルスレッドで出し、後から並列にできる形で作る。並列化は、VM とネイティブのランタイムの両方に入れる。詳細は [コンパイラの構成](implementation/architecture.md)、[Core IR](spec/core-ir.md)、[LIR](spec/lir.md)、[ランタイム](spec/runtime.md)、[マルチコア対応の設計](future/multicore.md) にある。
```

New:

```
実行系は、型付き Core IR (前向きの辺だけを持つ基本ブロックの列で、エフェクトを明示する) を EP の後に LIR (RC と box を明示し、エフェクトの命令を持たない) に写し、LIR をレジスタ型のバイトコードの VM で実行する。メモリは Perceus 方式の参照カウントで管理し、`Lin` の値を2回消費することはない。VM はシングルスレッドで出し、後から並列にできる形で作ってある。並列化は、VM とネイティブのランタイムの両方に入れる。詳細は [コンパイラの構成](implementation/architecture.md)、[Core IR](spec/core-ir.md)、[LIR](spec/lir.md)、[VM](spec/vm.md)、[オブジェクトモデル](spec/object-model.md)、[マルチコア対応の設計](future/multicore.md) にある。
```

| Old | New |
|---|---|
| `` エフェクトの命令を持たず、box、RC の命令、末尾呼び出しを明示する。インタプリタ、後の VM とネイティブ化の入力である | `` | `` エフェクトの命令を持たず、box、RC の命令、末尾呼び出しを明示する。VM と後のネイティブ化の入力である | `` |

- [ ] **Step 6: `README.md` and `docs/README.md`**

`README.md`:

| Old | New |
|---|---|
| `- 型付きの Core IR を CEK 機械で実行する。メモリは Perceus 方式の参照カウントで管理する。` | `- 型付きの Core IR を LIR に写し、レジスタ型のバイトコードの VM で実行する。メモリは Perceus 方式の参照カウントで管理する。` |
| `` `eml` の終了コードは、成功で 0、診断または実行時エラーで 1、使い方の誤りで 2 である。 `` | `` `eml` の終了コードは、成功で 0、診断または実行時エラーで 1、使い方の誤りで 2、Ctrl-C で止めたときは 130 である。 `` |

Replace the crate table rows (between the header `| crate | 役割 |` / `|---|---|` and the blank line) with:

```
| `eml_cli` | `check` / `run` のコマンド。各段階をつなぐ |
| `eml_vm` | LIR をレジスタ型のバイトコードに変換し、`eml_rt` の上で実行する |
| `eml_rt` | VM とネイティブで共有するランタイムの核 (物体、ヒープ、参照カウント、検査モード、extern)。Verus で検証する |
| `eml_core_ir` | 型付き HIR から Core IR (基本ブロックの列) への変換、evidence passing、LIR への変換 |
| `eml_lir` | LIR と、`box` / `unbox` の挿入、縮約、`dup` / `decref` / `release` の挿入 |
| `eml_types` | Kind、型、row の推論と、線形性、多重度、網羅性の検査 |
| `eml_hir` | CST から HIR への変換と名前解決 |
| `eml_syntax` | lexer、レイアウト段、パーサ、rowan の CST |
| `eml_diagnostics` | 診断のデータ構造と表示 |
| `eml_extern` | 標準ライブラリの extern の表 |
| `eml_test_support` | 統合テスト用の補助 (開発時のみ) |
```

`docs/README.md`:

| Old | New |
|---|---|
| `` LIR のパス (box の挿入、縮約、Perceus) とその境界の不変条件、LIR の verifier、インタプリタ、実行時エラー | `` | `` LIR のパス (box の挿入、縮約、Perceus) とその境界の不変条件、LIR の verifier、実行の意味、実行時エラー | `` |
| the whole row `` | [spec/runtime.md](spec/runtime.md) | 規範 | ヒープと参照カウント、オブジェクトのヘッダ、不死の物体、`eml_runtime` の API、evidence と `evv` の物体、文字列の連結、`debug_heap`、実行の API | `` | (delete the line) |
| `` | [spec/vm.md](spec/vm.md) | 規範 | VM の置き場所、`CodeBase` と `FnEntry`、 `` | `` | [spec/vm.md](spec/vm.md) | 規範 | VM の置き場所と実行の API、`CodeBase` と `FnEntry`、 `` |
| (same row, later) `` 誤りと終わりの処理、`RunStats`、並列化への備え | `` | `` 誤りと終わりの処理、上限と割り込み、`RunStats`、並列化への備え | `` |
| `` | `eml_lir` / `eml_interp` | lir | `` | `` | `eml_lir` | lir | `` |
| `` | `eml_runtime` | runtime | `` | `` | `eml_vm` | vm、lir、object-model | `` |

- [ ] **Step 7: `CLAUDE.md` (English)**

Task 12 owns the rule numbers, `saved` / `save` and the Perceus sentence of the LIR paragraph; check with `grep -nE 'saved|save|R1-R9' CLAUDE.md` (expected: no output, else stop and report). Other sentences may already have been touched by Tasks 9 and 10 (the expected-value table, `run_vm`). For each row: if the old text is present, replace it; if an earlier task already rewrote that sentence, make sure the result says what the new text says and keep the earlier task's wording when it is equivalent.

| Section | Old | New |
|---|---|---|
| Commands | the line `` cargo run -p eml_cli -- run --vm <file.em>  # run on the VM (hidden flag, S10a only; fast mode, --debug-heap for checked) `` | (delete the line) |
| Commands | `` cargo run -p eml_cli -- check <file.em>    # or: run [--debug-heap] <file.em> `` | `` cargo run -p eml_cli -- check <file.em>    # or: run [--debug-heap] <file.em> (the VM; --debug-heap is checked mode) `` |
| Architecture intro | `` (currently all but `eml_extern`, `eml_core_ir`, `eml_lir`, `eml_runtime`, `eml_interp`, and `eml_vm`; `` | `` (currently all but `eml_extern`, `eml_core_ir`, `eml_lir`, `eml_vm`, and `eml_rt`; `` |
| crate block | `` eml_vm           LIR -> `CodeBase` (register bytecode) run on `eml_rt`'s `Ctx` (reads `eml_lir`, `eml_rt`, `eml_extern`); S10a: only `eml run --vm` `` | `` eml_vm           LIR -> `CodeBase` (register bytecode) run on `eml_rt`'s `Ctx` (reads `eml_lir`, `eml_rt`, `eml_extern`); `eml run` executes here `` |
| crate block | the two lines `` eml_interp       runs LIR on a CEK machine (reads only `eml_lir`, no front end) `` and `` eml_runtime      object model, heap, RC, debug_heap checks, OutputSink `` | the one line `` eml_rt           runtime core shared by the VM and native code (words, objects, allocator, RC, checked mode, `Ctx`, externs); depends only on vstd, verified with Verus `` |
| crate block | `` consulted by HIR, types, Core IR, LIR, the CEK and the VM `` | `` consulted by HIR, types, Core IR, LIR and the VM `` |
| `eml_rt` paragraph | `` `eml_rt` (`docs/spec/object-model.md`) is used only by the VM (`eml_vm`, `docs/spec/vm.md`) until S10b switches `eml run` to it: `` | `` `eml_rt` (`docs/spec/object-model.md`) is the runtime under the VM (`eml_vm`, `docs/spec/vm.md`): `` |
| same | `` (the Arith / Output / Io / NotUtf8 texts byte-identical to `eml_interp`'s) `` | `` (the Arith / Output / Io / NotUtf8 texts are the runtime error texts of `docs/spec/lir.md`) `` |
| same | `` with every heap through an `Arc`, and `eml_rt` installs no signal handler. `` | `` with every heap through an `Arc`, and `eml_rt` installs no signal handler (`eml_cli` installs the SIGINT handler and builds `eml run`'s default limits with `default_limits()`). `` |
| same | `` one expected-value table (`heap/tests/expected.txt`) is checked against both `eml_rt` and `eml_interp`. `` | `` one expected-value table (`heap/tests/expected.txt`, written from `eml_rt`'s own results by an `#[ignore]` test) pins the externs' results as a regression table. `` |
| LIR paragraph (if Task 12 left this sentence) | `` So a `Lin` field is never dupped, and `eml_runtime`'s `Heap::dup` of a `File` object is always a heap error (`HeapError::LinShared`, "a Lin object is shared"), with or without `debug_heap`. `` | `` So a `Lin` field is never dupped, and in checked mode `eml_rt` reports a `dup` of an object with the `lin` bit (a `File`) as a heap error (`HeapError::LinDup`, "a Lin object is shared"). `` |
| same | `` The interpreter's control is (function, block, statement); its frames are `Frame::{Return, Apply, Root}` (`Frame::Root` is the bottom of every continuation), `Frame::Return.resume` is an executor-defined `u64`, primitives live in `eml_interp/src/prims.rs` (their table, `Prim::row`, in `eml_lir`) and do not release their borrowed arguments, `clause` runs like `apply`, and `Value::Dummy` is a non-heap value. `` | `` The VM keeps each task's frames on a stack of one-word slots (a two-word header per frame; the bottom frame returns to `Finish`), primitives are `Ctx` methods (their table, `Prim::row`, in `eml_lir`) and do not release their borrowed arguments, `clause` runs like `apply`, and `prim dummy()` is the heap's immortal dummy object (a zero `BigInt`) that RC instructions leave alone (`docs/spec/lir.md`, 「実行の意味」). `` |
| `Session` paragraph | `` The interpreter and `eml_runtime` are single-threaded (`u32` RC, `OutputSink` wraps `Rc<RefCell<dyn Write>>`, `eml_interp::run` and `eml_cli::execute` take `&eml_lir::Program` and return `RunStats`; `Session::compile` / `compile_until(eml_lir::Pass)` return `Compiled<eml_lir::Program>` and `compile_core_until(eml_core_ir::Pass)` returns `Compiled<eml_core_ir::Program>`); parallelism is planned for a native runtime only (`docs/future/multicore.md`). `RunConfig` is `#[non_exhaustive]`; build it from `Default`. `` | `` The VM is single-threaded but keeps the constraints that let it run in parallel later (`docs/future/multicore.md`). `eml_cli::execute` takes `&eml_lir::Program`, a `&RunConfig` and a `HostRef` and returns `RunStats` (counted in checked mode only); `Session::compile` / `compile_until(eml_lir::Pass)` return `Compiled<eml_lir::Program>` and `compile_core_until(eml_core_ir::Pass)` returns `Compiled<eml_core_ir::Program>`. `RunConfig` (`debug_heap` and an `Arc<Control>`) is `#[non_exhaustive]`; build it from `Default` with `with_debug_heap` / `with_control`, and use a fresh one per run (a `Control`'s stop bits clear only with `Control::clear`). `` |
| Testing, UI bullet (text after 14c's rename) | `` Run tests always enable `debug_heap`. `run` / `run-fail` tests also run on the VM: checked mode must match the CEK's output, error text and six `RunStats` counts (all but `rc_*` and `peak_objects`), and only then fast mode runs (「2つのモードで流す」). `` | `` `run` / `run-fail` tests run on the VM in checked mode with `default_limits()` (stdout and error text are snapshotted), then in fast mode, which must give the same stdout and the same outcome (「2つのモードで流す」). `` |
| Testing, cli bullet | `` checks the binary's exit codes (0 success / 1 diagnostic or runtime error / 2 usage error). `` | `` checks the binary's exit codes (0 success / 1 diagnostic or runtime error / 2 usage error / 130 interrupted; the two SIGINT tests are `cfg(unix)`). `` |
| Testing, Benchmarks bullet | `` have their `RunStats` pinned by inline snapshots in `crates/eml_interp/tests/bench.rs` (and the VM's in `crates/eml_vm/tests/bench.rs`), so `` | `` have their `RunStats` pinned by inline snapshots in `crates/eml_vm/tests/bench.rs` (checked mode), so `` |
| same | `` `bench/run.sh` (macOS only) measures `instructions retired` with `/usr/bin/time -l` on a release build; `` | `` `bench/run.sh` (macOS only) measures `instructions retired` of `eml run` (the VM in fast mode) with `/usr/bin/time -l` on a release build and prints each program's execution part (minus `empty`); `` |
| Testing, `eml_test_support` bullet | `` `run` / `run_stats` / `execute`, their VM counterparts `run_vm` / `run_stats_vm` (checked mode) and `execute_vm` (takes the mode), multi-file `` | `` `run` / `run_stats` (the VM in checked mode with a fresh unbounded `Control`) / `execute` (takes a `&RunConfig`), multi-file `` |
| same | `` `execute` takes an `eml_lir::Program`; `run_stats` also returns the interpreter's `RunStats` (eight work counts `evidence_steps`, `yields`, `yield_frames`, `string_bytes_copied`, `rc_increments`, `rc_decrements`, `boxes`, `unboxes`, and `peak_objects`, the peak number of live heap objects), which `eml_interp`'s scaling tests bound instead of timing; `` | `` `execute` takes an `eml_lir::Program` and a `&RunConfig`; `run_stats` and `execute` also return the VM's `RunStats` (eight work counts `evidence_steps`, `yields`, `yield_frames`, `string_bytes_copied`, `rc_increments`, `rc_decrements`, `boxes`, `unboxes`, plus `peak_objects`, the peak number of live heap objects, and `peak_stack`, the peak number of task-stack words), which `eml_vm`'s scaling tests bound instead of timing; `` |
| Testing, `eml_rt` bullet (if it mentions the CEK or `eml_interp`) | any clause comparing with `eml_interp` | drop the clause |

Also check the line `cargo run -p eml_cli -- run --vm …` is gone from the Commands block and that the `eml_vm` Miri command line stays.

- [ ] **Step 8: Verify**

```bash
grep -nE 'eml_interp|eml_runtime|CEK|OutputSink|--vm|execute_vm|run_vm|run_stats_vm|runtime\.md|LIR とインタプリタ|インタプリタ' CLAUDE.md README.md docs/README.md docs/overview.md docs/future/multicore.md
s11=$(grep -n '^## S11 REPL' docs/future/roadmap.md | cut -d: -f1)
grep -nE 'eml_interp|eml_runtime|CEK|OutputSink|--vm|execute_vm|run_vm|run_stats_vm|runtime\.md|LIR とインタプリタ' docs/future/roadmap.md | awk -F: -v s11="$s11" '$1 >= s11'
grep -rn '(\.\./spec/runtime\.md)\|(spec/runtime\.md)\|(runtime\.md)' docs | grep -vE '^docs/(superpowers|reports)/'
cargo test -p eml_cli --test integration citations
```

Expected: the first grep prints nothing; the roadmap check prints nothing (every remaining hit is in the finished stages S7-S10 above `## S11 REPL`); the third grep prints only lines of `docs/spec/runtime.md` itself (no other document links to it any more); citations pass.

- [ ] **Step 9: Gate and commit**

```bash
cargo test && cargo clippy --all-targets && cargo fmt --check
git add CLAUDE.md README.md docs
git commit -m "Finish S10 in the roadmap and bring the overview documents to the VM" -m "roadmap.md: the S10b row is done with where its decisions went, the S10 decisions list what S10b changed from the plan, and history links to runtime.md become plain text (benchmarks.md too). multicore.md, overview.md, README.md, docs/README.md and CLAUDE.md describe the VM and eml_rt instead of the CEK and eml_runtime. No test changes." -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01MK52fNp5uA711Q2D4kWEtR"
```

### Task 14f: code comments, `std/Prelude.em`, delete `docs/spec/runtime.md`, final grep

**Files:**
- Modify: `crates/eml_vm/src/error.rs`, `crates/eml_vm/src/lib.rs`, `crates/eml_vm/src/vm.rs`, `crates/eml_vm/tests/bench.rs`, `crates/eml_vm/tests/evidence.rs` (comments)
- Modify: `crates/eml_extern/src/lib.rs`, `crates/eml_core_ir/src/lib.rs`, `crates/eml_core_ir/src/translate/expr.rs`, `crates/eml_lir/src/lib.rs` (comments)
- Modify: `crates/eml_cli/src/main.rs` (a comment, if Task 10/11 left it), `std/Prelude.em:134` (comment)
- Modify: `crates/eml_rt/src/**` comments only if Task 9 left any CEK reference (then Verus and verusfmt run)
- Modify: any other comment under `crates/` that the grep in Step 1 finds (comments moved by Task 7, written by Tasks 8-12)
- Delete: `docs/spec/runtime.md`
- Test: `crates/eml_cli/tests/citations.rs` (run)

**Interfaces:**
- Consumes: the final code of Tasks 1-12 (comments only are changed here), the headings from 14a-14e.
- Produces: no `docs/spec/runtime.md`; no live reference to the CEK, `eml_interp`, `eml_runtime`, `OutputSink`, `--vm`, the `*_vm` test helpers, `saved` or `save [` outside history records.

- [ ] **Step 1: List the remaining sites in code**

```bash
grep -rnE 'eml_interp|eml_interp/tests|eml_runtime|CEK|OutputSink|--vm|execute_vm|run_vm|run_stats_vm|(^|[^A-Za-z0-9_])saved([^A-Za-z0-9_]|$)|save \[|runtime\.md|LIR とインタプリタ|今のインタプリタ|インタプリタは' crates std --include='*.rs' --include='*.em' --include='*.toml' --include='*.lir' --include='*.txt'
```

Expected now (before this step's edits): the sites in the table of Step 2 plus whatever earlier tasks left; nothing under `crates/eml_interp` or `crates/eml_runtime` (deleted in Task 10), no `*.lir` hit (Task 12 removed `save [`), and no `saved` in `eml_lir`/`eml_vm`/`eml_core_ir` code (Task 12). If `saved` or `save [` still appears in code, Task 12 is incomplete: stop and report. The hand-off from Task 7: comments that name `crates/eml_interp/tests/bench.rs` or `scaling.rs` may remain in moved files; they show up here and are fixed in Step 3.

- [ ] **Step 2: Rewrite the known comment sites**

Apply each row whose old text is still present (Tasks 8, 10 and 12 may have rewritten some of them already; then check the new text says the same thing and move on).

| File | Old | New |
|---|---|---|
| `crates/eml_vm/src/error.rs` (module doc, 2 lines) | `//! 実行時エラー。表示は CEK (`eml_interp::RuntimeError`) と1文字も違わない形にする。S10b で CEK を消すときに、` / `//! `eml_cli` が公開する誤りの型をこちらに替える (docs/spec/vm.md の「置き場所」)。` | `//! 実行時エラー (docs/spec/lir.md の「実行時エラー」)。表示は CLI と UI テストが使う文言である。`eml_cli` はこの型を` / `//! 再公開する (docs/spec/vm.md の「置き場所」)。` |
| `crates/eml_vm/src/lib.rs` (`RunStats` doc, 2 lines) | `/// 実行の仕事の回数と、同時に生きていたヒープの物体の数の最大。検査モードだけで数える。欄は `eml_interp::RunStats`` / `/// と同じで、出どころは docs/spec/vm.md の「`RunStats`」にある。` | `/// 実行の仕事の回数と、同時に生きていたヒープの物体の数とスタックの語の数の最大。検査モードだけで数える。` / `/// 各欄の意味と出どころは docs/spec/vm.md の「`RunStats`」にある。` |
| `crates/eml_vm/src/lib.rs` | `/// 上限のない `Control` で実行する。上限の指定は S10b で `RunConfig` から渡す。` | (Task 8 replaced this function; if the line is still there, delete it and let the `execute` doc say: `/// `config` のモードと `Control` で実行する (docs/spec/vm.md の「置き場所」)。`) |
| `crates/eml_vm/src/vm.rs` | `/// 入口の関数の結果を手放す。CEK のいちばん下のフレームも、結果を手放してから終わる。` | `/// 入口の関数の結果を手放す (docs/spec/vm.md の「誤り、止めること、終わり」)。` |
| `crates/eml_vm/tests/bench.rs` (module doc, 3 lines) | `//! 基準のプログラム (`bench/`) の、VM での実行の仕事の回数を固定する。CEK の回数 (`crates/eml_interp/tests/bench.rs`)` / `//! とは `rc_*` と `peak_objects` が違う。理由は docs/spec/vm.md の「`RunStats`」にある。S10b で CEK を消すときに、` / `//! このテストが CEK のテストに替わる。` | `//! 基準のプログラム (`bench/`) の実行の仕事の回数を、VM の検査モードで固定する` / `//! (docs/implementation/benchmarks.md の「回数のテスト」)。` |
| `crates/eml_vm/tests/evidence.rs` (doc of `a_renewed_evidence_has_its_own_state_and_the_current_evv_outside`) | `(docs/spec/runtime.md の「evidence と `evv` の物体」)` | `(docs/spec/object-model.md の「evidence の3つの物体」)` |
| `crates/eml_vm/tests/evidence.rs` (two docs) | `/// 終わりに残る `evv` や yield は VM が `Ctx::check_finished` で見つけ、CEK のように誤りの種類は分けない。` | `/// 終わりに残る `evv` や yield は VM が `Ctx::check_finished` で見つけ、誤りの種類は分けない。` |
| `crates/eml_extern/src/lib.rs` (module doc, lines 1-2) | `//! extern の表。標準ライブラリ (`std/`) の extern の宣言ごとに、処理系が知る性質を1行に持つ。HIR、型検査、Core IR、` / `//! インタプリタはこの表を引き、名前の文字列を照らし合わせない。` | `//! extern の表。標準ライブラリ (`std/`) の extern の宣言ごとに、処理系が知る性質を1行に持つ。HIR、型検査、Core IR、` / `//! LIR、VM はこの表を引き、名前の文字列を照らし合わせない。` |
| `crates/eml_core_ir/src/lib.rs` and `crates/eml_lir/src/lib.rs` (`strings` doc, 2 lines, same in both) | `/// 文字列リテラルの定数表。`ConstString` と `CasePattern::String` が添字で引く。インタプリタは項目ごとに不死の` / `/// 物体を1つ作り、`ConstString` はその物体の参照を1つ作る (docs/spec/runtime.md の「不死の物体」)。` | `/// 文字列リテラルの定数表。`ConstString` と `CasePattern::String` が添字で引く。VM のコード生成は項目ごとに` / `/// 凍結した領域の物体を1つ作り、`ConstString` はその物体の参照を1つ作る (docs/spec/vm.md の「`CodeBase` と `FnEntry`」)。` |
| `crates/eml_core_ir/src/lib.rs` (`ConstString` doc, 2 lines) | `/// 文字列リテラルの参照を1つ作る。リテラルは不死の物体だが、LIR の `dup` と `decref` は釣り合いを保つ` / `/// (docs/spec/runtime.md)。` | `/// 文字列リテラルの参照を1つ作る。リテラルは不死の物体だが、LIR の `dup` と `decref` は釣り合いを保つ` / `/// (docs/spec/lir.md の「値の表現」)。` |
| `crates/eml_lir/src/lib.rs` (`ConstString` doc, 2 lines) | `/// 文字列リテラルの参照を1つ作る。リテラルは不死の物体だが、`dup` と `decref` の釣り合いは保つ` / `/// (docs/spec/runtime.md)。` | `/// 文字列リテラルの参照を1つ作る。リテラルは不死の物体だが、`dup` と `decref` の釣り合いは保つ` / `/// (docs/spec/lir.md の「値の表現」)。` |
| `crates/eml_core_ir/src/translate/expr.rs` (in the handle translation) | `// 節はエフェクトの操作の順に並べる。インタプリタは操作の番号で節を引く` | `// 節はエフェクトの操作の順に並べる。evidence の入口の表は操作の番号で引く` |
| `crates/eml_cli/src/main.rs` (if Task 10 left it) | `// 相対パスは CEK と同じくカレントディレクトリを基準にする` | `// 相対パスはカレントディレクトリを基準にする (docs/spec/vm.md の「置き場所」)` |
| `std/Prelude.em:134` | `-- 左辺を一意な文字列として伸ばすので、長さに比例する時間で済む (docs/spec/runtime.md の「文字列の連結」)` | `-- 左辺を一意な文字列として伸ばすので、長さに比例する時間で済む (docs/spec/object-model.md の「文字列の操作」)` |

(`/` separates the lines of a multi-line comment. Keep each file's existing line breaks and indentation; `cargo fmt` does not rewrap comments.)

`std/Prelude.em` is embedded in the binary and tied to `std/` by `eml_hir`'s test; a comment change keeps both in step because `eml_hir::STD` reads the file at build time. `cargo test -p eml_hir` checks it.

`eml_rt` (Task 9 rewrote these with the expected-value table; check):

```bash
grep -rnE 'eml_interp|eml_runtime|CEK|今のインタプリタ|インタプリタの' crates/eml_rt/src
```

Expected: no output. If lines remain, rewrite each with this table and run Verus and verusfmt in Step 6:

| Old (as of the S10b start) | New |
|---|---|
| `externs.rs:342` `// 文言は今の `eml_interp::Fault` と1文字も違わないようにする。S10 で VM に切り替えても UI テストの出力が変わらない` | `// 文言は実行時エラーの文言そのものである (docs/spec/lir.md の「実行時エラー」)` |
| `error.rs:46` `// `eml_runtime` の `File` の検査と同じ文言にする` | (delete the comment line) |
| `error.rs:70` `// `eml_runtime` の `release_fields` と同じ文言にする` | (delete the comment line) |
| `heap/ctx.rs:46` `今のインタプリタの `RunStats::string_bytes_copied` と同じ` (rest of the sentence) | `VM の `RunStats::string_bytes_copied` はこの数である (docs/spec/vm.md の「`RunStats`」)` (keep the start of the sentence) |
| `heap/ctx/strings.rs:87` `今のインタプリタの `show_string` は文字の単位で同じ規則を使い、` (rest of the sentence) | delete that sentence; keep the rule the comment states for `eml_rt` |
| `heap/ctx/strings.rs:914` `// インタプリタの数え方で、S10 で固定した `RunStats` を変えないためである` | `// `RunStats` の数え方 (docs/spec/vm.md の「`RunStats`」) に合わせるためである` |
| test files (`heap/tests/ctx.rs:62`, `:319`, `externs.rs:1`, `:75`, `:189`, `:270`, `:331`, `:639`, `host.rs:261`, `mod.rs:32`, `table.rs:1-2`, `:331`, `expected.txt:1`) | drop the clause that compares with the old implementation; keep what the test checks |

- [ ] **Step 3: Retarget every other citation of `runtime.md` in code**

```bash
grep -rn 'docs/spec/runtime\.md' crates std
```

Expected after Step 2: only comments that earlier tasks moved or wrote (for example `crates/eml_vm/tests/scaling.rs` from Task 7). Rewrite each with this table (the citations test checks the headed form, so every new citation names a heading that exists):

| Old citation | New citation |
|---|---|
| `docs/spec/runtime.md の「実行の API」` | `docs/spec/vm.md の「`RunStats`」` |
| `docs/spec/runtime.md の「evidence と `evv` の物体」` | `docs/spec/object-model.md の「evidence の3つの物体」` |
| `docs/spec/runtime.md の「文字列の連結」` | `docs/spec/object-model.md の「文字列の操作」` |
| `docs/spec/runtime.md の「不死の物体」` | `docs/spec/lir.md の「値の表現」` |
| `docs/spec/runtime.md の「`File` の参照の検査」` | `docs/spec/object-model.md の「`lin` ビット」` |
| `docs/spec/runtime.md の「`debug_heap`」` | `docs/spec/vm.md の「置き場所」` |
| `(docs/spec/runtime.md)` about string literals or `ConstString` | `(docs/spec/vm.md の「値と定数」)` |

If a comment's sentence describes the CEK heap (frames as objects, `Value`, generation numbers), rewrite the sentence for the VM before retargeting it. Then:

```bash
grep -rn 'runtime\.md' crates std
```

Expected: no output.

- [ ] **Step 4: Delete `docs/spec/runtime.md`**

```bash
grep -rn 'runtime\.md' docs CLAUDE.md README.md | grep -vE '^docs/(superpowers|reports)/'
```

Expected before deleting: hits only in `docs/spec/runtime.md` itself, the plain-text history in `docs/future/roadmap.md` (S8 rows, the S8 note, the S10 section) and `docs/implementation/benchmarks.md` (the S7 record), and no Markdown link `](…runtime.md)` anywhere else (14e Step 8 checked this).

```bash
git rm docs/spec/runtime.md
cargo test -p eml_cli --test integration citations
```

Expected: citations pass (no document links to the deleted file, no comment cites it).

- [ ] **Step 5: The final grep (spec 「文書」, last bullet, plus Task 7's hand-off)**

Run exactly:

```bash
pat='eml_interp|eml_interp/tests|eml_runtime|CEK|OutputSink|--vm|execute_vm|run_vm|run_stats_vm|(^|[^A-Za-z0-9_])saved([^A-Za-z0-9_]|$)|save \[|runtime\.md|LIR とインタプリタ'
grep -rnE "$pat" docs crates std bench CLAUDE.md README.md \
  | grep -vE '^docs/(superpowers|reports)/' \
  | grep -vE '^docs/implementation/benchmarks\.md:|^docs/future/roadmap\.md:'
```

Expected output, exactly these two lines (program output in two UI snapshots; the word is data, not a reference to `saved`):

```
crates/eml_cli/tests/snapshots/integration__ui__run@effects__resume_reaches_handlers_around_the_resumption.em.snap:10:saved
crates/eml_cli/tests/snapshots/integration__ui__run@effects__resume_under_another_handler.em.snap:7:saved
```

Then the two history documents:

```bash
rec=$(grep -n '^## 記録' docs/implementation/benchmarks.md | cut -d: -f1)
grep -nE "$pat" docs/implementation/benchmarks.md | awk -F: -v rec="$rec" '$1 < rec'
s11=$(grep -n '^## S11 REPL' docs/future/roadmap.md | cut -d: -f1)
grep -nE "$pat" docs/future/roadmap.md | awk -F: -v s11="$s11" '$1 >= s11'
```

Expected: both print nothing. In `benchmarks.md` every hit is in a record below `## 記録` (S4a to S10a, and the S10b record); in `roadmap.md` every hit is in the finished stages above `## S11 REPL` (the S8 and S10 rows of 「段の列」, the 「順序の理由」 bullets on S7 and on the CEK, and the S8, S9 and S10 sections). `docs/reports/` is excluded as written research. If any other line prints, fix it the same way as the sites above (describe the VM, or drop the comparison with the deleted code) and rerun.

- [ ] **Step 6: Gate**

```bash
cargo test
cargo clippy --all-targets
cargo fmt --check
nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_vm --test integration miri::
nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance -Zmiri-tree-borrows" cargo miri test -p eml_vm --test integration miri::
```

Expected: all pass, clippy 0 warnings. Miri runs because `crates/eml_vm/src` changed (comments only; the gate rule applies to any change there). If Step 2 changed any file under `crates/eml_rt/src`, also run:

```bash
cargo verus verify -p eml_rt
verusfmt --verus-only --check $(git ls-files 'crates/eml_rt/src/*.rs')
```

Expected: `… verified, 0 errors` and no verusfmt diff.

- [ ] **Step 7: Commit**

```bash
git add -A crates std docs
git commit -m "Delete runtime.md and the last references to the CEK in comments" -m "Comments in eml_vm, eml_extern, eml_core_ir, eml_lir, eml_cli and std/Prelude.em describe the VM and cite the documents that now hold the rules (vm.md, lir.md, object-model.md). docs/spec/runtime.md is deleted; its rules live in vm.md, lir.md and object-model.md. The final search finds the CEK, eml_interp, eml_runtime, OutputSink, --vm, the *_vm helpers, saved, save [ and runtime.md only in history records. No test changes." -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01MK52fNp5uA711Q2D4kWEtR"
```

### Task 14g: delete the S10b design and plan

The project process (CLAUDE.md, 「docs/ are the source of truth」; the S10a precedent `64ac649c Delete the S10a design and plan`): once the decisions are in `docs/`, the stage's spec and plan go.

**Files:**
- Delete: `docs/superpowers/specs/2026-10-15-s10b-switchover-design.md`
- Delete: `docs/superpowers/plans/2026-10-15-s10b-switchover.md`
- Test: `crates/eml_cli/tests/citations.rs` (run)

**Interfaces:**
- Consumes: 14a-14f done (every decision of the spec's 「文書」 section is in `docs/`).
- Produces: nothing.

- [ ] **Step 1: Check nothing points at the two files**

```bash
grep -rn '2026-10-15-s10b-switchover' docs crates std CLAUDE.md README.md | grep -v '^docs/superpowers/'
```

Expected: no output.

- [ ] **Step 2: Delete and run the gate**

```bash
git rm docs/superpowers/specs/2026-10-15-s10b-switchover-design.md docs/superpowers/plans/2026-10-15-s10b-switchover.md
cargo test -p eml_cli --test integration citations
cargo test && cargo clippy --all-targets && cargo fmt --check
```

Expected: all pass.

- [ ] **Step 3: Commit**

```bash
git commit -m "Delete the S10b design and plan" -m "Every decision of the S10b design now lives in docs/spec, docs/implementation and docs/future, so the design and its plan are removed, as after S10a. No test changes." -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01MK52fNp5uA711Q2D4kWEtR"
```
