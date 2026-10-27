# async-cancel: タスクの取り消しと名前付きのスコープ

- 作成: 2026-10-27
- 基準: `main` @ `16b8b73` (「3. 背景」と file:line の参照はこの状態を指す)
- 状態: 2026-10-27 に利用者が承認した。5 章の決めたことと、「10. 危険と未決の項目」の U-1〜U-4 は既定のとおりに進める
- 付録: `docs/plans/2026-10-27-async-cancel/`
  - `T-1.patch` 〜 `T-5.patch`: 各 task の参照の実装 (コードとテストとスナップショット)。基準に順に `git apply` で当たり、当てた後の各段で `cargo nextest run`、clippy、`cargo fmt --check` が通ることを確かめてある。`docs/spec/stdlib.md` への変更は引用の検査を通すための見出しの仮置き (`(仮)`) だけで、spec の本文はこの計画書の各 task の「spec」に書いてある
  - `Async-T3.em`、`Async-T4.em`、`Async.em`: T-3、T-4、T-5 を終えた時点の `std/Async.em` の全文 (パッチの中身と同じ)
  - `ui-tests.md`: 新しいテストと変えるテストのプログラムと、期待するスナップショット

## 1. 目的と成功の条件

- 目的: roadmap の段 `async-cancel` を実装する。1つの worker の VM の上で `Std.Async` のタスクに取り消しを入れ、名前付きのスコープ (`scope`、`start_in`、`spawn_in`、`cancel_scope`)、`cancel`、取り消しになる `drop`、`shield`、`check_cancelled` と `cancel_requested`、操作の予算を入れる
- 成功の条件:
  - 「9. 確かめ方」の全体の確認がすべて通る
  - 後の段 (`async-time`、`async-chan`、`ffi-blocking`、複数の worker、ネイティブのコード) が、この段の核 (`cells.rs`) の呼び口を変えずに待ちの種類と表現を足せる形になっている (D-2、D-3)
  - `crates/eml_rt` を変えない

## 2. 範囲

- 含む: 取り消しのビットとスコープの旗、`Wake.Cancelled` で届けること、待ちを外すこと (セルの待ちと実行待ち)、失敗したタスクが同じスコープのタスクを取り消すこと、入れ子の `run` への取り消し、`drop` と `cancel` と `detach`、名前付きのスコープ、`shield`、柔らかい確かめ、操作の予算、`RunStats.cancels` と seed の比較の規則、spec と docs、段を閉じる作業
- 含まない: 時間 (`await_time`、`sleep_ms`、`timeout`、`race`、`drain_within`)、channel、I/O の driver とプールの待ち、複数の worker、救済、`par` の子の取り消し、タスクごとの上限。これらは後の段で、この段はそのための口 (D-2 の `Wait` と `Unpark::External`、D-3 の区分) だけを決める

## 3. 背景

- `std/Async.em` は 87 行で、`Wake = Woke | Cancelled`、抽象エフェクト `Async` (生の操作 `await_cell` と `reschedule`)、`try_all`、入口 `start_task` と `resume_task`、底 `bottom`、`run`、`start`、`spawn`、`await`、`yield_now`、extern の10行を持つ。`spawn f = drop (start_raw (bottom f))` (`std/Async.em:60`)。`yield_now` は `Cancelled` を受けても何もせず (`std/Async.em:68-72`)、`await` は受けた `Wake` を捨てる (`std/Async.em:65`)
- `IOErrorKind` の `Cancelled` はタグ 5 で、もうある (`std/Prelude.em:211`)。`fail_io` は `Fault::Raised` を起こし (`crates/eml_vm/src/vm.rs:2325`)、最も近い `try_io` に届く。`Std.Async` の中では素の `Cancelled` は `Wake.Cancelled` になるので、IO の種類は `Prelude.Cancelled` と書く
- `eml_lir::WAKE_CANCELLED = 1` はあるが、使うのはタグの対応のテストだけである。核は `WOKE = 0` しか作らない (`crates/eml_vm/src/cells.rs:18`)
- 核 `cells.rs` (664 行) は `std` と `eml_rt` だけを import する (テスト `the_task_cells_import_only_std_and_the_runtime`)。`TaskCell { phase: Placed|Running|Parked|Finished, outcome, scope, worker, spawner, waiters, handle_dropped }`、`ScopeCell { live, first_failure, waiters, closed }`。待ち手 `Waiter::Cont { task, k }` は待つ相手のセルが継続を持つ。待ちを外す操作はない。`live` が 0 になるとスコープは閉じる (`cells.rs:485-488`)
- ハンドルの `Drop` は受け箱に `HandleDropped` を積むだけで、セルに触れない (`cells.rs:191-199`)。受け箱を処理するのは `switch_away` (`vm.rs:1220`)、`preempt` (`vm.rs:1314`)、`main` の `Finish` (`vm.rs:1592`) の3か所である
- `Async` の行は `Op::Async` → `async_op` (`vm.rs:1626`) で動き、`Scheduler` に触れる。汎用の `run_extern` (`vm.rs:2020`) は `Ctx` と `Task` しか見ない。`run_extern` の `Async` の行を断る腕 (`vm.rs:2545-2557`) と `Extern::is_async` (`crates/eml_extern/src/lib.rs:1136`) は網羅の match なので、行を足し忘れるとコンパイルが通らない
- 行を足すときに触るのは `eml_extern` の表と `is_async`、`std/Async.em` の宣言、`vm.rs` の腕、タイの検査 (`crates/eml_hir/tests/externs.rs`、`crates/eml_core_ir/tests/externs.rs`) である。`eml_core_ir/src` と `eml_lir/src` には行ごとのコードはない
- `RunStats` は 17 の欄を持ち (`crates/eml_vm/src/lib.rs:35-62`)、`crates/eml_vm/tests/bench.rs` の 44 のインラインのスナップショットが固定している
- `async/` の UI テストは `run/async/` 9本、`run-fail/async/` 3本で、検査モードと速いモードのほかに seed 1 と 2026 で流す (`crates/eml_cli/tests/ui.rs:262-295`、testing.md の「seed で流す」)。どのファイルも少なくとも1回横取りされなければならず、`start` の後に `spin 40` を置く約束がある
- 今の spec と、この段の設計が食い違う所: `drop t` は取り消さない (`docs/spec/stdlib.md:934`、`docs/implementation/status.md:141`)、失敗したタスクはほかのタスクを取り消さない (`status.md:141`)、タスクの中の入れ子の `run` は自分の根を開く (`stdlib.md:931`)。設計 (`docs/future/async.md` の「確定した設計判断」の #3、#5) は、捨てると取り消し、失敗した子はほかを取り消す、である
- 試しのプログラムで確かめた今の振る舞い: タスクが `fail_io Prelude.Cancelled "task was cancelled"` で終わると、今はスコープの失敗として `run` が起こし直す

## 4. 前提

| 要るもの | 使う所 | 確かめ方 | ないとき |
|---|---|---|---|
| nix の devShell | すべての task | `. /nix/var/nix/profiles/default/etc/profile.d/nix-daemon.sh && nix develop -c cargo --version` → `cargo 1.99.1` | クラウドのセッションで GitHub の archive が 403 なら、`nix/prefetch-inputs.sh` を流してからやり直す (CLAUDE.md) |
| `.#miri` の shell | T-3、全体の確認 | `nix develop .#miri -c cargo miri --version` → 版が出る | 止めて利用者に伝える |
| `.#cross` の shell | 全体の確認 | `nix develop .#cross -c rustc --print target-list \| grep -c x86_64-pc-windows-msvc` → `1` | 止めて利用者に伝える |
| macOS の機械 | T-6 の計測 | `uname` → `Darwin` | 計測を T-7 (`Measure async-cancel`) に分け、利用者の Mac で行う |
| `yomiyasu` スキル | 日本語を書くすべての task | スキルの一覧にある | 止めて利用者に伝える |

## 5. 決めたこと

この章の日本語の spec の文案はyomiyasu を通す前の下書きである。spec に書くときに yomiyasu で整える。意味 (主張、条件、言い切りの強さ) は変えない。

### 5.1 長く効く方針 (変えるときは見直しが要る)

実装がこれらと食い違ったら、止めて報告する。

#### D-1: 取り消しの届け方
- 決めたこと:
  - 取り消しは2つの所から来る。タスクの取り消しのビット (`cancel t`、`drop t`、属するスコープの取り消し、旗の立ったスコープに加えることで立つ) と、スコープの取り消しの旗 (`cancel_scope`、スコープのタスクの失敗、`scope` の本体の失敗、親のスコープの取り消しで立ち、開くときに親の旗を写す) である。どちらも一度立てたら下ろさない
  - ランタイムは取り消したタスクの継続に `Wake.Cancelled` を渡し、普通の `resume_task` の入口で再開する。止めるビットもランタイムによる巻き戻しも使わない
  - std の包みは `Cancelled` を受けると `fail_io Prelude.Cancelled "task was cancelled"` を投げる。投げるのは普通の IO の失敗で、タスクのフレームを巻き戻し、`try_io` と `try_all` が捕まえる。捕まえても取り消しは続く
  - 種類 `Cancelled` の IO の失敗で終わったタスクは、スコープの失敗にならず、旗も立てない。判定は種類だけで行うので、利用者が `fail_io Prelude.Cancelled` で投げた失敗も同じになる。核で判定するのは `Outcome::failure()` の1か所だけである
  - `Cancelled` が `await`、`run`、`scope` から出ると、呼んだタスクの普通の IO の失敗になる。取り消しの状態ではない。`scope` で吸収しない (`scope` は値 `a` を返す型で、取り消された本体は値を持たない)。吸収する形は後の段の `timeout` と `race` である
- 理由: async.md の確定した設計判断 #5。届け方をネイティブのコードと揃えるため (native.md:77)。種類で判定するのは、包みが投げるのが普通の `fail_io` で、`try_io` で投げ直した失敗と見分けられないためである
- 却下した案: `Outcome` に取り消しの種類を足す (投げ直しと見分けられない)、`scope` で `Cancelled` を吸収する (返す値がない)、強い取り消しを主にする (async.md の「却下した案」)
- 影響する所: 後の段のすべての待ち
- 見直す条件: ネイティブのコードで、`resume_task` を通さない再開が要るとき
- リポジトリに残す所: `docs/spec/stdlib.md` の「取り消し」(T-3)、`docs/spec/vm.md` の「取り消しの届け方」(T-3)

#### D-2: 継続は待つタスクのセルが1つだけ持つ
- 決めたこと:
  - 待っている継続 `k` はいつも待つタスクのセル (`TaskCell.k`) にある。待ちの登録 (セルの待ち手。後の段では時間の輪、channel の待ち手、I/O の操作の記録、プールの仕事の記録) は、タスクのセルの番号 (`CellRef`) だけを持ち、`k` を持たない
  - 起こす側も外す側もタスクのセルから `k` を取り出す。先に取り出した側が勝ち、両方が起きることも、どちらも起きないこともない。これが async.md の「待ちを外す」の競合の規則である
  - `TaskCell.wait: Option<Wait>` が、何を待っているかを名指す。この段の `Wait` は `Cell(CellRef)` と `Ready` である。後の段は種類を足す。核の持たない登録 (時間の輪、I/O の操作) では、`unpark` が `Unpark::External(wait)` を返し、実行系が自分の登録を外してから `CANCELLED` で `k` を取り出す。時間と channel ではすぐに、I/O とプールでは完了の後に取り出す。「取り消し中」は `Parked && cancel && wait.is_some()` で表し、状態の種類は足さない。`Unpark::External` は最初の外の待ちを入れる段で足す (使わない変種は clippy が通さない)
  - 待つ相手の待ち手の項は取り消しても消さない。起こすときに、タスクがまだそのセルを待っている (`wait == Some(Cell(from))`) ときだけ `k` を取り出す。そのため1回の取り消しは O(1) である
- 理由: 競合の規則を待ちの種類ごとに書かずに、核の1か所で保つため
- 却下した案: 今の `Waiter::Cont` のように待つ相手が `k` を持つ (待ちの種類ごとに置き場所と競合の扱いが要る)、取り消しで待ち手の項を消す (待ち手の数に比例する)
- 影響する所: async-time、async-chan、io-driver、ffi-blocking
- 見直す条件: 待ちの登録が `k` を持たなければならない待ちの種類が出たとき
- リポジトリに残す所: `cells.rs` の先頭の不変条件のコメント (T-1、T-3)、vm.md の「待ちの2種類」と「取り消しの届け方」(T-1、T-3)

#### D-3: 印を付けることと届けることを分け、待ちを外すのは受け箱の処理だけにする
- 決めたこと:
  - 取り消しは2段で行う。印を付ける (`mark`: ビットと旗を立て、スコープの属するタスクと入れ子のスコープへたどる。一度立てたら2度目は何もしない。たどる道は作業の列で持ち、Rust の再帰を使わない) と、届ける (取り消しの知らせ `Notice::Cancel(cell)` を受け箱に積む) である
  - `CANCELLED` で再開する仕事をほかのタスクのために作るのは、受け箱の処理 (`drain`) だけである。`park` の待つ前の確認と、`resumed` の実行待ちの決め直しは、タスクが自分のセルを読むだけである
  - `drain` は受け箱が空になるまで繰り返す (落ちたハンドルの処理が取り消しの知らせを積むため)
  - セルの状態の区分: 共有 (どのタスクの操作からも書く) は `cancel` と `flag`、スコープの一覧、`closing`/`closed`/`first_failure`。持ち主だけ (タスクを動かすか受け箱を処理する worker だけが読み書きする) は `phase`、`k`、`wait`、`shield`、`budget`、`current`、`run`、タスクのセルの待ち手
  - 複数の worker を入れる段 (async-threads) は、受け箱を worker ごとにし、`cancel` と `flag` を atomic に、一覧を mutex にするなど表現だけを替える。`mark`、`drain`、`unpark`、`resumed` の呼び口は変えない
- 理由: multicore.md の「スケジューラ」の「待ちを外すのは持ち主の worker がその受け箱の lock の下でだけ行う」を、1つの worker の場合にそのまま当てた形である。受け箱はほかのタスクに切り替える所と横取りを試みる所の頭で処理するので、印を付けてから待ちを外すまでにほかのタスクは動かない
- 却下した案: `finish` や `async_op` から直接ほかのタスクの待ちを外す (multicore.md の規則と逆の形になり、async-threads で作り直しになる)
- 影響する所: async-threads、async-rescue
- 見直す条件: 受け箱を通す遅れが計測で問題になったとき
- リポジトリに残す所: vm.md の「取り消しの届け方」(T-3)、`cells.rs` の先頭のコメント (T-3)

#### D-4: 取り消しの地点の規則と `shield`
- 決めたこと:
  - 取り消しの地点の規則: `shield == 0 && (タスクのビット || 今のスコープの旗)`。旗は親から子へ写して伝えるので、読むのは今のスコープの旗1つである
  - 取り消しの地点は待ちうる包み (`await`、`yield_now`、`scope` の終わりの待ち。後の段の `sleep_ms`、`send`、`recv` など) と `check_cancelled` である
  - `shield f` は、タスクのセルの `shield` の深さを1つ増やし、`try_all f` を動かし、深さを戻して、値を返すか失敗を投げ直す。`shield` の中では、`park`、`resumed` の決め直し、`unpark` がビットと旗を見ない。ビットは残り、`shield` を出た後の最初の取り消しの地点が投げる。`shield` は自分の取り消しの地点だけを守り、印を付けることは止めない (`shield` の中のタスクの子は取り消される)
  - `shield` の深さはタスクのセルに持つ。std にはタスクごとの動的な値を持つ仕組みがなく (タスクの row は閉じた `<IO, Panic, Async>` で、`var` は捕まえられない)、VM の `Task` は起きるたびに作り直すためである
  - `cancel_requested ()` と `check_cancelled ()` は同じ行 `cancel_point` を読む。`cancel_point` は取り消しの地点の規則を返すので、`shield` の中とタスクの外では偽である。`check_cancelled` は `cancel_point` が真なら投げる
- 理由: async.md の「取り消し」の「`shield` の中の取り消しの地点は取り消しのビットも旗も見ない」を、柔らかい確かめにも当てる。`scope` の本体は旗だけで取り消される (ビットを立てない) ので、柔らかい確かめは旗も読まなければならない
- 却下した案: `cancel_requested` がビットだけを返す (async.md の文のまま。`scope` の本体が自分の取り消しを見られない)、`cancel_requested` が `shield` を無視する (取り消しの地点と食い違う)、`shield` の状態を `Ctx` に置く (eml_rt の検証が要り、移る先の worker に付いていかない)
- 影響する所: 後の段のすべての待ちの包み
- 見直す条件: `shield` の中で取り消しの要求を知る必要が出たとき (「その後の項目」に置く)
- リポジトリに残す所: stdlib.md の「取り消し」(T-3、T-5)、vm.md の「取り消しの届け方」(T-3)

#### D-5: スコープの木と `scope` の手順
- 決めたこと:
  - タスクは2つのスコープを持つ。加わったスコープ (`scope`。作るときに決まり、`live` の数と失敗の記録の先になる) と、今のスコープ (`current`。タスクが `scope` で開いたいちばん内側のスコープで、開いていなければ加わったスコープ) である。`start` と `spawn` は今のスコープに、`start_in` と `spawn_in` は名指したスコープに子を加える
  - スコープのセルは、親 (`parent`)、開いたタスク (`opener`)、属するタスクの一覧 (`members`)、入れ子のスコープの一覧 (`nested`)、旗、閉じかけ (`closing`)、閉じた (`closed`)、最初の失敗、待ち手を持つ。終わっていないタスクの数は `members` の長さである。`closed` は `closing && members が空` で、`main` の `run` の根は開くときから閉じかけである。名前付きのスコープは本体が終わるまで閉じかけにならないので、本体が子を加え続けても途中で閉じない
  - `scope f` の手順は std の中の eml で書く (付録 `Async.em`)。`open_scope` で今のスコープの子を開いて今のスコープにし、`try_all (fn () -> f s)` を動かす。本体が失敗か取り消しで終わったら `fail_scope` で記録して旗を立てる。続けて `wait_closed` が閉じかけの印を付け、`shield` の中でスコープのセルを待ち、`leave_scope` で今のスコープを親に戻してセルを放し、最初の失敗があれば起こし直す
  - 結果の優先: スコープに最初に記録した失敗 (本体か子かによらず終わった順) が勝つ。記録がなければ本体の結果 (値、失敗、`Cancelled`) である。2つ目からの失敗は報告せず `RunStats.dropped_failures` に数える。種類 `Cancelled` は記録しない
  - タスクにビットを立てると、そのタスクが開いているスコープ (今のスコープから親へ、加わったスコープの手前まで) にも旗を立てる。ビットが立った後に開くスコープも、開くときに旗を立てる (`flag = parent.flag || opener.cancel`)。スコープに旗を立てたとき、そのスコープを開いたタスクの今のスコープがそのスコープなら、そのタスクにも届ける (ビットは立てない)
  - タスクの中の入れ子の `run` は呼んだタスクの今のスコープの下に根を開き、根の `opener` を呼んだタスクにする。呼んだタスクの `TaskCell.run` がその根を名指し、そのタスクにビットを立てると根にも旗を立てる。入れ子の `run` の呼び出しは取り消しの地点でない (row が `<IO, Panic>` で、呼んだタスクはスタックを持ったまま待つ)。根は誰の今のスコープにもならない
  - `start_in` と `spawn_in` は閉じたスコープ、前の run のスコープ、放したセルに対して `Panic` `the scope is closed` を起こす。閉じかけでタスクの残るスコープには加えられる。旗の立ったスコープに加えたタスクは、作るときにビットを立てる。閉じたスコープへの `cancel_scope` は何もしない
  - 不変条件: `members` と `nested` の項はいつも生きているセルを指す。タスクの今のスコープから親へたどる列は、そのタスクが開いたスコープだけからなり、加わったスコープで終わる。スコープのセルを放すのは、閉じた後の `release_scope` (親の一覧から外してから放す) と run の終わりの `sweep_all` だけである
- 理由: async.md の「スコープとタスク」(nursery の形)。入れ子の `run` の扱いは「`run` の意味は `scope` と同じ」に合わせた。ビットが開いたスコープへ伝わらないと、`cancel t` で `t` が `shield` の中の終わりの待ちにいるとき、子が取り消されずに待ち続ける (試作で止まりを再現した)
- 却下した案: `live` が 0 になったら閉じる今の規則 (名前付きのスコープが途中で閉じる)、入れ子の `run` の根を旗だけで取り消す (`cancel t` が届かず、どのスケジュールでも止まった)、取り消しの地点ごとに親をたどる (一覧は印を付けるのに要るので、旗を伝える方が安い)
- 影響する所: async-time の `drain_within`、ネイティブの `Async.run` の入れ子
- 見直す条件: スコープを worker をまたいで共有する段 (async-threads) で、一覧の lock が計測で目立ったとき
- リポジトリに残す所: stdlib.md の「スコープ」(T-4)、vm.md の「セルと登録表」(T-1、T-3、T-4)

#### D-6: ABI と `Scope` の表現
- 決めたこと:
  - `Async` の生の操作は `await_cell` と `reschedule` の2つのままである。ランタイムの入口 `start_task` と `resume_task`、`Wake` のタグは変えない。async.md の残りの4つの操作は後の段で入れる
  - extern の行を13行足して23行にする (D-12 の表)。どれも `pub` でなく、`Purity::Effectful` で、`Op::Async` で動く。預かる引数を持つ行は 8 から 11 行になる (`detach`、`cancel_raw`、`start_in_raw`)
  - `Scope` は `Cell` と同じ形の即値 (`(generation << 32) | index`) で、Kind は `Unr`、Repr は `Int` である (`type_row("Std.Async.Scope", Unr, Repr::Int)`)
  - 前の run の `Scope` を見分けるため、新しい添字の generation は `Machine` が前の run から受け取る数 (`Registry::new(floor)`、`next_floor()`) から始める。`Machine` は run の終わりごとに (誤りと `exit` でも) この数を受け取り、回復では変えない。`Task a` のハンドルは今までどおり受け箱で見分ける
  - `eml_lir::IO_CANCELLED: u32 = 5` を足し、`vm.rs` で `cells::CANCELLED == eml_lir::WAKE_CANCELLED` と `cells::IO_CANCELLED == eml_lir::IO_CANCELLED` を `const` で確かめる
- 理由: 新しい生の操作が要らないので ABI を広げない。`Scope` を `Foreign` にすると、REPL の回復が `Foreign` を写せず、セッションがそれ以降すべて失敗する (試作で再現: `let s = Async.run (fn () -> Async.scope (fn s -> s))` の後の `1 / 0`)。floor がないと、前の run の `Scope` が次の run の同じ添字のスコープを指す (試作で再現)
- 却下した案: `Scope` を `Unr` の `Foreign` にする (回復が壊れる)、generation を 64 ビットにする (`Cell` の形が変わる。U-4)
- 影響する所: 後の段の行、ネイティブの extern の入口
- 見直す条件: generation の一周 (1つの添字で約 0.41 M 回/秒の始めと待ちを約 2.9 時間続けたとき) が実際の問題になったとき
- リポジトリに残す所: stdlib.md の「`Std.Async`」の行の表と `Cell` の項 (T-3、T-4、T-5)、vm.md の「セルと登録表」(T-4)

#### D-7: `drop`、`cancel`、`spawn`
- 決めたこと:
  - `drop t` はタスクを取り消してハンドルを放す。ハンドルの `Drop` は今までどおり `HandleDropped` を積むだけで、`drain` が、終わっていればセルを放し、終わっていなければ持ち手のないタスクにして印を付ける
  - `cancel t` (`cancel_raw`) は、印を付けることと手放すことを1つの行で行う。終わったタスクなら、残っている結果を落とすだけである
  - 新しい行 `detach` は取り消さずにハンドルを手放す。`spawn f = detach (start_raw (bottom f))`。今の `drop` を使う形のままだと `spawn` が自分の子を取り消すためである
  - `await` で待つ側が取り消されたら、包みはハンドルを `drop` して待っていたタスクも取り消し、`Cancelled` を投げる。`Task a` は `Lin` なので、ほかに待つ者はいない
  - 取り消しの地点を通らないタスクは、取り消しても最後まで動く
- 理由: async.md の確定した設計判断 #3。破棄処理は eml のコードを動かさない (linearity.md の「破棄処理」) ので、受け箱を通す
- リポジトリに残す所: stdlib.md の「`Std.Async`」と「取り消し」(T-3)

#### D-8: 操作の予算はタスクのセルに持つ
- 決めたこと: `TaskCell.budget` (128) を、`started` と `resumed` (タスクが始まるときと、待ちから再開するとき) で 128 に戻す。横取りでは戻さない。行 `spend_budget` が1つ減らし、0 になったら真を返す (次の再開まで 0 のまま)。真を受けた包みは `yield_now ()` を呼ぶ。この段で予算を使うのは `check_cancelled` だけで、`cancel_requested` は使わない (row に `Async` がなく、切り替えられない)。async-chan の `try_send` と `try_recv`、すぐに終わる完了の道も同じ行を呼ぶ。ネイティブのコードも同じ核の入口を呼ぶ
- 理由: 数える行はどれも登録表に触れるので `Op::Async` で動く。`Ctx` に置くと eml_rt の Verus の検証と `switch_task` の事後条件の変更が要る。再開のときに戻すので、`parks` がスケジュールによらず、横取りのないネイティブのコードとも同じに動く。async.md:61 と multicore.md:129 の「`Ctx` の予算」とは違う決定なので、段を閉じるときにその2か所を直す
- 却下した案: `Ctx` に置く (上の理由)、切り替えのたびに戻す (seed で `parks` が変わる)
- 影響する所: async-chan、ネイティブのコード
- 見直す条件: 予算を数える行が登録表に触れない文脈から呼ばれるとき
- リポジトリに残す所: stdlib.md の「操作の予算」(T-5)、async.md と multicore.md を直す所 (T-6)

#### D-9: `RunStats.cancels` と seed の比較
- 決めたこと:
  - `RunStats.cancels` を `dropped_failures` の直後に足す (18 の欄)。数えるのは、待つ前に取り消しを見つけた `park`、受け箱の処理が外した待ち、取り消しで再開する `yield_now`、真を返した `cancel_point` である。検査モードだけで数える
  - `cancels` はスケジュールで変わるので、`eml_test_support::schedule_independent` は 0 にする
  - `run/async/` で、逐次と seed のどちらかの run の `cancels` が 0 でなければ、seed の間で `RunStats` を比べない。出力と終わり方は比べる。`cancel_point` の真も数えるので、どちらも 0 なら、どのタスクも取り消しを受けず、本体を最後まで動かしている
- 理由: 取り消しが届くと、届いた時点からそのタスクの仕事が変わり、どの欄もスケジュールで変わりうる。今ある `exit` の例外と同じ理由である。取り消しのテストの数は `tasks.rs` で逐次のスケジュールの下に固定する
- 却下した案: 取り消しのある run でも欄の一部を比べる (始める前に取り消されたタスクは `tasks`、`shared_objects`、`parks`、RC の数まで変える)
- リポジトリに残す所: vm.md の「`RunStats`」、testing.md の「seed で流す」(T-2、T-3、T-5)

### 5.2 段の中の設計

理由があれば実装の中で変えてよい。変えたら記録する。

#### D-10: `cells.rs` の最後の形
```rust
pub(crate) const WOKE: i64 = 0;
pub(crate) const CANCELLED: i64 = 1;      // vm.rs で eml_lir::WAKE_CANCELLED と等しいことを確かめる
pub(crate) const IO_CANCELLED: i64 = 5;   // vm.rs で eml_lir::IO_CANCELLED と等しいことを確かめる
const BUDGET: u32 = 128;

enum Wait { Cell(CellRef), Ready }
enum Waiter { Task(CellRef), Token(WaitToken) }       // 非公開
enum Mark { Task(CellRef), Scope(CellRef) }           // 印を付ける作業の列の項

struct TaskCell {
    phase, outcome, scope /* 加わったスコープ */, worker, spawner, member_at, waiters,
    k: Option<Word>, current: CellRef, wait: Option<Wait>, shield: u32, budget: u32,  // 持ち主だけ
    cancel: bool,                                                                       // 共有
    run: Option<CellRef>,   // 入れ子の run で待っている根
    detached: bool,         // 今の handle_dropped
}
struct ScopeCell {
    parent: Option<CellRef>, opener: Option<CellRef>, flag: bool, closing: bool,
    nested_at: u32, members: Vec<CellRef>, nested: Vec<CellRef>,
    first_failure, waiters, closed,
}
pub(crate) enum Notice { HandleDropped(CellRef), Cancel(CellRef) }
pub(crate) struct TaskStats { tasks, parks, dropped_failures, cancels: u64 }
pub(crate) struct Registry { slots, free, floor: u32, inbox, stats }
```

| 操作 | 振る舞い |
|---|---|
| `Registry::new(floor)`、`next_floor()` | 新しい添字は generation `floor` から始める。`next_floor` は全添字の `generation.wrapping_add(1)` の最大と `floor` の大きい方 |
| 非公開 `new_scope(parent, opener, closing)` | `flag = parent.flag \|\| opener.cancel`。`nested_at` を決めて `parent.nested` に積む |
| `open_root(parent, caller)` | `new_scope(parent, caller, true)`。`caller.run = Some(root)` |
| `start(..) -> Result<Option<(Word, Runnable)>, CellFault>` | 閉じたスコープなら `None`。`cancel = scope.flag`、`current = scope`、`budget = BUDGET`、`members` に積む |
| `started(cell)`、`resumed(cell, wake) -> i64` | 予算を戻す。`resumed` は `wait` が `Ready` で、渡す値が `WOKE` で、取り消しの地点の規則を満たせば `CANCELLED` を返す (`cancels += 1`) |
| 非公開 `point(t)` | `shield == 0 && (cancel \|\| scope(current).flag)` |
| `park(c, me, k)` | `parks += 1`。規則を満たせば待ち手に積まず `Resume CANCELLED` (`cancels += 1`)。相手が終わっていれば `WOKE`。そうでなければ `k` をセルに置き、`wait = Cell(c)`、相手の待ち手に `Waiter::Task(me)` を積む。規則の確認は終わったかの確認より先に行う |
| `ready(me, k)` | `parks += 1`、`wait = Ready`、`Resume WOKE` |
| 非公開 `wake(waiter, from, out)` | `Task(t)`: `t` が生きていて `t.wait == Some(Cell(from))` のときだけ `k` を取り出し、`wait` を空にする |
| `finish(ctx, cell, outcome, depth_offset, out)` | (a) `members` から外す (`swap_remove` し、動いた項の `member_at` を直す) (b) `Finished` と終わり方 (c) 自分の待ち手を `Woke` で起こす (d) 失敗なら最初の失敗に書くか `dropped_failures += 1`、続けて `mark(Scope(scope))` (e) `closing` で `members` が空なら `closed` にして、スコープの待ち手を起こす。最後に、持ち手がなければセルを放す |
| `take(.., Take::Run)` | `release_scope(root)` |
| `detach(ctx, handle, cancel, depth_offset)`、非公開 `let_go(cell, cancel)` | ハンドルを受け取った印を付けて放す。終わっていればセルを放し、終わっていなければ `detached = true`、`cancel` なら `mark(Task)` |
| `scope_cell(i64) -> Option<CellRef>` | 生きているスコープのセル (閉じていてもよい) なら `Some` |
| `open_scope(me) -> i64` | `new_scope(Some(me.current), Some(me), false)`、`me.current = s` |
| `close_scope(s) -> i64` | `closing = true`。`members` が空ならすぐ `closed`。`s` を `Cell` として返す |
| `leave_scope(s, me) -> Result<Option<Raise>, CellFault>` | `closed` と `me.current == s` を要する。`current = parent`、最初の失敗を取り出し、`release_scope` |
| 非公開 `release_scope(s)` | `opener.run == Some(s)` なら空にする。親の `nested` から外して放す |
| `cancel_scope(s: i64)` | 生きていて閉じていなければ `mark(Scope)`。そうでなければ何もしない |
| `fail_scope(s: i64, failure: Outcome)` | 失敗なら記録し、続けて `mark(Scope)` |
| `shield_enter(me)`、`shield_leave(me)` | 深さを ±1 (0 から減らせば内部の誤り) |
| `cancel_point(me: Option<CellRef>) -> bool` | `None` なら偽。真なら `cancels += 1` |
| `spend_budget(me) -> bool` | `budget = budget.saturating_sub(1); budget == 0` |
| 非公開 `mark(first)` | 作業の列。`Task(t)`: ビットが立っていれば戻る。立てる。`t.current` から親へ、`t.scope` の手前までのスコープを `Scope` で積む。`t.run` があれば積む。`Notice::Cancel(t)` を積む。`Scope(s)`: 旗が立っていれば戻る。立てる。`members` を `Task` で、`nested` を `Scope` で積む。`s.opener == Some(o)` で `o.current == s` なら `Notice::Cancel(o)` を積む |
| `drain(ctx, depth_offset, out)` | 受け箱が空になるまで繰り返す。`HandleDropped` は `let_go(.., true)`、`Cancel` は `unpark` |
| 非公開 `unpark(t, out)` | 生きていて、`Parked` で、`wait == Cell(_)` で、規則を満たせば、`wait` を空にし、`k` を取り出し、`cancels += 1`、`Resume CANCELLED`。相手の待ち手の項は残す |

先頭の不変条件のコメント (今の段落を置き換える): `members` と `nested` の項は生きているセルを指す。`waiters` の項は古くてもよく、`wake` が `wait` で確かめる。`k.is_some() ⇔ wait == Some(Cell(_))` (`Ready` の継続は列に積んだ仕事が持つ)。今のスコープから親への列は、そのタスクが開いたスコープからなり、加わったスコープで終わる。入れ子の `run` の根の `opener` は呼んだタスクで、根は誰の今のスコープにもならず、放されるまで呼んだタスクの `run` が名指す。`closed ⇒ closing && members が空`。スコープのセルを放すのは、閉じた後の `release_scope` と `sweep_all` だけである。`cancel` と `flag` は共有で、タスクのセルの残りは持ち主だけが読み書きする。

#### D-11: `vm.rs` の呼び出し
| 場所 | 変更 |
|---|---|
| `switch_away`、`preempt` の頭 | 新しい補助 `drain::<R>(code, ctx, depth_offset, sched)`: `cells.drain(.., &mut out)` を呼び、返った仕事を `schedule` する。`out` が空でないのに入口がなければ `the Async entries are not loaded` |
| `main` の `Finish` | `cells.drain(.., &mut out)` の後、`out` が空でなければ内部の誤り `a task is left at the end`。その後で `sweep_all` |
| `enter` | `args` を `mut` にし、再開の枝で `args[1] = Word::from_bits(cells.resumed(cell, args[1].bits() as i64)? as usize)` |
| `AsyncStartRaw` | `current_of(me)` のスコープに加える。`None` は内部の誤り `a task starts in a closed scope` |
| `AsyncRunRaw` | 親は `task.cell.map(current_of)`。`open_root(parent, task.cell)` |
| `AsyncStartInRaw` | `current_cell(task)?`。`scope_cell(task.raw(arg(0)))` から `start`。`None` なら預かった閉包を `decref` してから `raise_again(Raise::Panic("the scope is closed"))` |
| `AsyncDetach`、`AsyncCancelRaw` | `cells.detach(ctx, task.get(arg(0)), ext == AsyncCancelRaw, depth_offset)`。`current_cell` は要らない |
| `AsyncOpenScope` | `set_raw(dst, cells.open_scope(current_cell(task)?)?)` |
| `AsyncCloseScope` | `set_raw(dst, cells.close_scope(task.raw(arg(0)))?)` |
| `AsyncLeaveScope` | `Some(raise)` なら `raise_again`。そうでなければ `UNIT` |
| `AsyncCancelScopeRaw` | `cells.cancel_scope(task.raw(arg(0)))`。`current_cell` は要らない |
| `AsyncFailScopeIo`、`AsyncFailScopePanic` | 文言を `copy_string` で写して手放し、`cells.fail_scope(raw(arg(0)), Outcome::Io{..} か Panic(..))` |
| `AsyncShieldEnter`、`AsyncShieldLeave`、`AsyncSpendBudget` | `current_cell` が要る |
| `AsyncCancelPoint` | `cells.cancel_point(task.cell)`。`set_raw(dst, i64::from(b))` |
| `run_extern` の断る腕 | 23 の `Async*` の変種すべて |
| `Scheduler::new` | 引数 `generations` を足す (呼ぶ所は `lib.rs` の1か所)。`Start` と `Scheduled` と `Machine.generations: u32` で受け渡す |

VM の `Task` の欄、`Ctx`、`eml_rt` は変えない。

#### D-12: extern の行
すべて `Purity::Effectful`、`pub` でなく、`Op::Async` で動き、`is_async()` に入れる。借りる引数はない。`Unr` だけの行は `row(name, &params, ret, Purity::Effectful)`、`Lin` か `Poly` を持つ3行は `holding(&[..], kinded(..))` で書く。

| 変種 | 名前 | std の型 | 引数の Repr | 印 | 返り値 / 印 | 預かる | task |
|---|---|---|---|---|---|---|---|
| `AsyncDetach` | `Std.Async.detach` | `Task a -> <IO> Unit` | `[Obj]` | `[Lin]` | `Unit` / `Unr` | `[true]` | T-3 |
| `AsyncCancelRaw` | `Std.Async.cancel_raw` | `Task a -> <IO> Unit` | `[Obj]` | `[Lin]` | `Unit` / `Unr` | `[true]` | T-3 |
| `AsyncShieldEnter` | `Std.Async.shield_enter` | `Unit -> <IO> Unit` | `[Unit]` | `[Unr]` | `Unit` / `Unr` | – | T-3 |
| `AsyncShieldLeave` | `Std.Async.shield_leave` | `Unit -> <IO> Unit` | `[Unit]` | `[Unr]` | `Unit` / `Unr` | – | T-3 |
| `AsyncStartInRaw` | `Std.Async.start_in_raw` | `Scope -> (Unit -> <IO, Async> Unit) -> <IO, Panic> Task a` | `[Int, TObj]` | `[Unr, Poly]` | `Obj` / `Lin` | `[false, true]` | T-4 |
| `AsyncOpenScope` | `Std.Async.open_scope` | `Unit -> <IO> Scope` | `[Unit]` | `[Unr]` | `Int` / `Unr` | – | T-4 |
| `AsyncCloseScope` | `Std.Async.close_scope` | `Scope -> <IO> Cell` | `[Int]` | `[Unr]` | `Int` / `Unr` | – | T-4 |
| `AsyncLeaveScope` | `Std.Async.leave_scope` | `Scope -> <IO, Panic> Unit` | `[Int]` | `[Unr]` | `Unit` / `Unr` | – | T-4 |
| `AsyncCancelScopeRaw` | `Std.Async.cancel_scope_raw` | `Scope -> <IO> Unit` | `[Int]` | `[Unr]` | `Unit` / `Unr` | – | T-4 |
| `AsyncFailScopeIo` | `Std.Async.fail_scope_io` | `Scope -> IOErrorKind -> String -> <IO> Unit` | `[Int, Enum, Obj]` | `[Unr; 3]` | `Unit` / `Unr` | – | T-4 |
| `AsyncFailScopePanic` | `Std.Async.fail_scope_panic` | `Scope -> String -> <IO> Unit` | `[Int, Obj]` | `[Unr; 2]` | `Unit` / `Unr` | – | T-4 |
| `AsyncCancelPoint` | `Std.Async.cancel_point` | `Unit -> <IO> Bool` | `[Unit]` | `[Unr]` | `Enum` / `Unr` | – | T-5 |
| `AsyncSpendBudget` | `Std.Async.spend_budget` | `Unit -> <IO> Bool` | `[Unit]` | `[Unr]` | `Enum` / `Unr` | – | T-5 |

行の数は 10 → 14 (T-3) → 21 (T-4) → 23 (T-5)、預かる引数を持つ行は 8 → 10 (T-3) → 11 (T-4) になる。`Bool` は `Std.Env.has` と同じく `Repr::Enum` のタグ 0/1 である。

タスクの外での振る舞い: `cancel_point` は偽を返す。`cancel_raw`、`detach`、`cancel_scope_raw`、`fail_scope_*` はセルの番号だけで動くので、タスクの外でも動く。前の run の `Scope` は閉じたスコープとして扱う (何もしないか `Panic`)。残りの行は今のセルが要り、タスクの外では内部の誤り `an Async operation runs outside a task` になるが、どれも `Async` を持つ包みからしか呼ばれないので、検査を通ったプログラムからは届かない。

#### D-13: `await` の競合を spec に書き、既存のテストを `shield` で直す
失敗したタスクを同じスコープの中から `await` で待つタスクは、失敗が記録されたときにもう待っていればその失敗を受け取り、まだ待っていなければスコープの取り消しを受けて `Cancelled` を受け取る。どちらになるかはスケジュールで決まる。スコープの最初の失敗はどちらでも同じなので、`run` と `scope` が起こし直す失敗は変わらない。待つ相手の失敗を確かに受け取るには `await` を `shield` で囲む。これを stdlib.md の「取り消し」に書く (T-3)。`tests/ui/run-fail/async/caught_await_still_fails_run.em` は、今のままだと seed の約半分で `caught Cancelled` を出す (100 の seed のうち 52) ので、`await` を `shield` で囲む。`shield` の中では `park` と `unpark` がビットを見ず、`finish` は印を付ける前に待ち手を起こすので、どのスケジュールでも `caught TimedOut` になる (seed 300 と既定の速いモード 50 回で確かめた)。`spin` の回数を増やす直し方は時間で切り替える速いモードでは機械の速さに依るので採らない。

#### D-14: task の分け方
- 欄はそれを初めて読む commit で足す。読まない欄は clippy が通さない
- T-1 は振る舞いを変えない組み替えで、テストを変えない。T-2 は 44 のスナップショットの機械的な書き換えを1回で済ませ、T-3 の UI テストが要る比較の規則を先に入れる。T-3 は `Scope` なしで届く取り消しのすべてと `shield` (D-13 でテストを直すのに要る)、T-4 は名前付きのスコープ、T-5 は柔らかい確かめと予算である
- `std/Async.em` の 1〜54 行目はどの task でも変えない。新しい定義は `run` の後に、付録の `Async-T*.em` の順で置く。そうすれば既存のスナップショットの位置 `<std>/Async.em:54:9` (`run-fail/async/` の3本と新しい `cancelled_run`) と `13:3`、`15:3` (`check-fail/names/async_op_handled.em`) が動かない

## 6. 実装の約束

- `crates/eml_rt` を変えない (D-8、D-10)。各 task の終わりに `git diff --stat main -- crates/eml_rt` → 出力なし
- `cells.rs` は `std` と `eml_rt` だけを import する (D-3)
- `CANCELLED` で再開する仕事をほかのタスクのために作るのは `drain` だけ (D-3)
- `finish` の順は D-10 の (a)〜(e) (D-5、D-13)
- 種類 `Cancelled` の判定は `Outcome::failure()` の1か所だけ (D-1)
- 予算を戻すのは `started` と `resumed` だけ (D-8)
- 各 task は TDD で進める。その task のテストを先に書き、失敗するのを見てから実装する
- 各 commit のコメントと spec の引用は、その commit にある見出しだけを指す (`crates/eml_cli/tests/citations.rs` は `docs/plans` だけを除いて調べる)。コードと `docs/` から `docs/plans/` を引かない
- spec の節は規則を実装する commit と同じ commit で書く。付録のパッチにある `(仮)` の見出しはその task の「spec」の文に置き換える
- 日本語 (コードのコメント、spec、testing.md など) は書く前に yomiyasu スキルを呼び、その規則で書く。付録のパッチと `Async-*.em` のコメントも yomiyasu を通してから入れる。意味は変えない
- 付録のパッチは参照の実装で、そのまま当ててよい。当てたら、差分をこの計画書と照らして読み、食い違いがあれば計画書を優先して直す
- 臨時に `.em` を動かすときは `timeout 20 cargo run -p eml_cli -- run --debug-heap [--sched seeded:N] <file>` とする
- UI テストの書き方は付録 `ui-tests.md` の冒頭の約束に従う (D-13、D-14)

各 task の共通の確認 (devShell の中で):
- `cargo nextest run` → 失敗 0
- `cargo clippy --all-targets -- -D warnings` → 終了コード 0
- `cargo fmt --check` → 終了コード 0
- `cargo clippy -p eml_cli --no-default-features -- -D warnings`、同じく `--features types`、`--features core` → どれも終了コード 0
- `git diff --stat main -- crates/eml_rt` → 出力なし

## 7. 実装の task

| ID | task | 決めたこと | 前提 | 並べられるか |
|---|---|---|---|---|
| T-0 | 計画の commit (`Plan async-cancel`。この文書と付録) | – | – | – |
| T-1 | セルの組み替え (継続をタスクのセルに、スコープの木)。振る舞いは変えない | D-2、D-5、D-10、D-14 | T-0 | T-2 と並べられる |
| T-2 | `RunStats.cancels` と seed の比較の規則 | D-9、D-14 | T-0 | T-1 と並べられる |
| T-3 | タスクの取り消しと `shield` | D-1〜D-4、D-6、D-7、D-9〜D-13 | T-1、T-2 | いいえ |
| T-4 | 名前付きのスコープ | D-4〜D-6、D-10〜D-12 | T-3 | いいえ |
| T-5 | `check_cancelled`、`cancel_requested`、操作の予算 | D-4、D-8、D-9、D-12 | T-4 | いいえ |
| T-6 | 段を閉じる (`Close async-cancel`) | すべて | T-5 | いいえ |
| T-7 | 計測 (`Measure async-cancel`)。macOS がないときだけ | D-14 | T-6 | いいえ |

### T-0: 計画の commit
- 目的: 承認した計画を段の最初の commit にする (roadmap の「進め方」)
- 対象: `docs/plans/2026-10-27-async-cancel.md`、`docs/plans/2026-10-27-async-cancel/`
- 条件: 計画の作成のときに、承認を受けてから行う。実装を始める時点で `git log --oneline main.. -- docs/plans/2026-10-27-async-cancel.md` に `Plan async-cancel` があれば済んでいる
- 完了の条件: `git show --stat HEAD -- docs/plans` に計画書と付録の8つのファイルが出る

### T-1: セルの組み替え
- 目的: 継続の持ち主とスコープの木を、振る舞いを変えずに入れ、T-3 が取り消しだけを足せるようにする
- 対象: `crates/eml_vm/src/cells.rs`、`crates/eml_vm/src/vm.rs` (`AsyncRunRaw` の腕)、`docs/spec/vm.md`。参照の実装は `T-1.patch`
- 作業:
  - `Waiter { Task(CellRef), Token(WaitToken) }` (非公開) が `Cont { task, k }` を置き換える。`TaskCell` に `k: Option<Word>` (セルが参照を1つ持つ) と `member_at: u32` を足し、`handle_dropped` を `detached` にする
  - `ScopeCell { parent, nested_at, members, nested, first_failure, waiters, closed }`。`live` を消す (数は `members.len()`)
  - `wake` は `Task(t)` のとき `t.k.take()` で再開の仕事を作る。`park` は終わっていない相手なら `me.k = Some(k)` にして `Waiter::Task(me)` を積む
  - `open_root(parent: Option<CellRef>)` は、`parent` があれば `nested_at` を決めて `parent.nested` に積む。`start` は `members` に積む
  - `finish` を D-10 の (a)〜(e) の順にする。(d) の印付けはまだない。(e) の条件は `members` が空
  - `release_scope` を足し、`take(Take::Run)` が使う
  - `vm.rs` の `AsyncRunRaw`: `let parent = task.cell.map(|c| sched.cells.scope_of(c)).transpose()?; let root = sched.cells.open_root(parent)?;`
  - `cells.rs` の先頭の不変条件のコメントに、`members` と `nested` の項が生きているセルを指すことと、スコープのセルを放す所を書く
- spec (`docs/spec/vm.md` の「`Async` のタスク」):
  - 「セルと登録表」のタスクのセルの項: 「タスクのセルは、状態 (置いた、動いている、待っている、終わった)、終わり方、加わったスコープとその一覧の中の位置、論理的な worker ID、始めたタスクの ID、待ち手、待っている間の継続、持ち手がないかを持つ。」
  - 同じくスコープのセルの項: 「スコープのセルは、親のスコープ、属するタスクの一覧、入れ子のスコープの一覧と親の一覧の中の位置、終わった順で最初の失敗、待ち手、閉じたかを持つ。終わっていないタスクの数は、属するタスクの一覧の長さである。`main` の `run` の根は親を持たず、タスクの中の入れ子の `run` の根は呼んだタスクのスコープを親にする。タスクは終わるときに一覧から外れ、一覧が空になるとスコープは閉じる。一覧の項はいつも生きているセルを指す。」
  - 「待ちの2種類」の1つ目の項 (`Waiter::Cont` → `Waiter::Task`): 「継続は待つタスクのセルが持ち、待つ相手のセルの待ち手はタスクのセルの番号だけを持つ。起こす側はタスクのセルから継続を取り出して再開の仕事を作る。」
  - 「命令」の `start_raw` の項 (:505): 「スコープの `live` を1つ増やし」→「スコープの属するタスクの一覧に加え」
  - 「終わり」の核の `finish` の項 (:521) を (a)〜(e) の順の文にする。:524 「スコープのセルを放すのは `live` が 0 のときだけで…」→「スコープのセルを放すのは閉じた後だけで、`run_result` の `take` と `sweep_all` が放す。`take` は親の一覧から外してから放す」
- 実装者に任せること: 補助の関数の名前。`members` と `nested` の `swap_remove` の補助を1つにするか
- 条件: いつも
- 完了の条件:
  - 共通の確認 → すべて通る (基準では `cargo nextest run` が 3768 件通る。この task はテストを足さない)
  - `git diff --stat HEAD~1 -- tests crates/eml_cli/tests crates/eml_vm/tests` → 出力なし
- 落とし穴: `swap_remove` は最後の項を穴に移すので、移った項の `member_at` か `nested_at` を直す。`opener`、`flag`、`closing`、`wait`、`current` はまだ足さない (読む所がなく clippy が通さない)

### T-2: `RunStats.cancels` と seed の比較の規則
- 目的: 18 番目の欄を1回で足し、T-3 の UI テストが要る比較の規則を先に入れる
- 対象: `crates/eml_vm/src/lib.rs`、`cells.rs` (`TaskStats.cancels`)、`crates/eml_test_support/src/lib.rs`、`crates/eml_cli/tests/ui.rs`、`crates/eml_vm/tests/bench.rs`、`docs/spec/vm.md`、`docs/implementation/testing.md`、`docs/implementation/architecture.md`。参照の実装は `T-2.patch`
- 作業:
  - `RunStats` の `dropped_failures` の直後に `pub cancels: u64` を足し、`run_stats` で `if M::CHECKED { scheduled.tasks.cancels } else { 0 }` とする。`TaskStats.cancels: u64` を足す (まだ増やさない)
  - `schedule_independent` で `stats.cancels = 0;` とし、doc に理由を書く
  - `ui.rs` の `assert_seeds_agree` の seed ごとの比較を、次の関数で絞る (コメントは yomiyasu を通す):
    ```rust
    /// 取り消しの届いた run は比べない。届いた時点で、それぞれのタスクがした仕事が変わる
    /// (docs/implementation/testing.md の「seed で流す」)
    fn compares_stats(checked: &Ran, other: &Ran) -> bool {
        let quiet = |ran: &Ran| ran.finished.is_none_or(|f| f.stats.cancels == 0);
        quiet(checked) && quiet(other)
    }
    // ループの中で:
    let compare = compare_stats && compares_stats(checked, &ran);
    assert_agrees(suffix, &seeded_label(seed), &ran, checked, compare);
    ```
  - `bench.rs` の 44 のスナップショットに `cancels: 0,` を足す: `sed -i 's/^\(\s*\)dropped_failures: 0,$/&\n\1cancels: 0,/' crates/eml_vm/tests/bench.rs`
- spec (この時点で正しい文だけを書く):
  - vm.md の「`RunStats`」: 「欄は次の18で」。`dropped_failures` の後に新しい項「`cancels`: 取り消しが届いた回数。登録表が数える」。「この4つの欄は、スケジュールの方針によらない。…」の段落の後に「`cancels` はスケジュールで変わる」。T-3 と T-5 が数える出来事を足す
  - vm.md の「追記」(:65) の「実行ごとの登録表が数える `tasks`、`parks`、`dropped_failures`」に `cancels` を足す
  - testing.md の「seed で流す」の「比べるのは…」の項の `exit` の例外の後: 「`run/async/` で、逐次か seed のどちらかの run の `cancels` が 0 でなければ、`RunStats` を比べない。取り消しが届くと、届いた時点からそのタスクの仕事が変わるためである」。:360 に「どの run も `cancels` が 0 のときに限る」。`schedule_independent` の一覧に「`cancels` は 0 にして比べない」
  - architecture.md:312: 「`tasks`、`parks`、`dropped_failures`、`cancels` は登録表が数える」
- テスト:
  - 新しい `ui.rs::stats_are_compared_only_without_cancellations` (付録 `ui-tests.md` の最後の節)。`RunStats::default()` から作り (`#[non_exhaustive]` なので欄を代入する)、0/0 なら真、1/0 と 0/1 なら偽、両方 `finished: None` なら真
  - 期待値の変更 (種類 2): `bench.rs` の 44 のスナップショットに `cancels: 0,`。理由は欄を足したこと
- 条件: いつも
- 完了の条件:
  - 共通の確認 → すべて通る
  - `rg -c 'cancels: 0,' crates/eml_vm/tests/bench.rs` → `44`
  - `rg -l 'shared_objects:' crates/*/tests` → `crates/eml_vm/tests/bench.rs` だけ
  - `cargo nextest run -p eml_cli --test integration -E 'test(ui::stats_are_compared_only_without_cancellations)'` → 1 件通る

### T-3: タスクの取り消しと `shield`
- 目的: 取り消しの核と届け方、`drop`、`cancel`、`spawn`、失敗したタスクによる取り消し、入れ子の `run` への取り消し、`shield` を入れる
- 対象: `cells.rs`、`vm.rs`、`crates/eml_extern/src/lib.rs`、`crates/eml_lir/src/lib.rs`、`std/Async.em` (= 付録 `Async-T3.em`)、`crates/eml_core_ir/tests/externs.rs`、`crates/eml_vm/tests/tasks.rs`、新しい UI テスト、`tests/ui/run/async/drop_handle.em`、`tests/ui/run-fail/async/caught_await_still_fails_run.em`、docs。参照の実装は `T-3.patch`
- 作業:
  - `eml_lir`: `pub const IO_CANCELLED: u32 = 5;` を `IO_OTHER` の隣に足す
  - `eml_extern`: D-12 の T-3 の4行。`is_async()` に足す
  - `cells.rs` は D-10 から T-4 と T-5 の分を除いた形にする:
    - 取り消しの地点の規則は `shield == 0 && (cancel || scope(t.scope).flag)` (今のスコープはまだない)
    - `mark(Task(t))` はビットを立て、`t.run` があれば `Scope` で積み、届ける。`mark(Scope(s))` は旗を立て、`members` と `nested` を積む
    - `open_root(parent, caller)` は `opener = caller`、`flag = parent.flag || caller.cancel`、`caller.run = Some(root)`。`release_scope` が `run` を空にする
    - `wake` は `wait == Some(Cell(from))` のときだけ `k` を取り出す。`unpark` は待ち手の項を残す
    - `Outcome::failure()` は `Io { kind: IO_CANCELLED, .. }` に `None` を返す
    - `shield_enter`、`shield_leave`、`detach`、`let_go`、`drain` の繰り返し、`unpark`、`resumed(cell, wake)`、`Notice::Cancel`
  - `vm.rs`: D-11 の `drain` の補助と2か所の頭、`main` の `Finish`、`enter`、`AsyncRunRaw`、T-3 の4行の腕、断る腕、2つの `const` の確認
  - std: 付録 `Async-T3.em` のとおり (`raise_cancelled`、`spawn` を `detach` で、`await` の `Cancelled` の枝、`cancel`、`yield_now` が投げる、`shield`、4つの extern、ABI のコメントの預かる引数)。コメントは「取り消し」と「`Std.Async`」だけを引く
- spec:
  - stdlib.md の「`Std.Async`」:
    - 表に `cancel` と `shield` の行
    - :931 の最後の文 →「タスクの中で `run` を入れ子に呼ぶと、その `run` は呼んだタスクのスコープの下に自分の根を開く。外のスコープの取り消しと呼んだタスクの取り消しは、その根のタスクにも届く (下の「取り消し」)」
    - :932 に「失敗したタスクは、同じスコープのほかのタスクを取り消す (下の「取り消し」)」を足す
    - :934 →「`drop t` と `cancel t` は、タスクを取り消してハンドルを放す。終わったタスクなら、残っている結果を落とす。`spawn` はタスクを取り消さずにハンドルを放す。取り消しの地点を通らないタスクは、取り消しても最後まで動く」
    - :937 に「取り消しの地点である」を足す
    - `Wake` の項に「ランタイムは取り消したタスクを `Cancelled` で再開する」を足す
    - 行の表に4行、「次の14行」。:32 の預かる行の一覧を10行に (`detach`、`cancel_raw`)
    - :734 (`Child` の drop) の `[非同期の設計](../future/async.md) の「スコープとタスク」` →「(下の「取り消し」)」
  - stdlib.md に新しい `### 取り消し` を「`Failure` と `try_all`」の前に足す:
    - 「取り消しは2つの所から来る。タスクの取り消しのビットは、`cancel t`、`drop t`、属するスコープの取り消し、取り消したスコープに加えることで立つ。スコープの取り消しの旗は、スコープのタスクの失敗と親のスコープの取り消しで立ち、開くときに親の旗を写す。どちらも一度立てたら下ろさない」
    - 「タスクにビットが立つと、そのタスクが入れ子の `run` で待っている根にも旗が立つ。ビットが立った後に呼ぶ入れ子の `run` の根も、開くときに旗を立てる」
    - 「取り消しの地点は `await` と `yield_now` である。取り消しの地点は、`shield` の外で、タスクのビットかタスクのスコープの旗が立っていれば、種類 `Prelude.Cancelled`、文言 `task was cancelled` の IO の失敗を投げる。待つ前に立っていれば待たずに投げ、待っている間に立てば待ちを外して投げる。`yield_now` で実行待ちの列にいる間に立てば、その `yield_now` が投げる。待っていた相手が終わって起こされた後に立ったときは、`await` は結果を受け取り、次の取り消しの地点が投げる」
    - D-13 の文 (「失敗したタスクを同じスコープの中から `await` で待つタスクは、…`await` を `shield` で囲む」)
    - 「取り消しは協調的で、取り消しの地点を通らないコードは止まらない」
    - 「投げるのは普通の IO の失敗なので、タスクのフレームを巻き戻して `Lin` の値を落とし、`try_io` と `try_all` が捕まえる。捕まえても取り消しは続き、次の取り消しの地点がまた投げる」
    - 「種類 `Cancelled` の IO の失敗で終わったタスクは、スコープの失敗にならず、旗も立てない。利用者が `fail_io Prelude.Cancelled` で投げた失敗も同じである。`await` はそれを種類と文言のまま起こし直し、待たないタスクなら報告しない」
    - 「`Cancelled` が `await` か `run` から出ると、呼んだタスクの普通の IO の失敗になる。捕まえない `Cancelled` は実行時エラー `task was cancelled` になる」
    - 「`await t` で待つ側が取り消されたときは、`t` も取り消す」
    - 「`shield f` の中の取り消しの地点は、ビットも旗も見ない。`shield` の中の待ちは、取り消しで外れない。`shield` を出た後の最初の取り消しの地点が投げる」
    - 「`cancel` の row は `Async` を持たないので、タスクの外でも呼べる。`run` は待つ間も取り消しの地点でない」
  - vm.md の「`Async` のタスク」:
    - 「セルと登録表」: タスクのセルに「取り消しのビット、待ちの登録 (セルの待ちか実行待ち)、`shield` の深さ、入れ子の `run` で待っている根」、スコープのセルに「開いたタスク (入れ子の `run` の根では呼んだタスク、`main` の根ではなし)、取り消しの旗」。受け箱の項に「受け箱は、落ちたハンドルの知らせと取り消しの知らせを積む。受け箱の処理は、知らせがなくなるまで繰り返す」
    - 「命令」: `park` に「待つ前に下の「取り消しの届け方」の規則を確かめる」、`ready` に「待ちの登録を実行待ちにする」、:511 の「`k` に `Woke` を渡す」→「`k` に `Woke` か `Cancelled` を渡す。実行待ちから再開するときは、核の `resumed` が渡す値を決める」。新しい項 `detach t`「ハンドルを消費し、受け取った印を付けて手放す。タスクが終わっていればセルを放し、終わっていなければ持ち手のないタスクにする」、`cancel_raw t`「`detach` と同じく手放し、終わっていないタスクには取り消しの印を付ける」、`shield_enter` と `shield_leave`
    - :239 の「`Std.Async` の10行」→ 14
    - 新しい `### 取り消しの届け方` (下の文。角括弧の部分は T-4 で足す):
      - 「取り消しは、印を付けることと届けることの2段で行う。印を付けるのは、タスクの取り消しのビットとスコープの取り消しの旗を立て、スコープの属するタスクと入れ子のスコープへたどることである。どちらも一度立てたら下ろさないので、2度目は何もしない。たどる道は作業の列で持ち、Rust の再帰を使わない」
      - 「印を付けるのは、核の `finish` (失敗したタスクのスコープ)、`cancel_raw`、落ちたハンドルの知らせの処理 [、`cancel_scope_raw`、`fail_scope_io`、`fail_scope_panic`] である」
      - 「タスクにビットを立てると、そのタスクが待っている入れ子の `run` の根 [と、そのタスクが開いているスコープ] にも旗を立てる。[スコープに旗を立てると、そのスコープを開いたタスクの今のスコープがそのスコープなら、そのタスクに届ける (ビットは立てない)。]」
      - 「届けることは、タスクのセルの番号を持つ取り消しの知らせを受け箱に積むことである。待ちを外すのは受け箱の処理だけである。待ちを外すときは、継続を取り出し、待ちの登録を空にする。待つ相手の待ち手の項は残し、相手が終わるか閉じたときに、まだそのセルを待つタスクだけを起こす。もう起こされていれば何もしない」
      - 「受け箱を処理するのは、ほかのタスクに切り替える所と横取りを試みる所の頭なので、印を付けてから待ちを外すまでに、ほかのタスクは動かない。`main` の `Finish` の受け箱の処理で再開の仕事ができれば、内部の誤り (`a task is left at the end`) にする」
      - 「取り消しの地点の規則: `shield` の外で、タスクのビットか [今の] スコープの旗が立っている。`park` は待つ前にこれを確かめ、満たせば待ち手に積まずに `Cancelled` で再開する仕事を返す。相手がもう終わっていても `Cancelled` である。`ready` は待ちの登録を実行待ちにし、再開する仕事が動き始めるときに `resumed` がこれを確かめて、渡す値を `Cancelled` に替える。実行待ちの再開は品物も結果も運ばないので、替えても失うものはない。終わったタスクか閉じたスコープが起こした `Woke` は替えず、次の取り消しの地点が投げる」
      - 「継続はいつも待つタスクのセルが持ち、待ち手はタスクのセルの番号だけを持つ。起こす側も外す側もタスクのセルから継続を取り出すので、先に取り出した側が勝ち、両方が起きることもどちらも起きないこともない」
      - 「ビットと旗はどのタスクの操作からも書く。状態、継続、待ちの登録、`shield` の深さ、[予算、今のスコープ、] 待ち手は、そのタスクを動かすか受け箱を処理する持ち主だけが読み書きする。ネイティブのコードも同じ核の入口を呼び、`Cancelled` の再開は `Woke` と同じ `resume_task` の入口を通る」
    - 「終わり」の (d): 「失敗なら、スコープの最初の失敗に書き (2つ目からは書かない)、スコープに取り消しの印を付ける。種類 `Cancelled` の IO の失敗は失敗に数えない」。`main` の `Finish` の項に、受け箱の処理で再開の仕事ができたら内部の誤り
    - 「`RunStats`」の `cancels`: 「待つ前に取り消しを見つけた `park`、受け箱の処理が外した待ち、取り消しで再開する `yield_now` を数える (「`Async` のタスク」の「取り消しの届け方」)」
  - core-ir.md の「表の印の列」の表に `Std.Async.detach`、`cancel_raw` の行 (`Lin` / `Unr`)
  - declarations.md:121 →「14個」
  - testing.md: 「UI テスト」の `async/` の行に「取り消し」。`tasks.rs` の段落 (:201) に「待っているタスクのハンドルを落としたときの数」
- テスト:
  - 新しい UI テスト `run/async/`: `drop_cancels`、`cancel`、`failed_child_cancels_siblings`、`cancel_persists`、`nested_run_cancelled`、`cancel_reaches_nested_run`、`cancelled_kind`、`cancelled_file` (付録 `ui-tests.md`)
  - 新しい `tasks.rs::a_dropped_handle_cancels_a_parked_task`: 逐次のスケジュールで `(tasks, parks, dropped_failures, cancels) == (3, 3, 0, 2)`
  - 期待値の変更 (種類 2):
    - `externs.rs::only_the_listed_rows_carry_a_lin_or_poly_mark` (`"Std.Async.detach" | "Std.Async.cancel_raw" => (&[Lin], Unr)`、doc の数 8 → 10) と `only_the_async_rows_hold_an_argument` (一覧に2つ、doc の数 8 → 10)。理由は行を足したこと
    - `run-fail/async/caught_await_still_fails_run.em`: `Async.await t` →`Async.shield (fn () -> Async.await t)`、コメントも直す。スナップショットは同じバイト列。理由は D-13
  - 機械的な追随 (種類 3): `the_io_error_kind_tags_match_the_prelude_declaration` の `("Cancelled", 5)` →`("Cancelled", eml_lir::IO_CANCELLED)`、コメントの「次の5つ」→「次の4つ」(同じ値)。`run/async/drop_handle.em` のコメントだけを「`drop` で落としても、取り消しの地点を通らないタスクは最後まで動く。落としたタスクの `Lin` の結果は終わったときに放し、検査モードで漏れが出ない (docs/spec/stdlib.md の「取り消し」)。」にする (出力は `still runs` のまま)
- 実装者に任せること: コメントの言い回し。`point` を `park`、`resumed`、`unpark` でどう共有するか
- 条件: いつも
- 完了の条件:
  - 共通の確認 → すべて通る
  - `cargo nextest run -p eml_vm --test integration -E 'test(tasks::)'` → 13 件通る
  - `cargo nextest run -p eml_cli --test integration -E 'test(ui::run) | test(ui::run_fail)'` → 2 件通る
  - `timeout 20 cargo run -p eml_cli -- run --debug-heap tests/ui/run/async/drop_cancels.em` → 標準出力が `victim: Cancelled: task was cancelled` と `after run` の2行、終了コード 0
  - `nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_vm --test integration miri::` → 通る。`-Zmiri-tree-borrows` を足してもう1回 → 通る (`switch_away` と `preempt` の頭が変わるため)
- 落とし穴:
  - `drain` は繰り返す。1回だけだと、`HandleDropped` の印付けが積んだ `Cancel` が残り、最後のタスクが終わった所で `switch_away` が `no task is ready to run` で失敗する
  - `unpark` と `wake` は放したセルに出会う (自分のスコープを取り消したタスクが、次の `drain` の前に終わって放されることがある)。generation を確かめる引き方で読み、なければ飛ばす
  - `park` は取り消しの地点の規則を、終わったかの確認より先に行う (取り消されたタスクが終わったタスクを `await` すると `Cancelled`)
  - `finish` は印を付ける前に `members` から外し (自分に印を付けない)、印を付ける前に自分の待ち手を起こす (待ち手が失敗を受け取る)
  - 算術をするループは row に `Panic` が要る (`+` が `Panic` を起こしうる。E2002)

### T-4: 名前付きのスコープ
- 目的: `Scope` と `scope`、`start_in`、`spawn_in`、`cancel_scope` を入れ、今のスコープと印付けの全体を仕上げる
- 対象: `cells.rs`、`vm.rs`、`sched.rs`、`crates/eml_vm/src/lib.rs`、`eml_extern`、`std/Async.em` (= 付録 `Async-T4.em`)、`externs.rs`、新しい UI テストと REPL のトランスクリプト、docs。参照の実装は `T-4.patch`
- 作業:
  - `eml_extern`: `ExternType::Scope => type_row("Std.Async.Scope", Unr, Repr::Int)` を `Task` の後に、D-12 の T-4 の7行を足し、`is_async()` に入れる
  - `cells.rs`: `current` と `closing`、`Registry::new(floor)` と `next_floor()` (`Default` を置き換える)、`current_of` (`scope_of` を置き換える)、`new_scope`、`start` が `Option` を返す形、取り消しの地点の規則が今のスコープの旗を読む形、`mark(Task)` の今のスコープから加わったスコープの手前までのたどり、`mark(Scope)` の開いたタスクへの届け、スコープの操作 (D-10)
  - `vm.rs`: D-11 の T-4 の腕。`AsyncStartRaw` は `current_of`、`AsyncRunRaw` の親も `current_of`
  - `sched.rs` と `lib.rs`: `Scheduler::new(policy, main_worker, max_depth, generations)`、`Start { .., generations }`、`Scheduled { .., generations: sched.cells.next_floor() }`、`Machine.generations: u32` (作るときは 0。run の終わりごとに、誤りでも `Scheduled` から受け取る。回復では戻さない)
  - std: 付録 `Async-T4.em` のとおり (`pub extern data Scope`、`scope`、`start_in`、`spawn_in`、`cancel_scope`、`reraise`、`fail_scope`、`wait_closed`、7つの extern)
- spec:
  - stdlib.md の「`Std.Async`」:
    - 表に `scope`、`start_in`、`spawn_in`、`cancel_scope` の行
    - :931 →「`start` と `spawn` は新しいタスクを、呼んだタスクの今のスコープに加える。今のスコープは、タスクが `scope` で開いたいちばん内側のスコープで、開いていなければタスクが加わったスコープである」。入れ子の `run` の文は「呼んだタスクの今のスコープの下に」にする
    - ABI のブロックに `pub extern data Scope`。`Cell` の項に「`Scope` は `Cell` と同じ形の即値で、Kind は `Unr`、Repr は `int` である」
    - 行の表に7行 (`start_in_raw` の預かる引数は「閉包 (2つ目の引数)」)、「次の21行」。:32 の預かる行の一覧を11行に
  - stdlib.md に新しい `### スコープ` を「取り消し」の前に足す:
    - 「`scope f` は、呼んだタスクの今のスコープの子のスコープを開いて今のスコープにし、同じタスクの中で `f s` を動かす。`f s` が値を返しても、失敗しても、取り消されても、スコープのすべてのタスクが終わるまで待ってから、今のスコープを元に戻す。この待ちは `shield` の中で行うので、取り消しで外れない」
    - 「`scope` の結果は、スコープに最初に記録した失敗 (本体か子かによらず、終わった順) を起こし直すか、記録がなければ本体の結果 (値、失敗、`Cancelled`) である。本体の失敗も記録し、記録した時点で子をすべて取り消す。2つ目からの失敗は報告せず、`RunStats.dropped_failures` に数える」
    - 「`start_in s f` と `spawn_in s f` は、名前を付けたスコープ `s` にタスクを加える。`s` が閉じていれば `Panic` `the scope is closed` を起こす。閉じかけでまだタスクの残るスコープには加えられる。旗の立ったスコープに加えたタスクは、作るときに取り消す」
    - 「`Scope` は `Unr` の即値で、子のタスクに渡せ、`scope` の外へも出せる。離れたスコープの値と前の run の値は、閉じたスコープとして扱う。REPL の束縛にも置ける」
    - 「タスクは自分のスコープより長く生きない。スコープはタスクがすべて終わるまで閉じず、タスクが開いたスコープはタスクの底より前に閉じる」
  - 「取り消し」に足す:
    - 旗は `cancel_scope` と `scope` の本体の失敗でも立つ
    - 「`scope` の本体を動かすタスクは、そのスコープの中にいる間だけ旗で取り消される (ビットは立てない)。スコープを出た後は、外の旗かビットが立っていなければ取り消されていない」
    - 「タスクにビットが立つと、そのタスクが開いているスコープにも旗が立つ。ビットが立った後に開くスコープも、開くときに旗を立てる」
    - 取り消しの地点の規則を「今のスコープの旗」にする
    - 「`Cancelled` が `scope` から出ると、呼んだタスクの普通の IO の失敗になる。`scope` を囲む `try_io` で捕まえれば、旗だけで取り消されていたタスクは続けて動ける」
    - 「閉じたスコープへの `cancel_scope` は何もしない。`cancel_scope` はタスクの外でも呼べる」
  - vm.md:
    - 「セルと登録表」: タスクのセルに「今のスコープ」、スコープのセルに「閉じかけか」。「閉じかけで一覧が空になると閉じる。根は開くときから閉じかけである」。放す所に `leave_scope` を足す。generation の項「新しい添字の generation は、`Machine` が前の run から受け取る数から始める。`Scope` の即値は run をまたいで残りうるので、前の run の値が後の run のセルを指さないようにするためである」
    - :512 の「添字は run ごとに使い回すので、generation では見分けられない」→「`Task a` のハンドルは、前の run のものを受け箱で見分ける。`Scope` の即値は generation で見分ける (「セルと登録表」)」
    - 「命令」に7行の項。:239 → 21。「取り消しの届け方」に角括弧の部分を足す
  - core-ir.md の「表の印の列」に `Std.Async.start_in_raw` の行 (`Unr`、`poly` / `Lin`)
  - declarations.md:121 →「21個」
  - testing.md の `async/` の行に「名前付きのスコープ、`shield`」
- テスト:
  - 新しい UI テスト `run/async/`: `scope_cancel`、`scope_child_failure_wins`、`closed_scope`、`start_in_flagged`、`cancel_reaches_open_scopes`、`shield`。`run-fail/async/cancelled_run`。REPL `crates/eml_cli/tests/repl/async/stale_scope.repl` (付録 `ui-tests.md`)
  - 期待値の変更 (種類 2): タイの2つの一覧 (`start_in_raw`、doc の数 10 → 11)。`externs.rs` の `PROBES` と `probe()` に `probe_scope : Async.Scope -> Async.Scope` と `ExternType::Scope => "probe_scope"` (網羅の match なので要る)。理由は型の行と行を足したこと
  - `only_the_listed_rows_borrow_or_return_a_pair` は変わらない (借りる `Scope` の引数はない)
- 実装者に任せること: `start` の `Option` と `start_task` を `vm.rs` でどう組むか。放したスコープへの `close_scope`、`leave_scope`、`fail_scope` の内部の誤りの文言 (std から届かない)
- 条件: いつも
- 完了の条件:
  - 共通の確認 → すべて通る
  - `cargo nextest run -p eml_cli --test integration -E 'test(ui::) | test(repl::)'` → すべて通る
  - `timeout 20 cargo run -p eml_cli -- run --debug-heap --sched seeded:1 tests/ui/run/async/cancel_reaches_open_scopes.em` → 標準出力が `worker: Cancelled: task was cancelled` と `after run`、終了コード 0
- 落とし穴:
  - `wait_closed` は `await_cell` を `shield_enter` と `shield_leave` で囲む。囲まないと、旗で取り消された本体がそこで `Cancelled` を受けて待たずに進み、`leave_scope` が閉じていないスコープに出会う
  - `leave_scope` は `me.current == s` を確かめ、`release_scope` の前に `current` を親に戻す
  - `close_scope` は `members` が空ならすぐ `closed` にする。続く `await_cell` は `park` ですぐ再開する
  - `start_in_raw` は閉包を預かるので、閉じたスコープの道では `Panic` の前に `decref` する。しないと検査モードが漏れを報告する
  - ビットが立った後に開くスコープの旗 (D-5) を忘れると、`cancel_reaches_open_scopes.em` が逐次のスケジュールでだけ止まる

### T-5: `check_cancelled`、`cancel_requested`、操作の予算
- 目的: 柔らかい確かめと操作の予算を入れる
- 対象: `cells.rs`、`vm.rs`、`eml_extern` (2行)、`std/Async.em` (= 付録 `Async.em`)、`tasks.rs`、新しい UI テスト、docs。参照の実装は `T-5.patch`
- 作業: `TaskCell.budget` (`BUDGET = 128`)、`started` と `resumed` で戻す。`spend_budget`。`cancel_point(me: Option<CellRef>)` (`None` は偽、真なら `cancels += 1`)。D-12 の T-5 の2行。`AsyncCancelPoint` は `current_cell` を呼ばず `task.cell` を渡す。std の `check_cancelled`、`cancel_requested`、2つの extern
- spec:
  - stdlib.md の表に `check_cancelled` と `cancel_requested` の行。行の表に2行、「次の23行」
  - 「取り消し」に足す: `check_cancelled` は取り消しの地点である。「`cancel_requested ()` は、取り消しの地点が投げるときだけ真を返す。`shield` の中とタスクの外では偽である。`await` から出た `Cancelled` を捕まえたタスクは、`cancel_requested ()` で自分が取り消されたかを確かめられる」
  - stdlib.md に新しい `### 操作の予算`: 「待たずに済む速い道は、タスクごとの予算を1つずつ使う。予算は 128 で、タスクが始まるときと再開するときに戻す。予算を使い切った速い道は、`yield_now` でほかのタスクに worker を渡す。今の速い道は `check_cancelled` だけで、`cancel_requested` は予算を使わない。予算は横取りの予算と別で、横取りでは戻さないので、`RunStats.parks` はスケジュールによらない」
  - vm.md: 「命令」に2行の項。タスクのセルに「操作の予算」。:239 → 23。「`RunStats`」の `cancels` に「真を返した `cancel_point`」を足す
  - declarations.md:121 →「23個」
  - testing.md の「seed で流す」: 「`cancels` は、`Cancelled` の再開のほかに、取り消しの地点が投げると答えた `cancel_point` も数える。そのため `cancels` が 0 の run では、どのタスクも取り消しを受けず、本体を最後まで動かした」。`tasks.rs` の段落に「予算が 128 回ごとに1回だけ切り替えること」
- テスト: 新しい UI テスト `run/async/check_cancelled`、`cancel_requested`、`await_cancelled`。新しい `tasks.rs::the_budget_yields_once_every_128_checks` (127 回 → `parks` 0、128 → 1、256 → 2、どれも `cancels` 0)
- 条件: いつも
- 完了の条件:
  - 共通の確認 → すべて通る (`cargo nextest run` は 3771 件通る)
  - `cargo nextest run -p eml_vm --test integration -E 'test(tasks::)'` → 14 件通る
  - `cargo nextest run -p eml_cli --test integration -E 'test(ui::)'` → すべて通る
- 落とし穴: `cancel_requested` は `Async` を持たないので、`main` でも動かなければならない (`task.cell == None` → 偽)。予算は `preempt` で戻さない

### T-6: 段を閉じる
- 目的: roadmap の「進め方」の閉じる手順を1つの commit (`Close async-cancel`) で行う
- 対象: `docs/spec/`、`docs/implementation/status.md`、`docs/implementation/benchmarks.md`、`docs/future/roadmap.md`、`docs/future/async.md`、`docs/future/multicore.md`、`docs/future/native.md`、`docs/future/stdlib.md`、`docs/README.md`、`CLAUDE.md`、`docs/plans/`
- 作業 (この順で):
  1. spec を仕上げる。stdlib.md の「`Std.Async`」(スコープ、取り消し、操作の予算)、vm.md の「`Async` のタスク」と「`RunStats`」、declarations.md:121 を読み直す
  2. 計測する。この段は `crates/eml_vm`、`crates/eml_lir`、`std/` を変えた。macOS ではまず `bench/scripts.sh` を流し、実時間が範囲を外れた行があれば別の commit (`Resize bench rows`) で大きさを変える。次にコードの最後の commit の作業ツリーで `bench/run.sh`、`bench/rt.sh`、`bench/scripts.sh` を流し、benchmarks.md の「今の数値」の3つの表を入れ替える。仕事の数は変わらない見込みである (`bench.rs` が固定している)。macOS がなければ T-7 に回す
  3. status.md: 36 行目の `Std.Async` の項に `scope`、`start_in` と `spawn_in`、`cancel` と `cancel_scope`、`shield`、`check_cancelled` と `cancel_requested` を足す。139〜144 行目の「`Async`」の制限を次にする: 「時間 (`sleep_ms`、`timeout`)、channel、I/O の driver はない。タスクの中の読み書きはその場で動き、その間はほかのタスクも動かない」「取り消しは協調的で、取り消しの地点 (`await`、`yield_now`、`check_cancelled`) でだけ届く。読み書きの extern と `par` の子は取り消しの地点でない」「タスクの中で `Async.run` を入れ子に呼ぶと、呼んだタスクはスタックを持ったまま待ち、その間は取り消しの地点でない。中のタスクには取り消しが届く」。最初の失敗の項と `IOErrorKind` の項は残す。「速い道で予算を使うのは `check_cancelled` だけである」を足す
  4. architecture.md: crate とパスの構成は変わらないので、T-2 の 312 行目のほかは直さない
  5. roadmap.md:
     - `async-cancel` の項を消す。`async-time` と `async-chan` を「前提: なし」、`ffi-blocking` を「前提: `ffi-c`、`io-driver`」にする (`Close async-tasks` の commit `4322b21` と同じ扱い)
     - 「その後の項目」の「非同期」に足す: 「`shield` の中で取り消しの要求を読む確かめ」、その下に「後に回した理由: `cancel_requested` は取り消しの地点と同じく `shield` の中では偽を返す。`shield` の中の後始末が要求を知る使い道が、まだない。」と「見直す条件: `shield` の中の後始末が、取り消されたかで動きを変える必要が出たとき。」
     - async.md と multicore.md を、まだ将来のもの (時間、channel、I/O とプールの待ち、複数の worker、`drain_within`、ネイティブ、救済、強い取り消し、却下した案、確定した設計判断の表) の今の時制の設計として書き直す。async.md から spec が持つものを消す: 「スコープとタスク」の API のブロック、`spawn_in`/`start_in`/`cancel`/`drop` の項、終わりの手順、`reraise`、`wait_closed`、結果の優先。「失敗と `exit`」の最初の項。「取り消し」の2つの所、届け方、std の包み、続くこと、`shield`、API のブロック。「待ちを外す」のセルの待ちの項。「スコープの取り消し」の、worker をまたぐ費用と I/O の操作の文を除く部分。「テストと決定性」の実装した項 (続く取り消し、操作の予算、`FileIn`) と `cancels`。「移した」「実装した」や段の名前は書かない
     - async.md の見出し「スコープとタスク」「取り消し」「待ちを外す」「強い取り消し」は将来の節として残す。そのうえで、spec へ移った中身を引く所を直す:

       | 引く行 | 今引く async.md の見出し | 閉じるときの扱い |
       |---|---|---|
       | docs/future/native.md:77 | 「取り消し」 | `[VM](../spec/vm.md) の「取り消しの届け方」` に替える |
       | docs/future/multicore.md:124 | 「スコープとタスク」(セルの欄) | 欄は `[VM](../spec/vm.md) の「セルと登録表」` に替える。持ち主の worker の話は async.md のまま |
       | docs/future/stdlib.md:5 | 「スコープとタスク」 | API の話なら `[標準ライブラリ](../spec/stdlib.md) の「スコープ」` に替える |
       | docs/future/multicore.md:125 | 「待ちを外す」 | そのまま (時間、channel、I/O、プールの待ちは async.md に残る) |
       | docs/future/native.md:121 | 「スコープとタスク」(ネイティブの `run` の入れ子) | そのまま |
       | docs/future/multicore.md:407、460 | 「失敗と `exit`」 | そのまま |
       | docs/future/multicore.md:473 | 「強い取り消し」 | そのまま |
       | docs/future/roadmap.md:31 | 「取り消し」 | 項と一緒に消える |
       | docs/spec/stdlib.md:734 | 「スコープとタスク」 | T-3 で直してある |

     - async.md の「`Async` のエフェクト」の「`Ctx` の予算」と multicore.md:129 の `ctx.budget` を「タスクのセルの予算 ([標準ライブラリ](../spec/stdlib.md) の「操作の予算」)」にする
     - async.md の見出しを消すときは、先に `rg -n 'async.md\)? の「<見出し>」' docs crates std tests` → 出力なし、を確かめる
  6. overview.md は変えない (`Async` の項がない)。README.md の「文書の地図」の `future/async.md` の行は、「取り消し」の節が残る間は「取り消し」を残す
  7. CLAUDE.md の Topics の表の `Std.Async` tasks の行に「docs/spec/stdlib.md の「取り消し」, docs/spec/vm.md の「取り消しの届け方」」を足す
  8. `docs/plans/2026-10-27-async-cancel.md` と `docs/plans/2026-10-27-async-cancel/` を消す。消す前に、5 章の決めたことがすべて `docs/` かコードのコメントにあることを確かめる
  9. commit のメッセージに書く: 文書で何をどこへ移したか、テストの変更の種類と理由 (T-2 の 44 のスナップショット、T-3 と T-4 のタイの一覧と `PROBES`、T-3 の `caught_await_still_fails_run.em` と機械的な追随)、計測の目立つ差か T-7 に回したこと、後に回した項目とその決めた日 (2026-10-27)
- 完了の条件:
  - `cargo nextest run -p eml_cli --test integration -E 'test(citations::)'` → 通る
  - `rg -n 'async-cancel' docs --glob '!docs/reports/**' --glob '!docs/plans/**'` → 出力なし
  - `git diff <T-0 の commit>..HEAD -- docs/spec docs/implementation | rg '^\+' | rg '今は|後で|将来|予定|段で'` → 出力なし (文案では「後で」でなく「後に」を使う)
  - `ls docs/plans` → `2026-10-27-async-cancel.md` も同じ名前のディレクトリもない
  - 「9. 確かめ方」の全体の確認 → すべて通る

### T-7: 計測 (macOS がないときだけ)
- 目的: T-6 で計測できなかったときに、表の入れ替えだけを直後の commit (`Measure async-cancel`) にする
- 条件: T-6 を macOS 以外で行ったとき。そうでなければ行わない
- 作業: 利用者の Mac で、T-6 の手順 2 を行う
- 完了の条件: `git show --stat HEAD` が `docs/implementation/benchmarks.md` だけを変えている

## 8. 進める順

```
T-0 ─┬─ T-1 ─┐
     └─ T-2 ─┴─ T-3 ── T-4 ── T-5 ── T-6 ──(T-7)
```

T-1 と T-2 は互いに依らない。付録のパッチは T-1 → T-2 → T-3 → T-4 → T-5 の順で確かめてあるので、その順に commit するのが安全である。

## 9. 確かめ方

- 基準 (最初の変更の前): `cargo nextest run` → 3768 件通り、20 件を飛ばす。`cargo clippy --all-targets -- -D warnings` → 終了コード 0
- 全体の確認 (T-6 の後):
  - 共通の確認 → すべて通る
  - `nix develop .#cross -c cargo clippy --target x86_64-pc-windows-msvc --workspace --all-targets -- -D warnings` → 終了コード 0
  - `nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" cargo miri test -p eml_vm --test integration miri::` → 通る。`-Zmiri-tree-borrows` を足してもう1回 → 通る
  - `cargo nextest run -j 1 -p eml_cli -p eml_vm -E 'test(ui::run) | test(repl::transcripts) | test(tasks::)'` → `ui::run` が 5 秒未満、ほかのテストがそれぞれ 1 秒未満 (計画の時点では 2.7 秒、1.7 秒、0.06 秒未満)
  - `git diff --stat main -- crates/eml_rt` → 出力なし
- 利用者が手で確かめること: T-6 の計測 (macOS)。計測を T-7 に回したときは、その commit の表

## 10. 危険と未決の項目

| ID | 項目 | 影響 | 決め方と時期 | 決まらないときの既定 |
|---|---|---|---|---|
| U-1 | `detach` と `fail_scope` を `pub` にするか | std の API | 利用者が使い道を示したとき | `pub` にしない |
| U-2 | `shield` を無視して取り消しの要求を読む確かめ | std の API | 使い道が出たとき | 入れない。T-6 で「その後の項目」に置く |
| U-3 | `Panic` の文言 `the scope is closed` | stdlib.md の「実行時エラー」、UI テスト | 実装の中 | この文言 |
| U-4 | generation を 32 ビットより広げるか | `Cell` と `Scope` の即値の形 | 一周が実際の問題になったとき | 広げない (D-6) |
| R-1 | `async/` の UI テストの出力がスケジュールに依らないことは、ハーネスでなくレビューで確かめる | seed で落ちるテスト | 各 task のレビュー | 付録 `ui-tests.md` の約束に従い、seed 1〜100 で確かめる |

## 付録: レビューで変わった主な点

- `check_cancelled` と `cancel_requested` が `shield` について食い違っていた → 1つの行 `cancel_point` にまとめ、`shield` の中では偽にした (D-4)
- `Cancelled` が `scope` から取り消されていない呼び手へ漏れる、という指摘 → 設計のとおりとした。`scope` は値を返す型で吸収できず、async.md も本体のタスクが `Cancelled` を親へ投げ直すとしている。規則を spec に書き、`try_io` で捕まえるテストを足した (D-1、D-5)
- 種類 `Cancelled` を種類で判定すると、利用者の `fail_io Prelude.Cancelled` も失敗にならない、という指摘 → 設計のとおりとし、テスト `cancelled_kind` で固定した (D-1)
- 取り消しで直接待ちを外す形は、複数の worker で作り直しになる → 印と届けを分けた (D-3)
- `Scope` を `Foreign` にすると REPL の回復が壊れる (試作で再現) → `Int` の即値と generation の下限にした (D-6)
- ビットが立った後に開くスコープと、入れ子の `run` に取り消しが届かず止まる (試作で再現) → `flag = parent.flag || opener.cancel` と `TaskCell.run` を入れた (D-5)
- `caught_await_still_fails_run.em` の出力が新しい規則でスケジュールに依る (seed 100 のうち 52 で変わる) → 競合を spec に書き、テストを `shield` で直した。`spin` を増やす直し方は採らない (D-13)
- seed の比較の規則にレビューだけの約束が残っていた → `cancel_point` の真も `cancels` に数え、ハーネスの規則だけで足りるようにした (D-9)
