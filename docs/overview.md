# eml の概要

位置づけ: 説明。

eml の目的、言語の性格、確定した設計判断の一覧、文書全体で使う用語をまとめる。個々の規則は、各行のリンク先の文書で定める。

## 目的

- 主目的は言語デザインの実験である。「線形型 × 代数的エフェクト」の組み合わせで新しい書き心地を探り、他の人にも使ってもらえる形にする。
- 副目的は、作者自身の日常スクリプト用途で実際に使うことである。テキスト・ファイル処理、シェル自動化、データ変換、ネットワーク・並行の処理を、短く安全に書けるようにする。

言語名の eml は、effect と作者の GitHub ID `ecofighter` を掛けた名前である。ソースファイルの拡張子は `.em` とする。

## 言語の性格

- 関数型、式指向で、イミュータブルがデフォルトである。
- 副作用は `IO` エフェクトとして管理する。
- 線形型と代数的エフェクトを持つ。
- 構文は Haskell 流を基本にし、F# からいくつかの書き方を借りる。構文で迷ったときは Haskell の慣習に寄せる。
- 将来はネイティブコンパイルする。コード生成のバックエンド (Cranelift か LLVM か) は、ネイティブ化の段階で決める。
- rowan (Red-Green Tree) の上に、リッチな診断と LSP を整える。

## 確定した設計判断

### 型システム

| 領域 | 決定 | 詳細 |
|---|---|---|
| 線形性 | Kind で区別する。`Type<m>`、`m ∈ {Unr ≤ Lin}`。Affine は持たず、値を捨てるときは組み込みキーワード `drop` を明示する | [型と Kind](spec/types.md)、[線形性](spec/linearity.md) |
| Kind の将来 | 内部では最初から Kind 変数と部分 Kind 関係を扱う。表面の構文では当面書かせず、将来ユーザーが Kind を書けるようにする | [型と Kind](spec/types.md) |
| エフェクト | Row 多相 (Koka 方式、scoped labels)。row 変数にも Kind `Row<s>`、`s ∈ {Never ≤ Once ≤ Multi}` を持たせる。`Never` は将来のマルチコア対応のための要素 | [型と Kind](spec/types.md)、[エフェクトと handler](spec/effects.md) |
| 継続の多重度 | 操作ごとに `never` / `once` / `multi` を宣言する。デフォルトは `once` | [エフェクトと handler](spec/effects.md) |
| 継続 | 継続 `k` は普通の関数で、`k v` で再開する。状態のある handler では `k v st` と書く。`once` の `k` は `Lin` の矢印を持ち、handler は `k` を呼ぶか `drop k` を必ず書く。`multi` の `k` は `Unr` の矢印を持つ | [エフェクトと handler](spec/effects.md) |
| handler の意味 | deep handler。再開した継続の中でも同じ handler が有効なまま | [エフェクトと handler](spec/effects.md) |
| `IO` | 操作を持たない、ラベルだけの組み込みのエフェクトである。`println` などは `<IO>` を持つ `extern` の関数で、`extern` のエフェクトは handle できない | [エフェクトと handler](spec/effects.md) |
| 暗黙の後始末 | 通常の制御フローでは一切行わない。中断時 (`drop k` や `never` 操作) だけ、捕まっていた `Lin` 値を、その型に宣言された破棄処理で drop する | [線形性](spec/linearity.md)、[エフェクトと handler](spec/effects.md) |
| 型付け | Bidirectional Typing + 単一化。トップレベルの関数は引数と戻り値の型注釈が必須。トップレベルの関数の row はシグネチャで決まり、省略した row は `<>` (純粋) である。Kind は推論する | [型と Kind](spec/types.md) |
| 直積型 | レコードは名前的で、コンストラクタが1つの `data` にフィールドの名前を付けて宣言する (`data Person = Person { name : String, age : Int }`)。タプルは構造的なままで、`t.0` で射影する。Unit は 0 要素のタプル `()` である。Kind はフィールドの Kind の join で推論する (S6 で入れる) | [直積型とレコード](spec/records.md) |
| 等価、比較、表示 | 型クラス `Eq`、`Ord`、`Show` を Prelude に置く。`class` と `instance` の宣言、シグネチャの制約 `Eq a =>`、`deriving` を持つ。証拠は Core IR への変換の単相化で決め、実行時の辞書は持たない (S5 で入れる) | [ロードマップ](future/roadmap.md) |

### 構文

構文は、次の3つを目標に設計した。

- 「線形型 × 代数的エフェクト」のプログラムが自然に読み書きできること
- 日常のスクリプト (テキスト・ファイル処理、シェル自動化、データ変換、ネットワーク・並行) が短く安全に書けること
- 文法に曖昧さがなく、rowan とイベント方式のパーサで素直に実装できること

| 領域 | 決定 | 詳細 |
|---|---|---|
| 全体の方針 | Haskell 流を土台にする。F# から、逐次実行 (`do` なしで行を並べる)、`\|>` のパイプライン、`match` / `handle` の `with` と `\|` の枝、`List.map` 形式のモジュール修飾を借りる | [文法](spec/grammar.md) |
| レイアウト | オフサイドルール。開始トークン (`=` `->` `with` `then` `else` `where`) で行が終わり、次の行が深いときだけブロックを開く | [レイアウト規則](spec/layout.md) |
| トップレベルのシグネチャ | Haskell / Idris と同じく、別の行に書く。必須であることは HIR で検査する。複数の等式で定義できる | [宣言](spec/declarations.md) |
| ラムダ | `fn x y -> e` | [式](spec/expressions.md) |
| `if` | then 節が `Unit` なら `else` を省略できる | [式](spec/expressions.md) |
| `match` / handler の枝 | `\|` を必須にし、字下げする。1行でも書ける。直和型の宣言も同じ形 | [式](spec/expressions.md)、[宣言](spec/declarations.md) |
| 型の宣言 | `data` は直和型、`type` は型の別名 | [宣言](spec/declarations.md) |
| エフェクトの宣言 | `effect Name a where` の後に操作を並べる。操作のカリー化を許す | [宣言](spec/declarations.md) |
| `use` | ブロックの残りを、最後の引数のラムダとして渡す糖衣構文 | [式](spec/expressions.md) |
| パラメータ付き handler | `handle e from init with`。状態を handler が持ち、節の最後の引数で受ける。状態のある handler では `k v st` で次の状態を渡す。状態があるかどうかは `from` の有無で決まる | [式](spec/expressions.md) |
| 線形値の受け渡し | 糖衣構文は今は入れない。入れるかどうかは S13 で決める。再束縛、`use`、パラメータ付き handler で吸収する | [式](spec/expressions.md) |
| 文字列 | 補間は `"\{x}"`。複数行の `"""`、raw の `r"..."` | [字句](spec/lexical.md) |
| コマンドリテラル | バッククォート。シェルを介さず、引数のリストを組む (Julia 方式) | [字句](spec/lexical.md) |
| fixity | ユーザーが宣言する。優先順位は整数 0〜9。CST では演算子の列を平たいまま持ち、HIR で組み直す | [宣言](spec/declarations.md)、[式](spec/expressions.md) |
| モジュール | 1ファイル = 1モジュール。`pub` で公開する。import は既定で修飾付き。標準ライブラリのモジュール (`Fs` など) は import なしで修飾付きで使える | [モジュールと名前解決](spec/modules.md) |
| 組み込みの宣言 | 修飾子 `extern` で組み込みの関数、型、エフェクトを宣言する (`pub extern println : String -> <IO> Unit`)。標準ライブラリはバイナリに埋め込んだ `std/` のツリーから読み、Prelude は `std/Prelude.em` である。ファイルの操作は標準ライブラリのモジュール `Fs` (`Fs.open` など) にある | [宣言](spec/declarations.md)、[モジュールと名前解決](spec/modules.md) |
| 並行処理 | 新しい構文は追加しない。エフェクトにより直接の形で書ける | [式](spec/expressions.md) |
| コメント | `--` と、入れ子にできる `{- -}` | [字句](spec/lexical.md) |

### コンパイラと実行系

処理系はバッチ型のパイプラインで、各段階を純粋な関数にし、後でクエリ化 (salsa など) できるようにしてある。実行系は、型付き Core IR (前向きの辺だけを持つ基本ブロックの列で、RC とエフェクトを明示する) を CEK 風のインタプリタで実行する。メモリは Perceus 方式の参照カウントで管理し、`Lin` の値を2回消費することはない。インタプリタはシングルスレッドで、`par` の子を順に実行する。子は決定的なので、順に実行しても意味は変わらない。並列化はネイティブのランタイムだけで行う。詳細は [コンパイラの構成](implementation/architecture.md)、[Core IR とインタプリタ](spec/core-ir.md)、[ランタイム](spec/runtime.md)、[マルチコア対応の設計](future/multicore.md) にある。

## 用語

| 用語 | 意味 |
|---|---|
| `Unr` / `Lin` | 型の線形性を表す Kind の要素。`Unr ≤ Lin`。`Lin` の値はちょうど1回使わなければならない |
| row | 関数が起こし得るエフェクトの並び。`<IO>` は閉じた row、`<IO \| e>` は row 変数 `e` で開いた row |
| `Row<s>` | row の Kind。`s ∈ {Never ≤ Once ≤ Multi}` は、その row に含まれてよい操作の上限を表す |
| 多重度 | 操作ごとに宣言する `never` / `once` / `multi`。handler 側の継続 `k` の有無と線形性 (`once` なら `Lin`、`multi` なら `Unr`、`never` には `k` がない) を決める |
| 持ち越し規則 | 操作の呼び出しをまたいで生きていてよい値の規則。`multi` の操作をまたげるのは `Unr` の値だけである |
| 中断 | `drop k` や `never` 操作によって、継続が再開されずに捨てられること。暗黙の後始末は中断時にだけ起きる |
| 破棄処理 | `Lin` 型に宣言された捨て方。標準ライブラリのリソース `Fs.File` では `Fs.close` |
| deep handler | 再開した継続の中でも同じ handler が有効なままの handler |
| パラメータ付き handler | `handle e from init with` の形で、状態を節の最後の引数として受け渡す handler。状態は handler フレームに置き、脱糖しない |
| レイアウト段 | lexer と parser の間で、仮想トークン `OPEN` / `SEP` / `CLOSE` (コードでは `LAYOUT_OPEN` / `LAYOUT_SEP` / `LAYOUT_CLOSE`) を挿入する段 |
| Perceus | 参照カウントの `dup` / `decref` / `release` を静的に挿入する方式。reuse analysis と借用の最適化は後で入れる |
| Core IR | 型付き HIR から変換する、前向きの辺だけを持つ基本ブロックの列の IR。すべての中間値に名前を付け、RC とエフェクトの命令を明示する |
| M1、M2 | 完了したマイルストーンである。M1 は言語の全体を一通り通した最初の vertical slice、M2 はモジュールと参照ごとの具体化の表である。今の範囲は [実装の現在地](implementation/status.md) にある |
| S0〜S13 | 再設計のサブプロジェクトである。S0 運用と文書、S1 row の健全性、S2a 継続を関数にする、S2b 組み込みを extern にする、S3a フロントエンドの土台、S3b-1 ランタイムとインタプリタの土台、S3b-2a Core IR v2 の構造、S3b-2b Core IR v2 の所有、S3b-2c-1 Core IR v2 の表、S3b-2c-2 Core IR v2 の境界、S4a 計測の基準、S4b 単相化、S5 型クラス、S6 リスト、文字列、レコード、S7 evidence passing、S8 LIR、S9 共通ランタイム、S10 VM と切り替え、S11 REPL、S12 スクリプトの MVP、S13 実例による判断がある。その後の言語の項目と処理系の項目は [ロードマップ](future/roadmap.md) の「段の列」にある。パイプラインの「段階」とは別の呼び方である |
| a1 / a2-wait / a2-cancel | 並列 API `par` を段階的に広げる計画の各段階。[マルチコア対応の設計](future/multicore.md) にある |
