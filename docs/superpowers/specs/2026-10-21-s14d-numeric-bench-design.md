# S14d 数値計算の計測の設計

## 目的と範囲

S14d (数値計算の計測) の設計である。S14 の設計 (`docs/superpowers/specs/2026-10-19-s14-numeric-design.md`) の「S14b〜S14e の方向」の S14d の項目と、[ロードマップ](../../future/roadmap.md) の「S14 数値計算の整備」の「計測」の決めたことと「計測に選ぶプログラム」の論点を、2026-10-21 の議論で詰めた。

S14d の目的は2つある。1つは、ネイティブ化の前に数値計算で Python と比べる基準を作ることである。ネイティブ化の完了の条件 (ロードマップの「処理系」の「ネイティブ化」) は、この基準で測る。もう1つは S14e (小さな単一コンストラクタの型の構造体の表現) の前の値を記録することである。S14e の完了の条件は S14d の計測で、構造体にした型の計算ごとの確保がなくなることである。

S14d ではコンパイラを変えない。計測のプログラムと Python の版、計測のスクリプト、回数のテスト、記録を足す。

## 2026-10-21 に決めたこと

- 計測のプログラムはロードマップの候補の5つ (nbody、spectral-norm、mandelbrot、fannkuch-redux、エラトステネスのふるい) である
- nbody と mandelbrot は、小さな値の型 (`data Vec3`、`data Complex`) とその上の関数で書く。eml で普通に書く形であり、S14e が消す確保がこの形に現れるためである。版は1つで、手で値を開いた版の組は作らない
- プログラムは `bench/` に置き、大きさを省略できる引数で受ける。引数がなければ小さな既定の大きさで動き、`crates/eml_vm/tests/bench.rs` の `RunStats` と `bench/run.sh` の命令の数はこの大きさで測る。`bench/scripts.sh` は大きな大きさを渡して Python の版と比べる
- fannkuch-redux とふるいは、今の `bench/fannkuch_var.em` と `bench/sieve_var.em` に大きさの引数を足して使う。既定の大きさは今のままにする
- mandelbrot は集合に入る格子点の数を書く。Benchmarks Game の版の出力は PBM のバイト列だが、今の言語にはバイト列がないためである
- 書いてみて分かった言語の使いにくさは、S14d では直さず、記録してロードマップの論点に移す。プログラムが正しく動かない誤りだけは、S14d で直してテストを足す

## 目標と目標にしないこと

### 目標

- 5つのプログラムが `bench/scripts.sh` で Python の版と同じ出力を出し、実時間と最大の常駐メモリを比べた表が出る
- 新しい3つのプログラムの `RunStats` が `bench.rs` のスナップショットで固定される。S14e はその差分で確保の減りを示す
- 結果と、書いてみて分かったことが、[計測の基準](../../implementation/benchmarks.md) の「S14d」に記録される

### 目標にしないこと

- 速くすること。S14d は基準を記録する段であり、遅い所は記録に残して後の段に回す
- 手で値を開いた `_by_hand` の版。S14e の効果は同じプログラムの前と後の `RunStats` で比べる
- Benchmarks Game の出力の形 (mandelbrot の PBM) と、並列の版

## プログラム

どのプログラムも `main` で `Env.args ()` を読む。引数がなければ既定の大きさで動き、整数の引数が1つならその大きさで動く。それ以外の引数は `panic` で止める。浮動小数は `Float.to_fixed 9` で書く。これは Python の `f"{x:.9f}"` と同じ文字列を作る。

| プログラム | 中身 | 出力 |
|---|---|---|
| `bench/nbody.em` (新しく足す) | Benchmarks Game の5つの天体を、`dt = 0.01` で引数の回数だけ進める | 進める前と後のエネルギー (小数点以下9桁) |
| `bench/spectral_norm.em` (新しく足す) | Benchmarks Game の行列 `A` で、`v` に `Aᵀ A` を掛けることを10回行い、スペクトルノルムを求める | ノルム (小数点以下9桁) |
| `bench/mandelbrot.em` (新しく足す) | 引数の大きさの正方形の格子で、各点の `c` について `z ← z² + c` を最大50回まわし、`\|z\|² > 4` で外れたとみなす | 外れなかった点の数 |
| `bench/fannkuch_var.em` (今のもの) | 今のまま。大きさ `n` を引数で受ける | 今のまま |
| `bench/sieve_var.em` (今のもの) | 今のまま。上限を引数で受ける | 今のまま |

### nbody

- `data Vec3 = | Vec3 Float Float Float` と、`add`、`sub`、`scale`、`dot` の関数で書く
- 位置と速度は、`var pos` と `var vel` の配列の変数 (`MutArray Vec3`) に持つ。質量は `Array Float` に持つ
- 初めに、Benchmarks Game と同じく、運動量を打ち消すように太陽の速度を決める
- 定数の太陽の質量 (`4π²`) は、トップレベルの値の本体に算術を書けないので、リテラルで書く ([S14b の設計](2026-10-19-s14b-float-design.md) の「2026-10-19 に決めたこと」)

### spectral-norm

- `A` の要素は `1.0 / Float.of_int ((i + j) * (i + j + 1) / 2 + i + 1)` である
- `u` と `v` は `MutArray Float` の配列の変数に持ち、`for` と `var` で掛け算を書く
- 結果は `Float.sqrt (vBv / vv)` である

### mandelbrot

- `data Complex = | Complex Float Float` と、足し算、掛け算、絶対値の2乗の関数で書く
- 点 `(x, y)` の `c` は Benchmarks Game と同じく `Complex (2.0 * x / n - 1.5) (2.0 * y / n - 1.0)` である (`x`、`y`、`n` は `Float.of_int` で変換する)

### Python の版と同じ結果にすること

`Float` は IEEE 754 の倍精度で、`Float.sqrt` は Python の `math.sqrt` と同じく正しく丸める。そのため、Python の版で同じ演算を同じ順に行えば、ビットまで同じ結果になる。Python の版は `(a + b) + c` の結合の順と、`dx * dx + dy * dy + dz * dz` の項の順を、eml の版と同じにする。

### 今の表現で起きる確保

`Vec3` と `Complex` は、今は `con` の箱で、`Float` のフィールドは一様な位置なので、それぞれが `Float` の箱である。そのため、`Vec3` を1つ作るたびに4つ、`Complex` を1つ作るたびに3つの確保が起きる。S14e はこれを構造体の値にして確保をなくす。

`MutArray Float` と `MutArray Vec3` の要素は、S14e の後も一様な位置なので、箱のままである。配列に書くたびの箱の確保は S14e では消えない。記録では、S14e が消す確保と消さない確保を分けて書く。

## 大きさ

- 既定の大きさは、`bench/` のほかのプログラムと同じく、debug ビルドで `debug_heap` を付けたときに1本1秒以内で終わるように決める
- `bench/scripts.sh` が渡す大きさは、記録する機種で Python の版がおよそ1秒から3秒で終わるように決め、`bench/scripts.sh` に書く
- どちらの値も、作る人が測って決める

## Python の版

- `bench/numeric/` に、`nbody.py`、`spectral_norm.py`、`mandelbrot.py`、`fannkuch.py`、`sieve.py` を置く。`bench/scripts/` の Python の版は S12d のスクリプトのためのものなので、分けておく
- Python の標準ライブラリで普通に書き、速さのために手で凝ったものにはしない (`bench/scripts/` の版と同じ方針)
- 大きさを唯一の引数で受ける

## 比べ方と測り方

### `bench/scripts/compare.sh`

パスを直接受ける形 `compare.sh --paths EML_FILE PY_FILE [ARG...]` を足す。今の `compare.sh NAME [ARG...]` は `tests/ui/run/scripts/NAME/main.em` と `bench/scripts/NAME.py` を渡す `--paths` の形にする。`--fixture` の形も今のまま動く。

### `bench/scripts.sh`

今のスクリプトの表の後に、同じ欄 (eml と Python の実時間、比、それぞれの最大の常駐メモリ) の「数値計算」の表を出す。行ごとに次のことを行う。

1. 大きな大きさで、`compare.sh --paths` で eml と Python の出力が同じことを確かめる。違えば止まる
2. 標準入力なしで3回ずつ走らせ、中央値を出す

## 回数のテスト

- `crates/eml_vm/tests/bench.rs` の `PROGRAMS` に、`mandelbrot`、`nbody`、`spectral_norm` を足す (29 から 32)。それぞれに、出力を確かめて `RunStats` の全項目をインラインスナップショットで固定するテストを足す
- テストは引数なしで走るので、既定の大きさで測る

## 書いてみて分かったことの扱い

- 言語の使いにくさは記録の「S14d」の「書いてみて分かったこと」に書き、ロードマップの論点か「その後の項目」に移す。たとえば、配列の変数を範囲の最後の式でしか取り出せない E3008 のために関数を分けたことである
- すでに決まっている制限 (トップレベルの値の本体に算術を書けないこと) は、新しい項目にせず、書いたことを記録に残す
- プログラムが誤った結果を出すか、内部の誤りで止まる誤りは、S14d で直す。直すときは、誤りを再現するテストを先に足す

## 文書の更新

- [計測の基準](../../implementation/benchmarks.md)
  - 「基準のプログラム」の表に3つのプログラムを足し、大きさの引数のことを書く。`sieve_var.em` と `fannkuch_var.em` の行にも引数のことを足す
  - 「スクリプトの計測」に、数値計算の表、`bench/numeric/`、`compare.sh --paths` を書く
  - 「記録」に「S14d」を足す。環境、数値計算の表、`bench/run.sh` の表、5つのプログラムの `RunStats` (計算ごとの確保を主に見る)、S14e が消す確保と消さない確保、書いてみて分かったことを書く
- [ロードマップ](../../future/roadmap.md)
  - S14 の表の S14d を済みにし、段の列の完了の条件の「済み」に S14d を足す
  - 論点の「計測に選ぶプログラム」を、決めたことに移す
  - 書いてみて分かったことを、論点か「その後の項目」に移す
- [テスト戦略](../../implementation/testing.md) の「よく使うコマンド」と `CLAUDE.md` の Commands の、`bench/scripts.sh` と `compare.sh` の説明を、数値計算と `--paths` の形を含むように直す
- [実装の現在地](../../implementation/status.md) と [コンパイラの構成](../../implementation/architecture.md) は、ロードマップの進め方どおり S14 の終わりに直すので、S14d では直さない

## テストの変更

### 種類1 (通過と失敗の変更)

なし。

### 種類2 (期待値の変更)

- `crates/eml_vm/tests/bench.rs` の `sieve_var` と `fannkuch_var` の `RunStats` のスナップショット。`main` が `Env.args` を呼ぶようになるためである
- 同じ理由で `var_matches_hand_written_work` の表 `RESIDUAL` の値が変われば、その値。空の引数の並びは確保しないので、変わらない見込みである

### 足すテスト

- `bench.rs` の `mandelbrot`、`nbody`、`spectral_norm` の回数のテスト (上の「回数のテスト」)

## 作る人に任せること

- 既定の大きさと、`bench/scripts.sh` が渡す大きさの値
- 各プログラムの関数の分け方と名前。Benchmarks Game の版の計算の順を保つ範囲で決める
- `compare.sh` の `--paths` の形の細部 (引数の検査、使い方の文言)
- `bench/scripts.sh` の数値計算の行を出す関数の形
