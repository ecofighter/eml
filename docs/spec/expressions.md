# 式

位置づけ: 規範。

式の構文と、HIR で行う脱糖を定める。handler については構文と脱糖だけを扱い、意味 (deep handler、操作の多重度、継続の線形性) は [エフェクト](effects.md) で定める。

## ブロックと `let`

```haskell
copy : String -> String -> <IO> Unit
copy src dst =
  let f = open src
  let (f, text) = read_all f
  close f
  write_text dst text
```

- ブロックは文を並べたものである。最後の文が式ならその値がブロックの値で、そうでなければ `()` になる
- 最後でない式文は `Unit` 型でなければならない。`Lin` の値を式文で捨てることは、これにより型エラーとして検出される
- `let` による同じ名前の再束縛 (シャドーイング) を許す。隠された変数が `Lin` でまだ消費されていなければ、消費漏れとして診断する
- 1行で書くための式の形 `let p = e in e2` を用意する。ブロックの `let` と同じ意味である
- ローカルの `let` は単相で、注釈なしで推論する ([型](types.md))

## ラムダ

```haskell
people |> filter (fn p -> p.age >= 18)
each items fn item ->
  println item
```

- `fn x y -> e`、`fn (a, b) -> e`、`fn () -> e`、`fn (x : Int) -> e` と書く
- 関数適用の最後の引数がラムダなら、括弧なしで書ける (Haskell の BlockArguments)
- `->` で行が終われば、本体はブロックになる
- 関数はカリー化する。部分適用はクロージャになる

## `if`

```haskell
if errors > 0 then
  eprintln "\{show_int errors} errors"
if s >= 1 then println line
else if verbose () then println "skipped"
```

- **then 節の型が `Unit` の場合、`else` を省略できる。** HIR で `else ()` を補う。正格評価なので、Haskell の `when c (println x)` は `println x` を必ず評価してしまう。そのため構文で用意する
- `else` の枝はできるだけ遠くまで伸びる (`if c then 1 else 2 + 3` は `else (2 + 3)`)
- `else if` を連ねられる。1行の中に入れ子にした場合、`else` は一番近い `if` に付く
- ブロックの中で `then` / `else` を `if` と同じ列に書いた場合の扱いは [レイアウト規則](layout.md) にある

## `match`

```haskell
match first_word line with
  | "ERROR" -> 2
  | "WARN"  -> 1
  | _ ->
      log "other"
      0

match b with | True -> 1 | False -> 0
```

- 枝は `|` で始める。`with` で行が終われば字下げしたブロック、そうでなければ同じ行に枝を並べる
- 1行の形では、枝の本体の式は次の `|` の手前で終わる (`|` 単独は演算子ではないため)。入れ子の `match` を1行に書いた場合、後ろの枝は内側の `match` に付く
- 網羅性の検査は [網羅性の検査](exhaustiveness.md) で定める

## 演算子の列と単項マイナス

- CST では、`a op1 b op2 c` を平たい列 (`OP_SEQ` ノード) のまま持つ
- HIR への変換で、名前解決の後に fixity の表を引いて木に組み直す (GHC の renamer と同じ方式)。結合しない演算子同士の並び (`a < b < c`) や、優先順位が同じで結合の向きが違う演算子の並びは、エラーとする (E1xxx)
- 単項の `-` は Haskell と同じ扱いにする。`negate` として優先順位 6 で組み直す。`f -1` は `f - 1` (引き算) と読むので、負の数を渡すときは `f (-1)` と書く
- 標準の演算子の表は [宣言](declarations.md) の「fixity」にある

## セクション

| 書き方 | 意味 |
|---|---|
| `(+)` | `fn a b -> a + b` |
| `(+ 1)` | `fn x -> x + 1` |
| `(1 +)` | `fn x -> 1 + x` |
| `(.name)` | `fn x -> x.name` |
| `(- 1)` | 負の数 -1 (セクションではない。Haskell と同じ) |

## `use`

```haskell
main () =
  use with_env
  use tmp <- with_temp_dir
  use with_log "\{tmp}/build.log"
  build tmp
  deploy ()
```

`use` は、ブロックの残りを最後の引数のラムダとして渡す糖衣構文である。

| 書き方 | 脱糖後 |
|---|---|
| `use f a b` の後にブロックの残り | `f a b (fn () -> 残り)` |
| `use p <- f a b` の後にブロックの残り | `f a b (fn p -> 残り)` |

- 純粋な糖衣構文で、型、線形性、エフェクトの規則は増えない
- ブロックの最後の文が `use` の場合はエラーとする (包む残りがない) (E1xxx)
- `use` の後ろのコードは、すべてラムダの中に入る。そのラムダが `multi` で複数回再開されれば、`use` の後ろのコードも複数回実行される。この点は LSP の hover とドキュメントで補う

## handler

```haskell
try : (Unit -> <Fail | e> a) -> <e> Option a
try action =
  handle action () with
    | fail _   -> None
    | return x -> Some x
```

- 節は `| op 引数... k -> 本体` と `| return x -> 本体` の2種類である
- 節の引数の個数は、`never` の操作なら「操作の引数の個数」、それ以外なら「操作の引数の個数 + 1 (`k`)」とする。個数は HIR で検査する
- `return` の節は省略でき、省略したら `| return x -> x` とみなす
- 継続は `resume k v` で再開し、`drop k` で捨てる

handler の意味、`k` の線形性、`IO` の操作に節を書けないことなどは [エフェクト](effects.md) で定める。

## パラメータ付き handler

```haskell
with_log : String -> (Unit -> <Log, IO | e> a) -> <IO | e> a
with_log path action =
  handle action () from open path with
    | log lv msg k f ->
        let f = write f "[\{show_level lv}] \{msg}"
        resume k () f
    | return x f ->
        close f
        x
```

- `from 初期値` で handler の状態の初期値を与える。各節 (`return` を含む) は、最後の引数として状態を受け取る。`resume k v s` で、次の状態を渡して再開する
- 次のように、関数を返す handler に初期値を適用する形へ脱糖する。handler の意味論 (deep handler、中断時の後始末) は変わらない

  ```haskell
  (handle action () with
     | log lv msg k -> fn f -> (resume k ()) (... f ...)
     | return x -> fn f -> ...) (open path)
  ```

- 状態は普通の引数なので、線形性の検査がそのまま効く。状態が `Lin` なら、各節で `resume` に渡すか、`close` / `drop` しなければならない。`drop k` する節でも、状態の後始末を忘れればエラーになる
- `resume` の引数の個数 (2 または 3) は HIR で検査する
- `Ref` には `Unr` の値しか入れられない ([マルチコア対応の設計](../future/multicore.md) の「可変状態」)。そのため、`Lin` なリソースを handler に持たせる方法は、この構文が基本になる

## 直接扱うリソース

`File` などのリソースは、明示的に `open` / `close` する。Python の `with` や Haskell の `bracket` に当たる構文は用意しない。`close` を忘れれば線形性の検査が検出し、中断時は [エフェクト](effects.md) で定める後始末が破棄処理を呼ぶからである。

## 並行処理

並行処理のための構文は追加しない。`par` / `spawn` / `run_scheduler` ([マルチコア対応の設計](../future/multicore.md)) は、ラムダと `use` で書ける。非同期の処理は、row に `Async` を持つ普通の関数として直接の形で書け、`async` / `await` も関数の色分けも要らない。

```haskell
fetch_all : List String -> <IO> List Int
fetch_all urls =
  use run_scheduler
  let tasks = map (fn url -> Async.start fn () -> Http.status url) urls
  map Async.await tasks
```

並行処理はマイルストーン1 の範囲外である。

## 型の明示

`(e : T)` と書く。`e` を check モードで `T` に対して検査する ([型](types.md))。

## HIR で行う脱糖と検査

この文書の構文のうち、HIR への変換で次の脱糖と検査を行う。

- 演算子の列の組み直し、単項マイナス、セクション
- `use`、パラメータ付き handler、`if` の `else` の補完、`let ... in`
- handler の節の引数の個数、`resume` の引数の個数

宣言とレコードに関する脱糖は [宣言](declarations.md) と [直積型とレコード](records.md) にある。
