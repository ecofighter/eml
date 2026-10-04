# リファクタリング R2b-1: HIR の構造の設計

位置づけ: 作業用の設計文書。R2b-1 を終えたら、残す価値のある内容を `docs/implementation/architecture.md` と `docs/implementation/status.md` に移し、この文書は削除する。

## 目的と範囲

[status.md](../../implementation/status.md) の「R2b データモデル」は、HIR の構造と、組み込みと ID の2つの塊からなる。エフェクトと型構成子を ID で表すには、ID が指す item の置き場所が先に要る。そのため、R2b を R2b-1 (HIR の構造) と R2b-2 (組み込みと ID) に分け、R2b-1 を先に行う。

R2b-1 は `eml_hir` のデータモデルと変換を変え、`eml_types` と `eml_core_ir` を追随させる。リファクタリング全体の方針 (UI テストの出力は原則として変えない、段階3〜5の器の形は作り替えるが機能は実装しない) と、テストの変更の運用 ([testing.md](../../implementation/testing.md)) に従う。

R2b-1 で言語の振る舞いが変わるのは、`|>` の評価順だけである。`x |> f a` で `x` を先に評価するようにする。ユーザーと合意済みである。

## 1. HIR のデータモデル

### `Function` と型の置き場所

今の `Function` は、シグネチャと本体の型の注釈を1つのアリーナ `types` に置き、シグネチャの型変数と row 変数の表 `type_vars` と `row_vars` を持つ。本体を書き換えるとシグネチャのアリーナも変わるので、`hir.rs` のコメントが述べる「関数単位で再計算できるようにする」という目的と合わない。これを次の形にする。

```rust
pub struct Function {
    pub name: String,
    /// 最初の等式の名前の位置。等式がなければシグネチャの名前の位置。
    pub name_range: TextRange,
    /// なければ `None` で、E1004 は報告済み。
    pub signature: Option<Signature>,
    /// 等式がなければ `None` で、E1005 は報告済み。
    pub body: Option<Body>,
}

pub struct Signature {
    pub ty: TypeRefId,
    /// シグネチャの型の範囲。
    pub range: TextRange,
    /// シグネチャの型の注釈。
    pub types: Arena<TypeRef>,
    pub generics: Generics,
}

/// 型変数と row 変数の表。シグネチャが持つ。段階3と4では、`data` とエフェクトの宣言も持つ。
pub struct Generics {
    pub type_vars: Arena<TypeVarDecl>,
    pub row_vars: Arena<RowVarDecl>,
}

pub struct Body {
    pub params: Vec<PatId>,
    pub root: ExprId,
    pub exprs: Arena<Expr>,
    pub pats: Arena<Pat>,
    pub locals: Arena<Local>,
    /// 本体の型の注釈。型変数と row 変数は、シグネチャの `Generics` を指す。
    pub types: Arena<TypeRef>,
}
```

- シグネチャがない関数の本体の注釈は、型変数を持てない。今と同じく、本体の注釈の型変数と row 変数は、シグネチャの表にある名前だけを引ける (docs/spec/types.md の「推論」)。表がなければ、今と同じ E1002 になる。
- 型を読む側は、どのアリーナの `TypeRefId` かを引数で受け取る。
  - `eml_types` の `scheme.rs`: `Rigids::new` は `&Generics` を受け取る。`lower_signature` は `Signature` を、`lower_type` は `&Body` の `types` と `Generics` を使う。
  - `eml_types` の `check/`: `check_main` の `has_error`、`body_arrow_range` は `Signature::types` を、本体の `Annot` は `Body::types` を引く。
  - `eml_hir` の `pretty.rs`: シグネチャは `Signature::types` を、本体の注釈は `Body::types` を引く。

### トップレベルの名前の解決

今は、トップレベルの名前の解決が3か所に分かれている。

- 値: `lower/expr.rs` の `HashMap<String, FunctionId>` と `Builtin::from_name`
- 型: `lower/types.rs` の `BuiltinType::from_name`
- エフェクト: `lower/types.rs` の文字列 `"IO"` との比較

これを、`eml_hir` の変換の中の `ItemScope` (`crates/eml_hir/src/lower/scope.rs`) にまとめる。

- [モジュール](../../spec/modules.md) の名前空間に合わせて、値の表と型の表の2つを持つ。値の表は名前から `ValueItem` (`Function(FunctionId)` か `Builtin(Builtin)`) を、型の表は名前から `TypeItem` (`Builtin(BuiltinType)` か `Effect(EffectRef)`) を引く。
- 組み込みを先に入れ、ユーザーの定義で上書きする。組み込みは名前解決の最も外側のスコープで、ユーザーの定義で隠せる、という今の振る舞いと同じである。
- `ItemScope` は `Module` には出さず、変換の中だけで使う。S2 のモジュールと import で外に出すかを決める。
- 局所変数のスコープ (`BodyLowering::scope`) は今のまま残し、局所変数で見つからなければ `ItemScope` の値の表を引く。
- `Res` は `Local`、`Function`、`Builtin` のままにする。コンストラクタと操作の `Res` は、段階3と4で足す。

R2b-2 では、組み込みの型とエフェクトを item にして、この表を通して引くようにする。

この節の変更は、HIR の pretty の出力も、型検査の結果も、Core IR も変えない。

## 2. HIR の走査関数と `|>` の脱糖

### 走査関数

`Body` に次のメソッドを置く。

| メソッド | 中身 | 使う側 |
|---|---|---|
| `walk_child_exprs(&self, id: ExprId, f: impl FnMut(ExprId))` | 式の直接の子を、ソースの順に `f` に渡す。`Call` は呼ばれるものと引数、`If` は条件と2つの枝、`Block` は各文の式 (`Let` の初期化式を含む) と最後の式、`Annot` は中の式、`Lambda` は本体である | `lambda_captures`。段階3と4で `match` や `handle` を足すとき、子を辿る規則はここだけを直す |
| `pat_bindings(&self, pat: PatId) -> Vec<LocalId>` | パターンが束縛する局所変数。`Annot` の内側も見る | `eml_core_ir` の `binder` と `bind_locals`、`eml_types` の `usage.rs` の `remove_bound` |
| `lambda_captures(&self, lambda: ExprId) -> Vec<LocalId>` | ラムダの本体が参照する局所変数のうち、ラムダの中で束縛していないもの。`LocalId` の順に並べる。入れ子のラムダが捕まえる変数は、外側のラムダも捕まえる | `eml_core_ir` の `captures` を置き換える |

`lambda_captures` の規則と順序は、今の `eml_core_ir` の `captures` と同じにする。そのため、Core IR のスナップショットは変わらない。

`usage.rs` は、捕まえた変数を使用回数と一緒に数えている。この数え方はエフェクトや線形性に固有の規則なので、走査は今のまま残す。ただし、`usage.rs` が求めた捕まえた変数の集合が `lambda_captures` と同じであることを、`debug_assert` で確かめる。2か所で別々に求めている集合が、ずれないようにするためである。

`scc.rs` の `callees` は、本体のアリーナを順に見るだけで済んでいるので、今のままにする。

### `|>` の脱糖

今は `x |> f a` を `f a x` の呼び出しに脱糖しているので、`x` は最後に評価される。パイプラインは左から右へ読むので、エフェクトもその順に起きるほうが自然である。

- `x |> f a` を、`{ let $pipe = x; f a $pipe }` に脱糖する。`$pipe` は変換が作る局所変数である。`$` は識別子に使えない文字なので、ユーザーの名前とは重ならない。
- 作った `let` のパターン、`$pipe` を参照する `Path` の式の範囲は、`x` の範囲にする。ブロックの範囲は、`|>` の式全体の範囲にする。型の誤りは、今と同じく `x` の位置に「argument N of `f`」として出る。
- `x` を変換してから `$pipe` の局所変数を作る。そのため、`1 |> f |> g 2` では内側の `$pipe` が先の番号になる。
- 右辺が呼び出しなら、今の `call` と同じく引数の並びの最後に `$pipe` を足して、1つの呼び出しにする。
- `<|` は今のまま、`f <| x` を `f x` にする。ソースの順 (`f`、`x`) と評価の順がもともと一致している。

spec は次のように直す。

- `docs/spec/declarations.md` の演算子の表の `|>` `<|` の行を、「`x |> f` は `x` を先に評価してから `f` に適用する。HIR で `{ let p = x; f p }` に脱糖する。`f <| x` は関数適用 `f x` に脱糖する」にする。
- `docs/spec/expressions.md` の脱糖の一覧の「`|>` と `<|` の関数適用への脱糖」を、「`|>` の `let` と関数適用への脱糖、`<|` の関数適用への脱糖」にする。

## 3. テスト、文書、成功の条件

### 変わるテスト

| テスト | 種類 | 変更 |
|---|---|---|
| `crates/eml_hir/tests/operators.rs` の `pipes_become_applications` | 2 | `p = 1 \|> f \|> g 2` の HIR が、`$pipe` の `let` を持つ入れ子のブロックになる。`q = g 1 <\| f 2` の行は変わらない。正確な期待値は、実装の後に差分を示して確かめる |
| 各 crate の単体テストと結合テストのうち、`Function::types`、`type_vars`、`row_vars` を直接使っているもの | 3 | フィールドの場所の変更への追随だけ。期待値は変えない |

上の表にないテストの期待値が変わった場合は、変えずに止まり、差分と理由をユーザーに示して承認を得る。

### 足すテスト

- `crates/eml_hir/tests/` に、HIR の構造のテストを足す。
  - `walk_child_exprs` が、`Let` の初期化式とラムダの本体も子として渡すこと
  - `pat_bindings` が、型を明示したパターン `(x : Int)` の内側の束縛を返すこと
  - `lambda_captures` が、入れ子のラムダの捕まえる変数を外側のラムダにも含め、ラムダの中で束縛した変数を含めないこと
  - シグネチャの注釈と本体の注釈が別々のアリーナに入り、本体の注釈の型変数がシグネチャの `Generics` を指すこと
- `tests/ui/run/pipe_evaluation_order.em` を足す。`|>` の左辺と、右辺の引数の両方がエフェクトを起こすプログラムで、左辺が先に動くことを確かめる。

```
-- `x |> f a` evaluates `x` before the arguments of `f a`.
say : String -> <IO> Int
say s =
  println s
  1

add : Int -> Int -> Int
add a b = a + b

main : Unit -> <IO> Unit
main () =
  let n = say "left" |> add (say "right")
  println (show_int n)
```

  stdout は `left`、`right`、`2` の3行になる。今の実装では `right`、`left`、`2` の順になる。

### 文書

| 文書 | 変更 |
|---|---|
| `docs/spec/declarations.md`、`docs/spec/expressions.md` | 2の `\|>` の脱糖 |
| `docs/implementation/architecture.md` | 「`eml_hir` の内部」で、「型変数と row 変数の表は `Function::type_vars` / `row_vars` に置く」と「型の注釈は関数ごとの `types` に置く」を、`Signature::generics` と、シグネチャと本体の2つのアリーナに直す。トップレベルの名前の解決が `ItemScope` の1か所であること、`Body` の走査関数を足す。「`\|>` / `<\|` は関数適用に脱糖する」を、2の脱糖に直す |
| `docs/implementation/status.md` | 「リファクタリング」の表の R2b の行を R2b-1 と R2b-2 の2行にし、R2b-1 を完了にする。表の上の段落の回の数と並びを直す。「R2b データモデル」の一覧を、R2b-1 で済んだものと R2b-2 に残るものに分ける。「完了した作業」に R2b-1 の行を足す |
| `docs/implementation/testing.md` | 「テストの変更の記録」に「リファクタリング R2b-1」の見出しを作り、`pipes_become_applications` の変更と、実装の途中で承認を得て変えたテストを記録する |

### R2b-2 に残すもの

この回では行わない。R2b-2 の spec で扱う。

- 組み込みの名前、fixity、型、Core IR への変換の情報を1つの表にまとめること。組み込みの型を、ユーザーの関数と同じスキームの経路で作ること。`Bool`、`Unit`、`IO`、`main` などを lang item として引くこと
- エフェクトと型構成子を ID で表すこと (`EffectRef::Io`、`Effect::Io`、`TyCon` をやめる)。組み込みの型とエフェクトを item にして、`ItemScope` を通して引くこと
- `TypedModule` がスキームを返すこと、テストの表示のためだけの `kinds` を除くこと、`Type::Var` が rigid な変数と解けなかった変数を区別できること

### 成功の条件

- `Function` が型の注釈と型変数の表を持たず、`Signature` と `Body` がそれぞれの注釈のアリーナを持ち、型変数と row 変数の表は `Signature::generics` にある
- トップレベルの名前の解決が `ItemScope` の1か所になり、`lower/types.rs` に文字列 `"IO"` との比較がない
- 子の式を辿る処理、パターンの束縛、ラムダが捕まえる変数が `Body` のメソッドになり、`eml_core_ir` から `captures` と `bind_locals` が消える
- `x |> f a` が `x` を先に評価し、`pipe_evaluation_order.em` の UI テストが通る
- 変わったテストが、上の「変わるテスト」の表のものと、実装の途中で差分を示して承認を得たものだけである
- `cargo test`、`cargo clippy --all-targets`、`cargo fmt --check` が通る
- 上の「文書」の表の変更が済んでいる
