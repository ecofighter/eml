# 標準ライブラリへの申し送り

位置づけ: 将来の設計。

標準ライブラリの規範は [標準ライブラリ](../spec/stdlib.md) にある。この文書は、構文の設計とマルチコア対応の設計から、まだ入れていない API へ申し送る事項をまとめる。

## 構文の設計からの申し送り

- handler でリソースを持つ API を中心にする: `with_log`、`with_output`、`with_temp_dir`、`with_cwd`、`with_env` など。`use` で並べて使う形を想定する ([式](../spec/expressions.md) の「`use`」と「パラメータ付き handler」)
- `Cmd` 型: コマンドリテラルの結果。`run : Cmd -> <IO> Int`、`read : Cmd -> <IO> String`、`spawn : Cmd -> <IO> Child`、パイプ (`.|` などの演算子)、リダイレクト、環境変数、作業ディレクトリの設定を用意する ([字句](../spec/lexical.md) の「コマンドリテラル」)
- 標準入出力のハンドル: `Stdin`、`Stdout`、`Stderr` の規則は [標準ライブラリ](../spec/stdlib.md) の「ハンドル」にある。extern はハンドルに対する読み書きの操作にとどめ、「行を流す」などのストリーム処理は標準ライブラリにエフェクトとして書く ([標準ライブラリ](../spec/stdlib.md) の「エフェクトのモジュール」)
- `Task a` を `Lin` にする: `Async.start` が返すタスクを、必ず `await` するか `drop` (キャンセル) させる。構造化された並行処理にする
- 標準ライブラリ (`FileIn`、`FileOut`、`Fs` の読み書き、`List` と並べ替え、`String`、`Char`、`Bytes`、`Map`、`Set`、`Env`、`Stdin`、`Stdout`、`Stderr`、`Proc.run`、`eprintln`) と extern の境界の方針は、[標準ライブラリ](../spec/stdlib.md) にある。extern は小さい核にとどめ、どのスクリプトも使う少数の関数だけを eml で書いている。`replace` と、`Option` と `Result` の補助関数は、決めていない
- `Prelude` の範囲: 今は `Bool`、`Ordering`、`List`、`Option`、`Result`、`println`、`eprintln`、`not`、`|>` などの標準の演算子と fixity ([宣言](../spec/declarations.md) の標準の演算子の表)、型クラス `Eq`、`Ord`、`Show`、`Num`、`Div`、`Bits` ([宣言](../spec/declarations.md) の「Prelude のクラス」) を持つ。
- モジュールの候補: `Path`、`Json`、`Csv`、`Toml`、`Regex`、`Http`、`Async`
- 補間の穴は、Prelude の `Show` の `display` で文字列にする ([式](../spec/expressions.md) の「補間」)。標準ライブラリの型を穴に書けるようにするには、`Show` の instance を置く。人が読む形が `show` の形と違う型は、`display` を上書きする
- `Bytes` と UTF-8: `String` は常に妥当な UTF-8 である。`String` の extern は、検証付きのデコード (`decode`)、エンコード (`push`)、バイト長 (`byte_length`)、バイト位置から `Char` を読む操作 (`char_at`)、境界を検査する切り出し (`slice`) の核と、バイトを借りて読む `byte_at`、std の中だけの `find_from` である。文字の反復、分割、検索は eml で書いた ([標準ライブラリ](../spec/stdlib.md) の「`String`」)
- エフェクトとジェネレータ: エフェクトのモジュール `Std.Yield`、`Std.Fail`、`Std.State` とその handler、`once` の継続で作る引いて使うジェネレータ `Std.Iter` とその上の変換、`FileIn.read_line` がある ([標準ライブラリ](../spec/stdlib.md) の「エフェクトのモジュール」)。多回の再開ができるジェネレータ (`multi` の `yield`) と、push の形の変換 (`Yield` を受けて `Yield` を出す handler) は入れない。実用のプログラムで要る例が出たら足す。多回の再開ができるジェネレータは、今の `Yield` の意味を変えずに、別のエフェクトとして足せる

## マルチコア対応の設計からの申し送り

詳細は [マルチコア対応の設計](multicore.md) にある。

- 並列計算のライブラリ: `par` / `par_map` / `par_try_map` / `both` は、実行系の核の上にライブラリとして作る。work stealing などのスケジューラもライブラリとして書く
- `par_try_map`: a1 の段階では、失敗の通知を `Result` で返す。「どれか1つが `Err` を返したら残りを打ち切る」処理はスケジューラ側でできるので、`par_try_map : (a -> <> Result e b) -> Array a -> Result e (Array b)` のような関数を用意する
- `both`: `par` と同じ形の逐次版。任意の row を許すので、デバッグするときだけ `par` を `both` に書き換えれば、子の中で `println` できる

  ```haskell
  both : (Unit -> <e> a) -> (Unit -> <e> b) -> <e> (a, b)
  ```

- 並行処理のライブラリ: `Async` の handler、`spawn`、channel
- 並行処理用の共有の可変状態: channel や atomic な参照などのプリミティブを、`IO` か `Async` の側に用意する。`Heap` では、並行タスクの間で共有する可変状態を書けないため
- `freeze`: 一意性だけが理由の `Lin` な型 (後始末のない可変バッファなど) には、型ごとに `freeze` を用意する (例: `freeze : MutArray a -> Array a`)。後始末を持つ型には定義しない
- 配列: `Array a` と `Bytes` は意味が不変の値で、RC が 1 ならその場で書き換える ([標準ライブラリ](../spec/stdlib.md) の「`Std.Array`」と「`Bytes`」)。写しが起きないことを型で保証する `Lin` の `MutArray` もある。`write : MutArray a -> Int -> a -> <Panic> MutArray a` のように値を線形に受け渡す API を持つ ([標準ライブラリ](../spec/stdlib.md) の「`Std.MutArray`」)。`MutArray` を `var` に入れて `a[i]` と `a[i] <- e` で読み書きできる ([式](../spec/expressions.md) の「`var`」)。範囲外の添字は、`Std.MutArray.read` と同じ文言の `Panic` になる。並列の書き換えのための分割と結合のプリミティブは、マルチコア対応で足す
- `trace` は用意しない: row に現れないデバッグ出力の抜け道は作らない
