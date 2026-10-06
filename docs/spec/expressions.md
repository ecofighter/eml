# 式

位置づけ: 規範。

式の構文と、HIR で行う脱糖を定める。handler については構文と脱糖だけを扱い、意味 (deep handler、操作の多重度、継続の線形性) は [エフェクトと handler](effects.md) で定める。

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
- 最後でない式文は `Unit` 型でなければならない
- `let` による同じ名前の再束縛 (シャドーイング) を許す
- この2つと `Lin` の値の関係 (式文で捨てる、消費前に隠す) は [線形性](linearity.md) の「基本の規則」で定める
- 1行で書くための式の形 `let p = e in e2` を用意する。ブロックの `let` と同じ意味である
- ローカルの `let` は単相で、注釈なしで推論する ([型と Kind](types.md))

## ラムダ

```haskell
people |> filter (fn p -> p.age >= 18)
each items fn item ->
  println item
```

- `fn x y -> e`、`fn (a, b) -> e`、`fn () -> e`、`fn (x : Int) -> e` と書く
- 型の明示 `(p : T)` は、ラムダの引数だけでなく、どのパターンの位置でも書ける。タプルの要素に書くときは `((a : Int), b)` とする。`(a : Int, b)` は構文エラーである
- 関数適用の最後の引数がラムダなら、括弧なしで書ける (Haskell の BlockArguments)
- `->` で行が終われば、本体はブロックになる

## 関数適用

- 関数はカリー化する。部分適用はクロージャになる
- 関数適用 `e0 e1 … en` は、呼ばれる式 `e0` を評価し、`i = 1..n` の順に `ei` を評価してから、それまでの値に矢印 `i` を適用する。括弧は順を変えない (`(f 1) (g ())` と `f 1 (g ())` は同じ式である)。`x |> f a` は `(|>) x (f a)` の呼び出しなので、引数の順に `x` を先に評価する ([宣言](declarations.md) の `|>`)
- 観測できる順を変えない範囲で、処理系は引数をまとめて1回の呼び出しで渡す。既知の関数、組み込み、操作、コンストラクタの引数の数までの引数は、本体が引数のそろうまで動かないのでまとめる。それを超える引数と関数値の呼び出しでは、値の引数を前とまとめる。引数のないトップレベルの値は、参照するたびに計算して関数値を返すので、既知の関数に含めず、関数値として扱う。値とは、リテラル、局所変数、ラムダ、組み込みと操作とコンストラクタの参照、引数のあるトップレベルの関数の参照、値に型注釈を付けた式である。まとめ方は持ち越し規則 ([エフェクトと handler](effects.md)) にも効くので、`eml_hir::call_steps` の1か所で決める

## `if`

```haskell
if errors > 0 then
  eprintln "\{show_int errors} errors"
if s >= 1 then println line
else if verbose () then println "skipped"
```

- **then 節の型が `Unit` の場合、`else` を省略できる。** `else ()` を補ったものとして扱う (HIR は `else` のない `if` として残し、型検査は then 節が `Unit` でなければ診断する)。正格評価なので、Haskell の `when c (println x)` は `println x` を必ず評価してしまう。そのため構文で用意する
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
- 網羅性の検査は [網羅性](exhaustiveness.md) で定める

## 演算子の列と単項マイナス

- CST では、`a op1 b op2 c` を平たい列 (`OP_SEQ` ノード) のまま持つ
- HIR への変換で、名前解決の後に fixity の表を引いて木に組み直す (GHC の renamer と同じ方式)。結合しない演算子同士の並び (`a < b < c`) や、優先順位が同じで結合の向きが違う演算子の並びは、エラーとする (E1006)
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
| `(+ a * b)` | `fn x -> x + a * b` |
| `(a * b +)` | `fn x -> a * b + x` |

- セクションの被演算子は演算子の列でもよい。HIR で fixity の表を引いて組み直し、セクションの演算子を空いた側に置いた式として読めるときだけ許す。被演算子の中の演算子は、セクションの演算子より強く結合するか、優先順位が同じで結合の向きが空いた側に合っていなければならない。そうでなければエラーとする (E1023)。たとえば `(* a + b)` はエラーである。左結合の `+` では、`(a + b +)` は許し、`(+ a + b)` はエラーとする
- 被演算子が前置の `-` で始まるときも、演算子の列と同じ規則で読む。右が空いたセクション `(- e op)` では、前置の `-` を優先順位 6 の左結合の演算子として数える。そのため `(- 2 *)` はエラー (E1023) で、`(- 2 +)` は `fn x -> (- 2) + x` である。左が空いたセクション `(op - e)` は、演算子の列 `x op - e` で前置の `-` が許されないときにエラー (E1006) とする。たとえば `(+ -1)` と `(* - 2)` はエラーである

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
- ブロックの最後の文が `use` の場合はエラーとする (包む残りがない) (E1024)
- `use` の後ろのコードは、すべてラムダの中に入る。そのラムダが `multi` で複数回再開されれば、`use` の後ろのコードも複数回実行される。この点は LSP の hover とドキュメントで補う

## handler

例は [エフェクトと handler](effects.md) の「handler の意味」にある。

- 節は `| op 引数... k -> 本体` と `| return x -> 本体` の2種類である
- 節の引数の個数は、`never` の操作なら「操作の引数の個数」、それ以外なら「操作の引数の個数 + 1 (`k`)」とする。個数は HIR で検査する (E1010)
- 節の先頭の名前は、エフェクトの操作だけから解決する ([モジュールと名前解決](modules.md))。操作でない名前は E1001 に、組み込みの `IO` の操作は E1009 にする
- `return` の節は省略でき、省略したら `| return x -> x` とみなす
- 継続は `resume k v` で再開し、`drop k` で捨てる
- 1つの handler は1つのエフェクトのすべての操作に節を書く ([エフェクトと handler](effects.md) の「handler の意味」)

handler の意味、`k` の線形性、`IO` の操作に節を書けないことなどは [エフェクトと handler](effects.md) で定める。

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
- 初期値を先に評価し、その後で本体を実行する
- 状態は handler が持ち、脱糖しない。観測できる振る舞いは、次のように関数を返す handler に初期値を適用した形と同じで、handler の意味論 (deep handler、中断時の後始末) も変わらない。ただし、評価の順は上の規則に従う

  ```haskell
  (handle action () with
     | log lv msg k -> fn f -> (resume k ()) (... f ...)
     | return x -> fn f -> ...) (open path)
  ```

  脱糖しないのは、状態を handler フレームに置くと、`| get () k st -> resume k st st` のような節がすぐに再開する形のまま残るためである。将来の evidence passing の最適化を、そのまま当てはめられる ([evidence passing の設計](../future/evidence-passing.md))
- `return` の節を省略したら、`| return x _ -> x` とみなす。状態を `_` で捨てるので、状態の型に `Unr` の制約が付く。`Lin` の状態で省くと E3004 になり、`from` の初期値を指す。`Lin` の状態では `return` の節を書く
- 状態の線形性の規則は [線形性](linearity.md) の「パラメータ付き handler の状態」で定める
- `resume` の引数が 2 個でも 3 個でもなければ、HIR で E1011 にする。2引数と3引数のどちらが合うかは、`k` の型の状態の欄の単一化で決まる。状態のない handler は2引数だけで、状態のある handler は3引数だけで再開する ([エフェクトと handler](effects.md) の「パラメータ付き handler」)
- `Ref` には `Unr` の値しか入れられない ([マルチコア対応の設計](../future/multicore.md) の「可変状態」)。そのため、`Lin` なリソースを handler に持たせる方法は、この構文が基本になる

## 並行処理

並行処理のための構文は追加しない。`par` / `spawn` / `run_scheduler` ([マルチコア対応の設計](../future/multicore.md)) は、ラムダと `use` で書ける。非同期の処理は、row に `Async` を持つ普通の関数として直接の形で書け、`async` / `await` も関数の色分けも要らない。

```haskell
fetch_all : List String -> <IO> List Int
fetch_all urls =
  use run_scheduler
  let tasks = map (fn url -> Async.start fn () -> Http.status url) urls
  map Async.await tasks
```

並行処理はまだ実装していない ([マルチコア対応の設計](../future/multicore.md))。

## 型の明示

`(e : T)` と書く。`e` を check モードで `T` に対して検査する ([型と Kind](types.md))。

## HIR で行う脱糖と検査

この文書の構文のうち、HIR への変換で次の脱糖と検査を行う。

- 演算子の列の組み直し、単項マイナス、セクション、`&&` と `||` の `if` への脱糖 ([宣言](declarations.md) の標準の演算子の表)
- `use`、`if` の `else` の補完、`let ... in`
- handler の節の引数の個数、`resume` の引数の個数
- 補間の `++` の連結への変換 (M3 で実装する)、コマンドリテラルの `Cmd` の構築 (M9 で実装する)

宣言とレコードに関する脱糖は [宣言](declarations.md) と [直積型とレコード](records.md) にある。
