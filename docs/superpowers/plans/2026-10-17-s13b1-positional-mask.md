# S13b-1 位置つきの `mask` Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 呼び出しの `mask` を、エフェクトごとの「先頭から n 個を残し、続く m 個を飛ばす」項目にし、E2008 をなくす。`Yield Int` を受けて `Yield String` を出す handler が動くようにする。

**Architecture:** 下の層から順に、意味を変えずに表し方を項目に替え、最後に型検査で n を正にする。Task 1 は `eml_rt` の `evv_mask` を項目で受ける形にして Verus で証明し直し、VM の表もその型にする。Task 2 は `eml_lir` に項目の型を置き、LIR の基本操作、Core IR の呼び出しの `mask`、EP、VM のコード生成を項目で通す。この時点では translate が型検査の多重集合を n = 0 の項目にまとめるので、どのプログラムの意味も変わらない。Task 3 で型検査が (n, m) を返し、E2008 を消し、UI テストで端から端まで確かめる。Task 4 は段の終わりの文書と計測である。

**Tech Stack:** Rust (edition 2024、stable 1.98.1)、`insta`、Verus (`cargo verus verify -p eml_rt`)、`verusfmt`、Miri (`.#miri` の shell)。新しい依存はない。

**Spec:** `docs/superpowers/specs/2026-10-17-s13b1-positional-mask-design.md`

この計画は、計画を書くときに調べた `main` (`f63481f1`) のコードに基づく。スナップショットの期待値は、各タスクで `cargo insta review` で中身を確かめてから受け入れる。

## spec からの変更

1. **項目の型を3つの crate に置く。** spec は「項目の型を `eml_lir` に置き、Core IR と `Prim::EvvMask` で共有する」とした。それに加えて、`eml_types` (`eml_lir` に依存しない) と `eml_rt` (vstd にだけ依存する) にもそれぞれ項目の型を置く。`eml_types::MaskItem` はエフェクトを `EffectId` で、`eml_rt::MaskItem` はエフェクトを `u16` で持つ。依存の向きを変えないためである
2. **作業の途中の写し替え。** Task 1 では VM のコード生成が、まだ多重集合の `Prim::EvvMask` を n = 0 の項目にまとめて `eml_rt` に渡す。Task 2 では translate が、まだ多重集合の型検査の記録を n = 0 の項目にまとめる。どちらも次のタスクで消える、タスクの間だけの写し替えである。各タスクの終わりにワークスペース全体が通るようにするためである

## 既存のテストの変更

spec の「テストの変更」のとおりである。コミットの本文に、合否の変更と期待値の変更はその理由を、機械的な追従は「mechanical」と書く。

- 期待値の変更 (Task 2): 同じエフェクトを2回以上飛ばす `mask` のテキスト (`[State, State]` → `[State*2]`、`mask$[State, State]` → `mask$[State*2]`、`evv_mask[State, State]` → `evv_mask[State*2]`) を含む期待値と手で書いた IR。`crates/eml_core_ir/tests/verify.rs` (`a_mask_may_name_one_effect_twice` は、名前を `a_mask_may_skip_one_effect_twice` に変え、`[State*2]` を受け入れることを確かめる)、`crates/eml_core_ir/tests/text.rs`、`crates/eml_core_ir/src/evidence/tests.rs` (`a_mask_names_its_multiset` は、名前を `a_mask_names_its_items` に変え、`"mask$[State*2]"` を期待する)、`crates/eml_lir/tests/text.rs` と `verify.rs`
- 期待値の変更 (Task 3): `crates/eml_types/tests/masks.rs` の出力の形を `[State*2]` や `[Log@1]` の項目のテキストにする (`a_label_twice_is_masked_twice` の期待値は `6:10#0 [State*2]`)
- 合否の変更 (Task 3): `tests/ui/check-fail/types/mask_conflict.em` と `mask_conflict_handles.em` を `tests/ui/run/effects/mask_behind.em` と `mask_behind_handles.em` に移して書き足す。`crates/eml_types/tests/masks.rs` の `a_callee_that_performs_the_masked_effect_itself_is_rejected` と `crates/eml_types/src/table/tests.rs` の `a_mask_cannot_skip_an_effect_the_callee_names` を、(n, m) の `mask` を確かめるテストに書き換える
- 機械的な追従: 項目の型にしたことによる、期待値を変えないテストの書き換え (`include_row(..) == Ok(Vec::new())`、`eml_rt` のテストと `rt_bench` の `evv_mask` の引数、`Prim::EvvMask(vec![…])` の構築)

## Global Constraints

- 作業はブランチ `s13b-1` で行う。コミットの末尾に次の2行を付ける:
  `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`
  `Claude-Session: https://claude.ai/code/session_015Sjq4FvKMzqB1AhP4iTKnX`
- 各タスクの終わりに `cargo test --workspace` (文書の引用の検査 `crates/eml_cli/tests/citations.rs` を含む)、`cargo clippy --all-targets` (警告 0)、`cargo fmt --check` を通す
- `crates/eml_rt` を変えたタスクは、`cargo verus verify -p eml_rt`、`verusfmt --verus-only --check $(git ls-files 'crates/eml_rt/src/*.rs')`、`nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_rt trusted::` も通す
- `git diff` は `--no-ext-diff` を付ける (difftastic が外部の diff に設定されている)
- テストの変更は上の「既存のテストの変更」の種類だけにする。ほかの合否の変更が要ると分かったら、作業を止めて spec の「テストの変更」に足してから進める
- コードのコメントと `docs/` は日本語で書き、`yomiyasu:yomiyasu` の規則に従う。コメントは理由を書き、規則は `docs/…/x.md の「見出し」` の形で引く。`CLAUDE.md` は英語である
- 項目の意味: エフェクト `L` の項目 (n, m) は、`L` の evidence のうち、先に同じ `L` が s 個あるもの (s は 0 から数える) を、n ≤ s < n + m のときに除く。項目のないエフェクトの evidence は残す。同じエフェクトの項目が2つあれば先の項目を使い、m が 0 の項目は何も除かない (`eml_rt` の `mask_spec` の意味。verifier はこの2つを誤りにする)
- 項目のテキストの形: エフェクトの名前 (入口のモジュールのエフェクトは修飾せず、ほかは修飾し、表にない番号は `#N`) の後に、n が 0 でなければ `@n`、m が 1 でなければ `*m` を続ける。`State`、`State*2`、`Yield@1`、`Yield@1*2`。項目は `, ` で区切る。`parse` の誤りの文言:
  - `@0` か `*1` を書いた: ``write `{最短の形}` for `{書いた語}` `` (例: ``write `Yield` for `Yield@0` ``)
  - `*0` を書いた: ```{書いた語}` skips no handler``
  - 0 で始まる数: 今の整数の読み方の誤りと同じ
- verifier の文言 (Core IR): 今の `a mask names an unknown effect #N`、`a mask is not in ascending order` (同じエフェクトの項目が2つあるときもこの文言)、新しい ``a mask item of `{L}` skips no handler``。LIR: 今の ``the effects of `prim {name}` are not in ascending order`` (重なりも含む)、新しい ``an item of `prim {name}` skips no handler``。``masks no effect`` は今のまま

## Review Focus

spec が求めるのに、どのタスクのテストも直接は通らないおそれが高い入力と、利用者が期待する振る舞いである。各行のテストを、持ち主のタスクに足してある。

1. REPL のセッションでエフェクトの番号が引き直されたときの、n が正の項目: 項目 (エフェクト、n、m) が1つの単位で並べ直され、n と m が別のエフェクトに付かない (Task 2 の `effects_are_renumbered_by_name` に `A@1` の項目を足す)
2. 2つのエフェクトを同時に飛ばし、片方だけ n が正の `mask` (`[State, Yield@1]`): 型検査の記録、テキストの往復、`mask$` の名前が、エフェクトの番号の順で同じになる (Task 2 の `a_mask_with_two_items_reads_back`、Task 3 の `two_effects_are_masked_with_their_own_counts`)
3. パラメータ付き handler の `k v st` の呼び出しに付く (n, m): 最後の矢印 (`#1`) に付き、最初の矢印には付かない (Task 3 の `a_stateful_continuation_keeps_its_own_label_at_its_last_arrow`)
4. `multi` の継続を2回再開したときの、位置つきの `mask` の付け直し: 2回とも同じ handler に届く (Task 3 の `run/effects/mask_behind_multi.em`)
5. 検査モードで、合う evidence が n + m 個に足りない `evv_mask`: 確保の前に `NoHandler` を返し、ヒープも `evidence_steps` も変えない (Task 1 の `a_mask_that_keeps_and_skips_more_evidence_than_the_list_holds`)

---

### Task 1: `eml_rt` の `evv_mask` を項目で受ける

**Files:**
- Modify: `crates/eml_rt/src/heap/ctx.rs` (項目の型、`mask_spec` と spec の関数 `occurs`、`kept_at`、`mask_from`、`wkept`、`wdropped`、`mask_walk`、`mask_dups`、`copy_seq`、実行の補助 `count_effect`、`Ctx::evv_mask` と `checked_mask`)
- Modify: `crates/eml_rt/src/heap/ctx/proof.rs` (`lemma_kept_*` から `lemma_evv_mask_ok` までの mask の補題)
- Modify: `crates/eml_rt/src/lib.rs` (`MaskItem` を公開する)
- Modify: `crates/eml_rt/src/heap/tests/ctx.rs`、`crates/eml_rt/src/heap/tests/trusted.rs`、`crates/eml_rt/examples/rt_bench.rs`
- Modify: `crates/eml_vm/src/code.rs` (`effect_lists: Vec<u16>` を `mask_lists: Vec<eml_rt::MaskItem>` にする)、`crates/eml_vm/src/compile/emit.rs`、`crates/eml_vm/src/vm.rs`、`crates/eml_vm/src/disasm.rs`、`crates/eml_vm/src/op.rs` (`EvvMask` の operands の説明)

**Interfaces:**
- Produces: `eml_rt::MaskItem { pub effect: u16, pub keep: u32, pub skip: u32 }` (`Clone`、`Copy`、`Debug`、`PartialEq`、`Eq`。`verus!` の中に置く)
- Produces: `pub unsafe fn Ctx::evv_mask(&mut self, items: &[MaskItem], w: Word) -> Result<Word, HeapError>`
- Produces: `pub closed spec fn mask_spec(items: Seq<MaskItem>, seq: Seq<(u16, Word)>) -> Seq<(u16, Word)>` (上の「Global Constraints」の項目の意味)。速いモードの前提は `mask_spec(items@, evv_seq(w)).len() + skip_total(items@) == evv_seq(w).len()`。`skip_total` は、同じエフェクトの先の項目がない項目の m の和である
- Produces: `CodeBase::mask_lists: Vec<MaskItem>`。`Op::EvvMask { dst, at }` の `operands[at..]` は [mask_lists の先頭, 長さ, リストのスロット]
- 逆アセンブルの形: `evv_mask s{dst}, [0, 1@1*2] s{l}` (項目を上の「項目のテキストの形」の数字の版で書く)

- [ ] **Step 1: 失敗するテストを書く** (`crates/eml_rt/src/heap/tests/ctx.rs`、`both_modes!` に登録する)
  - `a_mask_keeps_the_first_matches_before_it_skips`: tag の並びが先頭から 0、1、1、0、1 の evv に、項目 `{effect: 1, keep: 1, skip: 1}` をかける。結果の evidence の列は元の 0 番、1 番、3 番、4 番で、0 番と1番の節点は写し (元と違う語)、3 番からは元のリストを共有する (`masked[2].0 == original[3].0`)。検査モードでは `evidence_steps` が 3 増える (除く節点までたどる)
  - `a_mask_that_keeps_and_skips_more_evidence_than_the_list_holds`: tag 0、1 の evv に項目 `{1, keep: 1, skip: 1}`。検査モードで `NoHandler`、生きている物体の数と `evidence_steps` が変わらない
  - `a_second_item_of_the_same_effect_is_ignored`: tag 1、1 の evv に項目 `[{1, 0, 1}, {1, 0, 5}]`。先頭の1つだけを除く
  - `a_mask_skips_the_first_matches_of_each_effect_in_its_multiset` は、引数を `[{0,0,1},{1,0,1}]` と `[{1,0,2}]` にして名前を `a_mask_skips_the_first_matches_of_each_item` に変える (機械的な追従。期待値は同じ)。ほかの `evv_mask(&[…])` の呼び出しも同じく項目にする
- [ ] **Step 2: 失敗を確かめる** — Run: `cargo test -p eml_rt ctx::` / Expected: コンパイルの誤り (`MaskItem` がない)
- [ ] **Step 3: `MaskItem` と `evv_mask` を実装する**
  - spec の関数は、`occurs` を「エフェクトの最初の項目」を引く spec の関数に替え、`kept_at` を「最初の項目があり、`keep <= seen < keep + skip` なら除く」の否定にする。`wkept`、`wdropped` も同じ形にし、`mask_walk` の「除く数が尽きる」は `skip_total` と比べる。n と m は spec では `int` で扱う
  - 実行のコードは、節点ごとに `seen_at_least` を `keep` と `keep + skip` (u64 で足す) の2回呼んで判定する。除いた数の和の目標は、`skip_total` を返す補助の関数で求める。検査モードの前もってのたどり (`checked_mask`) は、項目ごとに `keep + skip` 個の合う組を数え、足りなければ確保の前に `NoHandler` を返す
  - 補題は今の構成 (`lemma_mask_count`、`lemma_cut`、`lemma_copy_seq_step`、`lemma_evv_mask_ok` など) をそのまま使い、`occurs` と `kept_at` に依る所を新しい定義に替える。`effects` という名前は `items` にする
  - `heap/tests/trusted.rs` の `every_primitive_once` と `rt_bench` の `mask` を項目の引数にする
- [ ] **Step 4: VM を新しい表にする**
  - `emit.rs` の `Prim::EvvMask(effects)` は、引き直した番号で昇順に並べたあと、同じ番号の連なりを `MaskItem { effect, keep: 0, skip: 連なりの長さ }` にまとめて `mask_lists` に足す。このまとめは Task 2 で、項目をそのまま写す形に置き換える
  - `vm.rs` は `mask_lists` の切り出しを `ctx.evv_mask` に渡す。`disasm.rs` は上の形で書く
- [ ] **Step 5: テストと検証を通す**
  - Run: `cargo test --workspace` / Expected: PASS (`crates/eml_vm/tests/machine.rs` の `evv_mask s0, [0, 1]` は表示が同じなので変わらない)
  - Run: `cargo verus verify -p eml_rt` / Expected: `verification results:: N verified, 0 errors`
  - Run: `verusfmt --verus-only --check $(git ls-files 'crates/eml_rt/src/*.rs')` / Expected: 出力なし
  - Run: `nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_rt trusted::` / Expected: PASS
  - `cargo clippy --all-targets` と `cargo fmt --check`
- [ ] **Step 6: コミットする** — `git commit -m "Take positional mask items in evv_mask"` (本文に、`mask_spec` の意味と、VM がまだ n = 0 の項目にまとめることを書く)

### Task 2: 項目を LIR、Core IR、EP、VM のコード生成に通す

**Files:**
- Modify: `crates/eml_lir/src/lib.rs` (`MaskItem` を置いて公開する)、`crates/eml_lir/src/prim.rs` (`Prim::EvvMask(Vec<MaskItem>)`、`Immediates::Mask(&[MaskItem])` が `Immediates::Effects` に代わる)、`crates/eml_lir/src/syntax.rs` (`effects_text` を `mask_text` に替え、`Reader` に項目の読み方を足す)、`crates/eml_lir/src/check.rs` (`check_prim`)
- Modify: `crates/eml_core_ir/src/lib.rs` (`Rhs::Call { call, mask: Vec<MaskItem> }`)、`pretty.rs`、`text.rs` (`mask`)、`verify.rs` (`check_mask`)、`translate/expr.rs` (`mask`)、`translate/program.rs` (空の `mask`)、`evidence/mod.rs` (`Helpers::masks`、`Helpers::mask`)、`evidence/helpers.rs` (`mask_name`、`mask`)、`evidence/handlers.rs` (`masked_call`)、`evidence/split.rs` (`Annotations::masked`、`Point::mask`)
- Modify: `crates/eml_vm/src/compile/emit.rs`
- Test: `crates/eml_lir/tests/text.rs`、`crates/eml_lir/tests/verify.rs`、`crates/eml_core_ir/tests/text.rs`、`crates/eml_core_ir/tests/verify.rs`、`crates/eml_core_ir/src/evidence/tests.rs`、`crates/eml_vm/tests/evidence.rs`、`crates/eml_vm/tests/machine.rs`

**Interfaces:**
- Consumes: `eml_rt::MaskItem` (Task 1)
- Produces: `eml_lir::MaskItem { pub effect: u32, pub keep: u32, pub skip: u32 }` (`Clone`、`Copy`、`Debug`、`PartialEq`、`Eq`、`Hash`)。並びは `effect` の真に昇順
- Produces: `eml_lir::syntax::mask_text(effects: &[EffectInfo], items: &[MaskItem]) -> String` (`State, Yield@1`)。Core IR の `mask [...]`、`mask$[...]`、`prim evv_mask[...]` がこれを使う
- Produces: `Reader::mask_item(&self, word: &str, line: usize) -> Result<MaskItem, ParseError>`。語の末尾の `*m` と `@n` を外してから `effect_ref` で引く
- Produces: `Helpers::mask(&mut self, program: &mut Program, items: &[MaskItem]) -> FnIdx`、`helpers::mask_name(program: &Program, items: &[MaskItem]) -> String`

- [ ] **Step 1: 失敗するテストを書く**
  - `crates/eml_lir/tests/text.rs`: `a_mask_item_reads_back` (`prim evv_mask[A, B@1, C*2, D@1*2](w.0)` を読んで同じ表示に戻る)、`a_mask_item_in_its_longer_form_is_an_error` (`A@0` → ``line N: write `A` for `A@0` ``、`A*1` → ``write `A` for `A*1` ``、`A@1*1` → ``write `A@1` for `A@1*1` ``)、`a_mask_item_that_skips_nothing_is_an_error` (`A*0` → ```A*0` skips no handler``)、`#3@1` と REPL の中段の名前 `Log#3@1` を読めること
  - `crates/eml_lir/tests/verify.rs`: `an_evv_mask_names_each_effect_once` (`[A, A@1]` を手で組んだ `Prim::EvvMask` が ``the effects of `prim evv_mask` are not in ascending order``)、`an_evv_mask_item_skips_some_handler` (`skip: 0` → ``an item of `prim evv_mask` skips no handler``)
  - `crates/eml_core_ir/tests/text.rs`: `a_mask_with_two_items_reads_back` (`let t.2: tobj = mask [State, Yield@1] yields apply c.0(())`)
  - `crates/eml_core_ir/tests/verify.rs`: `a_mask_names_each_effect_once`、`a_mask_item_skips_some_handler` (上の Core IR の文言)
  - `crates/eml_core_ir/src/evidence/tests.rs`: `a_mask_with_kept_handlers_is_named_by_its_items` (`Helpers::mask` に `[Yield@1]` を渡すと `mask$[Yield@1]` ができ、その本体の `prim evv_mask[Yield@1]`)
  - `crates/eml_vm/tests/evidence.rs`: `evv_mask_keeps_the_innermost_evidence_and_skips_the_next`。今の `evv_mask_skips_the_innermost_evidence_of_an_effect` と同じ形で `Counter` を3つ積み、`evv_mask[Counter@1]` の後の `ev_lookup[Counter]` が一番内側の `Counter` を、その外側を飛ばしたリストの2つ目の `Counter` が一番外側の `Counter` を返すことを、各 evidence の状態の値で確かめる
  - `crates/eml_vm/tests/machine.rs` の `effects_are_renumbered_by_name`: 2つ目のプログラムの `evv_mask[B, A]` を `evv_mask[B, A@1]` にし、`evv_mask s0, [0@1, 1]` を期待する (番号の引き直しで項目が1つの単位で動く)
  - 上の「既存のテストの変更」の、Task 2 の期待値を書き換える
- [ ] **Step 2: 失敗を確かめる** — Run: `cargo test -p eml_lir -p eml_core_ir -p eml_vm` / Expected: コンパイルの誤りか、新しいテストの失敗
- [ ] **Step 3: `eml_lir` を実装する** — `Prim::row` の `EvvMask` の行 (`tobj` の借用の引数1つ、結果 `tobj`、印 `unr`) は変えない。`KINDS` の見本は `EvvMask(Vec::new())` のまま
- [ ] **Step 4: Core IR と EP を実装する**
  - `translate/expr.rs` の `mask` は、型検査の多重集合をエフェクトの番号に写して昇順に並べ、同じ番号の連なりを `MaskItem { effect, keep: 0, skip: 連なりの長さ }` にする。Task 3 で型検査の項目を写す形に置き換える
  - `check_mask` は、表にないエフェクト、真に昇順でない並び、`skip` が 0 の項目を誤りにする。`Helpers::mask` の `debug_assert!` も同じ条件にする
  - `masked_call`、`helpers::mask`、`split.rs` は項目の並びをそのまま渡す。包みの形は変えない
- [ ] **Step 5: VM のコード生成を実装する** — `emit.rs` は、項目ごとにエフェクトの番号を `self.effect(e)` で引き直し、`eml_rt::MaskItem` にして `effect` で並べ直す (Task 1 のまとめを消す)
- [ ] **Step 6: テストを通す** — Run: `cargo test --workspace` / Expected: PASS。`cargo insta review` で、変わった期待値が上の「既存のテストの変更」の Task 2 の範囲だけであることを確かめる。`cargo clippy --all-targets`、`cargo fmt --check`
- [ ] **Step 7: コミットする** — `git commit -m "Carry mask items through Core IR, LIR and the VM"` (本文に、`[State, State]` を `[State*2]` に書き換えた期待値の変更とその理由、translate がまだ n = 0 の項目にまとめることを書く)

### Task 3: 型検査が (n, m) を決め、E2008 をなくす

**Files:**
- Modify: `crates/eml_types/src/lib.rs` (`MaskItem`、`BodyTypes::masks`、`codes::MASK_CONFLICT` を消す)、`crates/eml_types/src/table/row.rs` (`include_row`、`mask`)、`crates/eml_types/src/table/mod.rs` (`UnifyError::MaskConflict` を消す)、`crates/eml_types/src/check/body.rs` (`Typing::masks`、`signature_row_lists` を消す)、`crates/eml_types/src/check/report.rs` (`include_call_row` の `MaskConflict` の枝を消す)
- Modify: `crates/eml_core_ir/src/translate/expr.rs` (`mask`)
- Move: `tests/ui/check-fail/types/mask_conflict.em` → `tests/ui/run/effects/mask_behind.em`、`tests/ui/check-fail/types/mask_conflict_handles.em` → `tests/ui/run/effects/mask_behind_handles.em` (古いスナップショットを消す)
- Create: `tests/ui/run/effects/transform_handler.em`、`mask_behind_continuation.em`、`mask_behind_state.em`、`mask_behind_multi.em`
- Test: `crates/eml_types/tests/masks.rs`、`crates/eml_types/src/table/tests.rs`、`crates/eml_core_ir/tests/translate.rs`

**Interfaces:**
- Consumes: `eml_lir::MaskItem` (Task 2)
- Produces: `eml_types::MaskItem { pub effect: EffectId, pub keep: u32, pub skip: u32 }` (`Clone`、`Copy`、`Debug`、`PartialEq`、`Eq`)
- Produces: `BodyTypes::masks: HashMap<(ExprId, usize), Vec<MaskItem>>`。並びはエフェクトが rest に初めて現れた順である
- Produces: `Table::include_row(&mut self, callee: &Row, ambient: &Row) -> Result<Vec<MaskItem>, UnifyError>`。rest の handle できるエフェクト `L` ごとに、`keep` は `callee.labels` の中の `L` の数、`skip` は rest の中の `L` の数

- [ ] **Step 1: 失敗するテストを書く**
  - `crates/eml_types/tests/masks.rs`: 出力の形を `<行:列>#<矢印> [<項目>, …]` (項目は上の「項目のテキストの形」) にし、spec の「規則」の例の表の5行を1つずつ確かめる。`a_callee_that_performs_the_masked_effect_itself_keeps_its_own_label` (今の E2008 のテストの入力で `10:16#0 [Log@1]`)、`a_transforming_handler_keeps_the_producers_own_label` (`render` の `g ()` が `[Yield@1]`)、`two_effects_are_masked_with_their_own_counts` (今の row `<Log, State Int, Log | e>` から `<Log | e>` を呼ぶと `[State, Log@1]`。並びは rest に現れた順)、`a_stateful_continuation_keeps_its_own_label_at_its_last_arrow` (パラメータ付き handler の節が同じエフェクトの handler を内側に設けてその中で `k v st` を呼ぶと、`#1` に `[L@1]` が付き、`#0` には付かない)
  - `crates/eml_types/src/table/tests.rs`: `a_mask_cannot_skip_an_effect_the_callee_names` を `a_mask_keeps_the_labels_the_callee_names` にし、`include_row` が `Ok(vec![MaskItem { effect: state, keep: 1, skip: 1 }])` を返すことを確かめる
  - `crates/eml_core_ir/tests/translate.rs`: `a_masked_call_keeps_the_callees_own_labels` (`render` の `g ()` の呼び出しが `mask [Yield@1] yields apply …` になる)
  - UI テスト (`tests/ui/run/effects/`)。どれも、n と m がどちらも正の `mask` は、末尾が rigid な row 変数のシグネチャを持つトップレベルの関数の中に置く
    - `transform_handler.em` (完了の条件): `gen : (Unit -> <e> Unit) -> Unit -> <Yield Int | e> Unit` (`yield 1`、`cb ()`、`yield 2`)、spec の `render`、`strings : (Unit -> <Yield String, IO | e> a) -> <IO | e> a` と `bools : (Unit -> <Yield Bool, IO | e> a) -> <IO | e> a` (受けた値を `string: …`、`bool: …` と出して再開する)、`main` は `use bools`、`use strings`、`render (gen (fn () -> yield True))`。期待する標準出力は `string: n=1`、`bool: True`、`string: n=2` の3行
    - `mask_behind.em` (今の `mask_conflict.em` から): `tagged : String -> (Unit -> <Log, IO | e> a) -> <IO | e> a` (受けた文字列を `{tag}: {s}` と出して再開する) を3つ重ね (`third`、`second`、`first` の順に `use`)、`outer (fn () -> log "y")` を呼ぶ。`both` の `log "x"` は `first` に、`action` の `log "y"` は `second` を飛ばして `third` に届く。期待する出力は `first: x`、`third: y`
    - `mask_behind_handles.em` (今の `mask_conflict_handles.em` から): `outer` のシグネチャを `(Unit -> <IO | e> a) -> <IO | e> a` にし、内側の handler が `inner: {s}`、外側の handler が `middle: {s}` を出して再開する。`main` の `tagged "outside"` の下で `outer (fn () -> log "y")` を呼ぶ。期待する出力は `inner: x`、`outside: y`
    - `mask_behind_continuation.em`: `twice : (Unit -> <Log | e> a) -> <Log | e> a` の節が `log "outer sees \{s}"` の後に内側の `Log` の handler (`log "inner sees \{t}"` を出して再開する) を設け、その中で `k ()` を呼ぶ。`main` は `use printing` (受けた文字列をそのまま出す)、`use twice`、`log "a"`、`log "b"`。期待する出力は `outer sees a`、`inner sees outer sees b`
    - `mask_behind_state.em` と `mask_behind_multi.em`: `mask_behind_continuation.em` と同じ形を、パラメータ付き handler (状態に数を持つ) と、`multi` の操作の継続を2回再開する形で書く。どちらも、再開した区間の操作が届く handler を出力で示す
- [ ] **Step 2: 失敗を確かめる** — Run: `cargo test -p eml_types --test integration masks::` と `cargo test -p eml_cli --test integration ui::` / Expected: 新しいテストが E2008 で失敗する
- [ ] **Step 3: `include_row` と `mask` を実装する** — `mask` は rest の handle できるラベルをエフェクトごとに数え、`callee.labels` の同じエフェクトの数を `keep` にする。`UnifyError::MaskConflict`、`report.rs` の枝、`signature_row_lists`、`codes::MASK_CONFLICT` を消す (`body_arrow_range` は E2008 のほかでも使うので残す)。コメントは docs/spec/types.md の「推論」と docs/spec/effects.md の「健全性」を引く
- [ ] **Step 4: translate を実装する** — `translate/expr.rs` の `mask` は、型検査の項目のエフェクトを `effect_index` で写し、`eml_lir::MaskItem` にして `effect` で並べる (Task 2 のまとめを消す)
- [ ] **Step 5: テストを通す** — Run: `cargo test --workspace` / Expected: PASS。`cargo insta review` で、移した2つのテストの古いスナップショット (`crates/eml_cli/tests/snapshots/integration__ui__check_fail@types__mask_conflict*.snap`) が消え、新しい `run` のスナップショットの出力が上の期待のとおりであることを確かめる。`cargo clippy --all-targets`、`cargo fmt --check`
- [ ] **Step 6: コミットする** — `git commit -m "Keep the callee's own labels in positional masks"` (本文に、E2008 をなくしたことと、合否を変えた4つのテストとその理由を書く)

### Task 4: 段の終わりの文書と計測

**Files:**
- Modify: spec の「段の終わりの文書」の一覧のすべて (`docs/spec/types.md`、`effects.md`、`core-ir.md`、`lir.md`、`object-model.md`、`vm.md`、`diagnostics.md`、`docs/implementation/diagnostics.md`、`architecture.md`、`testing.md`、`status.md`、`benchmarks.md`、`docs/future/verification.md`、`docs/future/roadmap.md`、`CLAUDE.md`)
- spec とこの計画は、ブランチを合流させた後に別のコミットで消す (これまでの段と同じ)

**Interfaces:**
- Consumes: Task 1〜3 のすべて

- [ ] **Step 1: 規範の文書を書く** — spec の「規則」(項目の形、型検査での決め方と例の表、健全性の書き直し、推論用の row 変数が末尾にある場合) を `docs/spec/types.md` の「推論」と `docs/spec/effects.md` の「handler の意味」「健全性」に移す。健全性の不変条件は「具体化した row で、見える handler の並びがその row のラベルの並びで始まる」形にする。`docs/spec/core-ir.md` の「`perform` と節の3つの形」に、その場で動く節の中の `mask` が節の設けた handler にだけ触れる理由を足す。`docs/spec/diagnostics.md` は E2008 の行を消し、E2007 と同じく欠番と書く
- [ ] **Step 2: 実装と検証の文書を書く** — `object-model.md` の `mask_spec`、`evv_mask` の表の行、費用の議論 (文字を項目の n、m と替える)、`lir.md` と `vm.md` の即値と `mask_lists`、`testing.md` のテキストの形 (`@n`、`*m`、名前が `@` と `*` を含まないので読み方が一意であること)、`architecture.md` の `BodyTypes::masks`、`docs/future/verification.md` から E2008 を除く。`CLAUDE.md` の `mask` に触れる所を項目の形に直す
- [ ] **Step 3: 計測する** — Run: `bench/rt.sh` / Expected: `mask` の1回あたりの命令の数が出る。[計測の基準](../../implementation/benchmarks.md) に S13b-1 の節として記録する。`crates/eml_vm/tests/bench.rs` の `RunStats` が変わっていないことも書く
- [ ] **Step 4: ロードマップと現在地を更新する** — `docs/future/roadmap.md` の S13b の節から「決めたこと: 位置つきの `mask` (S13b-1)」を消し、分け方の表の S13b-1 を済みにする。`docs/implementation/status.md` の今の言語の範囲に、位置つきの `mask` を書く
- [ ] **Step 5: 検査を通す** — Run: `cargo test --workspace` (文書の引用の検査を含む) / Expected: PASS。`python3 <yomiyasu>/scripts/yomiyasu_lint.py` を書き直した文書にかけ、指摘を確かめる
- [ ] **Step 6: コミットする** — `git commit -m "Record S13b-1 in the specs, the status and the roadmap"`。spec と計画を消すコミットは、ブランチを合流させた後に別に作る
