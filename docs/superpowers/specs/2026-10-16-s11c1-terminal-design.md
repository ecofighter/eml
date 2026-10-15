# S11c-1 端末の REPL の設計

## 目的と範囲

S11c を S11c-1 (端末)、S11c-2 (前端に触れるコマンド `:load` / `:reload` / `:type`)、S11c-3 (`Show` を持たない型の表示器) の3つに分け、この順に進める。各作業が触れる crate と決めることが違うためである。S11c-1 の繰り返しとコマンドの表は、S11c-2 と S11c-3 がそのまま使う。

S11c-1 は、S11a の lib API (`eml_cli::Repl`) を端末から使えるようにする。範囲は次のとおりである。

- `eml repl` のコマンドと、行の編集 (`rustyline`)
- 行を入力にまとめる処理。`input_status` による終わりの判定、コマンド (`:quit`、`:help`)、`:{ … :}` の複数行の入力
- REPL 用の SIGINT のハンドラ。2回目の Ctrl-C でも REPL は終わらない (ロードマップの「S11 REPL」の「決めたこと」)
- 読みの中で待つプログラムを、割り込みで止められるようにすること
- 実行を始める前に来た割り込みで、入力を受け付けないこと (定義だけの入力の前端の検査の間に来たものを含む)
- REPL の確保量の上限の既定の値
- `input_status` が、閉じていないブロックコメントを閉じていない括弧と同じく扱うこと

範囲に入れないもの: `:load` / `:reload` / `:type` (S11c-2)、`Show` を持たない型の表示器 (S11c-3)、ロードマップの「S11 REPL」の「論点」のうち `IO` 以外のエフェクトの既定の handler、凍結した領域の詰め直し、増分の lowering。履歴をファイルに残すこと、トランスクリプトの harness を `Lines` で動かすことも入れない。

端末の規則 (コマンド、行のまとめ方、結果の表示、終わり方、割り込み) は、新しい仕様の文書 `docs/spec/repl.md` (「REPL の端末」) に書く。S11c-2 と S11c-3 は同じ文書に足す。

## コマンドとプロセス

- コマンドは `eml repl [--debug-heap]` である。`--debug-heap` は `eml run` と同じく検査モードにする。ファイルの引数は S11c-2 の `:load` と一緒に入れる
- import の根と Host の相対パスの基準は、`eml run` と同じくカレントディレクトリである。`Repl::new` の `root` は `""` で、読み込み先は `FsProvider::new(Path::new(""))`、Host は `stdio_host(PathBuf::new())` である
- 上限は新しい関数 `eml_cli::repl_limits()` が決める。深さは `default_limits()` と同じ 2^27 語である。確保量は物理メモリの 40% にする。回復は写す間に古いヒープと写しを同時に持ち、写しは根から届く物体だけなので古いヒープより大きくならない。合わせて、`eml run` の上限と同じ 80% を目安に収める。どちらの上限も、安全点で止めるまでの間に少し超えうる目安である (1回の連結や `Fs.read_all` の文字列の写しなど)。物理メモリを読めない OS では、`default_limits()` と同じく上限を置かない
- `rustyline` は `run` の feature の任意の依存 (`dep:rustyline`) にし、workspace の依存に足す。`ctrlc` は workspace の依存からも消す
- 端末の繰り返しは bin (`main.rs` と、新しい bin のモジュール) に置く。行を入力にまとめる処理は lib に置く (下の「行を入力にまとめる」)

### 入力の結果の表示

`Repl::eval` の結果 `Evaluated` を、次のように表示する。

| 結果 | 表示 |
|---|---|
| 診断 | `render(&evaluated.diagnostics, &evaluated.files)` を標準エラーに書く |
| `Empty`、`Accepted`、`Rejected` | 診断のほかに書くものはない。式の値は VM が Host の標準出力に書く |
| `Failed(error)` | `error.interrupted()` なら標準エラーに `interrupted` を書く。ほかは `runtime error: {error}` を書く。`eml run` と同じ形で、位置があれば `  at path:line:column` が続く |
| `Interrupted` | 標準エラーに `interrupted` を書く |
| `Broken(error)` | 標準エラーに `runtime error: {error}` を書き、`Repl::close` を呼ばずに終了コード 1 で終わる。この後の入力はどれも実行できないためである |

- 割り込みは、止まった所によらず `interrupted` と書く。VM の中で止まったときの関数の名前 (`entry$` や `f@[Int]`) は、利用者が書いた名前でないので見せない
- `eval` は実行を終えてから診断を返す。そのため、その入力で初めて読んだファイルの警告 (E4004、E4005) は、入力の出力の後に出る。誤りのある入力は実行しないので、誤りの診断と出力の順は問題にならない。トランスクリプトの表示も同じ順である
- VM は実行の終わりに、誤りのときも Host の出力を書き出す (`Machine::run`)。そのため、入力の出力は次のプロンプトより前に見える。標準出力が端末でなければ Host は 64 KiB ごとにまとめて書くが、実行の終わりに書き出すので、入力ごとの出力の順は保たれる

### 終わり方

- `:quit` と、入力の始めでの EOF (端末では空の行での Ctrl-D) で、`Repl::close` を呼んで終わる。入力の途中の EOF では、`Lines::end` が返したものを処理してから同じく終わる
- 終了コードは 0 である。ただし `close` が誤りを返したら (検査モードのリーク、根を手放すときの誤り)、標準エラーに `runtime error: {error}` を書いて 1 で終わる。入力の誤りや実行時エラー、`missing ':}'` では終了コードを変えない。対話の中の誤りだからである
- 行を読めない誤り (端末の誤り、パイプから読んだ行が UTF-8 でないなど) は、標準エラーに `error: cannot read input: {error}` を書き、`close` を呼んで 1 で終わる。ただし `EINTR` は誤りにしない (下の「端末の繰り返し」)
- 使い方の誤り (clap の誤り) は、今と同じく 2 である

## 行を入力にまとめる

行を入力とコマンドにまとめる処理は、端末に依存しない状態機械として、`eml_cli` の lib の公開のモジュール `eml_cli::lines` に置く。`eml_syntax::input_status` だけを使うので、feature を問わず組み立てる。端末を使わずにテストできるようにするためである。型は crate の根に再公開しない。

```rust
pub struct Lines { /* 読んだ行と、`:{` の中かどうか */ }

pub enum Step {
    /// 次の行を `prompt()` で読む。
    Read,
    /// 入力が終わった。`Repl::eval` に渡す。
    Eval(String),
    /// コマンドを実行する。
    Command(Command),
    /// 入力の誤り (知らないコマンド、`missing ':}'` など)。標準エラーに書き、何も評価しない。
    Error(String),
}

pub enum Command { Quit, Help }

impl Lines {
    /// 1行を渡す。`line` は `\n` を含まない。
    pub fn push(&mut self, line: &str) -> Step;
    /// 1回の読みが返した文字列を `\n` で行に分け、行末の `\r` を除いて順に `push` し、各行の結果を返す。
    pub fn push_text(&mut self, text: &str) -> Vec<Step>;
    /// Ctrl-C。読みかけの入力と `:{` の状態を捨て、入力の始めに戻す。
    pub fn interrupt(&mut self);
    /// EOF。読みかけの入力がなければ `None` を返す。
    pub fn end(&mut self) -> Option<Step>;
    /// 次の行を読むときのプロンプト。入力の始めは `> `、続きと `:{` の中は `. ` である。
    pub fn prompt(&self) -> &'static str;
}
```

- プロンプトは、REPL のトランスクリプトと同じ `> ` と `. ` である
- 入力の終わりは、読んだ行を `\n` でつないだテキストを `input_status` に渡して決める ([レイアウト規則](../../spec/layout.md) の「REPL の入力の終わり」)。空行で終わった入力は、その空行を含めずに `Eval` で返す
- 入力の始めの行が空白だけなら `Read` を返し、入力の始めのままにする。コメントだけの行は入力として評価する (`Repl` が `Empty` を返す)
- 入力の始めの行で、最初の空白でない文字が `:` なら、その行をコマンドとして読む。入力の続きの行と `:{` の中の行は、`:` で始まっても入力の本文である
- コマンドの名前は、`:` の直後から最初の空白の前までの文字である。名前の後の空白でない文字が引数になる。名前が空なら `Error("missing command name")` にする (`:` だけで `:quit` を選ばないため)
- `{` と `}` は特別な名前である。`:{` は下の複数行の入力を始め、引数があれば `Error("':{' takes no argument")` にする。入力の始めにある `:}` は `Error("':}' without ':{'")` にする
- ほかの名前は、GHCi と同じく、表の名前の接頭辞で書ける。表の順に見て、最初に接頭辞が合う名前を選ぶ (`:q` は `:quit`)。S11c-1 の表は `quit`、`help` の順である。S11c-2 は、GHCi と同じ省略 (`:l`、`:r`、`:t`) になるように表に行を足す
- 合う名前がなければ `Error("unknown command ':foo'")` にする。S11c-1 のコマンドは引数を取らないので、引数があれば `Error("':quit' takes no argument")` のように報告する
- `:help` は、コマンドの一覧と `:{ … :}` の使い方を標準出力に書く
- `:{` の後の行から、前後の空白を除くと `:}` だけになる行の前までを、1つの入力にする。空行も入力に含め、`input_status` は見ない。`:}` で閉じたとき、本文が空白だけなら `Read` を返し、そうでなければ `Eval` で返す
- `:{` の中で EOF になったら、`end` は `Error("missing ':}'")` を返し、何も評価しない
- `end` は、読みかけの入力があればそれを `Eval` で返す。終わっていない入力も評価して、その診断を見せる。パイプから読むときに、最後の入力が空行なしで終わっても評価するためである

### 端末の繰り返し

- 標準入力が端末のときだけ `rustyline` (18.0.1、`default-features = false`) を使う。端末でなければ、標準入力の `BufRead::read_line` で1行ずつ読み、プロンプトは書かない。`rustyline` は `TERM` が `dumb` などのときに、標準入力がパイプでもプロンプトを標準出力に書くので、パイプのときの動きを `TERM` によらず決めるためである
- `rustyline` の設定は、`Behavior::PreferTerm` (標準出力をリダイレクトしても、プロンプトと編集の表示を端末に書く) のほかは既定の値である。履歴は既定の `MemHistory` で、読んだ行ごとに `add_history_entry` で足す。既定の設定では、空の行と、直前と同じ行は足さず、100 行まで持つ。GHCi と同じく、行ごとの履歴である
- 1回の `readline` が複数の行を返すことがある。既定の設定で bracketed paste が有効なので、貼り付けた複数行が1つの文字列で返るためである。繰り返しは、返った文字列を `Lines::push_text` に渡し、返った `Step` を順に処理する。`:quit` か続けられない誤りで止まれば、残りは捨てる。`Lines` の状態は評価の結果によらないので、先にすべての行を渡してよい。パイプから読んだ行も同じ道を通るので、行末の `\r` (CRLF の入力) も除く
- `ReadlineError::Interrupted` (編集中の Ctrl-C) と、`EINTR` で戻った誤り (`ReadlineError::Errno(EINTR)` と、`Io` で種類が `Interrupted` のもの) は、どちらも `Lines::interrupt` に渡して次の行を読む。`rustyline` は、端末を raw にする `tcsetattr` を、自分のハンドラを入れる前に `ISIG` が立ったまま呼ぶ。この間の Ctrl-C で下のハンドラが動き、`SA_RESTART` がないので `EINTR` が返りうる。このときも REPL は終わらない
- `ReadlineError::Eof` は `Lines::end` に渡す。ほかの誤りは上の「終わり方」のとおりである
- `rustyline` は行を編集する間だけ端末の `ISIG` を外す。そのため、編集中の Ctrl-C は SIGINT にならず、キーとして `Interrupted` になる。標準出力が端末なら、`rustyline` は編集する間だけ SIGINT と SIGWINCH のハンドラを自分のものに替え、編集を終えると元のハンドラ (下の REPL のハンドラ) を、フラグも含めて戻す
- `TERM` が `dumb` などの端末では、`rustyline` は raw にせず、cooked のまま `read_line` で読む。このとき Ctrl-C は SIGINT になり、ハンドラが止める語を立て、std の `read_line` は読み直す。読みかけの入力は捨てず、止める語は次の入力の始めの `clear` が下ろす
- 標準入力が端末でないときの `read_line` も、`EINTR` で読み直す。待つ間の SIGINT が立てた止める語は、次の入力の始めの `clear` が下ろす

## SIGINT と割り込み

### ハンドラ

`ctrlc` crate をやめ、bin の新しいモジュール (`src/sigint.rs`) で `libc::sigaction` を使って SIGINT のハンドラを入れる。unix だけで入れる。

- `sa_flags` に `SA_RESTART` も `SA_NODEFER` も入れない。`SA_RESTART` を外すのは、ハンドラが動いた後に、待っている読み (`read`) が `EINTR` で戻るようにするためである。`SA_NODEFER` を入れないので、ハンドラが動く間は同じ SIGINT が塞がれ、ハンドラは入れ子にならない。`eml run` の動き方で、止める語を読んでから `_exit` するかを決める間に、別の SIGINT が割り込まないためである (今は `ctrlc` がハンドラを専用のスレッドで順に呼ぶことで、同じことを守っている)
- ハンドラが触るのは static だけである。`Control` は `OnceLock<Arc<Control>>` に、動き方は `AtomicBool` に置く。`OnceLock::get` は初期化の後はアトミックな読みだけで、ロックを取らない。ハンドラの中でするのは、`Control::interrupt()` (止める語へのアトミックな `fetch_or`) と、下の `libc::_exit(130)` だけで、どちらも async-signal-safe である。`errno` を変える関数は呼ばない
- 動き方は2つある
  - `eml repl`: SIGINT のたびに `interrupt()` を呼ぶ。プロセスは終わらない
  - `eml run`: 1回目は `interrupt()` を呼ぶ。止める語に `INTERRUPT` が立った後の SIGINT では `libc::_exit(130)` で終わる。今の `ctrlc` のハンドラと同じ扱いで、まだ書いていない出力は捨てる
- ハンドラは、`eml run` と `eml repl` だけで、`Control` を作った直後に入れる。static に `Control` と動き方を置いてから `sigaction` を呼ぶ。`eml check` には入れないので、Ctrl-C は OS の既定の動作でプロセスを終わらせる。入れられなければ、今と同じく警告を書いて続ける
- unix 以外ではハンドラを入れない。Ctrl-C は OS の既定の動作でプロセスを終わらせる。`eml run` の Ctrl-C で安全点で止まる扱いと、REPL が Ctrl-C で終わらない扱いは、unix だけのものになる。仕様にそう書く
- `src/` の `unsafe` は、`physical_memory` とこのモジュールの中だけに置く

### 読みの中の割り込み

`eml_rt` の `host.rs` の `read_text` は、今は `take(64 KiB).read_to_end` で区切りごとに読む。`read_to_end` は `Interrupted` を自分で読み直すので、`EINTR` が戻っても止める語を見ない。

- 読み手を包む `Read` の adapter (`verus!` の外の安全なコード) を足す。adapter は、中の読み手が `Interrupted` を返し、止める語が 0 でなければ、`Interrupted` でない誤りに変えて返す。止める語が 0 なら `Interrupted` をそのまま返し、`read_to_end` が読み直す
- `read_text` は、区切りの読みが誤りを返したとき、止める語が 0 でなければ `ReadError::Stopped(stop_reason(bits))` を返す。0 なら今と同じく `ReadError::Io` を返す
- 区切りの読みは、今と同じく `take(64 KiB).read_to_end` のままにする。短い読みと、終わりを小さな読みで確かめることは std が受け持つ。自前の `read` の繰り返しにすると、終わりを確かめる小さな読みがなくなり、長さの見込みちょうどに確保した `Vec` が次の区切りで倍に広がって写されるためである
- adapter は `read_buf` を特殊化できない (stable の Rust では、`read_buf` を実装できない)。そのため `read_to_end` は、空き領域を 0 で埋めてから読む。今は `BufReader` が `File` の `read_buf` に渡すので、0 で埋めていない。0 で埋めるのは空き領域の各バイトにつき1回で、費用は読むバイト数に比例する。`bench/rt.sh` の `read-all` と `read-all-unsized` で前後を測り、[計測の基準](../../implementation/benchmarks.md) に記録する
- `read_text` は `#[verifier::external_body]` なので、Verus の証明は変わらない。`fs_read_all` の `ensures` は、もともと `Fault::Stopped` を許している。`File` を閉じる扱い (止めたときは `file` を手放して閉じ、結果を作らない) も変えない
- `read_text` は信頼の一覧の T11 (Host の境界) に入る。T11 の行の「Miri で確かめること」に、止める語が立った後の `Interrupted` を足し、`heap/tests/trusted.rs` の `files_go_through_the_host` にその場合を足す。adapter は、「`unsafe` の核」の安全なつなぎの一覧に足す。新しい `external_body` は足さないので、`src/trust.rs` は変わらない
- [オブジェクトモデル](../../spec/object-model.md) の「`File`」の `Fs.read_all` の手順 4 と、`io_reason` の doc コメント (「`Interrupted` は `read_to_end` が読み直すので、ここに届かない」) を、この扱いに合わせて書き直す

REPL のプロセスはほかのスレッドを持たないので、SIGINT は読んでいるスレッドに届き、`read` は `EINTR` で戻る。`rustyline` も、`ExternalPrinter` を使わなければスレッドを作らない。`eml run` も、`ctrlc` の専用のスレッドがなくなるので同じである。

### 実行を始める前の割り込み

`Outcome` に `Interrupted` を足す。`Repl::eval` は、前端の検査に誤りがなければ、`Machine::load` の前に止める語を読む。`INTERRUPT` が立っていれば、入力の種類によらず受け付けずに `Interrupted` を返す。定義だけの入力も、実行する入力 (式と `let`) も同じである。セッションは変わらない。

- 実行する入力を `load` の前に止めるのは、VM が入口では安全点を通らないためである。入口から最初の呼び出しまでの extern (`println`、`Fs.open`) と `root_set` は、止める語が立っていても動く
- 誤りのある入力は、今と同じく `Rejected` で、診断を返す
- `load` の間と実行の間に来た割り込みは、今と同じく VM の安全点か `Fs.read_all` で止まり、`Failed` になる。最後の安全点の後に来た割り込みでは、入力は終わりまで動き、`let` の束縛もできる
- `readline` が行を返してから `eval` の始めの `clear` までに来た割り込みは、`clear` が捨てる

### 残る制限

Ctrl-C で REPL から抜けられない処理が残る。`eml run` は2回目の Ctrl-C で抜けられるが、REPL は終わらないためである。抜けるには、Ctrl-\ (SIGQUIT。既定の動作でプロセスを終わらせる) か `kill` を使う。仕様に次のものを並べて書く。

1. 前端の検査と回復。どちらも有限の時間で終わるので、終わるのを待つ。検査の間の割り込みは、上の「実行を始める前の割り込み」のとおり入力を取り消す
2. import で読むファイルが FIFO か遅いファイルのとき (`FsProvider` の `read_to_string` は `EINTR` で読み直す)
3. 書き手のいない FIFO を開く `Fs.open`。std の `File::open` が `EINTR` で開き直すためである。止めるには `O_NONBLOCK` で開いてから `fcntl` で戻す必要があり、`eml_rt` に `libc` の依存が要る。S12 の `Stdin` は何も開かないので、この制限を受けない
4. 詰まった標準出力への書き込み (`eml repl | less` で `less` を止めたときなど)。std の `write_all` が `EINTR` で書き直すためである
5. SIGINT が、止める語を見た後で `read` のシステムコールに入る前に届くと、`read` は待ち続ける。次の SIGINT の `EINTR` で止まる。防ぐには `ppoll` か self-pipe が要るので、今は入れない
6. 後の並列 VM で worker のスレッドを持つと、SIGINT が読んでいないスレッドに届きうる。そのときは、読まないスレッドで `pthread_sigmask` により SIGINT を塞ぐ。[マルチコア対応](../../future/multicore.md) に書く

## 閉じていないブロックコメント

`input_status` は、閉じていない `{-` を終わっていない入力とみなさない。字句解析が E0005 (`UNTERMINATED_BLOCK_COMMENT`) を出すだけで、`open_at_end` は文字列にしか立たないためである。そのため、端末で複数行のブロックコメントを書けない。

- [レイアウト規則](../../spec/layout.md) の「REPL の入力の終わり」の規則 1 に、閉じていないブロックコメントを足す。空行が来ても続く
- 字句解析は、補間の穴の外のブロックコメントが閉じずにテキストの終わりに着いたときにも `open_at_end` を立てる。穴の中のコメントは、今も改行の手前で止まって穴と一緒に閉じる ([字句](../../spec/lexical.md) の「改行での回復」) ので、数えない。1行の文字列と補間の穴を数えない規則 1 の扱いと同じである
- `eml_syntax` の `input_status` のテストに、閉じていないブロックコメントと、閉じた複数行のブロックコメントを足す

## テスト

テストを先に書く。既存のテストの期待値と成否は変えない。

- `eml_rt` (両方のモード、`heap/tests` の偽の reader): 止める語を立ててから `Interrupted` を返す reader では、`fs_read_all` が `Fault::Stopped(Stop::Interrupted)` を返し、`File` を閉じる。止める語を立てずに `Interrupted` を1回返す既存のテスト (`interrupt_once`) は、そのまま読み直しを確かめる。Miri では `trusted.rs` の `files_go_through_the_host` に同じ場合を足す。`cargo verus verify -p eml_rt`、`verusfmt --verus-only --check`、`bench/rt.sh` も流す
- `eml_syntax`: 上の「閉じていないブロックコメント」のテスト
- `Lines` (新しいテストのファイル `crates/eml_cli/tests/lines.rs`。`tests/main.rs` に足す)
  - 入力の終わり: 1行で終わる入力、空行で終わる入力と空行を除くこと、閉じていない括弧で続く入力、空白だけの行、コメントだけの行
  - コマンド: 接頭辞 (`:q`、`:h`)、`:` だけと `:   `、知らないコマンド、引数のあるコマンド、続きの行の `:` で始まる行
  - `:{ … :}`: 空行を含むもの、空のブロック、前後に空白のある `:{` と `:}`、`:{ foo`、入力の始めの `:}`、`:{` の中の `interrupt`、`:{` の中の EOF
  - `interrupt` で読みかけを捨てること、EOF での読みかけの評価、読みかけがないときの `end` の `None`、各段階の後の `prompt()`
- `Repl` の `Interrupted` (`crates/eml_cli/tests/api.rs`)
  - `read` の中で `Control::interrupt()` を呼ぶ `ModuleSource` を使う。`import Util` と関数の定義を持つ入力が `Interrupted` になり、次の入力でその関数を参照すると E1001 になる
  - 止める語を読むのは、前端の検査と lowering の後、`load` の前の1か所で、入力の種類によらない。式の入力は import を持てず、前端の検査の間に割り込みを立てる決まった道がないので、この1か所を定義の入力で確かめる
  - `repl_limits()` が深さと確保量を抑えること (`default_limits_bound_the_depth_and_the_memory` と同じ形)
- プロセスのテスト (新しいファイル `crates/eml_cli/tests/terminal.rs`、標準入力をパイプにする): 式の値、定義と後の入力での使用、標準エラーの診断、実行時エラーの後も続くこと、`:quit` と EOF で 0 で終わること、`--debug-heap` でリークがなく 0 で終わること、知らないコマンド、`:{ … :}`、`:help`、パイプのときプロンプトを書かないこと
- SIGINT のテスト (`cfg(unix)`、`terminal.rs`。`cli.rs` の `sigint`、`kill_after_deadline`、`DEADLINE` を `pub(crate)` にして使う)。どのテストも、標準入力を開いたままにし、最初に `1` を評価して標準出力に `1` が出るのを待ってから SIGINT を送る。ハンドラを入れる前の SIGINT は、既定の動作でプロセスを終わらせるためである。標準出力は別のスレッドで読み続ける。パイプが詰まると、子が書き込みの中で止まって安全点に来ないためである
  1. 終わらないループを実行する入力を書き、`interrupted` が標準エラーに出るまで 100 ms ごとに SIGINT を送る。出たら送るのをやめてから `1 + 1` を書き、`2` が出ることを確かめる。REPL では SIGINT を何回送っても終わらないので、上の「残る制限」の 5 の競合があっても揺れない
  2. 書き手が開いたまま何も書かない FIFO を `Fs.read_all` で読む入力を書き、1 と同じく SIGINT を送る。`interrupted` の後もセッションが続く
  3. 入力を待つ間に SIGINT を何回か送った後の `:quit` で、0 で終わる
- 既存の `eml run` の FIFO のテスト (`repeated_interrupts_end_a_program_waiting_in_a_read_with_130`) の期待値は変えない。終わるまで SIGINT を送り続け、終了コード 130 だけを確かめるので、読みが割り込みで止まっても、2回目の SIGINT で終わっても通る

### 機械的な直し

期待値を変えない次の直しは、合意なしで行い、コミットのメッセージに書く。

- `crates/eml_cli/tests/repl.rs` の harness は `Outcome` を網羅して照らすので、`Interrupted` の腕を足す。トランスクリプトでは起きない
- `repeated_interrupts_end_a_program_waiting_in_a_read_with_130` の名前と doc コメントは「2回目で終わる」と言うので、doc コメントを今の扱いに合わせる
- `lib.rs` の `physical_memory` と `main.rs` の、`ctrlc` と `unsafe` の置き場所を言うコメント

## 文書の更新

作業の終わりに、決めたことを次の文書に移し、この設計書と計画を消す。

- 新しい [REPL の端末](../../spec/repl.md): コマンドの表と名前の規則、行のまとめ方 (`:{ … :}` を含む)、プロンプト、結果の表示、終わり方と終了コード、割り込みの扱い、残る制限。[README](../../README.md) の一覧に足す
- [ロードマップ](../../future/roadmap.md) の「S11 REPL」: 表の S11c の行を S11c-1、S11c-2、S11c-3 に分け、S11c-1 を「終えた」にする。「S11c に渡すこと」の3項目を、決めたこととして書き直す。Ctrl-C の扱いが unix だけのものであることも書く
- [オブジェクトモデル](../../spec/object-model.md): 「実行の制御」(読みの中の割り込み、`ctrlc` をやめたこと)、「`File`」の `Fs.read_all` の手順 4、「`unsafe` の核」の T11 の行と安全なつなぎの一覧
- [VM](../../spec/vm.md): 「上限と割り込み」(2つの動き方、`SA_NODEFER` を入れない理由、`repl_limits()`、unix だけであること)、「追記」(定義だけの入力の間の割り込みを次の `clear` が捨てる、という文を、`Interrupted` で受け付けない扱いに改める)
- [モジュールと名前解決](../../spec/modules.md) の「REPL のセッション」: `Outcome::Interrupted`
- [レイアウト規則](../../spec/layout.md) の「REPL の入力の終わり」: 規則 1 のブロックコメント。「空行を含む貼り付けは、端末の `:{ … :}` で入力する (S11c)」の S11c を、REPL の端末の文書への参照に替える
- [テスト戦略](../../implementation/testing.md): 「CLI のテスト」(REPL のプロセスのテスト、SIGINT のテストの本数と、FIFO のテストの説明「2回目以降の SIGINT でハンドラが終わらせる」)、「`eml_vm` のテスト」の SIGINT に触れる行、「REPL のトランスクリプト」の api.rs の一覧、`Lines` のテスト
- [アーキテクチャ](../../implementation/architecture.md): 「crate の依存関係」(`ctrlc` と `libc` のハンドラ)、「外部 crate」(`ctrlc` の行を消し、`rustyline` の行を足し、`libc` の用途を改める)、「CLI と lib API」(`eml repl`、`eml_cli::lines`、REPL の終了コード、`repl_limits()`)
- [計測の基準](../../implementation/benchmarks.md): `read-all` と `read-all-unsized` の前後の数
- [状態](../../implementation/status.md)、[マルチコア対応](../../future/multicore.md) (上の「残る制限」の 6)
- `CLAUDE.md`: `ctrlc` の記述、`src/` の `unsafe` の置き場所、`eml repl` のコマンド、SIGINT のテストの数と終了コード
