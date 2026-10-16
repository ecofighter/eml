# S13a Kind の記法の設計

## 目的と範囲

S13 (実例による判断) の1つ目の作業である。利用者が型に Kind を書けるようにする。書けるのは、1回だけ呼べる関数の型 `once a -> b`、`data` の row の型引数、`data` の Kind の宣言 `data T : Lin` の3つである。あわせて、トップレベルの関数の参照を `once` の位置に置けるようにする。

ねらいは、`once` の操作の継続を捕まえた閉包を `data` に入れられるようにすることである。使い道は、S13b で std に入れる `Iter`、コルーチン、async のキュー (`once` の操作で捕まえた継続をキューに入れ、スケジューラが取り出して再開する形) である。今は `data` のフィールドの矢印が `Unr` に固定され (E2001)、row の型引数も書けない (E1002) ので、`multi` の操作と `<IO>` に固定した形でしか書けない。

ロードマップの S13a の「決めたこと」(`docs/future/roadmap.md`) を、この spec が細部まで決める。完了の条件は、ロードマップのとおり「`once` の継続を入れた閉包を持つ `data` と、row の型引数を持つ `data` が UI テストで動く」ことである。

次のものは範囲に入れない。どれもロードマップの「型システム」に置いた。

- 多重度の多相 (`data` のフィールドの矢印の線形性を型引数で変えること)
- `Unr` の宣言 (`data T : Unr`)
- row の引数の省略 (`Iter a` を `Iter <> a` と読むこと)
- 関数型の包摂全体 (`Unr` の局所変数を `once` の位置に渡すこと)。今までどおりラムダで包む
- `Lin` のフィールドを持つレコードの射影の負担を減らすこと。`r.name` は、残りのフィールドに `Lin` があれば今と同じく E3004 になる (「線形値の受け渡しの糖衣」)

std の変更 (`Iter` など) は S13b で行う。S13a は std を変えない。

決めたことは、段の終わりに下の「段の終わりの文書」の場所へ移す。

## 利用者から見た動き

### 1回だけ呼べる関数の型 `once`

```haskell
data Iter <e> a =
  | Done
  | More a (once Unit -> <e> Iter <e> a)

data Task <e> =
  | Task (once Unit -> <e> Unit)
```

- 関数型の前に `once` を置くと、その関数型の一番外側の矢印が `Lin` になる。1回だけ呼べる関数 (Rust の `FnOnce` に当たる) の型である
- `once` は、後ろに続く関数型の一番外側の矢印にだけ付く。`once a -> b -> c` は1回だけ呼べる関数で、返す `b -> c` には付かない。`a -> once b -> c` は、1回だけ呼べる関数を返す関数である
- `once` の後ろは関数型でなければならない。`once Int` は構文の誤り (E0011) にする
- `once` の操作の継続 `k` の型は `once a -> <e> r` になる。`once` という語は、操作の多重度と同じ意味を表す
- Linear Haskell の `a %1 -> b` とは意味が違う。あちらは「引数をちょうど1回使う関数」である。eml の `once` は、関数の値を何回呼べるかを表す

書く場所ごとの意味は次のとおりである。`once` を書かない `->` の意味は、どこでも今と変わらない。

| 場所 | `->` | `once … ->` |
|---|---|---|
| `data` のフィールド | `Unr` | `Lin` |
| 操作の引数の型、クラスのメソッドのシグネチャの内側の矢印 | `Unr` | `Lin` |
| トップレベルのシグネチャの内側の矢印、型の明示 `(e : T)`、ラムダの引数の型 | 推論する | `Lin` に固定する |
| トップレベルの関数、extern の関数、クラスのメソッド、操作のシグネチャの一番外側の矢印 | `Unr` | 誤り (下の「誤り」) |

最後の行の矢印は、宣言した関数そのものを表す。トップレベルの関数は何度でも呼べるので、`once` を書いても意味がない。

`data` の Kind は、今と同じくフィールドの Kind の join で求める。`once` の矢印は定数の `Lin` の型として数える。そのため、`Iter` と `Task` は型引数によらず `Lin` になる。`(Int, once Unit -> Unit)` や `Option (once Unit -> Unit)` のように、別の型の中に置いても同じである。ただし、別の関数型の中にある `once` の矢印 (`Unit -> once Unit -> Unit` の内側) は、外側の関数の Kind に効かない。今の「関数型の中の型引数は効かない」規則と同じ考え方である。

### row の型引数

```haskell
data Iter <e> a =
  | Done
  | More a (once Unit -> <e> Iter <e> a)

take  : Int -> Iter <e> a -> Iter <e> a
lines : File -> Iter <IO> String
empty : Iter <> a
both  : Iter <IO | e> a -> <IO | e> Unit
```

- `data` の型引数を `<e>` と書くと、row の型引数になる。宣言でも使う所でも、row の型引数はいつも `<…>` で書く。矢印の row (`-> <e> a`) と同じ書き方なので、宣言を知らなくても、どれが row かが見て分かる
- 型引数の種類は宣言の書き方で決まる。使い方からは推論しない。使わない row の型引数も書ける
- 宣言の型引数の `<…>` に書けるのは、row 変数の名前1つだけである。`data T <IO>` や `data T <e | f>` は構文の誤り (E0011) にする
- 型引数の名前は、row か型かによらず1つの宣言の中で重ねられない (今の E1003)。`data T <e> e` は誤りである
- フィールドの矢印の row の末尾 (`<e>`、`<IO | e>`) には、宣言の row の型引数を書ける。型の型引数を書いたら、今と同じく E1002 である
- row の型引数を持つ `data` を使うときは、row の位置に `<…>` の row を書く。書けるのは、row 変数 (`<e>`)、閉じた row (`<IO>`)、空の row (`<>`)、開いた row (`<IO | e>`) である
- 引数の数は、row の引数を含めて宣言と同じでなければならない。省略はできない (足りなければ今と同じく E1015)
- row の位置に型を書いたとき (`Iter e a`、`Iter Int a`) と、型の位置に `<…>` を書いたとき (`List <IO>`) は誤りにする (下の「誤り」)。row の位置に小文字の名前 `e` を書いたときは、`<e>` に直す fix を付ける
- row の型引数は Kind に効かない。row は値を持たないためである
- row の型引数を持つ `data` は、instance を持てない。instance の頭と `deriving` に書いたら E0004 (まだ対応していない) にする。こうした `data` は関数型のフィールドを持つので、`Show` や `Eq` を導出できず、`once` のフィールドがあれば E2010 で instance を持てない。instance の総称の引数に row 変数を足す仕事 (メソッドの参照の row の具体化、instance の解決、導出、名前) は、要る例が出るまで見送る (私の判断)

### `Lin` の宣言

```haskell
data Token : Lin =
  | Token

data Queue a : Lin =
  | Queue (List a)
```

- 型引数の後、`=` の前に `: Lin` を書くと、その型はフィールドによらず `Lin` になる。キューのハンドルや権限のトークンのように、フィールドが `Unr` でも複製されると困る型のためである
- `Lin` と宣言した型を含む `data` も、今の規則 (定数の `Lin` の型がフィールドにあれば `Lin`) で `Lin` になる
- 書ける Kind は `Lin` だけである。`: Unr` は E0004 (まだ対応していない) にする。それ以外の名前は誤りにする (下の「誤り」)
- `extern data` には書けない。extern の型の Kind は処理系の表が決める。書いたら今の `extern data` の後ろの誤りと同じく E0011 にする
- `Lin` と宣言した型は、`File` と同じく instance を持てない (E2010)。`deriving` も同じく E2010 になる。今の E2010 の規則がそのまま効く

### トップレベルの関数の参照

```haskell
worker : Unit -> <IO> Unit
worker () = println "work"

main : Unit -> <IO> Unit
main () =
  let t = Task worker      -- 今は E2001。S13a で通る
  ...
```

- トップレベルの関数 (引数のない等式で定義した関数型の値を含む)、extern の関数、コンストラクタ、クラスのメソッドを値として参照したときは、一番外側の矢印の線形性を、参照ごとの新しい Kind 変数にする。今は `Unr` に固定している
- これらは何も捕まえないので、1回だけ呼べる関数として扱っても健全である
- 新しい変数には制約がないので、`once` の位置に置かなければ、ふつうは解が最小値の `Unr` になる。例外は、参照が宣言自身の矢印の Kind 変数と等しくなる場合である。`get_worker : Unit -> (Unit -> <IO> Unit)` を `get_worker () = worker` と定義すると、今はシグネチャの内側の矢印が `Unr` に畳まれる。S13a の後は宣言の Kind 変数 `Var(i)` のまま残り、instance では `Poly` になる。呼ぶ側の動きは変わらないが、こうした関数の書き出した型と、IR の変数の印 (`unr` から `poly`) は変わる。参照ごとの変数を `Unr` 寄りにする特別な仕組みは作らず、この変化を受け入れる (私の判断)
- 操作とローカル関数は含めない。ローカル関数は値を捕まえうるので、今の規則のままにする
- `Unr` に決まった局所変数 (2回呼んだ引数など) は、今までどおり `once` の位置に置けない。ラムダで包む (`Task (fn () -> f ())`)

### 診断と表示

- 型の表示では、`Lin` に決まった矢印を `once` で書く (`once Int -> <IO> Unit`)。まだ決まっていない矢印と `Unr` の矢印は `->` のままである。診断は Kind を解く前の型を表示するので、`once` が付くのは、`once` の操作の継続と、`once` と書いた矢印である
- 矢印の線形性だけが食い違う E2001 の note (「呼べる回数が違う」) は外す。`once` を表示するので、期待した型と実際の型の表示が食い違って見えるようになるためである (私の判断)
- REPL の `:type` と、`Show` のない値の ` : T` の表示も同じ規則で `once` を書く

### IR のテキスト形式

型を書く所 (instance の名前、型のダンプ) でも、`%1` と `%m` をやめて語で書く。

| 今 | 新しい形 | 意味 |
|---|---|---|
| `Int %1 -> Int` | `once Int -> Int` | `Lin` の矢印 |
| `Int %m -> Int` | `poly Int -> Int` | `Poly` の矢印 (呼び出し側が決める) |
| `Int %m3 -> Int` | `poly3 Int -> Int` | 宣言の Kind 変数 `Var(3)` の矢印 (テストのダンプだけ) |
| `_%m`、`a%1` | `_:poly`、`a:lin` | 一様な位置と操作ごとの型変数の Kind |

- `once` と `poly` は、`once` と同じく、後ろに続く関数型の一番外側の矢印に付く。`id@[once Int -> <IO> Int]` と `id@[Int -> <IO> Int]` は、今と同じく別の名前になる
- 型変数の Kind は関数ではないので `once` を使わない。LIR の変数の印 (`lin`、`poly`) と同じ語にする
- `poly`、`poly3`、`:lin` の綴りは私の判断である。LIR の変数の印 (`x.1: tobj lin`) は今のまま変えない

## 設計

### 構文 (`eml_syntax`)

- 型の文法 (`grammar/types.rs` の `type_in_inner`) で、`ONCE_KW` を見たら、それを食べて続く関数型を読む。CST は、`FN_TYPE` の最初の子に `ONCE_KW` のトークンを置く形にする。`btype` の後に `THIN_ARROW` がなければ「`->` を期待した」誤り (E0011) にする
- 型引数 (`btype` の引数の並び) に、row の形 `<…>` を書けるようにする。今の `effect_row` を引数の位置でも読む。CST の節点の形 (`EFFECT_ROW` をそのまま型引数の子にするか、包む節点を足すか) は実装で決める
- `data_item` (`grammar/items.rs`) の型引数の並びで、`<` 名前 `>` を row の型引数として読む。今の型の中の `<` と `>` と同じく、`OP` のトークンを分けて読む (`at_angle`、`eat_angle`)。CST では、型の型引数の `NAME` と区別できる節点にする
- `data_item` (`grammar/items.rs`) で、型引数の後、`=` の前に `:` があれば Kind の宣言として読む。名前は新しい節点 (たとえば `KIND_ANNOT`) で包む。`DataItem::name()` が最初の `UIDENT` の `NAME` を返すので、包まないとぶつかる
- `extern data` の後ろの誤り (`reject_extern_tail`) に `:` を足す
- 型付きの AST: `FnType` に `once` を返す関数、`DataItem` に Kind の宣言を返す関数と、型引数を種類つきで宣言の順に返す関数、`AppType` に型引数を型と row を区別して返す関数を足す
- 型の位置以外 (型引数でない所) に `<…>` が来たときの扱いは、今の誤りの経路のままでよい

### HIR (`eml_hir`)

- `TypeRefKind::Fn` に線形性の欄 (書いた `once` の有無) を足す。`pretty.rs` は `once` を書く
- `TypeDef` に Kind の宣言の欄を足す。`Lin` 以外の名前は誤り、`Unr` は E0004 にする
- `item_tree.rs` の `DataItem` の型引数に種類 (型か row か) を持たせる。種類は構文で決まるので、item tree を作るときに決まる
- `declare_data` は、row の型引数を `generics.row_vars` に、型の型引数を `generics.type_vars` に置く。宣言の順と2つの並びの対応を持つ (表示と型引数の照合に要る)
- `Resolver::type_params` (`def_map.rs`) は、型引数の数だけでなく、位置ごとの種類を返すようにする。`applied` (`lower/types.rs`) は、位置ごとに種類を照合し、食い違えば E1048 にする
- `TypeRefKind::Con` の引数は、型か row のどちらかを持てるようにする。表し方 (型と row の和の型にするか、2つの並びに分けるか) は実装で決める
- `Vars::Data` で下ろすフィールドの型では、row の末尾に宣言の row の型引数を引けるようにする。宣言にない名前は今と同じく E1002 である
- 一番外側の矢印への `once` は、トップレベルの関数と値、extern の関数、クラスのメソッド、操作のシグネチャで誤りにする (下の「誤り」)

### 型検査 (`eml_types`)

- `shape.rs` の `lower` で、`once` と書いた矢印は、`Arrows` の種類によらず `ArrowLin::Known(Lin)` にする
- `data.rs` の `collect` は、関数型のうち一番外側の矢印が `once` のものを、定数の `Lin` の型として数える (`out.lin = true`)。関数型の中には今と同じく入らない。`data_kinds` の初期値で、`Lin` と宣言した型の `lin` を `true` にする。E2010 とそれに続く処理は `DataKind::lin` だけを見ているので、変えなくてよい
- 型引数の位置を `type_vars` の番号で引いている所 (`data.rs` の `collect` と `collect_variance`) は、種類が混ざっても正しい位置を引くように直す
- row の型引数の変位は「両方」にする。row のラベルの型引数と同じ扱いである (私の判断)
- 型の適用の引数に row を持てるようにする。推論の表の `TyShape::Con`、閉じた形の `ShapeTy::Con`、書き出した型の `TypeKind::Con` が対象である。単一化 (`unify_in`) は、row の引数を `unify_row_in` で単一化する。row の変数は今と同じく多重度の Kind (σ) を持つ。`TypeStore` の `for_each_child`、`linearity_of` (row の引数は見ない)、`lin_compatible`、`substitute` (row の末尾は今と同じく書き換えない)、表示も直す。下流の crate で `TypeKind::Con` を見る所 (`eml_core_ir` の `translate/types.rs`、`names.rs`、`instances.rs`) も合わせる
- コンストラクタの型 (`lower_constructor`) の結果の `Con` には、宣言の row の型引数を rigid な row として並べる
- トップレベルの関数などの参照 (上の「トップレベルの関数の参照」) は、`Shape::instantiate` で作った型の一番外側の `Known(Unr)` を、参照ごとの新しい `ArrowLin::Var` に置き換えて作る。置き換える場所は、参照を検査する `BodyCheck::path` か `instantiate` の周りである。この変数は `Instance.lin` に入れない。宣言のスキームの変数と数が合わなくなるためである。コンストラクタのパターンなど、値としての参照でない所は今のままにする
- 型の表示 (`store.rs` の `mark` と `Show`) を、上の「診断と表示」と「IR のテキスト形式」の形に直す。`Show::Plain` は `Lin` の矢印だけを `once` で書く
- E2001 の note (`check/report.rs` の `arrow_linearity`) を外す。`UnifyError::ArrowLinearity` を区別する必要がなくなるなら、それも消す

### Core IR (`eml_core_ir`)

- `translate/instances.rs` の `check_linearity` は、関数の参照の型と呼ぶ先のシグネチャを `lin_compatible` で比べている。参照の一番外側の矢印は `Lin` になりうるので、比べるときに一番外側の矢印を除く
- 値として使ったトップレベルの関数の変数は、矢印が `Lin` なら印が `lin` になる。`lin` の印の変数は `dup` も `decref` もされないので、今の検査のままでよい
- フィールドがない `Lin` の型 (`data Token : Lin = | Token`) は Repr が `enum` になる。`enum` の変数に `lin` の印が付いても、boxing、Perceus、検査器、VM が今のまま動くことをテストで確かめる。`drop` は `enum` でも何もしない
- instance の名前は、型の表示の変更 (`once`、`poly`、`:poly`) をそのまま使う。64文字を超えたら今と同じく構造の hash にする

### 誤り

新しい診断コードは次のとおりである。番号と定数の名前は実装で決めてよい。文言は今の診断に合わせて英語で書く。

| コード | 段 | 起きる場合 |
|---|---|---|
| E1048 | HIR | 型引数の種類が合わない。row の位置に型を書いた、または型の位置に `<…>` を書いた。宣言の型引数を label で示す。row の位置の小文字の名前には、`<e>` に直す fix を付ける |
| E1049 | HIR | `data T : K` の `K` が Kind の名前でない (`Lin` でも `Unr` でもない) |
| E1050 | HIR | トップレベルの関数と値、extern の関数、クラスのメソッド、操作のシグネチャの一番外側の矢印に `once` を書いた。help で `once` を消すよう促す |

`: Unr` は E0004、`once` の後ろが関数型でないときは E0011 である。

## テスト

`run` のテストは新しいカテゴリ `tests/ui/run/kinds/` に、`check-fail` のテストは `tests/ui/check-fail/kinds/` に置く。

### `run`

- `once` の継続を `data` に入れて後で再開するジェネレータ。`data Iter <e> a` を手で書き、`yield` の生産者から `once` の継続で `Iter` を作って引く。途中でやめる場合 (残りを `drop`) も含める。完了の条件の1つ目である
- row の型引数。上の `Iter` を `<IO>` の生産者、ユーザーのエフェクトの生産者 (外側に handler がある)、純粋な生産者 (`Iter <> Int`) の3通りで使う。`take : Int -> Iter <e> a -> Iter <e> a` のような総称な関数も通す。完了の条件の2つ目である
- 協調的なスケジューラ。`once` の操作 `suspend` の継続を `Task (once Unit -> <e> Unit)` としてキューに入れ、順に再開する。`Task worker` のように、トップレベルの関数をそのまま入れる場合を含める
- トップレベルの関数、extern の関数、コンストラクタの参照を `once` の位置に置く。同じ関数を `Unr` の位置にも置き、両方が動くこと
- `Lin` の宣言。`data Token : Lin = | Token` を1回だけ使う。キューのハンドルと `Unr` のフィールドを混ぜたレコードを分解して組み直す
- `File` を持つ生産者の `Iter` を途中で `drop` し、`File` が閉じること (検査モードで漏れがない)
- 別のモジュールで宣言した row の型引数を持つ `data` を、フィールドとシグネチャで使う場合。ディレクトリの形のテストにする

### `check-fail`

- `once` の閉包を2回呼ぶ (E3002)、捨てる (E3004)
- `Lin` と宣言した型の値を2回使う、`_` で捨てる
- `Lin` と宣言した型の `deriving` と instance (E2010)
- E1048 (fix を含む)、E1049、E1050 をそれぞれ1つ以上
- 宣言の型引数の名前の重なり (`data T <e> e`、E1003) と、宣言の `<…>` に row 変数の名前以外を書いた場合 (E0011)
- `data T : Unr` (E0004)、`once Int` (E0011)、`extern data` の後ろの `: Lin` (E0011)
- row の引数が足りない (E1015)
- 矢印が `Unr` に固定された関数の値 (`->` のフィールドから取り出した関数) を `once` のフィールドに入れる (E2001。`once` を表示し、note がない)
- 2回呼んだ引数を `once` のフィールドに入れる。矢印の Kind 変数が `Lin` と `Unr` の両方に縛られ、今と同じ使用回数の誤り (E3002) になることを確かめる。ラムダで包む help は付けない (私の判断)
- row の型引数を持つ `data` の instance と `deriving` (E0004)

### REPL

- `:type` でコンストラクタ `More` の型 (`a -> (once Unit -> <e> Iter <e> a) -> Iter <e> a`) と、`once` の継続を返す式の型を表示するトランスクリプトを1つ足す

### 単体テスト

- `eml_syntax`: `once` の型 (入れ子、括弧、`once` の後ろが関数型でない場合)、`data T a : Lin`、型引数の row の形
- `eml_hir`: 種類つきの型引数の下ろし方、使う所での種類の照合 (E1048 とその fix)、Kind の宣言の下ろし方
- `eml_types`: `once` のフィールドと `Lin` の宣言の `DataKind`、参照の一番外側の矢印が `once` の位置で `Lin` になり、そうでなければ `Unr` に解けること、row の型引数の単一化と表示
- `eml_core_ir`: `enum` の `lin` の変数を通す LIR、`once` の位置に置いたトップレベルの関数の instance と印

## テストの変更

- 期待値の変更 (種類2): `%1`、`%m`、`%m{i}`、`_%m`、`a%1` を含む期待値を、上の「IR のテキスト形式」の新しい形に書き換える。`crates/eml_core_ir/tests/instances.rs`、`crates/eml_types/tests/kind_export.rs` と、instance の名前を含む Core IR / LIR のスナップショットが対象である。表記を変えたためである
- 期待値の変更 (種類2): `Lin` の矢印 (`once` の操作の継続など) の型を表示する診断のスナップショットを、`once` を書く形に書き換える。矢印の線形性だけが食い違う E2001 からは note を消す。`tests/ui/check-fail/types/linear_function_in_unr_field.em` のスナップショットと、`crates/eml_types/tests/effects.rs` の `an_arrow_linearity_note_*` が対象になる。後者は、note があることを確かめる代わりに、`once` が表示されて note がないことを確かめる形にする。`once` を表示できるようになり、note が要らなくなったためである
- 合否の変更 (種類1): なし。今 E2001 になるトップレベルの関数を `once` の位置に置くテストは、今は存在しない (ないことを実装の初めに確かめる)
- 期待値の変更 (種類2): トップレベルの関数などの参照が、宣言自身の矢印の Kind 変数と等しくなる関数 (上の「トップレベルの関数の参照」の例外) の、Kind のダンプ、instance の名前、Core IR / LIR のスナップショット。参照の一番外側の矢印を参照ごとの変数にしたためである。あるかどうかは実装で確かめる
- 機械的な追従 (種類3): 型引数や `TypeKind::Con` の表し方を変えたことによる、期待値を変えないテストの書き換え

## 段の終わりの文書

- `docs/spec/types.md`: 「Kind」(`once` の矢印と `Lin` の宣言が `data` の Kind に効くこと、row の型引数)、「関数型」(表面の構文で `once` を書けること、参照の一番外側の矢印)、「Kind の書き出し」の表記
- `docs/spec/grammar.md` と `docs/spec/declarations.md`: `once` の型、型引数の row、`data` の Kind の宣言
- `docs/spec/linearity.md`: `once` の矢印と `Lin` の宣言
- `docs/spec/core-ir.md`: instance の名前の表記
- `docs/implementation/diagnostics.md`: 新しいコードと、E2001 の note を外したこと
- `docs/implementation/status.md`: 今の言語の範囲
- `docs/future/roadmap.md`: S13a の節を消し、表の S13a の行を済みにする
- `CLAUDE.md`: instance の名前の例 (`id@[Int %1 -> Int]`) と、表記の説明

## 私の判断で決めたこと

- 矢印の線形性だけが食い違う E2001 の note を外す
- IR の `Poly` の矢印を `poly`、宣言の Kind 変数を `poly{i}`、型変数の Kind を `:lin` / `:poly` と書く
- row の型引数の変位を「両方」にする
- 新しい診断コードを HIR の3つ (E1048〜E1050) にする。`once` を一番外側に書く誤りも、シグネチャの形が分かる HIR で出す
- トップレベルの関数の参照の扱いを、クラスのメソッドと、引数のない等式で定義した関数型の値にも広げる。どれも何も捕まえないためである。操作は含めない
- 新しいテストのカテゴリを `kinds` にする
- row の型引数を持つ `data` の instance を E0004 にする
- 参照ごとの変数が宣言の Kind 変数と等しくなって、書き出した型と印が変わる場合を受け入れる
- 2回呼んだ引数を `once` の位置に置いたときの誤りは、今の E3002 のままにする
