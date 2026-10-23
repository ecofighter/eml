# コマンドリテラル

- 作成日: 2026-10-23
- 基点: 段「UTF-8 と標準ライブラリ」を閉じた後の main (計画は `docs/plans/2026-10-23-utf8-stdlib.md`)。ファイルの行番号は main @ 2319493b で読んだもので、前の段の後では動いている。行番号は探す手がかりにとどめ、関数や節の名前で引く
- 状態: 計画は確定。決めていない細部は 10 章に既定とともに置く

この計画は docs/future/roadmap.md の「コマンドリテラル」の段の設計と計画である。roadmap の「進め方」に従い、この文書を段の最初のコミットで入れ、段を閉じるときに消す。コードと `docs/` はこの文書を引かない。

## 1. 目的と成功の基準

- 目的
  - コマンドリテラル `` `git log -n \{n}` `` に、HIR から先の意味を与える。今は HIR が E0004 を出している。
  - 標準ライブラリの `Cmd` と `Proc` を、引数のリストを組んでシェルを介さずに起動する API にする。
  - 起動の形は3つある。標準入出力を受け継ぐ形、出力を取る形、pipe でやりとりする `Child` の形である。
- 成功の基準 (確かめ方は 9 章)
  - E0004 のコマンドリテラルの枝がなくなる。
  - `Proc.run "sh" [...]` の形がリポジトリから消える。
  - `tests/ui/run/scripts/deploy` の出力が変わらない。
  - 全体の検証のコマンドがすべて通る。

## 2. 範囲

- 含むもの
  - 字句の `'...'` の引用 (D-1)
  - 引数への分け方と穴の型 (D-2)
  - Prelude の `Cmd` と脱糖 (D-3)
  - `Std.Proc` の API (D-4)
  - `Child` と pipe の規則 (D-5)
  - Host と `eml_rt` の `spawn` (D-10)
  - 今の `Proc.run` と `status` の削除
  - テストとスクリプトの書き直し
  - 段を閉じる作業
- 含まないもの: D-11 の後に回す項目。パイプ `.|`、ファイルへのリダイレクト、文字列からの標準入力、標準エラーの別取り、環境変数の削除、`Child` の標準エラーの pipe がこれにあたる。

## 3. 背景

- 字句と CST はある。
  - `Command` モードの lexer が `CMD_START`、`CMD_TEXT`、`ESCAPE`、穴、`CMD_END` を出す (docs/spec/lexical.md の「コマンドリテラル」)。
  - `'` の引用はまだない。今あるコマンドリテラルで `'` を含むものはない (tests/ui、docs/spec/examples.md、std を grep した)。
  - コマンドリテラルを使うテストは `tests/ui/check-fail/not-yet-supported/command_literal.em` の1本だけである。
- HIR は `crates/eml_hir/src/lower/expr.rs:324-327` で E0004 を出す。
- lexical.md:196 の今の規範は穴を「`\{x}` (`x : String`)」としている。
- `Std.Proc` の今の形
  - `run : String -> List String -> <IO, Panic> Int`
  - extern `status` (引数を NUL で区切った1つの文字列)
  - `Host::run` は `Command::status` で待つ。`CaptureHost` は `output()` で捕まえる (`crates/eml_rt/src/host.rs`、`crates/eml_vm/src/hosts.rs`)。
  - 依存の鎖は `ProcStatus` → `Ctx::proc_status` → `host_run` → `Host::run` で、`trust.rs` の `host_run` の行 (T11) がある。
- ハンドルの読みは `Source` の総称で、`FileIn` と `Stdin` が同じ `fill`、`read_text` を使う (object-model.md の「読み手」)。
- 前の段で決まったこと (この段の前提で、蒸し返さない)
  - std は import したモジュールからだけ見える。
  - Prelude に置くのは、lang item と、リテラルを持つ型と、どこでも要る入出力だけである。lang item の補助は `pub` にせず、`LangItems` が名前で引く (`crates/eml_hir/src/def_map.rs` の `lang_items`)。
  - 名前の規則は次のとおりである。
    - 文字列の読み書きが短い名前を持ち、バイト列の版は `_bytes` を足す。
    - ハンドルは型ごとのモジュール (`Stdin`、`Stdout`、`Stderr`、`FileIn`、`FileOut`) に置く。
  - `FileOut` は、書けない誤りを `Io { op: Write }` で返して、ハンドルを手放す。`handle_write` は `Handle` を受ける。
  - `String.byte_length` は Pure で借りる。
- `Unr` の `Foreign` は回復のときに写せない (object-model.md の回復の表、`Unrecoverable`)。
- eml のコードからは `io_failed` を呼べない (modules.md:79)。std の中でも同じである。
- `std::io::pipe` と `PipeWriter::try_clone` で `2>&1` を組めることを、rustc 1.98.1 で確かめた。小さな Rust のプログラムで、子の `"out\nerr\n"` と終了コード 3 を受け取った。

## 4. 前提の環境

| 要るもの | 使う所 | 確かめ方 | ないとき |
|---|---|---|---|
| 段「UTF-8 と標準ライブラリ」が閉じていること | すべて | `test -e docs/plans/2026-10-23-utf8-stdlib.md \|\| echo closed` が `closed`、`test -e std/FileIn.em && test -e std/FileOut.em && echo ok` が `ok` | 止めて利用者に聞く |
| Nix の devShell (Rust 1.98.1、cargo-nextest、cargo-insta、verus、verusfmt) | すべて | `cargo nextest --version`、`verus --version` | 止めて利用者に聞く |
| `.#miri` の shell | T-3、T-5 | `nix develop .#miri -c cargo miri --version` | 止めて利用者に聞く |
| `sh`、`sort`、`cat`、`printf` | UI テスト | `command -v sh sort cat printf` | 止めて利用者に聞く |
| macOS (`bench/*.sh`) | T-6 | `uname` が `Darwin` | 表の入れ替えだけを後のコミットにする (roadmap の「進め方」の 2) |

試しの worktree はその中の `target/` で build する。本体の `target/` を使うと、`eml_rt` の `trust::every_source_file_is_listed` が消えた道を指して落ちる。落ちたら `touch crates/eml_rt/src/trust.rs` で build し直す。

## 5. 決めたこと

### 5.1 長く効く方針 (変えるときは設計からやり直す)

実装の途中でこれらとぶつかったら、止めて報告する。

#### D-1: `'...'` の引用は lexer が読む

- 決定
  - エスケープでない `'` を、`Command` モードの幅1のトークン `CMD_QUOTE` にする。`Mode::Command` に `quote: Option<usize>` (開きの位置) を持たせ、`'` に着くたびに開閉を切り替える。`body` が止まる文字に `'` を足す。
  - 引用の中でも、エスケープと穴は今までどおり `ESCAPE`、`INTERP_START` … として読む。
  - 引用の中の `'` は `\'` と書く。`\'` は、すでにエスケープの表にある (lexical.md:88)。
  - 穴の中の `'` は穴の `Code` モードの文字のリテラルで、引用とは関係がない。
  - バッククォートは引用の中でもリテラルを閉じる。
  - 引用が開いたまま、閉じのバッククォート、改行、ファイルの終わりに着いたら、E0017 `UNTERMINATED_QUOTE` を出す。
    - メッセージは ``unterminated `'` in a command literal``、ラベルは `missing closing '`、範囲は開きの `'` の1文字である。
    - 改行とファイルの終わりでは、`close_layers` が今までどおりリテラルも閉じる。引用の E0017 を先に、リテラルの E0002 を後に出す (2件)。
    - `\` と改行の扱い (長さ1の `ESCAPE`、E0002 だけ) は変えない。
  - E0017 は `eml_syntax::codes` で E0002 の近くに置く。
  - CST は `COMMAND_LIT` の子に `CMD_QUOTE` が増えるだけである。grammar.md は `command ::= CMD_START (CMD_TEXT | CMD_QUOTE | ESCAPE | '\{' '..'? expr INTERP_END)* CMD_END` になる。
- 理由: 閉じていない引用の診断を、エスケープの誤り (E0008) と同じ字句の段に置ける。
- 退けた案: `'...'` を lexer でなく `words` で解く。閉じていない引用の診断が HIR の番号になり、E0008 と置き場所が食い違う。
- 見直す条件: なし
- リポジトリに残す所: lexical.md の「コマンドリテラル」(引用の規則とトークンの表)、grammar.md の `command`、docs/implementation/diagnostics.md の E0017。T-1 で書く。

#### D-2: 引数への分け方と、穴は `String` だけ

- 決定
  - 区切りは引用の外の `CMD_TEXT` にある空白とタブだけである。`\t` などのエスケープ、穴、引用の中の空白は区切らない。改行はリテラルに書けない。
  - 区切りを挟まずに隣り合う本文、エスケープ、引用、穴は、1つの引数になる。
    - `--out=\{dir}` は1つの引数である。
    - `a'b c'd` は `ab cd` という1つの引数である。
    - `''` は空の引数である。
  - 最初の引数がプログラムである。引数が1つもなければ E1056 `COMMAND_WITHOUT_PROGRAM` (`a command literal needs a program`、範囲はリテラル全体) にする。
  - 穴 `\{e}` の型は `String` だけにする。`display` を呼ばない。
    - lexical.md:196 の今の規範をそのまま保ち、lexical.md の文言は変えない。
    - 文字列の補間 (`display` を呼ぶ) との違いは、expressions.md の「コマンドリテラル」の節に1文で書く。
  - `\{..xs}` (`xs : List String`) は、`xs` の要素を別々の引数として広げる (spread)。spread になるのは、次の3つがそろうときだけである。
    1. 引数が spread の穴1つだけでできている。
    2. 引用の中にない。
    3. 最初の引数 (プログラム) でない。
  - それ以外の位置の `\{..}` は E1055 `SPREAD_NOT_AN_ARGUMENT` にする。
    - メッセージは ``a `\{..}` hole must be a whole argument after the program`` で、範囲は穴 `\{..xs}` 全体である。
    - 穴ごとに出す。
  - E1055 と E1056 はどちらもリテラル全体を `Missing` にし、穴の式は lower しない (今の E0004 と同じ方針)。
- 理由
  - 引数の並びは OS に渡すものである。
  - 穴を `String` に限ると、`\{xs}` (`xs : List String`) が `["a", "b"]` という1つの引数になる取り違えを型の誤りで止められる。`Cmd` 自身や `Option` が気づかないうちに引数になることも止められる。
  - この利点は `\{n}` を `\{show n}` と書く手間より大きい。
- 退けた案: 穴で `display` を呼ぶ。上の取り違えが型の誤りにならない。
- 見直す条件: なし
- リポジトリに残す所: lexical.md の「コマンドリテラル」(区切り)、expressions.md の新しい節「コマンドリテラル」(引数、穴の型、脱糖)。T-1 と T-2 で書く。

#### D-3: `Cmd` は Prelude の `Unr` なレコードで、リテラルは補助の呼び出しに脱糖する

- 決定
  - 次の宣言を `std/Prelude.em` に足す。

    ```haskell
    -- 標準エラーの向き。`Proc.merge_stderr`、`discard_stderr` が設定し、利用者がコンストラクタを書く必要はない
    pub data StderrMode =
      | InheritStderr
      | MergeStderr
      | DiscardStderr
      deriving (Eq, Show)

    pub data Cmd =
      | Cmd { program : String, args : List String, cwd : Option String, env : List (String, String), stderr : StderrMode }
      deriving (Eq)

    -- `show` は導出した形、`display` は人が読む1行 (引数を空白でつなぐ。構文には戻せない)
    instance Show Cmd where
      show c = "Cmd { program = \{show c.program}, args = \{show c.args}, cwd = \{show c.cwd}, env = \{show c.env}, stderr = \{show c.stderr} }"
      display c = join_words c.program c.args

    -- コマンドリテラルの脱糖が名前を引かずに呼ぶ (docs/spec/expressions.md の「コマンドリテラル」)
    cmd_new : String -> List String -> Cmd
    cmd_new program args = Cmd { program = program, args = args, cwd = None, env = Nil, stderr = InheritStderr }

    cmd_spread : List String -> List String -> List String
    cmd_spread Nil rest = rest
    cmd_spread (x :: xs) rest = x :: cmd_spread xs rest
    ```

    - `join_words` は Prelude の中だけの関数で、`++` だけで書く。
    - レコードの `deriving (Eq)`、`show`、`{ c | cwd = … }` が動くことは、小さなプログラムで確かめた。
  - `Cmd` を Prelude に置く理由は、リテラルを持つ型だからである (前の段の Prelude の規則)。関数は `Std.Proc` に置く。
  - コンストラクタとフィールドは公開される (modules.md:129)。今はコンストラクタを隠す手段がない (roadmap の「抽象型」は未着手) ので、次の規則を stdlib.md の「`Proc`」に書く。
    - `Cmd` の値はコマンドリテラルと `Std.Proc` の関数で組む。
    - フィールドの名前と数は処理系の都合で変わりうるので、利用者のコードは `Cmd { … }` の作る式とパターンを書かない。読むだけの射影 (`cmd.program`、`cmd.args`) は書いてよい。
    - 見直す条件: 抽象型が入ったとき、`Cmd` と `StderrMode` のコンストラクタを隠す。
  - 脱糖では新しい HIR の節点を作らない。`LangItems` に `cmd_new: FunctionId` と `cmd_spread: FunctionId` を足す。
    - 各引数は文字列と同じ形にする。穴がなければ `Literal::String` (`''` は `""`) にする。
    - 穴があれば `ExprKind::Interpolation` にし、各穴を `ExprKind::Annot { expr, ty: String }` で包む。
      - `ty` は `TypeRef { kind: Con(extern_type(ExternType::String), [], []), range: 穴の範囲 }` である。
      - 範囲は引数の範囲である。
    - 引数の並びは後ろから `Nil` と `word :: rest` (`lang.cons`。リストのリテラルと同じ) で組む。spread は `cmd_spread xs rest` の呼び出しにする。
    - 全体は `cmd_new program args` の呼び出しで、範囲はリテラル全体である。
    - 型の誤りは今ある道で出る。
      - `\{e}` が `String` でなければ、`Annot` の E2001 になる。
      - `\{..xs}` が `List String` でなければ、`cmd_spread` の引数の E2001 になる。
      - 型検査と Core IR には足すものがない。
- 理由
  - `extern data` (`Foreign`) にしない理由
    - `Unr` の `Foreign` は回復のときに写せない。REPL で `` let c = `ls` `` と束縛しただけで、回復ができなくなる。
    - `Lin` にすると、同じ `Cmd` を2回動かせない。
  - 脱糖を補助の呼び出しにすると、型検査と Core IR に新しい規則が要らない。
- 退けた案
  - 設定を入れ子のレコード (`Cmd { program, args, config : CmdConfig }`) にする: フィールドが増えるときに壊れる式は、入れ子でも同じである。Prelude に型が1つ増えるだけになる。
  - 新しい HIR の節点: 型検査と Core IR の規則が増える。
- 見直す条件
  - 抽象型が入ったとき (上の規則)
  - `Cmd` のフィールドを、D-11 の項目が足すとき。足すのは、`stdin` と `stdout` の向きの2つまでと見込む。
- リポジトリに残す所: expressions.md の「コマンドリテラル」と「HIR で行う脱糖と検査」、declarations.md:75 (Prelude の型)、modules.md の「Prelude」、stdlib.md の「`Proc`」。T-2 と T-4 で書く。

#### D-4: `Std.Proc` の API

`import Proc` が要る。名前の規則は前の段の規則に従う。

| 関数 | 型 | 意味 |
|---|---|---|
| `run` | `Cmd -> <IO> Int` | 標準入出力を受け継いで起動し、終わるのを待って終了コードを返す |
| `output` | `Cmd -> <IO> (String, Int)` | 標準出力を文字列に取り、終了コードと組で返す。標準入力は空 (`/dev/null`) |
| `output_bytes` | `Cmd -> <IO> (Bytes, Int)` | `output` のバイト列の版 |
| `capture` | `Cmd -> <IO> String` | `output` と同じだが、終了コードが 0 でなければ IO の失敗 (`kind` は `Other`、文言は ``command `git` exited with code 128``) |
| `capture_bytes` | `Cmd -> <IO> Bytes` | `capture` のバイト列の版 |
| `spawn` | `Cmd -> <IO> Child` | 標準入力と標準出力を pipe にして起動する |
| `cwd` | `String -> Cmd -> Cmd` | 作業ディレクトリ。相対パスは `FileIn.open` と同じ基準 (`file_root`) |
| `env` | `String -> String -> Cmd -> Cmd` | 環境変数を1つ上書きする (後の組が勝つ) |
| `arg` / `args` | `String -> Cmd -> Cmd` / `List String -> Cmd -> Cmd` | 引数を後ろに足す |
| `merge_stderr` | `Cmd -> Cmd` | 標準エラーを標準出力と同じ先に向ける (`2>&1`)。`output`、`capture` では文字列に入り、`run` では標準出力に出る |
| `discard_stderr` | `Cmd -> Cmd` | 標準エラーを捨てる (`2>/dev/null`) |
| `Child` | `pub extern data Child` (`Lin`) | 子のプロセスと、その標準入力への書き口と、標準出力の読み手 |
| `write` / `write_bytes` / `write_line` | `String -> Child -> <IO> Child` など | `FileOut` と同じ形で、子の標準入力に書く |
| `close_stdin` | `Child -> <IO> Child` | 子の標準入力を閉じる (子に EOF を届ける) |
| `read_all` / `read_line` / `read_all_bytes` / `read_line_bytes` / `read_chunk` | `Stdin` と同じ名前と形 (`Child -> <IO> (String, Child)` など) | 子の標準出力を読む |
| `wait` | `Child -> <IO> Int` | 標準入力を閉じてから子を待ち、終了コードを返す。ハンドルを消費する |

- どの起動も、標準エラーの向きは `Cmd` の `stderr` に従う (既定は受け継ぐ)。`spawn` でも `merge_stderr` が効き、子の標準エラーを `read_*` で読める。
- 設定の関数は対象を最後に置く (`` `make` |> Proc.cwd "build" |> Proc.merge_stderr |> Proc.capture ``)。`with_` の名前は handler の慣習 (examples.md の `with_env`) に取っておく。
- プログラムの解決
  - `program` が `/` を含まなければ、`PATH` から探す。
  - `/` を含む相対パス (`./run.sh`) は子の作業ディレクトリ (`cwd` があればそれ、なければ `file_root`) に対して解決し、Host が絶対パスにしてから起動する。
  - Rust の `Command` は `current_dir` と相対のプログラムの組み合わせを決めていないためである。
- `capture` の誤りは次の順に出る。
  1. 起動の誤り (`Io { op: Run }`)
  2. 標準出力の読みの誤り (`Io { op: Read }`、`NotUtf8`)
  3. 終了コード (`Exited`)
  - 本体は `let (out, c) = read_all (spawn_with … cmd); let code = wait c; if code == 0 then out else exited cmd.program code` で、この順は本体の順そのものである。
  - UTF-8 でない出力を出して 0 でなく終わった子は、`NotUtf8` で失敗する。
  - 読みの失敗で巻き戻るときは、`Child` の破棄処理が子を kill する。
- `read_line` と `read_line_bytes` は、`FileIn`、`Stdin` と同じく`read_line_raw` と `read_line_bytes_raw` の上に eml で書く。
- std の中だけの extern (`std/Proc.em`)
  - `spawn_raw : String -> String -> String -> String -> Int -> Int -> Int -> <IO> Child`
    - 引数は次の7つである。
      1. プログラム
      2. 引数の列
      3. 環境の列
      4. 作業ディレクトリ (空なら設定なし)
      5. 標準入力の向き (0 受け継ぐ、1 空、2 pipe)
      6. 標準出力の向き (同じ)
      7. 標準エラーの向き (0 受け継ぐ、1 標準出力に合わせる、2 捨てる)
    - 列は各要素を「バイト長の十進、NUL、本文」の順に並べたものである。環境の列はキーと値を交互に置く。`String.byte_length` が Pure で借りるので、組み立てに `Panic` がなく、`run` の型から今の `Panic` が消える。
    - NUL を含む要素は Rust の側で `InvalidInput` (`I/O error`) にする (今の規則を保つ)。向きの値が範囲の外のときも、同じく `InvalidInput` にする。
  - `exited : String -> Int -> <IO> a`
    - いつも `Fault::Exited { program, code }` で失敗する。`exit` と同じ `OpRet::Never` の行で、`Purity::Effectful` である。
    - eml からは `io_failed` を呼べないので、失敗を起こす窓口がこの1行だけ要る。
  - 読みの `read_line_raw`、`read_line_bytes_raw` と、`pub` の読み書きの8行
- 今の `extern status` と、`String -> List String` の `run` は消す。プログラムを変数で与えるときは `` `\{prog} \{..args}` `` と書く。
- 失敗と届け方

  | 操作 | 失敗 | 届け方 |
  |---|---|---|
  | `run`、`output*`、`capture*`、`spawn` | 起動できない (`Io { op: Run, name: program }`。``cannot run `rsync`: not found``。NUL を含む引数は `I/O error`) | `IO` (`try_io`) |
  | `capture`、`capture_bytes` | 終了コードが 0 でない (`Exited`。``command `sh` exited with code 3``。シグナルなら 128 + 番号) | `IO` (`kind` は `Other`) |
  | `Child` の読み | 読めない (`Io { op: ReadOutput, name: program }`。``cannot read the output of `sort`: I/O error``)、UTF-8 でない (``the output of `sort` is not valid UTF-8``) | `IO` |
  | `Child` の書き込み | 書けない (子が先に終わって EPIPE など)。`Io { op: WriteInput, name: program }` (``cannot write to `sort`: I/O error``) で、`FileOut` と同じくハンドルを手放す | `IO` |
  | `wait`、読み、書き込み | 割り込み。子が終わった後に `Stopped` | 位置を付けた実行時エラー |

  - `Child` の書き込みの失敗を、`Stdout` と違って `try_io` で捕まえるのは、`Child` が自分の pipe に直に書き、共有の buffer を持たないためである (`FileOut` と同じ理由)。
- 理由
  - 起動の形を3つ (受け継ぐ、取る、pipe) に分けると、どの形でも引数と設定は同じ `Cmd` で組める。
  - 終了コードは `run` と `output` では値で返す。`capture` は失敗するコマンドを IO の条件として扱う。
- 退けた案
  - 非 0 の終了を `Panic` にする: `Panic` はバグのためのもので、失敗するコマンドは IO の条件である。
  - `IOErrorKind` に変種を足す: 変種を足しても終了コードは持てない。`Other` と文言で足りる。
  - `wait_checked` の extern: `wait` と同じ行がもう1つ増える。失敗を起こすだけの `exited` のほうが小さく、`capture` の順を eml で読める。
  - 標準エラーを別の文字列に取る (`output` が `(String, String, Int)` を返す): extern は組を1つしか作れない (`with_arg`)。Host が2本の pipe を同時に読む仕組みも要る。
  - `spawn_raw` の引数を1つの文字列に詰める (今の `status` の形): 入れ子の符号化が増える。引数を分ければ、スカラーと文字列だけで済む。
  - `read` という名前 (後に `capture` と改名した): `output` は組を返し、`capture` は確かめる版、`read_*` は `Child` から読む関数、と役割で分ける。
- 見直す条件: D-11 の各項目の条件
- リポジトリに残す所: stdlib.md の「`Proc`」(API、`Cmd` の規則、誤りの順)、「ハンドル」(`Child`)、「`try_io`」の表、「実行時エラー」。T-4 で書く。

#### D-5: `Child` の後始末と pipe の規則

- 決定
  - `Child` を使い終える道は `wait` だけにし、閉じる関数は置かない。
  - 巻き戻し (`try_io`、`try_panic`、実行時エラー、`Heap` を落とすとき) で手放した `Child` は、破棄処理が次の順で片づける。
    1. pipe を閉じる。
    2. 直接の子を `kill` する。
    3. 子を待つ。
  - 直接の子は `try_io` で抜けた後に残らず、zombie にもならない。`sh -c` の孫は残りうるので、spec にそう書く。
  - 方針は stdlib.md の `Task a` の申し送り (`drop` はキャンセル) と同じである。
  - pipe の規則を stdlib.md の「`Proc`」に書き、`proc_spawn.em` の先頭のコメントにも書く。
    - eml は1度に1本の pipe しか読み書きしない。pipe の buffer は OS が持つ (ふつう 64 KiB)。
    - 入力を読み終えてから出力を書く子 (`sort`、`wc`) には、書き終えて `close_stdin` してから読む。入力の量に上限はない。
    - 読みながら書く子 (`cat`、`tr`、`grep`) に buffer より多く書いてから読むと、子は出力を書けず、eml は入力を書けず、両方が止まる。そういう子には buffer より少なく書くか、入力を分けて書くたびに読む。
    - `wait` の前に標準出力を読み切る。読まずに `wait` すると、buffer より多く出力する子で止まる。
    - 止まったときは、Ctrl-C で終われる (書き込みと読みは `EINTR` で戻り、止める語を見る)。処理系はこの状態を検出しない。
- 理由
  - 破棄処理は失敗せず、どのスレッドで呼んでもよい必要がある (object-model.md の破棄処理、multicore.md の「スレッドに縛られたリソース」)。`kill` と `wait` は lock を持たず、誤りを捨てられる。
  - 並行処理がないので、詰まらない読み書きの順は規則で示すしかない。
- 退けた案
  - 落としたときに待つだけにする: pipe が詰まって止まりうる。
  - 落としたときに切り離す: `try_io` で抜けた後に子が残る。
  - `Child` を、`Process`、`Stdin`、`Stdout` の3つの `Lin` なハンドルの束にする (examples.md:142-152 の今の例): 並行処理がないので別々に持つ利点がなく、型の名前が `Stdin`、`Stdout` のモジュールとぶつかる。
- 見直す条件: `Async` が入ったとき。D-11 のパイプと、標準エラーの pipe もそのときに見直す。
- リポジトリに残す所: stdlib.md の「`Proc`」、object-model.md の破棄処理の段落。T-3 と T-4 で書く。

### 5.2 短中期の設計 (理由があれば実装の中で変えてよい。変えたらコミットのメッセージに書く)

#### D-10: Host と `eml_rt` の `spawn`

- Host に `spawn` を足す (`run` は T-5 で消す)。

  ```rust
  pub enum Stdio { Inherit, Null, Pipe }
  pub enum StderrMode { Inherit, ToStdout, Null }
  pub struct Spawn<'a> {
      pub program: &'a str, pub args: &'a [&'a str], pub env: &'a [(&'a str, &'a str)],
      pub cwd: Option<&'a str>, pub stdin: Stdio, pub stdout: Stdio, pub stderr: StderrMode,
  }
  pub trait Process: Send {
      fn wait(&mut self) -> io::Result<i32>;   // 終了コード (`exit_code`)
      fn kill(&mut self) -> io::Result<()>;
  }
  pub struct Spawned {
      pub stdin: Option<Box<dyn Write + Send>>,
      pub stdout: Option<Box<dyn Read + Send>>,
      pub process: Box<dyn Process>,
  }
  // Host の trait に足す
  fn spawn(&self, spawn: &Spawn<'_>) -> io::Result<Spawned> { Err(io::Error::from(io::ErrorKind::Unsupported)) }
  ```

  - `std::process::Child` を Host の型に出さない。組み込み先 (WASM、ブラウザ) と Miri のテストの Host が、プロセスなしで実装できるようにするためである。
- `StdHost::spawn`
  - 今の `run` と同じく、標準出力を書き出してから `Command` を組む。
  - `cwd` は `file_root.join(cwd)` にする (なければ今と同じ `file_root`)。`/` を含む相対の `program` はその作業ディレクトリに対して絶対パスにする。
  - `stdin` と `stdout` は `Stdio` のとおりにする。
  - `stderr` の扱い
    - `Inherit`: 受け継ぐ。
    - `Null`: `Stdio::null()` にする。
    - `ToStdout`、標準出力が `Pipe` のとき: `std::io::pipe()` で作った書き口を `try_clone` して両方に渡し、読み口を `Spawned.stdout` にする。
    - `ToStdout`、標準出力が `Inherit` のとき: 受け継いだ標準出力の fd を `try_clone` して、標準エラーに渡す。
  - `Process` の impl は `std::process::Child` の `wait` に `exit_code` を掛ける。`kill` は `Child::kill` である。
- `CaptureHost::spawn` (`crates/eml_vm/src/hosts.rs`)
  - 今の `run` と同じく、環境を空にして、表と `PATH` だけを渡す。
  - `Inherit` の標準出力と標準エラーは pipe にし、それぞれスレッドで最後まで読む。`wait` のとき、子が終わってから `out`、`err` の順に足す。捕まえた出力の順を今と同じにするためである。
  - `ToStdout` は `StdHost` と同じ pipe の共有で、`Inherit` の標準出力の pipe に合流する。
  - `Null` の標準入力は今と同じ `Stdio::null()` である。
  - スレッドを使うのは、子が 64 KiB を超えて書いたときに、eml の側の読み書きと詰まらないためである。`child_stderr.em` (出力と誤りの順) が、この経路を固定する。
- `eml_rt`
  - 型: `pub(crate) struct Child { program: Box<str>, process: Option<Box<dyn Process>>, stdin: Option<Box<dyn Write + Send>>, stdout: Option<Reader> }` (`host.rs` の `external_body`)。
    - `impl Source for Child` を置く。`reader` は `stdout.as_mut()`、`take_size` は `None`、`name` は新しい `SourceName::Child(program)` である。
    - `Utf8Source::Child(program)` も足す。
  - `impl Drop for Child`
    - `stdin` と `stdout` を落としてから、`process` が `Some` なら `kill` して `wait` する。誤りは捨てる。
    - `wait` が取り出した後は `None` なので、2回は待たない。
    - I/O をするが lock を持たず、どのスレッドで落としてもよい。
  - 信頼の関数 (T11)
    - `host_spawn(host, control, spawn) -> Result<Child, Fault>`: 今の `host_run` と同じく、前に止める語と `flush` を見て、`Fault::Io { op: Run, .. }` を返す。
    - `child_write(c: *mut Child, bytes, line, control) -> Result<(), Fault>`: 書き込みが `EINTR` で戻ったら止める語を見て、立っていれば `Fault::Stopped` を返す (D-4 の表、D-5)
    - `child_close_stdin(c)`
    - `child_wait(c, control) -> Result<i64, Fault>`: `stdin` を落とし、`process` を取り出して待ち、後で止める語を見る。
    - `fill`、`consume`、`read_text` は総称なので、`Child` にそのまま使える。
  - `Ctx` のメソッド (`heap/ctx/script.rs` か新しい `proc.rs`)
    - `proc_spawn(program, args, env, cwd, stdin, stdout, stderr) -> Result<Word, Fault>`: 4つの文字列を確かめて読み、`fs_open` と同じく `alloc_foreign(child, true)` を呼び、文字列を手放す。
    - `proc_wait(h)`: `handle_close` と同じ形で消費し、`Int` を返す。
    - `proc_close_stdin(h)`: `h` を返す。
    - `proc_exited(program, code) -> Result<Word, Fault>`: 文字列を手放し、いつも `Err(Fault::Exited)` を返す。
    - 読みは `handle_read(h, Handle::Child, ..)` である。書きは `handle_write` に `Handle::Child` の枝を足す。`FileOut` の枝と同じ形で、`foreign_unique::<Child>` を使う。
    - 約束は既存の行と同じである: RC の増減、`Fault::Heap` を返さないこと、誤りのときに作りかけの物体を手放すこと。
  - `Fault::Exited { program: Box<str>, code: i64 }` を足す。`io_error_kind` は `IO_OTHER` を返す。`IoOp::Write` の文言 (``cannot write `<name>` ``) は変えない。`Child` の書き込みは新しい `IoOp::WriteInput` (``cannot write to `<program>` ``)、読みは新しい `IoOp::ReadOutput` (``cannot read the output of `<program>` ``) にする。`Fault` の大きさを保つため、`Exited` は箱に入れる (`Fault::Exited(Box<Exited>)`)。
  - 信頼の一覧 (`trust.rs`) に5行を足す (すべて T11): `host.rs: external_body Child`、`host_spawn`、`child_write`、`child_close_stdin`、`child_wait`。`host_run` の行は T-5 で消す。
  - `Process` の trait と、`Spawn`、`Spawned`、`Stdio`、`StderrMode` は、`verus!` の外の glue である。object-model.md の「`unsafe` の核」の一覧に足す。
- 書き出しの地点の文 (stdlib.md の「`Stdout` と `Stderr`」の「`Proc.run` の前」と、object-model.md の「出力のバッファ」) は、「子を起動する前」にする。
- VM: `Op::Extern` の汎用の dispatch に、次の枝を足す。`ProcStatus` の枝は消す。
  - `ProcSpawn`、`ProcWait`、`ProcExited`、`ProcCloseStdin`
  - `ProcWrite*` (3)
  - `ProcRead*` (5)。読みは `Handle::Child` を既存の `handle_read` の枝に足すだけである。
- `eml_extern` の行 (12 行を足し、`ProcStatus` の1行を消す)
  - `ExternType::Child => row("Std.Proc.Child", Lin, Obj)`
  - `ProcSpawn = kinded("Std.Proc.spawn_raw", &[Obj, Obj, Obj, Obj, Int, Int, Int], &[Unr; 7], Obj, Lin, Effectful)`
  - `ProcWait = kinded("Std.Proc.wait", &[Obj], &[Lin], Int, Unr, Effectful)`
  - `ProcExited`: `Exit` と同じ `OpRet::Never` の形で、`params: &[Obj, Int]`
  - `ProcCloseStdin = kinded(.., &[Obj], &[Lin], Obj, Lin, Effectful)`
  - `ProcWrite`、`ProcWriteBytes`、`ProcWriteLine`: `writing(..)`
  - 読みの5行: `Std.Stdin` の5行と同じ `borrowing` と `with_arg`

#### D-11: 後に回す項目 (roadmap の「その後の項目」に書く文)

T-6 で、各項目を「後に回した理由: …。」と「見直す条件: …。」の入れ子の箇条で書く。

| 項目 | 後に回した理由 | 見直す条件 |
|---|---|---|
| パイプ (`.\|`) | eml で読んで書き写すと、2段目の標準出力を pipe にした途端に 64 KiB で詰まる | `Async` が入ったとき。または、`Cmd` の列を1回の `spawn` で OS の fd でつなぐ形を決めたとき |
| 標準入力と標準出力のファイルへのリダイレクト (`< in`、`> out`) | Host の側は `Stdio::from(File)` で済むが、Prelude に向きの型と `Cmd` のフィールド、`spawn_raw` の引数、開けないときの誤りの形が要る。`FileIn`、`FileOut` と `spawn`、`output` で書ける | メモリに収まらない出力をファイルに流すスクリプトが出たとき。`Cmd` に `stdin`、`stdout` のフィールドを足す形にし、`StderrMode` と同じ道をとる |
| `run`、`output` の標準入力を文字列で与えること | 読みながら書く子で詰まる (D-5 の pipe の規則)。`spawn` で書ける | `Async` が入ったとき。または、Host が書きながら読む `output` を持つとき |
| 標準エラーを別の文字列に取ること | extern は組を1つしか作れず、Host が2本の pipe を同時に読む仕組みが要る | `merge_stderr` で分けられない例が出たとき |
| 環境変数の削除と全消し | 上書きで足りる例しかない | `env -i` に当たる用途が出たとき |
| `Child` の標準エラーの pipe (読み分け) | 3本の pipe を1つのスレッドで扱うと詰まる | `Async` が入ったとき |

## 6. 実装の決まり

- どのコミットの後も、`cargo build` と `cargo nextest run` が通る (CLAUDE.md)。
- 新しい extern は表の行と std の宣言を同じコミットに入れる。`crates/eml_core_ir/tests/externs.rs` の借用の表と、`expected.txt` (`#[ignore]` のテストが書き直す) も同じコミットで直す。
- `eml_rt` を変えるコミット (T-3、T-5) は crates/eml_rt/CLAUDE.md の門を通る: Verus、`verusfmt`、信頼の一覧、Miri の2つの設定。`Child` の読みの約束は `FileIn` の読みの補題を使い回す。
- Miri は本物のプロセスを起動できない。Miri のテストは Host を差し替えて境界だけを確かめる。
- `spec/` の節は規則を実装するコミットで書く。コードの注釈はその節を引き、この計画を引かない。
- 日本語の注釈と docs は `yomiyasu:yomiyasu` の規則で書く (CLAUDE.md)。
- プロセスを起動する UI テストは2つのモードで2回起動する。1ファイル1秒に収まる小さな子 (`sh -c`、`sort`、`cat`) だけを使う。
- UI テストのファイルに、import するモジュールと大文字小文字だけが違う名前 (`proc.em` など) を付けない。macOS では、隣のファイルが当たって E1026 になる。
- 新しく書く eml のプログラムは `import Proc` を書く (std は import したモジュールからだけ見える)。

## 7. タスク

| ID | タスク | もとの決定 | 前のタスク | 並べて進められるか |
|---|---|---|---|---|
| T-1 | 字句と AST | D-1、D-2 | - | いいえ |
| T-2 | `Cmd` と脱糖 | D-2、D-3 | T-1 | いいえ |
| T-3 | `eml_rt` と Host の `spawn` | D-5、D-10 | T-2 | いいえ |
| T-4 | extern の行、VM、`std/Proc.em` | D-4、D-5、D-10 | T-3 | いいえ |
| T-5 | `Host::run` の削除 | D-10 | T-4 | いいえ |
| T-6 | 段を閉じる | D-11 | すべて | いいえ |

### T-1: 字句と AST

- 目的: D-1 の引用と、D-2 の引数への分け方を、`eml_syntax` の中だけで入れる。HIR は今までどおり `CommandLit` を E0004 にするので、ほかの crate は変わらない。
- 対象
  - `crates/eml_syntax/src/lexer/` (`Mode::Command`、`body`、`close_layers`)
  - `crates/eml_syntax/src/codes.rs` (E0017)
  - `crates/eml_syntax/src/ast.rs`
  - docs/spec/lexical.md、grammar.md、docs/implementation/diagnostics.md
- 作業
  - D-1 の `CMD_QUOTE` と `quote` と E0017 を入れる。
  - 型付き AST に次のものを足す。

    ```rust
    impl Interp { pub fn spread(&self) -> bool }          // `DOT2` のトークンがあるか

    pub enum CommandPart { Text(String), Hole(Interp) }   // 穴は `Interp` の節点のまま持つ (`expr()`、`spread()`、`range()`)
    pub struct CommandWord {
        pub range: TextRange,        // 引数の先頭の文字から末尾の文字まで (区切りを含まない)
        pub parts: Vec<CommandPart>, // 書いた順。隣り合う本文、エスケープ、引用の中の本文は1つの `Text` にまとめ、空の `Text` は入れない
        pub quoted: bool,            // 引数の中に `CMD_QUOTE` があったか
    }
    impl CommandLit {
        pub fn words(&self) -> Option<Vec<CommandWord>>;  // lexer が報告した誤り (閉じていないリテラル、不正なエスケープ、`CMD_QUOTE` が奇数個) では `None`
        pub fn holes(&self) -> AstChildren<Interp>;
    }
    ```

  - `words` の形の例

    | 書いたもの | `parts` | `quoted` |
    |---|---|---|
    | `--out=\{dir}` | `[Text("--out="), Hole]` | false |
    | `a'b c'd` | `[Text("ab cd")]` | true |
    | `''` | `[]` | true |
    | `a\{..xs}` | `[Text("a"), Hole(spread)]` | false |
    | `\{..xs}\{y}` | `[Hole(spread), Hole]` | false |
    | `'\{..xs}'` | `[Hole(spread)]` | true |

  - spec
    - lexical.md の「コマンドリテラル」: 引用の規則、区切り、トークンの表
    - grammar.md の `command` (D-1 の形)
    - diagnostics.md: E0017 の行
  - テスト
    - `crates/eml_syntax/tests/lexer.rs` と `tests/strings.rs`: `CMD_QUOTE` の場合を足す (既存の期待は変わらない)。
    - `words` の単体テスト: 区切り、隣接、`''`、`quoted`、spread の3つの位置
    - 新規 `tests/ui/check-fail/syntax/command_quote_unterminated.em`: 閉じのバッククォートの前で閉じていない引用 (E0017) と、行末で閉じていない引用 (E0017 と E0002 の2件)
- 実装者に任せること: `words` の内部の組み立て方
- 条件: いつも行う
- 完了の基準
  - `cargo nextest run` → すべて通る。`cargo clippy --all-targets -- -D warnings` と `cargo fmt --check` → 通る
  - `timeout 20 cargo run -q -p eml_cli -- check tests/ui/check-fail/syntax/command_quote_unterminated.em` → E0017 を出し、行末の例では E0002 も出す。終了コード 1
  - `git diff --stat HEAD~1 -- crates/eml_hir crates/eml_types crates/eml_core_ir` → 空
- テストの変更の種類: 種類1 は新規の check-fail 1本。既存の期待は変わらない。

### T-2: `Cmd` と脱糖

- 目的: D-3 の Prelude の宣言と、D-2 と D-3 の脱糖を入れる。`std/Proc.em` は触らない。
- 対象
  - `std/Prelude.em`
  - `crates/eml_hir/src/def_map.rs` (`LangItems`)
  - `crates/eml_hir/src/lower/expr.rs` (E0004 の枝と `lower_string`)
  - `crates/eml_hir/src/codes.rs` (E1055、E1056)
  - テスト、docs
- 作業
  - D-3 の Prelude の宣言と、`join_words` を足す。
  - `LangItems` に `cmd_new` と `cmd_spread` を足す。
  - `lower_command` を書き、E0004 の枝を置き換える。
    - `lower_string` の段の組み立て (1191-1230 行のあたり) を、穴の包み方を引数に取る関数に切り出して、`lower_command` と共有する。
    - 穴は D-3 の `Annot { expr, ty: String }` で包む。
  - E1055 と E1056 (D-2)
  - spec
    - expressions.md に「コマンドリテラル」の節を足す (引数、穴の型、文字列の補間との違い、脱糖)。「HIR で行う脱糖と検査」の項目を直す。
    - declarations.md:75 (Prelude の型に `Cmd` と `StderrMode`)
    - modules.md の「Prelude」(`Cmd` をリテラルを持つ型として挙げる)
    - diagnostics.md (E0004 の一覧からコマンドリテラルを外し、E1055 と E1056 を足す)
    - architecture.md:169、198 (コマンドリテラルの E0004 の記述)
    - status.md:40
  - テスト
    - 種類1
      - `tests/ui/check-fail/not-yet-supported/command_literal.em` を消す (E0004 が出なくなる)。
      - 新規 `tests/ui/run/basics/command_literal.em`: リテラルの `show` と `display`、`''`、`--out=\{dir}`、`\{..xs}`。`Proc` を使わず、脱糖だけを確かめる。
      - 新規 check-fail 6本
        - `names/command_spread_in_word.em` (E1055)
        - `names/command_spread_as_program.em` (E1055)
        - `names/command_spread_quoted.em` (E1055)
        - `names/command_literal_empty.em` (E1056)
        - `names/command_literal_unknown_name.em` (穴の E1001)
        - `types/command_hole_not_string.em` (`\{1}` と `\{xs}` の E2001)
    - 種類2: E0004 の期待を、脱糖した HIR と `Cmd` の型に書き換える。
      - `crates/eml_hir/tests/strings.rs:34-38`
      - `crates/eml_hir/tests/lower.rs:95-106, 353-358`
      - `crates/eml_types/tests/check.rs:527-540`
- 実装者に任せること: 次の2つが通らないときの代わりの道 (10 章の Q-1 と Q-2)
  - Prelude の `pub` でない関数を `Res::Item` で直に指した呼び出しが、名前の検査を通ること
  - `Annot` の `TypeRef` を lowering で組めること
- 条件: いつも行う
- 完了の基準
  - `cargo nextest run` → すべて通る。`cargo clippy --all-targets -- -D warnings` → 警告なし
  - `timeout 20 cargo run -q -p eml_cli -- run tests/ui/run/basics/command_literal.em` → 期待の出力
  - `timeout 20 cargo run -q -p eml_cli -- check tests/ui/check-fail/types/command_hole_not_string.em` → 穴ごとに E2001 を出す (`\{1}`、`\{xs}`)
  - `grep -rn "COMMAND_LIT\|CommandLit" crates/eml_hir/src | grep -i "not_yet_supported\|E0004"` → 何も出さない

### T-3: `eml_rt` と Host の `spawn`

- 目的: D-10 の実行時の部品を入れる。`Host::run`、`host_run`、`Ctx::proc_status`、`ProcStatus` は残す。VM と `std/Proc.em` がまだ使っているためである。
- 対象
  - `crates/eml_rt/src/host.rs`、`externs.rs`、`heap/ctx/` (`script.rs` か新しい `proc.rs`、`io.rs`)、`trust.rs`
  - `heap/tests/trusted.rs`
  - `crates/eml_vm/src/hosts.rs`
  - docs/spec/object-model.md
- 作業
  - D-10 の型、`Host::spawn` (既定は `Unsupported`)、`StdHost::spawn`、`CaptureHost::spawn`、`Child`、信頼の関数、`Ctx` のメソッド、`handle_read` と `handle_write` の `Child` の枝、`Fault::Exited`、信頼の一覧の5行を入れる。
  - `MemoryHost` に `spawn` を実装する。
    - 標準入力は `Vec<u8>` に写す。
    - 標準出力は `Cursor` にする。
    - `Process` は、呼ばれた `wait` と `kill` の回数を数える。
  - Miri のテスト (`heap/tests/trusted.rs`、両モード)
    - `spawn`、`write`、`close_stdin`、`read_all`、`wait` の流れが通る。
    - `wait` せずに `Heap` を落とすと、`kill` と `wait` が1回ずつ呼ばれる。
    - `wait` した後は、`kill` が呼ばれない。
    - 書けない書き口 (`Err` を返す `Write`) で、ハンドルが手放される。
    - `proc_exited` が文字列を手放す。
  - `expected.txt` を再生成する (`Ctx` の新しいメソッドが出るなら)。
  - spec (object-model.md)
    - 「Host」(`spawn` と型)
    - 「出力のバッファ」(書き出しの地点を「子を起動する前」に)
    - 「読み手」(`Child` が3つ目の `Source`)
    - 「誤りのときの約束」(`Child` の書き込み)
    - 破棄処理の段落 (D-5)
    - 「`unsafe` の核」(glue と T11)
- 実装者に任せること: `Ctx` のメソッドを `script.rs` に置くか、`proc.rs` に分けるか
- 条件: いつも行う
- 完了の基準
  - `cargo build` と `cargo nextest run` → 通る
  - `cargo verus verify -p eml_rt` → 誤りなし
  - `verusfmt --verus-only --check $(git ls-files 'crates/eml_rt/src/*.rs')` → 何も出さない
  - `nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_rt trusted::` と、`-Zmiri-tree-borrows` を足した同じコマンド → 通る。`/usr/bin/time -p` の `user` がそれぞれ 60 s と 120 s に収まる。このコミットの前後で測り、差をコミットのメッセージに書く
  - `git diff --stat HEAD~1 -- std crates/eml_extern` → 空
- テストの変更の種類: 新しいテストだけ。種類1はなく、種類2は `expected.txt` の行の追加だけである。

### T-4: extern の行、VM、`std/Proc.em`

- 目的: D-4 の API を std に出し、VM と既存のテストを `spawn` に移す。
- 対象
  - `crates/eml_extern/src/lib.rs`
  - `crates/eml_vm/src/` (`vm.rs`、`compile/emit.rs`)
  - `std/Proc.em`
  - `crates/eml_core_ir/tests/externs.rs`
  - テスト、スクリプト、docs
- 作業
  - D-10 の 12 行を足し、`ProcStatus` の行と dispatch を消す。
  - `std/Proc.em` を D-4 の形に書き直す。設定の関数、`run`、`output*`、`capture*`、`spawn`、`Child` の関数、std の中だけの extern を置く。
  - spec
    - stdlib.md
      - 「`Proc`」を書き直す (API、`Cmd` の規則、pipe の規則、誤りの順、孫が残りうること)。
      - 「ハンドル」に `Child` を足す。
      - 「`try_io`」の表と「実行時エラー」を直す。
      - 「`Stdout` と `Stderr`」の書き出しの地点を「子を起動する前」にする。
    - lir.md:261、818、837 (行の数と、借りる行の一覧)
    - vm.md:28、319-320、351
    - effects.md:67
    - testing.md:327
    - examples.md:106-153 の例を、D-4 の API で書き直す (D-5 の退けた案の `Child` の束を含む)。
  - テスト
    - 種類2: `Proc.run "sh" [...]` をコマンドリテラルに書き換える (標準出力は変わらない見込みで、`missing_program` の位置の行番号は変わりうる)。
      - `tests/ui/run/std/processes.em`
      - `tests/ui/run/std/child_stderr.em`
      - `tests/ui/run-fail/std/missing_program.em`
      - `tests/ui/run/io/try_io_missing_program.em`
    - 種類1 (新規)
      - `run/std/proc_args.em`: `` Proc.output `sh -c 'for a in "$@"; do echo "[$a]"; done' sh 'b c' --x=\{dir} \{..xs}` `` で、引数の分け方を出力で確かめる。
      - `run/std/proc_capture.em`
      - `run/std/proc_spawn.em`: `sort` に 10 行を書いて読む。先頭のコメントに D-5 の pipe の規則を書く。
      - `run/std/proc_spawn_large.em`: `sort` に 256 KiB を書き、`close_stdin` してから `read_all` し、行数を出す。書き終えてから読む経路が、buffer を超えても動くことを確かめる。
      - `run/std/proc_cwd/main.em`: `cwd "sub"` で `cat hello.txt` と `./run.sh` を動かす。
      - `run/std/proc_env.em`: `sh -c 'echo $FOO'`
      - `run/std/proc_stderr.em`: `merge_stderr` で `capture` に入ること、`discard_stderr` で何も出ないこと
      - `run/io/try_io_cmd_exit.em`: `capture` の非 0 を `try_io` で受ける。
      - `run/std/proc_child_released_on_abort.em`: `Child` を持ったまま `try_io` で巻き戻る。
      - `run-fail/std/proc_capture_nonzero.em`: ``command `sh` exited with code 3``
      - `run-fail/std/proc_capture_not_utf8.em`: `printf '\377'; exit 1` が `NotUtf8` で止まる。誤りの順を固定する。
      - `check-fail/linearity/child_never_waited.em`: E3003 `LINEAR_VALUE_NOT_CONSUMED`
      - `check-fail/types/child_is_not_a_file.em`: `FileIn.read_all` に `Child` を渡すと E2001
      - `crates/eml_cli/tests/repl/std/command_literal.repl`: `` let c = `ls \{"a b"}` ``、次の入力で実行時エラー、その次の入力で `show c`。回復の後も `Cmd` を読めることを固定する。
    - 種類3
      - `tests/ui/run/scripts/deploy/main.em` の `real_run` を `` k (try_io (fn () -> Proc.run `sh -c \{cmd}`)) `` にする (`import Proc` は前の段で入っている。なければ足す)。標準出力と終了コードは同じである。
      - `crates/eml_cli/tests/cli.rs` の SIGINT のテスト (581 行のあたり) のプログラムを、同じ形に書き換える。割り込みの振る舞い (子にも SIGINT が届き、子が終わった後に `Stopped`) は `wait` に移るだけで、テストの意味は変わらない。
    - 種類2: `crates/eml_vm/tests/bench.rs` の `unwind_points` の `scripts/deploy` の値 (前の段を閉じた時点の値から、測った値に)
- 実装者に任せること: `std/Proc.em` の中の補助の関数 (列の組み立て) の書き方
- 条件: いつも行う
- 完了の基準
  - `cargo nextest run` → すべて通る。`cargo clippy --all-targets -- -D warnings` と `cargo fmt --check` → 通る
  - `timeout 20 cargo run -q -p eml_cli -- run tests/ui/run/std/proc_args.em` → `[b c]`、`[--x=out]`、`[1]`、`[2]` をこの順で出す
  - `timeout 20 cargo run -q -p eml_cli -- run tests/ui/run-fail/std/proc_capture_nonzero.em` → ``runtime error: command `sh` exited with code 3`` を出し、終了コード 1
  - `timeout 20 cargo run -q -p eml_cli -- run tests/ui/run-fail/std/proc_capture_not_utf8.em` → ``the output of `sh` is not valid UTF-8`` で止まる
  - `timeout 20 cargo run -q -p eml_cli -- run tests/ui/run/std/proc_stderr.em` → `merge_stderr` の行に標準エラーの文を含み、`discard_stderr` の行に含まない
  - `cargo nextest run -p eml_cli --test integration -E 'test(repl::)'` → 通る。`std/command_literal.repl` が、実行時エラーの次の入力で `Cmd { program = "ls", … }` を表示する
  - `git diff --stat HEAD~1 -- 'crates/eml_cli/tests/snapshots/integration__ui__run@scripts__deploy*'` → 空 (deploy の出力は変わらない)
  - `cargo nextest run -j 1 -p eml_cli --test integration -E 'test(ui::)'` → `SLOW` を出さない (`proc_spawn_large.em` を含む)
  - `grep -rn 'Proc.run "' tests std docs/spec crates` → 何も出さない

### T-5: `Host::run` の削除

- 目的: 使われなくなった `run` の系統を消す。依存の鎖のため、T-4 の後でなければ通らない。
- 対象: `crates/eml_rt/src/host.rs`、`heap/ctx/script.rs`、`trust.rs`、`heap/tests/host.rs`、`crates/eml_vm/src/hosts.rs`、`expected.txt`、docs/spec/object-model.md
- 作業
  - 次のものを消す。
    - `Host::run`、`StdHost::run`
    - `host_run`
    - `Ctx::proc_status`
    - `CaptureHost::run`
    - `heap/tests/host.rs` の `run` の impl (248 行のあたり)
    - `trust.rs` の `host_run` の行
    - `expected.txt` の `proc_status` の行
    - object-model.md の `run` の記述
  - `eml_rt` の門を、もう1度通す。
- 実装者に任せること: なし
- 条件: いつも行う
- 完了の基準
  - `cargo nextest run` → すべて通る
  - `cargo verus verify -p eml_rt`、`verusfmt` の確かめ、Miri の2つの設定 (T-3 のコマンド) → 通る
  - `git grep -n 'Proc.run "\|Host::run\|host_run\|proc_status\|ProcStatus' -- tests std docs crates ':!docs/plans' ':!docs/reports'` → 何も出さない
- テストの変更の種類: 種類2 は `expected.txt` の行の削除。ほかは出力の変わらない削除 (種類3)。

### T-6: 段を閉じる

- 目的: roadmap の「進め方」の閉じる作業を、1つのコミット `Close command literals` にまとめる。
- 作業
  1. `spec/` を仕上げる。段の名前と「今は〜、後で〜」の予告が残っていないことを確かめる。
  2. 計測する。
     - `bench/scripts.sh` を流し、実時間が範囲を外れた行があれば、大きさを変えて別のコミットにする。
     - 次に `bench/run.sh`、`bench/rt.sh`、`bench/scripts.sh` を、コードの最後のコミットの作業ツリーで流し、docs/implementation/benchmarks.md の「今の数値」の3つの表を入れ替える。
  3. docs/implementation/status.md の範囲と制限を直す (コマンドリテラル、`Proc` の API、後に回したもの)。
  4. roadmap
     - 段の節と、段の列の行を消す。
     - D-11 の項目を「その後の項目」へ移す。
     - 「フォーマッタ」の「前提: コマンドリテラル」を直す。
  5. docs/future/stdlib.md の `Cmd` の項目を、今の状態に直すか消す。
  6. docs/overview.md と docs/README.md の「文書の地図」を、変わった所だけ直す。
  7. `CLAUDE.md` は、規則か道しるべが変わったときだけ直す。
  8. この文書を消す。消す前に、5 章の決めたことがすべて `docs/` にあることを確かめる。
  9. コミットのメッセージに、次のことを書く。
     - 文書で何をどこへ移したか
     - テストの変更の種類と理由 (7 章の各タスクの記録)
     - 計測の目立つ差
     - 後に回した項目を決めた日 (2026-10-23)
- 実装者に任せること: benchmarks.md の「所見」で、この段が変えた規則を根拠にしているものの測り直しの範囲
- 条件: いつも行う
- 完了の基準
  - 9 章の全体の検証 → すべて通る
  - `test -e docs/plans/2026-10-23-command-literals.md || echo gone` → `gone`
  - `grep -n "### コマンドリテラル" docs/future/roadmap.md` → 何も出さない

## 8. 順番

```
T-1 → T-2 → T-3 → T-4 → T-5 → T-6
```

- どれも前のタスクに依存するので、並べて進めない。
- T-3 は `std` と `eml_extern` を変えずに入れる。T-5 は T-4 で `ProcStatus` を消した後でなければ通らない。

## 9. 検証

- ベースライン (最初の変更の前)
  - `cargo nextest run` → すべて通る。件数を記録する (前の段を閉じた時点の数で、基点 2319493b の 3450 件より多い)。
  - 試しの worktree が本体の `target/` に build していたら、`trust::every_source_file_is_listed` が落ちる。そのときは 4 章のとおりに直る。
- 全体の検証 (すべてのタスクの後)
  - `cargo nextest run` → すべて通る
  - `cargo clippy --all-targets -- -D warnings` と `cargo fmt --check` → 通る
  - `cargo clippy -p eml_cli --no-default-features --features types -- -D warnings` → 通る (`none`、`core` の feature も同じ)
  - `cargo verus verify -p eml_rt` → 誤りなし
  - `verusfmt --verus-only --check $(git ls-files 'crates/eml_rt/src/*.rs')` → 何も出さない
  - `nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_rt trusted::` と、`-Zmiri-tree-borrows` を足した同じコマンド → 通る
  - `nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_vm --test integration miri::` (同じ2つの設定) → 通る
  - `cargo nextest run -j 1 -p eml_cli --test integration -E 'test(ui::)'` → `SLOW` を出さない
  - `nix build` → 通る
- 利用者が手で確かめること
  - `eml repl` で `import Proc` を打ち、次に `` Proc.capture `echo hi` `` が `"hi\n"` を返すこと
  - 端末で、`spawn` した子を持つスクリプトを Ctrl-C で止められること (D-5 の、止まったときの規則)

## 10. 危険と未決の項目

| ID | 項目 | 影響 | 決め方と時期 | 決まらないときの既定 |
|---|---|---|---|---|
| Q-1 | Prelude の `pub` でない関数を `Res::Item` で直に指した呼び出しが、名前の検査を通るか (`&&` の脱糖が `lang.and` を直に指すのと同じ道と見込む) | T-2 | 実装者が T-2 で確かめる | 通らなければ `cmd_new` と `cmd_spread` を `pub` にする。Prelude の名前空間に2つ増えることを、コミットのメッセージに書く |
| Q-2 | `Annot` の `TypeRef` を lowering が `extern_type(ExternType::String)` で組めるか | T-2 | 実装者が T-2 で確かめる | 組めなければ、`pub` でない `cmd_arg : String -> String` の呼び出しで穴を包む |
| Q-3 | `handle_write` が `Handle` を受ける形になっているか (前の段の `FileOut` で変えた) | T-3 | 実装者が T-3 の初めに確かめる | なっていなければ、T-3 で引数を `Handle` にする |
| Q-4 | `capture` の標準エラーの既定 | T-4 | 決めてある | 受け継ぐ |
| Q-5 | `Child` の読みの誤りの文言 | T-3、T-4 | 決めてある | ``cannot read the output of `sort`: …`` |
| Q-6 | `proc_spawn_large.em` が1秒に収まるか | T-4 | 実装者が `-j 1` で測る | 収まらなければ、大きさを buffer の4倍 (256 KiB) から2倍 (128 KiB) に下げる。buffer を超えることは保つ |
| Q-7 | `unwind_points` の `scripts/deploy` の値 | T-4 | 実装者 | 測った値で固定し、コミットのメッセージに書く |

## 付録: レビューで変わったこと (蒸し返さないために)

- 穴は、初めは `display` を呼ぶ案だった。`\{xs}` と `\{..xs}` の取り違えを型で止めるため、`String` だけにした (D-2)。
- `read` は `capture` に改名した (D-4)。
- `/` を含む相対のプログラムを、子の作業ディレクトリに対して解決することにした (D-4)。
- コミットを6つに分け直した。`Host::run` の依存の鎖があるので、`spawn` を足すコミットと `run` を消すコミットを分けた (7 章)。
- AST の形を `CommandWord { range, parts, quoted }` に決め、spread の判定を HIR の3つの条件にした (T-1、D-2)。
- pipe の規則を spec に書き、buffer を超える `proc_spawn_large.em` を足した (D-5)。
- `wait_checked` をやめ、失敗を起こすだけの `exited` にした (D-4)。
- `kill` するのは直接の子だけで、`sh -c` の孫は残りうることを書いた (D-5)。
- コンストラクタを公開したままにし、利用者が `Cmd { … }` を書かない規則と、見直す条件を置いた (D-3)。
- 標準エラーの向き (`merge_stderr`、`discard_stderr`) をこの段に入れ、ほかの向きは後に回した (D-4、D-11)。
- 実行の途中で決めたこと (2026-10-23): `Child` の書き込みの誤りは `IoOp::WriteInput` で ``cannot write to `sort`: …`` とし、`FileOut` の文言は変えない。`IoReason` に broken pipe の理由はないので、理由は `I/O error` になる。`child_write` は止める語を見る (D-4、D-5 の `Stopped` の行を満たすため)。
