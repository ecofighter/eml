# S12b `try_io` と `exit` Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** IO の失敗を `try_io` で捕まえ、`exit` で終了コードを選んで終われるようにする。どちらも、途中のフレームが持っていた値 (`File` など) を EP の中断の経路で後始末する。

**Architecture:** 規範の文書を先に書く (Task 1)。`eml_rt` に、見つからなくてもよい evidence の探索 `ev_find`、`exit` の中断を始める `exit_abort`、`Fault` から `IOErrorKind` のタグを決める関数を足す (Task 2)。`extern effect` に操作を書けるようにし (Task 3)、Prelude の `IO` に隠れた `never` の操作 `io_failed` を持たせ、`IOError` と `try_io` を eml で書く (Task 4)。`IO` を yield しうるラベルとして数え (Task 5)、`IO` をいつも Core IR のエフェクトの表に入れ (Task 6)、`Effectful` の extern の呼び出しを EP の yield の地点にする (Task 7)。VM は、捕まえる `Fault` を `io_failed` の節の入口の呼び出しに変える (Task 8)。`OpRet` を `eml_extern` に移し (Task 9)、戻らない extern `exit` を IR、VM、CLI に通す (Task 10)。REPL は `exit` の後も続ける (Task 11)。最後に段の終わりの文書と計測を直す (Task 12)。

**Tech Stack:** Rust (edition 2024、stable 1.98.1)、Verus と vstd (`eml_rt`)、Miri (nightly)、`insta`。新しい依存はない。

**Spec:** `docs/superpowers/specs/2026-10-16-s12b-try-io-exit-design.md`

この計画は、計画を書くときに調べた main (`c121f392` の次のコミット) のコードに基づく。プロトタイプは組んでいない。スナップショットの期待値は、各タスクで `cargo insta review` で中身を確かめてから受け入れる。

## spec からの変更

1. **`exit` の中断を始める専用のメソッド `Ctx::exit_abort` を足す。** spec は、VM がダミーの物体を宛先にして `yield_abort` を直接呼ぶとし、前提が満たせなければ専用のメソッドを足すとしていた。`yield_abort` の前提 (`holds_args`) がダミーで満たせることの証明を1か所に閉じ込め、VM が安全の議論をしなくて済むように、はじめから `exit_abort` を足す
2. **VM の結果の型 `eml_vm::Finished`。** spec は「成功の結果に `exit` の終了コード (`Option<i64>`) を足す」としていた。`RunStats` は仕事の回数の表なので、そこには入れず、`Finished { stats: RunStats, exit: Option<i64> }` を返す。`run_shown` は `(Finished, Option<String>)` を返し、文字列は `exit` したときだけ `None` である。`eml_test_support` の `run_stats` と `execute` は今の型のまま (`.stats` を取り出す) にする
3. **`IOErrorKind` のタグの結びつけ。** spec は `eml_vm` の `tags.rs` で Prelude の宣言の順と確かめるとしていた。`Ordering` と同じ形にそろえ、`eml_lir` にも定数を置き、`tags.rs` で `eml_rt` と `eml_lir` を、`eml_core_ir` の `tests/externs.rs` で `eml_lir` と Prelude の宣言の順を結ぶ
4. **VM は IO の番号を `CodeBase` の欄に持たない。** `CodeBase` の `index.effects` (名前から実行時の番号) を `Prelude.IO` の名前で引く。REPL で入力を足しても名前で引くので、付け替えが要らない

## 既存のテストの変更

spec の「テストの変更」のとおりである。コミットの本文に、期待値の変更はその理由を、機械的な追随は「mechanical」と書く。

- 成否の変更 (Task 3): `crates/eml_syntax/tests/declarations.rs` の `extern_on_other_items_is_one_error_each` の `"extern effect E where\n  op : Unit -> Unit"` は、誤りなしになる
- 成否の変更 (Task 4): `crates/eml_hir/tests/externs.rs` の `every_extern_effect_is_declared_once_in_std` の `assert!(def.operations.is_empty(), …)` を消す
- 期待値の変更 (Task 3): 同じテストの `extern effect E a` の文言
- 期待値の変更 (Task 5): `IO` が yield しうるようになったことによる `eml_types/tests/yields.rs` (`a_handleable_label_always_yields_and_io_never_does` を `every_label_always_yields` に改め、`an_operation_row_variable_in_a_clause_always_yields` と `an_unresolved_row_variable_never_yields` の `main` の行)、Core IR と LIR のダンプ、`eml_vm/tests/bench.rs` の `RunStats`
- 期待値の変更 (Task 6): エフェクトの表を書くダンプに `effect Prelude.IO { never io_failed(enum, obj) -> never }` の行が足され、利用者のエフェクトの番号がずれる
- 期待値の変更 (Task 7): IO の extern の後の yield の確認によるダンプと `RunStats`
- 期待値の変更 (Task 10): `crates/eml_core_ir/tests/externs.rs` の2つのテストの比べ方 (型変数と `ret_kind` を、`ret` が `Never` の行では見ない)
- 機械的な追随 (Task 9): `OpRet` の置き場所。(Task 10): `Machine::run`、`run_shown`、`eml_vm::execute`、`eml_cli::execute` の結果の型を使う呼び出し側

## Global Constraints

- 作業はブランチ `s12b-try-io-exit` (main から) で行う。コミットの末尾に次の2行を付ける:
  `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`
  `Claude-Session: https://claude.ai/code/session_016u71woFNQDfWtVwfuTQUUd`
- 各タスクの終わりに `cargo test --workspace` (文書の引用の検査を含む)、`cargo clippy --all-targets` (警告 0)、`cargo fmt --check` を通す
- `crates/eml_cli` か `crates/eml_test_support` を変えたタスクは、`cargo clippy -p eml_cli --no-default-features --features types`、`--features core`、`cargo clippy -p eml_test_support --no-default-features --features hir` も通す
- `crates/eml_rt` を変えたタスク (Task 2) は、`cargo verus verify -p eml_rt` (0 errors)、`verusfmt --verus-only --check $(git ls-files 'crates/eml_rt/src/*.rs')`、Miri の2つの設定 (`nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_rt trusted::` と、`MIRIFLAGS="-Zmiri-strict-provenance -Zmiri-tree-borrows"` の同じコマンド) も通す。`verus!` の中を書き換えたら `verusfmt --verus-only <file>` で整えてから `cargo fmt` を流す
- `crates/eml_vm` を変えたタスク (Task 8、9、10) は、Miri の VM のテストを同じ2つの設定で通す (`nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_vm --test integration miri::` と、`-Zmiri-tree-borrows` を足した設定)
- `git diff` は `--no-ext-diff` を付ける (difftastic が外部の diff に設定されている)
- テストの変更は上の「既存のテストの変更」の種類だけにする。ほかの成否の変更が要ると分かったら、作業を止めて spec の「テストの変更」に足してから進める
- `eml_rt` には `verus!` の外の `unsafe` を足さない。`verus!` の外に置ける安全な部分の一覧 (`docs/spec/object-model.md` の「`unsafe` の核」) は変えない。信頼の一覧 (T0〜T11) は変えない。`eml_cli` の `src/` に `unsafe` を足さない
- コードのコメントと `docs/` は日本語で書き、`yomiyasu:yomiyasu` の規則に従う。コメントは理由を書き、規則は `docs/…/x.md の「見出し」` の形で引く。引用の検査は見出しがあることを確かめるので、引く見出しは Task 1 で足す。`CLAUDE.md` は英語である
- 文言は spec のとおりにする: REPL の `exited with code {n}`、UI テストのスナップショットの `--- exit: {n} ---`、構文の誤り ``` `extern effect` cannot have parameters or `=` ```。`message` は今の実行時エラーの文言と1文字も違わない (`Fault` の `Display`)
- 数は spec のとおりにする: `IOErrorKind` のタグは `NotFound` 0、`PermissionDenied` 1、`NotAvailable` 2、`NotUtf8` 3、`Other` 4。`eml run` の終了コードは `n & 255`。`io_failed` は `IO` の操作の 0 番である

## Review Focus

spec が求めるのに、どのタスクのテストも直接は通らないおそれが高い入力と、利用者が期待する振る舞いである。各行のテストを、持ち主のタスクに足してある。

1. `Fs.read_all` が返す組の `File` を持ったまま、後の IO が失敗する: その `File` も1回だけ閉じる (Task 8 の `try_io_releases_files` の `held`)
2. 結果が `int` の extern (`Proc.run`) が失敗する: 行き先に入った `obj` のダミーが `box` に届かず、検査モードでも誤りにならない (Task 8 の `try_io_missing_program`)
3. handler の節の中の IO の失敗: handle の本体の中の `try_io` でなく、外側の `try_io` が捕まえる (Task 8 の `try_io_clause_outside`)
4. 状態に `File` を持つ利用者の handler と `try_io` をまたぐ `exit`: どの `File` も閉じ、`try_io` も `return` の節も動かない (Task 10 の `exit_releases_files`)
5. REPL の `let` の入力が `exit` する: 束縛を作らず、次の入力がその名前を引けば E1001 になる (Task 11 の `exit.repl`)

---

### Task 1: 規範の文書

**Files:**
- Modify: `docs/spec/stdlib.md`、`docs/spec/effects.md`、`docs/spec/types.md`、`docs/spec/declarations.md`、`docs/spec/modules.md`、`docs/spec/core-ir.md`、`docs/spec/lir.md`、`docs/spec/vm.md`、`docs/spec/object-model.md`、`docs/spec/repl.md`、`docs/spec/grammar.md`、`docs/implementation/testing.md`

**Interfaces:**
- Produces: 後のタスクのコメントが引く見出し。`docs/spec/stdlib.md` の「`try_io`」と「`exit`」、`docs/spec/core-ir.md` の「IO の extern」、`docs/spec/lir.md` の「戻らない extern」、`docs/spec/vm.md` の「IO の失敗と `exit`」

規範の文書を、実装より先に spec の決定に合わせる。`docs/` が正なので、後のタスクはここを引く。`status.md`、`architecture.md`、`benchmarks.md`、ロードマップ、`CLAUDE.md` は Task 12 で直す。

- [ ] **Step 1: `invoke yomiyasu:yomiyasu`**

日本語を書く前に、`yomiyasu:yomiyasu` のスキルを読み込む。

- [ ] **Step 2: `stdlib.md` に2つの節を足す**

`## \`Prelude\`` の節の後ろに `## \`try_io\`` と `## \`exit\`` を足す。

- 「`try_io`」には、spec の「Prelude に足すもの」のうち `IOErrorKind`、`IOError`、`try_io` の宣言 (コードの塊) と、`IOError` の `show` と `display` の説明、spec の「`try_io`」の箇条 (いちばん内側が捕まえること、`wrap` の例、節の中の失敗、捕まえないもの)、`Fault` と `kind` の表、書き出しの失敗を捕まえない理由の段落を移す。表の「起こす extern」の列は利用者の関数の名前にする
- 「`exit`」には、`pub extern exit : Int -> <IO> a` と、spec の「`exit`」の箇条 (終了コード `n & 255`、捕まえないこと、書き出しの失敗、`par`、1・2・130 の重なり) と、REPL では入力を終えて続けることを1行で書き、詳しくは [REPL の端末](repl.md) の「割り込みと実行時エラー」を引く
- 「実行時エラー」の節の最初の段落の後に、「I/O の失敗の一部は `try_io` で捕まえられる (上の「`try_io`」)。捕まえなかった失敗は、この表の文言の実行時エラーになる」と足す

- [ ] **Step 3: `effects.md` の「組み込みの `IO`」と「健全性」を書き直す**

- 「`IO` は、操作を持たない extern のエフェクト」の項を、「`IO` は、Prelude が `pub extern effect IO where never io_failed : IOErrorKind -> String -> a` と宣言する extern のエフェクトである。操作 `io_failed` は Prelude の外から見えず、Prelude の `try_io` だけが handle する ([標準ライブラリ](stdlib.md) の「`try_io`」)」に替える
- 「extern の関数は handler を通らず、その場で実行して値を返す。eml のコードを呼び返さず、継続も切り離さないので、中断は起こらない」を、「extern の関数は handler を通らず、その場で実行して値を返す。ただし、`try_io` が捕まえる種類の誤りで失敗すると、VM が `io_failed` の節の入口を呼んで中断を始める。`exit` はいつも中断を始める ([VM](vm.md) の「IO の失敗と `exit`」)。そのため、`IO` を起こす extern の呼び出しは yield の地点で ([Core IR](core-ir.md) の「IO の extern」)、`IO` は yield しうるラベルである」に替える
- 「extern のエフェクトは、型検査では普通のエフェクトと次の点が違う」の段落の後に、「重なりを1つにまとめ、`mask` に入れなくてよいのは、`IO` の操作が `never` の `io_failed` だけで、捕まえるのが例外と同じく実行の時点で最も近い `try_io` だからである。どちらの `IO` に届いても、捕まえる `try_io` は同じになる」と足す
- 「健全性」の最後の項 (「extern のエフェクト (`IO`): handler が存在せず、…」) を、「extern のエフェクト (`IO`): handler は Prelude の `try_io` だけで、`io_failed` は外から名指せない。`IO` の失敗は row の上の位置でなく、実行の時点で最も近い `try_io` に届くと定める。そのため、row の中の `IO` の数は、どの handler に届くかに関わらない。extern のエフェクトは `mask` に入れず、row の中で重なったら1つにまとめる」に替える

- [ ] **Step 4: `types.md` を直す**

- 「Kind」の「extern のエフェクト (`IO`) は、操作を持たないラベルだけのエフェクトである。…」の項を、「extern のエフェクト (`IO`) の多重度は `Once` と決める。`IO` の操作は `never` の `io_failed` だけなので、操作から計算すると `Never` になるが、extern の関数がその場で1回だけ戻るので `Once` として扱う」に替える
- 「推論」の「row を展開するとき、handle できないラベルの2つ目以降を落とす。…」の項の理由の部分 (「extern のエフェクトには handler がなく、…意味は同じである。同じ理由で、」) を、「`IO` の失敗は実行の時点で最も近い `try_io` に届くので、どちらの `IO` に届いても意味は同じである ([エフェクトと handler](effects.md) の「健全性」)。同じ理由で、」に替える
- 「row の具体化の記録」の `RowYield` の3項を、`Always`: 「ラベルを1つ以上含む。`IO` も含む。`IO` を起こす extern は失敗すると `try_io` まで中断するためである。または、末尾が…(今のまま)」、`Never`: 「ラベルを含まず、…(今のまま)」、`Var(i)`: 「ラベルを含まず、…(今のまま)」に替える

- [ ] **Step 5: `declarations.md`、`modules.md`、`grammar.md` を直す**

- `declarations.md` の「`where` のない型引数なしの `effect` (`pub extern effect IO`)。操作を持たない、ラベルだけのエフェクトである」を、「型引数のない `effect`。`where` と操作の並びを書ける (`pub extern effect IO where …`)。型引数と `=` は書けない (E0011、``` `extern effect` cannot have parameters or `=` ```)」に替え、例のコード塊の `pub extern effect IO` を `pub extern effect IO where\n  never io_failed : IOErrorKind -> String -> a` にする
- `modules.md` の「標準ライブラリ」の、標準ライブラリのモジュールの `pub` でない item の項の後に、「extern のエフェクトの操作 (Prelude の `io_failed`) は、`pub` のエフェクトでも、宣言したモジュールの外から見えない。修飾しても修飾しなくても E1001 である。handler の節の先頭に書いても同じである」と足す
- `grammar.md` の `extern_decl` の `'effect' UIDENT` を `'effect' UIDENT ('where' op_block)?` にし (`op_block` は `effect` の宣言と同じ規則の名前に合わせる)、110 行目付近の説明も同じにする

- [ ] **Step 6: `core-ir.md` に「IO の extern」を足す**

「evidence passing」の「`never` の中断」の後ろに `#### IO の extern` を足し、次を書く。

```markdown
#### IO の extern

- `IO` を起こす extern (表の行の `purity` が `Effectful`) の呼び出しは、`yields` の関数の中では、どれも yield の地点である。`try_io` が捕まえる種類の誤りで失敗すると、VM が `io_failed` の節の入口を呼んで中断を始め、`exit` はいつも中断を始めるためである ([VM](vm.md) の「IO の失敗と `exit`」)。直後に yield の確認と遅い道を置く形は、ほかの yield の地点と同じである
- extern は末尾呼び出しにならないので、末尾の形の例外はない。「yield の確認のない呼び出しの後」の不変条件は、extern には関わらない
- `IO` を row に持つ関数はいつも `yields` なので ([型と Kind](types.md) の「row の具体化の記録」)、`yields` でない関数が `IO` の extern を呼ぶことはない。EP はそれを内部の誤りとしてパニックする
- すぐに再開する節の条件 (「`perform` と節の3つの形」) では、`IO` の extern の呼び出しも yield の地点として数える。in-place の節は `perform` の場所の `evv` で動くので、そうしないと、handle の本体の中の `try_io` が節の IO の失敗を捕まえてしまう
- extern の関数を包む関数 (`$externN`) は、行が `Effectful` なら `yields` である
- `IO` は、Prelude の最初のエフェクトとして、いつもエフェクトの表にある。番号は 0 である。`try_io` の handle、`io_failed` の節、`prompt$` は、普通の `never` の操作の handle と同じ形に変換する
```

同じ文書の「`perform` と節の3つの形」の「すぐに再開する節」の箇条の「`yields` の関数への末尾でない直接の呼び出し、印の立った末尾でない `apply`」の後に「、`IO` の extern の呼び出し (下の「IO の extern」)」を足す。

- [ ] **Step 7: `lir.md` に「戻らない extern」を足す**

「`never` の操作」の後ろに `#### 戻らない extern` を足す。

```markdown
#### 戻らない extern

- 表の行の `ret` が `Never` の extern (`exit`) は値を返さない。結果の束縛は位置でなく、Repr と Kind の印は translate が具体化した結果の型から決める。`never` の操作の `clause` の束縛と同じ扱いで、box の挿入は受け直さず、使う所にも変換を入れない。T3 は見ない。verifier は結果を行と比べず (R7 の結果の項と、Kind の印の比較)、互換の位置での束縛の使いも比べない
- 行の `ret` と `OpRet` は、操作の表と同じ型 (`eml_extern::OpRet`) である
```

「実行時エラー」の最初の段落の後に、「`try_io` の中で起きた I/O の失敗の一部は、実行時エラーにせず `try_io` に届ける ([VM](vm.md) の「IO の失敗と `exit`」)」と足す。「ダミーの値」の約束の箇条の後に、「`IO` の extern が失敗して `io_failed` の節を呼んだときは、extern の行き先が `obj` のダミーを受ける。行き先の Repr が `int`、`enum`、`unit` でも同じである。直後に yield の確認があるので、この値は遅い道の `drop` と `decref` にしか届かない」と足す。

- [ ] **Step 8: `vm.md` に「IO の失敗と `exit`」を足す**

「誤り、止めること、終わり」の前に `## IO の失敗と \`exit\`` を足し、spec の「VM」の「IO の失敗」と「`exit`」の箇条を移す。`exit` の中断は `Ctx::exit_abort` で始め、`Finish` では「κ を取り出して手放し、`yield_finish`、`evv` を空に戻す」の順であることを書く。「誤り、止めること、終わり」の「`Finish` に着いたら」の手順の前に「`exit` で終わったときは、先に上の「IO の失敗と `exit`」の手順で中断を片付ける」と足し、`Machine::run` などが `Finished { stats, exit }` を返すことを「`RunStats`」の節の最初に書く。

- [ ] **Step 9: `object-model.md`、`repl.md`、`testing.md` を直す**

- `object-model.md` の「実行文脈 `Ctx`」に、「`ev_find(effect)` は `ev_lookup` と同じく `evv` をたどり、見つからなければ `None` を返す。`evidence_steps` は検査モードでたどった節点ごとに数える。`exit_abort` は、宛先を不死のダミーの物体、節と κ を `()` にした中断を始める。`fresh_string` は文言から文字列を作る」と足す。「誤り」の `Fault` の表の後に、「`Fault::io_error_kind` は、`try_io` が捕まえる誤り (`Io`、`Stdin`、`NotUtf8`) に `IOErrorKind` のタグ (`IO_NOT_FOUND` 0 から `IO_OTHER` 4) を返し、ほかには `None` を返す ([標準ライブラリ](stdlib.md) の「`try_io`」)」と足し、「I/O の誤りを文字列でなく値の種類で分けるのは、S12 の `try_io` が…」の項を「I/O の誤りを文字列でなく値の種類で分けるのは、`try_io` が I/O の誤りを eml の `IOError` の値に変えるためである」に替える
- `repl.md` の「割り込みと実行時エラー」に、spec の「REPL」の箇条 (`exited with code n`、受け入れないこと、`let x : Int = exit 3`、式の入力は表示しないこと、回復を呼ばないこと) を移す
- `testing.md` の「UI テスト」に、「`run` のテストは `exit` で終わってもよい。`exit` で終わったら、スナップショットの最後に `--- exit: {n} ---` を足す。`n` は `& 255` をしない値である。速いモードでも同じ終了コードであることを確かめる」と足し、「REPL のトランスクリプト」に「`exit` した入力は `exited with code {n}` と書く」と足す

- [ ] **Step 10: 引用の検査とコミット**

Run: `cargo test -p eml_cli --test integration citations` (引用の検査のテストの名前が違えば `cargo test --workspace citations`)
Expected: PASS

```bash
git add docs/spec docs/implementation/testing.md
git commit -m "Write the S12b rules into the normative docs" -m "try_io, exit, the IO extern yield points, externs that never return, and the VM's handling of IO failures, ahead of the code that cites them."
```

---

### Task 2: `eml_rt` の `ev_find`、`exit_abort`、`IOErrorKind` のタグ

**Files:**
- Modify: `crates/eml_rt/src/heap/ctx.rs` (`ev_lookup` の後に `ev_find`、`yield_abort` の後に `exit_abort`)
- Modify: `crates/eml_rt/src/heap/ctx/script.rs:71` (`fresh_string` を `pub` に)
- Modify: `crates/eml_rt/src/externs.rs:17-29` (タグの定数)、`Fault` の `impl` (`io_error_kind`)
- Modify: `crates/eml_lir/src/lib.rs:682-692` (タグの定数)
- Test: `crates/eml_rt/src/heap/tests/ctx.rs`、`crates/eml_rt/src/heap/tests/externs.rs`、`crates/eml_vm/tests/tags.rs`

**Interfaces:**
- Produces:
  - `pub unsafe fn ev_find(&mut self, effect: u16) -> Result<Option<Word>, HeapError>` (見つかれば `dup` した evidence)
  - `pub unsafe fn exit_abort(&mut self) -> Result<(), HeapError>` (yield していないこと。検査モードは `YieldMisuse`)
  - `pub fn fresh_string(&mut self, text: &str) -> Word`
  - `impl Fault { pub fn io_error_kind(&self) -> Option<i64> }`
  - `eml_rt::externs::{IO_NOT_FOUND, IO_PERMISSION_DENIED, IO_NOT_AVAILABLE, IO_NOT_UTF8, IO_OTHER}: i64`、`eml_lir::{IO_NOT_FOUND, …, IO_OTHER}: u32`

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_rt/src/heap/tests/ctx.rs` に2つのテストを足し、ファイル末尾の `both_modes!(…)` の並びに `find_returns_the_innermost_evidence_or_none` と `exit_abort_targets_the_dummy` を足す。

```rust
fn find_returns_the_innermost_evidence_or_none<M: Mode>() {
    let mut ctx = ctx::<M>();
    unsafe {
        assert_eq!(ctx.ev_find(0).unwrap(), None);
    }
    let e0 = push(&mut ctx, 0, 1);
    let e1 = push(&mut ctx, 1, 1);
    let e0_inner = push(&mut ctx, 0, 1);
    unsafe {
        let found = ctx.ev_find(0).unwrap().unwrap();
        assert_eq!(found, e0_inner);
        ctx.heap_mut().decref(found).unwrap();
        let found = ctx.ev_find(1).unwrap().unwrap();
        assert_eq!(found, e1);
        ctx.heap_mut().decref(found).unwrap();
        assert_eq!(ctx.ev_find(2).unwrap(), None);
        for e in [e0, e1, e0_inner] {
            ctx.heap_mut().decref(e).unwrap();
        }
    }
    clear(&mut ctx);
    assert!(ctx.check_finished());
}

fn exit_abort_targets_the_dummy<M: Mode>() {
    let mut ctx = ctx::<M>();
    let dummy = ctx.heap().dummy();
    unsafe {
        ctx.exit_abort().unwrap();
        assert_eq!(ctx.yield_status(dummy), 2);
        assert_eq!(ctx.yield_cont_take().unwrap(), Word::UNIT);
        assert_eq!(ctx.yield_finish().unwrap(), Word::UNIT);
    }
    assert!(ctx.check_finished());
}
```

`crates/eml_rt/src/heap/tests/externs.rs` に足す。

```rust
#[test]
fn io_faults_map_to_their_kinds() {
    use crate::externs::{
        ArithFault, Fault, IO_NOT_AVAILABLE, IO_NOT_FOUND, IO_NOT_UTF8, IO_OTHER,
        IO_PERMISSION_DENIED, IoOp, IoReason, Utf8Source,
    };
    let io = |reason| Fault::Io { op: IoOp::Open, name: "x".into(), reason };
    assert_eq!(io(IoReason::NotFound).io_error_kind(), Some(IO_NOT_FOUND));
    assert_eq!(io(IoReason::PermissionDenied).io_error_kind(), Some(IO_PERMISSION_DENIED));
    assert_eq!(io(IoReason::NotAvailable).io_error_kind(), Some(IO_NOT_AVAILABLE));
    assert_eq!(io(IoReason::Other).io_error_kind(), Some(IO_OTHER));
    assert_eq!(Fault::Stdin(IoReason::NotAvailable).io_error_kind(), Some(IO_NOT_AVAILABLE));
    assert_eq!(Fault::NotUtf8(Box::new(Utf8Source::Stdin)).io_error_kind(), Some(IO_NOT_UTF8));
    assert_eq!(Fault::Output("broken pipe".into()).io_error_kind(), None);
    assert_eq!(Fault::Arith(ArithFault::DivisionByZero).io_error_kind(), None);
}
```

`crates/eml_vm/tests/tags.rs` の `the_tag_constants_agree` の最後に足し、先頭の `//!` の説明に `IOErrorKind` を足す。

```rust
    assert_eq!(externs::IO_NOT_FOUND, i64::from(eml_lir::IO_NOT_FOUND));
    assert_eq!(externs::IO_PERMISSION_DENIED, i64::from(eml_lir::IO_PERMISSION_DENIED));
    assert_eq!(externs::IO_NOT_AVAILABLE, i64::from(eml_lir::IO_NOT_AVAILABLE));
    assert_eq!(externs::IO_NOT_UTF8, i64::from(eml_lir::IO_NOT_UTF8));
    assert_eq!(externs::IO_OTHER, i64::from(eml_lir::IO_OTHER));
```

- [ ] **Step 2: 失敗を確かめる**

Run: `cargo test -p eml_rt find_returns 2>&1 | tail -5`
Expected: コンパイルの誤り (`no method named ev_find`)

- [ ] **Step 3: タグの定数を足す**

`crates/eml_rt/src/externs.rs` の `TUPLE` の後 (`verus!` の中) に足す。

```rust
/// `IOErrorKind` のタグ。Prelude の宣言の順で、`try_io` の `io_failed` の節が受け取る
/// (docs/spec/stdlib.md の「`try_io`」)。
pub const IO_NOT_FOUND: i64 = 0;

pub const IO_PERMISSION_DENIED: i64 = 1;

pub const IO_NOT_AVAILABLE: i64 = 2;

pub const IO_NOT_UTF8: i64 = 3;

pub const IO_OTHER: i64 = 4;
```

`crates/eml_lir/src/lib.rs` の `TUPLE` の後に、同じ名前で `u32` の定数を足す。コメントは「`IOErrorKind` のタグ。`eml_rt::externs` と同じ値で、Prelude の宣言の順である (`eml_core_ir` の `tests/externs.rs` が確かめる)」とする。

- [ ] **Step 4: `io_error_kind` を足す**

`crates/eml_rt/src/externs.rs` の `verus!` の中、`Fault` の定義の後に足す。

```rust
impl Fault {
    /// `try_io` が捕まえる誤りなら、`IOErrorKind` のタグを返す。書き出しの誤り (`Output`) は、標準出力をまとめて
    /// 書くので見つかる場所が決まらず、捕まえない (docs/spec/stdlib.md の「`try_io`」)。
    pub fn io_error_kind(&self) -> Option<i64> {
        match self {
            Fault::Io { op: _, name: _, reason } => Some(reason_kind(*reason)),
            Fault::Stdin(reason) => Some(reason_kind(*reason)),
            Fault::NotUtf8(_) => Some(IO_NOT_UTF8),
            Fault::Heap(_) | Fault::Arith(_) | Fault::Output(_) | Fault::Index(_) | Fault::Stopped(_) => None,
        }
    }
}

fn reason_kind(reason: IoReason) -> i64 {
    match reason {
        IoReason::NotFound => IO_NOT_FOUND,
        IoReason::PermissionDenied => IO_PERMISSION_DENIED,
        IoReason::NotAvailable => IO_NOT_AVAILABLE,
        IoReason::Other => IO_OTHER,
    }
}
```

- [ ] **Step 5: `fresh_string` を公開する**

`crates/eml_rt/src/heap/ctx/script.rs:71` の `fn fresh_string` を `pub fn fresh_string` にし、doc コメントの先頭に「文言から文字列を作る。VM が `io_failed` の節に渡す文言を作るのにも使う (docs/spec/vm.md の「IO の失敗と `exit`」)。」と足す。証明は変えない。

- [ ] **Step 6: `ev_find` を足す**

`crates/eml_rt/src/heap/ctx.rs` の `ev_lookup` (2017-2193) の後に、`ev_lookup` を写して次のように変えた `ev_find` を足す。`ev_lookup` の速い道の形 (1歩で `meta` を1回比べるだけ) を保つため、`ev_lookup` を `ev_find` で書き直すことはしない。

- `requires` は `old(self).wf()` だけにする
- `ensures` は次のとおりにする

```rust
    pub unsafe fn ev_find(&mut self, effect: u16) -> (r: Result<Option<Word>, HeapError>)
        requires
            old(self).wf(),
        ensures
            final(self).wf(),
            r is Err ==> final(self).heap_spec().settled_heap() == old(
                self,
            ).heap_spec().settled_heap(),
            final(self).evv_spec() == old(self).evv_spec(),
            final(self).yield_spec() == old(self).yield_spec(),
            !M::CHECKED ==> r is Ok,
            old(self).held_ok() && old(self).lookup(effect) is Some ==> (r matches Ok(Some(e)) && {
                &&& e == old(self).lookup(effect)->0
                &&& final(self).held_ok()
                &&& final(self).holds(e)
                &&& final(self).ev_shape(e)
                &&& final(self).ev_tag(e) == effect + 1
                &&& final(self).evolved_by(*old(self), |b: usize| points(e, b), Set::empty())
            }),
            old(self).held_ok() && old(self).lookup(effect) is None ==> (r == Ok::<
                Option<Word>,
                HeapError,
            >(None) && final(self).held_ok() && final(self).heap_spec() == old(self).heap_spec()),
```

- ループの中の `if node.bits() == 1 { …; return Err(HeapError::NoHandler); }` を `if M::CHECKED` の外に出し、`return Ok(None)` にする。`steps >= self.max_evv || !node.is_ptr()` の `BadEvidence` と `evidence_steps` を数えるのは、今のとおり検査モードだけにする
- 不変条件の `!M::CHECKED ==> c && find(s.subrange(i, s.len()), tag) is Some` (2094) を `!M::CHECKED ==> c` にする。`decreases` の `s.len() - i` は、速いモードの `wf` が `held_ok` を含むので、そのまま通る
- 見つからなかったときの証明は、2131-2134 の「`node.bits_spec() == 1` なら残りは空で、`find(rest, tag) is None`」をそのまま使う
- doc コメントは次のとおりにする

```rust
    /// `ev_lookup` と同じく今の `evv` を先頭からたどり、エフェクト `effect` の最初の evidence を `dup` して返す。
    /// なければ `None` を返す。VM は、IO の失敗を届ける `try_io` があるかをこれで確かめる
    /// (docs/spec/vm.md の「IO の失敗と `exit`」)。`evidence_steps` は、見つからなくてもたどった節点を数える。
    ///
    /// # Safety
    /// `heap_mut` の前提を守っている。検査モードは、形の違う節点や上限を超える長さに `BadEvidence` を返す。
```

- [ ] **Step 7: `exit_abort` を足す**

`yield_abort` (947) の後に足す。

```rust
    /// `exit` の中断を始める。宛先は不死のダミーの物体で、どの `prompt$` の evidence とも違うので、中断は実行の底
    /// (`Finish`) まで届く。節と κ は `()` である (docs/spec/vm.md の「IO の失敗と `exit`」)。
    ///
    /// # Safety
    /// `heap_mut` の前提を守っている。yield していない (検査モードは `YieldMisuse` を返す)。
    pub unsafe fn exit_abort(&mut self) -> (r: Result<(), HeapError>)
        requires
            old(self).wf(),
            !M::CHECKED ==> !old(self).is_yielding(),
        ensures
            final(self).wf(),
            r is Err <==> M::CHECKED && old(self).is_yielding(),
            r is Ok ==> final(self).yield_spec() == (
                old(self).heap_spec().dummy_word(),
                Word::UNIT,
                Word::UNIT,
                true,
            ),
            final(self).heap_spec() == old(self).heap_spec(),
            final(self).evv_spec() == old(self).evv_spec(),
    {
        let d = self.heap.dummy();
        proof {
            lemma_unit_bits();
            // ダミーは rc が 0 の不死の物体なので、`owned` と数の条件は rc が 0 でない物体にしか効かず、`held_word`
            // も `holds_args` の数の項も満たす (heap/mod.rs の `dummy_wf`)
            self.lemma_dummy_held();
        }
        self.start_yield(d, Word::UNIT, Word::UNIT, true)
    }
```

`lemma_dummy_held` を `ctx.rs` の証明の補題の並びに足す。中身は `dummy_wf` (heap/mod.rs:356-364、`closed spec`。`ctx` は `heap` の子のモジュールなので開ける) を開いて、`self.heap_spec().word_in(d)`、`!M::CHECKED ==> d.is_ptr_spec() && self.holds_args(seq![d, Word::UNIT, Word::UNIT])` を示す。`holds_args` (719-728) の各項のうち、`held_word` は `readable` (`word_ok` と、物体なら `settled_state == Live`) と `droppable` (`owned` は `rc_of > 0` のときだけ効く) で、数の項は `rc_of(b) != 0` の物体だけに効く。`settled_state` と `core.state` の一致は、`held_ok` が含む `settled` (組み立て中の物体がないこと) から出す。

```rust
    proof fn lemma_dummy_held(&self)
        requires
            self.wf(),
        ensures
            self.heap_spec().word_in(self.heap_spec().dummy_word()),
            self.heap_spec().dummy_word().is_ptr_spec(),
            !M::CHECKED && !self.is_yielding() ==> self.holds_args(
                seq![self.heap_spec().dummy_word(), Word::UNIT, Word::UNIT],
            ),
```

`start_yield` の `requires` が `!M::CHECKED ==> !is_yielding && ev.is_ptr && holds_args(…)` なので、この補題で呼べる。`dummy_word` が `heap_spec()` から見えない形なら、`Heap::dummy` の `ensures` (`w == self.dummy_word()`) に合わせて書き方を変える。

- [ ] **Step 8: テストと検証を通す**

Run: `cargo test -p eml_rt && cargo test -p eml_vm --test integration tags`
Expected: PASS

Run: `cargo verus verify -p eml_rt`
Expected: `0 errors`

Run: `verusfmt --verus-only --check $(git ls-files 'crates/eml_rt/src/*.rs') && cargo fmt --check`
Expected: 出力なし

Run: Global Constraints の Miri の2つの設定 (`eml_rt` の `trusted::`)
Expected: PASS

- [ ] **Step 9: コミット**

```bash
git add crates/eml_rt crates/eml_lir/src/lib.rs crates/eml_vm/tests/tags.rs
git commit -m "Add ev_find, exit_abort and the IOErrorKind tags to eml_rt" -m "ev_find walks the evv like ev_lookup but may find nothing; exit_abort starts the abort that exit sends to the bottom of the run, with the immortal dummy as its target. Fault::io_error_kind tells which faults try_io catches."
```

---

### Task 3: `extern effect` の `where`

**Files:**
- Modify: `crates/eml_syntax/src/grammar/items.rs:146,163,248-269`
- Test: `crates/eml_syntax/tests/declarations.rs:440-513`

**Interfaces:**
- Produces: `extern effect E where` と操作の並び (`OP_DECL`) を持つ `EFFECT_ITEM`。`eml_hir` の `item_tree` (401-426) と `lower/effect.rs` (50-79) は操作を今のまま集める

- [ ] **Step 1: テストを変える**

`extern_on_other_items_is_one_error_each` の期待値を次のように変える (成否の変更と文言の変更)。

```
        "\"extern effect E where\\n  op : Unit -> Unit\" => []",
        "\"extern data T a\" => [\"E0011 1:15 `extern data` cannot have parameters, `=` or `where`\"]",
        "\"extern effect E a\" => [\"E0011 1:17 `extern effect` cannot have parameters or `=`\"]",
```

`extern_declarations` (440-484) の入力に `"pub extern effect IO where\n  never io_failed : Int -> a"` を足し、`cargo insta` で CST の形 (`EFFECT_ITEM` の子に `WHERE_KW` と `OP_DECL`) を確かめる。

- [ ] **Step 2: 失敗を確かめる**

Run: `cargo test -p eml_syntax --test integration declarations::extern_on_other_items_is_one_error_each`
Expected: FAIL (`extern effect E where` に E0011)

- [ ] **Step 3: 構文を変える**

`items.rs` の `extern_effect` を次にする。`reject_extern_tail` は `extern data` のために今のまま残す。

```rust
/// `extern effect` は名前の後に `where` と操作を書ける。型引数と `=` は書けない (docs/spec/declarations.md の
/// 「`extern`」)。誤って書いた型引数も名前として読む
fn extern_effect(p: &mut Parser, m: Marker) {
    p.bump(EFFECT_KW);
    expect_name(p, UIDENT);
    if p.at(LIDENT) || p.at(EQ) || p.at(PIPE) {
        p.error(
            codes::SYNTAX_ERROR,
            "`extern effect` cannot have parameters or `=`".to_string(),
            "an extern effect is the name and its operations",
        );
        while p.at(LIDENT) {
            name(p);
        }
        skip_to_sep(p, false);
    } else if p.eat(WHERE_KW) {
        if p.at(LAYOUT_OPEN) {
            block_of(p, "an operation signature", op_decl);
        } else {
            expected(p, "the operations on indented lines after `where`");
        }
    }
    m.complete(p, EFFECT_ITEM);
}
```

`p.error` の第2引数の型は今の `reject_extern_tail` に合わせる (`format!` なら `String`)。146 行目のコメントと 163 行目の help の「an `effect` without `where`」を「an `effect` without parameters」に替える。

- [ ] **Step 4: テストを通す**

Run: `cargo test -p eml_syntax && cargo insta review`
Expected: PASS。CST のスナップショットは中身を確かめて受け入れる

- [ ] **Step 5: コミット**

```bash
git add crates/eml_syntax
git commit -m "Let extern effects declare operations" -m "Pass/fail change: \`extern effect E where\` now parses (spec: the IO effect gains io_failed). Expected-value change: the message for parameters on an extern effect no longer mentions \`where\`."
```

---

### Task 4: Prelude の `io_failed`、`IOError`、`try_io`

**Files:**
- Modify: `std/Prelude.em:33-38`
- Modify: `crates/eml_hir/src/def_map.rs:495-505`、`crates/eml_hir/src/hir.rs:89-107`、`crates/eml_hir/src/lower/handler.rs:219-221`
- Test: `crates/eml_hir/tests/externs.rs:165`、`crates/eml_hir/tests/def_map.rs`、`crates/eml_core_ir/tests/externs.rs`、`tests/ui/check-fail/names/io_failed_is_hidden.em`

**Interfaces:**
- Consumes: Task 3 の構文
- Produces: Prelude の `IOErrorKind` (コンストラクタの順は `NotFound`、`PermissionDenied`、`NotAvailable`、`NotUtf8`、`Other`)、`IOError { kind, message }`、`pub try_io`、`IO` の操作 `io_failed` (0 番)

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_hir/tests/def_map.rs` の `the_entry_sees_only_public_prelude_names` (82-93) の最後に足す。

```rust
    assert_eq!(resolver.value(Plain("io_failed")), Resolved::NotFound);
    assert_eq!(resolver.operation(Plain("io_failed")), Resolved::NotFound);
    assert_eq!(resolver.value(qualified("Prelude", "io_failed")), Resolved::NotFound);
```

(`qualified` は同じファイルの 224-227 の書き方に合わせる。`resolver` の関数の名前が違えば、`Resolver::operation` (def_map.rs:1218) に合わせる)

`tests/ui/check-fail/names/io_failed_is_hidden.em` を作る。

```haskell
-- The operation of the extern effect `IO` is hidden outside the Prelude: plain, qualified and as a clause head.
f : Unit -> <IO> Int
f () = io_failed NotFound "x"

g : Unit -> <IO> Int
g () = Prelude.io_failed NotFound "x"

h : Unit -> Int
h () =
  handle 1 with
    | io_failed kind message -> 0
```

`crates/eml_core_ir/tests/externs.rs` の `the_ordering_tags_match_the_prelude_declaration` の後に足す。

```rust
/// VM は `try_io` の `io_failed` の節に `IOErrorKind` のタグを渡す。そのタグが Prelude の宣言の順と一致することを
/// 確かめる (docs/spec/stdlib.md の「`try_io`」)。
#[test]
fn the_io_error_kind_tags_match_the_prelude_declaration() {
    let lowered = eml_test_support::lower("");
    let program = &lowered.program;
    let (_, kind) = program
        .types()
        .find(|(id, def)| program.modules[id.module].name == "Prelude" && def.name == "IOErrorKind")
        .expect("the Prelude declares `IOErrorKind`");
    let eml_hir::TypeDefKind::Data { constructors } = &kind.kind else {
        panic!("`IOErrorKind` is a data type");
    };
    let names: Vec<(&str, u32)> = constructors
        .iter()
        .map(|&c| (program[c].name.as_str(), program[c].tag))
        .collect();
    assert_eq!(
        names,
        [
            ("NotFound", eml_lir::IO_NOT_FOUND),
            ("PermissionDenied", eml_lir::IO_PERMISSION_DENIED),
            ("NotAvailable", eml_lir::IO_NOT_AVAILABLE),
            ("NotUtf8", eml_lir::IO_NOT_UTF8),
            ("Other", eml_lir::IO_OTHER)
        ]
    );
}
```

`crates/eml_types/tests/` の、Prelude の関数の型を表示する今のテストの書き方 (たとえば `check` と `short_text`) で、`try_io` の型が `(Unit -> <IO | e> a) -> <IO | e> Result IOError a` と検査されることを確かめるテストを `crates/eml_types/tests/modules.rs` に足す。入口は `f : Unit -> <IO> Result IOError String\nf () = try_io (fn () -> Fs.read_text "x")` とし、診断がないことを確かめる。

```rust
#[test]
fn try_io_wraps_an_io_action() {
    let checked = eml_test_support::check(
        "f : Unit -> <IO> Result IOError String\nf () = try_io (fn () -> Fs.read_text \"x\")",
    );
    assert_eq!(eml_test_support::short_text(&checked), "");
}
```

(`short_text` の引数の形は、同じファイルの今のテストに合わせる)

- [ ] **Step 2: 失敗を確かめる**

Run: `cargo test -p eml_hir --test integration def_map::the_entry_sees_only_public_prelude_names`
Expected: FAIL (`io_failed` がまだない。`NotFound` で通る場合は、Step 3 の後に Prelude の中から見えることを Step 5 の UI テストと `cargo test -p eml_types` で確かめる)

- [ ] **Step 3: Prelude に宣言を足す**

`std/Prelude.em` の 33-38 行を次にする。`Result` の宣言 (`Err`、`Ok`) より後に `IOError` と `try_io` を置く必要はない (Prelude の中の順は自由) が、読みやすさのため `Result` の後に置く。

```haskell
-- 組み込みの `IO`。下の extern の関数がこのエフェクトを起こし、handler を通らずにその場で実行する。操作の
-- `io_failed` は Prelude の外から見えず、`try_io` だけが handle する。IO の失敗は、VM がこの操作の節を呼んで
-- `try_io` まで中断する (docs/spec/effects.md の「組み込みの `IO`」)
pub extern effect IO where
  never io_failed : IOErrorKind -> String -> a
pub extern println : String -> <IO> Unit
-- 標準エラーに1行を書く。C の stdio と同じく、標準出力は書き出さない
pub extern eprintln : String -> <IO> Unit
```

`Result` の宣言の後に足す。

```haskell
-- `try_io` が捕まえる IO の失敗の種類。VM は宣言の順のタグで渡す (docs/spec/stdlib.md の「`try_io`」)
pub data IOErrorKind =
  | NotFound
  | PermissionDenied
  | NotAvailable
  | NotUtf8
  | Other
  deriving (Eq, Show)

pub data IOError =
  | IOError { kind : IOErrorKind, message : String }

-- `show` は導出した `Show` のレコードと同じ形で、`display` は文言だけを返す。導出では `display` を上書きできない
instance Show IOError where
  show e = "IOError { kind = \{show e.kind}, message = \{show e.message} }"
  display e = e.message

-- IO の失敗を `Err` にする。捕まえるのは実行の時点で最も近い `try_io` である (docs/spec/stdlib.md の「`try_io`」)
pub try_io : (Unit -> <IO | e> a) -> <IO | e> Result IOError a
try_io action =
  handle action () with
    | io_failed kind message -> Err (IOError { kind = kind, message = message })
    | return x -> Ok x
```

- [ ] **Step 4: 操作を外から隠す**

`crates/eml_hir/src/def_map.rs:495-505` の `push` の `effect.public` を次にする。

```rust
            // extern のエフェクトの操作 (Prelude の `io_failed`) は、宣言したモジュールの外から見せない
            // (docs/spec/modules.md の「標準ライブラリ」)
            let public = effect.public && effect.extern_keyword.is_none();
```

として、`push(…, public, usable)` に渡す。

`hir.rs:89-107` の `EffectDef::operations` の doc を「宣言した順の操作。extern のエフェクトは Prelude の `IO` の `io_failed` のように操作を持てる」に、`EffectKind::Extern` の doc を「`extern effect`。extern の関数が起こすラベルで、操作は宣言したモジュールの外から見えない。ユーザーのモジュールの extern (E1033) は行を持たない」に替える。`lower/handler.rs:219-221` の `unhandleable_clause` の doc の「extern のエフェクトは操作を持たないので、」を「extern のエフェクトの操作は外から見えないので、」に替える。

`crates/eml_hir/tests/externs.rs:165` の `assert!(def.operations.is_empty(), "{}", row.name);` を消す (成否の変更)。

- [ ] **Step 5: テストを通す**

Run: `cargo test --workspace 2>&1 | grep -E "^test result|FAILED|panicked" | head -20`
Expected: すべて PASS。`check-fail` の新しいスナップショットは、3つの E1001 (`cannot find value \`io_failed\``、`cannot find value \`io_failed\` in module \`Prelude\``、`cannot find effect operation \`io_failed\``) を確かめてから `cargo insta review` で受け入れる。Prelude の型が増えたことで変わるスナップショットがあれば、中身が Prelude の名前の追加だけであることを確かめて受け入れる

- [ ] **Step 6: コミット**

```bash
git add std/Prelude.em crates/eml_hir crates/eml_core_ir/tests/externs.rs crates/eml_types/tests tests/ui/check-fail/names crates/eml_cli/tests/snapshots
git commit -m "Declare io_failed, IOError and try_io in the Prelude" -m "Pass/fail change: an extern effect may now have operations, so the HIR table test no longer requires none. io_failed is hidden outside the Prelude."
```

---

### Task 5: `IO` を yield しうるラベルにする

**Files:**
- Modify: `crates/eml_types/src/table/yields.rs:9-19`、`crates/eml_types/src/lib.rs:254-264`、`crates/eml_types/src/table/row.rs:18-20,291-294`、`crates/eml_types/src/context.rs:21-23,41-48,67-68`、`crates/eml_types/src/table/tests.rs:396-404` (コメント)
- Modify: `crates/eml_core_ir/src/translate/program.rs:363-390`
- Test: `crates/eml_types/tests/yields.rs:77-150`、Core IR、LIR、VM のスナップショット

**Interfaces:**
- Produces: `IO` を row に持つ関数とラムダと矢印が `RowYield::Always`。`Effectful` の extern の包み `$externN` は `yields`

- [ ] **Step 1: テストを変える**

`crates/eml_types/tests/yields.rs` の `a_handleable_label_always_yields_and_io_never_does` を `every_label_always_yields` に改め、`g` の期待値を `body Always` と `8:8#0 Always` にする。テストの上のコメントを「`IO` も、失敗すると `try_io` まで中断するので yield しうる」にする。

- [ ] **Step 2: 失敗を確かめる**

Run: `cargo test -p eml_types --test integration yields::every_label_always_yields`
Expected: FAIL (`body Never`)

- [ ] **Step 3: 分類を変える**

`yields.rs` の `row_yield` を次にする。

```rust
    pub fn row_yield(&self, row: &Row, own: &[RowVar]) -> RowYield {
        let row = self.resolve_row(row);
        // `IO` も、失敗すると `try_io` まで中断するので yield しうる (docs/spec/types.md の「row の具体化の記録」)
        if !row.labels.is_empty() {
            return RowYield::Always;
        }
        self.tail_yield(row.tail, own)
    }
```

`lib.rs` の `RowYield` の doc の「handle できるラベル」を「ラベル」に替える。`row.rs:18-20` と `291-294`、`context.rs:21-23` と `67-68` のコメントの理由「extern のエフェクトは handle できない」を「`IO` の失敗は実行の時点で最も近い `try_io` に届くので、どちらの `IO` に届いても同じである (docs/spec/effects.md の「健全性」)」に替える。`context.rs:41-48` のコメントを「`IO` の操作は `never` の `io_failed` だけなので、操作から求めると `Never` になってしまう。extern の関数はその場で1回だけ戻るので `Once` である (docs/spec/types.md の「Kind」)」に替える。`table/tests.rs:396-404` のコメントの「操作のない extern のエフェクト」を「extern のエフェクト」に替える。

- [ ] **Step 4: extern の包みを `yields` にする**

`crates/eml_core_ir/src/translate/program.rs` の `extern_wrapper` (363-390) の `self.simple(name, false, …)` の `false` を次にする。ファイルの先頭に `use eml_extern::Purity;` を足す。

```rust
        // `IO` を起こす extern は yield の地点なので、包む関数も yield しうる (docs/spec/core-ir.md の「IO の extern」)
        let yields = row.row().purity == Purity::Effectful;
        self.simple(
            name,
            yields,
            …
```

- [ ] **Step 5: テストを通し、スナップショットを確かめる**

Run: `cargo test --workspace 2>&1 | grep -E "^test result|FAILED" | head -30`
Expected: スナップショットの差分で FAIL する

Run: `cargo insta review`
差分が次の種類だけであることを確かめてから受け入れる。

- `eml_types/tests/yields.rs` の `main : Unit -> <IO> Unit` の行が `Never` から `Always` へ
- Core IR と LIR のダンプで、`IO` を row に持つ関数に `yields` の印が付き、それを呼ぶ所に yield の確認と遅い道と継続の関数 (`f$kN`)、`compose$` などの補助の関数が増える。row のビットを含む instance の名前が変わる。`println` を呼ぶ節が in-place でなく再開する節になる
- `eml_vm/tests/bench.rs` の `RunStats` (yield の確認は数えないので、`peak_stack` などの変化だけのはず)
- `eml_vm/tests/scaling.rs` が上限で落ちたら、作業を止めて原因を調べる (`IO` のある関数のフレームが重くなっただけなら、上限の値の変更を spec の期待値の変更の範囲として書き足してから直す)

UI テストとトランスクリプトの出力は変わらない。変われば誤りである。

- [ ] **Step 6: コミット**

```bash
git add crates/eml_types crates/eml_core_ir crates/eml_lir crates/eml_vm
git commit -m "Count IO as a label that may yield" -m "Expected-value change: functions and instances whose rows hold IO are now yields, so their dumps gain yield checks and continuation functions, and instance names with row bits change. An IO extern will abort to try_io when it fails."
```

---

### Task 6: `IO` をエフェクトの表に入れる

**Files:**
- Modify: `crates/eml_core_ir/src/translate/program.rs:558-577`
- Test: `crates/eml_core_ir/tests/translate.rs`、ダンプのスナップショット

**Interfaces:**
- Produces: Core IR と LIR の `Program.effects` の 0 番が `Prelude.IO` で、操作 `io_failed(enum, obj) -> never`。`try_io` が translate と EP を通る

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_core_ir/tests/translate.rs` に足す。

```rust
#[test]
fn try_io_handles_the_io_effect() {
    let text = "main : Unit -> <IO> Unit\nmain () =\n  match try_io (fn () -> Fs.read_text \"x\") with\n    | Ok s -> println s\n    | Err e -> println \"\\{e}\"";
    let shown = core_text(text, Pass::Translate);
    let effects: Vec<&str> = shown.lines().filter(|line| line.starts_with("effect ")).collect();
    insta::assert_debug_snapshot!(effects, @r#"
    [
        "effect Prelude.IO { never io_failed(enum, obj) -> never }",
    ]
    "#);
    assert!(shown.contains("handle Prelude.IO("), "{shown}");
}
```

(印の書き方 `enum` か `enum unr` は、`syntax.rs` の `unr` を書かない規則に従う。期待値は実行して確かめ、形が違えば `cargo insta` で中身を確かめて直す)

- [ ] **Step 2: 失敗を確かめる**

Run: `cargo test -p eml_core_ir --test integration translate::try_io_handles_the_io_effect`
Expected: FAIL (`an extern effect never reaches Core IR` のパニック)

- [ ] **Step 3: 表に入れる**

```rust
/// Core IR のエフェクトの表に入るエフェクト。`eml_hir::Program::effects` の順 (モジュールの番号の順、モジュールの中の
/// 宣言の順) に並べる。`IO` は Prelude の最初のエフェクトなので、いつも 0 番である
/// (docs/spec/core-ir.md の「IO の extern」)。
fn core_effects(hir: &HirProgram) -> impl Iterator<Item = (EffectId, &EffectDef)> {
    hir.effects()
}

/// エフェクトの番号は、`core_effects` の中の位置である。
pub(super) fn effect_index(hir: &HirProgram, effect: EffectId) -> u32 {
    core_effects(hir)
        .position(|(id, _)| id == effect)
        .expect("every effect is in the program") as u32
}
```

使わなくなった `EffectKind` の import を消す。

- [ ] **Step 4: テストを通し、スナップショットを確かめる**

Run: `cargo test --workspace 2>&1 | grep -E "^test result|FAILED" | head -30`、`cargo insta review`
差分が次だけであることを確かめて受け入れる。

- エフェクトの表を書くダンプに `effect Prelude.IO { never io_failed(enum, obj) -> never }` の行が足される
- 利用者のエフェクトの番号が1つずつずれる (`ev_lookup[…]` の即値は名前で書くので、番号が見える所だけ)
- `eml_vm/tests/codegen.rs` の逆アセンブルのエフェクトの番号

- [ ] **Step 5: コミット**

```bash
git add crates/eml_core_ir crates/eml_lir crates/eml_vm
git commit -m "Keep IO in the effect table" -m "Expected-value change: every dump that prints the effect table gains the Prelude.IO row and user effects are numbered from 1, so try_io can handle IO."
```

---

### Task 7: IO の extern を yield の地点にする

**Files:**
- Modify: `crates/eml_core_ir/src/evidence/split.rs:115-176`、`crates/eml_core_ir/src/evidence/handlers.rs:244-302`、`crates/eml_core_ir/src/evidence/mod.rs:22-35`
- Test: `crates/eml_core_ir/tests/evidence.rs`

**Interfaces:**
- Consumes: Task 5 (`IO` の関数が `yields`)
- Produces: `yields` の関数の中の `Effectful` の `Rhs::Extern` の直後に `prim yielding()` の確認と遅い道

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_core_ir/tests/evidence.rs` に足す。`function` は `tests/common/mod.rs` の、ダンプから1つの関数を取り出す関数である。

```rust
#[test]
fn an_io_extern_is_a_yield_point() {
    let shown = core_text(
        "main : Unit -> <IO> Unit\nmain () =\n  println \"a\"\n  println \"b\"",
        Pass::Evidence,
    );
    insta::assert_snapshot!(function(&shown, "main"), @"");
}
```

期待値は空にしておき、Step 4 で `cargo insta review` の中身が「1つ目の `extern Prelude.println` の直後に `prim yielding()` と `switch`、遅い道で `main$k0` を `compose$` で κ に足す」形であることを確かめて受け入れる。

- [ ] **Step 2: 失敗を確かめる**

Run: `cargo test -p eml_core_ir --test integration evidence::an_io_extern_is_a_yield_point`
Expected: FAIL (確認のない形)

- [ ] **Step 3: yield の地点を広げる**

`split.rs` の先頭に `use eml_extern::Purity;` を足し、`yield_points` のループの中身の先頭を次にする。

```rust
            let (var, yields) = match stmt {
                Stmt::Let {
                    var,
                    rhs: Rhs::Call { call, mask: _ },
                } => (
                    var,
                    match call {
                        Call::Direct(target, _) => program.function(*target).yields,
                        Call::Apply {
                            callee: _,
                            args: _,
                            yields,
                        } => *yields,
                        Call::Clause { .. } => true,
                        Call::Handle { .. } | Call::Perform { .. } | Call::Resume { .. } => false,
                    },
                ),
                // `IO` を起こす extern は、失敗すると VM が `io_failed` の節を呼んで中断を始め、`exit` はいつも中断を
                // 始める。extern は末尾呼び出しにならないので、末尾の例外はない (docs/spec/core-ir.md の「IO の extern」)
                Stmt::Let {
                    var,
                    rhs: Rhs::Extern { ext, args: _, at: _ },
                } => (var, ext.row().purity == Purity::Effectful),
                _ => continue,
            };
            if tail == Some(at) || annotations.not_yield_points.contains(&(id, at)) {
                continue;
            }
            if !yields && !annotations.forced.contains(&(id, at)) {
                continue;
            }
```

`let mask = …` 以降は今のままにする (`var` は参照なので、`live.remove(var)` と `result: *var` はそのまま通る)。

`handlers.rs` の `runs_in_place` の文のループを次にする。先頭に `use eml_extern::Purity;` を足す。

```rust
        for (at, stmt) in block.stmts.iter().enumerate() {
            let yield_point = match stmt {
                Stmt::Let {
                    var: _,
                    rhs: Rhs::Call { call, mask },
                } => {
                    !mask.is_empty()
                        || match call {
                            // 末尾の位置の `resume` は、上で `k` の再開だと確かめた
                            Call::Resume { .. } => tail != Some(at),
                            Call::Perform { .. } | Call::Clause { .. } => true,
                            Call::Handle { .. } => clause.yields,
                            Call::Direct(target, _) => program.function(*target).yields,
                            Call::Apply {
                                callee: _,
                                args: _,
                                yields,
                            } => *yields,
                        }
                }
                // in-place の節は `perform` の場所の `evv` で動くので、IO の extern を持つと、handle の本体の中の
                // `try_io` が節の失敗を捕まえてしまう (docs/spec/core-ir.md の「IO の extern」)
                Stmt::Let {
                    var: _,
                    rhs: Rhs::Extern { ext, args: _, at: _ },
                } => ext.row().purity == Purity::Effectful,
                _ => continue,
            };
            if yield_point {
                return false;
            }
        }
```

`mod.rs` の `evidence` のループを次にし、同じファイルに `calls_io_extern` を足す。分けない関数は、`yields` でない関数と in-place の入口 (`clause` の印を持つ) である。in-place の入口は `runs_in_place` が IO の extern を除くので、どちらも IO の extern を持たない。

```rust
        let lowered = program.function(function);
        if lowered.yields && lowered.clause.is_none() {
            split(program, function, &annotations, &mut helpers);
        } else {
            // `IO` を row に持つ関数はいつも `yields` で、in-place の入口は IO の extern を持たない
            // (docs/spec/core-ir.md の「IO の extern」)
            assert!(
                !calls_io_extern(lowered),
                "`{}` calls an IO extern but is not split at yield points",
                lowered.name
            );
        }
```

```rust
fn calls_io_extern(function: &CoreFn) -> bool {
    function.blocks.iter().flat_map(|block| &block.stmts).any(|stmt| {
        matches!(
            stmt,
            Stmt::Let { var: _, rhs: Rhs::Extern { ext, args: _, at: _ } }
                if ext.row().purity == Purity::Effectful
        )
    })
}
```

(`CoreFn`、`Stmt`、`Rhs` の型の名前と、関数の名前の欄 (`name`) は `split.rs` の import に合わせる)

- [ ] **Step 4: テストを通し、スナップショットを確かめる**

Run: `cargo test --workspace 2>&1 | grep -E "^test result|FAILED" | head -30`、`cargo insta review`
新しいテストの中身を Step 1 の説明どおりか確かめる。ほかの差分は、IO の extern の後の yield の確認、遅い道、継続の関数の増加だけであることを確かめて受け入れる。UI テストとトランスクリプトの出力は変わらない。

- [ ] **Step 5: コミット**

```bash
git add crates/eml_core_ir crates/eml_lir crates/eml_vm
git commit -m "Make every IO extern call a yield point" -m "Expected-value change: dumps gain a yield check after each IO extern, and a clause that calls one no longer runs in place, so a failure there aborts to the try_io outside the handle."
```

---

### Task 8: VM が IO の失敗を `try_io` に届ける

**Files:**
- Modify: `crates/eml_vm/src/vm.rs:322,488-609`
- Create: `tests/ui/run/io/` の UI テストと副ファイル、`tests/ui/run-fail/io/try_io_does_not_catch_arith.em`
- Test: 上の UI テスト

**Interfaces:**
- Consumes: Task 2 (`ev_find`、`fresh_string`、`io_error_kind`)、Task 6 (`IO` の番号と `io_failed` の 0 番)、Task 7 (yield の確認)

- [ ] **Step 1: UI テストを書く**

`tests/ui/run/io/input.txt` に `hello\n` (6 バイト) を書く。`tests/ui/run/io/not_utf8.txt` は `cp tests/ui/run-fail/files/not_utf8.txt tests/ui/run/io/not_utf8.txt`、`tests/ui/run/io/try_io_kinds.stdin` は `cp tests/ui/run-fail/std/stdin_not_utf8.stdin tests/ui/run/io/try_io_kinds.stdin` で作る。

`tests/ui/run/io/try_io_missing_file.em`:

```haskell
-- `try_io` turns a failed `Fs.read_text` into `Err`, whose kind and message can be shown; a success is `Ok`.
main : Unit -> <IO> Unit
main () =
  match try_io (fn () -> Fs.read_text "missing.txt") with
    | Ok text -> println "read \{text}"
    | Err e -> println "\{show e.kind}: \{e}"
  match try_io (fn () -> Fs.read_text "input.txt") with
    | Ok text -> println "read \{String.byte_length text} bytes"
    | Err e -> println "\{e}"
```

`tests/ui/run/io/try_io_missing_program.em`:

```haskell
-- `Proc.run` returns an `Int`; when it fails, its result slot holds the dummy object until the yield check sends the
-- abort on, and `try_io` catches it.
main : Unit -> <IO> Unit
main () =
  match try_io (fn () -> Proc.run "eml-no-such-program" []) with
    | Ok code -> println "exit \{code}"
    | Err e -> println "\{show e.kind}: \{e}"
```

`tests/ui/run/io/try_io_releases_files.em`:

```haskell
-- An IO failure under `try_io` releases the file of every frame on the way, and the file of a pair from
-- `Fs.read_all`, once each.
deep : Int -> <IO> Int
deep n =
  let f = Fs.open "input.txt"
  let r = if n == 0 then String.byte_length (Fs.read_text "missing.txt") else deep (n - 1) + 1
  Fs.close f
  r

held : Unit -> <IO> Int
held () =
  let (f, text) = Fs.read_all (Fs.open "input.txt")
  let n = String.byte_length (Fs.read_text "missing.txt")
  Fs.close f
  n + String.byte_length text

main : Unit -> <IO> Unit
main () =
  match try_io (fn () -> deep 3) with
    | Ok r -> println "ok \{r}"
    | Err e -> println "deep: \{e}"
  match try_io held with
    | Ok r -> println "ok \{r}"
    | Err e -> println "held: \{e}"
```

`tests/ui/run/io/try_io_nested.em`:

```haskell
-- With nested `try_io`, the inner one catches the failure and the outer one returns `Ok`.
inner : Unit -> <IO> String
inner () =
  match try_io (fn () -> Fs.read_text "missing.txt") with
    | Ok _ -> "inner ok"
    | Err e -> "inner caught: \{e}"

main : Unit -> <IO> Unit
main () =
  match try_io inner with
    | Ok text -> println "outer ok, \{text}"
    | Err e -> println "outer caught: \{e}"
```

`tests/ui/run/io/try_io_clause_outside.em`:

```haskell
-- A clause runs outside its handle, so the `try_io` inside the handle body does not catch an IO failure in the
-- clause; the `try_io` outside the handle does.
effect Load where
  load : String -> String

body : Unit -> <Load, IO> String
body () =
  match try_io (fn () -> load "missing.txt") with
    | Ok text -> "body ok: \{text}"
    | Err e -> "body caught: \{e}"

run : Unit -> <IO> String
run () =
  handle body () with
    | load path k -> k (Fs.read_text path)

main : Unit -> <IO> Unit
main () =
  match try_io run with
    | Ok text -> println text
    | Err e -> println "outside caught: \{e}"
```

`tests/ui/run/io/try_io_callback.em`:

```haskell
-- A function that calls its callback inside `try_io` also catches the callback's IO failures: `IO` is never masked.
attempt : (Unit -> <e> a) -> <IO | e> Result IOError a
attempt action = try_io (fn () -> action ())

main : Unit -> <IO> Unit
main () =
  match attempt (fn () -> Fs.read_text "missing.txt") with
    | Ok text -> println text
    | Err e -> println "attempt caught: \{e}"
```

`tests/ui/run/io/try_io_kinds.em`:

```haskell
-- `kind` tells apart a file and a standard input that are not UTF-8.
report : Result IOError String -> <IO> Unit
report (Ok text) = println "ok \{text}"
report (Err e) = println "\{show e.kind}: \{e}"

main : Unit -> <IO> Unit
main () =
  report (try_io (fn () -> Fs.read_text "not_utf8.txt"))
  report (try_io (fn () -> Stdin.read_all ()))
```

`tests/ui/run-fail/io/try_io_does_not_catch_arith.em`:

```haskell
-- `try_io` does not catch an arithmetic error.
main : Unit -> <IO> Unit
main () =
  match try_io (fn () -> 1 / 0) with
    | Ok n -> println "ok \{n}"
    | Err e -> println "caught: \{e}"
```

- [ ] **Step 2: 失敗を確かめる**

Run: `cargo test -p eml_cli --test integration ui::run 2>&1 | tail -20`
Expected: FAIL (`try_io_missing_file` などが `cannot open \`missing.txt\`: not found` の実行時エラーで止まる)

- [ ] **Step 3: VM で捕まえる**

`vm.rs` の今の `extern_call` を `run_extern` に改め (中身は変えない)、新しい `extern_call` と `io_failed` を足す。先頭の import に `eml_extern::{ExternEffect, Purity}` と `eml_rt::externs::Fault` を足す (今の import に合わせる)。

```rust
/// `Prelude.IO` の操作 `io_failed` の番号。Prelude の宣言の唯一の操作である (docs/spec/vm.md の「IO の失敗と `exit`」)。
const IO_FAILED: usize = 0;

fn extern_call<M: Mode>(
    code: &CodeBase<M>,
    ctx: &mut Ctx<M>,
    task: &mut Task,
    dst: Slot,
    at: u32,
) -> Result<(), VmFault> {
    match run_extern(code, ctx, task, dst, at) {
        Err(VmFault::Extern(fault)) => {
            let ext = Extern::ALL[code.operands[at as usize] as usize];
            match fault.io_error_kind() {
                Some(kind) if ext.row().purity == Purity::Effectful => {
                    io_failed(code, ctx, task, dst, kind, fault)
                }
                _ => Err(VmFault::Extern(fault)),
            }
        }
        other => other,
    }
}

/// IO の失敗を、実行の時点で最も近い `try_io` に届ける。`try_io` がなければ、今までどおり実行時エラーにする。
/// `io_failed` の節の入口は `never` の節の入口なので、中断を始めてダミーを `dst` に返し、次の yield の確認が遅い道へ
/// 進む。`dst` の Repr が `int` などでも、そのダミーは遅い道の `drop` と `decref` にしか届かない
/// (docs/spec/vm.md の「IO の失敗と `exit`」)。失敗した extern は引数をすでに消費している。
fn io_failed<M: Mode>(
    code: &CodeBase<M>,
    ctx: &mut Ctx<M>,
    task: &mut Task,
    dst: Slot,
    kind: i64,
    fault: Fault,
) -> Result<(), VmFault> {
    let Some(&io) = code.index.effects.get(ExternEffect::Io.row().name) else {
        return Err(VmFault::Extern(fault));
    };
    let Some(ev) = (unsafe { ctx.ev_find(io) })? else {
        return Err(VmFault::Extern(fault));
    };
    let clause = unsafe { ctx.ev_clause(ev, IO_FAILED) }?;
    let message = ctx.fresh_string(&fault.to_string());
    task.scratch.clear();
    task.scratch
        .extend([ev, Word::from_bits(kind as usize), message]);
    let ret = task.pc;
    apply::apply(code, ctx, task, clause, apply::Target::Frame { ret, dst }, true)?;
    ctx.poll(task.depth())?;
    Ok(())
}
```

`code.index` の欄が `pub(crate)` でなければ `pub(crate)` にする。`step` は `extern_call` を呼ぶ前に `task.pc` を次の命令にしている (vm.rs:197) ので、`ret` は extern の次の命令である。

- [ ] **Step 4: テストを通す**

Run: `cargo test -p eml_cli --test integration ui:: && cargo insta review`
Expected: スナップショットの中身を確かめて受け入れる。

- `try_io_missing_file`: ``NotFound: cannot open `missing.txt`: not found`` と `read 6 bytes`
- `try_io_missing_program`: ``NotFound: cannot run `eml-no-such-program`: not found``
- `try_io_releases_files`: ``deep: cannot open `missing.txt`: not found`` と ``held: cannot open `missing.txt`: not found`` (検査モードのリークの確かめで通ること自体が、`File` を1回ずつ閉じたことの確かめである)
- `try_io_nested`: ``outer ok, inner caught: cannot open `missing.txt`: not found``
- `try_io_clause_outside`: ``outside caught: cannot open `missing.txt`: not found``
- `try_io_callback`: ``attempt caught: cannot open `missing.txt`: not found``
- `try_io_kinds`: ``NotUtf8: `not_utf8.txt` is not valid UTF-8`` と `NotUtf8: the standard input is not valid UTF-8`
- `try_io_does_not_catch_arith`: `division by zero` と `at run-fail/io/try_io_does_not_catch_arith.em:4:…`

Run: `cargo test --workspace` と Global Constraints の VM の Miri の2つの設定
Expected: PASS

- [ ] **Step 5: コミット**

```bash
git add crates/eml_vm tests/ui crates/eml_cli/tests/snapshots
git commit -m "Send IO failures to the nearest try_io" -m "When an IO extern fails with a fault try_io catches and an IO evidence is in scope, the VM calls the io_failed clause entry, which aborts to try_io and releases what the frames held."
```

---

### Task 9: `OpRet` を `eml_extern` に移す

**Files:**
- Modify: `crates/eml_extern/src/lib.rs`、`crates/eml_lir/src/lib.rs:6-7,532-537`
- Modify: `OpRet` を使う所 (`eml_lir` の `check.rs`、`boxing.rs`、`verify.rs`、`contract.rs`、`syntax.rs`、`tests/text.rs`、`eml_core_ir` の `verify.rs`、`evidence/handlers.rs`、`translate/program.rs`) の import

**Interfaces:**
- Produces: `eml_extern::OpRet { Repr(Repr), Never }`。`eml_lir::OpRet` は再公開で今のまま使える

- [ ] **Step 1: 移す**

`crates/eml_lir/src/lib.rs:532-537` の `OpRet` の定義を `crates/eml_extern/src/lib.rs` の `Repr` の定義の後に移し、doc を「操作と extern の関数の結果。`never` の操作と戻らない extern (`exit`) は戻らないので、結果を呼び出した側の位置と比べない (docs/spec/lir.md の「戻らない extern」)」にする。`eml_lir/src/lib.rs:6` を `pub use eml_extern::{OpRet, Repr};` にする。ほかの crate の `use eml_lir::OpRet` などは、再公開でそのまま通る。

- [ ] **Step 2: テストを通す**

Run: `cargo test --workspace 2>&1 | grep -E "^test result|FAILED" | head`
Expected: PASS (期待値はバイト単位で変わらない)

- [ ] **Step 3: コミット**

```bash
git add crates/eml_extern crates/eml_lir
git commit -m "Move OpRet into eml_extern" -m "Mechanical: extern function rows will use it for exit."
```

---

### Task 10: `exit`

**Files:**
- Modify: `crates/eml_extern/src/lib.rs` (`Extern::Exit`、行の `ret: OpRet`)
- Modify: `std/Prelude.em` (`pub extern exit`)
- Modify: `crates/eml_lir/src/check.rs:744-791`、`crates/eml_lir/src/verify.rs:305-332`、`crates/eml_lir/src/boxing.rs:500-552`、`crates/eml_core_ir/src/verify.rs:268-292`
- Modify: `crates/eml_vm/src/vm.rs` (`Task`、`run_extern`、`finish`)、`crates/eml_vm/src/compile/emit.rs:753-797`、`crates/eml_vm/src/lib.rs:86-98,177-315,371-387`
- Modify: `crates/eml_cli/src/lib.rs:247-255`、`crates/eml_cli/src/main.rs:84-91`、`crates/eml_cli/src/repl.rs:342-379` (型の追随だけ)、`crates/eml_test_support/src/lib.rs:270-300`
- Modify: `crates/eml_cli/tests/ui.rs:128-273`
- Test: `crates/eml_core_ir/tests/externs.rs:139-221`、`crates/eml_lir/tests/boxing.rs`、`tests/ui/run/io/exit_code.em`、`tests/ui/run/io/exit_releases_files.em`、`crates/eml_cli/tests/cli.rs`

**Interfaces:**
- Consumes: Task 2 (`exit_abort`)、Task 7 (`Effectful` の extern の yield の地点)、Task 9 (`OpRet`)
- Produces:
  - `pub struct Finished { pub stats: RunStats, pub exit: Option<i64> }` (`eml_vm`、`eml_cli` が再公開)
  - `Machine::run(&mut self, entry: u32) -> Result<Finished, RuntimeError>`、`Machine::run_shown(&mut self, entry: u32, max_chars: usize) -> Result<(Finished, Option<String>), RuntimeError>` (`AnyMachine` も同じ)
  - `eml_vm::execute(…) -> Result<Finished, RuntimeError>`、`eml_cli::execute(…) -> Result<Finished, RuntimeError>`

- [ ] **Step 1: 失敗するテストを書く**

`tests/ui/run/io/exit_code.em`:

```haskell
-- `exit` ends the program with its code, after the output so far.
main : Unit -> <IO> Unit
main () =
  println "before"
  exit 3
  println "after"
```

`tests/ui/run/io/exit_releases_files.em`:

```haskell
-- `exit` passes through frames holding files, a `try_io` and a handler whose state is a file; everything they hold is
-- released once, and neither `try_io` nor the `return` clause runs.
effect Tick where
  tick : Unit -> Unit

deep : Int -> <Tick, IO> Int
deep n =
  let f = Fs.open "input.txt"
  tick ()
  let r = if n == 0 then exit 4 else deep (n - 1) + 1
  Fs.close f
  r

main : Unit -> <IO> Unit
main () =
  let g = Fs.open "input.txt"
  let result =
    handle try_io (fn () -> deep 2) from g with
      | tick () k st -> k () st
      | return x st ->
        Fs.close st
        x
  match result with
    | Ok n -> println "ok \{n}"
    | Err e -> println "caught: \{e}"
```

`crates/eml_cli/tests/cli.rs` に足す。

```rust
#[test]
fn exit_ends_with_its_code_modulo_256() {
    let output = eml(&["run", "--debug-heap", "run/io/exit_code.em"]);
    assert_eq!(output.status.code(), Some(3));
    assert_eq!(String::from_utf8_lossy(&output.stdout), "before\n");
    let dir = common::temp_project("cli-exit", &[("main.em", "main : Unit -> <IO> Unit\nmain () = exit 259\n")]);
    let output = eml_in(&dir, &["main.em"]);
    assert_eq!(output.status.code(), Some(3));
    std::fs::remove_dir_all(&dir).unwrap();
}
```

(`common::temp_project` の呼び方は 126-132 のテストに合わせる)

`crates/eml_lir/tests/boxing.rs` に、`ret` が `Never` の extern の束縛を受け直さないことのテストを足す。入力は手書きの LIR で、`int` に束縛した `exit` の結果を `tobj` を返す `return` に渡す。

```rust
#[test]
fn the_result_of_exit_is_not_converted() {
    let text = "fn f(n.0: int) -> tobj {\n  let t.1: int = extern Prelude.exit(n.0)\n  return t.1\n}\n";
    insta::assert_snapshot!(boxed(text), @"");
}
```

(`boxed` は同じファイルの、テキストを読んで box の挿入を流す今の補助の関数の名前に合わせる。期待値は Step 5 で、`box` が入らないことを確かめて受け入れる)

- [ ] **Step 2: 失敗を確かめる**

Run: `cargo test -p eml_cli --test integration ui::run 2>&1 | tail -5`
Expected: FAIL (`exit` がない。E1001)

- [ ] **Step 3: extern の行と Prelude**

`crates/eml_extern/src/lib.rs`:

- `FunctionRow::ret` の型を `OpRet` にし、doc を「結果の Repr。`Never` は戻らない extern で、`ret_kind` は読まない」にする
- `kinded` と `row` の `ret: Repr` を受ける所で `ret: OpRet::Repr(ret)` にする
- `Extern` の最後に `Exit` を足し、`Extern::ALL` の最後にも足す。行は次にする

```rust
            // 戻らない extern。VM が実行の底まで中断する (docs/spec/vm.md の「IO の失敗と `exit`」)
            Extern::Exit => FunctionRow {
                name: "Prelude.exit",
                params: &[Repr::Int],
                ret: OpRet::Never,
                param_kinds: &[Linearity::Unr],
                ret_kind: Linearity::Unr,
                purity: Purity::Effectful,
            },
```

`std/Prelude.em` の `eprintln` の後に足す。

```haskell
-- 途中のフレームが持っていた値を後始末して、終了コード `code` で終わる。`try_io` は捕まえない
-- (docs/spec/stdlib.md の「`exit`」)
pub extern exit : Int -> <IO> a
```

- [ ] **Step 4: verifier と box の挿入**

`check.rs` の `check_extern` の結果の比較を次にする。

```rust
        // 戻らない extern の束縛は位置でなく、translate が具体化した型の Repr と印を持つ
        // (docs/spec/lir.md の「戻らない extern」)
        let OpRet::Repr(ret) = row.ret else {
            return Ok(());
        };
        let repr = self.repr(var);
        if repr != ret {
            return Err(format!(
                "`{}` ({}) is bound to `{}`, which returns {}",
                self.name(var),
                repr.name(),
                row.name,
                ret.name()
            ));
        }
```

(引数の Repr と印の検査は、結果の比較より前に今のまま行う。結果の比較が引数の印の検査より前にある今の順なら、`let OpRet::Repr(ret) = row.ret else { … }` を、引数の印の検査の後、`bound_mark` の前に置き直し、`Never` の行では `bound_mark` を呼ばない)

`eml_lir/src/verify.rs:305-332` と `eml_core_ir/src/verify.rs:268-292` の `match rhs` の `Rhs::Extern` を、ほかの腕から分けて次にする。

```rust
                    Rhs::Extern { ext, args: _, at: _ } => {
                        self.cx.note_never(*var, ext.row().ret == OpRet::Never);
                    }
```

`boxing.rs` の extern の近道 (543-552) から `Rhs::Extern` を外して、次の腕にする。

```rust
                    // extern の引数と結果は表の行と同じ Repr で、translate の出力ですでに合っている。戻らない extern の
                    // 束縛には制御が届かないので、使う所も変換しない (docs/spec/lir.md の「戻らない extern」)
                    Rhs::Extern { ext, args: _, at: _ } => {
                        self.never[var.0 as usize] = ext.row().ret == OpRet::Never;
                        None
                    }
```

`check.rs` の `never` の欄の doc を「`never` の操作の `perform` か `clause` の束縛か、戻らない extern の束縛かどうか」にし、`boxing.rs` の `never` の欄の doc も同じく直す。

`crates/eml_core_ir/tests/externs.rs` の2つのテストを次のように変える (期待値の変更)。

- `every_extern_function_row_has_the_reprs_of_its_std_signature`: `assert!(!has_type_var(types, ty), …)` を、引数の型ごとの `assert!(!has_type_var(types, *param), …)` にし、結果は `match row.ret { OpRet::Repr(ret) => { assert!(!has_type_var(types, ty), …); assert_eq!(type_repr(types, ty, &checked.program), ret, …) } OpRet::Never => assert!(has_type_var(types, ty), "`{}` never returns, so its result is a type variable", row.name) }` にする。上のコメントを「行の Repr は引数の型と結果の型から比べる。型変数を持てるのは、戻らない extern の結果だけである (docs/spec/lir.md の「戻らない extern」)」にする
- `every_extern_function_row_has_the_kinds_of_its_std_signature`: 結果の `kind_of` の比較を、`if let OpRet::Repr(_) = row.ret { … }` の中に入れる

`only_the_file_functions_carry_a_lin` は `exit` の `ret_kind` が `Unr` なので今のまま通る。

- [ ] **Step 5: VM**

`vm.rs` の `Task` に欄を足し、`Task::new` で `exit: None` にする。

```rust
    /// `exit` の終了コード。立っていれば、中断が `Finish` まで届く (docs/spec/vm.md の「IO の失敗と `exit`」)。
    pub(crate) exit: Option<i64>,
```

`run_extern` の `match ext` に足す。

```rust
            Extern::Exit => {
                task.exit = Some(task.raw(arg(0)));
                ctx.exit_abort()?;
                let dummy = ctx.heap().dummy();
                task.set(dst, dummy);
            }
```

`emit.rs` の `arithmetic` の `None` の並び (773-796) に `Extern::Exit` を足す。

`finish` を `&mut Task` を受ける形にし、先頭に足す。

```rust
fn finish<M: Mode>(code: &CodeBase<M>, ctx: &mut Ctx<M>, task: &mut Task) -> Result<(), VmFault> {
    if task.exit.is_some() {
        // 中断の κ を手放すと、途中のフレームが持っていた値が解放の連鎖で後始末される。入口の結果はダミーなので
        // 手放さない。中断の途中の `evv` には意味がないので、空に戻す (docs/spec/vm.md の「IO の失敗と `exit`」)
        let k = ctx.yield_cont_take()?;
        unsafe {
            ctx.heap_mut().decref(k)?;
            ctx.yield_finish()?;
            ctx.evv_set(Word::UNIT)?;
        }
        return Ok(());
    }
    if code.functions[task.entry as usize].ret_rc && !task.keep_result {
        unsafe { ctx.heap_mut().decref(task.get(0)) }?;
    }
    Ok(())
}
```

`Op::Finish` の腕 (221-224) は `finish(code, ctx, task)?` のまま通る (`task` は `&mut`)。

`lib.rs`:

```rust
/// 1回の実行の結果。`exit` で終わったら、その終了コードを持つ (docs/spec/vm.md の「`RunStats`」)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Finished {
    pub stats: RunStats,
    pub exit: Option<i64>,
}
```

- `run_entry` の戻り値を `Result<(Finished, Option<String>), RuntimeError>` にする。`copied` を `match (&result, shown, task.exit) { (Ok(()), Some(max_chars), None) => Some(unsafe { ctx.copy_string(task.get(0), max_chars) }), _ => None }` にし、最後を `Ok((Finished { stats, exit: task.exit }, text))` にする
- `run_recovering`、`run` (`Result<Finished, RuntimeError>`)、`run_shown` (`Result<(Finished, Option<String>), RuntimeError>`。今の `.expect("a shown run copies its result")` を消す)、`AnyMachine::run` と `run_shown`、`execute` (`Result<Finished, RuntimeError>`) の型を合わせる
- `eml_cli/src/lib.rs` の `execute` の型を合わせ、`Finished` を `RunStats` と同じく再公開する

`main.rs:84-91`:

```rust
    match eml_cli::execute(&program, &config, script_host(PathBuf::new(), script_args)) {
        // シェルと同じく、終了コードは下の 8 ビットである (docs/spec/stdlib.md の「`exit`」)
        Ok(finished) => ExitCode::from(finished.exit.map_or(0, |code| (code & 255) as u8)),
        Err(error) => {
```

`repl.rs:342-379` は型の追随だけにする (`Exited` は Task 11): `run_shown(…).map(|(_, text)| text)`、`run(…).map(|_| None)` とし、`(Ok(Some(text)), Some(shown))` の腕は今のまま、`(Ok(_), _) => Outcome::Accepted` も今のままにする。

`eml_test_support/src/lib.rs` の `run_files`、`run_stats`、`execute` は、`eml_cli::execute(…)` の結果に `.map(|finished| finished.stats)` を付けて今の型を保つ。

`eml_vm` のテストの呼び出し側 (`tests/calls.rs`、`tests/api.rs`、`tests/machine.rs`、`tests/miri.rs`) は、`.stats` を取るか `(_, shown)` の `shown` を `Option` として扱う形に直す (機械的な追随)。

- [ ] **Step 6: UI テストの仕組み**

`crates/eml_cli/tests/ui.rs`:

- `Ran::result` を `Result<Option<i64>, RuntimeError>` にし、`execute_in` (174) を `execute(…).map(|finished| finished.exit)` にする
- `assert_fast_mode_agrees` (212) の比較を `checked.result.as_ref().copied().map_err(ToString::to_string)` にし、`fast.result.map_err(|error| error.to_string())` と比べる。終了コードも比べることになる
- `run` (231-249) の `assert_eq!(ran.result, Ok(()), …)` を `assert!(ran.result.is_ok(), …)` にし、スナップショットの文字列の最後に `exit` を足す

```rust
        let exit = match ran.result {
            Ok(Some(code)) => format!("--- exit: {code} ---\n"),
            _ => String::new(),
        };
        …
            insta::assert_snapshot!(format!("--- stdout ---\n{stdout}--- stderr ---\n{stderr}{written}{exit}"));
```

- `run_fail` の `let Err(error) = &ran.result` はそのまま通る

- [ ] **Step 7: テストを通す**

Run: `cargo test --workspace 2>&1 | grep -E "^test result|FAILED" | head -20`、`cargo insta review`
スナップショットを確かめて受け入れる。

- `exit_code`: stdout は `before` だけで、最後に `--- exit: 3 ---`
- `exit_releases_files`: stdout は空で、最後に `--- exit: 4 ---` (検査モードのリークの確かめで通ることが、`File` と handler の状態が1回ずつ閉じた確かめである)
- `the_result_of_exit_is_not_converted`: `return t.1` の前に `box` がない
- ほかのスナップショットは変わらない

Run: Global Constraints の clippy の4つ (`eml_cli` の feature を含む) と VM の Miri の2つの設定
Expected: PASS

- [ ] **Step 8: コミット**

```bash
git add -A crates std tests
git commit -m "Add exit" -m "exit is an extern that never returns: the VM aborts to the bottom of the run with the dummy as the target, and Finish releases the continuation before the run ends with the code. Expected-value change: the extern row tests allow a type variable in the result of a row that never returns. Mechanical: callers of Machine::run, run_shown and execute read Finished."
```

---

### Task 11: REPL の `exit`

**Files:**
- Modify: `crates/eml_cli/src/repl.rs:95-108,342-379`、`crates/eml_cli/src/terminal.rs:282-303`
- Modify: `crates/eml_cli/tests/repl.rs:277-291`
- Create: `crates/eml_cli/tests/repl/io/exit.repl`

**Interfaces:**
- Consumes: Task 10 (`Finished::exit`)
- Produces: `Outcome::Exited(i64)`

- [ ] **Step 1: トランスクリプトを書く**

`crates/eml_cli/tests/repl/io/exit.repl`:

```
> println "a"
> exit 3
> let x : Int = exit 4
> x
> 1 + 1
> try_io (fn () -> Fs.read_text "missing.txt")
```

- [ ] **Step 2: 失敗を確かめる**

Run: `cargo test -p eml_cli --test integration repl::transcripts 2>&1 | tail -5`
Expected: FAIL (`exit 3` の入力が受け入れられ、表示されない)

- [ ] **Step 3: `Outcome::Exited` を足す**

`repl.rs` の `Outcome` に足す。

```rust
    /// `exit` で終わった。実行時エラーと同じく受け入れず、`let` の束縛も作らない。中断で後始末が済んでいるので、
    /// 回復は要らない (docs/spec/repl.md の「割り込みと実行時エラー」)。
    Exited(i64),
```

`run` を次にする。

```rust
        let result = match &shown {
            Some(_) => self
                .machine
                .run_shown(loaded.entry, pretty::LIMIT + 1)
                .map(|(finished, text)| (finished.exit, text)),
            None => self.machine.run(loaded.entry).map(|finished| (finished.exit, None)),
        };
        match (result, shown) {
            (Ok((Some(code), _)), _) => Outcome::Exited(code),
            (Ok((None, Some(text))), Some(shown)) => {
                … (今の表示の腕)
            }
            (Ok(_), _) => Outcome::Accepted,
            (Err(error), _) if error.is_broken() => {
                self.broken = true;
                Outcome::Broken(error)
            }
            (Err(error), _) => Outcome::Failed(error),
        }
```

`terminal.rs` の `show` の `match` に `Outcome::Exited(code) => eprintln!("exited with code {code}"),` を足す。`tests/repl.rs` の `record` に `Outcome::Exited(code) => writeln!(block, "exited with code {code}").unwrap(),` を足す。

- [ ] **Step 4: テストを通す**

Run: `cargo test -p eml_cli && cargo insta review`
スナップショットを確かめて受け入れる。`println "a"` は `a`、`exit 3` は `exited with code 3`、`let x : Int = exit 4` は `exited with code 4`、`x` は `E1001 … cannot find value \`x\``、`1 + 1` は `2`、`try_io …` は `Err IOError { kind = NotFound, message = "cannot open \`missing.txt\`: not found" }`、最後は `--- close ---` と `no leaks`。速いモードも同じ出力で通る。

Run: Global Constraints の `eml_cli` の feature の clippy
Expected: 警告 0

- [ ] **Step 5: コミット**

```bash
git add crates/eml_cli
git commit -m "Keep the REPL going after exit" -m "An input that calls exit ends with 'exited with code n' and is not accepted, like a runtime error, but needs no recovery."
```

---

### Task 12: 段の終わりの文書と計測

**Files:**
- Modify: `docs/future/roadmap.md`、`docs/future/stdlib.md`、`docs/implementation/status.md`、`docs/implementation/architecture.md`、`docs/implementation/benchmarks.md`、`CLAUDE.md`

- [ ] **Step 1: `invoke yomiyasu:yomiyasu`**

- [ ] **Step 2: ロードマップ**

`docs/future/roadmap.md` の S12 の節の表の S12b の行の状態を「終えた。決めたことは [標準ライブラリ](../spec/stdlib.md) の「`try_io`」と「`exit`」、[エフェクトと handler](../spec/effects.md) の「組み込みの `IO`」と「健全性」、[型と Kind](../spec/types.md) の「row の具体化の記録」、[Core IR](../spec/core-ir.md) の「IO の extern」、[LIR](../spec/lir.md) の「戻らない extern」、[VM](../spec/vm.md) の「IO の失敗と `exit`」、[REPL の端末](../spec/repl.md) の「割り込みと実行時エラー」、[テスト戦略](../implementation/testing.md) の「UI テスト」に移した」にする。「決めたこと」の (S12b) の3項と、「論点」の (S12b) の項を消し、論点の代わりに「S12b では、標準ライブラリの関数の中の extern の誤りの位置を今のまま `<std>/…` にした。困れば S13 で利用者の呼び出しの位置を足す」と、閉じた論点の段落に足す。S12a で閉じた論点の「多相な extern の行」の後に「(S12b の `exit` は結果だけが多相で、戻らない extern として扱った)」と足す。`docs/future/stdlib.md` の `exit` の項を、[標準ライブラリ](../spec/stdlib.md) の「`exit`」を引く1行にする。

- [ ] **Step 3: 実装の文書**

`status.md` の「今の言語の範囲」に `try_io`、`IOError`、`exit` を足し、`architecture.md` の VM と EP の説明に「IO の extern は yield の地点で、VM が失敗を `io_failed` の節に届ける」「`exit` は `Finish` まで中断する」を足す。

- [ ] **Step 4: 計測**

Run: `bench/run.sh`
出力を `docs/implementation/benchmarks.md` に S12b の節として記録し、S12a との差 (IO の関数の yield の確認) を1〜2行で書く。

- [ ] **Step 5: CLAUDE.md**

英語で次を直す。

- `eml_rt` の段落: `Ctx` の evidence primitives の説明の後に "`ev_find` (an `ev_lookup` that may find nothing), `exit_abort` (starts the abort `exit` sends to the bottom of the run, with the immortal dummy as its target)" を足し、`Fault::io_error_kind` (the faults `try_io` catches: `Io`, `Stdin`, `NotUtf8`; `Output` is not caught because stdout is buffered) を足す。extern の数 (38) を 39 にする
- Built-ins の段落: "`IO` is a label-only extern effect" を "`IO` is an extern effect whose only operation, the `never` `io_failed`, is hidden outside the Prelude and handled only by the Prelude's `try_io : (Unit -> <IO | e> a) -> <IO | e> Result IOError a`; IO extern calls are EP yield points, and on a fault `try_io` catches the VM calls the `io_failed` clause entry of the nearest IO evidence (`Ctx::ev_find`), otherwise the fault is a runtime error as before. `exit : Int -> <IO> a` is the one extern whose row `ret` is `OpRet::Never` (moved to `eml_extern`); the VM aborts to `Finish`, which releases the continuation" に替える
- Types の段落: `RowYield` で `IO` も yield しうることを足す
- `eml_cli` の段落: `Machine::run` / `execute` が `Finished { stats, exit }` を返すこと、`eml run` が `exit n` で `n & 255` を返すこと、REPL の `Outcome::Exited` と `exited with code n` を足す
- Testing の段落: `run` の UI テストが `exit` で終わってよく、`--- exit: n ---` を足すことを足す

- [ ] **Step 6: 最後の確かめとコミット**

Run: `cargo test --workspace`、`cargo clippy --all-targets`、Global Constraints の feature の clippy、`cargo fmt --check`、`cargo verus verify -p eml_rt`、Miri の4つ (`eml_rt` と `eml_vm` の2つの設定)
Expected: すべて PASS、警告 0

```bash
git add docs CLAUDE.md
git commit -m "Record S12b in the roadmap, CLAUDE.md and benchmarks"
```
