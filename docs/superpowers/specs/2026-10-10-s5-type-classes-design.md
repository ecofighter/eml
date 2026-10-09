# S5 型クラス

位置づけ: 作業の設計。ロードマップの S5 (型クラス) を定める。S5 を終えたら、決まったことを `docs/spec/` と `docs/implementation/` に移し、この文書を削除する。

## 目的

Haskell 98 の形の単一引数の型クラスを入れ、等価、比較、表示を Prelude のクラス `Eq`、`Ord`、`Show` で作る。今の `==` と `!=` の特別扱い (`eml_types::equality`、extern の行の `by_type`、E2006 の「比べられない型」) を、この仕組みに置き換える。証拠 (どの instance を使うか) は、型検査が記録した参照ごとの型引数から、単相化の instance の表で確定する。実行時の辞書は持たない。

完了の条件は、型クラスの UI テスト (下の「テストの変更」の新しいテスト) が通ることである。

次のものは変えない。

- データの配置と row。row の多相は S7 で扱う
- 算術の演算子の型 (`Int -> Int -> Int`)。`Num` は `Float`、`Char`、`Num` の段に回す
- 演算子の参照とセクションをラムダに脱糖する HIR の処理 (`lower/section.rs`)。`==` に限った処理ではないので、そのまま残す (ファイルの先頭の、`==` の比べ方に触れた注釈だけを直す)
- ベンチマークの `RunStats`。`Int` の `==`、`<` などと `show` は、今と同じく extern 命令を直接出す (下の「translate」)

## 前提として確かめたこと

- `class` と `instance` は字句のキーワードで、パーサは項目の位置でこれらを読むと E0011 (将来のための予約) を出して項目の終わりまで読み飛ばす (`grammar/items.rs` の `reserved_item`)。`forall` も同じ扱いで、S5 の後も予約のまま残る。`deriving` は字句にない
- `=>` は普通の演算子として字句解析される。`=>` を定義するテストはない。字句のテスト (`eml_syntax/tests/lexer.rs` の `operators_and_reserved_symbols`) が `=>` を演算子として読む
- `==` と `!=` は Prelude の `pub extern (==) : a -> a -> Bool` で、extern の行が `by_type: true` を持つ。translate は参照の型引数から `eml_types::equality` で比べ方を決め、`Prelude.int_eq` などの行に置き換える (`translate/expr.rs`、`translate/types.rs` の `equality_extern`)。型検査は `check/equality.rs` で比べられない型を E2006 にし、E2006 を本体の誤りに数えて線形性の診断の連鎖を抑える
- 演算子の参照 (`(==)`) とセクション (`(== "a")`) は、どの演算子でも HIR がラムダに脱糖する
- 参照ごとの具体化の表 (`BodyTypes::instantiations`) は、本体の中の関数、操作、コンストラクタへの参照ごとに、宣言と型引数を持つ。型引数はシグネチャの型変数の順に並ぶ。translate は、型変数を持たない関数を含むすべての instance で、参照の型引数に代入をかけてから鍵を作る (`OpVar` は `Flexible` になる)
- 本体のない宣言 (extern の関数、操作) の Kind のスキームは、宣言だけから作る (`check/mod.rs` の `declaration_schemes`)。extern の関数のスキームは、部分適用のクロージャの Kind だけを持つ。操作の引数に書いた関数型と、`data` のフィールドに書いた関数型の線形性は `Unr` に固定している (`docs/spec/types.md` の「Kind」)
- `data` の Kind で、関数型の中の型引数は効かない。そのため `data G b = | G (Unit -> b)` は、`b` によらず `Unr` である
- Kind のスキーム (`KindScheme`) は、線形性と多重度の境界の組と、持ち越しの制約の列である
- 型の表の `Rigid` は名前で登録する。代入 (`Substitution`) も名前で引く
- 単相化の一様な位置 (`translate/instances.rs` の `uniform_positions`) は translate の中にあり、定義された関数の参照の型引数から型変数の流れのグラフを作る。`eml_core_ir/tests/instances.rs` のテストは、どれも instance の名前を通して一様な位置を確かめる。グラフだけを確かめるテストはない。`Instance.args` は「S5 が証拠を決めるときに読む」として未使用のまま置いてある
- `Unit` は型の表で空のレコード、タプルは数字ラベルのレコードである (`TypeKind::Record`)。Prelude は `pub extern data Unit` と書くが、型の表では `Con` にならない。Core IR では、タプルはタグ 0 のコンストラクタ1つの配置 (`(,)`、`(,,)`) で、`Unit` は即値の `()` である
- Core IR のテキストの形は、空白などを含む関数の名前を引用符で囲む (`pretty` と `parse`)。extern の名前は `pretty` が行の名前をそのまま書き、`parse` が語として読む (`pretty.rs` と `text.rs`)。`parse` を語の代わりに文字列も読む形 (`name()`) に変えれば、extern の名前も引用符で囲める
- 持ち越しの制約の表示は `eml_types/src/ty.rs` の `a => <e> <= Once` で、型検査のダンプだけに出る
- `show_int` は Prelude の `pub extern` で、UI テストの約100本、`bench/` のプログラム、各 crate の単体テスト、`docs/` の例が使う。`tests/ui/run/modules/qualified/Report/Csv.em` は自分のモジュールで `pub show` を定義し、その本体で `show_int` を呼ぶ。モジュールのトップレベルの名前は Prelude の名前を隠す (`docs/spec/modules.md`)
- `tests/ui/run/data/shared_scrutinee.em` は入口のモジュールで `show` を定義する。この名前は Prelude の `show` を隠すので、S5 の後もそのまま動く
- 文字列のエスケープは `\n` `\t` `\r` `\\` `\"` `\0` `\u{XXXX}` で、補間の `\{` は S6 で入れる (`docs/spec/lexical.md`)
- `crates/eml_cli/tests/citations.rs` は、文書の中の「…の「見出し」」の形の参照を確かめる。`docs/future/stdlib.md` はロードマップの「S5 型クラス」を参照している

## 構文

### 字句

- `deriving` をキーワードにする
- `=>` を予約の記号にする。ユーザーは `=>` を演算子として定義できなくなる
- `class` と `instance` は、予約の E0011 をやめて構文にする。`forall` は予約のまま残す

### 文法

```
item          ::= … | class_item | instance_item
class_item    ::= 'class' context? UIDENT LIDENT ('where' block(class_member))?
class_member  ::= signature | equation
instance_item ::= 'instance' context? qUIDENT atype ('where' block(inst_member))?
inst_member   ::= equation | 'extern' var
context       ::= btype '=>'
signature     ::= var ':' context? type
data_item     ::= 'data' UIDENT LIDENT* ('=' alts)? deriving?
deriving      ::= 'deriving' (qUIDENT | '(' qUIDENT (',' qUIDENT)* ')')
```

- 文脈 `context` は、型として読んだもの (`Eq a`、`(Eq a, Show b)`) を `=>` の後で文脈と決める。パーサは、`signature` の `:` の後と、`class` と `instance` のキーワードの後で、括弧の入れ子の外の `=>` が項目の終わりまでにあるかを先読みし (中置のコンストラクタの `has_conop_ahead` と同じ形)、あれば文脈として読む。CST は `CONTEXT` の節点の下に、制約ごとの `CONSTRAINT` (クラスの名前と型) を置く。制約の形 (クラスの名前と1つの型変数) は HIR で検査する
- `pub` は `class` に付けられる。`pub instance` と、`class` と `instance` のブロックの中の `pub` は E0011 にする。instance は名前を持たず、いつもプログラム全体で見えるためである
- `class` の `where` のブロックには、メソッドのシグネチャと既定のメソッドの等式を並べる。既定のメソッドの等式は、そのメソッドのシグネチャの直後に置く。規則と診断は、トップレベルの関数のシグネチャと等式 (E1004、E1018、E1019、E1020) と同じである。ただし、等式のないメソッドのシグネチャは E1005 にしない (既定のないメソッドである)。同じメソッドのシグネチャが2つあれば E1003 にする
- `instance` の `where` のブロックには、メソッドの等式と `extern` の行だけを書ける。シグネチャは書かない (Haskell 98 と同じ)。シグネチャを書いたら E0011 にする。等式には E1018 と E1020 をメソッドごとに当てはめる。同じメソッドを `extern` の行と等式の両方で、または `extern` の行2つで定義したら、2つ目を E1003 にする
- `where` のない `class` と `instance` も書ける (メソッドのないクラス、すべて既定のメソッドで済む instance)
- instance の頭は `atype` として読み、形 (型コンストラクタに互いに異なる型変数を適用したもの) は HIR で検査する (E1039)。そのため `instance Eq a`、`instance Eq (Int, Int)`、`instance Eq (Option Int)` は構文の誤りでなく E1039 になる
- `deriving` は、`data` の最後の選択肢の後に置く。最後の選択肢と同じ行、選択肢のブロックの項目 (`|` と同じ列)、最後の選択肢の続きの行 (`|` より深い字下げ)、ブロックを閉じた後の続きの行 (`data` より深く `|` より浅い字下げ) のどれに書いてもよい。パーサは、最後の選択肢の型を読む途中でも、ブロックの項目の始まりでも、ブロックを閉じた後でも `deriving` を受け付ける。`deriving` の後に選択肢が続いたら E0011 にする

  ```haskell
  data Color = | Red | Green deriving (Eq, Show)

  data Option a =
    | None
    | Some a
    deriving (Eq, Ord, Show)
  ```

- 文脈は、どの `signature` でも構文として読む (extern のシグネチャも同じ関数で読む)。書けるのは、トップレベルのシグネチャ、クラスのメソッドのシグネチャ、`class` と `instance` の頭である。extern のシグネチャと操作のシグネチャの文脈は HIR で E1040 にする。操作に制約を書けると、handler の節が perform の位置で選んだ証拠を実行時に受け取る必要が生じ、実行時の辞書が要るためである。本体の型の注釈 `(e : T)`、ラムダの引数の型、`data` のフィールドの型には文脈の構文がなく、`=>` は E0011 になる

### 持ち越しの制約の表示

型検査のダンプの持ち越しの制約の表示を、`docs/spec/types.md` がすでに使う記法 `carry(l, s)` に合わせる。`a => <e> <= Once` は `carry(a, <e>)`、値が定数の `Lin` なら `carry(Lin, <e>)`、row が定数なら `carry(a, Multi)` と表示する。意味 (「`l` が `Lin` なら `s ≤ Once`」) は変わらない。

## 名前解決と HIR

### 名前空間と公開

- クラスは型の名前空間に置く。型、エフェクト、ほかのクラスと名前が重なれば E1003 にする
- メソッドは値の名前空間に置くトップレベルの値で、エフェクトの操作と同じ扱いにする。関数、操作、コンストラクタ、ほかのメソッドと名前が重なれば E1003 にする。モジュールのトップレベルの名前が Prelude の名前を隠す規則も、操作と同じく当てはまる
- `pub class` は、クラスとそのメソッドをまとめて公開する。メソッドごとの `pub` はない。Prelude の `pub class` のメソッドは、ほかの Prelude の名前と同じく修飾なしで見える
- import の並びの `Eq(..)` は、クラスとそのメソッドを取り込む。`Eq` だけならクラスだけを取り込み、メソッドは修飾して使う。修飾した演算子の構文はまだないので、演算子のメソッドを修飾なしで使うには `C(..)` で取り込む。操作の `E(..)` と同じ規則である
- 制約、`instance` の頭のクラス、`deriving` のクラスは、型の名前空間で引く。引いた名前が型かエフェクトなら E1041 にする。型を書く位置に書いた名前がクラスなら E1043 にする
- 公開の範囲の検査 (E1032) は、`pub` の関数のシグネチャの制約のクラスにも当てはめる。`pub class` の上位クラスとメソッドのシグネチャにも当てはめる
- fixity の宣言はメソッドにも付けられる。E1022 の「このモジュールで定義した演算子」は、このモジュールのクラスのメソッドを含む。Prelude の `pub infix 4 ==, !=, <, <=, >, >=` は、`Eq` と `Ord` のメソッドに付く

### 宣言の検査

- 上位クラスの文脈の型変数は、クラスの型変数でなければならない (E1040)。上位クラスの関係が循環したら E1042 にする
- メソッドのシグネチャは、クラスの型変数を含まなければならない (E1040)
- シグネチャの制約の型変数は、そのシグネチャの型に現れなければならない (E1040)。現れない型変数への制約は、どの参照でも型が決まらないためである。メソッドのシグネチャの制約は、メソッド自身の型変数だけに書ける (クラスの型変数への制約は E1040)
- instance の頭は、`data` の型か extern の型のコンストラクタに、互いに異なる型変数を宣言の数だけ適用した形でなければならない (E1039)。タプル、`Unit`、関数型、型変数、型の別名は頭にできない。タプルと `Unit` は型の表でレコードなので、処理系の構造的な instance (`Eq`、`Ord`、`Show`) だけを持ち、ユーザーのクラスの instance は持てない。型引数の数の誤りは E1015 にする
- instance の文脈の型変数は、頭の型変数でなければならない (E1040)
- orphan 規則: instance は、クラスか頭の型を定義したモジュールにだけ書ける (E1034)。`deriving` は型を定義したモジュールにあるので、いつも満たす
- 同じ (クラス, 型) の instance が2つあれば、プログラムのモジュールの順 (Prelude、入口、標準ライブラリ、import したモジュール) とモジュールの中の位置で後にあるものを E1035 にする。手で書いた instance と `deriving` が重なった場合と、同じ `deriving` にクラスを2回書いた場合も含む
- instance の等式の名前がクラスのメソッドでなければ E1037 にする
- 既定のないメソッドを instance が定義していなければ、instance の頭を指して E1036 にする。`extern` で結んだメソッドは定義したものに数える
- `deriving` に、Prelude の `Eq`、`Ord`、`Show` 以外のクラスを書いたら E1038 にする。名前でなく、解決したクラスで判定する (ユーザーが `Eq` という名前のクラスを定義しても導出できない)
- instance の中の `extern` は、標準ライブラリのモジュールにだけ書ける (E1033 の範囲を広げる)。行の名前は `<モジュールの正式な名前>.<クラス> <型>.<メソッド>` (`Prelude.Eq Int.==`) である。クラスと型は、そのモジュールで書いた名前である

### HIR の形

- `ItemTree` に、クラス (名前、型変数、文脈、メソッドのシグネチャと既定の等式) と instance (クラスの名前、頭、文脈、メソッドの等式と `extern` の行) と、`data` の `deriving` の並びを足す
- `Items` に `classes` と `instances` の arena を足す。`TypeItem::Class` と `ValueItem::Method` を足す
- クラスは、型変数、上位クラスの列、メソッドの列を持つ。メソッドは、シグネチャと、既定の本体の関数 (あれば) を持つ
- instance は、クラス、頭の型、型変数、文脈、メソッドごとの定義を持つ。定義は、本体の関数か extern の行 (`Extern`) である。導出した instance は、本体を持たず、「導出した」という印と `deriving` のクラス名の位置を持つ
- 既定のメソッドの本体と instance のメソッドの本体は、今の関数と同じく、等式を `match` に脱糖した `Function` として持つ。`FunctionKind` に、既定のメソッド (`DefaultMethod`) と instance のメソッド (`InstanceMethod`) を足す。これらの関数は def_map の値の名前空間に登録せず、E1003 の対象にもしない。名前で引けるのは `ValueItem::Method` だけである
- これらの関数の型変数は、既定のメソッドならクラスの型変数とメソッド自身の型変数の順、instance のメソッドなら頭の型変数とメソッド自身の型変数の順に並ぶ。メソッド自身の型変数の名前が頭の型変数の名前と重なるときは、メソッド自身の型変数の名前の後に番号を付けて重ならない名前にする (型の表の `Rigid` と代入が名前で引くため)
- 本体の型の注釈で書ける型変数の名前は、instance のメソッドなら instance の頭の型変数だけ、既定のメソッドならクラスの型変数とそのメソッドのシグネチャの型変数である。instance のメソッドのメソッド自身の型変数は、クラスのシグネチャ (別のモジュールにあるかもしれない) が決めるので、名前で書けない
- 中置のコンストラクタの fixity を、導出した `Show` のために HIR に残す

## 型検査

### クラスの性質

S5 のクラスは、すべて `Unr` のクラスである。将来 `Lin` の型を受けるクラスを入れるときは、クラスの性質として区別し、S5 のクラスの意味は変えない (下の「ロードマップへの変更」)。`Unr` のクラスは次の規則を持つ。

- 制約 `C a` は `a ≤ Unr` を意味する。シグネチャの制約は、そのシグネチャの Kind のスキームに `a ≤ Unr` を足す
- instance の本体は、頭の型変数をすべて `Unr` とみなして検査する。本体は、頭の型変数の型の値を自由に写し、捨ててよい
- そのため、手で書いた instance か導出した instance で `C (H T1 … Tn)` を解くたびに、参照のある本体の Kind の制約に `Ti ≤ Unr` をすべて足す。`data G b = | G (Unit -> b)` は `b` によらず `Unr` なので、この規則がないと、文脈のない `instance Show (G b)` の本体が `b` の値を作って捨てられ、`G File` で線形性が破れる
- instance の頭は、頭の型変数をすべて `Unr` としたときに `Unr` でなければならない (E2010)。定数の `Lin` のフィールドを持つ `data` と、extern の `Lin` の型 (`Fs.File`) は、instance を持てない
- クラスのメソッドのシグネチャに書いた関数型 (一番外側の矢印を除く) の線形性は `Unr` に固定する。操作の引数とフィールドの関数型と同じ規則である。メソッドのシグネチャは本体を持たず、矢印の線形性を推論できないためである

### スキームと制約

- シグネチャのスキームに、制約の列 (クラス, 型変数の番号) を足す
- メソッドの型のスキームは、クラスの型変数を先頭に、メソッド自身の型変数を後ろに並べ、制約の先頭に `C a` (a はクラスの型変数) を、その後ろにメソッド自身の制約を持つ。メソッドへの参照の型引数も、この順に記録する
- 参照ごとの具体化の表は今のまま使う。証拠そのものは記録しない。translate と下の一様な位置の計算が、型引数から instance を引き直す

### 制約を解く

- 制約を持つ関数やメソッドを参照するたびに、具体化した制約 `C T` を求める制約として、参照の位置とともに集める
- 本体の単一化が終わった後で、求める制約をまとめて解く。今の `check_comparisons` と同じ位置 (線形性の検査より前) で解く
- 与えられた制約は、本体ごとに次のものを上位クラスでたどって閉じたものである
  - トップレベルの関数: シグネチャの制約
  - instance のメソッド: instance の文脈と、メソッド自身の制約
  - 既定のメソッド: クラスの制約 `C a` と、メソッド自身の制約
- `C T` の `T` の形で次のように場合を分ける
  - 型コンストラクタの適用 `H T1 … Tn`: `(C, H)` の instance を探す。あれば、instance の文脈を `Ti` に写して解き、上の `Ti ≤ Unr` を足す。導出した instance も同じである
  - タプル (`Unit` を含む): `C` が `Eq`、`Ord`、`Show` なら、要素ごとに `C Ti` を解く。ほかのクラスにはタプルの instance はない
  - シグネチャの型変数 (`Rigid`): 与えられた制約にあれば解ける
  - 関数型、操作ごとの型変数 (`OpVar`)、与えられた制約にない型変数: E2006
  - 最後まで決まらない推論用の変数: E2009。誤りは参照の位置に出す
  - `Error` を含む型: 何も出さない (連鎖の抑止)
- E2006 と E2009 は、今の E2006 と同じく本体の誤りに数え、線形性の診断を連鎖させない。推論用の変数のまま残った型について E2009 を出すのは、本体にほかの誤りがないときだけである (今の E2006 の扱いを引き継ぐ)
- 1つの参照から出た制約の誤りは1つにまとめる

### instance と既定のメソッドの検査

- instance のメソッドの本体は、クラスのメソッドのシグネチャのクラスの型変数を頭の型に置き換えた型で検査する (メソッド自身の型変数の名前は、上の「HIR の形」のとおり重ならないようにしてある)
- 上位クラスの instance があることを確かめる。`instance Ord a => Ord (Option a)` なら、`{Ord a, Eq a}` のもとで `Eq (Option a)` が解けなければならない。解けなければ instance の頭を指して E2006 にする
- 既定のメソッドは、クラスの中で1回だけ検査する
- instance のメソッドと既定のメソッドは、Kind の推論の SCC に普通の関数として入る
- メソッドの Kind の規則 (E2011) は次のとおりである
  - クラスのメソッドの Kind のスキームは、同じシグネチャの extern の関数と同じく、宣言だけから作る (部分適用のクロージャの Kind)
  - instance のメソッドの Kind のスキームは、次の前提のもとで、クラスのメソッドのスキームから導けなければならない。前提は、クラスの型変数を頭の型に置き換えたことと、頭の型変数がすべて `Unr` であることである
  - 既定のメソッドの Kind のスキームは、クラスの型変数が `Unr` であることを前提に、クラスのメソッドのスキームから導けなければならない
  - 導けない制約 (線形性と多重度の境界、持ち越しの制約) があれば E2011 にする。残るのは、メソッド自身の型変数と、シグネチャの row の多重度に関わる制約である。たとえば `class Pairable a where pair_with : a -> b -> (b, b)` の instance が `b` を2回使うと `b ≤ Unr` が要るので E2011 になる。`b` の値を持ったまま row の `e` の呼び出しをまたぐと `carry(b, e)` が要るので、これも E2011 になる
  - Prelude のクラスのメソッドはメソッド自身の型変数を持たないので、この規則で誤りになる Prelude の instance はない

### 導出した instance

- 導出した instance の文脈は、フィールドの型に現れる `data` の型引数のそれぞれに、そのクラスを付けたものにする。Haskell のような文脈の推論 (フィールドの制約を簡約して最小の文脈を求めること) はしない
- 各フィールドの型 `F` について、その文脈のもとで `C F` が解けることを確かめる。解けなければ `deriving` のクラス名の位置に E2006 を出し、どのフィールドかを note で示す
- `Ord` の導出では、上位クラスの規則で `Eq` の instance が要る (手で書いたものでも導出したものでもよい)
- 頭が `Unr` でなければ E2010 にする

### 一様な位置と制約付きの多相再帰

`translate/instances.rs` の `uniform_positions` を `eml_types` に移し、型検査の後に計算して `TypedProgram` に入れる。translate はこの結果を読む。グラフは S4b の規則を次のように広げる。

**節点**

- (関数, 型変数の番号): 関数には instance のメソッドと既定のメソッドを含む
- (instance, 頭の型変数の番号): 手で書いた instance、導出した instance、プログラムに現れる大きさのタプルの instance。以下、instance の節点と呼ぶ
- (メソッド, メソッド自身の型変数の番号): 以下、メソッドの節点と呼ぶ

**辺**

本体の関数 f の中の参照ごとに辺を引く。「f の型変数 i が型 T に現れるとき、(f, i) から X へ辺を引く」の形で書き、T が i の型変数そのものでなければ大きくなる辺とする。「現れる」は S4b と同じである。

1. 関数の参照 `g @[T0, …]`: S4b と同じく、Tj について (g, j) へ辺を引く
2. メソッドの参照 `m @[T, U1, …, Uk]` の `T` の頭で instance I が決まるとき (型コンストラクタの適用かタプル): 解決先の関数へ直接辺を引く
   - 解決先が I のメソッドの関数 F (頭の型変数が n 個) なら、`T` の型引数 Tj について (F, j) へ、Ul について (F, n + l) へ
   - 解決先が既定のメソッド D なら、`T` について (D, 0) へ、Ul について (D, l) へ
   - 解決先が生成する関数 (導出とタプル) なら、`T` の型引数 Tj について (I, j) へ
3. メソッドの参照の `T` が与えられた制約で解けるとき: `T` については辺を引かない。Ul について (m, l) へ辺を引く
4. 制約の解決: 本体の中の参照が求める制約 (関数の参照の制約と、メソッドの参照のメソッド自身の制約) を、上の「制約を解く」と同じ規則で解く。メソッドの参照の `C T` は、解決の木の根を 2. が扱い、根の instance の文脈から下の部分木だけをここで扱う。解決の木の、instance で解いた各節点 `D (H T1 … Tn)` について、Tj について (D の instance, j) へ辺を引く。各節点で、`D` の上位クラスの (H の) instance にも同じ辺を引き、その instance の文脈の解決の木にも同じ規則を当てはめる。与えられた制約で解けた部分には辺を引かない (その証拠は、呼び出し側の解決の辺が表す)
5. instance の節点から: (I, j) から I のメソッドの関数の (F, j) へ、大きくならない辺を引く。逆向きの辺は引かない。メソッドの関数は頭の型を作り出さないので、逆向きの流れはないためである。I がメソッド m を省いて既定のメソッド D を使うときは、(I, j) から (D, 0) へ大きくなる辺を引く (既定のメソッドのクラスの型変数には頭の型そのものが入るため)。導出した instance とタプルの instance は、生成するコードが求める制約 (フィールドの型 `F` の `C F`) を、本体を (I, ·) とみなして 4. の規則で扱い、使う既定のメソッドへの辺を同じく引く
6. メソッドの節点から: (m, l) から、m を定義するすべての instance のメソッドの関数の (F, n_F + l) と、m の既定のメソッドの (D, l) へ、大きくならない辺を引く

関数の参照の制約を解いた先 (4. と 5.) は、instance のすべてのメソッドへ辺を引くので、保守的である。メソッドの参照は 2. で解決先にだけ辺を引くので、同じ instance の別のメソッドの間で偽の循環を作らない。

**一様な位置と E2012**

- 大きくなる成分の節点を一様な位置とする。translate は、鍵を作るときに解決先の関数の一様な位置の型引数を `Flexible` にする。生成する関数の鍵の位置は、instance の節点の一様な位置に従う
- 一様な位置の型変数が与えられた制約を持てば、E2012 にする。与えられた制約を持つ位置は、関数のシグネチャで制約を持つ型変数、instance のメソッドの関数で instance の文脈かメソッド自身の制約を持つ型変数、既定のメソッドのクラスの型変数とメソッド自身の制約を持つ型変数、instance の節点で文脈を持つ型変数である
- 誤りは、関数ならシグネチャの名前を、instance のメソッドと instance の節点なら instance の頭を指す。既定のメソッドの位置なら、その位置を含む大きくなる成分へ 5. の辺 (既定のメソッドへの辺) を引いた instance の頭を指す (既定のメソッドは Prelude にあることが多く、そこを指しても直す場所が分からないため)
- 入れ子のデータ型に `deriving Show` を付けた形 (`data Nested a = | Flat a | Nest (Nested (a, a)) deriving Show`) は、5. の辺の自己ループが大きくなる辺なので、E2012 で弾かれる
- 解決に `Flexible` が要る位置は、ここまでの規則ですべて型検査の誤りになる。操作ごとの型変数は与えられた制約を持てないので E2006 に、決まらない推論用の変数は E2009 に、一様な位置の制約は E2012 になる。そのため translate は、制約付きの参照で `Flexible` に出会わない
- translate の入口に、制約を持つ関数は選べない。入口の選択は translate の呼び出し側が行い、制約を持つ関数を渡すのは呼び出し側の誤りとして `assert` で止める。`main` は制約を持たない

## translate

### メソッドへの参照の解決

instance の代入をかけた後の参照 `m@[T, 自分の型引数…]` について、`T` の頭で instance を引く。一貫性があるので、instance は1つに決まる。

| instance の種類 | 変換先 |
|---|---|
| 手で書いた instance で、メソッドを定義している | instance のメソッドの関数の instance。鍵は、頭の型引数とメソッド自身の型引数を並べた列 |
| 手で書いた instance で、メソッドを省いた | 既定のメソッドの関数の instance。鍵は `[T, 自分の型引数…]` |
| `extern` で結んだメソッド | `extern` 命令。値として使えば、今の extern と同じく `$externN` で包む。引数の数と Repr は行から、型はクラスのメソッドのシグネチャのクラスの型変数を頭の型に置き換えたものから取る |
| 導出した instance、タプルの instance | 下の生成器が作る関数 |

- 鍵の一様な位置は、型検査の結果を読んで `Flexible` にする
- 解決で `Flexible` に出会ったら、内部の誤りとして止める
- `Instance.args` の「S5 が読む」の注記を外す

### 名前

- instance のメソッドの関数の名前は、`<instance のモジュールの修飾><クラス> <型>.<メソッド>` に、S4b の鍵の表示 (`@[…]`) を付けた形である (`"Prelude.Eq Option.==@[Int]"`、入口のモジュールなら `"Show Color.show"`)。クラスと型は、診断と同じ表示 (名前が重なるときだけモジュールで修飾) で書く。そのため、別のモジュールを足して名前が重なると、名前が変わることがある。空白を含むので、Core IR のテキストでは引用符で囲む
- 既定のメソッドの関数の名前は、メソッドの正式な名前に鍵を付けた形である (`"Prelude.!=@[Color]"`)
- 生成した関数の名前は、手で書いた instance のメソッドと同じ形にする。型の名前は、タプルなら配置の名前 (`(,)`)、`Unit` なら `()` である (`"Prelude.Eq (,).==@[Int, String]"`、`"Prelude.Show ().show"`)。生成した関数の修飾は、導出した instance なら型を定義したモジュール、タプルと `Unit` なら `Prelude.` である
- 補助の関数は、`con$` と同じ修飾の規則で `tag$<型>` とする (`tag$Option`、`tag$Report.Csv.Row`)
- 生成した関数と `tag$` は `internal` にしない。多くの関数から呼ばれるためである
- extern の行の名前 (`Prelude.Eq Int.==`) も空白を含むので、Core IR のテキストの `extern` の位置でも、関数の名前と同じ規則で引用符で囲む。`parse` は `extern` の後でも語の代わりに文字列を読む

### 導出とタプルの生成器

`translate/derive.rs` に置く。(クラス, 型, 型引数) から、Core IR の関数を `FnBuilder` で組む。フィールドの値は、`switch` の case の束縛か `unpack` で得る (R9: `unpack` はコンストラクタが1つの配置にだけ使える)。

- 生成するのは中心のメソッドだけである。`Eq` は `==`、`Ord` は `compare`、`Show` は `show_prec` と `show` (`show x = show_prec 0 x`)。`!=`、`<`、`<=`、`>`、`>=` はクラスの既定のメソッドを通る
- `==`: 左の値で `switch` し、コンストラクタ Ci の case で右の値を `switch` する。右の `switch` は Ci の case と `default` を持つ。Ci の case では、両方のフィールドの `==` を左から短絡して呼ぶ。`default` は `False` を返す。コンストラクタが1つの型 (タプルを含む) は `switch` せずに両方を `unpack` する。フィールドのないコンストラクタだけの型も同じ形で、case にフィールドがないだけである
- `compare`: 左の値で `switch` し、Ci の case で右の値を `switch` する。右の `switch` の Ci の case では、フィールドを左から `compare` し、最初に `EQ` でない結果を返す (すべて `EQ` なら `EQ`)。`default` では、補助の関数 `tag$<型>` で両方のタグの番号を `Int` で求め、`Int` の `compare` の結果を返す。`tag$<型>` は型ごとに1つ作り、`switch` 1つと定数を返す case からなる。コンストラクタの組ごとに case を作ると大きさが `n²` になるので、この形にする。コンストラクタが1つの型は `unpack` してフィールドを比べるだけである
- `show_prec d x`: `x` で `switch` する。フィールドのないコンストラクタは、その名前の文字列を返す。フィールドのあるコンストラクタは `"Some " ++ show_prec 11 f1 ++ " " ++ show_prec 11 f2 …` を作り、`d > 10` なら括弧で囲む。中置のコンストラクタは、fixity の優先度 p について、両辺を `show_prec (p + 1)` で表示して演算子の前後に空白を置き、`d > p` なら括弧で囲む (Haskell 98 の導出と同じ)。コンストラクタの名前は修飾しない
- タプルは、コンストラクタが1つの配置として同じ規則を通る。`Show` は `(a, b)` の形で、要素を `show_prec 0` で表示して `, ` で区切る。`Unit` の `==` は `True`、`compare` は `EQ`、`show` は `"()"` である
- 型がすべて `Unr` なので、写しと解放は Perceus に任せる。生成した関数は translate の出力として verifier を通る
- 生成した関数は、HIR の関数の instance の後に、見つけた順に並べる

### 消すもの

- `eml_extern` の `by_type`、`Extern::{Eq, Ne}` と `Prelude.==` / `Prelude.!=` の行。`Extern::{IntEq, IntNe, StrEq, StrNe, BoolEq, BoolNe, ShowInt}` と `Int` の比較の行は、instance の行に改める (下の「Prelude と extern の表」)。`FunctionRow.ret` の注釈の「データを返す行は `Bool` と `read_all` の組」に `Ordering` を足す
- `eml_types::equality`、`Equality`、`check/equality.rs`、`codes::NOT_COMPARABLE`
- translate の `equality_extern` と `by_type` の分岐 (`translate/expr.rs`)、`translate/program.rs` の `extern_wrapper` の `by_type` の確認
- `eml_interp/src/externs.rs` の `Extern::Eq | Extern::Ne` の腕
- verifier の「`Prelude.==` と `Prelude.!=` の行を誤りにする」規則
- Core IR の定数に `Ordering` のタグ (`LT`、`EQ`、`GT`) を `FALSE` と `TRUE` の隣に足し、Prelude の宣言と一致することをテストで確かめる

## Prelude と extern の表

```haskell
pub data Ordering =
  | LT
  | EQ
  | GT
  deriving (Eq, Ord, Show)

pub class Eq a where
  (==) : a -> a -> Bool
  (!=) : a -> a -> Bool
  x != y = not (x == y)

pub class Eq a => Ord a where
  compare : a -> a -> Ordering
  (<) : a -> a -> Bool
  x < y = match compare x y with
    | LT -> True
    | _ -> False
  -- (<=)、(>)、(>=) も compare からの既定を持つ

pub class Show a where
  show : a -> String
  show_prec : Int -> a -> String
  show_prec _ x = show x
```

| instance | 定義 |
|---|---|
| `Eq Int` | `extern (==)`、`extern (!=)` |
| `Ord Int` | `extern compare`、`extern (<)`、`extern (<=)`、`extern (>)`、`extern (>=)` |
| `Show Int` | `extern show`。`show_prec` は Prelude に書き、`d > 6` で負の数なら括弧で囲む |
| `Eq String` | `extern (==)`、`extern (!=)` |
| `Ord String` | `extern compare` (バイト順)。`<` などは既定 |
| `Show String` | `extern show` (下のエスケープ) |
| `Eq Bool` | `extern (==)`、`extern (!=)` |
| `Ord Bool`、`Show Bool` | `data Bool = | False | True deriving (Ord, Show)` |

- `Show String` の `show` は、文字列を `"` で囲み、`"` を `\"`、`\` を `\\`、改行を `\n`、タブを `\t`、復帰を `\r`、NUL を `\0` と書く。ほかの U+0000〜U+001F と U+007F は `\u{…}` (16進の小文字、先頭の 0 を省く) と書く。それ以外の文字はそのまま書く。S6 で補間が入ったら `{` も `\{` にする
- extern の行は、`Prelude.int_eq` などを instance の行 (`Prelude.Eq Int.==` など) に改め、`Prelude.<` などの `Int` の比較の行を `Prelude.Ord Int.<` などに改める。新しい行は `Int` と `String` の `compare`、`String` の `show` の3つである。`Prelude.show_int` は `Prelude.Show Int.show` になり、`pub show_int` は Prelude からなくなる
- `compare` の行の結果の Repr は `enum` で、タグは `Ordering` の宣言の順 (`LT` が 0) である
- extern の行と `std/` を結ぶテストを、instance の `extern` に広げる。instance の行は `Function` を持たないので、テストは「どの行も、std の extern の関数か instance の `extern` の行のちょうど1つから指される」を確かめる。instance の行のシグネチャは、クラスのメソッドのシグネチャのクラスの型変数を頭の型に置き換えたものである
- 演算子の fixity の宣言 (`infix 4 ==, !=, <, <=, >, >=`) は今のままで、メソッドに付く
- `max` と `min` は S5 では入れない

## 診断

| 番号 | 名前 | 意味 |
|---|---|---|
| E1033 | `EXTERN_OUTSIDE_STD` | 範囲を広げ、ユーザーのモジュールの instance の中の `extern` を含める |
| E1034 | `ORPHAN_INSTANCE` | クラスも頭の型も定義していないモジュールの instance |
| E1035 | `DUPLICATE_INSTANCE` | 同じ (クラス, 型) の2つ目の instance。`deriving` との重なりを含む |
| E1036 | `MISSING_METHOD` | 既定のないメソッドを instance が定義していない |
| E1037 | `UNKNOWN_METHOD` | クラスにないメソッドを instance が定義した |
| E1038 | `NOT_DERIVABLE` | `deriving` に Prelude の `Eq`、`Ord`、`Show` 以外のクラスを書いた |
| E1039 | `INVALID_INSTANCE_HEAD` | instance の頭が、`data` か extern の型のコンストラクタに互いに異なる型変数を適用した形でない |
| E1040 | `INVALID_CONSTRAINT` | 制約を書けない位置の制約 (extern と操作のシグネチャ)、型に現れない型変数への制約、頭やクラスの型変数でない型変数への文脈、クラスの型変数への制約を持つメソッドのシグネチャ、クラスの型変数を含まないメソッドのシグネチャ、制約の形 (クラスと1つの型変数) でない文脈 |
| E1041 | `NOT_A_CLASS` | クラスを書く位置 (制約、instance の頭、`deriving`) の名前が、型かエフェクトを指す |
| E1042 | `SUPERCLASS_CYCLE` | 上位クラスの関係の循環 |
| E1043 | `CLASS_AS_TYPE` | 型を書く位置の名前がクラスを指す |
| E2006 | `NO_INSTANCE` | 名前と意味を改める。制約 `C T` を満たす instance がない。上位クラスの instance がないこと、導出のフィールドが instance を持たないことを含む |
| E2009 | `AMBIGUOUS_CONSTRAINT` | 制約の型が最後まで決まらない |
| E2010 | `LINEAR_INSTANCE_HEAD` | `Unr` のクラスの instance の頭が `Unr` でない |
| E2011 | `METHOD_KIND_MISMATCH` | instance のメソッドか既定のメソッドが、クラスのシグネチャから導けない Kind の制約 (線形性と多重度の境界、持ち越しの制約) を持つ |
| E2012 | `CONSTRAINED_POLYMORPHIC_RECURSION` | 一様な位置の型変数が制約を持つ |

- E2006 は、E2007 のように欠番にせず、名前と意味を改めて使い続ける。新しい意味 (instance がない) は、今の意味 (`Int`、`String`、`Bool` のどれでもない型の値を比べた) を含む一般化だからである。この理由を `docs/spec/diagnostics.md` に書く
- E2006 の文言は「`T` に `C` の instance がない」(`no instance of `Eq` for `Color``) の趣旨にする。`==` の位置に出る場合も、演算子でなくクラスの言葉で言う。文言と help の細部は実装の計画で決め、`docs/spec/diagnostics.md` と `docs/implementation/diagnostics.md` に移す

## テストの変更

テストの変更の種類は `docs/implementation/testing.md` の「テストの変更の運用」に従う。

### 成否の変更

- `eml_types/tests/tuples.rs` の `the_operand_type_decides_how_equality_compares` と `an_operand_type_decided_after_the_operator_is_used`、補助の関数 `decided` を消す。`Equality` の値を確かめるテストで、`eml_types::equality` がなくなるためである。比べ方の選択は、translate の解決のテスト (下の新しいテスト) が確かめる
- `eml_core_ir/tests/translate.rs` の `equality_picks_the_comparison_of_the_operand_type` を消す。`by_type` の置き換えがなくなるためである。解決のテストに置き換える
- `eml_core_ir/tests/verify.rs` の、`Prelude.==` と `Prelude.!=` の行を誤りにする規則のテストを消す。規則がなくなるためである
- `eml_core_ir/tests/externs.rs` の `extern_function_rows_not_chosen_by_type_are_monomorphic` を消す。`by_type` がなくなり、すべての行が型で選ばれないので、`extern` の行のシグネチャを確かめるテスト (下の期待値の変更) に含まれるためである
- `eml_hir/tests/structure.rs` の `prelude_functions_with_equations_are_not_extern` の、`show_int` を `Extern::ShowInt` の関数として引く部分は、`show` が関数でなくメソッドになるので成り立たない。この部分を消し、`negate` など残る extern の関数で確かめる
- `show_int` を `show` に書き換えると、`show_int` や `Int` だけの `<` が唯一オペランドの型を決めていた箇所は E2009 になりうる。書き換えでそうなった UI テストには、型の注釈を足して成否を保つ。足せない形が見つかったら、この節に1つずつ足す
- 実装の計画で、消す API や規則だけを確かめるテストがほかに見つかったら、この節に1つずつ足す

### 期待値の変更

- E2006 を使うテストの診断を、新しい E2006 (`NO_INSTANCE`) と E2009 に書き直す。対象は UI の `check-fail/types/not_comparable.em` と `check-fail/types/ambiguous_equality_section.em`、単体テストの8本 (`eml_types/tests/tuples.rs` の5本、`modules.rs`、`check.rs`、`instantiations.rs` の各1本) である。`not_comparable.em` の `(Int, Int) != …` は誤りでなくなる。`ambiguous_equality_section.em` は E2009 になる。どちらも check-fail のままである。`check.rs` の `an_equality_reference_with_an_unknown_type_is_not_comparable` は E2009 を確かめるので、名前も改める
- `show_int` を使うテスト (UI の約100本、`bench/` のプログラム、各 crate の単体テスト) を `show` に書き換える。標準出力は変わらない。変わるのは、check-fail の診断の列、run-fail の実行時エラーの位置 (`at path:line:column` の列。`integer_overflow`、`division_by_zero`、`fault_in_imported_module` など)、Core IR のダンプの extern の名前と位置 (`@"path":l:c`)、単体テストのスナップショットである
- `tests/ui/run/modules/qualified/Report/Csv.em` は、自分の `show` が Prelude の `show` を隠すので、`show_int n` を `Prelude.show n` に書き換える。ほかに `show` を定義するモジュールで `show_int` を使うテストがあれば、同じく `Prelude.show` にする
- `eml_hir/tests/structure.rs` の `prelude_extern_signatures_are_extern_functions` は、名前の並びから `show_int` と `==` を外す (どちらもメソッドになるため)。`eml_hir/tests/externs.rs` の `every_extern_function_is_declared_once_in_std` と `every_extern_declaration_in_std_names_a_row` は、instance の `extern` の行を含めた形に改める (上の「Prelude と extern の表」)
- `eml_syntax/tests/parser.rs` の `reserved_keywords_are_errors` は、予約のまま残る `forall` で確かめる形にする
- `==`、`!=`、`<` などを含む Core IR のダンプの extern の名前を、instance の行の名前 (引用符つき) に改める
- 型検査のダンプの持ち越しの制約の表示を `carry(…)` に改める (`eml_types/tests/linearity.rs` など)
- 字句のテストで、`=>` を予約の記号として、`deriving` をキーワードとして読む
- ベンチマークの `RunStats` は変わらない見込みである。`bench/` の書き換えは `show_int` の名前だけである。変わった場合は、差を記録したうえでこの節に足す

### 新しいテスト

- UI の `run/classes/`: ユーザーのクラスと instance、上位クラス、既定のメソッドと上書き、制約付きの多相な関数、メソッド自身の型変数と制約、`deriving` (列挙型、型引数、再帰する型、中置のコンストラクタ)、タプルと `Unit`、`Show` の括弧 (入れ子と負の数と中置のコンストラクタ)、`String` の `show` のエスケープ、値とセクションとしてのメソッド、モジュールをまたぐクラスと instance (`Eq(..)` の import を含む)、メソッドの演算子の fixity、別のメソッドを大きな型で呼ぶ instance (`p != q = not (Box p == Box q)`。E2012 にならないこと)
- UI の `check-fail/classes/`: 上の診断の表の新しい番号ごとに1本以上。`deriving Show` を付けた入れ子のデータ型 (E2012)、`Lin` のフィールドを持つ型の `deriving Eq` (E2010)、メソッド自身の型変数で大きくなる再帰 (E2012) を含む
- `eml_syntax`: `class`、`instance`、文脈の先読み、`deriving` の各位置の構文と、誤りの回復
- `eml_hir`: 名前空間、import、orphan 規則、重なる instance、メソッドの検査、型変数の名前の付け替え
- `eml_types`: 制約の解決、上位クラス、曖昧な制約、Kind の規則 (`Ti ≤ Unr`、E2010、E2011)、一様な位置のグラフの計算 (新しく書く。`eml_core_ir/tests/instances.rs` のテストは instance の名前を確かめるので、そのまま残す)
- `eml_core_ir`: 解決の結果の関数の名前、`extern` の直接の呼び出しと引用符、生成器の出力 (Core IR のテキスト)、`Ordering` のタグと Prelude の宣言の一致

## 文書の更新

段の終わりに、次の文書へ決定を移す。

- `docs/spec/lexical.md`: `deriving` のキーワード、`=>` の予約、`class` と `instance` の予約の記述、`show_int` の例
- `docs/spec/layout.md`、`docs/spec/expressions.md`、`docs/spec/effects.md`、`docs/overview.md`: `show_int` の例
- `docs/spec/grammar.md`: 上の文法
- `docs/spec/declarations.md`: `class`、`instance`、`deriving` の節、標準の演算子の表
- `docs/spec/types.md`: 制約、`Unr` のクラス、メソッドの Kind の規則、持ち越しの制約の表示
- `docs/spec/modules.md`: 名前空間、`Eq(..)`、instance の可視性と orphan 規則
- `docs/spec/core-ir.md`: 解決、名前、生成器、一様な位置の計算の置き場所、`==` の項目の削除、`by_type` と `Prelude.==` に触れた箇所 (変換の規則、verifier)、データの配置の「extern が作るデータ」に `Ordering` を足すこと
- `docs/spec/diagnostics.md`: 診断の表と E2006 の理由
- `docs/implementation/testing.md`: `by_type` に触れた箇所、Core IR のテキストの引用符の規則 (extern の名前を含める)、verifier の規則
- `docs/implementation/diagnostics.md`: E2006 と E1033 の出し方、新しい番号の出し方
- `docs/implementation/status.md`、`docs/implementation/architecture.md`: 段の終わりに1回で直す
- `docs/implementation/benchmarks.md`: `bench/run.sh` の結果を記録する
- `docs/future/stdlib.md`: `show_int` の記述と、ロードマップの「S5 型クラス」への参照を直す (S5 の節を消すと `citations.rs` が落ちるため)
- `docs/future/roadmap.md`: S5 の節を消す。「方式を選んだ理由」の表は、`docs/spec/types.md` の制約の節に「方式を選んだ理由」として移す

## ロードマップへの変更

- 「その後の項目」の「型システム」に、`Lin` の型を受けるクラスを足す。`Read` / `Write` のようにハンドルを受け取って返すクラスは、借用がなくても役に立つ。広げるときは、クラスの性質として `Lin` を受けるクラスを区別し、`Lin` を受けるクラスの instance の本体は頭の型変数の Kind に制約を足せない、という規則を置く。表面の構文での Kind の記述が前提になる
- S5 の論点「orphan 規則をパッケージ単位に広げるか」は、パッケージの仕組みがまだないので S5 では決めない。S5 の節を消すときに、この論点を「その後の項目」に移す
- ロードマップの決定から、次の点を変える
  - `Eq` の `Int`、`String`、`Bool` の instance は、Prelude の `int_eq`、`string_eq`、`bool_eq` と `_ne` の extern を呼ぶ、としていた。S5 では、それらの行を instance の行 (`Prelude.Eq Int.==` など) に改め、instance の中の `extern` で結ぶ
  - instance の頭は `data` の型コンストラクタに限る、としていた。S5 では extern の型 (`Int`、`String`) も頭にできる。組み込みの instance を書くためである
  - 論点「表示用のクラスを `Show` 1つにするか、`Show` と `Display` に分けるか」は、`Show` 1つに決めた。S6 の補間の穴と S11 の REPL の表示は `Show` に従う
  - 論点「`Ord` のメソッド」と「既定のメソッドをいつ入れるか」は、`compare` と `< <= > >=` をメソッドにし、既定のメソッドを S5 で入れると決めた。論点「メソッドの Kind」は、`Unr` のクラスと E2011 の規則に決めた。論点「導出した instance で比べないフィールドを `drop` する形」は、型がすべて `Unr` なので要らなくなった
