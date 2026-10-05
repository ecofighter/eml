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

- 操作ごとに多重度 `never` / `once` / `multi` を宣言する。省略したら `once` である。宣言の構文 (操作のカリー化、操作のシグネチャに row を書けないこと) は [宣言](declarations.md) の「`effect`」で定める。
- エフェクトは型引数を持てる (`effect State s`)。row では `<State Int | e>` のように、宣言と同じ個数の型引数を付けて書く。個数が違えば E1015 にする。操作のシグネチャで、エフェクトの型引数と同じ名前の型変数はその型引数を指す。ほかの型変数は、操作ごとに暗黙に量化する。
- `never` の操作の結果の型は、宣言で自由な型変数として書く (`never fail : String -> a`)。呼び出した側ではどんな型として使ってもよい。エフェクトの型引数は、呼び出した側が自由に選べないので、`never` の操作の結果の型に使えない (E1008)。
- 操作は普通の関数として呼ぶ (`get ()`)。表面の構文に `perform` はなく、HIR から Core IR への変換で操作の呼び出しを `perform` 命令に変える ([Core IR とインタプリタ](core-ir.md))。
- 操作は値の名前空間に置くトップレベルの値で、エフェクト名は型名と同じ型の名前空間に置く。名前の重複と、handler の節の先頭の名前の解決は [モジュールと名前解決](modules.md) で定める。

## 継続の多重度と持ち越し規則

| 操作の宣言 | handler 側の `k` | 呼び出しをまたいで生きていてよい値 |
|---|---|---|
| `never` | なし | `Unr`、`Lin` (中断時に drop される) |
| `once` (デフォルト) | `Lin`。`resume k v` か `drop k` を必ず書く | `Unr`、`Lin` (同上) |
| `multi` | `Unr` | `Unr` のみ |

`multi` の操作の `k` は何度でも再開でき、`drop k` しても使わなくてもよい。共有された継続を再開するときは、捕まえた区間を写してから再開する ([Core IR とインタプリタ](core-ir.md))。

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

- `open` の相対パスは、実行の設定 (`RunConfig::file_root`) の基準ディレクトリから解釈する。CLI の `run` はカレントディレクトリを基準にする。絶対パスはそのまま開く。
- `read_all` は現在の位置から最後までを UTF-8 として読み、同じ `File` と組にして返す。
- `close f` と `drop f` は、どちらも `File` の破棄処理 (オブジェクトの解放) を呼ぶ。
- 開けない、読めない、UTF-8 でないときは実行時エラーにする ([Core IR とインタプリタ](core-ir.md))。

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
- 継続は `resume k v` で再開し、`drop k` で捨てる。
- 1つの handler は1つのエフェクトを扱い、そのエフェクトのすべての操作に節を書く。複数のエフェクトは handler を入れ子にして扱う。別のエフェクトの節が混ざったら E1012、節のない操作があれば E1013、同じ操作の節が2つあれば E1014 にする ([診断](diagnostics.md))。
- `k` は第一級の値である。変数に束縛し、関数に渡し、クロージャで捕まえられる。再開は `resume`、破棄は `drop` だけで行う。`k` の型 (継続の型) は表面の構文に名前を持たず、シグネチャには書けない。診断では `Cont Int Unit <IO>` のように、操作の結果の型、handle の結果の型、handle の外側の row の順に表示する。継続の型は handler の状態の欄も持つ (下の「パラメータ付き handler」)。状態のある handler の `k` は、`Cont Int Unit <IO> from File` のように、最後に状態の型を付けて表示する。
- 操作の節は、handler が生きている間、操作が起きるたびに呼ばれる。そのため、節が捕まえる変数には `Unr` の制約が付く。handle の本体と `return` の節は高々1回しか動かないので、`Lin` の値も捕まえられる ([線形性](linearity.md))。ただし、扱うエフェクトに `multi` の操作があれば、`k` を再開するたびに handler フレームを含む区間が写され、`return` の節も何度も動きうる。そのため、`return` の節が捕まえる変数にも `Unr` の制約が付く。
- 操作の引数の型に現れる Kind 変数 (型変数の Kind と矢印の線形性) は `Unr` に固定する。操作は本体を持たないので、節が引数をどう使うかを操作の型に推論できないためである。そのため、`k` のような線形な値は、多相な操作の引数に渡せない (E3001)。節では、その引数を何回使ってもよい。結果の型だけに現れる Kind 変数は縛らないので、`never fail : String -> a` はどの型としても使える。単相な `Lin` 型の引数には、この規則は関わらない ([線形性](linearity.md))。エフェクトの型引数の Kind 変数は固定しない。handle ごとに具体的な型で節を検査するので、節での使い方がその型の Kind に伝わる。
- handle は、扱うエフェクトの型引数を handle ごとの新しい推論用の変数にし、本体を `<E α1 .. αn | ρ>` で検査する。節では、エフェクトの型引数をその変数で、操作自身の型変数を節だけの rigid な変数で具体化する。
- 中断 (`drop k`、または `never` の操作の perform) のときだけ、捕まっていた `Lin` 値を型に宣言された破棄処理で drop する。通常の制御フローでは暗黙の後始末を行わない ([線形性](linearity.md))。
- 節の書き方、節の引数の個数、`return` の節の省略は [式](expressions.md) の「handler」で定める。

## パラメータ付き handler

```haskell
run_state : s -> (Unit -> <State s | e> a) -> <e> (a, s)
run_state init action =
  handle action () from init with
    | get () k st -> resume k st st
    | put st2 k _ -> resume k () st2
    | return x st -> (x, st)
```

- パラメータ付き handler は、状態を handler フレームに持つ。脱糖はしない。構文、評価の順、省略した `return` の節は [式](expressions.md) の「パラメータ付き handler」で定める。deep handler の意味と中断時の後始末は、状態のない handler と同じである。
- 初期値の型を状態の型 σ とする。各操作の節は最後の引数に σ を受け取り、`return` の節は本体の値と σ を受け取る。
- 継続の型は、つねに状態の欄を持つ。欄は、状態のない handler では「状態なし」、状態のある handler では「状態 σ」である。`from ()` と書いた handler の欄は「状態 `Unit`」で、状態のない handler とは区別する。
- 欄は型とは別のもので、推論の中だけで使う。欄の推論用の変数は型変数と分け、型の位置には現れない。「状態なし」を `Unit` で代用しないのは、同じ再開に `resume k v` と `resume k v ()` の2つの書き方ができてしまうためである。また、欄を普通の型で表すと、`multi` の `k` を `resume k 1 s` と `resume k 2` の両方で再開したときに、`s` の型が「状態なし」になってしまう。
- 2引数の `resume k v` は `k` の欄を「状態なし」と単一化し、3引数の `resume k v s` は欄を「状態 σ」と単一化する (`s` は σ)。`k` を節の外に渡しても、どちらの個数が合うかは単一化で決まり、個数を `k` の型と照らし合わせる別の検査は要らない。状態のない handler は2引数だけで、状態のある handler (`from ()` を含む) は3引数だけで再開する。
- 状態は区間の外 (節の手元) にあるので、σ は `k` の Kind に影響しない。
- 欄の単一化に失敗したら、型の不一致 (E2001) ではなく専用の E2xxx として報告する ([診断](diagnostics.md))。
- 操作が起きると、handler フレームから状態を取り出して節に渡す。継続の区間に残る handler フレームは、状態を持たない。`resume k v s` は、`s` を区間の handler フレームに戻してから再開する。本体の値が handler フレームに届いたら、状態を取り出して `return` の節に渡す ([Core IR とインタプリタ](core-ir.md))。
- 中断 (`drop k`、または `never` の操作の perform) のとき、状態は節が持っている。そのため、区間の解放では状態を解放しない。状態の後始末は、節の線形性の検査で確かめる。
- 節の中の状態は普通の引数なので、線形性の検査がそのまま当てはまる ([線形性](linearity.md) の「パラメータ付き handler の状態」)。
- handle の本体を実行している間、状態は handler フレームにある。そのため、状態は handle の呼び出しをまたいで生きている値として、持ち越し規則にかける。たとえば handle の外側の row に `multi` の操作があれば、状態は `Unr` でなければならない。
