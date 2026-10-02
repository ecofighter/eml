# eml 言語設計 spec (マイルストーン1: vertical slice)

- 日付: 2026-10-03
- 状態: レビュー待ち (暫定構文・テスト基盤の決定を反映した改訂版)
- 言語名: eml (effect と作者の GitHub ID `ecofighter` を掛けた名前)
- ソースファイルの拡張子: `.em`

## 1. 目的と成功条件

### 目的

- **主目的**: 言語デザインの実験。「線形型 × 代数的エフェクト」の組み合わせで新しい書き心地を探り、他の人にも使ってもらえる形にする
- **副目的**: 作者自身の日常スクリプト用途で実際に使う

### 言語の性格

- 関数型、式指向、イミュータブルがデフォルト
- 副作用は `IO` エフェクトとして管理する
- 線形型と代数的エフェクトを持つ
- 将来は LLVM でネイティブコンパイルする
- rowan (Red-Green Tree) を土台に、リッチな診断と LSP を整える

### マイルストーン1 の成功条件

1. 暫定構文で書いた eml プログラムを `eml run <file>` で実行できる
2. `eml check <file>` で、構文・名前解決・型・線形性・エフェクト・`match` の網羅性のエラーを、ソース位置を指す診断として表示できる。1回の実行で、独立した複数のエラーを報告できる
3. ユーザー定義のエフェクト (`never` / `once` / `multi`) と handler (`resume` と `drop k`) が動く
4. 線形なリソース (`File`) を扱うプログラムで、消費漏れ・二重使用・`multi` をまたぐ持ち越しをコンパイル時に検出できる
5. すべての実行テストで RC のリーク検出と解放済みアクセスの検出が有効になっており、通過している

## 2. 確定した設計判断

| 領域 | 決定 |
|---|---|
| 線形性 | Kind で区別する。`Type<m>`、`m ∈ {Unr ≤ Lin}`。Affine は持たず、値を捨てるときは組み込みキーワード `drop` を明示する |
| Kind の将来 | 内部では最初から Kind 変数と部分 Kind 関係を扱う。表面の構文では当面書かせず、将来ユーザーが Kind を書けるようにする |
| エフェクト | Row 多相 (Koka 方式、scoped labels)。row 変数にも Kind `Row<s>`、`s ∈ {Never ≤ Tail ≤ Once ≤ Multi}` を持たせる。`Never` と `Tail` は将来のマルチコア対応のための要素 (§4) |
| 継続の多重度 | 操作ごとに `never` / `once` / `multi` を宣言する。デフォルトは `once` |
| 継続の線形性 | `once` の継続 `k` は `Lin`。handler は `resume k v` か `drop k` を必ず書く。`multi` の継続は `Unr` |
| handler の意味 | deep handler。`resume` した継続の中でも同じ handler が有効なまま |
| `IO` | 組み込みのエフェクト。ユーザーは handle できない |
| 暗黙の後始末 | 通常の制御フローでは一切行わない。中断時 (`drop k` や `never` 操作) だけ、捕まっていた `Lin` 値を、その型に宣言された破棄処理で drop する |
| メモリ管理 | Perceus 方式の参照カウント。`Lin` 値は静的に一意なので RC 操作を付けない。RC は将来のマルチコア対応で共有の印方式にできる形にしておく (§5) |
| 型付け | Bidirectional Typing + 単一化。トップレベルの関数は引数と戻り値の型注釈が必須。row と Kind は推論する |
| 構文 | マイルストーン1 は §7 の暫定構文で進める。本番の構文は作者が別途手で設計する。暫定構文で迷ったときは Haskell の慣習に寄せる |
| コンパイラ構成 | バッチ型のパイプライン。各段階を純粋な関数とし、Arena と ID で表現して、後でクエリ化 (salsa など) できるようにしておく |
| 実行系 | 型付き Core IR (ANF 形式で、RC とエフェクトを明示する) + CEK 風のインタプリタ。将来の LLVM バックエンドも同じ IR から変換する。ヒープと RC は `eml_runtime` に分離する |
| マルチコア | マイルストーン1 では実装しない。将来の設計は [マルチコア対応の設計 spec](2026-10-03-eml-multicore-design.md) にまとめ、その §9 の予防的な決定だけをこの spec に反映する |

## 3. 全体構成

### リポジトリ

`eml` リポジトリを Cargo workspace に変える。

```
eml/
  Cargo.toml            # [workspace] members = ["crates/*"], resolver = "3"
  crates/
    eml_diagnostics/
    eml_syntax/
    eml_hir/
    eml_types/
    eml_core_ir/
    eml_runtime/
    eml_interp/
    eml_cli/            # lib + バイナリ (バイナリ名 `eml`)
  tests/ui/             # UI テストのコーパス (§8)
    run/
    check-fail/
  docs/superpowers/specs/
```

- 既存の `src/main.rs` / `src/lib.rs` は `eml_cli` に移す
- `[workspace.package]` で `edition = "2024"` などを共有し、外部 crate のバージョンは `[workspace.dependencies]` でまとめて管理する
- `flake.nix` の `buildRustPackage` は、workspace のルートの `Cargo.lock` でそのまま動く。必要に応じて `cargoBuildFlags = ["-p" "eml_cli"]` を足す。devShell に `clippy`、`rustfmt`、`cargo-insta` を追加する
- `.gitignore` に `.direnv/` を追加する

### crate の依存関係

依存は上から下への一方向だけにする。`eml_diagnostics` はすべての crate から使う。

```
eml_cli        run / check コマンド。各段階をつなぐだけ。テストから呼べる lib API を公開する
eml_interp     Core IR を CEK 機械で実行する
eml_runtime    オブジェクトのモデル、ヒープ、参照カウント、debug_heap の検査
eml_core_ir    型付き HIR → Core IR。dup/decref の挿入パス
eml_types      Kind・型・row の推論、線形性・多重度の検査、match の網羅性検査
eml_hir        CST → HIR の変換、名前解決
eml_syntax     SyntaxKind、lexer、イベント方式のパーサ、rowan、型付き AST ラッパ
eml_diagnostics  Diagnostic 型、FileId と SourceFiles、ariadne による表示
```

### 各段階の規律

- 各段階は `fn stage(input: &In) -> (Out, Vec<Diagnostic>)` の形の純粋な関数にする。グローバルな可変状態は持たない
- HIR 以降は `ExprId` / `PatId` / `DefId` などの ID で参照する (`la-arena`)。型などの解析結果は `ExprId → Type` のような**別テーブル**に置く
- HIR の各ノードは、元の構文ノードへのポインタ (`SyntaxNodePtr`) を持つ

### エラーが出ても止まらない

- パーサは、壊れた入力に対して `ERROR` ノードを作って処理を続ける。セミコロン、波括弧、トップレベルのキーワード (`fn` / `type` / `effect`) を回復の同期点にする
- 名前解決と型推論は、エラーが起きた場所に `Error` 型を入れる。`Error` が関わる制約や線形性の検査からは、追加の診断を出さない

### 構文を差し替えられるようにする

- パーサは rust-analyzer と同じイベント方式にする (ノード開始 / トークン / ノード終了のイベント → 別の処理で rowan の木を組み立てる)
- 構文の規則は `eml_syntax/src/grammar/` に閉じ込める。暫定構文から本番の構文への差し替えは、原則として `grammar/`、`SyntaxKind`、lexer、型付き AST ラッパの変更だけで済むようにする

### ソースファイルと位置

- `FileId` と `SourceFiles` (`FileId` → パスとテキスト) は `eml_diagnostics` に置く。マイルストーン1 は単一ファイルなので中身は実質1件
- `TextRange` は `text-size` crate を直接使う (`rowan` が再公開しているものと同じ型)。`eml_diagnostics` は `rowan` に依存しない
- 将来クエリ化するときは、`SourceFiles` を salsa の入力に置き換え、必要なら別の crate に切り出す

### CLI

- 引数の解析は `clap` (derive) を使う
- `eml check <file>`: 診断を stderr に表示する。`eml run <file>`: 検査を通ったら実行する
- `eml run --debug-heap`: RC のリーク検出と解放済みアクセスの検出を有効にする (§5)
- 終了コード: 0 = 成功、1 = 診断のエラーあり、または実行時エラー、2 = 使い方の誤り (引数の誤り、ファイルが読めない)
- `eml_cli` の lib は `check(files, file_id) -> Vec<Diagnostic>` と `run(files, file_id, &RunConfig, stdout: OutputSink) -> RunOutcome` を公開する。UI テストはこれをプロセス内で呼ぶ
- `OutputSink` は `Send + Sync` な共有の出力先の型とする (`Arc<Mutex<dyn Write + Send>>` を包む型など)。将来、複数のスレッドから `println` するため。テストでは出力を捕まえられるバッファを渡す
- `RunConfig` は `Default` を実装し、フィールドを後から足せるようにする (将来 `threads` や `schedule_seed` を足す)

### 主な外部 crate

`rowan`、`logos`、`la-arena`、`text-size`、`ariadne`、`clap`、`insta` (`glob` 機能)

## 4. 型システム

### Kind

```
κ ::= Type<m> | Row<s> | κ → κ
m ::= Unr | Lin | μ          (μ は線形性の Kind 変数)
s ::= Never | Tail | Once | Multi | σ       (σ は多重度の Kind 変数)
Unr ≤ Lin,  Never ≤ Tail ≤ Once ≤ Multi
```

- `Row<s>` の `s` は、その row に含まれてよい操作の上限を表す。`Never` は `never` の操作だけを含んでよい。`Tail` はさらに、すぐに再開する操作 (組み込みの `IO`) まで含んでよい。`Once` はさらに `once` の操作まで、`Multi` はさらに `multi` の操作まで含んでよい
- `Never` と `Tail` は、将来のマルチコア対応 (並列タスクに許す row の制限) のために、内部の束に最初から用意しておく。後から束の下に要素を足すと、解き方や既存の制約の意味が変わるため。表面の構文で `tail` の操作を宣言する機能は、マイルストーン1 には入れない ([マルチコア対応の設計 spec](2026-10-03-eml-multicore-design.md) §6)

- データ型の Kind は、フィールドの Kind の上限 (join) で推論する
- 組み込みのリソース型 (`File`) は `Lin` で、破棄処理 (`close`) を持つ

### 関数型

- 内部表現は `a -[m]-> <e> b`。`m` は関数値自体の線形性、`e` はエフェクトの row
- 表面の構文では `m` を書かない (`A -> <E> B`)。`m` は Kind 変数として推論する
- `Lin` の値を捕まえたクロージャは `Lin` になる (Rust の `FnOnce` に相当)

### 推論 (Bidirectional Typing)

- 型付けは check モード (期待する型を上から渡す) と infer モード (型を下から求める) を持つ bidirectional 方式にする。注釈のないローカルの `let` やラムダ式の引数には推論用の型変数を使い、単一化で解く
- **トップレベルの関数は、引数と戻り値の型注釈が必須**。値の型はシグネチャで完全に決まるので、シグネチャの型変数は本体の中では rigid (固定された型) として扱う。値の型の多相化 (let 多相) は行わない。ローカルの `let` は単相
- シグネチャの中で省略した row (戻り値の row、引数の関数型の row) には、推論用の row 変数を入れる。row を明示した場合はその row に固定する (`<IO>` は閉じた row、開くときは `<IO | e>`)
- row と Kind はシグネチャから決まらないので推論する。row の推論は呼び出し関係に依存するため、トップレベルの関数を呼び出しグラフの強連結成分 (SCC) ごとにまとめて推論し、推論後に残った row 変数と Kind 変数を多相化する
  - 例: `fn map (f: a -> b) (xs: List a) : List b` は `(a -> <e> b) -> List a -> <e> List b` と推論される
  - 例: `fn dup (x: a) : (a, a)` は、本体で `x` を2回使うので `a : Type<Unr>` という Kind 制約がシグネチャに加わる
- row は Leijen の scoped labels 方式で単一化する
- Kind の制約 (`m₁ ≤ m₂` など) は、束の上で最小の解を求める。相互再帰する関数では SCC 内で不動点を求める
- 型の明示 `(e : T)` は、`e` を check モードで `T` に対して検査する
- 制約には必ず**由来** (引数の位置、`if` の各枝、型注釈など) を記録する。診断で使うため
- エントリポイントは `main : Unit -> <IO> Unit`。推論した `main` の row に `IO` 以外のエフェクトが残っていればエラーにする (handle されていないエフェクト)

### 線形性と `drop`

- `Lin` の値は、ちょうど1回使わなければならない。`drop x` も1回の使用に数える
- すべての `Lin` 型は捨て方を持つ。組み込みリソースは宣言された破棄処理を、複合型は各フィールドの再帰的な drop を、クロージャと継続は捕まえた値の drop を行う
- `Lin` の値をワイルドカードのパターン `_` で受けることはエラーとする (捨てるときは `drop` を明示する)
- 型の Kind が Kind 変数 (例えばシグネチャの型変数 `a`) の場合、その型の値を2回以上使う、使わない、`_` で受ける、のいずれかがあれば、エラーではなく `Unr` の制約を加える。制約が `Lin` と矛盾したときにエラーにする
- 式文 `e;` は `e : Unit` を要求する。`Lin` の値を式文で捨てることは型エラーとして検出される
- 「捨てることも許さない使い切り必須の型」は、マイルストーン1 では表現しない (§10 の将来の拡張を参照)

### エフェクト操作の多重度と持ち越し規則

| 操作の宣言 | handler 側の `k` | 呼び出しをまたいで生きていてよい値 |
|---|---|---|
| `never` | なし | `Unr`, `Lin` (中断時に drop される) |
| `once` (デフォルト) | `Lin`: `resume k v` か `drop k` を必ず書く | `Unr`, `Lin` (同上) |
| `multi` | `Unr` | `Unr` のみ |

- 呼び出しをまたいで `Lin` 変数が生きている場合、その呼び出しの row について次のように扱う。この検査は各呼び出し箇所で局所的に行えば、全体として健全になる
  - row に `multi` な操作が含まれていれば、エラーにする
  - row 変数 `e : Row<σ>` を含む場合は、制約 `σ ≤ Once` を追加する。その結果、`e` に `multi` な操作が入るような呼び出し方は、制約違反として検出される (束が4要素なので、`σ = Once` ではなく `≤` で書く)
- `never` の操作の結果の型は、宣言で自由な型変数として書く (`never raise : String -> a`)。呼び出した側ではどんな型として使ってもよい
- 組み込みの `IO` の操作は、実行時が必ずすぐにちょうど1回再開するので、`Tail` に分類する。持ち越し規則の上では `once` と同じく、`Lin` の値が呼び出しをまたいで生きていてよい。ただし中断は起こらない

### handler

- deep handler とする。`resume k v` で再開した継続の中で同じ操作が呼ばれたら、同じ handler が処理する
- 操作の節の引数の個数は、`never` の操作なら 1 (引数のパターン)、それ以外なら 2 (引数のパターンと `k`)。個数は名前解決の段階で検査する
- `return x => e` の節は省略でき、省略したら `return x => x` とみなす
- `IO` の操作に対する節を書いたらエラーにする

### 線形性の検査パス

- 推論の後に、型付き HIR の上で別のパスとして行う
- 制御フローに沿って変数の使用回数を数える。分岐の各枝で消費する `Lin` 変数の集合が一致していることを確認する
- 呼び出しの row と、その時点で生きている変数を照らし合わせる (上の表の規則)
- handler 節の中で、`once` の `k` が必ずちょうど1回 `resume` または `drop` されていることを確認する
- 出力: perform し得る各呼び出しについて、「その時点で生きている `Lin` 変数の集合」(中断時のクリーンアップ情報)

### 網羅性の検査パス

- 推論の後に、型付き HIR の上で別のパスとして行う (Maranget の行列アルゴリズム)
- 網羅されていない `match` はエラー、到達しない枝は Warning にする
- Int と String のリテラルは無限に値があるものとして扱い、ワイルドカードか変数の枝がない限り網羅されていないとみなす
- `let` の左辺とトップレベル・ラムダの引数のパターンも、反駁可能 (網羅されていない) ならエラーにする

## 5. Core IR とインタプリタ

### Core IR

型付き HIR から変換する ANF 形式の IR。すべての中間値に名前を付ける。

| 種類 | 命令 |
|---|---|
| 値 | `let x = 呼び出し / プリミティブ / コンストラクタ / クロージャ生成`、`match`、`return` |
| RC (`Unr` のヒープ値のみ) | `dup x`、`decref x` |
| 線形値 | `drop x` (宣言された破棄処理を呼ぶ) |
| エフェクト | `perform op args`、`handle body { ops; return }`、`resume k v`、`drop k` |

- 表面の構文には `perform` はないが、HIR から Core IR への変換で、操作の呼び出しを `perform` 命令に変換する
- 型の情報はほぼ消去する。各変数の Kind (`Unr` / `Lin`) とボックス化の有無は残す
- perform し得る各呼び出しに、線形性の検査パスが出したクリーンアップ情報を付ける
- マイルストーン1 で入れるパスは、Perceus の `dup` / `decref` の挿入だけにする。reuse analysis と借用パラメータの最適化は後で追加する

### インタプリタ (CEK 機械)

- 継続は、ヒープ上のイミュータブルなフレームの連結リストで表す。handler フレームはその中の目印になる
- `perform` は、連結リストを遡って対応する handler を探す。perform した場所から handler までのフレームへのポインタを集めたものが継続になる。`multi` の再開は、その列をもう一度積むだけで行える
- `drop k` は、継続の各フレームのクリーンアップ情報に従って、生きている `Lin` 変数を drop する
- `IO` は、連結リストの最下部にある組み込みの handler として Rust で実装する。標準出力は `run` に渡された `OutputSink` に書く (テストで出力を捕まえるため)

### ヒープと参照カウント (`eml_runtime`)

- Rust の `Rc` は使わず、自前のヒープと参照カウントを `eml_runtime` に持つ。`dup` / `decref` 命令が実際にカウントを増減する
- ヒープはインデックス方式のアリーナと世代番号で実装する。解放済みのオブジェクトへのアクセスは、世代番号の不一致で検出する
- ランタイムの API (確保、`dup` / `decref`、フィールドの読み出し、一意かどうかの判定) は、マルチコア版と同じものに固定する。共有の印付け `mark_shared` は API に名前だけ予約し、実装はマルチコアの段階で行う。マルチコア化では `eml_runtime` の内部だけを差し替える
- オブジェクトのヘッダの RC は符号付きの `AtomicI32` とする。正なら局所、負なら共有を表す。マイルストーン1 では常に正
- ヘッダから記述子 (フィールドのレイアウトと `Lin` の破棄処理) を引けるようにする。`decref` で子のオブジェクトをたどるときや `drop k` の後始末で、静的な型の情報なしに処理するため
- 継続のフレームと環境も、同じ RC で管理するランタイムのオブジェクトにする
- インタプリタの値とフレームに `Rc` と `RefCell` を使わない。Core IR は `Arc<Program>` で読み取り専用で共有する。将来、複数のスレッドがそれぞれの CEK 機械で同じプログラムを実行するため
- `RunConfig` の `debug_heap` を有効にすると、解放済みのオブジェクトへのアクセスを検出し、プログラムの終了時にすべてのオブジェクトが解放されていることを確認する (リーク検出)。CLI では `--debug-heap`、UI テストでは常に有効にする

## 6. 診断

### データ構造

```rust
struct Diagnostic {
    code: ErrorCode,          // E0001 などの安定した番号
    severity: Severity,       // Error / Warning / Note
    message: String,
    primary: Label,           // (FileId, TextRange, ラベル文)
    secondary: Vec<Label>,
    notes: Vec<String>,
    help: Vec<String>,
    fix: Option<Vec<TextEdit>>,
}
```

- CLI では `ariadne` で表示する。将来の LSP では、同じ構造体から LSP の診断と code action に変換する
- `ErrorCode` は段階ごとに番号の範囲を分ける

| 範囲 | 段階 |
|---|---|
| E0xxx | 字句・構文 |
| E1xxx | 名前解決 (重複定義、未定義の名前、handler 節の引数の個数) |
| E2xxx | 型・Kind・row |
| E3xxx | 線形性・継続の多重度 |
| E4xxx | パターンの網羅性 |

### 型エラー

制約の由来をもとに、「expected / found」と、その根拠になった場所を示すラベルを出す。

### 線形性の診断

| 誤り | 指す場所 |
|---|---|
| 二重使用 | 1回目に消費した場所と、2回目に使った場所 |
| 消費されていない | 束縛した場所とスコープの終わり。help と fix で `drop x` の追加を提案する |
| `_` で `Lin` の値を受けた | そのパターン。help で変数に束縛して `drop` するよう提案する |
| `multi` の呼び出しをまたぐ | 線形な変数、その呼び出し、`multi` と宣言している操作 |
| 継続の扱い忘れ | `k` に `resume` も `drop` もしていない handler 節 |

### 網羅性の診断

| 誤り | 指す場所 |
|---|---|
| 網羅されていない `match` | `match` 式。漏れているパターンの例を note で示し、fix で枝の追加を提案する |
| 到達しない枝 (Warning) | その枝のパターン |
| 反駁可能な `let` / 引数のパターン | そのパターン |

## 7. マイルストーン1 の言語機能

### 含めるもの

- 基本型: `Int`、`Bool`、`String`、`Unit`
- タプル、ユーザー定義の代数的データ型 (直和と直積) と `match` (網羅性の検査つき)
- トップレベルの関数 (引数と戻り値の型注釈は必須)、ローカルの `let`、`if`、再帰、クロージャ、型の明示 `(e : T)`
- `drop` キーワード
- エフェクトの宣言 (`never` / `once` / `multi`)、`handle` (deep)、`resume`、`drop k`
- 組み込みの `IO` エフェクト: `println : String -> <IO> Unit`、ファイル操作 (`open : String -> <IO> File`、`read_all : File -> <IO> (File, String)`、`close : File -> <IO> Unit`)
- 組み込みの線形型 `File`

### 含めないもの

- 本番の構文 (作者が別途設計する)
- LSP、フォーマッタ、REPL
- LLVM バックエンド
- 型クラスやトレイトなどのアドホック多相
- モジュールシステム (マイルストーン1 は単一ファイル)
- 可変参照 `Ref`
- Perceus の reuse analysis と借用の最適化
- 表面構文でのユーザーによる Kind の記述 (関数型の線形性 `m` を含む)
- `IO` をユーザーが handle すること
- レコード、or パターン、ガード、as パターン
- 浮動小数、文字型、16進リテラル、ブロックコメント

### 暫定構文

マイルストーン1 の実装とテストのための構文。作者が設計する本番の構文に置き換える前提で、パースしやすく曖昧さがないことを優先する。迷ったときは Haskell の慣習に寄せる。構文の規則は `eml_syntax/src/grammar/` に閉じ込める。

#### 字句

- 空白と改行には意味を持たせない (レイアウト規則なし)
- コメントは `//` から行末までの行コメントだけ
- 識別子
  - 小文字識別子 `[a-z][A-Za-z0-9_]*`、または `_` で始まり後に1文字以上続くもの: 変数、関数、型変数、row 変数、操作名
  - 大文字識別子 `[A-Z][A-Za-z0-9_]*`: 型名、コンストラクタ、エフェクト名
  - `_` 単独: ワイルドカード (専用のトークン)
  - 「大文字 = 具体的なもの / 小文字 = 変数」の区別により、パターンの `Some x` と `x`、row の `<IO>` と `<e>` を構文だけで区別できる
- リテラル
  - Int: 10進の `[0-9]+`。負の数は単項の `-` で書く
  - String: `"..."`。エスケープは `\n` `\t` `\\` `\"` だけ
  - Bool: `true` / `false`
- キーワード: `fn let if then else match type effect handle resume drop return never once multi true false`
- 記号: `( ) { } , ; : = -> => | < >` と演算子 `+ - * / % ++ == != < <= > >= && || !`

#### 文法

```
file        ::= item*
item        ::= fn_item | type_item | effect_item

fn_item     ::= 'fn' LIDENT fn_param+ ':' row? type block
fn_param    ::= '(' pat ':' type ')' | '(' ')'
type_item   ::= 'type' UIDENT LIDENT* '{' list(ctor) '}'
ctor        ::= UIDENT type_atom*
effect_item ::= 'effect' UIDENT LIDENT* '{' list(op_decl) '}'
op_decl     ::= ('never' | 'once' | 'multi')? LIDENT ':' type

block       ::= '{' (stmt ';')* expr? '}'
stmt        ::= 'let' pat (':' type)? '=' expr | expr

expr        ::= 'if' expr 'then' expr 'else' expr
              | 'match' expr '{' list(arm) '}'
              | 'handle' expr '{' list(clause) '}'
              | 'fn' lam_param+ block
              | block
              | binary
lam_param   ::= pat_atom | '(' pat ':' type ')'
arm         ::= pat '=>' expr
clause      ::= 'return' pat '=>' expr
              | LIDENT pat_atom+ '=>' expr

binary      ::= 演算子の優先順位表に従う (下記)。項は unary
unary       ::= ('-' | '!') unary | app
app         ::= 'drop' atom
              | 'resume' atom atom
              | atom atom*
atom        ::= INT | STRING | 'true' | 'false' | LIDENT | UIDENT
              | '(' ')' | '(' expr ')' | '(' expr ':' type ')'
              | '(' expr (',' expr)+ ','? ')'

pat         ::= UIDENT pat_atom+ | pat_atom
pat_atom    ::= '_' | LIDENT | UIDENT | INT | '-' INT | STRING | 'true' | 'false'
              | '(' ')' | '(' pat ')' | '(' pat (',' pat)+ ','? ')'

type        ::= type_app ('->' row? type)?
type_app    ::= UIDENT type_atom* | type_atom
type_atom   ::= UIDENT | LIDENT | '(' type ')' | '(' type (',' type)+ ')'
row         ::= '<' '>' | '<' LIDENT '>' | '<' effect (',' effect)* ('|' LIDENT)? '>'
effect      ::= UIDENT type_atom*

list(x)     ::= (x (',' x)* ','?)?
```

演算子の優先順位 (低い順):

| 優先順位 | 演算子 | 結合 |
|---|---|---|
| 1 | `\|\|` | 左 |
| 2 | `&&` | 左 |
| 3 | `==` `!=` `<` `<=` `>` `>=` | 結合しない |
| 4 | `++` | 右 |
| 5 | `+` `-` | 左 |
| 6 | `*` `/` `%` | 左 |
| 7 | 単項の `-` `!` | 前置 |
| 8 | 関数適用 (並置)、`drop`、`resume` | 左 |

#### 構文上の規則

- 関数はカリー化する。`fn add (x: Int) (y: Int) : Int` は `Int -> Int -> Int`。部分適用はクロージャになる
- 引数のない関数は `()` を引数に取る。トップレベルの `()` 引数は型注釈なしで書ける (型は `Unit`)
- 本体は必ずブロック。ブロックの最後の式がブロックの値で、なければ `Unit`
- ブロックの中の文は必ず `;` で終える。`if`、`match`、`handle` を文として書く場合も省略できない
- 中括弧の中に宣言や枝を並べるときはカンマ (`type`、`effect`、`match`、`handle`)、ブロックの中の文はセミコロンで区切る
- `if` は `else` が必須。`else` の枝はできるだけ遠くまで伸びる (`if c then 1 else 2 + 3` は `else (2 + 3)`)
- ブロック、`if`、`match`、`handle`、ラムダ式は atom ではない。関数適用の引数や演算の項にするときは括弧で囲む。これにより `match e {` や `handle e {` の `e` は `{` の手前で終わる
- `f -1` は `f - 1` と解釈する。負の数を渡すときは `f (-1)`
- 比較演算子は結合しない (`a < b < c` はエラー)
- ラムダ式はトップレベルの関数と同じ `fn` で書く。式の位置ならラムダ式、トップレベルなら宣言
- `let` による同じ名前の再束縛 (シャドーイング) を許す。隠された変数が `Lin` でまだ消費されていなければ、消費漏れとして診断する
- エフェクトの操作は普通の関数として呼ぶ (`get ()`)。操作名は値の名前空間に置き、トップレベルの関数名や他の操作名と重なったら重複エラー
- エフェクトの操作のシグネチャの一番外側の `->` には row を書けない (書いたらエラー)
- 型の位置での `<` は row の開始だけを意味する
- `A -> <E> B -> C` の row `<E>` は最初の `->` に付き、`A -> <E> (B -> C)` と読む。内側の `B -> C` の row は省略扱い
- エフェクト名と型名は同じ名前空間に置く
- Unit の型は `Unit` とだけ書く (値とパターンでは `()`)

#### 例

```
type List a { Nil, Cons a (List a) }

effect State s {
  get : Unit -> s,
  put : s -> Unit,
}

effect Exn {
  never raise : String -> a,
}

fn len (xs: List a) : Int {
  match xs {
    Nil => 0,
    Cons _ rest => 1 + len rest,
  }
}

fn run_state (init: s) (action: Unit -> <State s | e> a) : <e> a {
  let f = handle action () {
    get () k => fn st { (resume k st) st },
    put st2 k => fn _st { (resume k ()) st2 },
    return x => fn _st { x },
  };
  f init
}

fn counter () : Int {
  let n = get ();
  put (n + 1);
  n
}

fn main () : <IO> Unit {
  let f = open "input.txt";
  let (f, contents) = read_all f;
  close f;
  let n = run_state 10 counter;
  if n == 10 then println contents else println "unexpected"
}
```

## 8. テスト戦略

TDD で進める。テストを先に書き、実装をテストに合わせる。

| 層 | 方法 |
|---|---|
| 字句 | トークン列のスナップショット |
| パーサ | `insta` で CST をダンプしたスナップショット。壊れた入力から回復できるかのケースを多めに用意する |
| 型推論 | 推論したシグネチャ (Kind と row を含む) のスナップショット |
| 診断 | 表示した診断テキストのスナップショット |
| 線形性 | 不正なプログラム (二重使用、消費漏れ、`_` での破棄、`multi` をまたぐ、継続の扱い忘れ) と、正しく通るべきプログラムの対 |
| 網羅性 | 網羅されていない `match`、到達しない枝、反駁可能な `let` |
| 全体 (UI テスト) | 下記 |
| CLI | `eml run` / `eml check` の終了コードと引数の誤りを数件確認する |
| RC | すべての実行テストで `debug_heap` を有効にする。違反があればテストを失敗させる |

### UI テスト

- `tests/ui/run/*.em`: 診断のエラーなしで実行が正常に終了することを確認し、stdout と stderr をスナップショットにする
- `tests/ui/check-fail/*.em`: 診断のエラーが1件以上出ることを確認し、診断の表示をスナップショットにする
- テストは `crates/eml_cli/tests/ui.rs` に置き、`insta::glob!` で `tests/ui/` 以下の `.em` を走査する。`eml_cli` の lib API をプロセス内で呼び、`RunConfig { debug_heap: true }` で実行する
- 成功すべきか失敗すべきかはディレクトリで決めるので、スナップショットの承認を誤っても、成功と失敗の入れ替わりは検出できる
- スナップショットの更新は `cargo insta review` で行う

### テストの変更に関する合意済みの例外

暫定構文で書いたテストのソースは、本番の構文に差し替えるときに書き直す。これは構文の差し替えに伴う機械的な書き換えとして、事前に合意した例外とする。テストの期待値 (意味) は変えない。

## 9. 実装の順序 (最初の段階)

最初の段階として、次の範囲のスケルトンを作る。

- 8 つの crate と workspace の設定、flake の更新
- 各段階を `fn stage(&In) -> (Out, Vec<Diagnostic>)` の形の仮実装でつなぎ、空のファイルに対する `eml check` が診断 0 件で終了する
- UI テストの仕組みと `insta` のスナップショットが動く
- `eml_syntax` の土台: §7 の字句に従う `SyntaxKind` と logos による lexer、イベント方式のパーサの骨組み (イベント列 → rowan の木の組み立て、エラー回復の仕組み)

文法規則の全体の実装、名前解決以降の各段階の中身は、後の段階で TDD で進める。

## 10. 後回しにした論点と将来の拡張

- **可変参照 (方針は決定済み)**: 値 `Ref h a` と組み込みのエフェクト `Heap h` を持ち、`IO` と分離する。`run_heap` は rank-2 の組み込みとして特別扱いする。`Ref` には `Unr` の値だけを入れ、multi-shot で再開したときの状態は共有する。詳細は [マルチコア対応の設計 spec](2026-10-03-eml-multicore-design.md) §4
- **マルチコア対応**: 共有の印方式の RC、並列の `par` の段階的な拡張 (a1 → a2-wait → a2-cancel → a4)、`tail` の操作、並行処理の `spawn`、継続の移動、マルチコアのインタプリタ。[マルチコア対応の設計 spec](2026-10-03-eml-multicore-design.md) を参照
- **使い切り必須の型**: 捨てることを許さない線形型。ユーザーが Kind を書けるようにする段階で、Kind に「drop できるか」の次元を追加して扱う
- **row 要素ごとの捕捉 Kind**: `<IO, Async@lin>` のように、row の各要素に「捕まえてよい Kind」を推論で持たせ、`never` / `once` の持ち越し規則をより柔軟にする
- **ユーザーによる Kind の記述**: 内部の Kind 変数を表面の構文に開放する
- **アドホック多相**: 型クラスやトレイト
- **モジュールシステムと標準ライブラリの範囲**
- **インクリメンタル化**: 各段階をクエリ (salsa など) に載せ替える
- **LSP**: 診断、hover (型と row の表示)、補完、code action
- **ネイティブ化**: 同じ Core IR から LLVM へ変換する。`never` / `once` はスタック切り替え、`multi` は選択的な CPS 変換かスタックのコピーで実装する。evidence passing を検討する
- **Perceus の最適化**: reuse analysis (FBIP)、借用パラメータ
