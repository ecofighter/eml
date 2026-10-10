# S6a リスト

位置づけ: 作業の設計。ロードマップの S6 (リスト、文字列、レコード) を S6a、S6b、S6c に分け、最初の S6a を定める。S6a を終えたら、決まったことを `docs/spec/` と `docs/implementation/` に移し、この文書を削除する。

## 目的

Prelude に `List`、`Option`、`Result` を置き、リストのリテラル `[…]` と `::` を使えるようにする。S9 (`eml_rt`) でデータ構造のランタイム表現を決めるときに、標準のデータ型を使う実際のプログラムで詰められるようにするためである。

リストを扱う関数 (`map`、`filter`、`length` など) は入れない。関数の API は、S12 の標準ライブラリの spec で、`List.map` の形のモジュールと一緒に決める。S6a の UI テストは、要る関数をテストの中で定義する。

## 段の分け方

ロードマップの S6 を次の3段に分ける。

| 段 | 中身 | 前提 | 完了の条件 |
|---|---|---|---|
| S6a リスト | `List`、`Option`、`Result`、リストのリテラルとパターン、`::` | なし | 下の「完了の条件」 |
| S6b 文字列 | lexer のモード、補間と `Show`、複数行の文字列、raw 文字列 | S6a | 各構文の UI テストが通る |
| S6c レコード | 名前的なレコード、射影、更新、`t.0`、`(.x)`、レコードの `deriving` | S6b | 各構文の UI テストが通る |

- S6a を先に置くのは、括弧で書く構文の CST を、論点の少ないリストで先に作るためである。レイアウト規則3と `{…}` の論点は S6c で決める。S6b の補間の UI テストでは、`Show (List a)` を穴に使える
- ロードマップの S6 の論点のうち、`::` の fixity とリストの CST は S6a で決める。「補間、レコード、リストの CST」の残りは S6b と S6c に、「借用のオペランドと `Field`」と「レコードの `Ord` のフィールドの順」は S6c に回す
- S9 の前提は、S6c (S6 の全体) のままにする

## 前提として確かめたこと

- 自分のモジュールの定義は、Prelude の同じ名前を診断なしで隠す ([モジュールと名前解決](../../spec/modules.md) の名前の引き方の段2)。自分で `Option` や `List` を定義する今のテストとベンチマークは、Prelude に同じ名前ができても動く。UI テストと `bench/` の独自の `List` は、コンストラクタに `Cons` を使い、`::` を定義していない。`::` を定義するテストは、構文だけを見る `eml_syntax` の `tests/declarations.rs` の1件だけである
- 設計のレビューで、リポジトリの複製の `std/Prelude.em` に下の「Prelude」の宣言を足し、全テストを流した。Prelude の文脈でも通ること (extern の表との対応のテスト、E2012、`::` の導出、`pub` でない `show_rest`) と、`run` と `run-fail` の UI テストがすべて通ることを確かめた。失敗したテストは、下の「テストの変更」の表のものだけだった
- そのとき、次の振る舞いも確かめた。`show` の出力は `[1, 2]`、`Some [1]`、`Some (-1)`、`[]`、`Ok "a"` になる。`compare (1 :: Nil) (1 :: 2 :: Nil)` は `LT` である。セクション `(1 ::)`、`(::)`、`(:: Nil)` は今の実装で動く。配置 `Prelude.List { Nil, ::(tobj, tobj) }` は IR のテキストとして読み戻せる。使わない `List Fs.File` の引数は E3003 になる。`show` の `string_bytes_copied` は長さ 1000、2000、4000 で 12676、28676、60676 で、長さに比例する。導出した `==` と `compare` は末尾呼び出しで進む
- 型が決まらない `[]` と `Nil` の `show` と `==` は、既定の型を選ばないので E2009 になる ([型と Kind](../../spec/types.md) の「制約」)

## Prelude

`std/Prelude.em` に次を足す。

```haskell
pub infixr 5 ::

pub data List a =
  | Nil
  | a :: List a
  deriving (Eq, Ord)

pub data Option a =
  | None
  | Some a
  deriving (Eq, Ord, Show)

pub data Result e a =
  | Err e
  | Ok a
  deriving (Eq, Ord, Show)

instance Show a => Show (List a) where
  show Nil = "[]"
  show (x :: rest) = show_rest ("[" ++ show x) rest

-- 左辺を一意な文字列として伸ばすので、長さに比例する時間で済む (docs/spec/runtime.md の「文字列の連結」)
show_rest : Show a => String -> List a -> String
show_rest acc Nil = acc ++ "]"
show_rest acc (x :: rest) = show_rest (acc ++ ", " ++ show x) rest
```

- `::` の fixity は `++` と同じ `infixr 5` で、Haskell の `:` と同じである。fixity の宣言を `infixr 5 ++` の行に並べずに `data List` の前に置くのは、宣言と使う所を近くに置くためである
- `Result` の型引数は `Result e a` の順にする。[マルチコア対応の設計](../../future/multicore.md) の `Result e b` と、Haskell の `Either e a` に合わせる。コンストラクタを `Err`、`Ok` の順に宣言するので、導出した `Ord` では `Err _ < Ok _` になる
- `List` の `Show` だけを手で書く。`[1, 2, 3]` の形で表示するためである。`show_prec` は既定のまま (`show_prec _ x = show x`) にする。リストの表示は角括弧で閉じているので、引数の位置でも括弧が要らない。`Option` と `Result` の導出した `Show` は、`Some [1]`、`Some (-1)`、`Ok "a"` のように出す
- `show_rest` は `pub` にしない。ユーザーからは定義がないものとして扱われる ([モジュールと名前解決](../../spec/modules.md) の「標準ライブラリ」)
- 導出した `Ord` は `Nil < _ :: _` と要素ごとの辞書順になり、Haskell のリストの順と同じになる
- `List` の Kind は、ほかの `data` と同じく、フィールドの Kind から推論する。`List Fs.File` は `Lin` である

## `::` の予約

`::` は Prelude のリストのコンストラクタだけの名前にする。Haskell が `:` を予約しているのと同じである。

- Prelude 以外のモジュールで、コンストラクタ `::` を宣言すると E1044 (`RESERVED_CONSTRUCTOR`) にする。宣言ごとに1件出す。コンストラクタはそのまま宣言したものとして扱い、そのモジュールの `::` の使用には誤りを重ねない
- 予約する理由は2つある。1つ目に、自分の `::` を定義したモジュールでは、Prelude のリストを頭と残りに分解する書き方がなくなる。`Prelude.::` のような修飾した演算子の構文はない ([モジュールと名前解決](../../spec/modules.md))。2つ目に、2つのモジュールが `::` を定義すると、表示の表が両方を `Prelude.::` と `Main.::` に修飾し、診断と網羅性の例に書けない形が出る
- 予約により、`x :: xs` は式でもパターンでも、どのモジュールでも Prelude の `::` を指す。名前の引き方は変えない。誤りのないプログラムでは、`::` を定義するモジュールが Prelude だけになるためである
- `Nil` は予約しない。ユーザーは自分の `Nil` を定義でき、今の規則どおり Prelude の `Nil` を隠す。Prelude の `Nil` は、`[]` か `Prelude.Nil` で書ける

## 構文

### 文法

`grammar.md` の `atom` の `'[' list(expr) ']'` と、`apat` の `'[' list(pat) ']'` を実装する。リストの規則は変えない。

- 式 `[e1, e2, …]` とパターン `[p1, p2, …]` を書ける。末尾のカンマも書ける (`list(x)` の規則)。`[]` は空のリストである
- 型の構文は `List a` だけで、`[a]` の形はない。型の位置の `[` は、今のパーサのとおり型の誤り (E0011) になる。ほかの言語の書き方を示す help は付けない
- CST は、式を `LIST_EXPR`、パターンを `LIST_PAT` の節点にし、その下に要素の節点と `,` を並べる。typed AST に `ListExpr::elements()` と `ListPat::elements()` を足す
- 括弧の中の要素の読み方は、タプルの式とパターンと同じである。要素は `expr` と `pat` なので、`if` や `fn` も括弧なしで書ける
- `{…}` は S6c まで、今のとおりパーサが E0004 を出す
- `grammar.md` の `op_expr`、セクションの `atom`、`fixity_item` の `OP` は、今の文法の記述では `:` で始まる演算子を含まない。パーサはどれでも `CONOP` を受け付けているので、文法の記述を `(OP | CONOP)` に直す。`var` と `equation` の `OP` はそのままにする。コンストラクタにはシグネチャと等式がないためである

### 深さ

- `[` は、括弧と同じく1段に数える
- パターンのリストでは、k 番目 (1 始まり) の要素を、`[` の中の深さに k - 1 を足した深さで読む。HIR がパターンのリストを右に入れ子の木に組むためである (下の「HIR」)。組んだ木では前の要素がいつも浅い位置に来るので、中置のコンストラクタのパターンのように、前の要素の高さを下に確保する必要はない。外側に入れ子がなく、要素が変数か `_` なら、1つのパターンのリストに書ける要素は約250個になる。正確な境目の数は、決まった形 (`f [x1, …, xn] = 0`) のテストで固定する
- 式のリストは、要素の数を深さに数えない。HIR が平らな節点で持ち、後の段階もループでたどるためである

### レイアウト

- `[` は今もレイアウトの `Bracket` を積む ([レイアウト規則](../../spec/layout.md) の規則4)。複数行のリテラルは規則2で読め、レイアウトの規則は変えない

  ```haskell
  main () =
    let xs = [
      1,
      2,
    ]
    println (show xs)
  ```

## HIR

- `LangItems` に `list` (型)、`nil`、`cons` (コンストラクタ) を足す。`true_ctor` と同じく、Prelude の宣言から名前で引く
- `[…]` は、式でもパターンでも、名前を引かずに Prelude の `Nil` と `::` を指す。ユーザーが自分の `Nil` を定義しても、リストの構文の意味は変わらない。中置で書いた `x :: xs` は、ふつうの名前解決で Prelude の `::` に届く (上の「`::` の予約」)
- 式: `ExprKind::List(Vec<ExprId>)` を足す。`[]` も要素のない `ExprKind::List` にする
- 要素のない `ExprKind::List` は、引数をまとめて渡す規則の「値」に入れる (`eval.rs` の `is_value`。[式](../../spec/expressions.md) の「関数適用」)。コンストラクタの参照 `Nil` が値なので、`f a [] b` と `f a Nil b` の評価のまとまりをそろえるためである。要素のあるリストは、タプルと同じく値に入れない
- パターン: `[p1, …, pn]` を `p1 :: (p2 :: (… (pn :: Nil)))` の `PatKind::Con` に組む。コンストラクタは `lang.cons` と `lang.nil` である。変数は要素の順 (左から) に束縛する。E1017 の組と局所変数の番号を、ソースの順にそろえるためである
- 組んだパターンの範囲は次のとおりにする。1つ目の `::` は `[` から `]` まで、k 番目 (k ≥ 2) の `::` は p_k の始まりから `]` まで、`Nil` は `]` である。`[]` のパターンは `Nil` 1つで、範囲は `[]` 全体である
- `::` の式とパターンの E0004 の分岐 (`lower/ops.rs` と `lower/expr.rs` の `constructor_pat`) を消す。Prelude に `::` があるので、ふつうの名前解決で引ける
- HIR のダンプ (`pretty`) は、`ExprKind::List` を `[a, b]` と書く。パターンの `lang.cons` と `lang.nil` は、網羅性の例と同じ規則 (下の「網羅性」) で書く

## 型検査

- `ExprKind::List` の型は `List t` である (`List` は `lang.list`)
  - 期待する型が `List t` に単一化できるときは、各要素を `t` に対して検査する。誤りは要素の位置で報告する (E2001)
  - それ以外では、新しい型変数 `t` を作り、最初の要素の型を推論して `t` にし、2つ目からの要素を `t` に対して検査する。新しい由来 `Origin::ListElements(最初の要素の範囲)` を付け、`MatchArms` と同じく、最初の要素の位置に「the first element has this type」の secondary を出す。診断のコードは足さない
  - 空のリストは `List t` で、`t` は新しい型変数のまま残る
- 線形性 (`usage.rs`) は、`ExprKind::Tuple` と同じく、要素を左から順にたどる。要素はそれぞれ1回消費される
- 持ち越し (`carry.rs`) は、要素を左から順に評価する部分として扱う。ただし、評価済みの要素は、最初の要素1つで代表させる (`Held` の1項目)。要素の型がどれも同じなので、持ち越しの制約も同じになるためである。今の `parts` のように要素を1つずつ持つと、生きている値の集合を要素ごとに写すので、要素の数の2乗の時間がかかる (タプルの同じ形で、10000要素の `eml check` に約21秒かかった)。持ち越しの誤りは、代表の最初の要素の位置で報告する
- リストのパターンの型の誤りは、HIR が組んだコンストラクタのパターンとして報告する。そのため「`::` is a constructor of `List`」の note が出る。`::` はユーザーが書ける名前なので、この note のままにする
- 網羅性は、パターンを HIR がコンストラクタに組むので、検査そのものは変えない。漏れの例の表示だけを次のように変える (下の「網羅性」)

### 網羅性

漏れの例の書き方を、次のとおりにする。

- `lang.nil` は `[]` と書く。表示の表の修飾によらない
- `lang.cons` の鎖が `lang.nil` で終われば、リストの形で `[_, Some _]` のように書く。要素は、タプルの要素と同じく括弧なしの形で書く
- それ以外の `lang.cons` の鎖は、`_ :: _ :: _` のように右結合として書き、右の被演算子に括弧を付けない。左の被演算子は、今と同じく引数を持つコンストラクタを括弧で囲む (`(Some _) :: _`)。`::` は予約したので、修飾せずに書く
- `[]` とリストの形は、括弧の要らない形として扱う。引数の位置と `::` の左の被演算子にも、括弧なしで書く (`Some [_]`、`[[_]]`、`[_] :: _`)
- ユーザーが定義した中置のコンストラクタの書き方は変えない
- [網羅性](../../spec/exhaustiveness.md) に、この書き方を足す

## translate

- `ExprKind::List` は、要素を左から評価して変数に入れた後、`Nil` から始めて右の要素から順に `::` の `con` を積む。ループで組むので、要素が多くてもスタックは深くならない。要素を左から評価するのは、[式](../../spec/expressions.md) の「関数適用」の評価の順と同じ方針である
- 要素に呼び出しがあると、末尾でない呼び出しのたびに、それまでに評価した要素を `save` に並べる。そのため、呼び出しを要素に持つ長いリテラルでは、IR の `save` の大きさと、インタプリタのフレームの退避が、要素の数の2乗になる (タプルも同じである)。要素を先に評価して逆順のリストに積み、後で反転すれば避けられるが、確保が倍になるので採らない。この制限は [実装の現在地](../../implementation/status.md) の「深さと性能」に書く。要素が変数とリテラルだけなら線形である
- `[]` は、コンストラクタ `Nil` の参照と同じ値 (`Atom::Tag`) にする
- `Prelude.List`、`Prelude.Option`、`Prelude.Result` の配置は、ほかのデータ型と同じく、IR が初めて使ったときに配置の表に入る
- boxing、contract、Perceus、verifier、インタプリタ、ランタイムは変えない

## 診断

- 新しい診断のコードは E1044 (`RESERVED_CONSTRUCTOR`) だけである。[診断](../../spec/diagnostics.md) の表に足す
- リストの要素の型が合わなければ E2001 になる (上の「型検査」)
- 要素の型が決まらない `[]` の `show` や `==` は E2009 になる。型の明示 (`([] : List Int)`) で直す。既定の型を選ぶ規則は足さない
- 自分で `Option`、`List`、`Result`、`None`、`Some`、`Nil`、`Ok`、`Err` を定義したモジュールがあると、Prelude と同じ名前の型とコンストラクタになる。表示の表はプログラム全体で1つなので、どのモジュールの診断でも、両方を `Prelude.Option` と `Main.Option` のように修飾する。Core IR の関数の名前 (`Ord Main.Option.compare@[Int]`) と HIR のダンプの型も同じである。配置の名前と導出した `Show` の出力は、今の規則のとおり変わらない。どれも今の規則の帰結で、規則は変えない

## テストの変更

### 成否の変更

- `tests/ui/check-fail/not-yet-supported/cons_pattern.em` とそのスナップショットを消す。`::` のパターンの E0004 を示すテストで、`::` は未対応でなくなるためである
- `crates/eml_hir/tests/lower.rs` の `cons_patterns_are_not_supported_yet` を消す。同じ理由で、HIR の診断がなくなる

### 期待値の変更

- 次のテストの期待値を書き直す。Prelude の宣言を足した複製で失敗したテストの全体である

  | crate | テスト | 理由 |
  |---|---|---|
  | UI check-fail | `classes/no_instance`、`exhaustiveness/non_exhaustive_equation`、`exhaustiveness/non_exhaustive_equations`、`exhaustiveness/refutable_pattern`、`types/constructor_pattern_mismatch` | 名前の修飾 |
  | `eml_core_ir` | `perceus::a_nested_pattern_gives_up_the_parent_before_the_release`、`translate::derived_instances_generate_their_methods` | 名前の修飾 |
  | `eml_hir` | `def_map::prelude_fixities_follow_the_standard_table` | `::` の fixity が `infixr 5` になる |
  | `eml_hir` | `data::match_arms_and_constructor_patterns`、`data::data_declarations_become_items`、`tuples::literal_patterns_include_negative_numbers_and_strings` | 名前の修飾 |
  | `eml_hir` | `operators::unknown_and_unsupported_operators_and_missing_operands` | `::` の E0004 がなくなる |
  | `eml_hir` | `data::infix_constructors_and_a_declared_cons`、`lower::a_user_defined_cons_constructor_is_matched` | ユーザーのモジュールで `::` を宣言するので、E1044 が1件出る。どちらも E1044 のほかに診断を出さず、宣言した `::` で組み、照合することを確かめるテストとして残す (上の「`::` の予約」の、使用には誤りを重ねないこと) |
  | `eml_types` | `data.rs` の5件、`exhaustive.rs` の9件 | 名前の修飾 |

- 名前の修飾による差分は、上の「診断」の修飾に限る。テストのソースは、名前で関数を引く所だけを書き換える (`eml_core_ir` の `tests/translate.rs` の `"Ord Option.compare@[Int]"` と `"Show Option.show_prec@[Int]"` を、修飾した名前にする)。`def_map.rs` の `::` の fixity の断言とその注釈は、`infixr 5` に書き直す
- リストの構文の E0004 を期待する `eml_syntax` のテストを、新しい CST と診断に書き直す。`tests/expressions.rs` と `tests/declarations.rs` のリストの例、`tests/corpus.rs` の E0004 の文言の一覧とコーパスのスナップショットである。閉じ括弧の回復を見る `mismatched_closing_bracket_ends_an_unsupported_list` と `implicitly_closed_bracket_in_an_unsupported_list_does_not_swallow_the_file` は、回復を見る目的を保つ。名前から `unsupported` を外し、期待値を E0004 のない形 (閉じ括弧の誤りの E0011 だけ) にする
- `eml_syntax` の `tests/declarations.rs` で `::` を宣言する例は、構文だけを見るので変わらない
- `bench/` の `RunStats` は変わらない (複製で確かめた)。`bench/run.sh` の命令の数は、Prelude の構文解析と検査が増えるので変わる。記録するときに、その理由を書く

### 新しいテスト

テストを先に書く (TDD)。

- UI `tests/ui/run/lists/`
  - リテラルと `::` で作り、`match` の `[]`、`[a, b]`、`x :: rest` で分解する
  - 複数行のリテラルと末尾のカンマ
  - `Show`: `([] : List Int)`、`[1, 2]`、`["a"]`、`[-1]`、`Some [1]`、`[Some (-1)]`、`[[1], []]`
  - 導出した `Eq` と `Ord`: `[1] == [1]`、`compare [1] [1, 2]`、`compare [2] [1, 2]`、`compare ([] : List Int) [1]`
  - `Option` と `Result` の値と、導出した `Eq`、`Ord`、`Show`
  - `::` のセクションと参照: `(::)`、`(1 ::)`、`(:: [])`
  - fixity の組み合わせ: `1 :: 2 :: []`、`-1 :: xs`、`x :: xs == ys`
  - ユーザーが自分の `Nil` を定義したモジュールでも、`[]` は Prelude の空のリストになる
  - 要素の評価の順 (`println` を起こす関数を要素に書く)
  - `Lin` な要素のリスト (`[f]` を作り、`[f]` のパターンで分解して閉じる)
- UI `tests/ui/check-fail/`
  - `types/list_element_mismatch.em`: 要素の型の不一致 (E2001)。期待する型がある形 (`xs : List String` に `[1]`) と、ない形 (`["a", 1]` で最初の要素の secondary が出る)
  - `exhaustiveness/list_patterns.em`: 漏れの例の表示 (`[]`、`[_]`、`_ :: _ :: _`、`[[_]]`、`Some [_]`、リテラルの要素 `[0]` から出る `[_]`)
  - `linearity/list_of_files.em`: `List Fs.File` を使わずに捨てる誤り (E3003)
  - `names/reserved_cons.em`: ユーザーのモジュールでの `::` の宣言 (E1044)
  - `classes/ambiguous_empty_list.em`: `show []` (E2009)
- `eml_syntax`: `LIST_EXPR` と `LIST_PAT` の CST、空のリストと末尾のカンマ、型の位置の `[` が E0011 になること、パターンのリストの深さ (`f [x1, …, xn] = 0` で、通る最大の n と、その次の n で E0013 になること)、式のリストは要素の数を数えないこと
- `eml_hir`: パターンのリストの組み方、束縛の順、範囲、ユーザーの `Nil` や `import M (List(..))` があってもリストの構文が Prelude を指すこと、`[]` が値に入ること、E1044、`pretty` の出力
- `eml_types`: 網羅性の漏れの例の表示、`Origin::ListElements` の診断
- `eml_core_ir`: `[a, b]` の translate が、要素を左から評価してから右から `con` を積む IR になること
- `eml_types` の `scaling.rs` (性能のテスト): 呼び出しを要素に持つリテラルの型検査の時間が、要素の数にほぼ比例すること (2000 と 8000 で比が6以下)
- `eml_interp` の `scaling.rs`
  - 要素が10000個の、リテラルを要素に持つリストを生成して `run` が通ること
  - `show` の `string_bytes_copied` が、長さ n と 2n で2倍程度 (比が 2.5 以下) になること
  - 要素が10000個のリストの、導出した `==` と `compare` が通り、`peak_objects` がリストの大きさの定数倍に収まること

## 文書の更新

S6a の終わりに1回で直す。

- [宣言](../../spec/declarations.md): 先頭の `len` の例と `data` の節の `List` の例を Prelude の宣言に合わせる。`::` の fixity の注記と標準の演算子の表の `::` を、Prelude にあるものとして書き直す。Prelude の型として `List`、`Option`、`Result` と `List` の `Show` の形を書き、Prelude の instance の表に足す。`::` の予約 (E1044) を書く
- [式](../../spec/expressions.md): リストのリテラル、`[…]` がいつも Prelude を指すこと、要素の評価の順、`[]` が値に入ること
- [文法](../../spec/grammar.md): `op_expr`、セクション、`fixity_item` の `(OP | CONOP)`。文法上の補足に、リストの深さの数え方を足す
- [網羅性](../../spec/exhaustiveness.md): 漏れの例のリストの書き方
- [診断](../../spec/diagnostics.md): E1044 の行。E0004 の説明の「S6」を、S6b と S6c に直す
- [モジュールと名前解決](../../spec/modules.md): Prelude の範囲から「S6 で入る」を消す
- [Core IR とインタプリタ](../../spec/core-ir.md): 変換の規則に、リストのリテラルの評価と組み立ての順を足す
- [実装の現在地](../../implementation/status.md): 今の言語の範囲、「未対応の構文と E0004」から `::` と `[…]` を消す (型の位置の `[` は E0004 でなく E0011 であることも直す)。「深さと性能」に、パターンのリストの要素の上限と、呼び出しを要素に持つ長いリテラルの `save` の2乗を足す
- [コンパイラの構成](../../implementation/architecture.md): `LangItems` の項目と `ExprKind::List`、持ち越しの代表の扱い
- [テスト戦略](../../implementation/testing.md): 「性能のテスト」に、新しい scaling のテストを足す
- [ベンチマーク](../../implementation/benchmarks.md): 「S6 まではリストのリテラルも…ない」の文を直し、S6a の記録を足す
- [標準ライブラリへの申し送り](../../future/stdlib.md): Prelude の範囲の記述を今に合わせる。補間の「S6」を S6b に直す
- [ロードマップ](../../future/roadmap.md): 段の列の S6 の行を、S6b と S6c の2行にする。S6 の節を「S6b 文字列」と「S6c レコード」の2節に分け、リストと `List` / `Option` / `Result` と `::` の fixity に関する項目を消す。論点は上の「段の分け方」のとおりに分ける
- 「S6」を引用する文書と注釈を、S6b か S6c に直す。`citations.rs` が引用する見出しの存在を確かめるので、見出しを分けるのと同時に直す
  - 見出しの引用: [字句](../../spec/lexical.md) の「S6 リスト、文字列、レコード」(補間なので S6b) と、[evidence passing の設計](../../future/evidence-passing.md) の同じ見出し (レコードなので S6c)
  - 文書の「S6」: `lexical.md`、`expressions.md`、`declarations.md`、`core-ir.md`、`overview.md` の直積型の行と段の一覧、`architecture.md`、`status.md`
  - 注釈の「S6」: `eml_syntax` の `literal.rs`、`syntax_kind.rs`、`grammar/mod.rs`、`grammar/items.rs`、`lexer/string.rs`、`eml_hir` の `lower/expr.rs`
- [プログラム例](../../spec/examples.md) は、S6 の全体を終えたときに直す
- `CLAUDE.md`: Syntax の節の、S6 で入れる予定の構文の並びからリストを消し、S6b と S6c に直す

## 完了の条件

- 上の新しいテストが通り、`cargo test`、`cargo clippy --all-targets`、`cargo fmt --check`、`nix build` が通る
- 期待値の変更が、上の「テストの変更」に挙げた範囲に収まっている
- `bench/run.sh` の結果を `benchmarks.md` に記録する
