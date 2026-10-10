# S7 evidence passing 実装計画

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** エフェクトの操作と handler を、Core IR から Core IR への evidence passing (EP) の変換で実装し、CEK の handler フレームと継続の区間をなくす。

**Architecture:** 下の層から積む。Task 1〜3 で型検査と translate に「yield しうるか」の情報 (row のビット、`yields` の印、`apply` の印と分け方、操作の Repr) を通す。Task 4〜6 で EP の出力が使う命令 (基本操作 `prim`、節の呼び出し `clause`) を、verifier、後段のパス、ランタイムに通す。Task 7〜8 で EP のパスを作り、一時的な差分テストで全 UI テストの出力が一致するまで進める。Task 9 で EP を常に通して新しい指標を入れ、Task 10 でエフェクトを直接扱う仕組みを消し、Task 11 で文書を移す。

**Tech Stack:** Rust (edition 2024)、la-arena、insta、eml の UI テスト、`bench/run.sh`。

**Spec:** `docs/superpowers/specs/2026-10-11-s7-evidence-passing-design.md` (main の ce7ab4f)。計画も Task 1 の前にコミットする。実装する人は、各タスクの前に spec の該当する節を読む。spec が規範で、計画とずれたら spec に従い、ずれを報告する。

| 見出し | 中身 | 既存のテストの期待値 |
|---|---|---|
| Task 1 | 型検査が row の yield の分類 (`RowYield`) を記録する | 変わらない |
| Task 2 | Core IR の `yields` の印、`Call::Apply` の印、操作の表の Repr とテキストの形 | spec の「期待値の変更」のうち、`effect` の宣言の綴り (手書きの IR の入力は機械的な追随、`an_operation_needs_its_arity_after_the_last_slash` の文言とダンプは期待値の変更) |
| Task 3 | translate の row のビット、instance の名前、`yields` の印、`apply` の印と分け方 | spec の「期待値の変更」のうち、`<e>` の名前、`yields` の印、`apply` の分け方を含む translate 以降のダンプ |
| Task 4 | `Rhs::Prim`、`Call::Clause`、`clause` の印、テキストの形、`verify_evidence` と境界の検査 | 変わらない |
| Task 5 | box の挿入、T3、縮約の `prim` と `clause` の規則、`clause` の印の関数の ABI | 変わらない |
| Task 6 | ランタイムの evidence と `evv` の物体、ダミー、CEK の基本操作 | 変わらない |
| Task 7 | EP のパスの骨組み、補助の関数、yield の地点と継続の関数 | 変わらない |
| Task 8 | handle、`perform`、節の3つの形、`resume`、`mask` の変換。一時的な差分テストと新しい UI テスト | 変わらない |
| Task 9 | EP を常に通す。`RunStats` の新しい指標と scaling テスト | spec の「期待値の変更」のうち、translate より後の段のダンプ、`bench.rs`、`handler_visits` を使う `scaling.rs` のテスト |
| Task 10 | エフェクトを直接扱う仕組みと一時的な切り替えを消す | spec の「成否の変更」の表のすべて |
| Task 11 | 文書、ロードマップ、コメント、ベンチマークの記録、spec と計画の削除 | 変わらない |

## Global Constraints

- 各タスクの終わりに、`cargo test` がすべて通り、`cargo clippy --all-targets` が警告を出さず、`cargo fmt --check` が差分を出さない。`cargo clippy -p eml_cli --no-default-features --features types` と `--features core` も通る。`cargo test -p eml_cli --test integration citations` も通る。途中のタスクでも、ワークスペース全体を壊したままコミットしない
- 既存のテストの期待値は、上の表の「既存のテストの期待値」の列の範囲でだけ変える。スナップショットは、変わった中身を読んでから受け入れる (`cargo insta review`)。新しいテストのスナップショットも、中身を確かめてから受け入れる
- spec の「成否の変更」に挙げていないテストを、消したり、run と check-fail の間で動かしたりしない。必要になったら作業を止めて報告する (CLAUDE.md の Testing)
- 名前 (各タスクで共通に使う。ほかのタスクの実装者はこの名前を前提にする)
  - `eml_types`: `pub enum RowYield { #[default] Never, Always, Var(u32) }`、`Instantiation::rows: Vec<RowYield>`、`BodyTypes::body_yield: RowYield`、`BodyTypes::lambda_yields: ArenaMap<ExprId, RowYield>`、`BodyTypes::arrow_yields: HashMap<(ExprId, usize), RowYield>` (Task 1)
  - `eml_core_ir`: `CoreFn::yields: bool`、`Call::Apply { callee: Atom, args: Vec<Atom>, yields: bool }`、`OperationInfo { name, params: Vec<Repr>, ret: OpRet, resumable }`、`pub enum OpRet { Repr(Repr), Never }` (Task 2)。`pub enum Prim`、`Prim::row(&self, program: &Program) -> PrimRow`、`pub struct PrimRow { pub name: String, pub params: Vec<Repr>, pub ret: Repr }`、`Rhs::Prim(Prim, Vec<Atom>)`、`Call::Clause { effect: u32, op: u32, callee: Atom, args: Vec<Atom> }`、`CoreFn::clause: Option<(u32, u32)>`、`pub fn verify_evidence` (Task 4)。`pub fn evidence(program: &mut Program)`、`Pass::Evidence` (Task 7、パイプラインへの組み込みは Task 8)
  - `eml_runtime`: `Value::Dummy`、`Payload::EvTable { effect: u32, entries: Vec<Value> }`、`Payload::Evidence { table: ObjRef, state: Value, outer: Value }`、`Payload::Evv { evidence: ObjRef, next: Value }` (Task 6)
  - `eml_interp`: `RunStats { evidence_steps, yields, yield_frames, string_bytes_copied, rc_increments, rc_decrements, boxes, unboxes, peak_objects }` の順 (Task 9。`handler_visits` は Task 9 で消す)
  - 一時的なもの (Task 10 で消す): `eml_core_ir::lower_with_evidence` (Task 8) とそれを置き換える `eml_core_ir::lower_without_evidence` (Task 9)、`eml_cli::Session::compile_with_evidence` (Task 8) とそれを置き換える `compile_without_evidence` (Task 9)、`crates/eml_cli/tests/ui_evidence.rs` (Task 8)
- verifier の文言は spec と各タスクに書いたとおりにする。変えるときは、そのタスクのテストと一緒に変え、理由をコミットメッセージに書く
- 日本語のコメントと文書は、書く前に `yomiyasu:yomiyasu` のスキルを呼び、その規則に従う。コメントは「なぜ」を書き、規則を指すときは `docs/` のパスを引く。spec の内容がまだ `docs/spec/` に移っていない間は、`docs/superpowers/specs/2026-10-11-s7-evidence-passing-design.md` の節を引く (`citations.rs` は `docs/superpowers` を見ないので、引いてよい)。Task 11 で `docs/spec/` に移すときに、引いた先を直す。コメントに計画のタスクの番号を書かない。UI テストの `.em` のコメントは、既存の UI テストに合わせて英語で書く。CLAUDE.md は英語で書く
- UI テストを足したら `cargo test -p eml_cli --test integration ui::` で流し、新しいスナップショットを `cargo insta review` で中身を読んで受け入れる。run のテストは `debug_heap` 付きで走る。新しい UI テストの出力は、各タスクに書いた期待する出力と比べる。食い違ったら、まず今の CEK (EP を通さない経路) の出力を正として、どちらの誤りかを確かめる
- テストの例のうち、中身を `/* … */` やコメントで示したものは、同じタスクの隣のテストと同じ書き方で、完全なコード (入力の IR かソースと `assert_eq!` かスナップショット) にする。スナップショットの `@"…"` は、中身を読んで、そのテストのコメントに書いた性質を満たすことを確かめてから受け入れる
- コミットメッセージは英語で、既存のコミットの形 (命令形の1行目、本文に理由) に合わせる。テストの期待値を変えたコミットは、本文に変更の種類 (期待値の変更、機械的な追随、成否の変更) と理由を書く

## Review Focus

spec が求めるが、どのタスクのテストも直接は確かめない形のうち、使う人が最初に踏みそうなもの。各行のテストは、対応するタスクに入れてある。

- row 多相の高階関数を、同じプログラムの中で純粋な文脈と handler の下の両方から使う。2つの instance がどちらも正しく動き、純粋な方には yield の確認が入らない (Task 3 の UI `run/effects/map_pure_and_effectful.em`、Task 9 の `evidence.rs` の `a_pure_instance_of_a_row_polymorphic_function_has_no_yield_check`)
- 深い末尾でない再帰の各段で、すぐに再開する操作を呼ぶ。ヒープの大きさが EP の後も深さに比例するだけで、2倍以上に膨らまない (Task 9 の `scaling.rs` の `immediate_operations_under_deep_recursion_keep_the_peak_linear`)
- `never` の操作による中断が、`File` を持ついくつもの末尾でないフレームを越える。どの `File` も1回だけ閉じ、リークがない (Task 8 の UI `run/files/files_released_by_abort_across_frames.em`)
- handle が返った後の継続を、同じエフェクトの別の handler の下で再開する (Task 8 の UI `run/effects/resume_under_another_handler.em`)
- handler の下で、関数値を通る末尾呼び出しのループを回す。ヒープが平らなまま (Task 9 の `scaling.rs` の `a_function_value_loop_under_a_handler_keeps_the_heap_flat`)

---

### Task 1: 型検査の `RowYield`

spec の「単相化と `yields` の印」の「row の具体化の記録」。translate はまだ読まない。

**Files:**
- Modify: `crates/eml_types/src/lib.rs` (`RowYield`、`Instantiation::rows`、`BodyTypes` の3つの表)
- Create: `crates/eml_types/src/check/yields.rs` (分類の関数)
- Modify: `crates/eml_types/src/check/mod.rs` (モジュールの宣言、`check_body` の書き出し)
- Modify: `crates/eml_types/src/check/body.rs` (`BodyTyping` に row の記録を足す。`instantiate`、`path`、`call`、`check_lambda`)
- Modify: `crates/eml_types/src/shape.rs` (`Instantiated::rows`)
- Test: `crates/eml_types/tests/instantiations.rs`、新しい `crates/eml_types/tests/yields.rs` (`tests/main.rs` に宣言する)

**Interfaces:**
- Consumes: なし
- Produces:
  ```rust
  // crates/eml_types/src/lib.rs
  /// 具体化した row が yield しうるか。`Var(i)` は、参照を含む定義の i 番目の row 変数 (`generics.row_vars` の順)
  /// に従う (docs/superpowers/specs/2026-10-11-s7-evidence-passing-design.md の「row の具体化の記録」)。
  #[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
  pub enum RowYield { #[default] Never, Always, Var(u32) }

  pub struct Instantiation { pub decl: ValueItem, pub args: Vec<TypeId>, pub rows: Vec<RowYield> }

  pub struct BodyTypes {
      // 既存のフィールドに加えて
      pub body_yield: RowYield,                               // 本体が動く row (引数の数番目の矢印)
      pub lambda_yields: ArenaMap<ExprId, RowYield>,          // ラムダの本体が動く row
      pub arrow_yields: HashMap<(ExprId, usize), RowYield>,   // masks と同じ鍵。呼び出しの矢印の row
  }
  ```

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_types/tests/yields.rs` を作り、`tests/main.rs` に `mod yields;` を足す。表の書き方は `instantiations.rs` に合わせる。

```rust
use eml_test_support::check;
use eml_types::RowYield;

/// 参照ごとの row の分類を「行:列 名前 [分類]」で並べる。
fn rows(text: &str, name: &str) -> String { /* instantiations.rs の table と同じ形で、args の代わりに rows を書く */ }

/// 関数の本体、ラムダ、呼び出しの矢印の分類を「位置 分類」で並べる。
fn bodies(text: &str, name: &str) -> String { /* body_yield、lambda_yields、arrow_yields を位置の順に */ }

#[test]
fn a_handleable_label_always_yields_and_io_never_does() {
    let text = "effect Ask where\n  ask : Unit -> Int\n\nf : Unit -> <Ask, IO> Int\nf () = ask ()\n\ng : Unit -> <IO> Unit\ng () = println \"x\"";
    insta::assert_snapshot!(bodies(text, "f"), @"body Always");
    insta::assert_snapshot!(bodies(text, "g"), @"body Never");
}

#[test]
fn a_row_variable_of_the_definition_is_var() {
    let text = "twice : (Unit -> <e> Int) -> <e> Int\ntwice f = f () + f ()";
    insta::assert_snapshot!(bodies(text, "twice"), @"
    body Var(0)
    2:11#0 Var(0)
    2:18#0 Var(0)
    ");
}

#[test]
fn the_rows_of_a_reference_follow_the_signature_order() {
    let text = "twice : (Unit -> <e> Int) -> <e> Int\ntwice f = f () + f ()\n\nmain : Unit -> <IO> Unit\nmain () = println (show (twice (fn () -> 1)))";
    insta::assert_snapshot!(rows(text, "main"), @"5:26 twice [Never]");
}

#[test]
fn an_operation_row_variable_in_a_clause_always_yields() {
    let text = "effect Run where\n  run : (Unit -> <e> Int) -> Int\n\nbody : Unit -> <Run> Int\nbody () = run (fn () -> 1)\n\nmain : Unit -> <IO> Unit\nmain () =\n  let r =\n    handle body () with\n      | run f k ->\n        let g = fn () -> f () + 1\n        k 7\n  println (show r)";
    // 節の中のラムダ g の row は操作の row 変数で、main の row 変数ではない
    insta::assert_snapshot!(bodies(text, "main"), @"…");
}

#[test]
fn an_unresolved_row_variable_never_yields() {
    let text = "main : Unit -> <IO> Unit\nmain () =\n  let h = fn f -> f 1 + 1\n  println \"x\"";
    insta::assert_snapshot!(bodies(text, "main"), @"…");
}
```

`@"…"` の2つは、Step 4 で中身を読んで受け入れる。`an_operation_row_variable_in_a_clause_always_yields` ではラムダ g の行が `Always`、`an_unresolved_row_variable_never_yields` ではラムダ h の行が `Never` であることを確かめる。

- [ ] **Step 2: テストが失敗することを確かめる**

Run: `cargo test -p eml_types --test integration yields`
Expected: `rows`、`body_yield` などがないので、コンパイルが通らない。

- [ ] **Step 3: 実装する**

- `shape.rs`: `Instantiated` に `rows: Vec<RowVar>` を足し、`Shape::instantiate` が作る row 変数 (538-542 行) をそこに入れる
- `check/body.rs`
  - `BodyTyping::instantiations` を `ArenaMap<ExprId, (ValueItem, Vec<Ty>, Vec<RowVar>)>` にし、`BodyCheck::instantiate` が `(ty, args, rows)` を返すようにする。記録するのは今と同じ `path` の1か所だけである
  - `BodyTyping` に `ambient: Option<Row>` (本体の row)、`lambdas: ArenaMap<ExprId, Row>`、`arrows: HashMap<(ExprId, usize), Row>` を足す。`ambient` は `check_body` が本体の検査に渡す row、`lambdas` は `check_lambda` (844 行のあたり) で本体の検査に渡す row、`arrows` は `call` の `next_arrow` が返す矢印の row で、`include_arg_row` の鍵と同じ `(call, index)` に入れる
- `check/yields.rs`

  ```rust
  /// 解いた row を分類する。`own` は、この定義の row 変数を `generics.row_vars` の順に並べたものである。
  pub(super) fn row_yield(table: &Table, row: &Row, own: &[RowVar]) -> RowYield {
      let row = table.resolve_row(row);
      if row.labels.iter().any(|label| !table.context.is_extern_effect(label.effect)) {
          return RowYield::Always;
      }
      match row.tail {
          Tail::Closed | Tail::Error => RowYield::Never,
          Tail::Var(var) if !table.is_rigid_row(var) => RowYield::Never,
          Tail::Var(var) => match own.iter().position(|&o| o == var) {
              Some(index) => RowYield::Var(index as u32),
              // 操作のシグネチャの row 変数を節の中で具体化したもの
              None => RowYield::Always,
          },
      }
  }
  ```

  `table.context` が `pub(crate)` でなければ、`Table` に `is_extern_effect` を委ねるメソッドを足す
- `check/mod.rs` の書き出し (445-452 行): `own.rigids.row_vars()` を `own` として、`instantiations` の `rows`、`ambient`、`lambdas`、`arrows` を `row_yield` で分類して `BodyTypes` に入れる。`table` はこの時点でまだ生きている
- 本体のない関数 (extern) と、`instantiate` を通らない参照の `rows` は空のままでよい

- [ ] **Step 4: テストを流す**

Run: `cargo test -p eml_types`
Expected: すべて通る。`yields.rs` の `@"…"` を中身を読んで受け入れる。`instantiations.rs` の既存の期待値は変わらない (表は `rows` を書かない)。

- [ ] **Step 5: lint とコミット**

```bash
cargo clippy --all-targets && cargo fmt
git add -A crates/eml_types
git commit -m "Record whether each instantiated row may yield"
```

---

### Task 2: Core IR の印と操作の表の Repr

spec の「`yields` の印」「`apply` の印と分け方」の Core IR の部分と、「操作の表の Repr」。translate はまだ印を立てない (いつも `false`)。

**Files:**
- Modify: `crates/eml_core_ir/src/lib.rs` (`CoreFn::yields`、`Call::Apply` を名前付きのフィールドに、`OperationInfo`、`OpRet`)
- Modify: `crates/eml_core_ir/src/pretty.rs`、`src/text.rs` (`yields fn`、`yields apply`、`effect` の新しい綴り)
- Modify: `crates/eml_core_ir/src/verify.rs` (`check_clause_arities` の `op.arity` を `op.params.len()` に)
- Modify: `crates/eml_core_ir/src/translate/program.rs` (`effect_table` に操作の Repr を入れる)
- Modify: `Call::Apply` と `OperationInfo` を使うすべての所 (`boxing.rs`、`contract.rs`、`perceus.rs`、`translate/*.rs`、`crates/eml_interp/src/runtime.rs`)
- Test: `crates/eml_core_ir/tests/text.rs`、手書きの IR を持つすべてのテスト

**Interfaces:**
- Consumes: なし
- Produces:
  ```rust
  pub struct CoreFn { pub name: String, pub internal: bool, pub yields: bool, pub vars: Vec<VarInfo>, pub ret: Repr, pub blocks: Vec<Block> }
  pub enum Call { Direct(FnIdx, Vec<Atom>), Apply { callee: Atom, args: Vec<Atom>, yields: bool }, /* Handle、Perform、Resume は今のまま */ }
  pub struct OperationInfo { pub name: String, pub params: Vec<Repr>, pub ret: OpRet, pub resumable: bool }
  pub enum OpRet { Repr(Repr), Never }
  ```

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_core_ir/tests/text.rs` に足す。

```rust
#[test]
fn yields_marks_round_trip() {
    round_trip("yields fn f(c.0: tobj) -> tobj {\n  let t.1: tobj = yields apply c.0(())\n  return t.1\n}\n");
    round_trip("internal yields fn g(c.0: tobj) -> tobj {\n  let t.1: tobj = mask [Ask] yields apply c.0(())\n  return t.1\n}\n");
}

#[test]
fn operation_reprs_round_trip() {
    round_trip("effect Counter { get(unit) -> int, put(int) -> unit }\neffect Fail { never fail(tobj) -> never }\neffect Done {}\n");
}

#[test]
fn a_never_operation_returns_never() {
    assert_eq!(parse_error("effect Fail { never fail(tobj) -> tobj }\n").to_string(), "line 1: a `never` operation returns `never`");
    assert_eq!(parse_error("effect Ask { ask(unit) -> never }\n").to_string(), "line 1: only a `never` operation returns `never`");
}
```

`yields_marks_round_trip` の2つ目の入力には、`effect Ask { ask(unit) -> tobj }` の宣言を先頭に足す。

- [ ] **Step 2: テストが失敗することを確かめる**

Run: `cargo test -p eml_core_ir --test integration text::`
Expected: 新しい3つのテストが失敗する。

- [ ] **Step 3: 実装する**

- `lib.rs`: 型を上のとおりに変える。`Call::for_each_atom` と `for_each_atom_mut` の `Apply` の腕は、`callee` を先に、`args` をその次に渡す (今と同じ順)
- `pretty.rs`
  - 関数の見出しは `"{internal}{yields}fn …"` の順にする (`internal ` と `yields ` はどちらも後ろに空白を持つ)
  - `apply` は、印があれば `yields apply` と書く。`mask` があれば `mask [..] yields apply` の順になる
  - `effect` の操作は `"{never}{name}({params}) -> {ret}"` と書く。`params` は Repr の名前を `, ` でつなぎ、`ret` は `OpRet::Never` なら `never` と書く
- `text.rs`
  - 関数の見出しで、`internal` の後に `yields` を読む。`declare_functions` の前読みも、見出しの語の並びの変化に合わせる
  - `rhs()` の呼び出しの語の一覧 (682-685 行) と `masked_call` (809 行) と `call()` に、`yields` を `apply` の前の語として足す。`yields` の後が `apply` でなければ誤りにする (``expected `apply` after `yields` ``)
  - `operation_decl` を、`name(repr, ..) -> ret` の形を読むように書き直す。`ret` は Repr か `never` である。`never` の印と `-> never` が食い違えば、Step 1 の文言で誤りにする。名前に `(` が含まれるので、今の `a/b/2` の形はもう読まない
- `verify.rs`: `check_clause_arities` の `op.arity` を `op.params.len()` にする
- `translate/program.rs` の `effect_table`: 引数に `types: &TypeStore` と `typed: &TypedProgram` を足し、操作のスキームの型 (`typed.decls[&ValueItem::Operation(op)].ty`) を `split_arrows(types, ty, operation.arity)` で分け、引数と結果を `repr` で Repr にする。型変数は `repr` が `tobj` にする。`never` の操作の `ret` は `OpRet::Never` にする
- 手書きの IR を持つテストと `crates/eml_interp/tests/ir/mask.core` の `effect` の宣言を、新しい綴りに書き換える。操作の Repr は、その IR が使う形に合わせる (引数が1つで値を返す操作なら `ask(tobj) -> tobj`)。これは機械的な追随である
- `text.rs` の `an_operation_needs_its_arity_after_the_last_slash` は、新しい綴りの誤り (`ask` だけ、引数の括弧がない、`->` がない) を確かめるテストに書き直し、名前を `an_operation_needs_its_reprs` にする (期待値の変更)

- [ ] **Step 4: テストを流す**

Run: `cargo test`
Expected: すべて通る。`effect` を含む Core IR のダンプのスナップショットが変わるので、`effect` の行だけが変わっていることを読んで受け入れる。

- [ ] **Step 5: lint とコミット**

```bash
cargo clippy --all-targets && cargo fmt
git add -A crates
git commit -m "Add yields marks and operation reprs to Core IR"
```

本文に「`effect` の宣言の綴りを、操作の引数と結果の Repr を書く形に替えた。ダンプの `effect` の行と `an_operation_needs_its_reprs` の文言は期待値の変更、手書きの IR の書き換えは機械的な追随」と書く。

---

### Task 3: translate の row のビットと印

spec の「row のビットと instance の鍵」「`yields` の印」「`apply` の印と分け方」。

**Files:**
- Modify: `crates/eml_core_ir/src/translate/instances.rs` (鍵、`add`、`add_uniform`、`method_target`、`default_target`、`function_body`、`collect`、`instance_name`、`order`)
- Modify: `crates/eml_core_ir/src/translate/mod.rs` (`FnLowering` に `yields: bool`、`lower` と `lift` の引数)
- Modify: `crates/eml_core_ir/src/translate/expr.rs` (`apply` の分け方と印、ラムダ、handle の本体、節)
- Modify: `crates/eml_core_ir/src/translate/program.rs` (`simple`、`continuation_wrapper`、`operation_wrapper`、`entry` の印)
- Modify: `crates/eml_core_ir/src/translate/derive.rs` (`yields: false`)
- Create: `tests/ui/run/effects/over_application_yields.em`、`tests/ui/run/effects/operation_row_variable.em`、`tests/ui/run/effects/map_pure_and_effectful.em`
- Test: `crates/eml_core_ir/tests/instances.rs`、`crates/eml_core_ir/tests/translate.rs`

**Interfaces:**
- Consumes: Task 1 の `RowYield`、`Instantiation::rows`、`BodyTypes` の3つの表。Task 2 の `CoreFn::yields`、`Call::Apply { yields }`
- Produces: 鍵 `(FunctionId, Vec<TypeId>, Vec<bool>)`。`FunctionInstance::bits: Vec<bool>`。

  ```rust
  // translate/instances.rs
  fn resolve(row: RowYield, bits: &[bool]) -> bool {
      match row { RowYield::Never => false, RowYield::Always => true, RowYield::Var(i) => bits[i as usize] }
  }
  ```

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_core_ir/tests/instances.rs` に足す。

```rust
#[test]
fn a_row_polymorphic_function_has_an_instance_per_row_bit() {
    let text = "effect Ask where\n  ask : Unit -> Int\n\ntwice : (Unit -> <e> Int) -> <e> Int\ntwice f = f () + f ()\n\nmain : Unit -> <IO> Unit\nmain () =\n  println (show (twice (fn () -> 1)))\n  println (show (handle twice ask with\n    | ask () k -> k 2))";
    let shown = core_text(text, Pass::Translate);
    // 純粋な instance は今の名前で印がなく、yield しうる instance は `<e>` を持つ
    insta::assert_snapshot!(function(&shown, "twice"), @"…");
    insta::assert_snapshot!(function(&shown, "twice<e>"), @"…");
}

#[test]
fn an_apply_is_split_after_an_arrow_that_may_yield() {
    let text = "effect Ask where\n  ask : Unit -> Int\n\nrun : (Int -> <Ask> (Int -> Int)) -> <Ask> Int\nrun h = h 1 2\n\nmain : Unit -> <IO> Unit\nmain () = println \"x\"";
    let shown = core_text_with_entry(text, "run", Pass::Translate);
    // h 1 で yield しうるので、1つ目の apply の後で分け、1つ目に印を付ける
    insta::assert_snapshot!(function(&shown, "run"), @"…");
}
```

`core_text_with_entry` がなければ、`run` を `main` から呼ぶ形にして `core_text` を使う。`twice` の純粋な instance のダンプには `yields` が1つもないこと、`twice<e>` には `yields fn` と `yields apply` があることを確かめる。`run` のダンプでは、`yields apply h.0(1)` と、印のない `apply t.1(2)` の2つに分かれていることを確かめる。

新しい UI テストを3つ書く。

```haskell
-- tests/ui/run/effects/over_application_yields.em
-- One application passes two arguments to a function that performs an operation after the first.
effect Ask where
  ask : Unit -> Int

adder : Int -> <Ask> (Int -> Int)
adder x =
  let n = ask ()
  fn y -> x + y + n

run : (Int -> <Ask> (Int -> Int)) -> <Ask> Int
run h = h 1 2

main : Unit -> <IO> Unit
main () =
  let r =
    handle run adder with
      | ask () k ->
        let v = k 10
        v * 2
  println (show r)
```

期待する出力は `26` である。

```haskell
-- tests/ui/run/effects/operation_row_variable.em
-- An operation whose parameter is an effectful function; the clause builds a lambda over that row.
effect Run where
  run : (Unit -> <e> Int) -> Int

body : Unit -> <Run> Int
body () = run (fn () -> 1)

main : Unit -> <IO> Unit
main () =
  let r =
    handle body () with
      | run f k ->
        let g = fn () -> f () + 1
        k 7
  println (show r)
```

期待する出力は `7` である。型検査が通らないとき (未使用の `g` の扱いなど) は、`g` を使う形に変えて、操作の row 変数を節の中のラムダの型に使う形を保つ。

```haskell
-- tests/ui/run/effects/map_pure_and_effectful.em
-- A row-polymorphic function is used both without handlers and under a handler in one program.
effect Ask where
  ask : Unit -> Int

mapl : (a -> <e> b) -> List a -> <e> List b
mapl f xs =
  match xs with
    | [] -> []
    | x :: rest -> f x :: mapl f rest

main : Unit -> <IO> Unit
main () =
  println (show (mapl (fn x -> x + 1) [1, 2, 3]))
  let r =
    handle mapl (fn x -> x + ask ()) [1, 2, 3] with
      | ask () k -> k 10
  println (show r)
```

期待する出力は `[2, 3, 4]` と `[11, 12, 13]` である。

- [ ] **Step 2: テストが失敗することを確かめる**

Run: `cargo test -p eml_core_ir --test integration instances::`
Expected: `twice<e>` がないので失敗する。UI テストは今の CEK で通る (Step 4 でスナップショットを受け入れる)。

- [ ] **Step 3: 実装する**

- `instances.rs`
  - `Collector::keys` を `HashMap<(FunctionId, Vec<TypeId>, Vec<bool>), usize>` にし、`FunctionInstance` と `FoundFunction` に `bits: Vec<bool>` を足す。`add`、`add_uniform` は `bits` を受ける
  - `function_body` で参照ごとに、`instantiation.rows` を含む instance の `bits` で `resolve` してビットの列を作り、`add_uniform` か `method_target` に渡す。`method_target` と `default_target` は、メソッドのシグネチャの row 変数の順のビットをそのまま解決先に渡す。導出とタプルの生成器はビットを持たない
  - 型変数を持つ関数で `BodyTypes` を作り直す所 (344-402 行) で、`masks` と同じく `body_yield`、`lambda_yields`、`arrow_yields` を写す
  - `collect` の入口のビットは、すべて `true` にする (row 変数の数だけ)
  - `instance_name` に `bits` と row 変数の名前を渡し、立ったビットの名前を `<e, f>` の形で後ろに付ける。型変数がなくビットが立つ関数も、名前の後ろに付ける。`@[…]` が長くて番号の形にした名前にも付ける
- `mod.rs`
  - `FnLowering` に `yields: bool` を足す。`lower` の引数に `yields` を足し、`CoreFn` に入れる
  - トップレベルの関数の instance は `resolve(body_yield, bits)`、ラムダは `resolve(lambda_yields[id], bits)`、handle の本体は `true`、操作の節と `return` の節は、handle を含む今の関数の `self.yields` にする
- `program.rs`: `simple` に `yields` を足す。`op$` と `cont$` と `cont$state` は `true`、`con$` と `$externN` は `false`。入口の関数 `entry$…` は、入口の instance の印にする
- `derive.rs`: 生成する関数は `yields: false`
- `boxing.rs` の `boxed`: `f$boxed` は `f` の `yields` の印を写す (`internal` と同じ扱い)
- `expr.rs` の `apply`: 矢印ごとに `(mask, yields)` を求める。`yields` は `resolve(arrow_yields[(id, arrow)], bits)` で、表になければ `false` である。`chunk_by` の条件を「`mask` が同じで、前の矢印が yield しない」にする。1つの `apply` の印は、受け持つ矢印のどれかが yield しうるときに立てる

  ```rust
  let arrows: Vec<(Vec<u32>, bool)> = (first..last).map(|arrow| (self.mask(id, arrow), self.arrow_yields(id, arrow))).collect();
  for run in arrows.chunk_by(|a, b| a.0 == b.0 && !a.1) {
      let yields = run.iter().any(|(_, y)| *y);
      // 今と同じく part を作り、masked_call(Call::Apply { callee: function, args: part, yields }, run[0].0.clone())
  }
  ```
- `saturate` の `Call::Direct` の後に余った引数を渡す `apply` も、同じ `apply` を通るので変更は要らない

- [ ] **Step 4: テストを流す**

Run: `cargo test`
Expected: すべて通る。translate 以降のダンプのスナップショットのうち、`<e>` の名前、`yields` の印、`apply` の分け方だけが変わっていることを読んで受け入れる。新しい UI テストの出力が期待どおりであることを確かめて受け入れる。

- [ ] **Step 5: lint とコミット**

```bash
cargo clippy --all-targets && cargo fmt
git add -A crates tests/ui
git commit -m "Split instances by whether their rows may yield and mark yielding calls"
```

本文に「row のビットで instance を分け、`yields` の印と `apply` の印を付け、yield しうる矢印の後でも `apply` を分けた。ダンプの名前と印の変化は期待値の変更」と書く。

---

### Task 4: `prim`、`clause`、`verify_evidence`

spec の「基本操作」「節の呼び出し」「入口の関数の印」「verifier」。この段では、範囲の段と所有の段は `handle`、`perform`、`resume` を今のまま受け付ける (それらを拒むのは Task 10)。

**Files:**
- Create: `crates/eml_core_ir/src/prim.rs` (`Prim`、`PrimRow`、名前の表)
- Modify: `crates/eml_core_ir/src/lib.rs` (`Rhs::Prim`、`Call::Clause`、`CoreFn::clause`、走査のメソッド、`pub use`)
- Modify: `crates/eml_core_ir/src/pretty.rs`、`src/text.rs`
- Modify: `crates/eml_core_ir/src/verify.rs` (`Level::Evidence`、`verify_evidence`、規則と文言)
- Modify: `crates/eml_core_ir/src/boxing.rs`、`contract.rs`、`perceus.rs`、`crates/eml_interp/src/machine.rs`、`runtime.rs` (新しい腕。この段では `unreachable!` か、Task 5 と Task 6 の実装を待つ `todo` でなく、下の最小の形)
- Test: `crates/eml_core_ir/tests/text.rs`、`tests/verify.rs`

**Interfaces:**
- Consumes: Task 2 の `OperationInfo::params`、`OpRet`
- Produces:
  ```rust
  // crates/eml_core_ir/src/prim.rs
  #[derive(Debug, Clone, PartialEq, Eq, Hash)]
  pub enum Prim {
      EvvGet, EvvSet, EvvPush, EvvMask(Vec<u32>),
      EvNew(u32), EvSetClause { effect: u32, op: u32 }, EvRenew,
      EvLookup(u32), EvClause { effect: u32, op: u32 }, EvEvv, EvTakeState, EvSetState,
      Yielding, YieldStatus, YieldTo, YieldAbort, YieldContTake, YieldContSet, YieldFinish, Dummy,
  }
  pub struct PrimRow { pub name: String, pub params: Vec<Repr>, pub ret: Repr }
  impl Prim {
      pub fn row(&self, program: &Program) -> PrimRow;  // 名前は即値をエフェクトと操作の名前で書いた形 (`ev_clause[State.get]`)
      pub fn pure(&self) -> bool { false }
  }
  // lib.rs
  pub enum Rhs { /* 既存 */ Prim(Prim, Vec<Atom>) }
  pub enum Call { /* 既存 */ Clause { effect: u32, op: u32, callee: Atom, args: Vec<Atom> } }
  pub struct CoreFn { /* 既存 */ pub clause: Option<(u32, u32)> }
  pub fn verify_evidence(program: &Program) -> Result<(), VerifyError>;
  ```

  spec の表との違い: `ev_set_clause` と `ev_clause` の即値に、操作だけでなくエフェクトも持たせる。テキストの形で操作の名前をエフェクトで修飾するためである。spec の綴り `ev_clause[op]` は `ev_clause[State.get]` になる。

  | `Prim` | テキスト | 引数 | 結果 |
  |---|---|---|---|
  | `EvvGet` | `evv_get` | なし | `tobj` |
  | `EvvSet` | `evv_set` | `tobj` | `unit` |
  | `EvvPush` | `evv_push` | `tobj`、`obj` | `tobj` |
  | `EvvMask(effects)` | `evv_mask[E, ..]` | `tobj` | `tobj` |
  | `EvNew(e)` | `ev_new[E]` | `tobj`、`tobj` | `obj` |
  | `EvSetClause` | `ev_set_clause[E.op]` | `obj`、`tobj` | `unit` |
  | `EvRenew` | `ev_renew` | `obj`、`tobj` | `obj` |
  | `EvLookup(e)` | `ev_lookup[E]` | なし | `obj` |
  | `EvClause` | `ev_clause[E.op]` | `obj` | `tobj` |
  | `EvEvv` | `ev_evv` | `obj` | `tobj` |
  | `EvTakeState` | `ev_take_state` | `obj` | `tobj` |
  | `EvSetState` | `ev_set_state` | `obj`、`tobj` | `unit` |
  | `Yielding` | `yielding` | なし | `int` |
  | `YieldStatus` | `yield_status` | `obj` | `int` |
  | `YieldTo` | `yield_to` | `obj`、`tobj`、`tobj` | `unit` |
  | `YieldAbort` | `yield_abort` | `obj`、`tobj`、`tobj` | `unit` |
  | `YieldContTake` | `yield_cont_take` | なし | `tobj` |
  | `YieldContSet` | `yield_cont_set` | `tobj` | `unit` |
  | `YieldFinish` | `yield_finish` | なし | `tobj` |
  | `Dummy` | `dummy` | なし | `obj` |

- [ ] **Step 1: 失敗するテストを書く**

`tests/text.rs` に足す。

```rust
#[test]
fn prims_and_clauses_round_trip() {
    round_trip("effect State { get(unit) -> tobj, put(tobj) -> unit }\nyields fn f(s.0: tobj) -> tobj {\n  let w.1: tobj = prim evv_get()\n  let e.2: obj = prim ev_new[State](s.0, w.1)\n  let u.3: unit = prim ev_set_clause[State.get](e.2, &g)\n  let m.4: tobj = prim evv_mask[State, State](w.1)\n  let c.5: tobj = prim ev_clause[State.get](e.2)\n  let r.6: tobj = clause State.get c.5(e.2, ())\n  return r.6\n}\ninternal yields clause State.get fn g(e.0: obj, u.1: unit) -> tobj {\n  let s.2: tobj = prim ev_take_state(e.0)\n  return s.2\n}\n");
}
```

`tests/verify.rs` に足す。`check_evidence` は `verify_evidence` を呼ぶ補助で、ほかの `check_*` と同じ形で足す。

```rust
#[test]
fn the_evidence_level_rejects_effect_instructions() {
    assert_eq!(check_evidence(&format!("{ASK}fn f() -> tobj {{\n  let t.0: tobj = perform Ask.ask(())\n  return t.0\n}}\n")), Err("a perform remains after the evidence pass in `f`".to_string()));
    // handle、resume、mask 付きの呼び出しも同じ形で `a handler`、`a resume`、`a mask` を確かめる
}

#[test]
fn the_translated_level_rejects_prims_and_clauses() {
    assert_eq!(check_translated("fn f() -> tobj {\n  let w.0: tobj = prim evv_get()\n  return w.0\n}\n"), Err("`prim evv_get` is used before the evidence pass in `f`".to_string()));
}

#[test]
fn a_prim_takes_its_arguments() {
    assert_eq!(check_evidence("fn f() -> unit {\n  let u.0: unit = prim evv_set()\n  return u.0\n}\n"), Err("`prim evv_set` takes 1 arguments but is given 0 in `f`".to_string()));
}

#[test]
fn the_boundaries_of_prims_and_clauses_are_compatible() {
    // 範囲の段: 状態の引数に int の変数は渡せない
    assert_eq!(check_scopes("fn f(n.0: int, w.1: tobj) -> obj {\n  let e.2: obj = prim ev_new[State](n.0, w.1)\n  return e.2\n}\n" /* State の宣言を先頭に */), Err("argument 0 of `prim ev_new[State]` is `n.0` (int), but the primitive takes tobj in `f`".to_string()));
    // clause: 操作の引数と結果
    // "argument 1 of a clause of `Counter.put` is `n.1` (tobj), but the operation takes int"
    // "`t.0` (int) is bound to a clause of `State.get`, which returns tobj"
    // "a tail clause of `Counter.get` returns int, but this function returns tobj"
}

#[test]
fn a_clause_function_has_the_reprs_of_its_operation() {
    // "`g` is a clause of `Counter.put`, but its parameter 2 is tobj"
}

#[test]
fn a_clause_function_is_only_put_into_a_handler_table() {
    // `&g` を apply に渡すと "`g` is a clause of `Counter.put`, but it is used as a function value"
    // `call g(..)` は "`g` is a clause of `Counter.put`, but it is called directly"
}

#[test]
fn ev_set_clause_names_the_clause_of_its_function() {
    // ev_new[State] の evidence に、Counter.put の印の関数を入れる
    // "`g` is a clause of `Counter.put`, but it is put into the `State.get` entry"
}
```

コメントにした文言の行も、実際のテストではそれぞれ IR を書いて `assert_eq!` にする。IR は上の `prims_and_clauses_round_trip` と同じ書き方で、`effect Counter { get(unit) -> int, put(int) -> unit }` を先頭に置く。

- [ ] **Step 2: テストが失敗することを確かめる**

Run: `cargo test -p eml_core_ir --test integration text:: verify::`
Expected: 新しいテストが、コンパイルが通らないか失敗する。

- [ ] **Step 3: 実装する**

- `prim.rs`: 上の表を `row` の `match` で書く。`row` は名前を作るために `Program` の `effects` を読む
- `lib.rs`: `Rhs::Prim` を `for_each_atom`、`for_each_consumed` (引数をすべて消費)、`for_each_atom_mut` に足す。`Call::Clause` は `callee`、`args` の順に渡す。`CoreFn` に `clause` を足し、`CoreFn` を作るすべての所で `clause: None` にする
- `pretty.rs` と `text.rs`
  - `prim name[imm](args)` を `rhs()` の2つ目の `match` (691-731 行) に足す。即値は、エフェクトなら名前、操作なら `E.op`、多重集合なら `E, E` を書く
  - `clause E.op callee(args)` を `call()` と呼び出しの語の一覧に足す。`mask` は付けられない (`masked_call` の一覧に入れない)
  - 関数の見出しは `[internal ][yields ][clause E.op ]fn …` の順にする
- `verify.rs`
  - `Level` を `Translated < Evidence < Scopes < Ownership` の4段にし、`pub fn verify_evidence` を足す。`checks_boundaries` は `Scopes` 以上のままにする
  - `Translated` の段で `prim` と `clause` と `clause` の印の関数を拒む (``prim evv_get` is used before the evidence pass``、``a clause call is used before the evidence pass``)
  - `Evidence` の段で `handle`、`perform`、`resume` と、空でない `mask` を拒む (``a perform remains after the evidence pass``、``a handler …``、``a resume …``、``a mask …``)
  - `prim` の引数の数は、すべての段で extern と同じ形で確かめる。引数と結果の Repr は、`checks_boundaries` の段で `passes` と `compatible` で比べる
  - `clause`: 操作が表にあり、引数の数が `1 + params.len()` である。境界の段で、`callee` は `tobj`、先頭は `obj`、残りは操作の `params` と、束縛は操作の `ret` と比べる (`OpRet::Never` なら `never` の `perform` と同じく比べない。`self.never` に記録する)
  - `clause` の印の関数: 境界の段で、捕獲の後の引数が `obj` と操作の `params`、`ret` が操作の `ret` と互換であることを確かめる (`Never` は `obj`)。`Tables` を作るときに、関数ごとに1回だけ確かめる
  - 値の参照 (`&g`、`closure g`) の対象が `clause` の印の関数なら、それが `ev_set_clause` の2つ目の引数として使われる所のほかでは拒む。直接の呼び出しも拒む。`ev_set_clause[E.op]` に渡す値の関数が分かるとき (`&g` か、同じ関数の `closure g`) は、印が `(E, op)` と同じであることを確かめる
- `boxing.rs`、`contract.rs`、`perceus.rs`、`eml_interp` の新しい腕は、この段では次の最小の形にする。Task 5 と Task 6 で中身を入れる
  - `contract::pure`: `Rhs::Prim` は `false`
  - `perceus`: 新しい規則は要らない (`for_each_consumed` で足りる)
  - `boxing`、`eml_interp`: `unimplemented!("{} reaches {} before the evidence pass exists", ..)` で止める。translate の出力には現れないので、テストは通る

- [ ] **Step 4: テストを流す**

Run: `cargo test`
Expected: すべて通る。既存の期待値は変わらない。

- [ ] **Step 5: lint とコミット**

```bash
cargo clippy --all-targets && cargo fmt
git add -A crates
git commit -m "Add evidence primitives, clause calls and the evidence verifier level"
```

---

### Task 5: 後段のパスの `prim` と `clause`

spec の「後段のパス」の「box の挿入」「縮約」「Perceus」。

**Files:**
- Modify: `crates/eml_core_ir/src/boxing.rs` (`Converter::stmt` と `call`、`tail_position_call`、`raise_tail_returns`、`uniformize`、新しい `conform_clause`)
- Modify: `crates/eml_core_ir/src/contract.rs` (`tail_call`)
- Test: `crates/eml_core_ir/tests/boxing.rs`、`tests/contract.rs`、`tests/perceus.rs`

**Interfaces:**
- Consumes: Task 4 の `Prim::row`、`Call::Clause`、`CoreFn::clause`
- Produces: box の挿入の後の `clause` の印の関数は、操作の ABI に合う

- [ ] **Step 1: 失敗するテストを書く**

`tests/boxing.rs` に、`boxing_text` (入力は `verify_evidence` で確かめるように変える。下の Step 3) で足す。

```rust
#[test]
fn prim_operands_are_converted_like_call_arguments() {
    let text = "effect State { get(unit) -> tobj, put(tobj) -> unit }\nfn f(n.0: int) -> obj {\n  let w.1: tobj = prim evv_get()\n  let e.2: obj = prim ev_new[State](n.0, w.1)\n  return e.2\n}\n";
    insta::assert_snapshot!(function(&boxing_text(text), "f"), @"…"); // `let n.3: tobj = box n.0` を置いて渡す
}

#[test]
fn a_clause_call_uses_the_reprs_of_its_operation() {
    let text = "effect Counter { get(unit) -> int, put(int) -> unit }\nyields fn f(c.0: tobj, e.1: obj, n.2: tobj) -> tobj {\n  let u.3: unit = clause Counter.put c.0(e.1, n.2)\n  let c.4: tobj = prim ev_clause[Counter.get](e.1)\n  let m.5: tobj = clause Counter.get c.4(e.1, ())\n  return m.5\n}\n";
    insta::assert_snapshot!(function(&boxing_text(text), "f"), @"…"); // n.2 を unbox して渡し、get の int の結果を受け直して box する
}

#[test]
fn a_clause_function_takes_the_reprs_of_its_operation() {
    let text = "effect Counter { get(unit) -> int, put(int) -> unit }\ninternal yields clause Counter.put fn g(e.0: obj, n.1: tobj) -> tobj {\n  return ()\n}\nfn main() -> unit {\n  let w.0: tobj = prim evv_get()\n  let e.1: obj = prim ev_new[Counter](0, w.0)\n  let u.2: unit = prim ev_set_clause[Counter.put](e.1, &g)\n  return ()\n}\n";
    insta::assert_snapshot!(function(&boxing_text(text), "g"), @"…"); // 引数 n は int、ret は unit になる
}

#[test]
fn a_never_clause_call_is_not_rebound() {
    // `let r.2: int = clause Fail.fail c.0(e.1, s)` の束縛を受け直さず、`return r.2` にも変換を入れない
}
```

`tests/contract.rs` に足す。

```rust
#[test]
fn a_clause_call_returned_at_the_end_becomes_a_tail_call() {
    // `ret` が int の関数で `clause Counter.get` の結果を返すと、`tail clause Counter.get …` になる
    // `ret` が tobj の関数で同じ形は、int と tobj が互換でないので `tail` にならない
}

#[test]
fn a_never_clause_returned_at_the_end_becomes_a_tail_call_in_any_function() {
    // `perform never` の既存のテストと同じ形を `clause Fail.fail` で書く
}
```

`tests/perceus.rs` に足す。

```rust
#[test]
fn prims_consume_their_arguments() {
    // `prim ev_clause[..](e.1)` と `clause ..(e.1, ..)` で e.1 を2回使うと、1回目の前に `dup e.1` が入る
}
```

- [ ] **Step 2: テストが失敗することを確かめる**

Run: `cargo test -p eml_core_ir --test integration boxing:: contract:: perceus::`
Expected: Task 4 の `unimplemented!` で止まる。

- [ ] **Step 3: 実装する**

- テストの補助: `boxing_text` の入力の検査は、入力に `prim` や `clause` があれば `verify_evidence`、なければ今の `verify_translated` にする (`contract_text`、`perceus_text` は今の `verify_scopes` のまま)
- `boxing.rs` の `Converter::stmt`: `Rhs::Prim(prim, args)` は、`prim.row(program)` の `params` へ各引数を `convert` し、束縛を `rebind(var, row.ret)` で受け直す。`Converter` に `program` の参照 (か、前もって作った行の表) を持たせる
- `Converter::call`: `Call::Clause` は、`callee` を `tobj`、`args[0]` を `obj`、残りを操作の `params` へ変換する。結果は `OpRet::Repr(r)` なら `Some(r)`、`Never` なら `None` (今の `never` の `perform` と同じ)
- `tail_position_call` は、`never` の操作の `clause` を `None` にする。`raise_tail_returns` は `clause` の結果を操作の `ret` とみなす
- `uniformize`: `clause` の印の関数を対象から外し、新しい `conform_clause(function, operation)` を呼ぶ。`uniformize_in_place` と同じ手順で、捕獲 (`params` のうち、後ろの `1 + 操作の引数の数` 個より前) は `uniform_repr`、evidence は `obj`、残りは操作の `params` にし、`ret` は操作の `ret` (`Never` なら `obj`) にする。合わない引数は入口で受け直し、`ret` が変わったら `return` で変換する
- `contract.rs` の `tail_call`: `Call::Clause` は操作の `ret` と `ret` が互換なら、`Never` ならいつも `tail` にする

- [ ] **Step 4: テストを流す**

Run: `cargo test`
Expected: すべて通る。新しいスナップショットの中身を読んで受け入れる。

- [ ] **Step 5: lint とコミット**

```bash
cargo clippy --all-targets && cargo fmt
git add -A crates
git commit -m "Convert prim operands and clause calls in the boxing pass"
```

---

### Task 6: ランタイムの evidence と CEK の基本操作

spec の「実行時のモデル」「ダミーの値」「CEK とランタイム」。

**Files:**
- Modify: `crates/eml_runtime/src/heap.rs` (`Value::Dummy`、`Payload` の3つの種類、`kind_name`、`children`、`copy`)
- Modify: `crates/eml_runtime/src/heap/tests.rs`
- Create: `crates/eml_interp/src/prims.rs`
- Modify: `crates/eml_interp/src/runtime.rs` (`evv`、`yield_state`、カウンタ、`Runtime::new` の引数)、`machine.rs` (`Rhs::Prim`、`Call::Clause`、`scalar`)、`lib.rs` (`mod prims`、終わりの検査)
- Test: `crates/eml_interp/tests/evidence.rs` (`tests/main.rs` に宣言する)

**Interfaces:**
- Consumes: Task 4 の `Prim`、`Rhs::Prim`、`Call::Clause`
- Produces:
  ```rust
  // eml_runtime
  pub enum Value { Int(i64), Unit, Tag(u32), Fn(u32), Obj(ObjRef), Dummy }
  pub enum Payload { /* 既存 */ EvTable { effect: u32, entries: Vec<Value> }, Evidence { table: ObjRef, state: Value, outer: Value }, Evv { evidence: ObjRef, next: Value } }
  // eml_interp の Runtime
  pub(crate) evv: Value,                   // 空のリストは Value::Unit
  pub(crate) yield_state: YieldState,
  pub(crate) evidence_steps: u64, pub(crate) yields: u64, pub(crate) yield_frames: u64,
  pub(crate) enum YieldState { None, Yielding { target: ObjRef, clause: Value, cont: Value, abort: bool } }
  impl Runtime<'_> { pub(crate) fn prim(&mut self, prim: &Prim, args: Vec<Value>) -> Result<Value, Fault> }
  ```

- [ ] **Step 1: 失敗するテストを書く**

`heap/tests.rs` に足す。

```rust
#[test]
fn an_evidence_releases_its_table_state_and_outer_list() {
    let mut heap = Heap::new();
    let clause = string(&mut heap, "clause");
    let table = heap.alloc(Payload::EvTable { effect: 0, entries: vec![Value::Obj(clause)] });
    let state = string(&mut heap, "state");
    let outer_evidence = heap.alloc(Payload::Evidence { table: { heap.dup(table).unwrap(); table }, state: Value::Unit, outer: Value::Unit });
    let outer = heap.alloc(Payload::Evv { evidence: outer_evidence, next: Value::Unit });
    let evidence = heap.alloc(Payload::Evidence { table, state: Value::Obj(state), outer: Value::Obj(outer) });
    heap.decref(evidence).unwrap();
    assert!(heap.live_objects().is_empty());
}
```

`crates/eml_interp/tests/evidence.rs` を作る。IR は EP の出力の形で手で書き、`run_core` で動かす。

```rust
use crate::common::run_core;

/// すぐに再開する節を evidence から引いてその場で呼ぶ。状態は evidence の欄に残る。
#[test]
fn an_immediate_clause_reads_and_writes_the_state_of_its_evidence() {
    let text = "effect Counter { get(unit) -> tobj, put(tobj) -> unit }\n\
internal yields clause Counter.get fn get(e.0: obj, u.1: unit) -> tobj {\n  let s.2: tobj = prim ev_take_state(e.0)\n  dup s.2\n  …\n}\n\
… main は ev_new、ev_set_clause、evv_push、evv_set、ev_lookup、ev_clause、clause、evv_set で元に戻し、println で値を出す …";
    let (output, result) = run_core(text);
    assert_eq!(result, Ok(()));
    assert_eq!(output, "5\n");
}

/// yield_to で始めた yield を、yield_status と yield_finish で受け取る。
#[test]
fn a_yield_reaches_the_status_of_its_target() { /* yield_status が 1、ほかの evidence では 3、yield_finish の後は 0 */ }

#[test]
fn evv_mask_skips_the_innermost_evidence_of_an_effect() { /* 同じエフェクトを2つ積み、mask の後の ev_lookup が外側を返す */ }

#[test]
fn a_dummy_passes_through_box_unbox_dup_and_decref() { /* dummy を box、unbox、dup、decref しても誤りにならず、リークもない */ }

#[test]
fn a_program_that_ends_with_a_nonempty_evidence_list_is_an_internal_error() {
    // evv_push と evv_set のまま終わると "internal error: the evidence list is not empty at the end"
}
```

IR の本文は、Step 3 の基本操作の動作に合わせて書き、`verify` (所有の段) を通る形にする (`run_core` が検査する)。所有の段を通すには `dup`、`decref`、`save` を Perceus と同じ規則で手で置く。手で置くのが難しいテストは、`boxing`、`contract`、`perceus` を順にかけてから `run_core_unverified` で動かす補助 `run_ep_core` を `tests/evidence.rs` に作ってよい。

- [ ] **Step 2: テストが失敗することを確かめる**

Run: `cargo test -p eml_runtime && cargo test -p eml_interp --test integration evidence::`
Expected: 新しい `Payload` と `prim` がないので失敗する。

- [ ] **Step 3: 実装する**

- `heap.rs`
  - `Value::Dummy` を足す。`Runtime::dup` と `decref` は `Obj` のほかを何もしないので、そのまま通る
  - `Payload` に3つの種類を足し、`kind_name` を `"EvTable"`、`"Evidence"`、`"Evv"` にする
  - `children`: `EvTable` は `entries` の物体、`Evidence` は `table`、`state` と `outer` の物体、`Evv` は `evidence` と `next` の物体をたどる
  - `copy` はこの3つを写さない (共有されたクロージャの写しでは子を `dup` するだけで、物体そのものは写さない)。`take_or_copy` は今のまま、クロージャと継続だけを受け付ける
- `machine.rs`: `scalar` は `Value::Dummy` も通す。`Rhs::Prim(prim, args)` は `self.rt.prim(prim, self.env.atoms(args)?)` で束縛する。`Call::Clause { callee, args, .. }` は `Call::Apply` と同じく `apply_and_continue` に渡す
- `runtime.rs`: `Runtime::new` に、エフェクトごとの操作の数 `effect_sizes: Vec<usize>` を渡す (`Machine::new` が `program.effects` から作る)。`evv: Value::Unit`、`yield_state: YieldState::None`、3つのカウンタを足す
- `prims.rs`: 各基本操作を次のとおりにする。引数はすべて所有した参照として受け取り、残さないものは `decref` する
  - `evv_get`: `evv` を `dup` して返す。`evv_set(w)`: `evv` を w に替え、元の値を `decref` する
  - `evv_push(w, ev)`: `Evv { evidence: ev, next: w }` を作る
  - `evv_mask[effects](w)`: w を先頭からたどり、`effects` に残っている数だけ、合うエフェクトの節点を飛ばす。飛ばした節点の前までを新しい節点に写し (evidence を `dup`)、残りは `dup` して共有する。たどった節点の数を `evidence_steps` に足す。最後に w を `decref` する
  - `ev_new[E](st, w)`: `EvTable { effect: E, entries: vec![Value::Unit; effect_sizes[E]] }` と、それを指す `Evidence { table, state: st, outer: w }` を作る
  - `ev_set_clause[E.op](ev, c)`: evidence の表の `entries[op]` に c を入れ (元は `Unit`)、ev を `decref` する
  - `ev_renew(ev, st)`: ev の表を `dup` し、`Evidence { table, state: st, outer: evv の dup }` を作り、ev を `decref` する
  - `ev_lookup[E]()`: `evv` を先頭からたどり、表のエフェクトが E の最初の evidence を `dup` して返す。たどった節点の数を `evidence_steps` に足す。見つからなければ `Fault::Internal("an operation without evidence")`
  - `ev_clause[E.op](ev)`: 表の `entries[op]` を `dup` して返し、ev を `decref` する。`ev_evv(ev)`: `outer` を `dup` して返し、ev を `decref` する
  - `ev_take_state(ev)`: `state` を `Unit` と入れ替えて返し、ev を `decref` する。`ev_set_state(ev, s)`: `state` を s に替え、元の値を `decref` し、ev を `decref` する
  - `yielding()`: yield の状態がなしなら `Int(0)`、そうでなければ `Int(1)`
  - `yield_status(ev)`: なしなら 0。宛先が ev と同じ物体なら、中断でなければ 1、中断なら 2。違えば 3。ev を `decref` する
  - `yield_to(ev, c, k0)` と `yield_abort`: yield の状態がなしでなければ `Fault::Internal("a yield starts during another yield")`。`Yielding { target: ev, clause: c, cont: k0, abort }` にし、`yields` に 1 を足す
  - `yield_cont_take()`: `cont` を `Unit` と入れ替えて返す。`yield_cont_set(k)`: `cont` に k を入れ (元が `Unit` でなければ内部の誤り)、`yield_frames` に 1 を足す
  - `yield_finish()`: yield の状態をなしに戻し、宛先を `decref` し、`clause` を返す (`cont` が `Unit` でなければ内部の誤り)
  - `dummy()`: `Value::Dummy`
- `lib.rs` の `finish`: `debug_heap` の検査の前に、`evv` が `Unit` で yield の状態がなしであることを確かめ、そうでなければ `Fault::Internal("the evidence list is not empty at the end")` か `"a yield is left at the end"` を返す

- [ ] **Step 4: テストを流す**

Run: `cargo test`
Expected: すべて通る。

- [ ] **Step 5: lint とコミット**

```bash
cargo clippy --all-targets && cargo fmt
git add -A crates
git commit -m "Run evidence primitives in the interpreter"
```

---

### Task 7: EP のパスの骨組みと継続の関数

spec の「パスの位置と対象」「EP が作る補助の関数」「yield の地点」「ダミーの値」「継続の関数」「継続の組み立て」。handle などの変換は Task 8 で入れる。この段の EP は、`yields` の関数の yield の地点を分けるだけで、パイプラインにはまだ入れない。

**Files:**
- Move: `crates/eml_core_ir/src/translate/builder.rs` を `crates/eml_core_ir/src/builder.rs` へ (`pub(super)` を `pub(crate)` に。機械的な追随)
- Create: `crates/eml_core_ir/src/evidence/mod.rs` (`pub fn evidence`、対象の選び方、`Helpers`)
- Create: `crates/eml_core_ir/src/evidence/helpers.rs` (`id$`、`compose$`、`prompt$`、`resume$`、`rewind$`、`mask$[..]` を作る)
- Create: `crates/eml_core_ir/src/evidence/split.rs` (yield の地点、遅い道、継続の関数、合流のブロックの関数)
- Modify: `crates/eml_core_ir/src/lib.rs` (`mod evidence; pub use evidence::evidence;`)、`src/pipeline.rs` (`Pass::Evidence` を足すが、`lower_until` の流れにはまだ入れない)、`tests/common/mod.rs` (`read_back` の `Pass::Evidence` を `verify_evidence` に)、`src/boxing.rs` (`tail_position_call` を、文の位置も返す共有の関数にして `evidence` からも使う)
- Test: `crates/eml_core_ir/tests/evidence.rs` (`tests/main.rs` に宣言する)

**Interfaces:**
- Consumes: Task 2 の印、Task 4 の `Prim` と `Call::Clause`
- Produces:
  ```rust
  pub fn evidence(program: &mut Program);
  // evidence/mod.rs
  pub(crate) struct Helpers { id: Option<FnIdx>, compose: Option<FnIdx>, prompt: Option<FnIdx>, resume: Option<FnIdx>, rewind: Option<FnIdx>, masks: HashMap<Vec<u32>, FnIdx> }
  impl Helpers { pub(crate) fn id(&mut self, program: &mut Program) -> FnIdx; /* compose、prompt、resume、rewind、mask(effects) も同じ形 */ }
  // evidence/split.rs
  /// 変換の後の関数で、yield の地点の扱いを変える所。Task 8 の変換が作る。
  pub(crate) struct Annotations {
      pub(crate) not_yield_points: HashSet<(BlockId, usize)>,         // handle の本体の apply
      pub(crate) masked: HashMap<(BlockId, usize), Vec<u32>>,        // 直後に evv_set(w0) が続く mask の呼び出し
      pub(crate) forced: HashSet<(BlockId, usize)>,                  // mask の呼び出し (相手が yields でなくても地点にする)
  }
  pub(crate) fn split(program: &mut Program, function: FnIdx, annotations: &Annotations, helpers: &mut Helpers);
  ```

- [ ] **Step 1: 失敗するテストを書く**

`tests/evidence.rs` を作る。入力は手書きの translate の出力の形で、`evidence_text` は `parse`、`verify_translated`、`evidence`、`pretty`、`verify_evidence` を順にかける。

```rust
use eml_core_ir::{evidence, parse, pretty, verify_evidence, verify_translated};
use crate::common::function;

fn evidence_text(text: &str) -> String { /* 上の順にかけ、誤りなら panic する */ }

#[test]
fn a_function_that_does_not_yield_is_unchanged() {
    let text = "fn g() -> int {\n  return 1\n}\nfn f() -> int {\n  let x.0: int = call g()\n  let y.1: int = extern \"Prelude.+\"(x.0, 1)\n  return y.1\n}\n";
    assert_eq!(evidence_text(text), pretty(&parse(text).unwrap()));
}

#[test]
fn a_yield_point_continues_in_a_continuation_function() {
    let text = "yields fn g() -> int {\n  return 1\n}\nyields fn f(a.0: int) -> int {\n  let x.1: int = call g()\n  let y.2: int = extern \"Prelude.+\"(x.1, a.0)\n  return y.2\n}\n";
    let shown = evidence_text(text);
    insta::assert_snapshot!(function(&shown, "f"), @"
    yields fn f(a.0: int) -> int {
      let x.1: int = call g()
      let y.3: int = prim yielding()
      switch y.3 { 0 -> b2, _ -> b1 }
    b1:
      let k.4: tobj = prim yield_cont_take()
      let c.5: tobj = closure f$k0(a.0)
      let k.6: tobj = closure compose$(k.4, c.5)
      let u.7: unit = prim yield_cont_set(k.6)
      return 0
    b2:
      let y.2: int = extern \"Prelude.+\"(x.1, a.0)
      return y.2
    }
    ");
    insta::assert_snapshot!(function(&shown, "f$k0"), @"
    internal yields fn f$k0(a.0: int, x.1: int) -> int {
      let y.2: int = extern \"Prelude.+\"(x.1, a.0)
      return y.2
    }
    ");
}

#[test]
fn a_tail_position_call_is_not_a_yield_point() { /* `let x = call g(); return x` は確認を持たない */ }

#[test]
fn a_continuation_ends_at_the_next_yield_point() {
    // 2つの yield の地点。f$k0 の中の2つ目の地点で、遅い道は f$k1 を足し、速い道は `let r = call f$k1(..)` と `return r`
}

#[test]
fn a_continuation_calls_a_join_block_function() {
    // yield の地点の後に if があり、2つの枝が合流のブロック b3 に jump する。f$k0 の2つの jump は `call f$j3(..)` になる
}

#[test]
fn an_apply_without_the_yields_mark_is_not_a_yield_point() { /* `apply` は確認を持たず、`yields apply` は持つ */ }

#[test]
fn the_dummy_follows_the_return_repr() {
    // ret が obj と tobj の関数の遅い道は `let d = prim dummy()` と `return d`、enum は `#0`、unit は `()`
}

#[test]
fn helpers_are_made_once_in_their_final_form() {
    // 2つの関数が yield の地点を持っても compose$ と id$ は1つずつで、compose$ の中の apply は分けられない
    // compose$ の形は spec の「EP が作る補助の関数」と、下の Step 3 の IR のとおり
}
```

変数の番号と名前 (`y.3`、`k.4` など) は、Step 3 の実装の振り方で決まる。スナップショットの形 (文の並び、`switch` の向き、遅い道の中身) が上と同じであることを確かめ、番号は実装に合わせて受け入れる。

- [ ] **Step 2: テストが失敗することを確かめる**

Run: `cargo test -p eml_core_ir --test integration evidence::`
Expected: `evidence` がないのでコンパイルが通らない。

- [ ] **Step 3: 実装する**

- `builder.rs` を移し、`translate` からは `crate::builder::FnBuilder` で使う
- `evidence/mod.rs` の `evidence`
  1. 変換の前の関数の数 n を覚える
  2. Task 8 で handle、`perform`、`resume`、`mask` の変換を入れる (この段では何もしない)
  3. 番号が n より小さい関数のうち、`yields` で `clause` の印のない関数に `split` をかける。`Annotations` はこの段では空である
- `boxing.rs` の `tail_position_call` を `pub(crate) fn tail_position_call(block: &Block) -> Option<(usize, &Call)>` (呼び出しの文の位置と呼び出し) にし、boxing は今と同じに使う。`never` の `perform` と `never` の操作の `clause` を除くのも今と同じである
- `split.rs` の `split`
  1. yield の地点を集める。ブロックの順、文の順に見て、`Stmt::Let { rhs: Rhs::Call { call, .. } }` のうち、次のどれかで、末尾の位置の呼び出し (`tail_position_call` が返す位置) でなく、`not_yield_points` にないもの。`forced` にあるものは相手によらず地点にする
     - `Call::Direct(g, _)` で g が `yields`
     - `Call::Apply { yields: true, .. }`
     - `Call::Clause { .. }`
  2. 生存解析 (`liveness::live_in`、`live_after_term`、`step_back`) で、各地点の直後で生きている変数を求める。`masked` の地点では、`evv_set(w0)` の文の後で生きている変数を使う
  3. 元の関数を作り直す。地点のある元のブロックを、地点の前と後で2つ (遅い道を入れて3つ) に分ける。新しいブロックは元のブロックのすぐ後ろに置き、番号を付け直して、辺がすべて番号の大きい方へ向くことを保つ。遅い道は次のとおりで、`masked` の地点なら κ を先に `closure mask$[effects](κ)` で包む

     ```
     let y: int = prim yielding()
     switch y { 0 -> b_fast, _ -> b_slow }
     b_slow:
       let k0: tobj = prim yield_cont_take()
       (masked なら) let k1: tobj = closure "mask$[E]"(k0)
       let c: tobj = closure f$kN(生きている変数)      // なければ &f$kN
       let k2: tobj = closure compose$(k1 か k0, c)
       let u: unit = prim yield_cont_set(k2)
       return <ダミー>
     b_fast:
       (masked なら evv_set(w0) の文をここに残し、その後に) 元のブロックの残り
     ```
  4. 地点ごとに `f$kN` を作る。名前は `{f の名前}$k{N}`、N は 0 から、地点の順に振る。変数の表は f をそのまま写し (変数の番号を変えないので、R5 はそのまま成り立つ)、新しい変数は表の末尾に足す。引数は「地点の後で生きている変数 (地点の結果を除き、番号の順)」と地点の結果である。本体は区間の写しで、区間は地点の直後 (`masked` なら `evv_set(w0)` の後) から辺をたどり、次で止める
     - 次の yield の地点 q: 呼び出しと確認を同じ形で置き、遅い道は `f$kq` を足して返し、速い道は `let r = call f$kq(..)` と `return r` にする
     - 合流のブロック J への `jump bJ(args)`: `let r = call f$jJ(J の入口で生きている変数 (J の引数を除き番号の順), args)` と `return r`。`f$jJ` を作っていなければ作り、本体は J からの区間の写しである
  5. `f$kN` と `f$jJ` は `internal`、`yields` で、`ret` は f と同じである。作った関数は、もう `split` にかけない
  6. ダミーは、`ret` が `obj` か `tobj` なら `let d: obj = prim dummy()` と `return d`、`int` なら `return 0`、`enum` なら `return #0`、`unit` なら `return ()` にする
- `evidence/helpers.rs`: 補助の関数を、最初に使うときに `program.functions` の末尾に足す。`internal` にせず、`yields` を立てる。形は次の IR のとおりにする (変数の番号は作る順で変わってよい)

  ```
  yields fn id$(x.0: tobj) -> tobj {
    return x.0
  }

  yields fn compose$(k.0: tobj, c.1: tobj, x.2: tobj) -> tobj {
    let r.3: tobj = yields apply k.0(x.2)
    let y.4: int = prim yielding()
    switch y.4 { 0 -> b2, _ -> b1 }
  b1:
    let k.5: tobj = prim yield_cont_take()
    let k.6: tobj = closure compose$(k.5, c.1)
    let u.7: unit = prim yield_cont_set(k.6)
    let d.8: obj = prim dummy()
    return d.8
  b2:
    let t.9: tobj = yields apply c.1(r.3)
    return t.9
  }

  yields fn prompt$(ev.0: obj, ret.1: tobj, x.2: tobj) -> tobj {
    let w.3: tobj = prim ev_evv(ev.0)
    let u.4: unit = prim evv_set(w.3)
    let s.5: int = prim yield_status(ev.0)
    switch s.5 { 0 -> b1, 1 -> b2, 2 -> b3, _ -> b4 }
  b1:
    let st.6: tobj = prim ev_take_state(ev.0)
    let r.7: tobj = yields apply ret.1(x.2, st.6)
    return r.7
  b2:
    let k.8: tobj = prim yield_cont_take()
    let c.9: tobj = prim yield_finish()
    let st.10: tobj = prim ev_take_state(ev.0)
    let k.11: tobj = closure resume$(ev.0, ret.1, k.8)
    let r.12: tobj = yields apply c.9(k.11, st.10)
    return r.12
  b3:
    let k.13: tobj = prim yield_cont_take()
    let u.14: unit = drop k.13
    let c.15: tobj = prim yield_finish()
    let st.16: tobj = prim ev_take_state(ev.0)
    let r.17: tobj = yields apply c.15(st.16)
    return r.17
  b4:
    let st.18: tobj = prim ev_take_state(ev.0)
    let k.19: tobj = prim yield_cont_take()
    let k.20: tobj = closure rewind$(ev.0, ret.1, st.18, k.19)
    let u.21: unit = prim yield_cont_set(k.20)
    let d.22: obj = prim dummy()
    return d.22
  }

  yields fn resume$(ev.0: obj, ret.1: tobj, k.2: tobj, v.3: tobj, s.4: tobj) -> tobj {
    let e.5: obj = prim ev_renew(ev.0, s.4)
    let w.6: tobj = prim evv_get()
    let w.7: tobj = prim evv_push(w.6, e.5)
    let u.8: unit = prim evv_set(w.7)
    let x.9: tobj = yields apply k.2(v.3)
    let r.10: tobj = call prompt$(e.5, ret.1, x.9)
    return r.10
  }

  yields fn rewind$(ev.0: obj, ret.1: tobj, s.2: tobj, k.3: tobj, v.4: tobj) -> tobj {
    let r.5: tobj = call resume$(ev.0, ret.1, k.3, v.4, s.2)
    return r.5
  }

  yields fn "mask$[State]"(k.0: tobj, v.1: tobj) -> tobj {
    let w.2: tobj = prim evv_get()
    let m.3: tobj = prim evv_mask[State](w.2)
    let u.4: unit = prim evv_set(m.3)
    let r.5: tobj = yields apply k.0(v.1)
    let y.6: int = prim yielding()
    switch y.6 { 0 -> b2, _ -> b1 }
  b1:
    let k.7: tobj = prim yield_cont_take()
    let k.8: tobj = closure "mask$[State]"(k.7)
    let u.9: unit = prim yield_cont_set(k.8)
    let d.10: obj = prim dummy()
    return d.10
  b2:
    let u.11: unit = prim evv_set(w.2)
    return r.5
  }
  ```

  `prompt$` の b2〜b4 で `x.2` は使われず、Perceus が手放す。`x.2` はそのときダミーなので、手放しても何も起こらない
- `pipeline.rs`: `Pass::Evidence` を `Translate` と `Boxing` の間に足し、`name` を `"evidence"` にする。`check` は `Pass::Evidence` で `verify_evidence` を呼ぶ。`lower_until` の流れにはまだ入れない (Task 8)

- [ ] **Step 4: テストを流す**

Run: `cargo test`
Expected: すべて通る。既存の期待値は変わらない。

- [ ] **Step 5: lint とコミット**

```bash
cargo clippy --all-targets && cargo fmt
git add -A crates
git commit -m "Split yielding functions into continuation functions in a new evidence pass"
```

本文に「translate の FnBuilder を crate の共有の builder に移した (機械的な追随)」と書く。

---

### Task 8: handler の変換と差分テスト

spec の「handle と `prompt$`」「再開と状態」「`perform` と節の3つの形」「`never` の中断」「`mask`」「差分テストと段の終わり」の前半。既定の lower は、まだ EP を通さない。

**Files:**
- Create: `crates/eml_core_ir/src/evidence/handlers.rs` (節の分類と入口の関数、handle、`perform`、`resume`、`mask` の変換)
- Modify: `crates/eml_core_ir/src/evidence/mod.rs` (変換と `split` をつなぐ)
- Modify: `crates/eml_core_ir/src/pipeline.rs` (`lower_until` は `last == Pass::Evidence` のときだけ EP を通す。一時的な `pub fn lower_with_evidence`)
- Modify: `crates/eml_core_ir/src/lib.rs` (`pub use pipeline::lower_with_evidence`)
- Modify: `crates/eml_cli/src/lib.rs` (一時的な `Session::compile_with_evidence`)
- Modify: `crates/eml_cli/tests/ui.rs` (`ui_root`、`load`、`classify`、`Entry` を `pub(crate)` に。機械的な追随)
- Create: `crates/eml_cli/tests/ui_evidence.rs` (`tests/main.rs` に宣言する)
- Create: UI テスト `tests/ui/run/effects/resume_inside_own_handler.em`、`never_in_function_value.em`、`continuation_in_outer_state.em`、`resume_under_another_handler.em`、`tests/ui/run/files/files_released_by_abort_across_frames.em`
- Test: `crates/eml_core_ir/tests/evidence.rs`

**Interfaces:**
- Consumes: Task 7 の `evidence`、`split`、`Annotations`、`Helpers`
- Produces:
  ```rust
  // eml_core_ir (Task 9 で lower_without_evidence に置き換える)
  pub fn lower_with_evidence(hir: &HirProgram, typed: &TypedProgram, entry: FunctionId, files: &SourceFiles) -> Program;
  // eml_cli (Task 9 で compile_without_evidence に置き換える)
  impl Session { pub fn compile_with_evidence(&self) -> Compiled }
  ```

- [ ] **Step 1: 失敗するテストを書く**

`tests/evidence.rs` に、ソースから EP の直後までを見るテストを足す (`core_text(text, Pass::Evidence)`)。

```rust
#[test]
fn a_handle_installs_evidence_and_calls_prompt() {
    let text = "effect Ask where\n  ask : Unit -> Int\n\nmain : Unit -> <IO> Unit\nmain () =\n  let r =\n    handle ask () + 1 with\n      | ask () k -> k 41\n  println (show r)";
    let shown = core_text(text, Pass::Evidence);
    insta::assert_snapshot!(function(&shown, "main"), @"…");
}

#[test]
fn an_immediate_clause_becomes_its_entry() {
    // `| ask () k -> k 41` の節の関数は、`internal yields clause Ask.ask fn main$handle0$ask(e: obj, u: unit) -> int` の形になり、
    // 本体は `prim ev_take_state`、`prim ev_set_state`、`return 41` を持つ。resume はない
}

#[test]
fn a_clause_that_does_not_resume_at_once_yields_to_its_handler() {
    // `| ask () k -> let v = k 10 in v * 2` の入口は `closure main$handle0$ask(..)`、`prim yield_to(e, c, &id$)`、ダミーを返す
}

#[test]
fn a_clause_that_performs_another_effect_is_not_run_in_place() {
    // 節の本体が外の handler の操作を呼ぶ (yield の地点を持つ) と、すぐに再開する節でも yield_to の入口になる
}

#[test]
fn a_never_clause_aborts() { /* `yield_abort` の入口。ret は obj で prim dummy() を返す */ }

#[test]
fn a_perform_looks_up_evidence_and_calls_its_clause() {
    // `let ev = prim ev_lookup[Ask]()`、`prim ev_clause[Ask.ask](ev)`、`clause Ask.ask c(ev, ())`
}

#[test]
fn a_masked_call_restores_the_evidence_list_and_wraps_the_continuation() {
    // spec の「mask」の形。遅い道は `closure "mask$[State]"(k0)` を compose$ の内側に置き、
    // 速い道の先頭に `prim evv_set(w0)` がある。f$kN の引数に w0 がない
}

#[test]
fn a_resume_becomes_an_apply() { /* `resume k(v, s)` が `yields apply k(v, s)` になる */ }
```

`@"…"` と各テストのダンプは、Step 4 で spec の形と比べてから受け入れる。

UI テストを5つ書く。どれも、今の CEK で走らせた出力を確かめてから期待値にする。

```haskell
-- tests/ui/run/effects/resume_inside_own_handler.em
-- A continuation is resumed inside the extent of its own handler; each run of the handler keeps its own state.
data Re = | Re (Re -> Int -> Int)

effect G where
  multi grab : Unit -> Re
  get : Unit -> Int
  put : Int -> Unit

body : Unit -> <G> Int
body () =
  let r = grab ()
  let n = get ()
  if n == 0 then
    put 1
    match r with
      | Re k ->
        let inner = k r 100
        inner + get ()
  else n

run : Unit -> Int
run () =
  handle body () from 0 with
    | grab () k st -> k (Re k) st
    | get () k st -> k st st
    | put n k _ -> k () n
    | return x _ -> x

main : Unit -> <IO> Unit
main () = println (show (run ()))
```

期待する出力は `101` である。

```haskell
-- tests/ui/run/effects/never_in_function_value.em
-- A function that fails with a `never` operation is passed as a value and fails inside `mapl`.
effect Fail where
  never fail : String -> a

check : Int -> <Fail> Int
check n = if n < 0 then fail "neg" else n

mapl : (a -> <e> b) -> List a -> <e> List b
mapl f xs =
  match xs with
    | [] -> []
    | x :: rest -> f x :: mapl f rest

main : Unit -> <IO> Unit
main () =
  let r =
    handle mapl check [1, -2, 3] with
      | fail _ -> []
      | return x -> x
  println (show r)
```

期待する出力は `[]` である。

```haskell
-- tests/ui/run/effects/continuation_in_outer_state.em
-- The continuation of an inner handler is kept in the state of an outer handler and resumed later.
data R =
  | Done Int
  | Paused (Int -> R)

effect Ask where
  multi ask : Unit -> Int

effect Keep where
  stash : R -> Unit
  fetch : Unit -> R

step : Unit -> R
step () =
  handle Done (ask () + 1) with
    | ask () k -> Paused k

body : Unit -> <Keep> Int
body () =
  stash (step ())
  match fetch () with
    | Paused k ->
      match k 41 with
        | Done n -> n
        | Paused _ -> 0
    | Done n -> n

main : Unit -> <IO> Unit
main () =
  let r =
    handle body () from Done 0 with
      | stash v k _ -> k () v
      | fetch () k st -> k st st
      | return x _ -> x
  println (show r)
```

期待する出力は `42` で、`debug_heap` がリークを報告しない。

```haskell
-- tests/ui/run/effects/resume_under_another_handler.em
-- A continuation captured by one handler is resumed after that handle returns, inside another handler of the same effect.
effect Ask where
  multi ask : Unit -> Int

data Saved =
  | Saved (Int -> Int)
  | Value Int

capture : Unit -> Saved
capture () =
  handle Value (ask () + ask ()) with
    | ask () k -> Saved k

main : Unit -> <IO> Unit
main () =
  match capture () with
    | Saved k ->
      let r =
        handle k 1 with
          | ask () k2 -> k2 100
      println (show r)
    | Value n -> println (show n)
```

`capture` の handle の結果の型は `Saved` なので、`k : Int -> Saved` であり、`k 1` も `Saved` を返す。型が合わなければ、`k` を再開した結果を `Saved` から取り出す形に直す。期待する出力は、今の CEK で走らせた値である (deep handler なので、`k 1` の中の2つ目の `ask` は、捕まえた handler が `Saved k'` にして返す)。

```haskell
-- tests/ui/run/files/files_released_by_abort_across_frames.em
-- A `never` operation aborts through several frames that each hold a file; every file is released once.
effect Fail where
  never fail : String -> a

deep : Int -> <Fail, IO> Int
deep n =
  let f = Fs.open "input.txt"
  let r = if n == 0 then fail "stop" else deep (n - 1) + 1
  Fs.close f
  r

main : Unit -> <IO> Unit
main () =
  let r =
    handle deep 3 with
      | fail msg -> 0
  println (show r)
```

期待する出力は `0` で、`debug_heap` がリークを報告しない。

`crates/eml_cli/tests/ui_evidence.rs` を書く。

```rust
//! S7 の開発中だけの差分テスト。EP を通す経路と通さない経路で、run と run-fail の出力が一致することを確かめる
//! (docs/superpowers/specs/2026-10-11-s7-evidence-passing-design.md の「差分テストと段の終わり」)。

use std::path::{Path, PathBuf};
use eml_cli::{Compiled, OutputSink, RunConfig};
use crate::ui::{Entry, classify, load, ui_root};

fn outcome(compiled: Compiled, dir: &Path) -> (String, Result<(), String>) {
    let program = compiled.program.expect("the test compiles");
    let (sink, captured) = OutputSink::capture();
    let config = RunConfig::default().with_debug_heap(true).with_file_root(dir.to_path_buf());
    let result = eml_cli::execute(&program, &config, sink).map(|_| ()).map_err(|error| error.to_string());
    (captured.contents(), result)
}

fn tests(top: &str) -> Vec<(PathBuf, String)> { /* ui_root()/top の下の .em をたどり、classify が Test を返すものを集める */ }

#[test]
fn every_run_test_has_the_same_output_with_evidence_passing() {
    for top in ["run", "run-fail"] {
        for (path, suffix) in tests(top) {
            let session = load(&path, &suffix);
            let dir = path.parent().unwrap();
            let direct = outcome(session.compile(), dir);
            let passed = outcome(session.compile_with_evidence(), dir);
            assert_eq!(passed, direct, "{suffix}");
        }
    }
}
```

`OutputSink::capture` などの名前は、今の `ui.rs` の `compile_and_execute` の書き方に合わせる。

- [ ] **Step 2: テストが失敗することを確かめる**

Run: `cargo test -p eml_core_ir --test integration evidence:: && cargo test -p eml_cli --test integration ui_evidence`
Expected: handle などがまだ変換されないので、`verify_evidence` か、コンパイルで失敗する。

- [ ] **Step 3: 実装する**

`evidence/handlers.rs` に、次をこの順に行う関数を置き、`evidence` の手順2から呼ぶ。

1. **節の分類と入口**: すべての関数から `Call::Handle` を探し、節の値 c_i を、同じ関数の `&g` か `closure g(caps)` の文から読む (なければ内部の誤りとして panic)。各 g を spec の「`perform` と節の3つの形」の条件で分類する。g の引数は「捕獲、操作の引数、k、状態」の順である
   - すぐに再開する節: g をその場で書き換える。引数を「捕獲、新しい変数 `e: obj`、操作の引数」にし、入口のブロックの先頭に `let st = prim ev_take_state(e)` を置き (st は元の状態の引数の変数)、末尾の `let r = resume k(v, s)` と `return r` を `let u = prim ev_set_state(e, s)` と `return v` にする。`ret` を v の Repr にし、`clause` の印を `(E, op)` にし、名前に `$entry` を付ける。入口の関数は g そのものなので、handle の場所の c_i をそのまま使える
   - 再開する節と `never` の節: 新しい関数 `{g の名前}$entry` を足す。引数は「捕獲 (g の捕獲と同じ Repr)、`e: obj`、操作の引数 (操作の表の Repr)」、`ret` は操作の `ret` (`Never` なら `obj`) で、`clause` の印を付け、`yields` は g の印を写す。本体は `let c = closure g(捕獲, 操作の引数)` (どちらもなければ `&g`)、`prim yield_to(e, c, &id$)` (`never` なら `yield_abort`)、ダミーの `return` である。handle の場所では、c_i の代わりに `closure g$entry(caps)` (捕獲がなければ `&g$entry`) を渡す
   - 「本体に yield の地点がない」は、Task 7 の yield の地点の規則を g に当てて決める。ただし g の中の `resume` は、最後の末尾の `resume` のほかがあれば、この節は当たらない
2. **handle**: spec の「handle と `prompt$`」の形に書き換える。`apply body(())` の位置を `Annotations::not_yield_points` に入れる。`apply` の印は立てる
3. **perform**: `prim ev_lookup[E]()`、`prim ev_clause[E.op](ev)`、`clause E.op c(ev, args)` にする。束縛の変数はそのまま使う
4. **resume**: `Call::Apply { callee: k, args: vec![v, s], yields: その関数の印 }` にする
5. **mask**: `mask` を持つ `Rhs::Call` を、spec の「`mask`」の形にし、呼び出しの位置を `Annotations::masked` (エフェクトの多重集合) と `forced` に入れる。`Term::TailCall` は translate の出力にない
6. 書き換えた関数ごとに、`yields` なら `split` に `Annotations` を渡す。`yields` でない関数は分けない

`pipeline.rs`:

```rust
pub fn lower_until(hir, typed, entry, files, last: Pass) -> Program {
    // last が Evidence のときだけ EP を通して止める。それ以外は、まだ EP を通さない
}

/// S7 の差分テストのための一時的な入口。EP を通して最後まで変換する。
pub fn lower_with_evidence(hir, typed, entry, files) -> Program { /* translate、evidence、boxing、contract、perceus と各段の check */ }
```

`eml_cli::Session::compile_with_evidence` は `self.compile_with(eml_core_ir::lower_with_evidence)` である。`#[doc(hidden)]` を付け、一時的であることをコメントに書く。

- [ ] **Step 4: テストを流す**

Run: `cargo test`
Expected: すべて通る。差分テストが全 UI テストで一致する。一致しないテストがあれば、EP の誤りとして直す (CEK の出力を正とする)。`evidence.rs` のスナップショットを spec の形と比べて受け入れる。新しい UI テストの出力を期待どおりか確かめて受け入れる。

- [ ] **Step 5: lint とコミット**

```bash
cargo clippy --all-targets && cargo fmt
git add -A crates tests/ui
git commit -m "Lower handlers, operations, resumptions and masks in the evidence pass"
```

本文に「既定の lower はまだ EP を通さない。一時的な lower_with_evidence と差分テストで、全 UI テストの出力が一致することを確かめた。ui.rs の補助を pub(crate) にしたのは機械的な追随」と書く。

---

### Task 9: EP を常に通し、新しい指標を入れる

spec の「`RunStats`」と、「差分テストと段の終わり」の切り替え。

**Files:**
- Modify: `crates/eml_core_ir/src/pipeline.rs` (`lower_until` がいつも EP を通す。`lower_with_evidence` を `lower_without_evidence` に置き換える)
- Modify: `crates/eml_core_ir/src/lib.rs`、`crates/eml_cli/src/lib.rs` (`compile_with_evidence` を `compile_without_evidence` に)
- Modify: `crates/eml_cli/tests/ui_evidence.rs` (比べる2つの経路を入れ替える)
- Modify: `crates/eml_interp/src/lib.rs` (`RunStats`)、`runtime.rs` (`stats`、`handler_visits` を消す)、`effects.rs` (`handler_visits` の加算を消す)
- Modify: `crates/eml_interp/tests/scaling.rs`、`tests/bench.rs`
- Test: translate より後の段のダンプを持つ `crates/eml_core_ir/tests/*.rs`、`crates/eml_core_ir/tests/evidence.rs`

**Interfaces:**
- Consumes: Task 6 のカウンタ、Task 8 の EP
- Produces: `RunStats { evidence_steps, yields, yield_frames, string_bytes_copied, rc_increments, rc_decrements, boxes, unboxes, peak_objects }`

- [ ] **Step 1: 失敗するテストを書く**

`scaling.rs` の `handler_visits` を使う6つのテストを、次の形に書き直す (期待値の変更)。

```rust
/// 末尾でない再帰の深さ d の底で、すぐに再開する操作を同じ回数行う。探す数も yield も深さによらない
/// (docs/superpowers/specs/2026-10-11-s7-evidence-passing-design.md の「RunStats」)。
#[test]
fn immediate_operations_cost_the_same_at_any_depth() {
    let at = |depth: u64| {
        let text = format!("effect Counter where\n  get : Unit -> Int\n  put : Int -> Unit\n\ndown : Int -> <Counter> Int\ndown n = if n == 0 then\n    put (get () + 1)\n    put (get () + 1)\n    get ()\n  else down (n - 1) + 0\n\nmain : Unit -> <IO> Unit\nmain () =\n  let (r, _) =\n    handle down {depth} from 0 with\n      | get () k st -> k st st\n      | put n k _ -> k () n\n      | return x st -> (x, st)\n  println (show r)\n");
        stats(&text, "2\n")
    };
    let (shallow, deep) = (at(10), at(1000));
    assert_eq!(shallow.evidence_steps, deep.evidence_steps);
    assert_eq!((shallow.yields, shallow.yield_frames), (0, 0));
    assert_eq!((deep.yields, deep.yield_frames), (0, 0));
}

/// 再開する操作の yield は、間のフレームの数に比例して継続を組み立てる。方式の性質としてここに書く。
#[test]
fn a_resuming_operation_builds_a_frame_per_level() {
    // 同じ形で、底で `ask` を1回行い、節を `| ask () k -> k 1 + 0` (すぐに再開しない形) にする
    // deep.yield_frames - shallow.yield_frames が深さの差 (990) 以上であることを確かめる
}

#[test]
fn a_program_without_operations_or_strings_does_no_counted_work() { /* evidence_steps、yields、yield_frames も 0 */ }

#[test]
fn every_operation_looks_up_its_evidence() { /* 旧 every_operation_visits_at_least_its_handler。evidence_steps >= 操作の回数 */ }

#[test]
fn a_perform_under_one_handler_looks_at_one_evidence() { /* 旧 a_perform_under_one_handler_visits_only_that_handler。evidence_steps <= n */ }

#[test]
fn a_perform_passes_an_inner_evidence_of_another_effect_once() { /* evidence_steps <= 2n */ }

#[test]
fn a_perform_in_a_masked_callback_skips_the_inner_evidence_once() { /* evidence_steps は mask の写しを含めて 4n 以下 */ }

#[test]
fn immediate_operations_under_deep_recursion_keep_the_peak_linear() {
    // peaks で n=1000 と 2000 を比べ、長い方が短い方の 2 倍 + 定数を超えない
}

#[test]
fn a_function_value_loop_under_a_handler_keeps_the_heap_flat() {
    // `loop f n = if n == 0 then 0 else f (n - 1)` と `go n = loop go n` を handler の本体で回す。peaks の2つが等しい
}
```

`concatenation_writes_the_bytes_of_both_sides` は、`handler_visits` の比較を消すだけにする。

`evidence.rs` に足す。

```rust
#[test]
fn a_pure_instance_of_a_row_polymorphic_function_has_no_yield_check() {
    // Task 3 の twice の例を Pass::Evidence で見て、`twice` に `prim yielding` がなく、`twice<e>` にある
}
```

- [ ] **Step 2: テストが失敗することを確かめる**

Run: `cargo test -p eml_interp --test integration scaling::`
Expected: 新しいフィールドがないので失敗する。

- [ ] **Step 3: 実装する**

- `pipeline.rs`: `lower_until` を translate、evidence、boxing、contract、perceus の順にし、各段の後に `check` をかける。`lower_with_evidence` を消し、一時的な `lower_without_evidence` (EP を飛ばす今までの経路) を足す。`eml_cli` の一時的なメソッドも `compile_without_evidence` に置き換え、差分テストは `compile()` と `compile_without_evidence()` を比べる
- `RunStats`: `handler_visits` を消し、`evidence_steps`、`yields`、`yield_frames` を先頭に足す。`Runtime::stats` が Task 6 のカウンタを写す。`find_handler` の `handler_visits` の加算を消す
- `bench.rs` の6つのスナップショットを受け入れ直す

- [ ] **Step 4: テストを流す**

Run: `cargo test`
Expected: すべて通る。translate より後の段のダンプのうち、handler を使うプログラムと `yields` の instance を含むプログラムだけが変わっていることを読んで受け入れる。`bench.rs` の `state` で、`evidence_steps` が操作の回数と同じ桁で、`yields` が 0 であることを確かめる。

- [ ] **Step 5: lint とコミット**

```bash
cargo clippy --all-targets && cargo fmt
git add -A crates
git commit -m "Always run the evidence pass and count evidence lookups and yields"
```

本文に「EP を常に通すようにした。translate より後の段のダンプ、bench の RunStats、handler_visits を使う scaling テストの書き直しは、spec の期待値の変更の範囲である」と書く。

---

### Task 10: エフェクトを直接扱う仕組みを消す

spec の「CEK とランタイム」「verifier」の段の終わりの部分と、「テストの変更」の「成否の変更」。

**Files:**
- Modify: `crates/eml_runtime/src/heap.rs` (`Frame::Handler`、`Frame::Mask`、`Attachment`、`Payload::Continuation`、継続のための `take_or_copy` と `copy_segment`、`segment_frames`、`set_next`、`chain_link`、`remap`)、`src/lib.rs` (`Attachment` の再公開)
- Modify: `crates/eml_runtime/src/heap/tests.rs` (spec の表の16のテスト)
- Delete: `crates/eml_interp/src/effects.rs`
- Modify: `crates/eml_interp/src/runtime.rs` (`handlers`、`pop_chain`、`call` の `Handle`、`Perform`、`Resume` の腕、`mask` の `Frame::Mask`、`ret` の `Handler` と `Mask` の腕)、`lib.rs`
- Modify: `crates/eml_core_ir/src/verify.rs`、`boxing.rs`、`contract.rs`、`perceus.rs` (範囲の段と所有の段の `handle`、`perform`、`resume`、`mask` の規則)
- Modify: `crates/eml_core_ir/src/pipeline.rs`、`lib.rs`、`crates/eml_cli/src/lib.rs` (一時的な関数とメソッドを消す)
- Delete: `crates/eml_cli/tests/ui_evidence.rs` (`tests/main.rs` の宣言も)。`ui.rs` の `pub(crate)` を戻す
- Test: spec の「成否の変更」の表のテスト

**Interfaces:**
- Consumes: Task 9 までのすべて
- Produces: `Frame { Return, Apply, Root }`。範囲の段と所有の段は `handle`、`perform`、`resume`、空でない `mask` を拒む

- [ ] **Step 1: 成否の変更のテストを直す**

spec の「成否の変更」の表の順に、各テストを表の「変更」のとおりにする。

- `verify.rs`: 範囲の段と所有の段で `perform`、`resume`、`handle` を確かめるケースを消し、Task 4 で足した `clause` と `prim` のケースが同じ規則を受け持つことを確かめる。変換の段に残る規則のテスト (節の数、節の引数の数、`perform` の操作、`resumable`、`mask` の順と番号、`handle` と `perform` の `mask`) は `check_translated` で確かめる形に移し、ヘルパー `handler_program` も変換の段に合わせる。`handlers_operations_and_resume_are_calls` を消す。`the_translated_level_rejects_tail_calls` の範囲の段の部分を消す
- 範囲の段と所有の段が拒むことを確かめるテストを足す: `the_scope_level_rejects_effect_instructions` (`a perform remains after the evidence pass` など、Task 4 の `Evidence` の段と同じ文言)
- `contract.rs`、`perceus.rs`、`eml_interp` の `run.rs` と `closures.rs` と `tests/ir/mask.core`、`heap/tests.rs` を、表のとおりに消すか書き直す。`run.rs` の `a_mask_skips_outer_handlers_only` と `closures.rs` の `an_inner_handle_returns_through_each_resumption_of_an_outer_multi_operation` は、spec のとおり EP の出力の形の IR に書き直す。`tests/ir/mask.core` は、同じ意味のソースのプログラムを `core_until(.., Pass::Perceus)` で変換し、`pretty` で書き出した IR に置き換えてよい。手で書き直すより確かで、EP の出力の形にもなる
- `heap/tests.rs` の `take_or_copy_refuses_data_strings_and_files` は、継続を作る部分を消して残す (`take_or_copy` はクロージャのために残る)

- [ ] **Step 2: テストが失敗することを確かめる**

Run: `cargo test -p eml_core_ir --test integration verify::`
Expected: `the_scope_level_rejects_effect_instructions` が失敗する (範囲の段がまだ受け付ける)。

- [ ] **Step 3: 仕組みを消す**

- `verify.rs`: `Level::Scopes` 以上でも、`Evidence` の段の拒否を当てる。`check_operands` と `check_result` から `Perform`、`Resume`、`Handle` の腕を消し、`unreachable!` でなく、上の拒否が先に止める形にする
- `boxing.rs`: `Converter::call` の `Handle`、`Perform`、`Resume` の腕と、`never` の `perform` の扱いを消す (入力に現れたら `unreachable!("{} reaches the boxing pass", ..)`)。`tail_position_call` の `never` の `perform` の腕を消す
- `contract.rs`: `tail_call` の `Perform`、`Resume`、`Handle` の腕を消す
- `eml_interp`: `effects.rs` を消し、`runtime.rs` の `call` の3つの腕は `Fault::Internal("an effect instruction reaches the interpreter")` にする。`mask` は空でなければ同じ内部の誤りにする。`handlers` と `pop_chain` を消す
- `eml_runtime`: `Frame::Handler`、`Frame::Mask`、`Attachment`、`Payload::Continuation` と、それだけが使う関数を消す。`take_or_copy` はクロージャだけを受け付ける
- 一時的なもの (`lower_without_evidence`、`compile_without_evidence`、`ui_evidence.rs`) を消し、`ui.rs` の可視性を戻す

- [ ] **Step 4: テストを流す**

Run: `cargo test`
Expected: すべて通る。UI テストの出力は変わらない。

- [ ] **Step 5: lint とコミット**

```bash
cargo clippy --all-targets && cargo fmt
git add -A crates
git commit -m "Remove direct effect handling from the later passes and the interpreter"
```

本文に、spec の「成否の変更」の表のとおりにテストを消したり書き直したりしたことと、その理由 (後段のパスと CEK が `handle`、`perform`、`resume` を受け付けなくなったため) を書く。

---

### Task 11: 文書とベンチマーク

spec の「文書の更新」。

**Files:**
- Modify: `docs/spec/core-ir.md`、`docs/spec/effects.md`、`docs/spec/expressions.md`、`docs/spec/types.md`、`docs/spec/runtime.md`
- Modify: `docs/implementation/architecture.md`、`testing.md`、`status.md`、`benchmarks.md`
- Modify: `docs/README.md`、`docs/overview.md`、`docs/future/roadmap.md`、`docs/future/multicore.md`
- Delete: `docs/future/evidence-passing.md`
- Modify: `CLAUDE.md`
- Modify: spec を引くコードのコメント (`grep -rn "2026-10-11-s7-evidence-passing-design" crates`) と、spec の「文書の更新」に挙げたコメント
- Delete: `docs/superpowers/specs/2026-10-11-s7-evidence-passing-design.md`、`docs/superpowers/plans/2026-10-11-s7-evidence-passing.md` (最後のステップで、別のコミット)

**Interfaces:**
- Consumes: Task 1〜10 の実装
- Produces: なし

- [ ] **Step 1: ベンチマークを測る**

Run: `cargo build --release -p eml_cli && bench/run.sh`
Expected: 6つのプログラムの `instructions retired` が出る。`docs/implementation/benchmarks.md` に、S6c の記録と同じ形で「S7 (evidence passing)」の節を足し、前の段との差を書く。`state` の差に、すぐに再開する節をその場で実行する効果が出ることを確かめる。

- [ ] **Step 2: 文書を移す**

`yomiyasu:yomiyasu` のスキルを呼んでから書く。spec の「文書の更新」の一覧の各項目を、spec の該当する節から移す。特に次を確かめる。

- `docs/spec/core-ir.md`: パスの表に EP を足す。「位置の規則」の「エフェクトの位置」「`never` の操作」「T3」を `clause` の形に書き直す。末尾呼び出しの保証の文と理由の段落を、spec の「不変条件と末尾呼び出しの保証」のとおりに書き直す。「インタプリタ (CEK 機械)」の `handle`、`perform`、`resume`、`mask` の項目を、evidence と `evv` の基本操作の説明に替える。verifier の段を4つにする
- `docs/future/roadmap.md`: S7 の節を消す。段の列の表から S7 の行を消す。S8 の論点に spec の「S8 への申し送り」を足す。「処理系」に spec の「文書の更新」の残る項目を足す
- `CLAUDE.md`: Core IR のパスの列 (`Pass::{Translate, Evidence, Boxing, Contract, Perceus}`)、verifier の4つの段、`RunStats` の作業の回数の名前と数、`Frame::Root` の説明、`IO` の説明の「`Frame::Root` is the bottom of every continuation」を、今の形に直す
- コードのコメントで spec を引いている所を、移した先の `docs/` の節に直す

- [ ] **Step 3: 引用を確かめる**

Run: `cargo test -p eml_cli --test integration citations && grep -rn "evidence-passing.md\|handler_visits\|Frame::Handler" docs crates CLAUDE.md`
Expected: テストが通り、grep が消した文書と仕組みへの参照を出さない (`docs/superpowers` の下は除く)。

- [ ] **Step 4: lint とコミット**

```bash
cargo clippy --all-targets && cargo fmt --check
git add -A docs CLAUDE.md crates
git commit -m "Move the S7 decisions into the docs and record the benchmarks"
```

- [ ] **Step 5: spec と計画を消す**

```bash
git rm docs/superpowers/specs/2026-10-11-s7-evidence-passing-design.md docs/superpowers/plans/2026-10-11-s7-evidence-passing.md
git commit -m "Delete the S7 design and plan"
```

本文に「決まったことは docs/spec と docs/implementation に移した」と書く。
