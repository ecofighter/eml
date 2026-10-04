# リファクタリング R3a: Core IR の形の設計

位置づけ: 作業用の設計文書。R3a を終えたら、残す価値のある内容を `docs/spec/core-ir.md`、`docs/implementation/architecture.md`、`docs/implementation/status.md` に移し、この文書は削除する。

## 目的と範囲

[status.md](../../implementation/status.md) の「R3 Core IR とランタイム」の一覧のうち、Core IR の形にかかわる項目を行う。R3 は2回に分け、R3a で Core IR を、R3b でランタイムとインタプリタを直す。Core IR の命令の形が先に決まると、R3b のフレームの種類がそれに合わせて決まるためである。

R3a で行う項目は次のとおりである。

- `Rhs::Nested` を join point に替え、末尾の位置の `Switch` と末尾呼び出しを入れる
- Perceus の挿入を独立したパスにし、Core IR の不変条件を確かめる verifier を足す
- boxed かどうかの判定を1か所にする
- 入口の関数を Core IR の側で作り、引数のない `main` の扱いをインタプリタから除く
- 組み込みの変換を R2 の表から引く

インタプリタは、新しい命令を実行するのに要る分だけ追随させる。`Frame` の種類の enum、記述子、共有されたオブジェクトの複製、`Owned::refs`、型を付けた `RuntimeError` は R3b で行う。

リファクタリング全体の方針 (UI テストの出力は原則として変えない、段階3〜5の器の形は作り替えるが機能は実装しない) と、テストの変更の運用 ([testing.md](../../implementation/testing.md)) に従う。R3a は言語の観測できる振る舞いを変えない。UI テストの期待値は変わらない。Core IR のスナップショットは変わる (種類2、下の「変わるテスト」)。

次のことは、ユーザーと合意済みである。

- R3 を R3a (Core IR) と R3b (ランタイムとインタプリタ) に分ける
- join point は ANF の木に足す形にする。基本ブロックの CFG にはしない
- verifier は構造と所有権の両方を確かめ、デバッグビルドの `lower` で毎回走らせる
- boxed の判定、組み込みの変換、入口の関数は、下の 3〜5 の形にする

## 1. 制御の命令

### 命令の形

`CExpr` と `Rhs` を次の形にする。

```rust
pub enum CExpr {
    Let { var: VarId, rhs: Rhs, body: CExprId },
    /// `scope` の中の `Jump` が `body` に入る。`param` は `Jump` が渡す値を受ける。
    Join { join: JoinId, param: VarId, body: CExprId, scope: CExprId },
    Switch { scrutinee: Atom, arms: Vec<(u32, CExprId)> },
    Jump { join: JoinId, arg: Atom },
    Return(Atom),
    /// 関数の末尾の呼び出し。呼び出し元のフレームを積まない。
    TailCall(Call),
    Dup { var: VarId, body: CExprId },
    Decref { var: VarId, body: CExprId },
}

pub enum Rhs {
    Atom(Atom),
    Call(Call),
    MakeClosure(FnIdx, Vec<Atom>),
    Prim(PrimOp, Vec<Atom>),
    ConstString(u32),
    Perform(IoOp, Vec<Atom>),
}

pub enum Call {
    /// 呼ぶ相手が分かっていて、引数の個数が揃っている呼び出し。
    Direct(FnIdx, Vec<Atom>),
    /// 関数値の呼び出し。実行時に引数の個数を比べる (eval/apply)。
    Apply(Atom, Vec<Atom>),
}
```

- `Rhs::Nested` と、`Rhs::CallDirect` / `Rhs::Apply` は無くなる。呼び出しは `Rhs::Call(Call)` と `CExpr::TailCall(Call)` で共有する。
- `JoinId` は関数ごとの番号 (`pub struct JoinId(pub u32)`) である。`CoreFn` に、`JoinId` から `Join` の式を引く索引 `joins: Vec<CExprId>` を足す。インタプリタは `Jump` の行き先の本体と引数を、この索引から引く。式のアリーナを作り直すパス (Perceus) は、索引も作り直す。
- join point の引数は1つである。`if` と段階4の `match` は値を1つ返すだけだからである。
- join point は再帰しない (`Jump` は `scope` の中にだけ現れ、`body` の中には現れない)。ループは今の範囲にない。

### 変換の規則

式を変換するとき、値をどこへ渡すか (出口) を持って回る。出口は「関数から返す」(`Return`) か「join point に渡す」(`Jump`) のどちらかである。

- 末尾の `if` (出口が `Return`): 各枝を同じ出口で変換し、枝が直接 `Return` する `Switch` にする。join point は作らない。
- 末尾にない `if`: 続きの束縛と出口を join point の本体にし、`if` の値をその引数で受ける。各枝は出口を `Jump` にして変換する。`if` の条件の中の `if` のように、join point は入れ子になってよい。
- ブロックと注釈の式は、末尾の式に出口を引き継ぐ。
- 出口が `Return` のときの呼び出し (`Direct` と `Apply`) は `TailCall` にする。出口が `Jump` のときは、`let t = 呼び出し` の後に `jump j(t)` にする。
- `else` のない `if` の偽の枝は、出口に `()` を渡す。

今の変換は、束縛を平らな列 (`Bindings`) に積んでから後ろから組み立てる。この列の要素に「join point を開く」を足す。組み立てのとき、それより後ろで組み立てた式を join point の本体にし、`Switch` を `scope` にする。

表示は次の形にする (今の `a_non_tail_if_keeps_strings_used_later` の、Perceus の後の形)。

```
fn pick(b0, s1) {
  join j0(t3) {
    let t4 = prim ++(t3, s1)
    return t4
  }
  switch b0 {
    #0 ->
      let s2 = const "none"
      jump j0(s2)
    #1 ->
      dup s1
      jump j0(s1)
  }
}
```

末尾呼び出しは `tailcall count(t3)` と `tailcall apply c1(2)` のように表示する。

### インタプリタ

- `Jump`: join の引数の変数に値を入れ、join の本体に制御を移す。ヒープにフレームを積まない。
- `TailCall`: フレームを積まずに関数に入る。`Apply` で引数が余るときは、今と同じく余りを持つフレームを積む。下の verifier が「`TailCall` の時点で、所有している参照は残っていない」ことを保証するので、今の環境はそのまま捨ててよい。
- `Rhs::Nested` が無くなるので、環境を退避しないフレームを積む経路 (`push_frame` の `save_env: false`) も無くなる。`Frame::slots` は常に `Some` になるが、型は `Option` のまま残す。R3b で `Frame` を種類の enum にするとき、まとめて直す。

## 2. Perceus のパスと verifier

### パスの流れ

`eml_core_ir::lower` の中を、次の3段にする。

1. 型付き HIR から、RC の命令のない Core IR を作る (`lower.rs`)
2. `perceus::insert(&mut Program)` で、プログラム全体に `dup` / `decref` を挿入する
3. `cfg(debug_assertions)` のとき `verify(&Program)` で不変条件を確かめ、誤りがあればコンパイラの内部の誤りとして panic する

今は関数を1つ変換するたびに `ProgramBuilder::finish` が Perceus を呼んでいる。これをやめ、変換が終わった後にプログラム全体に1回かける。

Perceus は `Join` を次のように扱う。

- join の本体は、引数 (RC の対象なら) と、本体で使う RC の対象の変数を、1つずつ所有して始まる
- `Jump` の前で、join の本体が使わない変数を `decref` する。`Jump` に渡す変数を join の本体でも使うときは、`Jump` の前で `dup` する
- `TailCall` は `Return` と同じく、後で使う変数がない位置として扱う

### verifier

`pub fn verify(program: &Program) -> Result<(), VerifyError>` を足す。`VerifyError` は、関数の名前と誤りの内容を持つ。

構造の検査は次のとおりである。

- 各変数は関数の中で1回だけ束縛する (引数、`Let`、join の引数)
- 変数は束縛の範囲の中でだけ使う
- `Jump` は、外側の `Join` の `scope` の中にだけ現れる。`JoinId` は関数の中で重複せず、`CoreFn::joins` の索引がその `Join` を指す
- `Switch` のタグは重複しない
- `Call::Direct` と `TailCall` の直接呼び出しの引数の数は、呼ぶ関数の引数の数と等しい。`MakeClosure` の引数の数は、関数の引数の数より少ない

所有権の検査は、RC の対象 (`Unr` で boxed の変数) について次を確かめる。

- 関数の入口で、対象の引数を1つずつ所有する
- 使うと所有の数が1減り、`dup` で1増え、`decref` で1減る。所有の数が 0 の変数を使う、`dup` する、`decref` するのは誤りである
- 束縛した対象の変数は、所有の数 1 で始まる
- `Switch` の各枝は、同じ所有の状態から始まる
- `Return` と `TailCall` の時点で、値として渡したもの以外に所有しているものが残っていない
- `Jump` の時点で、渡す値を除き、行き先の join の本体が使う対象の変数を、ちょうど1つずつ所有している

対象でない変数 (`Int`、`Bool`、`Unit` など) は、構造の検査だけを受ける。

`eml_interp` のテストにある手書きの Core IR (わざとリークさせるもの) は `lower` を通らないので、verify されない。

## 3. boxed の判定を1か所にする

今は4か所 (型から決める `new_var`、型を見ない `bind_boxed`、組み込みの引数の表 `builtin_params`、組み込みの結果の `builtin_result_boxed`) にある。これを `lower.rs` の `fn boxed(ty: &Type, lang: &LangItems) -> bool` の1つにする。規則は今と同じで、`String`、関数型、型変数 (`Rigid` と `Flexible`) なら真である。

型を見ずに boxed にしていた変数にも型を与える。

- クロージャを作った結果 (部分適用、ラムダ、関数や組み込みを値として使うもの) は、その式の型 (関数型) を使う
- 余った引数がある呼び出しの途中の結果は、呼ばれる式の型を、呼ぶ関数の引数の数だけたどった残りの型を使う。呼ばれる式の型は `BodyTypes::exprs` にある

組み込みの型は、Prelude から作ったスキームを使う。`eml_types` の `TypedModule` に `pub builtins: HashMap<Builtin, Scheme>` を足し、型検査で作った組み込みのスキームを外に出す (`True` と `False` は含まない)。組み込みを包む関数の引数と結果が boxed かどうかは、このスキームの型をたどって決める。

## 4. 組み込みの変換を R2 の表から引く

- 組み込みの引数の数は `Builtin::arity()` (R2b-2 の `BUILTINS` の表) から引く。
- 引数と結果が boxed かどうかは、上の 3 のスキームから決める。
- 変換の種類は、`eml_core_ir` の網羅的な match 1つに置く。`eml_hir` の表は `PrimOp` を知らないからである。

```rust
enum Lowering {
    Prim(PrimOp),
    Perform(IoOp),
    /// `>>` は `g (f x)`、`<<` は `f (g x)` である。
    Compose { forward: bool },
    Constructor(u32),
}

fn lowering(builtin: Builtin) -> Lowering
```

これで `builtin_params`、`builtin_result_boxed`、`prim` と、`call_builtin` と `wrapper` の中の個別の match が無くなる。

## 5. 入口の関数

- `Program::main` を `Program::entry` に替える。変換は、引数のない関数 `entry$main` を関数の表の最後に足す。
  - `main ()` の形: `tailcall main(())`
  - 引数のない `main = fn () -> ...` の形: `let f = call main()` の後に `tailcall apply f(())`
- インタプリタは、引数なしで `entry` から始める。`main` の引数に `()` を入れる処理と、最初に余りの引数のフレームを積む処理は無くなる。

## 6. テスト、文書、成功の条件

### 変わるテスト

| テスト | 種類 | 変更 |
|---|---|---|
| `crates/eml_core_ir/tests/lower.rs` の9件 (`hello_world`、`strings_are_dupped_and_decreffed`、`shadowed_and_discarded_strings`、`a_non_tail_if_keeps_strings_used_later`、`recursion_and_top_level_values`、`partial_and_extra_arguments_use_closures`、`builtins_used_as_values_are_wrapped`、`lambdas_are_lifted_with_their_captures_first`、`a_zero_arity_callee_is_evaluated_before_its_arguments`) | 2 | すべてに `entry$main` が加わる。末尾にない `if` が join point に、末尾の `if` が直接返す `Switch` に、末尾の呼び出しが `tailcall` になる。`call` と `apply` の表示の形は変えない |
| `crates/eml_interp/tests/run.rs` と `closures.rs` の手書きの Core IR | 3 | `main: FnIdx(..)` を `entry: FnIdx(..)` にし、`Rhs::CallDirect` / `Rhs::Apply` を `Rhs::Call(Call::..)` にし、`CoreFn` に `joins` を足す。期待値は変えない |

UI テスト、HIR と型のスナップショットは変わらない。上の表にないテストの期待値が変わった場合は、変えずに止まり、差分と理由をユーザーに示して承認を得る。

### 足すテスト

- Core IR のスナップショットで、次を確かめる
  - 末尾の `if` が直接返す `Switch` になる
  - `if` の条件の中の `if` が、入れ子の join point になる
  - 末尾の直接呼び出しと `Apply` が `tailcall` になる
  - 引数のない `main` の入口が `call main()` と `tailcall apply` になる
- verifier の単体テストで、正しい Core IR を受け入れ、次の壊れた Core IR を拒むことを確かめる
  - 二重の束縛
  - 範囲の外の変数の使用
  - 範囲の外の `Jump`
  - move の後の使用
  - `decref` の不足と重複
  - `Jump` の時点での所有の過不足
  - 直接呼び出しの引数の数の不一致
- UI テスト (`tests/ui/run/`): 100万回の末尾再帰のループ。今の実装でもヒープのフレームで通るので、回帰を防ぐためのものである。末尾呼び出しになったことは Core IR のスナップショットで確かめる。

### 文書

| 文書 | 変更 |
|---|---|
| `docs/spec/core-ir.md` | 「Core IR」の命令の表に join point、`jump`、末尾呼び出しを足す。Perceus を独立したパスにし、verifier が構造と所有権を確かめることを書く。入口の関数を書く |
| `docs/implementation/architecture.md` | 「`eml_core_ir`、`eml_runtime`、`eml_interp` の内部」の `Rhs::Nested` の記述を join point に直す。Perceus のパスと verifier、boxed の判定、組み込みの変換の表、入口の関数を書く。入れ子の式のフレームが環境を使い続ける記述を除く |
| `docs/implementation/status.md` | 「リファクタリング」の表で R3 を R3a と R3b の2行に分け、R3a を完了にする。「R3 Core IR とランタイム」の一覧から済んだ項目を除き、済んだことを1段落で書く。「完了した作業」に R3a の行を足す |
| `docs/implementation/testing.md` | 「リファクタリング R3a」の見出しを作り、種類2の Core IR のスナップショットの変更を記録する |

### 成功の条件

- `Rhs::Nested` がなく、末尾にない `if` は join point、末尾の呼び出しは `TailCall` になる
- Perceus は変換の後にプログラム全体にかける独立したパスで、デバッグビルドの `lower` が毎回 `verify` を通す
- boxed の判定が `boxed` の1か所にあり、`builtin_params`、`builtin_result_boxed`、`bind_boxed` がない
- 組み込みの変換の種類が `lowering` の1つの match にあり、引数の数は `Builtin::arity()` から引く
- `Program::entry` があり、インタプリタに `main` の引数と最初の余りの引数のフレームを扱う処理がない
- UI テストの期待値が変わらない。変わったスナップショットは上の表のものだけである
- `cargo test`、`cargo clippy --all-targets`、`cargo fmt --check` が通る
- 上の「文書」の表の変更が済んでいる
