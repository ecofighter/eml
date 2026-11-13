# unixでSIGINTとSIGTERMを確かに届ける段 (signal-delivery)

- 作成日: 2026-11-13
- 基準: fix/interrupt-test (bdd860a0)
- 状態: 利用者が方針を承認した。この計画はそれを記録する

## 1. 目的

unixでは、runの間にSIGINTとSIGTERMを受けるスレッドを、隙間なくworker 0だけにする。これで、テスト`cli::an_interrupt_stops_a_program_waiting_for_a_child_with_130`は負荷の下でも毎回通り、どのタスクが始めた子もCtrl-CとSIGTERMで終わらせられる。設計はworker 0をただ1つの受け手のままにし、シグナル専用のスレッドを足さない。専用のスレッドを置くと、届け方がworker 0から見て非同期になり、worker 0が止めるビットの立つ前に待ちから戻りうるためである。

## 2. 4つの不具合

1. メインスレッドは、worker 0を作った後でしか2つのシグナルを塞がない (`crates/eml_vm/src/lib.rs`の`run_scheduled`)。その間にメインスレッドへ行ったシグナルは扱いが遅れ、runは自分の終了コードで終わることがある。
2. `spawn_masked`と`spawn_scoped_masked` (`crates/eml_io/src/mask.rs`) は、`spawn`の間、呼んだスレッドを塞ぐ。呼んだのがworker 0なら、すべてのスレッドが塞いでいて、macOSではシグナルがrunの終わりまでメインスレッドの保留に残る。`SA_NODEFER`がないので、worker 0のハンドラが動く間の2回目のシグナルも同じく保留に残る。
3. worker 0のほかのスレッドが始めた子 (`Async.start`か`Async.start_on_main`のタスクが始めた子) は、SIGINTとSIGTERMを塞いだまま始まる。子は始めたスレッドの塞ぎを受け継ぐためである。Ctrl-CとSIGTERMではその子を終わらせられない。
4. `eml_nrt::run` (`crates/eml_nrt/src/run.rs`) は、メインスレッドを一度も塞がずにworker 0を作ってjoinする。macOSではメインスレッドが古いほうのスレッドなので、すべてのシグナルをそちらが受けることになる。

## 3. 設計

不変条件: unixでは、runの間、VMのスレッド (`eml run`ではworker 0、`eml repl`では呼んだスレッド) は、ハンドラの中でもSIGINTとSIGTERMを塞がない。emlが作るほかのスレッドは、どれも本体を動かす前に2つを塞ぐ。`Machine`を持つスレッドは、worker 0が最初の命令を動かす前に2つを塞ぐ。

- 始めの門 (不具合1と4): worker 0は塞がずに作るが、その本体はまずチャネルで待つ。呼んだスレッドは`eml_io::Masked`で2つのシグナルを塞いでから門を開く。呼んだスレッドがpanicすると送り手が落ちるので、worker 0は止まったままにならない。門に`cfg`は要らない。
- ネイティブのrun (不具合4): `eml_nrt`は自分で`pthread_sigmask`を呼ばない。`eml_nrt::image::Options`に、2つのシグナルを塞いだまま呼んだスレッドでクロージャを動かすフックを足す。`eml_nrt::run`は、その中で門を開いてworker 0をjoinする。既定のフックは何も塞がず、`eml_cli::execute_on`は`Masked`を使うフックを渡す。
- 子の側で塞ぐ (不具合2): `spawn_masked`と`spawn_scoped_masked`は、呼んだスレッドの塞ぎに触れない。新しいスレッドが自分の本体の始めに2つのシグナルを塞ぎ、戻さない。そのためworker 0は塞がない。塞いだスレッドが作るスレッドは塞ぎを受け継ぐので、生まれるときに数マイクロ秒の隙間を持つのは、worker 0かREPLのスレッドが作るスレッドだけである。そこで受けたシグナルも、ビットを立てて自己パイプを鳴らす。
- 保留に残ったものを送り直す (Appleだけ): 塞いだ後で`sigpending`を読む。2つのシグナルのうちこのスレッドで保留になっているものを、`sigwait`で取って`kill(getpid(), sig)`で送り直す。シグナルは塞いでいないスレッドへ行く。Linuxでは行わない。`sigpending`がプロセス全体の集合も返し、`sigwait`が待ち続けうるためである。
- `SA_NODEFER`と、順に動くハンドラ (不具合2): 2つのハンドラを`SA_NODEFER`で入れ、ハンドラが動く間もworker 0が受け手であり続けるようにする。種類ごとに1つの数で、2段の数え方を正しく保つ。ハンドラは数を1増やし、前の値が0でなければ戻る。0なら`respond`を呼んで1減らし、数が0になるまで繰り返す。入れ子のシグナルは外のシグナルの後に扱い、`respond`の途中では扱わない。
- 子は塞がずに始める (不具合3): `eml_io::Unmasked`の守りで、子を1つ作る`Command::spawn`の呼び出しの間だけ、呼んだスレッドの2つのシグナルの塞ぎを外し、終わったら送り直しとともに塞ぎ直す。標準出力の書き出しとリダイレクトのファイルを開くことは包まない。そのため、`eml_rt`の`StdHost`に、各段の`Command::spawn`を包むフック`with_around_spawn(fn(&mut dyn FnMut()))`を足す (既定はそのまま呼ぶ。`eml_rt`はlibcもシグナルも扱わない)。`eml_vm`の`script_host`と`stdio_host`は`Unmasked`を持つフックを渡し、`CaptureHost`は自分の`command.spawn()`を包む。worker 0では何も変わらない。メインのworkerでその呼び出しの間に来たシグナルは、メインスレッドへ行く (macOSは塞いでいないいちばん古いスレッド、Linuxはスレッドグループの先頭を先に選ぶ。Linuxは推論)。受けても扱われ、逃すのはworker 0のブロックする読みの`EINTR`だけである。これは`eml_rt`の変更なので、`crates/eml_rt/CLAUDE.md`の門を通す。
- 始めに塞ぎを外す: 親が2つを塞いだままemlを起動すると、どのスレッドもCtrl-Cを受けない。`interrupt::install`は、どのスレッドもできる前にメインスレッドで2つの塞ぎを外す。
- Cのライブラリのスレッド: `eml run`は`link_program` (`crates/eml_cli/src/main.rs`) の間メインスレッドで2つのシグナルを塞ぎ、ライブラリの初期化が始めるスレッドに塞ぎを受け継がせる。worker 0の上で動くC関数が作るスレッドは扱わず、specに制限として書く。
- 変えないもの: `Std.Signal`、自己パイプ、監視のスレッドの中継、GUIの`Park`、Windows。

## 4. 作業の単位

単位ごとに1つのコミットにし、実装する規則のspecの文を同じコミットに書く。

1. `Gate signal delivery on unix` (`eml_io`、`eml_vm`、`eml_nrt`、`eml_cli`): 始めの門、子の側の塞ぎ、Appleの送り直し、ネイティブのrunのフック、`link_program`の間の塞ぎ。完了: 下の負荷の試しで失敗が0回。送り直しのテストがmacOSで通る。
2. `Serialize the signal handlers` (`eml_cli/src/interrupt/unix.rs`): `SA_NODEFER`と数。完了: 数のテストが通る。
3. `Start children with signals unblocked` (`eml_io`、`eml_rt`、`eml_vm`): `Unmasked`、`StdHost`のフック、UIテスト。完了: UIテストと`eml_rt`の門が通る。
4. `Unblock the two signals when the handlers are installed` (`eml_cli`): 始めに塞ぎを外す。完了: その単体テストが通る。
5. `Close signal delivery`: この計画を消す。

どの単位も: `cargo nextest run`で失敗が0、`cargo clippy --all-targets -- -D warnings`、`cargo fmt --check`、tlsとguiを外した`eml_io`、`eml_vm`、`eml_nrt`のfeatureの組み合わせのclippy、Windowsへのcrossのclippy。単位1と3では、`eml_io`のAddressSanitizerと、CLAUDE.mdのコマンドのThreadSanitizerも流す。

不具合1と2の受け入れ: `cli::an_interrupt_stops_a_program_waiting_for_a_child_with_130`の50回のループを40本並べて流し、2000回のうち失敗が0回。単独の200回でも0回。worker 0を作ってから塞ぐまでの隙間を突く決定的なテストは書けない。ここからはLinuxの機械に届かないので、Linuxの確かめはCI (x86_64とaarch64のLinux。ASanとTSanを含む) が行う。

## 5. テストの変更

- 新しいテスト:
  - `eml_io`の`mask.rs`の単体テスト (Appleだけ): SIGUSR2を塞ぎ、`pthread_kill`でこのスレッドに送り、送り直す。このスレッドの保留から消え、数えるハンドラがほかのスレッドで動いたことを確かめる。
  - `eml_rt`の`StdHost`の単体テスト: フックが段ごとに1回`Command::spawn`を包み、クロージャを呼ばないフックでは起動の誤りになり、2回呼ぶフックでも子は1つである。
  - `eml_cli`の`interrupt/unix.rs`の単体テスト: 塞いだスレッドで`unblock`を呼ぶと2つの塞ぎが外れる。
  - `eml_cli`の`cli.rs`のテスト: `eml run --workers 2`で、worker 1とメインのworkerのタスクが始めた子が自分へのSIGTERMで143で終わる (`script_host`のフックの道)。execの前から塞いで保留にしたSIGINTで始めたemlが、130と`interrupted`で終わる。
  - `eml_cli`の`interrupt/unix.rs`の単体テスト: 1回だけ入れ子に入るcallbackで数の関数を呼び、callbackが2回、入れ子にならずに動くことを確かめる。
  - `tests/ui/run/async/`のUIテスト (本物の時計、Windowsで流さない印): `Async.start`のタスクと`Async.start_on_main`のタスクから`sh -c 'kill -TERM $$; echo survived'`を始め、どちらも終了コード143で何も書かないことを確かめる。
- 種類2: `crates/eml_vm/tests/spawns.rs`の一覧に、`Masked::new`と`Unmasked::new`を呼ぶ所の行を足し、コメントを変える。`ui::run_native`の数のスナップショット (`integration__ui__run_native.snap`) は、新しいUIテストをネイティブのコード生成器が断る分だけ動く (`rejected`と`async row`が1ずつ増える)。
- コメントだけの変更: `crates/eml_io/tests/real.rs`の`pool_threads_block_interrupt_and_terminate`のコメント。`cli::an_interrupt_stops_a_program_waiting_for_a_child_with_130`のdocコメントから、知られた不具合の文を消す。

## 6. 文書

- `docs/spec/vm.md`の「上限と割り込み」: `SA_NODEFER`と数 (unixと比べるWindowsの項目も)、門、不変条件、子の側の塞ぎ、Appleの送り直し、C関数が作るスレッドの制限。知られた不具合を消す。
- `docs/spec/stdlib.md`の「`Proc`」と「`Std.Exec`」: 子は、どのworkerが始めても、SIGINTとSIGTERMを塞がずに始まる。
- `docs/spec/native.md`: ネイティブのrunのworkerのスレッドを書いていれば、門。
- `docs/future/roadmap.md`: 項目「unixでSIGINTが届くスレッドの隙間」を消す。
- `docs/future/multicore.md`: 2つの隙間を消し、子の側の塞ぎを書く。
- `docs/implementation/architecture.md`: `mask.rs`と門。
- `docs/implementation/testing.md`: 新しいテスト。残る失敗の文を消す。
- コードのコメント: `mask.rs`の頭、`run_scheduled`、`interrupt/unix.rs`、`monitor.rs`、`workers.rs`。
