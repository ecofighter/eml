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
- 将来は LLVM でネイティブコンパイルする。
- rowan (Red-Green Tree) の上に、リッチな診断と LSP を整える。

## 確定した設計判断

### 型システム

| 領域 | 決定 | 詳細 |
|---|---|---|
| 線形性 | Kind で区別する。`Type<m>`、`m ∈ {Unr ≤ Lin}`。Affine は持たず、値を捨てるときは組み込みキーワード `drop` を明示する | [型と Kind](spec/types.md)、[線形性](spec/linearity.md) |
| Kind の将来 | 内部では最初から Kind 変数と部分 Kind 関係を扱う。表面の構文では当面書かせず、将来ユーザーが Kind を書けるようにする | [型と Kind](spec/types.md) |
| エフェクト | Row 多相 (Koka 方式、scoped labels)。row 変数にも Kind `Row<s>`、`s ∈ {Never ≤ Once ≤ Multi}` を持たせる。`Never` は将来のマルチコア対応のための要素 | [型と Kind](spec/types.md)、[エフェクトと handler](spec/effects.md) |
| 継続の多重度 | 操作ごとに `never` / `once` / `multi` を宣言する。デフォルトは `once` | [エフェクトと handler](spec/effects.md) |
| 継続の線形性 | `once` の継続 `k` は `Lin`。handler は `resume k v` か `drop k` を必ず書く。`multi` の継続は `Unr` | [エフェクトと handler](spec/effects.md) |
| handler の意味 | deep handler。`resume` した継続の中でも同じ handler が有効なまま | [エフェクトと handler](spec/effects.md) |
| `IO` | 組み込みのエフェクト。ユーザーは handle できない | [エフェクトと handler](spec/effects.md) |
| 暗黙の後始末 | 通常の制御フローでは一切行わない。中断時 (`drop k` や `never` 操作) だけ、捕まっていた `Lin` 値を、その型に宣言された破棄処理で drop する | [線形性](spec/linearity.md)、[エフェクトと handler](spec/effects.md) |
| 型付け | Bidirectional Typing + 単一化。トップレベルの関数は引数と戻り値の型注釈が必須。トップレベルの関数の row はシグネチャで決まり、省略した row は `<>` (純粋) である。Kind は推論する | [型と Kind](spec/types.md) |
| 直積型 | SML 方式。タプルは数字ラベルのレコード、Unit は `{}`。構造的な row 多相のレコード。Kind はフィールドの Kind の join で推論する | [直積型とレコード](spec/records.md) |

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
| パラメータ付き handler | `handle e from init with`。状態を節の最後の引数で受ける糖衣構文 | [式](spec/expressions.md) |
| 線形値の受け渡し | 糖衣構文は入れない。再束縛、`use`、パラメータ付き handler で吸収する | [式](spec/expressions.md) |
| 文字列 | 補間は `"\{x}"`。複数行の `"""`、raw の `r"..."` | [字句](spec/lexical.md) |
| コマンドリテラル | バッククォート。シェルを介さず、引数のリストを組む (Julia 方式) | [字句](spec/lexical.md) |
| fixity | ユーザーが宣言する。優先順位は整数 0〜9。CST では演算子の列を平たいまま持ち、HIR で組み直す | [宣言](spec/declarations.md)、[式](spec/expressions.md) |
| モジュール | 1ファイル = 1モジュール。`pub` で公開する。import は既定で修飾付き。標準ライブラリは import なしで修飾付きで使える | [モジュールと名前解決](spec/modules.md) |
| 並行処理 | 新しい構文は追加しない。エフェクトにより直接の形で書ける | [式](spec/expressions.md) |
| コメント | `--` と、入れ子にできる `{- -}` | [字句](spec/lexical.md) |

### コンパイラと実行系

| 領域 | 決定 | 詳細 |
|---|---|---|
| コンパイラ構成 | バッチ型のパイプライン。各段階を純粋な関数とし、Arena と ID で表現して、後でクエリ化 (salsa など) できるようにしておく | [コンパイラの構成](implementation/architecture.md) |
| 実行系 | 型付き Core IR (ANF 形式で、RC とエフェクトを明示する) + CEK 風のインタプリタ。将来の LLVM バックエンドも同じ IR から変換する。ヒープと RC は `eml_runtime` に分離する | [Core IR とインタプリタ](spec/core-ir.md)、[ランタイム](spec/runtime.md) |
| メモリ管理 | Perceus 方式の参照カウント。`Lin` 値は静的に一意なので RC 操作を付けない。RC は将来のマルチコア対応で共有の印方式にできる形にしておく | [ランタイム](spec/runtime.md) |
| マルチコア | マイルストーン1 では実装しない。将来の設計はマルチコア対応の設計にまとめ、予防的な決定だけをマイルストーン1 に入れる | [マルチコア対応の設計](future/multicore.md)、[ランタイム](spec/runtime.md) |

## 用語

| 用語 | 意味 |
|---|---|
| `Unr` / `Lin` | 型の線形性を表す Kind の要素。`Unr ≤ Lin`。`Lin` の値はちょうど1回使わなければならない |
| row | 関数が起こし得るエフェクトの並び。`<IO>` は閉じた row、`<IO \| e>` は row 変数 `e` で開いた row |
| `Row<s>` | row の Kind。`s ∈ {Never ≤ Once ≤ Multi}` は、その row に含まれてよい操作の上限を表す |
| 多重度 | 操作ごとに宣言する `never` / `once` / `multi`。handler 側の継続 `k` の有無と線形性 (`once` なら `Lin`、`multi` なら `Unr`、`never` には `k` がない) を決める |
| 持ち越し規則 | 操作の呼び出しをまたいで生きていてよい値の規則。`multi` の操作をまたげるのは `Unr` の値だけである |
| 中断 | `drop k` や `never` 操作によって、継続が再開されずに捨てられること。暗黙の後始末は中断時にだけ起きる |
| 破棄処理 | `Lin` 型に宣言された捨て方。組み込みリソース `File` では `close` |
| deep handler | `resume` した継続の中でも同じ handler が有効なままの handler |
| パラメータ付き handler | `handle e from init with` の形で、状態を節の最後の引数として受け渡す handler。関数を返す handler に脱糖する |
| レイアウト段 | lexer と parser の間で、仮想トークン `OPEN` / `SEP` / `CLOSE` (コードでは `LAYOUT_OPEN` / `LAYOUT_SEP` / `LAYOUT_CLOSE`) を挿入する段 |
| Perceus | 参照カウントの `dup` / `decref` を静的に挿入する方式。reuse analysis と借用の最適化は後で入れる |
| Core IR | 型付き HIR から変換する ANF 形式の IR。RC とエフェクトの命令を明示する |
| マイルストーン1 (M1) | 最初の vertical slice。成功条件と範囲は [実装の現在地](implementation/status.md) にある |
| 暫定構文 | 本番の構文を決める前に、最初の実装とテストのために使っていた仮の構文。S1 で本番の構文に置き換えて廃止した |
| S1 / S2 / S3 | 本番の構文を実装する段階。S1 は M1 の機能の文法、S2 はレコードやモジュール、S3 はコマンドリテラル |
| a1 / a2-wait / a2-cancel | 並列 API `par` を段階的に広げる計画の各段階。[マルチコア対応の設計](future/multicore.md) にある |
