# 参照ごとの具体化の表の設計 (M2b)

位置づけ: 作業用の設計文書。作業を終えたら削除する。

## 目的と範囲

[ロードマップ](../../future/roadmap.md) の M2 のうち、残りの参照ごとの具体化の表を実装する。式の中のトップレベルの item への参照ごとに、どの型引数で具体化したかを型検査の出力に記録し、今の `==` と `!=` の比べ方をその表に通す。振る舞いは変えない。

この表は、M4 で `+` などの演算子を組み込みの型ごとに解決するときと、M5 で制約を持つ関数を証拠ごとに複製するときの土台になる。M2b で M2 は終わる。

## 今のコード

- `BodyCheck::path` が `==` と `!=` の参照だけを `Comparison` として積む。本体の検査が終わってから、`resolve_equalities` が最初の引数の型を見て `Equality::{Int, String, Bool}` を決め、`BodyTypes::equalities: ArenaMap<ExprId, Equality>` に入れる。決まらなければ E2006 にする
- Core IR は `equalities[callee]` を引いて、比べる命令 (`PrimOp::IntEq` など) を選ぶ
- 参照の具体化は `Shape::instantiate` が行い、シグネチャの型変数ごとに新しい変数を作る。ただし外に残すのは、Kind の段2のための `Instance` (宣言と Kind 変数) だけで、どの式の参照かも型引数も残していない

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
    /// シグネチャの型変数の順 (`Shape` の rigid の順) に並べた型引数
    pub args: Vec<Type>,
}

/// `==` と `!=` の比べ方。比べられない型なら `None`。
pub fn equality(lang: &LangItems, ty: &Type) -> Option<Equality>;
```

- 表の名前は `instantiations` にする。M5 では「instance」が型クラスの instance を指すので、それと区別するためである
- キーは参照を表す式 (`ExprKind::Path` の式) である。`==` の参照では、呼ばれる側の式になる
- 型引数の順は、シグネチャの型変数の順 (`Shape::rigids` の順) である。`==` は `a -> a -> Bool` なので、`args[0]` が比べる値の型になる

### 記録するもの

- 式の中で、関数、コンストラクタ、操作を参照したとき (`Res::Function`、`Res::Constructor`、`Res::Operation`)。intrinsic の関数とほかのモジュールの item も含む

### 記録しないもの

- 局所変数の参照。具体化しないためである
- パターンのコンストラクタと handler の節の操作。M5 でコンストラクタと操作は制約を持てない ([ロードマップ](../../future/roadmap.md) の「M5 型クラス」) ので、解決が要らない
- row 変数と Kind 変数。M4 と M5 の解決は型引数だけで決まる
- シグネチャがなく、型が `Error` になる参照

### 記録の手順

- `Shape::instantiate` は、作った型引数の変数も返す
- `BodyCheck::path` が、参照を具体化するたびに、式の ID と宣言と型引数の変数 (内部の `Ty`) を記録する
- 本体の検査が終わってから、`exprs` と同じく `Type` に書き出す。後の文の単一化で決まる型 (`let` で束縛したラムダの引数など) を取り込むためである。最後まで決まらない型変数は、`exprs` と同じく `Type::Flexible` になる

## `==` を表に通す

- `Comparison`、`BodyCheck::comparisons`、`BodyTypes::equalities` をなくす
- 本体の終わりに、表のうち宣言が `lang.eq` か `lang.ne` の記録について、最初の型引数を見て比べ方を決める。比べ方の判定は `equality` の1か所にまとめる
- 決まらなければ、今と同じ規則で E2006 にする。同じ本体に別の誤りがあって型が決まっていないとき (内部の型が変数のとき) と、型が誤りの跡を含むときは報告しない ([診断](../../spec/diagnostics.md) の「連鎖する診断の抑止」)。この判定は書き出す前の内部の型で行う
- Core IR は、`instantiations[callee].args[0]` を `equality` に渡して比べる命令を選ぶ。Core IR は誤りのないプログラムだけを受け取るので、決まらない場合は起きない。今の `expect` と同じく、起きたら処理系の誤りとして panic する

`equality` は `eml_types` に置き、型検査と Core IR が同じ関数を使う。M5 で特殊化した本体の中の参照も、型引数に代入してから同じ形で解決できる。

## 変えないもの

- 振る舞い。UI の出力、診断の番号と文言、Core IR の出力は変わらない
- Kind の段2のための `Instance` (`kind/problem.rs`)。これは Kind の制約を展開するための内部の記録で、式の参照とは役目が違う。名前もそのままにする
- `Equality` の enum と、`==` と `!=` の規則 ([宣言](../../spec/declarations.md) の標準の演算子の表)

## テスト

### 足すテスト

`eml_types/tests/instantiations.rs` を足し、`tests/main.rs` に宣言する。表の中身を、参照の式の位置と、書き出した型引数の表示で確かめる。

- 多相な関数の参照: `id 1` の `id` が `[Int]` で具体化される
- ユーザーの `data` のコンストラクタ: `Box 1` が `[Int]`
- 型引数を持つエフェクトの操作の参照
- 後の文で決まる型引数: `let` で束縛したラムダの中の `==` が、後の呼び出しで `Int` に決まる
- ほかのモジュールの item への参照 (`check_files` を使う)
- 型変数のない item の参照は、型引数が空になる
- 記録しないもの: 局所変数の参照と、パターンのコンストラクタ

### テストの変更

テストの変更の種類は [テスト](../../implementation/testing.md) の分け方に従う。

- 種類3: `eml_types/tests/tuples.rs` の補助関数 `decided` は `BodyTypes::equalities` を読んでいる。これを `instantiations` と `equality` を通して読む形に書き換える。期待値はバイト単位で変わらない
- 種類1と種類2の変更はない。Core IR の既存のテスト (`==` と `!=` の命令の選択を含む) と UI テストは、そのまま通る

## 文書の更新

- `implementation/architecture.md`: `eml_types` の別テーブルの説明に、参照ごとの具体化の表と `equality` を足す。`equalities` の記述があれば直す
- `future/roadmap.md`: M2 は終わるので、「マイルストーンの列」の表から M2 の行を、本文から「M2 モジュール」の節を消す。「マイルストーンの進め方」に従い、決まったことは `architecture.md` に移す。M2 を最初に置く理由の項目は、M2 を終えたことに合わせて書き直す。「M2〜M9」の言い方を「M3〜M9」に直す
- `implementation/status.md`: M2 を完了とし、次を M3 にする
- `README.md` と `overview.md`: マイルストーンの範囲の言い方 (「M2〜M9」) を、終えたマイルストーンに合わせて直す

## 範囲外

- M4 の演算子の解決と、M5 の型クラスの証拠と特殊化
- row 変数と Kind 変数の記録
- `Type` から書き出した型引数を、型検査のダンプ (`dump`) に表示すること
