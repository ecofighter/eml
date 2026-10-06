# リファクタリング R6: 段階6b の前の継ぎ目の整理の設計

位置づけ: 作業用の設計文書。R6 を終えたら、残す価値のある内容を `docs/spec/`、`docs/implementation/architecture.md`、`docs/implementation/testing.md`、`docs/implementation/status.md` に移し、この文書は削除する。

## 目的と範囲

段階6b (パラメータ付き handler) と S2 に進む前に、互換性とテストの変更を気にせずに、既存のコードベースを見直した。見直しは crate ごとの6つの観点 (`eml_syntax` と `eml_diagnostics`、`eml_hir`、`eml_types`、`eml_core_ir`、`eml_runtime` と `eml_interp` と `eml_cli`、crate をまたぐ構成とテスト基盤) で行った。見つかった改善の候補は、時期で3つに分かれた。

| 時期 | 内容 | 扱い |
|---|---|---|
| 6b の前 | 6b が触る継ぎ目 (評価の順、型の走査、Kind の由来、handler の節の型、診断の順、Core IR と interp の道具) | この R6 |
| 6b の中 | handler の形そのもの (HIR の closure の形、Core IR の `Handle` と `Resume`、ランタイムの handler フレーム) | 6b の spec に回す (3章) |
| S2 の前 | 単一ファイルの前提をなくす作り替え (item の ID、名前解決、組み込み、型検査の出力、source の読み込み、CST の名前) | R7 として別に扱う (3章) |

R6 は2回に分け、R6a → R6b の順に、それぞれ計画と実装のサイクルで進める。A1 が Core IR の変換を変えるので、変換の後のパスと道具は R6a の後に整える。

- R6a 評価の順と型検査: A1、A2、A3、A4、A8 (1章)
- R6b Core IR と interp の道具: A5、A6、A7 (2章)

### 見直しで確かめた不具合

次の4件は、手元で再現して確かめた。

| # | 現象 | 原因 | 直す回 |
|---|---|---|---|
| 1 | `<M.E>` が E0004 にならず、「cannot find effect `M`」(E1002) になる | `ast::Effect::name()` が最初の `UIDENT` を取る。CST に名前の経路のノードがない | R7 |
| 2 | 操作と同じ名前の関数を定義すると、E1003 の後に handler の節で E1001 が連鎖する | `ItemScope` の重複の扱いが箇所ごとに違い、関数が操作を上書きする | R7 |
| 3 | `(f 1) (g ())` で、`g ()` の副作用が `f 1` の副作用より先に起きる | HIR が括弧を書いた呼び出しも1つの呼び出しにまとめ、引数をすべて評価してから矢印を適用する | R6a (A1) |
| 4 | `let y = if c then g x else h x` の後に `y` を返すと、各枝が `let t = call g(x); return t` になり、末尾呼び出しにならない | 末尾呼び出しを作るのが translate だけで、simplify が枝を動かした後に作り直さない | R6b (A7) |

コードを読んで確かめた危うい箇所も2つある。

- `occurs` などの型の走査が `..` で欄を読み飛ばす (`table/unify.rs` の `occurs`)。6b で `Cont` に状態の欄を足すと、occurs の検査からその欄が気づかないうちに漏れる (A2)
- `solve_scc` が、由来のない制約の違反を `filter_map` で捨てる (`kind/solve.rs` の `solve_scc`)。由来を1か所でも付け忘れると、線形性の違反が通って Core IR が実行してしまう (A3)

## 1. R6a 評価の順と型検査

### 1.1 評価の順 (A1)

#### 規則

関数適用の評価の順を ML 式にする。今の規則 (呼ばれる式と引数をすべて評価してから矢印を順に適用する) を置き換える。

呼び出し `e0 e1 … en` は、次の順に評価する。

1. 呼ばれる式 `e0` を評価する
2. `i = 1..n` の順に、`ei` を評価し、それまでの値に矢印 `i` を適用する

- `x |> f a` の `x` は今どおり最初に評価する。その後は同じ順で、`x` を渡す矢印に来たときに、評価済みの `x` を渡す
- 括弧は順を変えない。関数適用は左結合なので、`(f 1) (g ())` と `f 1 (g ())` は同じ式であり、同じ順に評価する
- `resume k v`、タプル、コンストラクタの引数は、今どおり左から評価してから動作が起きる

この順の違いが観測できるのは、途中の矢印の適用が何かを起こすときである。既知の関数を引数の数より少ない引数に適用しても、本体は動かない。そのため `Int -> Int -> <IO> Int` のような形の関数の呼び出しは、今と同じ結果になる。違いが出るのは、`Int -> <IO> (Int -> <IO> Int)` のように関数値を返す関数を呼んで、その結果にさらに引数を渡すときである。この型を書く人は「1つ目の引数で IO をして関数を返す」と読むので、その IO が後の引数の評価より先に起きる ML 式の順が、型の読み方と合う。

規則は [式](../../spec/expressions.md) の「関数」の節に置く。[エフェクトと handler](../../spec/effects.md) の持ち越し規則と [宣言](../../spec/declarations.md) の `|>` の行は、そこを参照する。

#### 引数をまとめて渡す範囲

手順どおりに矢印ごとに分けると呼び出しが増えるので、観測できる順が変わらない範囲で、引数をまとめて1回の呼び出しで渡す。

- 呼ばれる式が既知の関数、組み込み、操作、コンストラクタなら、その引数の数までの引数はまとめる。本体は引数がそろうまで動かないためである。引数のないトップレベルの値は、参照するたびに計算するので既知の呼ばれる式に含めない
- それを超える引数と関数値の呼び出しでは、値である引数 (下の `is_value`) を前の引数とまとめる。値でない引数の前で区切り、それまでの引数を渡す矢印を適用してから、その引数を評価する
- 値の評価は何も起こさないので、まとめても観測できる順は ML 式の規則と同じである

まとめて渡す引数は、呼び出しの実行中、前の矢印の呼び出しをまたいで持たれる (`Apply` のフレームに入る)。そのため、持ち越しの検査も実行と同じまとめ方を知らなければならない。まとめ方は次の契約の中で決め、持ち越しのパスと Core IR の変換の両方がそれを読む。

#### HIR に置く契約

評価の順とまとめ方は、`eml_hir` の1つの関数だけが知る。

```rust
pub enum EvalStep {
    /// 部分式を評価する。
    Eval(ExprId),
    /// それまでの値に、引数 `i` (0 から数える) を渡す矢印を適用する。続けて並ぶ `Arrow` は1回の呼び出しにまとめる。
    Arrow(usize),
}

/// 呼び出し (`ExprKind::Call`) の評価の手順。
pub fn call_steps(module: &Module, body: &Body, call: ExprId) -> Vec<EvalStep>;
/// 評価しても何も起きない式か。
pub fn is_value(module: &Module, body: &Body, expr: ExprId) -> bool;
```

- 例: 既知の関数 `f` (引数2つ) の `f a (g ())` は `Eval(f)`、`Eval(a)`、`Eval(g ())`、`Arrow(0)`、`Arrow(1)` になる。`h 1 (g ())` で `h` が引数1つの関数なら、`Eval(h)`、`Eval(1)`、`Arrow(0)`、`Eval(g ())`、`Arrow(1)` になる
- `x |> f a` (`evaluate_first`) では、`x` の `Eval` を最初に1回だけ置く。`x` を渡す矢印では、評価済みの `x` を値として扱う
- `is_value` は、リテラル、局所変数の参照、ラムダ、組み込みと操作とコンストラクタの参照、引数のあるトップレベルの関数の参照、値に型の明示を付けた式で真になる
- `carry.rs` と Core IR の変換 (`translate/expr.rs`) は、どちらも `call_steps` で順を得る。どちらにも、評価の順を独自に組み立てる処理を残さない

#### 持ち越し規則の変更

[エフェクトと handler](../../spec/effects.md) の持ち越し規則の評価の順の文は、式の節への参照にする。表の「関数とクロージャの呼び出し」の行の「後の矢印に渡す評価済みの引数を含む」は、まとめて渡す引数のことなので残す。

新しく数える値が1つある。値でない引数の前で区切ると、その引数を評価している間、それまでの矢印を適用した結果 (部分適用のクロージャか、返された関数値) を持っている。返された関数値は、捕まえた値によって `Lin` になりうるので、この値も途中の値として持ち越しに数える。

#### 型検査の記録

型検査は、呼び出しごとの記録 (`CallRows::Call`) に、矢印ごとの row に加えて矢印ごとの結果の型 (表の `Ty`) を持たせる。持ち越しのパスは、適用の結果の Kind をこの型から求める。

#### 持ち越しのパス

`carry.rs` の呼び出しの処理は、`call_steps` を後ろからたどる。

- `Eval(e)` の間に持っている値は、後で使う値、評価済みでまだ矢印に渡していない引数、それまでの適用の結果 (最初の矢印の前は呼ばれる式の値) である
- 続けて並ぶ `Arrow` は1回の呼び出しである。その中の各矢印 `i` で、後で使う値と、評価済みでまだ渡していない引数を持って、矢印 `i` の row と組にした持ち越しの制約を出す。操作の直接の呼び出しは今どおり操作の多重度を使う

#### Core IR の変換

変換は `call_steps` を前からたどる。`Eval` で部分式をアトムにし、続けて並ぶ `Arrow` を1回の呼び出しにする。最初のまとまりは、呼ばれる式が既知なら今の `call_known` などの場合分けに渡し、それ以外は `Apply` にする。後のまとまりは、前の結果への `Apply` にする。

#### テストの変更

| 種類 | テスト | 理由 |
|---|---|---|
| 1 | `tests/ui/run/` に、`(f 1) (g ())` と `f 1 (g ())` の副作用の順を確かめるテストを足す | 不具合3の固定 |
| 1 | `eml_types/tests/linearity.rs` に、`Lin` の関数値を返す適用の結果を、後の引数を評価する間に持つ場合の E3006 のテストを足す | 持ち越しの新しい場合 |
| 2 | 引数の数を超える呼び出しや関数値の呼び出しで、値でない引数を持つ Core IR のスナップショット | 呼び出しの区切りが変わる。計画で実行して洗い出す |

既存の持ち越しのテスト (`an_evaluated_argument_is_kept_across_a_later_argument`、`an_argument_for_a_later_arrow_is_kept_across_the_call`) は、引数の数までの引数と値の引数をまとめるので、期待値が変わらない。

### 1.2 型の走査 (A2)

`TyShape` (表の型)、`Type` (書き出す型)、`ShapeTy` (閉じた形) のそれぞれに、子をすべて訪ねる関数を1つずつ置く。

```rust
pub(crate) enum Child { Ty(Ty), Row(&Row) }   // 6b で状態の欄 (Slot) を足す
impl TyShape { fn for_each_child(&self, f: impl FnMut(Child)) }
```

- 関数の中では `..` を使わず、すべての欄を名前で受ける。欄を足したときに、この関数が直るまでコンパイルが通らないようにするためである
- `occurs`、`row_occurs_in`、`Type::contains_error`、Kind 変数の収集 (`kinds.rs`)、`ShapeTy` の `close`・`build`・`export_ty`・`collect_names` のうち子をたどるだけの部分を、この関数の上で書き直す
- `Fn` と `Cont` で同じことをしている腕は、子の走査に寄せて1つにする。`Fn` と `Cont` で意味の違う処理 (単一化の相手の形の照合、Kind の境界) は今のまま分けておく
- 振る舞いは変えない

### 1.3 Kind の由来を必須にする (A3)

制約の由来を `Option<KindOrigin>` から次の enum に変える。

```rust
pub(crate) enum Provenance {
    /// 報告する由来。
    At(KindOrigin),
    /// 誤りの跡がある本体 (`usage::reliable` が偽) の、使用回数と持ち越しの制約。違反しても報告しない。
    Suppressed,
    /// 宣言の型から作る制約。具体化するときに `At` を付けて複写する。
    Declaration,
    /// 表の既定値。由来を付け忘れた制約。値は本体を検査している関数の名前の範囲。
    Unattributed(TextRange),
}
```

- `KindProblem::origins`、`CarryConstraint::origin`、表の「今の由来」(`Table::kind_origin`) をこの型にする。関数ごとの表は、既定値を `Unattributed(関数の名前の範囲)` にして作る
- `solve_scc` は、違反した制約を由来で分ける。`At` は報告し、`Suppressed` は捨てる。`Declaration` と `Unattributed` の違反は処理系の誤りなので、debug ビルドでは panic にする。release ビルドでは、`Unattributed` の範囲を指す E3001 (「線形な値の誤った使い方」) にし、違反のあるプログラムを通さない。`Declaration` の違反は、宣言だけの問題を解いたときに起きないことを今も確かめているので、release でも panic にする
- 今 `filter_map` で捨てている3か所 (`lin`、`mult`、`carries`) はこの分け方に置き換わる

既存のテストで debug の panic が起きたら、それは今まで報告せずに捨てていた違反である。由来を付ける箇所を直し、新しい診断が出るならその場で相談する (種類1)。

### 1.4 handler の節の型と、呼ばれる位置の参照 (A4)

#### 節の型を `Shape` から作る

`check/handle.rs` の `op_clause` は、操作の宣言を HIR から下ろし直さず、操作の `Shape` (`Signatures::operations`) を具体化して型を得る。

- `Shape` に、先頭の `n` 個の rigid 変数の位置に与えた型を入れ、残りの rigid 変数を新しい rigid 変数にする具体化 `instantiate_with_effect_args(table, effect_args)` を足す。`n` はエフェクトの型引数の数 (`Operation::effect_params`) である
- `op_clause` はその結果の矢印をたどって、節の引数と `k` の型を決める
- `Rigids::with_effect_args` と、節での `lower_operation` の呼び出しを消す。`lower_operation` は `operation_shape` の中だけで使う
- 操作の型の作り方が1か所になり、S2 で別のモジュールのエフェクトを handle するときにも、そのモジュールの HIR を見ずに済む
- Kind 変数はどちらの経路でも新しく作るので、診断は変わらない見込みである

#### 呼ばれる位置の参照を開かない

今は、トップレベルの値の参照をすべて `open_spine` で開き (`check/body.rs` の `value`)、開く前の型を副表 `declared` に覚えておく。`call` は、`declared` の spine と開いた spine を並べてたどり、開く前の row を `CallRows` に記録する。

これを次の形にする。

- 呼ばれる位置にあるトップレベルの値の参照は、具体化だけして開かない。`call` は、たどった矢印の row をそのまま記録する
- 引数を当てた後に部分適用の残りがあれば、その残りの型だけを `open_spine` で開く
- 呼ばれる位置にない参照は、今どおり開く
- 副表 `declared` と、2つの spine を並べてたどる処理を消す

閉じた row を今の row に含めるとき、`include_row` は末尾を新しい row 変数に替えてから今の row と単一化する。開いた row を含めるときに作る制約と同じになるので、診断は変わらない見込みである。

#### テストの変更

| 種類 | テスト | 理由 |
|---|---|---|
| 3 | `eml_types` の表と形の単体テストのうち、型、row、`Rigids` を組み立てるもの | 組み立ての関数が変わる。期待値は変えない |

### 1.5 診断の順 (A8)

#### 規則

`eml check` と `eml run` が表示する診断は、(ファイル、primary の開始位置、番号) の順に並べる。3つとも同じなら、段階が出した順を保つ (安定な並べ替え)。各段階は診断の順を約束しない。この規則を [診断](../../spec/diagnostics.md) に書く。

#### 実装

- `eml_diagnostics` に並べ替えの関数 `sort_diagnostics(&mut [Diagnostic])` を置く。ファイルの順は `FileId` の順とする
- `eml_cli::front` と `eml_test_support` のパイプラインの両方が、この関数を呼ぶ。段階ごとのテストも、表示と同じ順で診断を見る
- 各段階の並べ替え (`lexer/mod.rs`、`eml_syntax/src/lib.rs`、`eml_hir/src/lower/mod.rs`、`eml_types/src/exhaustive.rs`) を消す
- 型検査の本体の検査 (段1) は、SCC の順ではなく関数のアリーナの順で回す。SCC の順は段2 (Kind の解決) だけが使う。`check/mod.rs` の「SCC の順に呼ぶのは、型の誤りの診断の並びを保つためだけである」という説明もなくなる
- `Instance::at` と `Table::kind_counts` を消す。段2の `merge` は、具体化で展開した制約を、宣言の制約の後ろにまとめて足す

#### 残す並べ替え

`check/mod.rs` の `report_violations` の並べ替えは残す。順を決めるだけでなく、同じ値の持ち越しの違反から位置が最も前の1件を選んでいるためである ([診断](../../spec/diagnostics.md) の E3006)。今は同じ範囲どうしの順を `at` に頼っているので、キーを (開始、終了、理由の順位) にする。理由の順位は `KindReason` の種類ごとに固定した全順序で、同じ種類どうしは由来の中の値の名前で比べる。制約の並びに関係なく、選ぶ1件が決まる。

#### テストの変更

| 種類 | テスト | 理由 |
|---|---|---|
| 1 | `ui__check_fail@syntax__missing_indented_block.em` | 今は E0009 (2:8)、E1004 (2:1)、E1004 (3:1) の順。位置順で E1004 (2:1) が先になる |
| 1 | `ui__check_fail@syntax__tab_indentation.em` | 今は E0006 (3:1)、E1004 (2:1) の順。E1004 が先になる |
| 1 | `eml_types/tests/check.rs` の `if_without_else_must_be_unit` | E2001 (2:17)、E2001 (2:7) の順が逆になる |
| 1 | `eml_types/tests/tuples.rs` の `undecided_operands_are_reported_and_errors_are_not` | E1001 (7:12)、E2006 (3:32) の順が逆になる |
| 1 | `kind/solve.rs` の、`merge` が展開した制約を差し込む位置を固定する単体テスト (89f09b1 で足したもの) を削除する | 差し込む位置という概念がなくなる |
| 1 | 同じ範囲に違反が2つあり、選ぶ1件か並びが変わるテスト | 計画で実行して確かめる。見つかれば列挙する |

上の4件は、スナップショットの診断の位置を機械的に調べて見つけた。計画では全テストを流して確かめ直す。

### 1.6 R6a で直す文書

- [式](../../spec/expressions.md): 「関数」の節に評価の順の規則を足す
- [エフェクトと handler](../../spec/effects.md): 持ち越し規則の評価の順の文を式の節への参照にし、まとめて渡す引数と、値でない引数の評価の間に持つ適用の結果を書く
- [宣言](../../spec/declarations.md): `|>` の行の評価の順を、式の節への参照にする
- [診断](../../spec/diagnostics.md): 診断の順の規則を足す
- [コンパイラの構成](../../implementation/architecture.md): `call_steps` と `is_value` の契約 (まとめて渡す範囲を含む)、持ち越しのパスと変換がそれを使うこと、診断の並べ替えの位置、`Provenance`、節の型の作り方、`declared` と `Instance::at` の記述の削除
- [実装の現在地](../../implementation/status.md): R6 の行、確かめた不具合1と2を R7 の項目として記録、診断の言い方の注意点のうち `(1 + 1) 2` は残す
- [テストの変更の記録](../../implementation/test-changes.md): 種類1と種類2の変更

## 2. R6b Core IR と interp の道具

### 2.1 Core IR の道具 (A5)

#### 組み立て

`FnBuilder` を1つ置く。変数の追加 (`new_var`、`fresh_like`)、式の追加 (`push`)、join point の追加、束縛の並びを式にする `seq` を持つ。次の4か所が、それぞれの組み立てをやめてこれを使う。

- translate (`FnLowering` の `push`、`new_var`、`new_join`、`seq`)
- 組み込み、操作、コンストラクタを包む関数と入口の関数 (`translate/program.rs` で `CExprId(0)` などを手で書いている箇所)
- simplify (`push`、`fresh_like`)
- Perceus (`push`)

#### 走査

`CExpr` に、確保しない走査の関数を置く。

```rust
impl CExpr {
    fn for_each_child(&self, f: impl FnMut(CExprId));
    fn for_each_child_mut(&mut self, f: impl FnMut(&mut CExprId));
    fn for_each_atom(&self, f: impl FnMut(&Atom));
    fn for_each_atom_mut(&mut self, f: impl FnMut(&mut Atom));
}
```

`simplify.rs` の `children`、`replace_child`、`used_atoms` (式をまるごと複製して読む)、`atoms_mut` (呼ぶたびに `Vec` を作る) をこれで置き換える。liveness、verify、pretty の走査も、子をたどるだけの部分はこれを使う。

#### テキストの IR

- `pretty` の表示で、boxed の変数に、束縛の位置 (関数の引数、`let`、join point の引数、`switch` の枝のフィールド) で `^` を付ける。例: `fn twice(s0^)`、`let s1^ = const "a"`。使用の位置には付けない
- その表示を読む `eml_core_ir::parse(&str) -> Result<Program, ParseError>` を足す。関数の名前、変数の名前と番号、join point の番号、文字列定数、エフェクトの表を表示から復元する
- 表示と読み込みが往復することを、Core IR のテストの全スナップショットの出力でテストする
- 置き場所は `eml_core_ir` の `text.rs` とし、`eml_interp` のテストからも使えるように公開する

#### 手組みのテストの書き直し

`eml_core_ir/tests/verify.rs` と `eml_interp/tests/` で、アリーナを手で組み立てているテストを、IR のテキストを `parse` する形に書き直す。確かめる内容 (verifier が受け入れるか拒否するか、実行の結果) は変えない。`eml_test_support::ir` の組み立ての関数は、使われなくなったものを消す。

#### アリーナの正規化

- simplify の最後に、各関数を根からの前順で組み直す `settle` を置く。木から外れた式を捨て、join point の番号を元の順に振り直す。今 simplify の最後にある振り直し (`renumber`) はここに移す
- `verify_scopes` と `verify` は、どの式も根からちょうど1回たどれること (共有も取り残しもないこと) を確かめる
- Perceus は、アリーナを作り直すことでゴミ集めを兼ねなくてよくなる。Perceus が自分のためにアリーナを作り直すのは構わない

#### 使っていない欄の削除

`VarInfo::linearity` はつねに `Unr` なので消す。RC の対象かどうかは `boxed` だけで決まる (`liveness.rs` の `tracked`)。`eml_core_ir` が `eml_types::Linearity` を再公開する理由もなくなる。

### 2.2 `eml_interp` の分割 (A6)

`lib.rs` (845行) を次のファイルに分ける。

| ファイル | 中身 |
|---|---|
| `lib.rs` | `run`、`RunConfig`、公開する型 |
| `machine.rs` | `Machine`、step、bind、call、apply、enter、ret |
| `effects.rs` | handle、perform、resume、`drop k`、find_handler |
| `prim.rs` | `Int` と `String` のプリミティブ |
| `io.rs` | `println`、`open`、`read_all`、`close` |
| `error.rs` | `RuntimeError`、`Fault` |

- `Prepared` を消す。呼び出しのフレームを積んでから、`&Call` で直接分岐する。フレームの退避は環境のスロットを読むだけで書き換えないので、引数を読むのをフレームを積む前にずらす必要はない
- 戻り位置を表す構造体 `Resume` を `ReturnPoint` に改名する。継続の再開 (`resume`) と名前が重ならないようにするためである
- プリミティブと IO を `eml_runtime` に移すことは、S2 の標準ライブラリと一緒に行う。R6b では crate の中で分けるだけにする

### 2.3 末尾呼び出し (A7)

- simplify の最後 (DCE の後、`settle` の前) に、`let x = call …` の直後が `return x` なら `tailcall …` に書き換える規則 T を足す。`Call` のどの種類も対象にする (今の translate と同じ)
- translate の `tail_after` で、呼び出しの結果の変数を `vars.pop` して番号を詰める処理を消す。末尾呼び出しを作る場所は T の1か所にする
- これで不具合4 (`if` の枝に動いた呼び出し) も末尾呼び出しになる
- `simplify` を1巡だけ回す今の方針は変えない ([Core IR とインタプリタ](../../spec/core-ir.md) のパスの順に T を足す)

### 2.4 R6b で直す文書

- [Core IR とインタプリタ](../../spec/core-ir.md): パスの順に T と `settle` を足す。表示の boxed の印。式がちょうど1回たどれることを verifier が確かめること
- [コンパイラの構成](../../implementation/architecture.md): `FnBuilder` と走査の関数、テキストの IR、`eml_interp` のファイルの分け方、「Perceus がアリーナを作り直して木から外れた式を捨てる」という記述の更新
- [テスト戦略](../../implementation/testing.md): 手組みの IR のテストを IR のテキストで書くこと
- [ロードマップ](../../future/roadmap.md): 再帰する join point の項に、生存解析が1回の走査で自分への `jump` の `captures` を読むので、ループ化では不動点の計算か再帰する join point の印が要ることを書き足す (見直しで分かった誤り)
- [実装の現在地](../../implementation/status.md)、[テストの変更の記録](../../implementation/test-changes.md)

### 2.5 テストの変更

| 種類 | テスト | 理由 |
|---|---|---|
| 2 | `eml_core_ir/tests/{translate,simplify,perceus}.rs` のすべてのスナップショット | boxed の印 `^` |
| 2 | translate のパスで止めるスナップショットのうち `tailcall` を含むもの | translate は `let t = call …; return t` を出し、T が後で書き換える |
| 2 | 変数の番号がずれる simplify と Perceus のスナップショット | translate が結果の変数を詰めなくなる |
| 2 | 末尾にない `if` の枝の呼び出しが末尾呼び出しになるスナップショット | 不具合4の修正 |
| 3 | `eml_core_ir/tests/verify.rs`、`eml_interp/tests/*.rs` | IR のテキストへの書き直し。確かめる内容は同じ |
| 3 | `eml_test_support::ir` を使うテスト | 組み立ての関数の整理 |

UI テストの出力は変わらない。

## 3. R6 に含めないもの

### 6b の spec に回すもの

- HIR: ラムダ、handle の本体、操作の節、`return` の節を1つの closure の形 (`Closure { params, body }`) にそろえ、捕まえる変数を変換のときに1回だけ求める。省いた `return` の節を HIR で合成する。状態のない handler は HIR では `init: None` のまま残し、型検査が状態のない handler と `Unit` の状態を区別できるようにする。`from` のある handler の本体も変換し、名前の誤りを報告する
- Core IR: `Call::Handle { effect, init, body: FnRef, clauses: Vec<FnRef>, ret: FnRef }` と `Call::Resume { k, arg, state }`。`ret` はつねにある。verifier が節の引数の数を確かめる
- Core IR とランタイム: 捕まえた変数のない関数を、クロージャを確保しない値 (`Atom::Fn`、`Value::Fn`) にする
- ランタイム: `Frame::Handler` の「つながっている部分」を `Option<Link { next, state }>` にし、つながっているのに状態がない形を表せなくする。`perform` に操作の多重度を持たせ、`Program::effects` を引かずに済むようにする
- 型検査: `Cont` に状態の欄を足す (A2 の走査の上で)。状態の欄の単一化の誤りを `UnifyError` の専用の種類にする
- 診断: `Diagnostic::fix` を、題名を持つ複数の fix (`fixes: Vec<Fix { title, edits }>`) にする。6b の `resume` の引数の数の誤りに、状態の引数を足す fix と除く fix を付けるため

### R7 (S2 の前) に回すもの

- item の ID をプログラム全体で一意にし (`ModuleId`)、HIR をモジュールのインタフェースと本体に分ける。Prelude に本物の `FileId` を与えて普通のモジュールにする。`FileId::PRELUDE` をなくす
- 名前解決を、item の収集、モジュールごとのスコープ表、item ごとの変換の3段に分け、重複の扱いを1つにする (不具合2)。fixity は解決した先の定義に付ける
- 組み込みを Prelude の intrinsic にし、Rust の表を1つにする。`Res::Builtin`、`Decl::Builtin`、`Module::builtins`、`TypedModule::builtins` をなくす。`Bool` のタグを HIR のコンストラクタから引く
- 型検査の出力を、損失のないモジュールのインタフェース (`Shape` と `KindScheme`、由来に `FileId`) にする。表示用の `Scheme` は `dump` の中の表示にする
- source の読み込み (ローダ、session 型の lib API)、複数ファイルのテストの fixture、ディレクトリを1件とする UI テスト
- CST に名前と経路のノード (PATH、NAME) を入れ、grammar.md の全体を CST まで組む。E0004 を出す層の方針を1つにする (不具合1)
- Core IR の `Switch` を、default の枝とリテラルの case を持つ平らな形にする。リテラルが約1000個の `match` で debug ビルドのスタックがあふれる問題を直す。呼び出しの飽和の処理を1つにまとめる
- spec で決めること: import の循環を許すか、モジュールの根をどこにするか

### S2 の設計の材料と、後回しにするもの

- S2 の設計の材料: row の仕組みを sort 付きの1つにし、エフェクトの row とレコードの row で共有する。レコードの実行時の表し方。文字列のトークンを lexer のモードで分けること。レイアウト規則3とレコードの `with` の衝突。借用のオペランドと `Field`。参照ごとの具体化を記録する表と、`==` の比べ方の一般化
- 後回し: simplify を use と def の索引で書き直すことと、足りない書き換え (別名の伝播、定数の `switch` の畳み込み)。実行時エラーの位置と backtrace と、利用者向けの関数名。HIR の位置を source map に移すこと。n 列の `match`。`let` で束縛したラムダの row の多相化。doc comment の trivia の付け方。記述子に種類ごとの方針を持たせること。文書の簡素化

## 4. 完了の条件

### R6a

- 1章の変更がすべて入り、1.6 の文書を直してある
- 評価の順を組み立てる処理が `eml_hir` の `call_steps` の1か所にあり、`carry.rs` と `translate/expr.rs` に独自の順の組み立てがない
- `TyShape`、`Type`、`ShapeTy` の子の走査に `..` がない
- `solve_scc` に由来を `filter_map` で捨てる処理がない
- `declared`、`Instance::at`、`Table::kind_counts`、`Rigids::with_effect_args` がない
- 段階の中に診断の並べ替えがない (`report_violations` を除く)
- `cargo test`、`cargo clippy --all-targets`、`cargo fmt --check` が通る。種類1と種類2の変更は、この文書の表と `test-changes.md` に記録したものだけである

### R6b

- 2章の変更がすべて入り、2.4 の文書を直してある
- アリーナを手で組み立てるテストがない (IR のテキストで書いてある)
- 表示と読み込みの往復のテストが通る
- `Prepared` と `VarInfo::linearity` がない。translate に `vars.pop` がない
- `cargo test`、`cargo clippy --all-targets`、`cargo fmt --check` が通る。種類1と種類2の変更は、この文書の表と `test-changes.md` に記録したものだけである
