# 直積型とレコード

位置づけ: 規範。

直積型の構成 (タプルと名前付きのレコード) と、レコードの構文、Kind、線形性の規則、実行時の表現を定める。

## 構成

- 直積型は2種類ある。タプルは構造的で、名前付きのレコードは名前的である
- タプルは `(a, b)` と書き、型は `(A, B)` である。要素は 0 から始まる番号で射影する (`t.0`)
- Unit は要素のないタプルである。型は `Unit` とだけ書き、値とパターンは `()` である
- 名前付きのレコードは、`data` のコンストラクタに名前付きのフィールドを持たせたものである。どの `data` のどのコンストラクタにも持たせてよく、位置のコンストラクタや中置のコンストラクタと同じ `data` に混ぜてよい。フィールドが0個のコンストラクタ (`| Empty {}`) も宣言できる
- 名前付きのフィールドを持つコンストラクタも、位置の引数で使える (Haskell と同じ)。`Person "a" 3` で作り、パターン `Person n a` で分解し、部分適用もできる
- 射影と更新は、コンストラクタが1つの型でだけ使える (Rust の enum の構造体の選択肢と同じ)。コンストラクタが2つ以上の型のフィールドは、`match` で分解して取り出す
- 無名のレコード (`{ name : String }`、`{ name = "a" }`)、レコードの拡張、row 変数を持つレコードはない。row 多相の構造的なレコードを採らないのは、レコードの row の sort、ネイティブでのオフセットの証拠か単相化、`with` とレイアウトの衝突を持ち込み、ユーザーの instance も書けないためである
- 診断と hover では、タプルを `(A, B)`、要素のないタプルを `Unit` と表示する

## 構文

```haskell
data Person =
  | Person { name : String, age : Int }
  deriving (Eq, Ord, Show)

data Shape =
  | Circle { radius : Int }
  | Rect { width : Int, height : Int }
  | Empty

main : Unit -> <IO> Unit
main () =
  let name = "a"
  let p = Person { name, age = 3 }
  let q = { p | age = 31 }
  let Person { name = n, age } = q
  println "\{p.name} \{n} \{age}"
```

| 用途 | 書き方 |
|---|---|
| 宣言 | `\| Person { name : String, age : Int }` |
| 作る式 | `Person { name = "a", age = 3 }`、省略形 `Person { name, age }` |
| 射影 | `p.name`、`t.0` |
| 更新 | `{ p \| age = 31 }`、省略形 `{ p \| age }` |
| 分解のパターン | `Person { name, age = a }` |
| フィールドのセクション | `(.name)`、`(.0)` |

文法は [文法](grammar.md) の `alt`、`atom`、`apat` に、括弧の中のレイアウトは [レイアウト規則](layout.md) の規則 3 と規則 4 に、式の意味と評価の順は [式](expressions.md) の「レコード」にある。

- 作る式の省略形 `{ name }` は `{ name = name }` と同じである。更新の省略形 `{ p | age }` も `{ p | age = age }` と同じである
- 作る式には、宣言したフィールドをすべて1回ずつ書く。順は問わない
- パターンの省略形 `{ name }` は、フィールドの名前の変数を束縛する。パターンに書かないフィールドは `_` と同じに扱う (Haskell と同じ)
- 更新にはフィールドが1つ以上要る。更新は名前でだけ書けるので、タプルは更新できない
- フィールドの名前は値の名前空間に入れない。セレクタの関数は作らず、`p.name` は `p` の型から引く ([型と Kind](types.md) の「フィールドの解決」)。そのため、別々の `data` も、同じ `data` の別々のコンストラクタも、同じフィールドの名前を持ってよい
- フィールドの名前を持たないコンストラクタには、作る式もパターンも `{…}` で書けない。`P {}` のパターンも誤りである (Haskell の `P {}` とは違う)。誤りの番号は [診断](diagnostics.md) の「割り当て済みの番号」にある

## Kind

- 名前付きのレコードは `data` なので、Kind はほかの `data` と同じく、フィールドの Kind の join で推論する ([型と Kind](types.md) の「Kind」)。`Lin` なフィールドが1つでもあれば、その型は `Lin` になる
- タプルの Kind は要素の Kind の join である (`(Fs.File, String) : Type<Lin>`)。要素に書いた型変数は、どれも Kind に効く。`Unit` は `Unr` である

## 線形性の規則

| 操作 | 規則 |
|---|---|
| 射影 `e.f` | `e` を1回消費する。**取り出さない残りのフィールドは、すべて `Unr` でなければならない**。取り出すフィールド自体は `Lin` でもよい |
| 更新 `{ e \| f = v }` | `e` を1回消費する。**上書きされる古い値は `Unr` でなければならない**。残りのフィールドは新しい値に移る |
| 分解のパターン | 各フィールドを別の変数に束縛する。書かないフィールドは `_` で受けたのと同じで、`Unr` でなければならない。`Lin` なレコードは、これで分解する |

```haskell
data Job =
  | Job { name : String, log : Fs.File }

log_of : Job -> Fs.File
log_of j = j.log                  -- OK: 残りの name : String は Unr

name_of : Job -> String
name_of j = j.name                -- エラー: 残りの log : Fs.File が Lin
                                  -- help: 分解のパターンで取り出し、log を drop に渡す

close_job : Job -> <IO> String
close_job j =
  let Job { name, log } = j       -- OK
  Fs.close log
  name

let n = (Fs.read_all f).1         -- エラー: 残りの .0 (Fs.File) が Lin
let (f, text) = Fs.read_all f     -- OK
```

- 違反は E3004 にし、分解のパターンを help で示す。fix は付けない。セクション `(.name)` が組むラムダの隠れた引数は分解を書けないので、セクションの射影には help を付けない ([診断の出し方](../implementation/diagnostics.md) の「線形性の診断」)
- 射影と更新が捨てるフィールドの型が型変数なら、その型変数に `Unr` の制約が付く。`Box a` の値 `b` の `b.x` は、残りのフィールドの `a` に `Unr` の制約を付ける (パターン `Box x _` と同じ)
- `{ p | age = p.age + 1 }` は `p` を2回使うので、`p` の型が `Unr` でなければならない。`Lin` なレコードは、分解して組み直す。どちらも何も捨てないので、制約は付かない

  ```haskell
  data Log =
    | Log { file : Fs.File, lines : Int }

  bump : Log -> Log
  bump l =
    let Log { file, lines } = l
    Log { file, lines = lines + 1 }
  ```

## 実行時の表現

- タプルは、タグ 0 のコンストラクタの値と同じオブジェクトで、要素の順にフィールドを持つ。分解は `match` の分岐と同じ命令で行う ([Core IR とインタプリタ](core-ir.md))
- 名前付きのレコードは、ほかの `data` のコンストラクタと同じ値で表す。フィールドは宣言の順に並び、名前は実行時に残らない。名前付きのフィールドのためだけの配置や命令はない ([Core IR とインタプリタ](core-ir.md) の「変換の規則」)
- フィールドの位置は、コンストラクタとフィールドの名前から静的に決まる。そのため、ネイティブ化でもフィールドのオフセットを証拠として渡す必要はない
- 物理的な表現 (ヘッダ、フィールドの並び) は S9 で決める ([ロードマップ](../future/roadmap.md) の「S9 共通ランタイム `eml_rt`」)
