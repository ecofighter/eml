# async-files の設計と計画

- 作成: 2026-11-04
- 基準: main @ 949edf5c (「3. 現状」と、この文書と付録の `file:line` はこのコミットを指す)
- 状態: 計画は確定。G-7 のファイルについての改訂は利用者が承認した (2026-11-04)
- 付録 (`docs/plans/2026-11-04-async-files/`、英語):
  - `design.md`: 長期の方針の本文。std の API と意味、driver の仕組み、適合の条件 G-1〜G-12 (§8)
  - `commits.md`: コミットごとの実装の設計。この文書の T-1〜T-13 は、その Commit 1〜13 である。型、extern の行、記録の変更、std の包みの下書き、spec の節、テスト、確かめのコマンドがある
  - `ruling.md`: G-7 (ファイル) と G-12 (結果の番号) の裁定。Q1 の改訂は承認済みで `design.md` に入っている

この文書は判断と順番をまとめ、細部は付録に任せる。付録とこの文書が食い違ったら、この文書を正とし、食い違いを報告する。付録の中では、確かめのコマンドは `commits.md` の §5b を正とする。

## 1. 目的と完了の条件

- 目的: 通常のファイルと、子のプロセスの標準入出力を I/O の driver に載せ、`<IO, Async>` の操作にする。タスクがファイルや子を待っても、同じ worker のほかのタスクは止まらない。あわせて、今のブロックする std の行では書けない次の3つを書けるようにする
  - パイプライン: 子の標準出力を次の子の標準入力へつなぐ
  - `run` と `output` の標準入力を文字列で与える
  - `Child` の標準エラーを標準出力と別に読む
- 完了の条件:
  - 上の3つが UI テストで動く (同期の `Proc` の `proc_pipeline.em`、`proc_stdin_text.em`、`proc_redirect.em` と、`Std.Exec` の `exec_*.em`)
  - `Std.Stream` でのファイルの読み書きが、仮想の時計の上で、seed と worker の数 N = 2、8 で決定的に動く (`stream_*.em`)
  - 今のブロックする `FileIn`、`FileOut`、`Fs`、`Stdin`、`Stdout`、`Stderr`、`Proc` の UI テストの期待値が、`Cmd` の表示の2つを除いて変わらない
  - 「9. 確かめ方」の全体の確かめがすべて通る

## 2. 範囲

- 範囲に入るもの:
  - `Cmd` の標準入力と標準出力の向き (文字列、ファイル、前の段の `Cmd`、ファイルへの書き出しと追記)、演算子 `.|`、`Proc.wait_all`
  - 新しいモジュール `Std.Stream` (ファイルと子のパイプに共通の `Reader` と `Writer`) と `Std.Exec` (非同期の子)
  - driver の新しい操作: パイプ、子の終了の待ち (`Op::Wait`)、ファイルの取り込み (`Op::Adopt`)、通常のファイルの読み書きのプールの仕事
  - 3つの OS の backend (epoll、kqueue、IOCP) とテストの driver
  - 貸した記憶の規則 (object-model.md の T10 の行、vm.md 「時計と driver」) の改訂と、その Miri のテスト
  - UI テストの印 `-- clock: real (<理由>)`
- 範囲に入らないもの (どれも閉じるコミットで roadmap の「その後の項目」に移す):
  - バッファを持つ非同期の `Writer` と `flush`
  - `Stream` の `_within` の読み
  - `Exec.output_err` (標準出力と標準エラーを1回で読む便利な関数)
  - パイプラインの途中の段の標準エラーを読むこと
  - 非同期の `Fs` の関数、seek、位置を指定する読み書き
  - Linux 5.3 より前のカーネル (pidfd がない) での `Exec.wait`。`NotAvailable` で失敗させる
  - Windows の Ctrl-C で、子のパイプを読むタスクが止まることを確かめるテスト。`cli.rs` にコンソールのイベントを起こす仕組みがないためである
  - 非同期の `Stdin`。Windows のコンソールの標準入力は overlapped にできず、3つの目的にも入らない
  - ネイティブのコード、`ffi-blocking`、io_uring

## 3. 現状

実装者が知っておくべき今の形を並べる。調べた事実の全体は付録 `design.md` と `commits.md` に `file:line` つきである。

- driver の型 (`IoDriver`、`Op`、`Done`、`RawBuf`、`HandleBin`) は `crates/eml_rt/src/driver.rs` にあり、`eml_io` にはない。`Op` は `Readable`、`Read`、`Write`、`Listen`、`Accept`、`Connect`、`ShutdownWrite`、`SetNodelay`、`Job` で、ファイル、パイプ、プロセスの操作はない (`crates/eml_rt/src/driver.rs:36`)
- `eml_io` の Unix の backend が扱うのは TCP のソケットだけで、`recv`、`send`、`MSG_PEEK` を使う。IOCP の backend にもパイプとファイルのコードはなく、`windows-sys` の feature も足りない (`crates/eml_io/Cargo.toml`)
- プールは driver ごとに32本までのスレッドを持ち、仕事 (`Op::Job`) は `io::Result<usize>` だけを返す。取り消しは効かず、`quiesce` は終わるまで待つ。今は VM が `Op::Job` を出す所はない
- 貸した記憶 (`RawBuf`) を使ってよいのは driver を持つスレッドだけという規則がある (docs/spec/vm.md 「時計と driver」、docs/spec/object-model.md 「`unsafe` の核」の T10 の行)。`RawBuf` は `Send` なので、コンパイラはこの規則を確かめない
- `Std.Net` の包みは、取り消しの確かめ、`submit_*`、`io_wait`、`op_stream` と `op_outcome`、`take_*` か `finish` の順で書いてある (`std/Net.em:100-119`)。記録、`Bind`、`NetStream` は `crates/eml_vm/src/cells.rs` にある
- 縛っていないストリームを、I/O を受け持たない worker で使うと内部の誤りになる (`crates/eml_vm/src/cells.rs:4175-4179`)。仮想の時計では worker 0 だけが I/O を受け持つ
- 今の `Proc` と `Child` は `Host` (`crates/eml_rt/src/host.rs`) の上のブロックする行で、`Spawned` は標準エラーの読み手を持たない。`Child` を手放すと、パイプを閉じ、子を kill し、待つ
- `Cmd` は Prelude のレコードで、`program`、`args`、`cwd`、`env`、`stderr` (`StderrMode`) を持つ (`std/Prelude.em:259`)。`.|` は今の文法にはないが、演算子の文字の並びなので、1つの演算子のトークンとして字句解析される (docs/spec/lexical.md:46-47)
- UI テストはいつも仮想の時計とテストの driver で流れ、Host は `CaptureHost` である。`CaptureHost` は本物の子 (`eml` 自身) を起動し、`FileOut` の書き込みをメモリに持つ。`eml run` と REPL は本物の時計を使う
- `ui::run` の実時間は、すでに5秒の条件の前後にある (docs/future/roadmap.md の「その後の項目」の「`ui::run` の実時間を5秒の条件に収めること」)

## 4. 前提の環境

| 要るもの | 用途 | 確かめ方 | ないとき |
|---|---|---|---|
| nix の devShell (`direnv` か `nix develop`) | すべて | `nix develop -c cargo nextest --version` → 版が出る | 止めて利用者に聞く |
| `.#miri` の shell | T-3〜T-5、T-7、T-11 の Miri、AddressSanitizer、ThreadSanitizer | `nix develop .#miri -c cargo miri --version` → 版が出る | 止めて利用者に聞く |
| `.#cross` の shell | 各コミットの Windows と Linux の clippy | `nix develop .#cross -c sh -c 'ls $(rustc --print sysroot)/lib/rustlib/'` → `x86_64-pc-windows-msvc`、`x86_64-unknown-linux-gnu`、`aarch64-unknown-linux-gnu` が並ぶ | 止めて利用者に聞く |
| Verus (`cargo verus`、`verusfmt`) | `eml_rt` を変える T-2〜T-5、T-11 | `cargo verus --version` → 版が出る | 止めて利用者に聞く |
| GitHub の CI (Windows、Linux x86_64 と aarch64) | IOCP と Windows のテスト、Linux の pidfd、ThreadSanitizer、x86_64 Linux の AddressSanitizer | PR を開いて CI が走ること | 利用者の決め (2026-11-04): T-1 の後でブランチを push して draft の PR を開き、各作業を push して CI で確かめる。CI の失敗は次の作業に進む前に直す |

## 5. 決めたこと

### 5.1 長期の方針 (変えるときは見直しが要る)

実装がこれらとぶつかったら、止めて報告する。それぞれの本文、理由、退けた案は `design.md` にある。適合の条件 G-n の確かめのコマンドは `commits.md` の §5b にある。

#### D-1: ブロックする行を残し、非同期の行は新しいモジュールに置く (G-1)
- 決め: `FileIn`、`FileOut`、`Fs`、`Stdin`、`Stdout`、`Stderr`、`Proc` は名前、ハンドルの型、`<IO>` の行を保つ。非同期の I/O は `Std.Stream` (`Reader`、`Writer`) と `Std.Exec` (`Child`) に置き、待つ行は `<IO, Async>` にする。同期と非同期でハンドルの型を共有しない。`Cmd` は両方で共有する
- 理由: スクリプト、REPL、`bench/scripts.sh` は `Async.run` なしでファイルと子を使う。`<IO>` の行は driver を待てない
- 退けた案: 同期の行を非同期に置き換える案。どのスクリプトも REPL も `Async.run` が要り、Windows のコンソールの標準入力は非同期にできない
- 残す場所: docs/spec/stdlib.md 「`Std.Stream`」「`Std.Exec`」(T-7、T-11)

#### D-2: パイプラインは `Cmd` で表し、1回の Host の起動で OS のパイプでつなぐ (G-2)
- 決め: `Cmd` の標準入力の向きに `StdinFrom Cmd` を持たせ、`.|` (`infixl 2`、`Std.Proc`) は右の `Cmd` の先頭の段の入力に左をつなぐ。全部の段を1回の `Host::spawn` で `SPAWN_LOCK` の下で起動し、段の間のパイプの端を親は1本も持たない。入力の設定 (`.|` を含む) は先頭の段に、ほかの設定は最後の段に当たる
- 理由: どの起動の形もパイプラインを型を変えずに受ける。読み手が先に終わったとき、書き手に EPIPE か SIGPIPE が届く
- 退けた案: 文法に `.|` を足す案 (演算子として字句解析されるので要らない)、`List Cmd` の別の型 (起動の関数がすべて二重になる)、eml のタスクでバイトを汲む案 (同期の `Proc` で使えない)
- 残す場所: docs/spec/stdlib.md 「`Proc`」、docs/spec/expressions.md 「コマンドリテラル」(T-3、T-4)

#### D-3: 標準入力の文字列は、すぐに消す一時ファイルで渡す (G-3)
- 決め: `Proc.stdin_text` の文字列は、Host が std だけで作る一時ファイルに書いて子の標準入力にする。eml はパイプやスレッドで文字列を書かない。子からは seek できる通常のファイルに見える。一時ファイルを作れないときは起動の誤りで、理由は `Proc` の起動の誤りの4つの決まった語にする
- 理由: 読みながら書く子でも、どの大きさでも詰まらない。同期と非同期で同じに動く
- 残す場所: docs/spec/stdlib.md 「`Proc`」(T-3)

#### D-4: 子の終了は OS の終了の知らせで待ち、driver は子を回収しない (G-4)
- 決め: `Exec.wait` は `Op::Wait` を使う。Linux は pidfd、macOS は kqueue の `EVFILT_PROC` と、登録の後の `waitid(WNOWAIT)`、Windows は `RegisterWaitForSingleObject` である。プールのスレッドで wait しない。回収は、知らせの後に持ち主の worker の上で Host の `Process::wait` が行う
- 理由: 子ごとにプールの仕事にすると、32本のスレッドを使い切り、取り消せない。REPL の `quiesce` は取り消しを要る
- 残す場所: docs/spec/vm.md 「時計と driver」、docs/spec/stdlib.md 「`Std.Exec`」(T-8、T-9、T-11)

#### D-5: 通常のファイルの読み書きは、どの OS でも貸した記憶を使うプールの仕事にする (G-5)
- 決め: ファイルの `Read` と `Write` はプールの仕事で、OS の呼び出しの中で `RawBuf` のポインタを直に使い、スライスを作らない。そうした仕事は driver を落とすときに `Memory` として保たれる。パイプとファイルには `Readable` を出さず、受け付けのときに buffer を広げる
- 理由: ソケット、パイプ、ファイルで記録の形が1つになり、写しも要らない
- 残す場所: docs/spec/vm.md 「時計と driver」(T-5)

#### D-6: `Stream.open` は通常のファイルだけを開く (G-6)
- 決め: `O_NONBLOCK | O_NOCTTY | O_CLOEXEC` で開いてから `fstat` で確かめ、通常のファイルでなければ、プールの仕事を出す前に種類 `NotAvailable` で断る。ディレクトリは、どの OS とどの開き方でも ``cannot open `<path>`: not a regular file`` にする
- 理由: FIFO や端末や `/dev/stdin` は、プールのスレッドを止めたままにする
- 残す場所: docs/spec/stdlib.md 「`Std.Stream`」(T-7)

#### D-7: パイプは遅れて取り込み、ファイルは開くときに縛る (G-7、改訂済み)
- 決め: パイプの端は縛らずに作り (`Bind::Unbound`)、最初に操作を出した worker の driver が取り込む。ファイルは Host が開き (`Host::open_raw`)、開くときに `Op::Adopt { res: RawResource::File }` で、開いたタスクの I/O の host の driver に縛る。`connect` の結果と同じである。Unix の親の端は `O_NONBLOCK`、子の端は blocking にする。Windows の親の端は overlapped の名前付きパイプ、子の端は同期で、作るときは継承させない。段の間のパイプは std の `std::io::pipe()` の端にする
- 理由: 読むタスクの worker にパイプを置ける。仮想の時計では worker 0 だけが I/O を受け持つので、ファイルを遅れて取り込むと N = 2、8 で動かない (`ruling.md` Q1)
- 退けた案: ファイルの取り込みを worker 0 へ mail で回す案と、`bind_stream` が worker 0 の driver に触る案 (`ruling.md` Q1 に根拠がある)
- 残す場所: docs/spec/vm.md 「時計と driver」(T-5、T-7、T-8)

#### D-8: テストの driver はファイルを扱い、子を扱わない。子のテストは本物の時計で流す (G-8)
- 決め: テストの driver はファイルの操作を `Job` と同じ扱い (取り消せず、`wait` と `quiesce` で片づく) で本物のファイルに読み書きする。パイプ、パイプの取り込み、`Op::Wait` は `Unsupported` で断る。非同期の子の UI テストには先頭の印 `-- clock: real (<理由>)` を付け、子の起動は3回まで、実時間の待ちはなし、検査モードと速いモードだけで流す (seed、worker の数、`RunStats` の比較、横取りの確かめはしない)。`Exec.spawn` は仮想の時計では、どの worker でも ``cannot run `<prog>`: the test driver has no processes`` (`NotAvailable`) で失敗する
- 理由: ファイルのテストを seed と N = 2、8 で決定的に流せる。本物の子は仮想にできない。CI はやり直さない
- 残す場所: docs/implementation/testing.md 「UI テスト」「seed で流す」(T-6、T-10)

#### D-9: UI テストが書いたファイルはリポジトリの外に置く (G-9)
- 決め: `CaptureHost` は書いたファイルを、最初の書き込みで作る一時ディレクトリに置き、`--- written:` のスナップショットに合わせて見せる。最後の `Written` の写しを落とすときに消す
- 残す場所: docs/implementation/testing.md 「UI テスト」(T-1)

#### D-10: 貸した記憶の規則は、最初のファイルの仕事と同じコミットで書き直す (G-10)
- 決め: object-model.md の T10 の行と vm.md の `RawBuf` の箇条を、プールのスレッドも書き手に入れ、mutex がつくる happens-before の辺を書いて改める。T-5 で、最初のファイルの仕事、2つの文書、`eml_rt` の `trusted::` と `eml_io` の `driver::tests::` の Miri のテストを一緒に入れる
- 理由: この規則は文書と Miri でしか守られない
- 残す場所: docs/spec/object-model.md、docs/spec/vm.md (T-5)

#### D-11: パイプラインの終了コードは最後の段のもの。すべての段を回収する (G-11)
- 決め: `wait`、`run`、`output`、`capture` は最後の段の終了コードを返し、`wait_all` は全部の段のコードを先頭から返す。`wait`、`wait_all`、手放しはすべての段を回収し、手放しはまだ回収していない段をすべて kill する。途中の段の標準出力のリダイレクトと、`Exec.spawn` の最後の段以外の `PipeStderr` は起動の誤りにする。`pipefail` は入れない
- 残す場所: docs/spec/stdlib.md 「`Proc`」「`Std.Exec`」(T-4、T-11、T-12)

#### D-12: `Stream` と `Exec` の待つ関数は `Std.Net` の包みの形に従う (G-12)
- 決め: 取り消しの確かめ、`submit_*`、`io_wait`、`op_*`、`take_*` か `finish` の順に書き、結果の番号 0〜6 をそのまま使う (`BrokenPipe` は 2)。`OpId` を `pub` にしない。`Op::Wait` の記録は持ち主と host が同じ worker である。各モジュールは記録の prefix を読む行 `op_prefix` と、モジュールの中だけの `fail_code` を持つ。`op_failed` は作らない
- 理由: cells.rs の記録の手順を1つに保つ。救済の前提 (終わっていない記録を持つタスクはその待ちにいる) は、受け付けと待ちが隣り合うときだけ成り立つ
- 残す場所: std/Stream.em、std/Exec.em、docs/spec/stdlib.md (T-7、T-11)

### 5.2 短中期の設計

理由があれば実装の中で変えてよい。変えたら、コミットのメッセージに書く。本文は `commits.md` の §1 の表 D1〜D11 にある。

- D-20 (`commits.md` D1): 段の並びの起動の誤りは `Proc.launch_args` が eml で `fail_io Other` を起こす。Host の側の起動の誤りは `LaunchError { stage, cause }` を持ち、`Fault::Io { op: Run }` か新しい `Fault::Launch` に写す
- D-21 (D2): `Proc.launch_args` を `pub` にし、「`Std.Exec` のためのインターフェース」と書く。そのコメントに `Async` の語を書かない (G-1 の grep のため)
- D-22 (D3): D-7 のファイルの縛り方。開いたタスクは、I/O の host がほかの worker のときだけmail の往復で待ち、`connect` と同じく片づく。待っている間は救済されない
- D-23 (D4): `NetStream` を `IoStream { bind, origin, rest, start }` に置き換え、`TcpStream`、`Stream.Reader`、`Stream.Writer` が共有する。eml の型は別々のまま
- D-24 (D5): D-12 の結果の番号と `op_prefix`
- D-25 (D6): `Std.Stream` と `Std.Exec` の行はすべて `is_async()` に入れ、`Async` の腕で動かす。`<IO>` の行 (`submit_open`、`stdin`、`stdout`、`stderr`、`reap`) も同じである
- D-26 (D7): `CaptureHost` の書いたファイルの置き場 (D-9 の実装)
- D-27 (D8): 同期の `Proc.wait` はすべての段を先頭から回収する。`Proc.wait_all` は新しい行 `stage_count` と `wait_stage` の上の eml である。パイプラインで詰まるのは、最後の段の標準出力を読み切らずに待つときだけで、今の規則「`wait` の前に標準出力を読み切る」をパイプラインにも書く
- D-28 (D9): 起動の誤りは4つの決まった理由の語を使い、`Stream` と `Exec` の読み、書き、待ちの番号 6 の誤りは `Net` と同じく OS の文言を保つ
- D-29 (D10): `SPAWN_LOCK` は葉の lock である。持つのは `IoDriver::pipe` (1組のパイプを作る間) と `Host::spawn` (最初のファイルかパイプから最後の起動が戻るまで) だけで、持っている間はほかの lock、コールバック、スレッドの作成、書き出しをしない。`launch` は持たない。eml の外のスレッドが fork することは覆わないと spec に書く
- D-30 (D11): ディレクトリの扱い (D-6)
- D-31: `Exec.output`、`output_bytes`、`capture` は、標準出力をリダイレクトしたとき `Proc` と同じく何も読まず、空の文字列か空のバイト列を返す
- D-32: SIGPIPE。eml は Rust の既定の `SIG_IGN` のままで、子には std が `SIG_DFL` に戻して渡す。埋め込む側が SIGPIPE を無視したままにすることを spec に書く

## 6. 実装の約束

- 各コミットは、その木の上で次を満たす (`commits.md` の「Commit sequence」の下の段落)。`cargo nextest run` が通る。`cargo clippy --all-targets -- -D warnings && cargo fmt --check` がきれいである。CLAUDE.md の feature の組み合わせの clippy と、Windows と2つの Linux の cross clippy がきれいである。`cargo nextest run -p eml_cli --test integration -E 'test(citations::)'` が通る
- `pub` でない項目、variant、field は、それを読むか作るコミットより前に足さない。`-D warnings` の dead code で落ちるためである
- `crates/eml_rt` を変えるコミット (T-2〜T-5、T-11) は crates/eml_rt/CLAUDE.md の関門を通す。Verus、verusfmt、Miri の `trusted::` (2つの設定) である。どのコミットで何を流すかは `commits.md` §5 の Q4 にある (D-10)
- `eml_io` の核は OS に依らないままにし、OS のコードは `sys/` に置く。新しい `unsafe` は `src/trust.rs` に載せたファイルにだけ書く (D-5、D-7)
- spec の節は、規則を実装するコミットと同じコミットで書く。コードのコメントは日本語で、理由を書き、`docs/` の節を引く。この計画と付録を引かない
- 日本語を書くときは natural-japanese のスキルに従う
- 子を起動するテストは `eml` 自身を子にする。`sh` に頼るものと、シグナルでの終わりと FIFO を確かめるものだけを `#[cfg(unix)]` か `-- windows: skip` にする (D-8)
- テストの変更の種類は「7. 実装の作業」の各作業と `commits.md` §5 の Q7 のとおりである。種類 1 (成否の変更) は次の1つだけである
  - 利用者が承認した種類 1 (2026-11-04、T-9 の後): `crates/eml_io/tests/real.rs` の `dropping_a_driver_waits_for_a_running_file_job` を `#[cfg(unix)]` にする。パイプの読む端をファイルとして取り込んで止まる読みを作るテストで、T-9 で Windows の driver が `FILE_TYPE_DISK` でないハンドルを断るようになったため、Windows では作れない。止まるディスクの読みもない (別のハンドルが錠をかけた範囲の `ReadFile` は待たずに `ERROR_LOCK_VIOLATION` で失敗する)。Windows では核の偽の backend の落とすテストと、プールの本物の読み書きのテストが残る
- 利用者が承認した種類 2 の範囲 (2026-11-04、段の全体): `std/` のコードを変えたことだけで動く期待値は、変えてよい。UI のスナップショットの中の `<std>/X.em` の行と列と、`crates/eml_vm/tests/bench.rs` の `unwind_points` と `RunStats` のスナップショットである。変えたものはコミットのメッセージに1つずつ書く

## 7. 実装の作業

各作業は1つのコミットで、それだけで確かめられる。作業 T-n の中身 (触るファイル、具体的な変更、spec、テスト、確かめのコマンド、実装者に任せること) は `commits.md` の「Commit n」にある。ここでは目的、根拠、順番と、要点だけを書く。

| ID | 作業 (コミットの題) | 根拠 | 前提 | 並べて進められるか |
|---|---|---|---|---|
| T-1 | Keep CaptureHost's written files in a temp directory | D-9、D-26 | - | T-2、T-10 と並べられる |
| T-2 | Pass a stage list and stdio specs to Host::spawn | D-2 | - | T-1、T-10 と並べられる |
| T-3 | Add stdin and stdout modes to Cmd | D-2、D-3、D-20、D-28、D-29 | T-1、T-2 | いいえ |
| T-4 | Spawn a Cmd pipeline in one Host call | D-2、D-11、D-27、D-32 | T-3 | いいえ |
| T-5 | Run regular-file reads and writes as pool jobs | D-5、D-7、D-10、D-22 | T-4 | いいえ |
| T-6 | Model regular files in the test driver | D-8 | T-5 | T-8 と並べられる |
| T-7 | Add Std.Stream | D-1、D-5、D-6、D-7、D-9、D-12、D-23〜D-25、D-30 | T-6 | T-8、T-9 と並べられる |
| T-8 | Add pipes and child-exit waits to the epoll and kqueue backends | D-4、D-7、D-29、D-32 | T-5 | T-6、T-7 と並べられる |
| T-9 | Add pipes and child-exit waits to the IOCP backend | D-4、D-7 | T-8 | T-7 と並べられる |
| T-10 | Add the real-clock mark to UI tests | D-8 | - | T-1〜T-9 と並べられる |
| T-11 | Add Std.Exec | D-1、D-4、D-7、D-8、D-11、D-12、D-31 | T-7、T-9、T-10 | いいえ |
| T-12 | Add the standard error pipe to Exec | D-11 | T-11 | いいえ |
| T-13 | Close async-files | すべて | T-12 | いいえ |

完了の条件は、どの作業でも次の2つである。

- 「6. 実装の約束」の、各コミットの木の上の確かめがすべて通る
- `commits.md` の該当する Commit の「Verification」のコマンドが、書かれた結果になる

### T-1: CaptureHost の書いたファイルを一時ディレクトリに置く
- 目的: 後の作業で、書いたファイルの本物の fd を子と driver に渡せるようにする
- 要点: `Written(Arc<Store>)`。一時ディレクトリは最初の書き込みで作り、最後の `Written` の写しを落とすときに消す。`ui.rs` が host を落とした後で `written` を読むためである
- テストの変更: 既存の期待値は変わらない。`--- written:` を含む UI のスナップショットと REPL のトランスクリプトは、バイト単位で同じでなければならない (種類 3)
- 実装者に任せること: 一時ディレクトリの名前の付け方
- 落とし穴: `ui::run` の時間を親のコミットと比べ、0.2 秒を超えて増えていないことをコミットのメッセージに書く

### T-2: Host::spawn に段の並びと標準入出力の指定を渡す
- 目的: T-3 と T-4 の下準備の書き換え。外から見える動きは変えない
- 要点: `StdinSpec`、`StdoutSpec`、`StderrSpec`、`Stage`、`Spawn`、`Spawned.processes` の形は `design.md` §5 の Host と driver の起動のインターフェースのとおりである。この作業では、今の起動の形が使う variant だけを足す
- テストの変更: `eml_rt`、`eml_vm` のテストの呼び出しの形だけを直す (種類 3)
- 落とし穴: `eml_rt` の関門 (Verus、verusfmt、Miri) を流す

### T-3: Cmd に標準入力と標準出力の向きを足す
- 目的: 文字列の標準入力 (`stdin_text`)、ファイルの入出力 (`stdin_file`、`stdout_file`、`stdout_append`、`no_stdin`) を同期の `Proc` で使えるようにする
- 要点: Prelude の `StdinMode`、`StdoutMode`、`Cmd` の形は `design.md` §1 の A2 にある。`spawn_raw` の引数は5つ (段の列、標準入力の番号と中身、標準出力の番号とパス) になる。`SPAWN_LOCK` と `Fault::Launch` もここで入れる。`proc_spawn` の Verus の証明を、3つの文字列と2つの整数の形に書き直す
- テストの変更: `Cmd` の値を表示するスナップショット (`git grep -ln "stderr = InheritStderr" -- crates/eml_cli/tests/snapshots` が出すもの) と、`crates/eml_core_ir/tests/externs.rs` の表 (種類 2、理由: `Cmd` に欄が増え、`spawn_raw` の型が変わる)
- 落とし穴: Windows の一時ファイルは `access_mode` に `DELETE` が要る (`FILE_FLAG_DELETE_ON_CLOSE` のため)。`launch` は `SPAWN_LOCK` を取らない (D-29)

### T-4: Cmd のパイプラインを1回の Host の呼び出しで起動する
- 目的: `.|` と `Proc.wait_all`
- 要点: 段の間のパイプは `std::io::pipe()` で作り、起動が戻る前に親の端をすべて落とす。途中の段の標準出力のリダイレクトは ``cannot run `<prog>`: the standard output of a stage that is not last is redirected`` にする
- テスト: `tests/ui/run/std/proc_pipeline.em`、`proc_pipeline_show.em`、`tests/ui/run-fail/std/proc_stage_redirect.em` (段の名前を `eml-first`、`eml-second` のように分ける) など。一覧は `commits.md` の Commit 4 にある
- 落とし穴: `proc_pipeline` の標準エラーの出力は、EPIPE の起きる時点で変わりうる。スナップショットを決定的に保てる子の動きにする

### T-5: 通常のファイルの読み書きをプールの仕事にする
- 目的: driver の側のファイルの道。T10 の規則の改訂 (D-10) をここで入れる
- 要点: `eml_rt` の `OsHandle`、`RawFile`、`RawResource`、`Op::Adopt`、`Done::Handle`。核の `Class` と、`Memory` として保たれる `FileRead` と `FileWrite` の仕事。`crates/eml_io/Cargo.toml` に `Win32_Storage_FileSystem` を足す。vm.md に、ファイルの host は取り込んだ worker で、開くときに決まり、救済は持ち主のタスクだけを動かすことと、`quiesce` の前に driver を落とさないかぎり1秒の abort に届かないことを書く
- 完了の条件に加えるもの: G-10 の確かめ (`commits.md` §5b)。このコミットは分けない。G-10 の `git log -S"FileRead"` が、このコミットを指すためである
- 落とし穴: Windows のファイルの仕事は、null の `OVERLAPPED` で同期のハンドルに `ReadFile` と `WriteFile` を呼ぶ。overlapped のファイルの I/O は使わない
- 実装で決めたこと: ファイルの仕事の slot は `Hold::None` のままで、driver の `Drop` が種類 (ファイルの読み書きの仕事) で見分けて `Hold::Memory` と同じに保つ。既存のテスト `trust::the_core_reads_hold_only_to_drop` が、`Hold::Memory` を `drop` の中にしか置かせないためである。D-5 の「`Memory` として保たれる」は、この形で満たす

### T-6: テストの driver に通常のファイルを持たせる
- 目的: `stream_*` の UI テストを、仮想の時計の上で seed と N = 2、8 で流せるようにする
- 要点: `adopt(File)`、`Job` と同じ扱いのファイルの読み書き、パイプと `Op::Wait` の拒否 (D-8)
- テスト: `test_driver::a_cancelled_file_read_still_delivers_its_bytes`、`test_driver::the_test_driver_has_no_pipes` など

### T-7: Std.Stream を足す
- 目的: 非同期のファイルの API
- 要点: 行の一覧は `commits.md` §5 の Q2 にある。`Host::open_raw` と通常のファイルの確かめ (D-6、D-30)。`Open` の記録は `Connect` と同じく片づく。取り消されたとき、`finish` が作ったストリームを落とし、ハンドルは host の bin に入る
- テスト: `stream_read_file.em`、`stream_write_read_back.em`、`stream_cancelled_reader.em`、`stream_timeout_after_job.em`、`stream_open_special.em`、`stream_open_cancelled.em`、`cells::an_open_record_binds_the_file_to_the_openers_host`、`cells::an_open_cancelled_after_adoption_bins_its_handle`、`api::a_stream_open_of_a_fifo_fails_at_once` (本物の時計、unix だけ)

### T-8: epoll と kqueue の backend にパイプと子の終了の待ちを足す
- 目的: Unix の `IoDriver::pipe` と `Op::Wait`
- 要点: 親の端だけに `O_NONBLOCK` を付ける。パイプは `read` と `write` で読み書きする。Linux は pidfd (`ENOSYS` なら `Unsupported`)、macOS は `EVFILT_PROC` を登録してから `waitid(WNOWAIT)` で、すでに終わった子を拾う。IOCP の backend は、この作業の時点ではパイプと待ちを `Unsupported` で断り、vm.md にもそう書く
- テスト: `sys::unix::tests::a_pipe_parent_end_is_nonblocking_and_the_child_end_is_not`、`real::a_wait_on_a_running_child_is_cancelled_at_once` など
- 落とし穴: `libc` が pidfd と `waitid` の記号を両方の Unix の対象で持つことを、最初に確かめる。Linux の `O_NONBLOCK` と `O_NOCTTY` の値は、FIFO の api のテストが CI で確かめる

### T-9: IOCP の backend にパイプと子の終了の待ちを足す
- 目的: Windows の `IoDriver::pipe` と `Op::Wait`
- 要点: 名前付きパイプ (`CreateNamedPipeW` に `FILE_FLAG_OVERLAPPED | FILE_FLAG_FIRST_PIPE_INSTANCE`、子の端は `CreateFileW` で同期、継承させない)。overlapped の `ReadFile` と `WriteFile` の box。`RegisterWaitForSingleObject` の callback は `Exited` の列に積んで `PortWaker::wake` で起こす。待ちの文脈と表の項は、`UnregisterWaitEx(wait, INVALID_HANDLE_VALUE)` が戻った後でだけ放す。発火した待ちも取り消した待ちも同じである。`Cargo.toml` に `Win32_System_Pipes` と `Win32_Security` を足す
- テスト: `sys::iocp::tests::a_pipe_child_end_is_not_inheritable`、`a_fired_wait_is_unregistered_before_its_context_is_freed` など
- 落とし穴: Windows で走らせられるのは CI だけである。手元では cross clippy まで

### T-10: UI テストに本物の時計の印を足す
- 目的: `-- clock: real (<理由>)` の印を読み、その印のファイルを `Clock::Real` で検査モードと速いモードだけで流す
- 要点: 印は `crates/eml_cli/tests/common/mod.rs` で読む。seed と worker の数で流す範囲の確かめから外す。testing.md 「UI テスト」「seed で流す」に書く
- 落とし穴: この作業の時点では印の付いたファイルがない。印のファイルがないときに確かめが空振りしない書き方にする (`commits.md` の Commit 10)

### T-11: Std.Exec を足す (標準エラーのパイプを除く)
- 目的: 非同期の子。`spawn`、`run`、`output`、`capture`、`stdin` と `stdout` の取り出し、`wait`、`wait_all`
- 要点: 仮想の時計では `spawn_raw` がどの driver にも触る前に失敗する (D-8)。`Exec.output` は、標準出力をリダイレクトしたとき読み手を取らない (D-31)
- テスト: `tests/ui/run/async/exec_pipeline.em`、`exec_stdin_text.em`、`exec_child_released_on_abort.em`、`exec_wait_all.em` (印つき、子は3回まで)、`tests/ui/run-fail/async/exec_needs_the_real_driver.em` (印なし。seed と N = 2、8 で流れる)、`tests/ui/run-fail/async/exec_write_after_exit.em`、`cells::a_pipe_write_broken_pipe_is_outcome_reset`、`cells::a_wait_record_is_hosted_by_its_owner`、`api::a_child_pipe_is_adopted_by_another_worker` (N = 4、AddressSanitizer と ThreadSanitizer の対象)
- 落とし穴: `ui::run` の時間。CI の時間の確かめ (`--profile timing -j 1`) が超えたら、印のファイルを1つ `api.rs` に移す (Q-2)

### T-12: Exec に標準エラーのパイプを足す
- 目的: `Exec.pipe_stderr` と `Exec.stderr`。標準出力と標準エラーを2つのタスクで読み分ける
- 要点: `run`、`output`、`capture` で `PipeStderr` を使うと起動の誤りである (D-11)
- テスト: `exec_stderr_split.em`、`tests/ui/run-fail/async/exec_pipe_stderr_on_run.em`

### T-13: async-files を閉じる (`Close async-files`)
- 目的: docs/future/roadmap.md 「進め方」の閉じる手順。この計画と付録を消すのは、最後の全体のレビューの後の別のコミット (`Remove the async-files plan`) にする。前の段 (async-rescue) と同じ順である
- 要点: `commits.md` の Commit 13 と §5 の Q8 にある。roadmap からこの段を消し、範囲の外に回した項目 (「2. 範囲」の一覧) を「後に回した理由」と「見直す条件」つきで「その後の項目」に移す。この段が含んだ「標準入力と標準出力のファイルへのリダイレクト」の項目を消す。status.md、overview.md の表、README.md の地図、benchmarks.md の3つの表 (`bench/run.sh`、`bench/rt.sh`、`bench/scripts.sh`) を直す
- 落とし穴: 消す前に、`docs/` に残っていない決めがないことを確かめる。とくに D-1〜D-12 の「残す場所」がすべて書かれていること

## 8. 実行の順番

```
T-1 ─┐
T-2 ─┴─ T-3 ── T-4 ── T-5 ─┬─ T-6 ── T-7 ──────┐
                          └─ T-8 ── T-9 ──────┼─ T-11 ── T-12 ── T-13
T-10 ─────────────────────────────────────────┘
```

- 並べて進められる組: {T-1、T-2、T-10}、T-5 の後の {T-6 → T-7} と {T-8 → T-9}
- コミットの順は、上の表の番号の順にそろえてよい。並べて進めたときは、上の依存を崩さない順でつなぐ

## 9. 確かめ方

- 基準の確かめ (最初の変更の前): `cargo nextest run` → すべて通る (基準のコミット 949edf5c に知られた失敗はない)。`cargo clippy --all-targets -- -D warnings` → きれいである
- 全体の確かめ (すべての作業の後):
  - `cargo nextest run` → すべて通る
  - `cargo clippy --all-targets -- -D warnings && cargo fmt --check` → きれいである。CLAUDE.md の feature の組み合わせの clippy と cross clippy もきれいである
  - `cargo verus verify -p eml_rt` → 通る。`verusfmt --verus-only --check $(git ls-files 'crates/eml_rt/src/*.rs')` → きれいである
  - `nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_rt trusted::` と、`-Zmiri-tree-borrows` を足したもの → 通る。同じ2つの設定で `cargo miri test -p eml_io --lib -- driver::tests::` → 通る
  - CLAUDE.md の AddressSanitizer のコマンド (`eml_io` と、`eml_vm --test integration -- api:: signal:: io:: workers::`) → 報告なし。ThreadSanitizer のコマンドに `-p eml_io --lib -- driver::tests::` を足したもの → 報告なし (T-5 で CI と testing.md に足す)
  - `commits.md` §5b の G-1〜G-12 のコマンド → それぞれ書かれた結果になる
  - `timeout 20 cargo run -p eml_cli -- run tests/ui/run/async/exec_pipeline.em` → スナップショットの標準出力を出し、0 で終わる
  - CI (macOS、Linux x86_64 と aarch64、Windows) がすべて通り、`cargo nextest run --profile timing -j 1` で 2 秒を超えるテストがない
- 利用者の手での確かめ: なし

## 10. 危険と未決の項目

| ID | 項目 | 影響 | どう決まるか | 決まらないときの既定 |
|---|---|---|---|---|
| Q-1 | macOS の `EVFILT_PROC` が、すでに終わって回収していない子の登録で、すぐ知らせるか `ESRCH` を返すか。FreeBSD の man と macOS の `man kqueue` しか確かめていない | T-8 | T-8 の `sys::unix` のテスト (`true` を起動し、終わるまで待ってから登録し、1秒以内に知らせが来る) | 登録の後の `waitid(WNOWAIT)` で拾う設計のままにする |
| Q-2 | `ui::run` の実時間が5秒の条件を超えること | T-7、T-11、T-12 | CI の時間の確かめ | 印のファイルを1つずつ `crates/eml_vm/tests/api.rs` に移す |
| Q-3 | `RegisterWaitForSingleObject` の callback から IOCP を起こすことが、今の port の使い方と両立するか | T-9 | T-9 の Windows の CI のテスト | 両立しなければ、専用のスレッドで `WaitForSingleObject` する形にする。API は変えない |
| Q-4 | `CreateNamedPipeW` と `CreateFileW` のパイプが、std の `child_pipe` と同じく継承と overlapped の読みで動くか | T-9 | T-9 の Windows の CI のテスト | 標準エラーを標準出力に合わせない (`MergeStderr` でない) パイプは、std の `Stdio::piped()` のハンドル (親の側は overlapped) を使う。API は変えない |
| Q-5 | `libc` が pidfd と `waitid` の記号を、x86_64 と aarch64 の Linux と macOS で持つか | T-8 | T-8 の最初に cross clippy で確かめる | 止めて利用者に聞く |

## 付録: レビューで変わった主なこと

蒸し返しを防ぐのに要る分だけを書く。

- 同時に起動する子が、別の子のパイプの端を継承する恐れ → 受け入れた。子の端を継承させずに作り、プロセス全体の `SPAWN_LOCK` を起動とパイプの作成で取る (D-2、D-7、D-29)
- FIFO や端末を `Stream.open` が開くと、プールのスレッドが止まる → 受け入れた。通常のファイルだけを開く (D-6)
- パイプとファイルの読みで、`Readable` と `Probe` の代わりがない → 受け入れた。受け付けのときに buffer を広げる (D-5)
- ファイルを遅れて取り込むと、仮想の時計の N = 2、8 で動かない → G-7 を改訂し、利用者が承認した。ファイルは開くときに縛る (D-7、`ruling.md` Q1)
- 結果の番号を 0 と 6 だけにする案 → G-12 に反すると裁定した。0〜6 と `op_prefix` に戻した (D-12、`ruling.md` Q2)
- テストの driver で、ファイルの操作を普通の操作にすると取り消しで中身を捨てる → 受け入れた。`Job` と同じ扱いにする (D-8)
- 同期の `Proc.wait` の回収の順で詰まる → 一部を受け入れた。順は変えず、「標準出力を読み切ってから待つ」の規則をパイプラインにも書く (D-27)
- 起動の誤りの理由に OS の文言を出すか → 今の `Proc` と同じ4つの決まった語にした (D-28)
- 子のプロセスをテストの driver で仮想にする案と、テストの driver に仮想のファイルシステムを持たせる案 → 退けた (`design.md` §3)
