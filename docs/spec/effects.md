# エフェクトと handler

位置づけ: 規範。

エフェクト操作の多重度、`Lin` 値の持ち越し規則、handler の意味、組み込みの `IO` と `File` を定める。`effect` 宣言と handler の構文は [宣言](declarations.md) と [式](expressions.md) で扱い、この文書では意味を定める。

## エフェクトと row

- エフェクトは Koka 方式の row 多相で扱う (scoped labels)。row 変数は Kind `Row<s>` (`s ∈ {Never ≤ Once ≤ Multi}`) を持つ ([型と Kind](types.md))。
- 副作用は組み込みの `IO` エフェクトとして管理する。

## エフェクトの宣言と操作

```haskell
effect State s where
  get : Unit -> s
  put : s -> Unit

effect Fail where
  never fail : String -> a
```

- 操作ごとに多重度 `never` / `once` / `multi` を宣言する。省略したら `once` である。
- 操作のカリー化を許す。操作の引数の個数は、シグネチャの一番外側の `->` の数とする。
- 操作のシグネチャの外側の `->` には、どれにも row を書けない (書いたらエラー)。
- `never` の操作の結果の型は、宣言で自由な型変数として書く (`never fail : String -> a`)。呼び出した側ではどんな型として使ってもよい。
- 操作は普通の関数として呼ぶ (`get ()`)。表面の構文に `perform` はなく、HIR から Core IR への変換で操作の呼び出しを `perform` 命令に変える ([Core IR とインタプリタ](core-ir.md))。
- 操作名は値の名前空間に置く。トップレベルの関数名や他の操作名と重なったら重複エラーにする。エフェクト名は型名と同じ型の名前空間に置く ([モジュールと名前解決](modules.md))。

## 継続の多重度と持ち越し規則

| 操作の宣言 | handler 側の `k` | 呼び出しをまたいで生きていてよい値 |
|---|---|---|
| `never` | なし | `Unr`、`Lin` (中断時に drop される) |
| `once` (デフォルト) | `Lin`。`resume k v` か `drop k` を必ず書く | `Unr`、`Lin` (同上) |
| `multi` | `Unr` | `Unr` のみ |

- `once` の継続 `k` は `Lin` なので、handler は `resume k v` か `drop k` を必ず書く。`multi` の継続は `Unr` である。
- 呼び出しをまたいで `Lin` 変数が生きている場合、その呼び出しの row について次のように扱う。この検査は各呼び出し箇所で局所的に行えば、全体として健全になる。
  - row に `multi` な操作が含まれていれば、エラーにする。
  - row 変数 `e : Row<σ>` を含む場合は、制約 `σ ≤ Once` を追加する。その結果、`e` に `multi` な操作が入るような呼び出し方は、制約違反として検出される。束が3要素なので、`σ = Once` ではなく `≤` で書く (`σ = Once` は誤り)。
- 持ち越し規則の検査は、線形性の検査パスで行う ([線形性](linearity.md))。

## 組み込みの `IO`

- `IO` は組み込みのエフェクトで、ユーザーは handle できない。handler に `IO` の操作に対する節を書いたらエラーにする。
- 実行時が必ずちょうど1回再開するので、`once` と同じ扱いになる。ただし中断は起こらない。
- インタプリタでは、継続の最下部にある組み込みの handler として実装する ([Core IR とインタプリタ](core-ir.md))。

マイルストーン1 で組み込みにする操作と型は次のとおり。

| 名前 | 型 |
|---|---|
| `println` | `String -> <IO> Unit` |
| `open` | `String -> <IO> File` |
| `read_all` | `File -> <IO> (File, String)` |
| `close` | `File -> <IO> Unit` |
| `File` | 組み込みの線形型 (`Lin`)。破棄処理は `close` |

組み込みのリソースは、どのスレッドで後始末してもよいものに限る。現状の `File` はこれを満たす ([マルチコア対応の設計](../future/multicore.md))。

## handler の意味

```haskell
try : (Unit -> <Fail | e> a) -> <e> Option a
try action =
  handle action () with
    | fail _   -> None
    | return x -> Some x
```

- deep handler とする。`resume k v` で再開した継続の中で同じ操作が呼ばれたら、同じ handler が処理する。
- 操作の節は `| op 引数... k -> 本体`、return の節は `| return x -> 本体` と書く。
- 節の引数の個数は、`never` の操作なら「操作の引数の個数」、それ以外なら「操作の引数の個数 + 1 (`k`)」である。個数は HIR で検査する (E1xxx)。
- `return` の節は省略でき、省略したら `| return x -> x` とみなす。
- 継続は `resume k v` で再開し、`drop k` で捨てる。
- 中断 (`drop k`、または `never` の操作の perform) のときだけ、捕まっていた `Lin` 値を型に宣言された破棄処理で drop する。通常の制御フローでは暗黙の後始末を行わない ([線形性](linearity.md))。

## パラメータ付き handler

```haskell
run_state : s -> (Unit -> <State s | e> a) -> <e> (a, s)
run_state init action =
  handle action () from init with
    | get () k st -> resume k st st
    | put st2 k _ -> resume k () st2
    | return x st -> (x, st)
```

`Lin` なリソース (`File`) を状態に持つ例は次のとおり。

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

この例は、構文の段階 S2 で実装する文字列の補間を使っている。

- `from 初期値` で handler の状態の初期値を与える。各節 (`return` を含む) は、最後の引数として状態を受け取る。`resume k v s` で、次の状態を渡して再開する。
- 関数を返す handler に初期値を適用する形へ、HIR で脱糖する。上の `with_log` は次のようになる。deep handler の意味と中断時の後始末は変わらない。

  ```haskell
  (handle action () with
     | log lv msg k -> fn f -> (resume k ()) (... f ...)
     | return x -> fn f -> ...) (open path)
  ```

- 状態は普通の引数なので、線形性の検査がそのまま効く。状態が `Lin` なら、各節で `resume` に渡すか、`close` / `drop` しなければならない。`drop k` する節でも、状態の後始末を忘れればエラーになる。
- `resume` の引数の個数 (2 または 3) は HIR で検査する。
- `Ref` には `Unr` の値しか入れられない ([マルチコア対応の設計](../future/multicore.md) の「可変状態」)。そのため、`Lin` なリソースを handler に持たせる方法は、この構文が基本になる。
- 上の `run_state` では、`put` の節で `_` により古い状態を捨てるので、`s` に `Unr` の制約が付く。
