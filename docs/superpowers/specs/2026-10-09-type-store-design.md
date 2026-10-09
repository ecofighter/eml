# 型の表 (設計)

位置づけ: 作業の設計。[実装の現在地](../../implementation/status.md) の「深さと性能」にある、多相な関数を自分自身に続けて適用する式が指数の時間とメモリを使う問題を直す。決まったことは作業の終わりに `docs/` の該当文書へ移し、この文書を削除する。

## 目的

型検査と実行の準備にかかる時間とメモリを、型の表の大きさに比例させる。対象は、次のように、部分を共有する型を木としてたどる処理のすべてである。

- 型検査が推論の後に型を書き出す処理 (`Table::export`)
- 推論の中で型の表をたどる処理 (`occurs`、`row_occurs_in`、`unify`、`kind_bounds`、`kind_vars`)
- 書き出した型を後の段階が複製し、たどり、解放する処理

型の表現は、ネイティブのバックエンドを作るときと salsa に移すときにも、そのまま使える形にする。

## 背景

- 推論の表 (union-find) は部分を共有する。`ident ident … ident 5`、引数を渡さない部分適用、`let f1 = f0 ident` の列では、適用を1つ足すごとに、型を木として書き下した大きさが2倍になる。局所の `let` は一般化しないので、`f0` の型は後の `let` の型を使って伸びていく
- `Table::export` は、式、局所変数、パターン、具体化の型引数ごとに、型を木の `Type` に書き出す。`BodyTypes` が指数の大きさになり、網羅性の検査の `Type::contains_error`、木の解放、Core IR が式ごとに行う `Type` の `clone` も指数になる
- 推論の中にも、表の型を木としてたどる処理がある。`occurs` と `row_occurs_in` は同じ節点を何度も訪れる。`unify` は関数型どうしを単一化しても2つの節点を結び付けないので、同じ組をまたたどる。`kind_bounds` はデータ型とレコードの中に入り、重複を除かずに境界を並べる。`kind_vars` は訪れた節点を覚えない
- `usage.rs` は、状態を持つ handler が `return` を省くたびに、状態の型を文字列にして `KindReason::OmittedReturn` に入れる。誤りのない経路でも型を表示するので、指数になる
- 試作で処理ごとの呼び出し回数を数えると、次の形はどれも長さを2つ増やすごとに約4倍になった。書き出しはどの形でも効き、ほかの処理は形ごとに次のとおりである
  - `ident` の連鎖、部分適用、`let` の列: 書き出しだけ
  - `let` の列の後の `let g = fn () -> put f0`: `occurs` と `row_occurs_in`
  - 2本の `let` の列を `if True then f0 else g0` で合わせる形: `unify` と `unify_row`
  - `let p1 = (p0, p0)` の列: `kind_bounds` と Kind の制約の数 (`usage.rs` の `unr_local` から)
  - `return` を省いた状態を持つ handler に列の型の値を渡す形: `OmittedReturn` の表示
- Core IR は型の根しか読まない。`repr` は根の形で決まり、`split_arrows` は引数の数だけ矢印をたどる。型を新しく作るのは `substitute`、`lang_type`、`Type::unit()`、`Type::Flexible` の4か所である

## 決めたこと

### 型の表

- `eml_types` に `TypeStore` を置き、`TypedProgram.types` に1つだけ持たせる。型は `TypeId(u32)` で指す
- 表の中身は `TypeKind` で、今の `Type` と同じ形である。子は `TypeId` で持つ

  ```rust
  pub enum TypeKind {
      Con { id: TypeDefId, args: Vec<TypeId> },
      Record(Vec<(String, TypeId)>),
      Fn { param: TypeId, effects: Vec<EffectLabel>, tail: Option<RowTail>, ret: TypeId },
      Rigid(String),
      Flexible,
      Error,
  }
  ```

  `EffectLabel` の型引数も `TypeId` で持つ。`RowTail` は変えない
- `intern(kind) -> TypeId` は、同じ形の型がすでにあればその ID を返す (hash consing)。ID が同じことと型が同じことは一致する
- 型が `Error` を含むかどうかは、登録するときに子のフラグと row の末尾から求めて持つ。登録で子をたどるのは `TypeKind::for_each_child` である
- 読む側の API は `kind(id)`、`contains_error(id)`、`display(id, names)` である。表示の文字列は今の `Type::display` と同じである。row のラベルは `EffectLabel::display(types, names)` で表示する
- `intern` は crate の中だけに公開する (`pub(crate)`)。後の段階は型を作れないので、Core IR が表を読むだけであることを型で守れる
- `TypeStore::new(program)` は、決まった型 (`Unit`、`Int`、`String`、`Bool`、`_`、`{error}`) を最初に登録し、`unit()`、`int()`、`string()`、`bool()`、`flexible()`、`error()` で返す
- 表は、プログラム全体で追記だけする登録表である。`check_module` が作り、宣言の検査と本体ごとの検査に `&mut` で渡し、最後に `TypedProgram` に入れる。ID は検査の順に依存するが、後の段階は ID の値に意味を持たせない。salsa に移すときは salsa の interned に置き換える
- `TypedProgram` から `Default` を外す。決まった型のない表を作れないようにするためである
- rigid な変数は名前で登録するので、同じ名前の rigid な変数は同じ ID になる。今も名前だけで書き出しているので、意味は変わらない
- `Type`、`Type::contains_error`、`Type::for_each_child`、`TypeChild` はなくなる

### 書き出し

- `Table::export` を `Exporter` に置き換える。`Exporter` は `&Table` と `&mut TypeStore` を借り、`HashMap<Ty, TypeId>` に結果を覚える。覚えるのは、渡された `Ty` とその代表 (束縛を辿った先) の両方である。記録を表の大きさの配列にしないのは、診断のために短命の `Exporter` を何度も作っても、費用が書き出した節点の数に比例するようにするためである
- `Exporter` が生きている間は表を変更できないので、覚えた結果が古くなることはない
- 本体の検査の後に `BodyTypes` を作るときは、1つの `Exporter` で、式、局所変数、パターン、具体化の型引数をすべて書き出す。表の各節点を書き出すのは1回だけになる
- `check_comparisons` も、本体の `==` と `!=` をすべて1つの `Exporter` で書き出す
- 推論の途中で診断の文言のために書き出す箇所 (`check/report.rs`、`usage.rs`) は、その場で短命の `Exporter` を作り、同じ表に登録する。診断にしか使わない型が表に残っても害はない
- シグネチャから作る型 (`Shape::export`) も同じ表に登録する
- `KindReason::OmittedReturn` は状態の型を `TypeId` で持つ。表示するのは、破れた制約を報告するときだけである。`report_violations` は表を受け取り、`order_key` もそこで表示した文字列を鍵にする。ID を鍵にしないのは、ID が検査の順で決まり、並びが検査の順に左右されるためである。`order_key` を使うのは破れた制約を並べるときだけなので、診断の文言と並びは今と同じである

### 推論の中のたどり方

表の型をたどる処理は、どれも代表ごとに1回だけ訪れる。

- `occurs`、`row_occurs`、`row_occurs_in`: 呼び出しごとに、訪れた代表に印を付ける。印は、表が持つ世代番号付きの配列 (`RefCell<Marks>`) に付ける。呼び出しごとに表の大きさの配列を作ると、呼び出しが多いときに2乗になるためである。世代は入口 (`occurs`、`row_occurs`) でだけ進め、`row_occurs` と `row_occurs_in` の間の再帰では進めない。配列は入口で表の大きさまで伸ばし、世代が一巡したら消す。`occurs` は `&self` で子をたどるので、印の配列は印を確かめて付ける間だけ借りる
- 印を付ける処理 (`occurs`、`row_occurs`、`kind_bounds`、`kind_vars`) は、別の印を付ける処理の途中で始めない。途中で始めると世代が進み、外側の処理が訪れた代表をもう一度たどるので、結果は正しいまま指数の時間に戻り、気づけない。入口は処理の間だけ印の配列を「たどっている」状態にし、debug ビルドではたどっている間に入口を呼ぶと `debug_assert!` で止める
- `unify`: 1回の呼び出しの中で、単一化を終えた代表の組を覚え、同じ組はすぐに `Ok` を返す。覚えるのは複合の型 (型構成子、レコード、関数型) の組だけで、記録は必要になったときに作る。`unify_row` のラベルの型引数の単一化にも同じ記録を渡す。同じ組をもう一度たどっても同じ Kind の制約を同じ由来でもう一度出すだけなので、飛ばしても結果は変わらない。表は occurs の検査で輪を持たないので、単一化の途中の組をもう一度訪れることはない。記録は呼び出しの中だけなので、同じ大きな型の組を何度も単一化すると、そのたびに表の大きさに比例する時間がかかる
- `kind_bounds`: 訪れた代表を覚え、重複した境界を除く。並びは最初に現れた順のままである。Kind の解は残った制約を整列して重複を除くので、診断は変わらない
- `kind_vars`: 訪れた代表を覚える
- 対象外: `open_spine` は関数型の戻り値の側だけをたどる。`close` と具体化は、ソースに書いたシグネチャの `ShapeTy` をたどるので、大きさはソースの大きさで決まる
- 再帰の深さは今と同じく型の深さに比例する。非常に深い型でスタックが尽きることは [実装の現在地](../../implementation/status.md) に記録する

### `eml_types` の中で型を読む側

- 網羅性の検査は `contains_error(id)` を引く
- `equality(program, ty)` は `equality(program, types, ty)` になり、`TypeId` を受ける。`==` の検査は書き出した ID の `contains_error` を見る
- `main` の型の確認 (`check/mod.rs`) は、期待する `Unit -> <IO> Unit` を表に登録し、ID どうしを比べる
- `usage::constrain` は表を受け取る
- `dump` は表を複製し、`Shape::kind_names` がテストの表示のために作る型をそこに登録する。`KindTerm::Of` は `TypeId` を持つ

### Core IR

- `ty(expr)` は `TypeId` を返す。`Copy` なので、式ごとの `clone` はなくなる。`repr(types, ty, hir)` と `split_arrows(types, ty, count)` も ID で受ける。`ProgramBuilder` の extern、操作、コンストラクタの型の表も ID を持つ。`ProgramBuilder` は表を持たず、型を読むメソッドが `hir` と並べて `&TypeStore` を受け取る
- Core IR は表を読むだけで、型を作らない。`substitute`、`field_types`、`con_field_types`、`tuple_field_types`、`lang_type` をなくす。型を作っていた残りの箇所は、型を経ずに値の `Repr` を直接使う (`Type::unit()` は `Repr::Unit`、`String` の型は extern の型の行の `Repr`、クロージャの型の `Type::Flexible` は `tobj`)
- 決定木の出現 (`Occ`) は、型も `Repr` も持たない
  - `Occ::Con` を値にするときの `Repr` は、`ConValue` から決める。`Data(ctor)` ならそのコンストラクタの型の `data_repr`、`Tuple` なら `obj` である。`Data` の `Occ::Con` はいつもフィールドを持ち、タプルは要素を2つ以上持つので、今の `repr(ty)` と同じ値になる
  - 調べる値をまとめるラベルの `Repr` は、呼び出し側が渡す。`Scrutinee::Expr` なら式の型から、`Scrutinee::Occ` なら変数の `Repr` から決める
  - `Known` は型を持たない
- `unpack` と `switch` で作るフィールドの変数の `Repr` は、パターンの型 (`BodyTypes.pats`) から決める。各 case で、そのコンストラクタ (タプル) が最初に現れる行の引数のパターンを使う。今の `field_vars` が変数の名前を探すのと同じ取り方である。型検査は、入れ子を含むすべてのパターンに、そのパターンが受けた値の型を記録している (`bind_pat`)。`switch` の case はどれかの行に現れるコンストラクタだけなので、引数のパターンはいつもある
- `con` で作った値 (`Known`) のフィールドは、`con` に渡したアトムをそのまま出現にする
- 出現が型を持たないので、`materialize_once` が同じ値を1回だけ作る判定は、型の違いを区別しなくなる。1つの葉が、同じコンストラクタと同じフィールドのアトムの出現を2回渡すと、型が違っても `con` が2回から1回になる (`data P a = P Int` の `P 1` を2つの型で渡す場合や、引数のないコンストラクタのタグを違う型のフィールドに持つ場合)。2つの `con` の命令はもともと同じだったので、意味は変わらない

## 対象外

- 引数の多い1つの呼び出しで、持ち越しの制約を、生きている値と矢印の組ごとに作ること (`carry.rs` の `carry` と `row_multiplicities`)。`ident ident … ident 5` と部分適用は、直した後も長さの2乗の時間がかかる。[実装の現在地](../../implementation/status.md) に記録する
- 値を使うたびに型をたどる Kind の検査 (`kind_at_most`)。`let p1 = (p0, p0)` の列は、直した後も長さの2乗の時間がかかる。同じく記録する
- 同じ大きな型の組を何度も単一化すること (上の「推論の中のたどり方」)。同じく記録する
- 再帰を作業の列に置き換えて、型の深さによらずスタックが尽きないようにすること
- 型を木として表示する処理。診断の文言の長さそのものが型の木の大きさなので、表示は木の大きさに比例したままである

## テスト

### 足すテスト

UI テストを `tests/ui/run/runtime/` に形ごとに1つのファイルで足す。どれも長さは40で、指数の実装では終わらず、線形の実装ではすぐ終わる。指数に戻ったときは失敗ではなく、終わらないかメモリが尽きる形で表れる。どのファイルも冒頭に `ident : a -> a` と `ident x = x` を持つ。

1. `ident` の連鎖: `println (show_int (ident ident … ident 5))`
2. 部分適用: `let g = ident ident … ident` の後に `println (show_int (g 5))`
3. `let` の列: `let f0 = ident`、`let f1 = f0 ident`、…、`let f40 = f39 ident` の後に `println (show_int (f40 5))`
4. `occurs` と `row_occurs_in`: `effect State s` (`get : Unit -> s`、`put : s -> Unit`) を宣言し、3 の列の後に `let g = fn () -> put f0` と `println (show_int (f40 5))`
5. `unify`: 3 の列と、同じ形の `g0` から `g40` の列の後に、`let h = if True then f0 else g0` と `println (show_int (f40 5))`
6. `kind_bounds`: `let p0 = 1`、`let p1 = (p0, p0)`、…、`let p40 = (p39, p39)` の後に `println "done"`
7. `OmittedReturn`: `effect Ask` (`ask : Unit -> Int`) を宣言し、3 の列の後に次を置く

   ```
   let n =
     handle ask () * 2 from f0 with
       | ask () k s -> k (f40 21) s
   println (show_int n)
   ```

`crates/eml_types/tests/scaling.rs` に2つの形を足す。3 の `let` の列と、5 の2本の列の単一化である。大きさはほかの形と同じく 2000 と 8000 にし (1本の列の長さ)、時間の比が6以下なら通る。型の深さが大きさに比例し、書き出し、単一化、occurs の検査の再帰がその深さまで進むので、この2つの形は大きなスタックのスレッドで測る。1、2、6 の形は、直した後も2乗の時間がかかる (「対象外」) ので、時間の比を測る形には使わない。

Core IR の出現が型を持たないことによる変化 (「Core IR」の最後の項目) は、2つのテストで示す。`crates/eml_core_ir/tests/translate.rs` に、`P 1` を2つの型で1つの葉に渡すと `con` が1回になるテストを足す。`tests/ui/run/data/same_value_at_two_types.em` は、ヒープのフィールドを持つ同じ値を2つの型で渡して片方を返し、まとめた `con` の参照カウントが `debug_heap` の検査を通ることを確かめる。

### テストの変更

- pass と fail が変わるテストはない
- 機械的な追随 (kind 3): 型の API の変更に合わせた書き直しで、期待値は1バイトも変えない。`ty.rs` と `table/tests.rs` の単体テスト、`shape.rs` の単体テスト、`kind/mod.rs` と `check/mod.rs` の単体テスト、`eml_types` の `tuples.rs`、`instantiations.rs`、`check.rs`、`eml_core_ir` の `type_repr` と `tests/externs.rs` が対象である
- 期待値の変更 (kind 2): なし。既存の Core IR のダンプのうち、「Core IR」の最後の項目で `con` が1回に減るものはない (試作で確かめた)。この変化は、足すテスト (translate のテストと UI テスト) で示す。既存のダンプや診断が変わったら、期待値を直さずに原因を調べる

## 確認の手順

```sh
cargo test
cargo test --release -p eml_types --test integration scaling:: -- --ignored
cargo clippy --all-targets && cargo fmt --check
cargo clippy -p eml_cli --no-default-features --features types
cargo clippy -p eml_cli --no-default-features --features core
nix build
```

## 更新する文書

- [実装の現在地](../../implementation/status.md) の「深さと性能」: 指数の項目を消す。「対象外」の2乗の3項目と、非常に深い型でスタックが尽きることを足す
- [コンパイラの構成](../../implementation/architecture.md)
  - 別テーブルの説明 (`ExprId → Type`) を `ExprId → TypeId` にする
  - 「`eml_types` の内部」: 本体を独立に検査する説明と、宣言ごとに結果を持つ説明に、型の表は追記だけする共有の登録表であることを足す。具体化の型引数は ID になる。rigid な変数を名前で登録するので同じ名前の変数が同じ ID になることを、節の型変数を区別できないという項目に書き足す。`Table::export` の項目を `Exporter` と型の表の説明にする
  - 型の走査の規則 (`for_each_child`) を、1つの型をたどる処理は `for_each_child` の子を、代表ごとの印を付けてたどる、という規則にする。単一化の組の記録はこの規則の外にあることも書く
  - `Type::Con` と `EffectLabel` の表示の項目を、`TypeStore::display` にする
  - 型の表の項目を足す (hash consing、`Error` を含むかのフラグ、決まった型、Core IR は読むだけ)
  - 決定木の出現の項目に、出現が型を持たないことと、`Repr` を `ConValue`、パターンの型、調べる値から決めることを書く
- [テスト](../../implementation/testing.md) の性能のテスト: 形の数を6から8にし、足した2つの形と、大きなスタックのスレッドで測ることを書く。`runtime/` の UI テストの例に、型の表を共有する形を足す
- `BodyTypes.pats` の doc コメント (`crates/eml_types/src/lib.rs`): Core IR がフィールドの変数の `Repr` に使うことを書く

## 完了の条件

- 「確認の手順」のコマンドがすべて通る
- 足した7つの UI テストが、debug ビルドの `cargo test` で通る
- `scaling.rs` の足した2つの形が、release ビルドで時間の比6以下になる
- 既存の Core IR のダンプと診断の期待値は変わらない
- 「更新する文書」をすべて直し、この文書を削除する
