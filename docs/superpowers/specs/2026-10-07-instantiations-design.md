# 参照ごとの具体化の表の設計 (M2b)

位置づけ: 作業用の設計文書。作業を終えたら削除する。

## 目的と範囲

[ロードマップ](../../future/roadmap.md) の M2 のうち、残りの参照ごとの具体化の表を実装する。式の中のトップレベルの item への参照ごとに、どの型引数で具体化したかを型検査の出力に記録し、今の `==` と `!=` の比べ方をその表に通す。振る舞いは変えない。

この表は、M4 で `+` などの演算子を組み込みの型ごとに解決するときと、M5 で制約を持つ関数を証拠ごとに複製するときの土台になる。M2b で M2 は終わる。

## 今のコード

- `BodyCheck::path` が `==` と `!=` の参照だけを `Comparison` として積む。`check_function` の直後に `resolve_equalities` が最初の引数の型を見て `Equality::{Int, String, Bool}` を決め、`BodyTypes::equalities: ArenaMap<ExprId, Equality>` に入れる。決まらなければ E2006 にする。この呼び出しは、線形性の検査が使う `usage::reliable` より前にある
- Core IR は `equalities[callee]` を引いて、比べる命令 (`PrimOp::IntEq` など) を選ぶ
- 参照の具体化は `Shape::instantiate` が行い、`Shape::rigids` ごとに新しい型変数を作る。ただし外に返すのは型と Kind 変数 (`Instantiated { ty, lin, mult }`) だけで、Kind の段2のための `Instance` (宣言と Kind 変数) にも、どの式の参照かと型引数は残らない
- HIR の脱糖も参照を `ExprKind::Path` で作る。前置の `-` は `negate` への `Path`、`&&` と `||` は `True` と `False` への `Path` を演算子の位置に置く (`lower/ops.rs`)。セクションと値として使う演算子はラムダに脱糖する (`lower/section.rs`)。どの `Path` も `BodyCheck::path` を通る

## 表の形

```rust
pub struct BodyTypes {
    pub exprs: ArenaMap<ExprId, Type>,
    pub locals: ArenaMap<LocalId, Type>,
    pub pats: ArenaMap<PatId, Type>,
    /// 式の中のトップレベルの item への参照ごとの具体化
    pub instantiations: ArenaMap<ExprId, Instantiation>,
}

pub struct Instantiation {
    pub decl: Decl,
    /// `Shape::rigids` の順に並べた型引数
    pub args: Vec<Type>,
}

/// `==` と `!=` の比べ方。比べられない型なら `None`。
pub fn equality(lang: &LangItems, ty: &Type) -> Option<Equality>;
```

- 表の名前は `instantiations` にする。M5 では「instance」が型クラスの instance を指すので、それと区別するためである
- キーは参照を表す式 (`ExprKind::Path` の式) である。`==` の参照では、呼ばれる側の式になる。脱糖で作った参照のキーは、脱糖が置いた式 (演算子の位置の `Path`) である

### 型引数の順

`args` は `Shape::rigids` の順である。宣言の種類ごとに次のとおりになる。

- 関数 (intrinsic を含む): シグネチャの型変数が最初に現れた順。`(<<) : (b -> <e> c) -> (a -> <e> b) -> a -> <e> c` は `[b, c, a]` である。row 変数 `e` は入らない
- 操作: エフェクトの型引数が先で、その後に操作自身の型変数が最初に現れた順。`effect State s` の `swap : a -> s -> (a, s)` は `[s, a]` である
- コンストラクタ: `data` の頭の型引数の順。`data P a b = | Q b a` の `Q` は `[a, b]` である
- `==` は `a -> a -> Bool` なので、`args[0]` が比べる値の型になる

### 記録するもの

- 式の中で、関数、コンストラクタ、操作を参照したとき (`Res::Function`、`Res::Constructor`、`Res::Operation`)。intrinsic の関数、ほかのモジュールの item、脱糖で作った参照 (`negate`、`&&` と `||` の `True` と `False`、セクションのラムダの中の演算子) も含む
- 型変数のない item の参照も記録し、`args` は空になる

### 記録しないもの

- 局所変数の参照。具体化しないためである
- パターンのコンストラクタと handler の節の操作。M5 でコンストラクタと操作は制約を持てない ([ロードマップ](../../future/roadmap.md) の「M5 型クラス」) ので、解決が要らない
- row 変数と Kind 変数。M4 と M5 の解決は型引数だけで決まる
- シグネチャがなく、型が `Error` になる参照

### 暗黙の参照

型ごとの解決が要る参照は、HIR で `ExprKind::Path` にしてこの表に届くようにする。前置の `-` がすでにそうなっている。M3 の補間の穴のように、処理系が暗黙に持ち込む参照も同じ形に脱糖する。別の形を選ぶマイルストーンは、表のキーと記録の場所を広げる。

### 記録の手順

- `Instantiated` に、`Shape::rigids` の順の型引数の変数 `args: Vec<Ty>` を足す
- `BodyCheck::path` が、参照を具体化するたびに、式の ID と宣言と型引数の変数 (内部の `Ty`) を記録する。具体化のたびに新しい変数を作るので、同じ宣言の参照でも記録は別々である
- 本体の検査が終わってから、`exprs` と同じく carry の後で `Type` に書き出す。後の文の単一化で決まる型 (`let` で束縛したラムダの引数など) を取り込むためである。最後まで決まらない型変数は、`exprs` と同じく `Type::Flexible` になる。E2006 になった参照も記録は残す

## `==` を表に通す

- `Comparison`、`BodyCheck::comparisons`、`BodyTypes::equalities` をなくす
- 比べ方の判定は、今の `resolve_equalities` と同じ位置で行う。`check_function` の直後で、`usage::reliable` を計算するより前である。E2006 が `diagnostics.is_empty()` に数えられ、E2006 だけの本体から線形性の診断 (E3001〜E3005) が出ないようにするためである ([診断](../../spec/diagnostics.md) の「連鎖する診断の抑止」)
- その位置で、表のうち宣言が `lang.eq` か `lang.ne` の記録について、`args[0]` を見て比べ方を決める。比べ方の判定は `equality` の1か所にまとめ、`args[0]` だけをその場で書き出して渡す
- 決まらなければ、今と同じ規則で E2006 にする
  - 「同じ本体に別の誤りがある」かどうかは、比べ方の判定を始める前の診断で1回だけ決める。そのため、1つの比べ方の E2006 が、同じ本体の別の比べ方の E2006 を抑えることはない
  - 本体に別の誤りがあり、`args[0]` の内部の型がまだ変数なら報告しない
  - 書き出した `args[0]` が誤りの跡を含むなら報告しない
- Core IR は、`instantiations[callee].args[0]` を `equality` に渡して比べる命令を選ぶ。Core IR は誤りのないプログラムだけを受け取るので、決まらない場合は起きない。今の `expect` と同じく、起きたら処理系の誤りとして panic する

`equality` は `eml_types` に置き、型検査と Core IR が同じ関数を使う。

## 変えないもの

- 振る舞い。UI の出力、診断の番号と文言、診断が出る本体と出ない本体、Core IR の出力は変わらない
- Kind の段2のための `Instance` (`kind/problem.rs`)。これは Kind の制約を展開するための内部の記録で、式の参照とは役目が違う。名前もそのままにする。型クラスの instance と名前が重なるので、改名は M5 で決める
- `Equality` の enum と、`==` と `!=` の規則 ([宣言](../../spec/declarations.md) の標準の演算子の表)

## テスト

### 表のテスト

`eml_types/tests/instantiations.rs` を足し、`tests/main.rs` に宣言する。

補助関数は、関数を (モジュール、名前) で選び、その本体の記録を式の位置の順に1行ずつ `<行:列> <宣言の名前> [<型引数>]` の形で出す。位置はその関数のモジュールのファイルで数える。宣言の名前は `program` の関数、コンストラクタ、操作の名前を引き、型引数は `Type::display(&program.names)` で表示する。`Flexible` は `_` と表示される。

- 多相な関数の参照と、参照ごとの記録: 同じ本体の `id 1` と `id "a"` が、それぞれ `id [Int]` と `id [String]` になる
- ユーザーの `data` のコンストラクタ: `Box 1` が `Box [Int]`
- 型引数の順
  - `effect State s` の `swap : a -> s -> (a, s)` を `swap 1 "x"` で起こすと `swap [String, Int]` になる (エフェクトの型引数が先)
  - `f << g` (`f : Int -> String`、`g : Bool -> Int`) が `<< [Int, String, Bool]` になる (最初に現れた順。名前の順ではない)
- 後の文で決まる型引数: `let f = fn x -> id x` の後で `f 1` を呼ぶと `id [Int]` になる。`==` で同じことを確かめるテストは `tuples.rs` にすでにあるので、ここでは `==` 以外の参照で確かめる
- 多相な本体の中の参照: `twice : a -> a`、`twice x = id (id x)` で、2つの `id` が `id [a]` になる
- 決まらない型引数: `data Option a = | None | Some a` の下で、使わない `let _ = None` が `None [_]` になる
- handler の節の型変数: 型変数 `a` を持つ関数の中で、型変数 `a` を持つ操作の節の引数を参照すると、記録は `[a]` と表示される。関数の `a` と区別できないことを、今の振る舞いとして固定する (「範囲外」の M5 の論点)
- 脱糖した参照: `n == 1 && b` は `==` の位置に `== [Int]` を、`&&` の位置に `False []` を記録する。`- n` は `-` の位置に `negate []` を記録する。`(== 1)` はラムダの中に `== [Int]` を記録する
- 型変数のない item: `not True` が `not []` と `True []` になる
- ほかのモジュール (`check_files`): `M.em` で `data Box a = | Box a` と `wrap : a -> Box a`、`wrap x = Box x` を定義し、入口で `M.wrap 1` を呼ぶ。`wrap` の本体に `Box [a]`、入口に `wrap [Int]` が記録される
- 記録しないもの: 局所変数の参照と、パターンのコンストラクタは記録がない
- 漏れのないこと: 演算子、セクション、前置の `-`、`&&`、ユーザーのコンストラクタ、操作を使うプログラムのすべての本体について、記録のキーが局所でない `Path` であること、シグネチャのある宣言を指す `Path` にはすべて記録があること、`args` の数が宣言の `Shape::rigids` の数と等しいことを確かめる

### `==` のテスト

- `tuples.rs` に足す。1つの本体に決まらない比べ方が2つ (`==` と `!=`) あると、E2006 が2件出る
- `tuples.rs` か `linearity.rs` に足す。`f : File -> Bool` で、使わない `let same = fn x -> fn y -> x == y` を持ち、`File` を捨てる本体は、E2006 だけを報告し、E3003 を報告しない。E2006 の判定が `usage::reliable` より前にあることを固定する

### `Shape` の単体テスト

`shape.rs` の単体テストに足す。`f : a -> b -> a` の具体化で、`args` が2つあること、2回の具体化が別の変数を作ること、`args[0]` を `Int` と単一化すると型が `Int -> _ -> Int` と書き出されることを確かめる。

### Core IR のテスト

`translate.rs` に足す。`main` から届く本体で、`Int`、`String`、`Bool` のそれぞれを `==` と `!=` で比べ、6通りの比べる命令がスナップショットに出ることを確かめる。既存の `equality_picks_the_comparison_of_the_operand_type` は、比べる関数が `main` から届かず、命令を確かめていないためである。新しいテストなので、テストの変更には当たらない。

### テストの変更

テストの変更の種類は [テスト](../../implementation/testing.md) の分け方に従う。

- 種類3: `eml_types/tests/tuples.rs` の補助関数 `decided` は `BodyTypes::equalities` を読んでいる。これを `instantiations` と `equality` を通して読む形に書き換える。期待値はバイト単位で変わらない
- 種類1と種類2の変更はない。`Instantiated` は欄を足すだけなので、`shape.rs` の既存の単体テストは書き換えない

## 文書の更新

M2b は言語の規則を決めないので、`spec/` に移すものはない。表の構造は `implementation/architecture.md` に書く。

- `implementation/architecture.md`
  - `eml_types` の別テーブルの説明に、参照ごとの具体化の表と `equality` を足す。`equalities` の記述があれば直す
  - 2つの記録を名前で区別する。Kind の段2のための記録は「Kind の具体化の記録」(`Instance`)、この表は「参照ごとの具体化の表」(`instantiations`) と呼ぶ。今「具体化の記録」と書いている箇所 (`architecture.md`、`spec/types.md`) は、Kind の記録を指すことがわかる言い方にする
- `implementation/testing.md`: 「今あるテストの地図」の `eml_types` の欄に、`instantiations.rs` (参照ごとの具体化の表。記録する参照、記録しない参照、型引数の順) を足す
- `future/roadmap.md`
  - 「マイルストーンの列」の表から M2 の行を、本文から「M2 モジュール」の節を消す
  - 前提から M2 を除く。M3 と M4 の行と節は「なし」、M6 は「M5」にする。マイルストーンの後の項目の「前提: M2」(抽象型、再エクスポート、修飾した演算子の構文、優先順位グループ) は前提を消す
  - 「順序の理由」の M2 を最初に置く理由の項目と、「M2 に続けて」「M4 が必要とするのは M2 だけ」の項目を、M2 を終えたことに合わせて書き直す
  - 「M2〜M9」の言い方を「M3〜M9」にする
  - M5 の「決めたこと」の「型検査が、制約を持つ名前の参照ごとに証拠 (どの instance を使うか) を記録し」を、「型検査は参照ごとの具体化を記録して制約を検査し、証拠は記録した型引数から、Core IR と共有する解決の関数で決める」という分担に書き直す
  - 「マイルストーンの進め方」の「決まったことを `spec/` に移し」を、「決まったことを `spec/` に移し (実装の構造に関わるものは `implementation/` に移し)」にする
- `spec/core-ir.md`: `==` と `!=` を「型検査が決めた比べ方」で選ぶという記述を、「型検査が記録した型引数から選ぶ」にする
- `implementation/status.md`: M2 を完了とし、次を M3 にする
- `README.md` と `overview.md`: マイルストーンの範囲の言い方 (「M2〜M9」) を、終えたマイルストーンに合わせて直す。`overview.md` のマイルストーンの一覧から「M2 モジュール」を除く

## 範囲外

- M4 の演算子の解決と、M5 の型クラスの証拠と特殊化
- row 変数と Kind 変数の記録
- 書き出した型引数を、型検査のダンプ (`dump`) に表示すること
- rigid な型変数の区別 (M5 の論点): `args` は rigid な型変数を名前だけで書き出す (`Type::Rigid(name)`)。handler の節は、操作の型変数と同じ名前の rigid な型変数を作る ([エフェクトと handler](../../spec/effects.md))。そのため、関数の型変数 `a` と節の型変数 `a` は表の上で区別できない。M2b ではこれで困らない (どちらの `==` も E2006 になる)。M5 で特殊化のときに型引数へ代入する前に、節の型変数を区別する表し方 (本体ごとの番号など) を決める
- intrinsic の包み (M5 の論点): Core IR は intrinsic の関数を値として使うとき、関数ごとに1つの包みを作り、参照の式の ID を渡さない。型ごとの解決が要る intrinsic は今はどれも演算子で、HIR が演算子の参照とセクションをラムダに脱糖するので、いつも飽和して呼ばれ、この問題は起きない。M5 で PrimOp に変換するメソッドを値として使えるようにするときに見直す
