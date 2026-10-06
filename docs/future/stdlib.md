# 標準ライブラリへの申し送り

位置づけ: 将来の設計。

標準ライブラリの API の設計は、別の spec で扱う。この文書は、構文の設計とマルチコア対応の設計から、その spec へ申し送る事項をまとめる。

## 構文の設計からの申し送り

- handler でリソースを持つ API を中心にする: `with_log`、`with_output`、`with_temp_dir`、`with_cwd`、`with_env` など。`use` で並べて使う形を想定する ([式](../spec/expressions.md) の「`use`」と「パラメータ付き handler」)
- `Cmd` 型: コマンドリテラルの結果。`run : Cmd -> <IO> Int`、`read : Cmd -> <IO> String`、`spawn : Cmd -> <IO> Child`、パイプ (`.|` などの演算子)、リダイレクト、環境変数、作業ディレクトリの設定を用意する ([字句](../spec/lexical.md) の「コマンドリテラル」)
- 標準入出力のハンドル: `Stdin` / `Stdout` / `Stderr` は別の型にして、取り違えを防ぐ。ハンドルは `Lin` で、バッファ付きの読み書きもハンドルにする。intrinsic はハンドルに対する read/write の操作にとどめ、「行を流す」などのストリーム処理は M8 の標準ライブラリにエフェクトとして書く ([ロードマップ](roadmap.md) の「M7 バイト列と入出力」)
- `Task a` を `Lin` にする: `Async.start` が返すタスクを、必ず `await` するか `drop` (キャンセル) させる。構造化された並行処理にする
- `exit`: `never` の操作として、`IO` 側に用意する。終了コードはこれで扱う
- `Prelude` の範囲: `Option`、`Result`、`List`、`Bool`、`println` / `eprintln`、`show_int`、`not`、`|>` などの標準の演算子と fixity ([宣言](../spec/declarations.md) の標準の演算子の表)。M5 の後は、クラス `Eq`、`Ord`、`Num` と表示用のクラスも入る。`show_int` は M5 までの中継ぎである ([ロードマップ](roadmap.md) の「M5 型クラス」)
- モジュールの候補: `Fs`、`Path`、`Proc`、`Env`、`String`、`List`、`Map`、`Json`、`Csv`、`Toml`、`Regex`、`Http`、`Async`
- 補間の穴は M5 まで `String` のみ: それまでは各型に `show_*` 関数を揃える。M5 で穴を表示用のクラスに広げる
- `Bytes` と UTF-8: `String` は常に妥当な UTF-8 である。intrinsic は、検証付きのデコード、エンコード、バイト長、バイト位置から `Char` と次の位置を読む操作、境界を検査する切り出しにとどめる。分割、検索、反復は eml で書く ([ロードマップ](roadmap.md) の「M8 UTF-8 と標準ライブラリ」)

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
- 配列: `Array a` と `Bytes` は意味が不変の値で、RC が 1 ならその場で書き換える (M7)。分割と結合と並列の書き換えのために、`Lin` の `MutArray` を後で足す。`write : MutArray a -> Int -> a -> MutArray a` のように値を線形に受け渡す API と、分割と結合のプリミティブを持つ。Perceus の reuse analysis が前提になる
- `trace` は用意しない: row に現れないデバッグ出力の抜け道は作らない
