# 標準ライブラリへの申し送り

位置づけ: 将来の設計。

標準ライブラリの API の設計は、別の spec で扱う。この文書は、構文の設計とマルチコア対応の設計から、その spec へ申し送る事項をまとめる。

## 構文の設計からの申し送り

- handler でリソースを持つ API を中心にする: `with_log`、`with_output`、`with_temp_dir`、`with_cwd`、`with_env` など。`use` で並べて使う形を想定する ([式](../spec/expressions.md) の「`use`」と「パラメータ付き handler」)
- `Cmd` 型: コマンドリテラルの結果。`run : Cmd -> <IO> Int`、`read : Cmd -> <IO> String`、`spawn : Cmd -> <IO> Child`、パイプ (`.|` などの演算子)、リダイレクト、環境変数、作業ディレクトリの設定を用意する ([字句](../spec/lexical.md) の「コマンドリテラル」)
- 標準入出力のハンドル: `Stdin` / `Stdout` / `Stderr` は別の型にして、取り違えを防ぐ。ハンドルは `Lin` で、バッファ付きの読み書きもハンドルにする。extern はハンドルに対する read/write の操作にとどめ、「行を流す」などのストリーム処理は標準ライブラリにエフェクトとして書く ([ロードマップ](roadmap.md) の「バイト列と `Array`」)
- `Task a` を `Lin` にする: `Async.start` が返すタスクを、必ず `await` するか `drop` (キャンセル) させる。構造化された並行処理にする
- `exit`: `exit : Int -> <IO> a` の組み込みにする。`never` の操作の中断と同じく `Lin` の値を後始末してから終了し、`try_io` は捕まえない。終了コードはこれで扱う ([ロードマップ](roadmap.md) の「S4 スクリプトの MVP」)
- 最初の標準ライブラリのモジュールは `Fs` で、`std/` ツリーに `File`、`open`、`read_all`、`close` がある ([モジュールと名前解決](../spec/modules.md) の「標準ライブラリ」)。ファイル全体の読み書きは、S4 でこのモジュールに足す
- `Prelude` の範囲: `Option`、`Result`、`List`、`Bool`、`println` / `eprintln`、`show_int`、`not`、`|>` などの標準の演算子と fixity ([宣言](../spec/declarations.md) の標準の演算子の表)。S4 で組み込みのクラス `Eq`、`Ord`、`Show` が入り、`Float`、`Char`、`Num` の段で `Num` が入る。`show_int` は S4 までの中継ぎである ([ロードマップ](roadmap.md) の「S4 スクリプトの MVP」)
- モジュールの候補: `Fs`、`Path`、`Proc`、`Env`、`String`、`List`、`Map`、`Json`、`Csv`、`Toml`、`Regex`、`Http`、`Async`
- 補間の穴は当面 `String` のみ: それまでは各型に `show_*` 関数を揃える。穴を `Show` に広げるかは S4 で決める
- `Bytes` と UTF-8: `String` は常に妥当な UTF-8 である。extern は、検証付きのデコード、エンコード、バイト長、バイト位置から `Char` と次の位置を読む操作、境界を検査する切り出しにとどめる。分割、検索、反復は、S4 では Rust の extern として入れる。eml で書き直すかは後で決める ([ロードマップ](roadmap.md) の「UTF-8 と標準ライブラリ」)

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
- 配列: `Array a` と `Bytes` は意味が不変の値で、RC が 1 ならその場で書き換える (バイト列と `Array` の段)。分割と結合と並列の書き換えのために、`Lin` の `MutArray` を後で足す。`write : MutArray a -> Int -> a -> MutArray a` のように値を線形に受け渡す API と、分割と結合のプリミティブを持つ。Perceus の reuse analysis が前提になる
- `trace` は用意しない: row に現れないデバッグ出力の抜け道は作らない
