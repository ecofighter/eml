# S14b `Float` と `Num` / `Div` の設計

## 目的と範囲

S14b (`Float` と `Num` / `Div`) の設計である。S14 の設計 (`docs/superpowers/specs/2026-10-19-s14-numeric-design.md`) の「S14b〜S14e の方向」の S14b の項目と、[ロードマップ](../../future/roadmap.md) の「S14 数値計算の整備」の `Float` と `Num` の決めたことと論点を、2026-10-19 の議論で詰めた。S14 の設計のほかの節 (作業の分け方、S14a で決めた `Panic`) は、そのまま S14b にも当てはまる。

S14b の目的は、S14d の数値計算の計測 (nbody、spectral-norm、mandelbrot) を eml で書けるようにすることである。計測では、eml と Python の版の標準出力が同じであることを `bench/scripts/compare.sh` で確かめる。そのため、`Float` の表示を Python に合わせる。

## 2026-10-19 に決めたこと

S14 の設計で決めたことに、次を足した。

- `Float` の既定の表示 (`show` と文字列の補間) は、Python の `repr` と同じ簡易な形にする。ほかの形式で書き出す関数を別に用意する
- `Float` の比較は、演算子 (`==`、`!=`、`<`、`<=`、`>`、`>=`) を IEEE 754 のとおりにし、`compare` だけを全順序にする (議論での案A)
- `Float` を含む複合の値の大小 (導出した `Ord` とタプルの instance の `<` など) は、今の導出のままクラスの既定の実装で `compare` を通し、全順序に従う (議論での案a)。IEEE に沿った辞書式の演算子は生成しない

S14 の設計で決めたことのうち、S14b に関わるのは次のとおりである。

- 除算は、クラス `Div` (`/` と `%`) に `Int` と `Float` の instance を置く形にする。`Num` は `+ - *` と `negate` を持つ。`Int` の `/` は今と同じく 0 の側へ切り捨てる
- `Num` と `Div` のメソッドは `<Panic>` を持つ。`Float` の演算は `Panic` を起こさないが、メソッドの型はクラスで1つなので、`Float` の instance も型の上では `<Panic>` を持つ
- リテラルは単相にする。整数のリテラルは `Int`、浮動小数のリテラルは `Float` である。`fromInteger` 相当の脱糖も defaulting も入れない。何も型を決めないオペランドは、曖昧さのエラー (E2009) にする
- トップレベルの値の本体は `<>` に限る。`Num` のメソッドは `<Panic>` を持つので、`tau = 2.0 * pi` は書けない。後で入れるコンパイル時の計算で緩める

## 目標、目標にしないこと、避けること

### 目標

- `Float` の型、リテラル、算術、比較、表示、変換、数学の関数、書式、読み取りが動き、nbody、spectral-norm、mandelbrot を書ける
- `Int` の算術は、クラスを通しても今と同じ速さで動く。単相化で instance が決まり、VM の専用の命令になる
- `Float` の算術と比較も、VM の専用の命令になる

### 目標にしないこと

- 型から決める呼び出しの規約 (スカラーの `box` と `unbox` をなくす ABI)。S14e の設計で扱う
- `Float` を確保なしで `tobj` の位置に置くこと (NaN boxing など)。`tobj` の位置の `Float` はいつも箱に入れる
- 書記素や地域の設定に従う書式、`Decimal` のような10進の型

### 避けること

- 表示が Python の `repr` とずれること。計測の出力の照合が通らなくなる
- `Int` だけを使う今のプログラムの実行の仕事の回数 (`RunStats`) が変わること

## 決めた作り

### 型とリテラル

- `Float` は IEEE 754 の倍精度 (64 ビット) である。Prelude の extern の型 `pub extern data Float` にし、`eml_extern` の型の行は `Prelude.Float`、Kind は `Unr`、Repr は新しい `float` である
- 字句は今の `FLOAT` のトークン (`1.5`、`1e9`、`2.5e-3`、`1_000.5`) をそのまま使う。HIR の E0004 をやめ、`Literal::Float` にする。値は Rust の `f64` の読み取りと同じ丸め (最も近い値) で決める。`inf` や `nan` のリテラルは置かず、`Float.infinity` と `Float.nan` を使う
- S14a の負のリテラルの畳み込みを `FLOAT` にも広げる。`-1.5` は負のリテラルになり、`Panic` を起こさない。S14a の最終の確認で見つかった、`-1.5` が E0004 と E2002 を重ねて出す問題も、これで直る
- `Float` のリテラルは、`match` のパターンにも `switch` にも使えない。パターンに書くと誤りにする (新しい番号。文言は「floating-point literals cannot be patterns; compare with `==` instead」)
- `Int` と `Float` を混ぜた演算は型の誤り (E2001) である。変換は下の `Std.Float` の関数で書く

### クラス

Prelude に次のクラスを置く。

```
pub class Num a where
  (+) : a -> a -> <Panic> a
  (-) : a -> a -> <Panic> a
  (*) : a -> a -> <Panic> a
  negate : a -> <Panic> a

pub class Div a where
  (/) : a -> a -> <Panic> a
  (%) : a -> a -> <Panic> a
```

- `Num` と `Div` は互いに上位クラスを持たない (私の判断。`Div` だけを持つ型を作る理由も、両方を要る関数で制約が1つ増える不便も小さい。要れば後で `Num a => Div a` にする)
- 今の Prelude の `extern (+) : Int -> Int -> <Panic> Int` などは、`instance Num Int` と `instance Div Int` の `extern` のメソッドに移す。表の行の名前は `Prelude.Num Int.+` のような instance の行の形に変わる
- 前置の `-` の脱糖は、`Num` のメソッド `negate` を呼ぶ。`negate` は `pub` のメソッドとして見えるようにする。利用者が自分の型 (3次元ベクトルなど) に `Num` の instance を書けるようにするためである
- `Float` の instance: `+ - * /` は IEEE 754 のとおりで、0 で割ると `inf`、`-inf`、`nan` になる。`%` は C の `fmod` で、結果の符号は割られる数と同じである。`Int` の `%` (0 の側への切り捨て) と揃えるためで、Python の `%` (符号は割る数と同じ) とは違う。どれも `Panic` を起こさない
- 単相化で instance が決まるので、`Int` と `Float` の算術は extern の呼び出しになり、VM の専用の命令になる。実行時の辞書は作らない (今のクラスと同じ)

### `Eq`、`Ord`、`Show`

- `Eq Float`: IEEE 754 の `==` と `!=`。NaN はどの値とも等しくなく、`0.0 == -0.0` は真である
- `Ord Float`: `<`、`<=`、`>`、`>=` は IEEE 754 のとおりで、NaN が絡むと偽である。`compare` は全順序 (Rust の `f64::total_cmp`) で、`-nan < -inf < … < -0.0 < 0.0 < … < inf < nan` である。並べ替えと後の `Map` が NaN を含んでも順序として成り立つためである。NaN と ±0 のときだけ、`==` と `compare … == EQ` の答えが食い違う。spec の「Prelude のクラス」にこの例外を書く
- `Show Float`: Python の `repr` と同じ形にする。
  - 往復できる最短の10進の桁を使う
  - 10進の指数が -4 以上 16 未満なら固定小数点で書き、整数の値にも `.0` を付ける (`3.0`、`0.1`、`123456.0`)
  - それ以外は指数の形で書き、指数には符号と2桁以上の数字を付ける (`1e-05`、`1e+16`、`1.5e+16`)
  - 特別な値は `inf`、`-inf`、`nan`、`-0.0` である
  - `show_prec` は、`Int` と同じく、関数適用の引数の位置で負号で始まる表示を括弧に入れる (`Some (-1.5)`)。負号で始まるかで決めるので、`-0.0` と `-inf` も括弧に入る。`Show` のメソッドの本体は `<>` なので、符号のビットを読む `Pure` の extern (`Prelude` の中だけの関数) で見分ける (私の判断)
- 導出した `Eq`、`Ord`、`Show` は、フィールドの `Float` に上の instance を使う。導出した `Ord` とタプルの instance は今と同じく `compare` だけを生成し、`<`、`<=`、`>`、`>=` はクラスの既定の実装が `compare` を通して決める。そのため、`Float` を含む複合の値の大小は全順序に従う (`-0.0 < 0.0` は偽だが、`Some (-0.0) < Some 0.0` は真で、`(0.0, 1) < (-0.0, 2)` は偽になる。最後の例は Python のタプルの比較と違う)。`==` はフィールドごとの `==` なので IEEE のままである。spec の「Prelude のクラス」の例外に、この点も書く

### `Std.Float` モジュール

新しい std のモジュール `std/Float.em` を置く (`docs/spec/stdlib.md` に節を足す)。

| 名前 | 型 | 意味 |
|---|---|---|
| `of_int` | `Int -> Float` | 最も近い値に丸める |
| `truncate` | `Float -> <Panic> Int` | 0 の側へ切り捨てる |
| `floor` / `ceil` | `Float -> <Panic> Int` | 床と天井 |
| `round` | `Float -> <Panic> Int` | 最も近い整数。ちょうど半分なら偶数の側 (Python の `round` と同じ) |
| `sqrt` / `abs` / `sin` / `cos` / `exp` / `log` | `Float -> Float` | IEEE 754 と Rust の `f64` のとおり。定義域の外は `nan` |
| `pow` | `Float -> Float -> Float` | `pow x y` は x の y 乗 |
| `pi` / `infinity` / `nan` | `Float` | 定数 |
| `is_nan` | `Float -> Bool` | NaN か |
| `to_fixed` | `Int -> Float -> <Panic> String` | 小数点以下を指定の桁数で書く。Python の `f"{x:.{n}f}"` と同じ (偶数への丸め、`inf`、`nan`)。桁数が負か 100 を超えると `Panic` |
| `parse` | `String -> Option Float` | 前後の空白を除き、符号、10進と指数の形、`inf` / `infinity` / `nan` (大文字小文字を問わない) を受ける。Python の `float(s)` と違い、数字の間の `_` は受けない (計画を書くときの私の判断。Rust の `f64` の読み取りに合わせる) |

- `truncate`、`floor`、`ceil`、`round` は、NaN、無限大、`Int` の範囲を外れた値で `Panic` になる (文言は「float `{x}` does not fit in `Int`」)
- `pi`、`infinity`、`nan` はトップレベルの値で、本体は extern の関数の呼び出しでなく、`Float` のリテラルか extern の値で書く (私の判断。値の本体は `<>` なので、`Panic` を起こさない書き方にする。`infinity` と `nan` はリテラルがなく、今の言語には extern の値もないので、`Pure` の extern の関数 `Unit -> Float` を std の中で呼ぶ形にする)
- 名前は、`std/List.em` や `std/String.em` と同じく、モジュールで修飾して使う (`Float.sqrt x`)。Prelude には入れない (私の判断。Prelude の名前を増やさない今の方針に合わせる)

### 実行系

- Repr `float` を足す。変数、ブロックの引数、`con` のフィールドでは生の 64 ビットで持つ。`Repr::BOXED_SCALARS` に入れ、`tobj` の位置との間で `box` と `unbox` を入れる
- `eml_rt` に、`Float` の箱の物体の種類を足す。`box` はいつも確保し、`unbox` は中身を読む。浮動小数には、`Int` の即値のような空きのビットがないためである。IR のテキストの形の `float` の定数は、Rust の `{:?}` の形 (`1.5`、`1e16`、`-0.0`、`inf`、`NaN`) で書く
- VM に、`Float` の算術 (`+ - * / %`、`negate`) と比較 (`== != < <= > >=`) の専用の命令を足す。`compare` と `Std.Float` の関数は、ほかの extern と同じ `Op::Extern` にする
- `Float` の extern の行はすべて `Pure` である。ただし、`truncate`、`floor`、`ceil`、`round`、`to_fixed` は `Panic` を起こすので `Panics` である。`Pure` の行は、縮約で使われなければ消える
- `eml_rt` の extern の結果は、`Int` と同じく Verus で確かめる。Verus が `f64` の算術の値を扱えないところは、算術を `verus!` の中に置いたまま結果の性質を約束しない形にするか、信頼の一覧に行を足す (どちらにするかは計画で決める)。`show`、`to_fixed`、`parse` の書式の処理は、Rust の `format!` と `str::parse` を使い、信頼の一覧に行を足す。期待値の表 (`heap/tests/expected.txt`) に `Float` の extern の結果を足す
- REPL の表示器は `Show Float` を使う。`:type 1.5` は `Float` を返す

## 完了の条件

- 上の型、リテラル、クラス、`Eq` / `Ord` / `Show`、`Std.Float` が動く
- `Show Float` と `Float.to_fixed` の出力が、Python 3.14 の `repr` と `f"{x:.{n}f}"` と一致する。代表的な値 (`0.1`、`1/3`、`1e16`、`1e-5`、`-0.0`、`inf`、`nan`、`2.5` と `3.5` の丸め、`1e22`、`5e-324`、`1.7976931348623157e308`) を並べた UI テストの出力を、Python で作った期待値と比べる
- 小さな nbody (数歩) を eml で書いた UI テストの出力が、同じ計算の Python の出力と一致する
- `bench/` の基準のプログラムの `RunStats` が変わらない
- いつもの検査 (`cargo test`、`cargo clippy --all-targets`、`cargo fmt --check`、`cargo verus verify -p eml_rt`、`verusfmt --verus-only --check`、Miri の trusted のテスト) を通る

## 文書の更新

- [字句](../../spec/lexical.md): 浮動小数のリテラル
- [宣言](../../spec/declarations.md): 標準の演算子の表と「Prelude のクラス」(`Num`、`Div`、`Float` の instance、`Ord Float` の例外)
- [型と Kind](../../spec/types.md): `Float`、`Num` と `Div` の制約
- [式](../../spec/expressions.md): 前置の `-` が `Num.negate` を呼ぶこと、`Float` の負のリテラル
- [パターン](../../spec/exhaustiveness.md) と [診断](../../spec/diagnostics.md): `Float` のリテラルのパターンの誤り
- [標準ライブラリ](../../spec/stdlib.md): `Std.Float`
- [Core IR](../../spec/core-ir.md)、[LIR](../../spec/lir.md)、[VM](../../spec/vm.md)、[オブジェクトモデル](../../spec/object-model.md): Repr `float`、箱、専用の命令、信頼の一覧
- [ロードマップ](../../future/roadmap.md): S14 の作業の表で S14b を済みにし、`Float` の論点の答えを書く
- CLAUDE.md
- [実装の現在地](../../implementation/status.md) と [コンパイラの構成](../../implementation/architecture.md) は、S14 の終わりに直す

## テストの変更

### 種類1 (通過と失敗の変更)

- 浮動小数のリテラルの E0004 を確かめていたのは、Rust の単体テスト2つ (`crates/eml_hir/tests/lower.rs` と `crates/eml_types/tests/check.rs` の該当箇所) である。UI の `check-fail` には該当するテストがない。この2つは、浮動小数のリテラルが通ることを確かめる形に変えるか、文字のリテラルの E0004 を確かめる形に変える。計画で1つずつ挙げる

### 種類2 (期待値の変更)

- `crates/eml_core_ir/tests/externs.rs` の `faulting_rows_carry_panic` は、`Pure` の行の std の型の row が空であることを求める。`Float` の `Num` と `Div` の instance の行は `Pure` だが、メソッドの型は `<Panic>` を持つ。そこで、「クラスのメソッドの instance の行は、`Pure` でも row に `Panic` だけを持ってよい」に緩める (私の判断)
- 使われない `let plus = (+)` のように型の決まらない算術は E2009 になる。そうした Rust のテスト (`crates/eml_types/tests/check.rs`、`crates/eml_hir/tests/lower.rs` の該当箇所) を、型を決める形に書き直す
- `+ - * / %` と `negate` が `Num` と `Div` のメソッドになるので、extern の行の名前 (`Prelude.Num Int.+` など) が IR のダンプと extern の表のテストで変わる
- 型の誤りの文言で、算術の演算子の型を表示する所 (制約 `Num a` を含む形になる)
- `eml_rt` の期待値の表に `Float` の行が増える

### 足すテスト

- `Float` のリテラル、算術、比較 (NaN と ±0 を含む)、`compare` の全順序、表示、`Std.Float` の各関数 (`run`)
- `Int` と `Float` を混ぜた演算 (E2001)、`Float` のリテラルのパターン、型の決まらない算術 (E2009) (`check-fail`)
- `truncate` などの範囲外が `try_panic` で捕まり、捕まえなければ実行時エラーになる (`run` と `run-fail`)
- 利用者の型の `Num` の instance (3次元ベクトルの `+`) (`run`)
- `List Float` と `Option Float` を通す多相なコード (`tobj` の位置の箱) を、検査モードで漏れなく動かす (`run`)
- 上の「完了の条件」の Python と照合する表示のテストと、小さな nbody
- REPL: `1.5 + 2.0`、`:type 1.5`、`Some (-0.5)` の表示

## 作る人に任せること

- `Float` の箱の物体の種類を `BigInt` と別の種類にするか、同じ形の物体に種類の番号だけを足すか
- Python の `repr` の最短の桁を、Rust の `{:e}` の出力から組むか、別の方法で求めるか
- `HIR` の `Literal::Float` の値の持ち方 (`f64` のビット列で持って `Eq` と `Hash` を導出するなど)
- 専用の命令の番号の付け方
