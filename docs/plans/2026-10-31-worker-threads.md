# `worker-threads` の設計と計画

- 作成: 2026-10-31
- 基準: `ccr-67068ee9-bcd10x` @ `0f9cd9b` (下の「背景」と付録の行番号は、すべてこのコミットの内容を指す)
- 状態: 計画は確定。実装中に決める項目が残る (10 章)

この文書は本文と付録4つからなる。本文は、決めたこと、タスクの順番、完了の条件を書く。細部の根拠と手順は付録に英語で書いてある。付録は設計の途中の成果物をそのまま収めたものである。付録に出てくる作業用のディレクトリ (`/tmp/...scratchpad/...`) は、実装のときにはもうない。

| 付録 | 中身 | 本文での呼び名 |
|---|---|---|
| [s1-thread-model.md](2026-10-31-worker-threads/s1-thread-model.md) | スレッドの模型、段の範囲、止める手順、監視のスレッド、シグナル。基準 G-S1-1〜12 | S1 |
| [s2-heap-model.md](2026-10-31-worker-threads/s2-heap-model.md) | `eml_rt` のページ、別のスレッドからの解放、ほかのヒープの物体の受け付け、run の表、N 個のヒープの回復、検証。基準 G-S2-1〜14 | S2 |
| [s3-lin-part.md](2026-10-31-worker-threads/s3-lin-part.md) | `share` の制限の扱い (`lin` の部分の不変条件と検査) | S3 |
| [s4-integration.md](2026-10-31-worker-threads/s4-integration.md) | S1〜S3 をコードの水準でつないだ設計と、単位 W0〜W17 | S4 |

付録どうしが食い違うときは、本文 > S4 > S1 と S2 > S3 の順に優先する。本文 5.3 節に、食い違いをどう決めたかを並べた。

## 1. 目的と完了の条件

- 目的: VM を複数の OS スレッドで動かす土台を入れる。worker ごとのヒープ、別のスレッドからの解放、スレッドをまたぐ知らせ、止める手順、シグナルの塞ぎを作り、`par` の右の子を空いている worker に置いて、その土台を eml のプログラムで端から端まで通す。後の段 (`async-threads`、`par-steal`、`task-limits`、`main-pinning`) は、この土台の上に作る
- 完了の条件: 9 章の全体の確認がすべて通ること。特に次の3つである
  - `par/` の UI テストが N = 1、2、8 でバイト単位で同じ出力を返し、検査モードで所有者の検査と漏れの検査を通る
  - `--workers 2` で `par` の右の子が別の worker で動き、親がその子の確保した物体を別のスレッドから解放する (検査モードの `Stats.remote_frees` で確かめる)
  - Verus、Miri (2つの借用の規則)、ThreadSanitizer (新しい CI の job) が通る

## 2. 範囲

- 範囲に入るもの
  - `eml_rt`: ページの頭を3つに分けること、`RemoteSink` と別のスレッドからの解放と取り込み (`reclaim`)、ほかのヒープのページの物体を受け付けること、検査モードのページの表 `PageMap`、run の ID の表 `WorkerIds`、N 個のヒープの回復、`lin` の部分の不変条件の検査 (S3)、信頼の一覧の T14 と T15、`Control` の worker ごとの止める語と `END`
  - `eml_vm`: `eml run` で VM を worker 0 のスレッドで動かすこと、run ごとの `Run`、worker の受け箱と待ち、最初の `par` の fork で worker 1..N-1 を作ること、右の子の配置と join の知らせ、監視のスレッドの変更、止める手順、N 個のヒープの run の終わり、`RunStats` の合算
  - `eml_io`: `spawn_scoped_masked`、`Masked` の公開
  - `eml_cli`: `--workers`、`EML_WORKERS`、UI テストの N = 2 と 8 の実行
  - CI: ThreadSanitizer の job
  - 文書: `docs/spec/` の該当の節、roadmap、multicore.md、status.md、architecture.md、testing.md、`CLAUDE.md`、`crates/eml_rt/CLAUDE.md`
- 範囲に入らないもの (どの段に回すか)
  - `Async` のタスクを worker 0 のほかで動かすこと、登録表の lock、`Counter` の atomic、worker をまたぐ `Chan`、worker 1..N-1 の I/O の driver (`async-threads`)
  - deque と盗み (`par-steal`)
  - 救済、`--sched rescue-always` (`async-rescue`)
  - `eml_sched` の crate、REPL の複数の worker、配置の粒度、`Async` の UI テストの N ≥ 2 の実行 (roadmap の「その後の項目」に移す。T-17)
  - Verus の並行の証明 (`rt-concurrent-proofs` として roadmap に足す。T-17)

## 3. 背景

今のコードについて、実装で前提にする事実を挙げる。根拠の行番号は付録にある。

- VM はプロセスのメインスレッドで動き、スタックの大きさを指定していない。VM の呼び出しのスタックは `Task` が持つデータである (`crates/eml_cli/src/main.rs:116`、`docs/spec/vm.md:846`)
- 1つの `Machine` が `Ctx` とヒープを1つずつ持つ。driver は動かすスレッドの上で遅延して作り、run ごとの `Scheduler` に移す (`crates/eml_vm/src/lib.rs`)
- 監視のスレッドは run ごとに必要なときだけ `Scheduler` が作る。スタックは 64 KiB、刻みは 10 ms で、SIGINT と SIGTERM を塞いで始まる (`crates/eml_vm/src/monitor.rs`、`sched.rs:348-383`)。監視のスレッドのない run では、VM が1回の待ちを 10 ms で切る (`WAIT_CAP_MS`、`sched.rs:187, 357-366`)
- `Control` は Relaxed の atomic の集まりで、止める語の `worker` は1つしかない (`crates/eml_rt/src/control.rs`)
- 確保器のページは 64 KiB で、頭 (`PageHeader`、32 バイト) に持ち主も遠隔の空きリストもない。大きい物体は自分のページを持ち、解放するとすぐ OS に返す (`crates/eml_rt/src/alloc.rs`)
- 検査モードの論理的な worker ID と生きている物体の数は、ヒープごとの表にある (`docs/spec/object-model.md:698-709`)
- 回復は1つのヒープを根から写す。rc < 0 の物体は局所の物体として写す (`docs/spec/object-model.md:753-778`)
- 信頼の一覧は T0〜T13 で、T0 は「1つのヒープには1つのスレッドしか触れない」である (`crates/eml_rt/src/trust.rs`、`docs/spec/object-model.md` の「`unsafe` の核」)
- `share` の制限 (`docs/spec/object-model.md:259`、`docs/implementation/status.md:142`) は、手で組んだ単体テスト `an_rc2_object_under_a_resent_lin_object_keeps_the_senders_owner` だけが作る形である。UI の 623 本、seed の run 435 本、`eml_vm` と `eml_rt` のテストのどれも、この形に着かなかった (S3 の 7 章で、所有者だけのたどりに打ち切りを入れて確かめた)
- CLI のフラグは `--sched`、`--debug-heap`、`--stats` だけで、`--workers` と `EML_WORKERS` はない
- 基準のコミットでの cachegrind の `I refs` (release、`--sched sequential`): empty 19,796,867、loop 196,045,081、fib 114,540,917、par_tree 41,767,918。何度測っても同じ値だった (S4 の 7 章)
- ThreadSanitizer は、基準のコミットで `eml_rt trusted::` と `eml_vm` の `signal:: preempt::` が警告なしに通る (S1 と S2 の 7 章)

## 4. 前提の環境

| 要るもの | 用途 | 確かめ方 | ないとき |
|---|---|---|---|
| nix の devShell | すべて | `nix develop -c cargo --version` → `cargo 1.99.1` | 止めて利用者に聞く |
| Verus | eml_rt のタスク | `nix develop -c cargo verus --version` → 版が出る | 止めて利用者に聞く |
| `.#miri` の shell | Miri と TSan | `nix develop .#miri -c cargo miri --version` → 版が出る | 止めて利用者に聞く |
| `.#cross` の shell | Windows の clippy | `nix develop .#cross -c cargo clippy --version` → 版が出る | T-9 以降の X の確認を飛ばし、報告に書く |
| valgrind | 命令数の確認 (IR) | `nix shell nixpkgs#valgrind -c valgrind --version` → 版が出る | 止めて利用者に聞く |
| macOS の機械 | T-17 の `bench/*.sh` | — | roadmap の「進め方」の手順 2 に従い、表の入れ替えを別のコミット (`Measure worker-threads`) に回す |

## 5. 決めたこと

### 5.1 長期の指針 (変えるには検討し直しが要る)

実装がこれらとぶつかったら、止めて報告する。各指針の基準 (MUST / MUST NOT と確かめ方) は付録の 8 章にそのまま書いてあり、コミットごとのレビューはそれに照らして行う。

#### D-1: この段の土台を使うのは `par` の右の子の配置である

- 決定: この段で worker 1..N-1 の上で動く eml のコードは、fork で渡した `par` の右の子とその子孫だけである。fork は印を付けた後、空いている worker があれば右の子をその受け箱に置く。親は join で worker を手放して待ち、子の終わりは `Joined` の知らせで親の worker に届く。deque と盗みは `par-steal` に残す。`Async` のタスク、登録表、`Chan`、`Counter`、根、Host を読む extern は worker 0 にとどまる
- 理由: 土台を使うコードがないと、別のスレッドからの解放やほかのヒープの物体の受け付けを eml のプログラムで通せない。右の子はもう印を付けた別の `Task` で、`Row<Never>` なので登録表にも IO にも触れない
- 却下した案: 土台だけ入れて worker を遊ばせる案 (端から端まで確かめられない)。`par-steal` や `async-threads` と1つの段にする案 (大きすぎてレビューできない)
- 影響: `par-steal` で替わるのは fork の判断 (約 20 行) と `Child` の知らせだけで、受け箱、待ち、join の知らせ、止める手順は残る (S1 の 1 章)
- 見直す条件: `par-steal` の設計に入るとき
- 基準: G-S1-1、G-S1-11
- リポジトリに残す所: roadmap の段の文 (T-1 と T-17)、vm.md の「タスクとスケジュール」と「`par` の fork と join」(T-11、T-12)

#### D-2: スレッドの模型

- 決定: `eml run` では、プロセスのメインスレッド (host) が worker 0 のスレッド (`eml-worker-0`、8 MiB) を作り、join する間だけ SIGINT と SIGTERM を塞ぐ。worker 1..N-1 は、根がなく、読み込んだコードに `RootSet` がない run の最初の `par` の fork で、worker 0 が `Machine::run` の中から `thread::scope` で一度に作る。各 worker は自分のスレッドで `Ctx` を作り、run の終わりに worker 0 へ返す。`par` のない run はスレッドを作らない。worker 1..N-1 は I/O の driver を持たない (`driver_mut()` は内部の誤り)。REPL は N = 1 のまま、メインスレッドで動く。N は `--workers` > `EML_WORKERS` > `available_parallelism()` の順で決め、ライブラリと UI テストの既定は 1 である
- 理由: run の後もほかの worker のヒープのページに物体が残るので、ヒープは worker 0 が join してから落とす必要がある。REPL の根は入力をまたいで残り、`transfer_back` は物体を写さないので、根のある run が worker を作ると、落としたヒープを根が指しうる
- 却下した案: host がすべての worker を作る案 (`Machine` を `Send` にする必要がある)。REPL で worker を入力の間も生かす案 (静止の手順と N 個のヒープを持ち続ける `Machine` が要る)
- 見直す条件: REPL の中の `par` を並列に動かしたいとき。`eml_native` が同じ worker を使うとき
- 基準: G-S1-2、G-S1-5、G-S1-9、G-S1-11
- リポジトリに残す所: vm.md の「上限と割り込み」「タスクとスケジュール」(T-9、T-11)、roadmap の「その後の項目」(T-17)

#### D-3: 止める語は書く側がすべてに書き、監視のスレッドは起こすだけにする

- 決定: `Control` は worker ごとの止める語を `limits.workers` 個持つ。止めるビットを書く側 (割り込み、`TERMINATE`、`MEMORY`、`end_run`) は、すべての語に直接書く。新しいビット `END` は run の終わりを worker に知らせ、`stop` には入れない。`Ctx` は自分の語を持ち、`poll` は自分の語の1回の読みのままにする。監視のスレッドは語を写さず、worker ごとに `PREEMPT` を立て、止めるビットや知らせのときにすべての worker を起こす。ドアベル (自己パイプ) を持つ監視のスレッドは、動いている worker がない間は刻まずに眠る。ドアベルのない監視のスレッドは 10 ms ごとに刻む。監視のスレッドを始められなかった run は N = 1 で動き、VM が待ちを 10 ms で切る
- 理由: 今の規則「書いた時点で止まる時点が決まる」(`docs/spec/object-model.md:819`) を保つ。監視のスレッドは始まらないことがあり、それに止まる時点を頼らない
- 却下した案: multicore.md の「監視のスレッドが写す」案 (止まるのが最大 10 ms 遅れ、始まらない監視のスレッドに頼る)
- 見直す条件: worker の数が大きく、すべての語に書く費用が測って目立ったとき
- 基準: G-S1-3、G-S1-4、G-S1-6
- 確かめ方の補足 (利用者の決定): G-S1-4 は `bench/run.sh` (macOS だけ) の代わりに Linux の cachegrind で確かめる (6 章の IR)
- リポジトリに残す所: object-model.md の「実行の制御」(T-8)、vm.md の「時計と driver」(T-10)

#### D-4: 止める手順と run の終わりの順番

- 決定: 終わりを決めるのは worker 0 のループか、worker k の致命的な誤り (`Run.fatal` に最初の書き手として書く) である。順番は、`END` をすべての語に立てて起こす → 各 worker はフレームを巻き戻さずにタスクを捨てて `Ctx` を返す → worker 0 が join する → すべてのヒープを `reclaim` する (表示する結果を写した後にもう一度) → Host を書き出す → 成功ならヒープの検査とヒープ 1..N-1 の破棄、誤りか捨てた run なら N 個のヒープの回復 → host が worker 0 を join する → 終了コード。worker 1..N-1 は `Control::end` に書かない。`Run.fatal` があれば、worker 0 の結果より優先する
- 理由: Host を検査の前に書き出す今の順番は、`> out 2>&1` で出力が誤りより前に出るようにするためである (`crates/eml_vm/src/lib.rs:566-572`)。利用者の決定で、この順番を保つ
- 基準: G-S1-8、G-S2-11
- リポジトリに残す所: vm.md の「誤り、止めること、終わり」と「子の誤り」(T-12)

#### D-5: ページの頭、所有の判定、別のスレッドからの解放

- 決定: ページの頭を 48 バイトにし、3つに分ける。作った後は書かない `PageHeader` (24 バイト。`class`、`block_words`、`sink`、`bytes`)、持ち主のスレッドだけが読み書きする `PageState` (16 バイト。`index`、`limit`)、atomic で触れる `remote` の語 (8 バイト) である。`FIRST_BLOCK` は 48 になる。ヒープごとに `RemoteSink` (`count`、`large`、検査モードの `pushing`) を `Box` で持ち、所有の判定は `page_sink_of(h) == self.sink` の1回の比較にする。凍結した領域のページは `sink` が null である。ほかのヒープのページの物体を解放するときは、そのページの `remote` のリストに積む (大きい物体はヒープの `large` の列)。持ち主は `reclaim` で取り込み、`reclaim_if_due` (`large != 0 || count >= 256`) をページを取るたびと `Heap::over_budget` の中で呼ぶ。ページを歩く操作は、まず `reclaim` を呼ぶ
- 理由: ほかのスレッドが読む欄と、持ち主が書き換える欄を分けないと、データ競合になる。大きい物体の返金を遅らせると、上限の判定がスレッドの順番に左右されるので、`over_budget` の前に取り込む
- 却下した案: `owner: u32` と run の表 (解放の道で表を引く)。クラスごとの遠隔のリスト (multicore.md の決定 7 に反する)。汚れたページの列 (2つ目の並行の手順が要る。roadmap に回す)
- 基準: G-S2-1、G-S2-2、G-S2-3、G-S2-4、G-S2-6、G-S2-9
- 確かめ方の補足: 局所の解放の道は、1回の読みと1回の比較と分岐までしか増やさない。`dup` と `decref` の解放しない道は変えない
- リポジトリに残す所: object-model.md の「ページ」「別のスレッドからの解放」(新しい節)(T-2、T-4)

#### D-6: ほかのヒープの物体の受け付けと検査モードのページの表

- 決定: どのヒープも、同じ `Control` のほかのヒープのページにある物体を受け付ける。受け付けるのは rc < 0、不死 (rc 0)、または持ち主の ID が今の ID と一致する rc > 0 の物体である。検査モードでは、`Control.pages: PageMap` (3段の基数の表) で、即値でない語をたどる前にページが登録されているかを確かめ、なければ `UnknownPage` を返す。速いモードは表に触れない。検査モードの歩き (`check_reach`、`check_lin_parts`、`freezable`) は、ほかのヒープのページの子を S2 の 1 章「Foreign children in checked walks」の規則で判定する
- 基準: G-S2-5、G-S2-12、G-S2-13
- リポジトリに残す所: object-model.md の「核の操作」「報告する誤り」「検査モード」(T-5)

#### D-7: run の ID の表と検査

- 決定: 検査モードの論理的な worker ID と ID ごとの数は、`Control.workers: WorkerIds` (Relaxed の atomic、塊を `Arc` で共有) に置く。`Machine` の間生き、run ごとには消さない (回復だけが消す)。`retire_worker` は退くときに数が 0 かを確かめ、呼ぶ側の happens-before (join の Release と Acquire) に頼る。`OwnedMiscount` は `Control::check_owned(&[&Heap])` だけが返し、呼ぶ前にすべてのヒープを `reclaim` しておく。そうでなければ `NotQuiescent` を返す
- 基準: G-S2-4、G-S2-10
- リポジトリに残す所: object-model.md の「論理的な worker ID」(T-3)

#### D-8: N 個のヒープの回復と破棄の順番

- 決定: 回復は、すべてのヒープを `reclaim` → worker 0 の `Ctx` で `recover` (ほかのヒープのページはここでだけ読む) → ヒープ 1..N-1 を落とす、の順にする。`Drop` はほかのヒープのページにも `sink` にも触れず、`decref` もしない。根のある run は worker を作らない (D-2) ので、ヒープ 1..N-1 に生き残る物体はない。VM の水準では順番と表の消去だけを確かめ、ほかのヒープのページから写すことは S2 の Miri (g) で確かめる
- 基準: G-S2-11
- リポジトリに残す所: object-model.md の「回復」(T-7)

#### D-9: 検証の境目

- 決定: Verus の証明は逐次のままにする。T0 を「ヒープの局所の物体と確保器の状態に触れるのは1つのスレッドである。共有の物体に複数のスレッドが触れるのは T2 の atomic の `rc` だけである」に書き直す。ほかのヒープのページに触れる関数は新しい T14「ほかのヒープのページ」に、`WorkerIds` と `PageMap` は T15「run の表」に入れる。T14 の関数にはどれも Miri の `trusted` のテストを付ける。並行の証明は `rt-concurrent-proofs` として roadmap に回す。ThreadSanitizer の CI の job を足す
- 理由: vstd の atomic は SeqCst だけを扱い、今の証明は逐次の模型である。並行の証明は 5 万行の証明の作り直しになる
- 見直す条件: ネイティブのコードが `eml_rt` を共有する前、または TSan か Miri が T0、T14 の誤りを見つけたとき
- 基準: G-S2-7、G-S2-8
- リポジトリに残す所: object-model.md の「`unsafe` の核」、testing.md の「`eml_rt` の検証」「Miri」「CI」、`crates/eml_rt/CLAUDE.md` の予算 (T-2〜T-7、T-16)

#### D-10: `lin` の部分の不変条件 (`share` の制限の扱い)

- 決定: object-model.md:259 の「制限」は、型の検査を通るプログラムでは作れない形である。不変条件として書き、検査モードで確かめ、制限の文を消す。不変条件: 共有の `lin` でない物体から局所の物体だけを通って届く局所の物体 (`lin` の部分) は、鎖を持つ者だけが届き、`share` のときはどれも rc 1 である。検査は2か所で、どちらも既存の `HeapError::SharedReachesLocal` を返す。R1: `share` の所有者だけのたどりが、`lin` でも rc 1 でもない局所の物体を取り出したとき。R2: `check_invariants` が `lin` の部分を歩いて rc ≠ 1 を見つけたとき。速いモードは何も足さない
- 理由: 最初の `share` は、`lin` の物体の下の rc > 1 の物体にもう印を付けている (`docs/spec/object-model.md:252`)。共有の物体は書き換えず、`lin` の部分に届く道は1本しかない
- 却下した案: 送り直しのたびに速いモードでも歩いて印を付ける案 (起きない形のために費用がかかり、送り手の参照の reuse を黙って失う)
- 前提が崩れたときの手当て: 型の検査を通るプログラムで `SharedReachesLocal` が出たら、送り直しの所有者だけのたどりを両方のモードで行い、rc > 1 の物体に印を付ける。この段では入れない
- 基準: G-S2-5
- リポジトリに残す所: object-model.md の「`share`」「報告する誤り」(T-1、T-6)

### 5.2 短期から中期の設計

理由があれば実装中に変えてよい。変えたら、コミットのメッセージに書く。

- D-20 配置と待ちの手順: S4 の 1.2 節 (不変条件 I1〜I4、`idle`、`running`、`in_flight` の増減の位置) に従う。新しい worker は待っている状態で `idle` に入り、`running` に数えない。知らせを送る側は、相手の受け箱の lock の下で相手を `running` にする。`in_flight` は置く worker が増やし、`Joined` を受け取った親の worker が減らす。デッドロックは worker 0 だけが、空の受け箱の lock の下で `in_flight == 0` のときに判定する。`Clock::Virtual` の時間は `running == 0` のときだけ進める
- D-21 呼び出しの順番: S4 の 1.3 節 (worker の開始、待つ前、配置した子の終わり、join、成功の終わり、誤りの終わり、`RunStats` の合算) に従う。配置した子は、残りの参照を手放してから結果を公開する (今の `child_done` と逆の順番)
- D-22 遅延の起動: 最初の `par` の fork は、何もせずに目印を返す。`run_tasks` が命令の位置を戻し、`run_scheduled` が `thread::scope` の中で監視のスレッドと worker を作ってから、VM に戻って同じ fork をやり直す (S4 の 1.4 節)。`Scheduler` に寿命の引数を足さないためである
- D-23 join の節点は親の worker に置く: 結果は `Joined { join, outcome }` の知らせで運び、run 全体の join の表は作らない。取り消した配置の join は、知らせが届くまで `orphan` として番号を取っておく
- D-24 worker の panic: 捕まえて、その worker のヒープは漏らし、`Machine` を壊れた状態にし、worker 0 で panic し直す
- D-25 `RunStats` の合算: 数はすべての worker で足し、`peak_objects` と `peak_stack` は最大を取る。`Finished` に `workers` と `placed` を足す。`RunStats` の欄は変えない
- D-26 シグナル: `eml_vm`、`eml_io`、`eml_cli` の `src/` が作るスレッドは、worker 0 のほかはすべて塞いで作る。`spawn_masked` は `'static` を求めるので、worker 1..N-1 には `spawn_scoped_masked` を足す。`CaptureHost` の読み手のスレッドも塞ぐ。許すスレッドの作り方の一覧をテスト (`spawns::every_thread_but_worker_0_is_masked`) で固定する。一覧の数え方は S4 の 1.5 節にある
- D-27 表示する結果: 表示する run でも worker を作ってよい。結果を写した後に、すべてのヒープをもう一度 `reclaim` してから統計を読み、検査する (Fable の裁定と S4 の選択 (i))
- D-28 検査モードの `Stats.remote_frees` と `Stats.reclaimed`: 解放した側が `remote_frees` を、取り込んだ持ち主が `reclaimed` を数える。`RunStats` には入れない。VM の水準の確かめは `crates/eml_vm/src/workers.rs` の単体テスト `the_parent_frees_a_child_result_remotely` で行う。`Machine` にテストのためだけの公開の読み手を足さないためである
- D-29 worker のスタックはどれも 8 MiB (`WORKER_STACK`)。ネイティブが 1 GiB の予約を求めるときに変える
- D-30 worker を作れなかったとき: 監視のスレッドを始められなければ、その run は worker を作らない (N = 1、待ちは 10 ms で切る)。worker k のスレッドを作れなかったら、それ以降は作らず、作れた worker だけを使う。1つも作れなければ N = 1 として続ける。この扱いは付録にないので、本文で決めた (10 章の Q-5)

### 5.3 付録どうしの食い違いの決め方

最後の照合で見つかった食い違いを、次のように決めた。実装は本文に従う。

| 食い違い | 決めたこと | 理由 |
|---|---|---|
| run の ID の表 (S4 の W6) が、別のスレッドからの解放の Miri のテスト (W3、W4) より後にある。ヒープごとの数のままでは、別のスレッドで解放した `lin` の物体の数が合わない | S2 の U4 (`WorkerIds`、`check_owned`) を、ページの頭の分割の直後 (T-3) に移す。`check_owned` の `pushing` の確かめは、`pushing` を足す T-4 で加える。S4 の W6 にあった2スレッドの `trusted` のテスト `reclaim_changes_no_owner_count_and_no_work_counter` は、別のスレッドからの解放ができる T-4 に置く | 数の表が先にあれば、後のテストを例外扱いせずに済む |
| W12 の `a_child_runs_on_another_worker` が、合算 (W13) より前に「統計が N = 1 と同じ」を確かめている | T-12 では出力と `placed ≥ 1` だけを確かめ、統計の比較は T-13 で `stats_at_two_workers_equal_one_worker` と一緒に足す | 合算がないと比べられない |
| `CLAUDE.md` を直すタスクがない | T-14 で `run` の行に `--workers` を、T-17 で「The VM is single-threaded...」の不変条件と `eml_vm` の行を直す | 道しるべが実装とずれる |
| IR の確かめ (loop と fib) は `rc_decrements` が 1 なので、`dup` と `decref` の速い道の後退を見ない | T-2〜T-7 で `list` と `par_tree` の差もコミットのメッセージに残す。`dup` と `decref` の行の最終の確かめは T-17 の `bench/rt.sh` (macOS) である | Linux で測れる行はこの2つしかない。残る危険は 10 章の Q-4 |
| ThreadSanitizer の job の対象と判定 | 対象は `eml_rt trusted::` と `eml_vm --test integration -- workers:: signal:: preempt::`。判定は `TSAN_OPTIONS=halt_on_error=1` の終了コードで行う | S4 の形に合わせる |
| 試験の名前と数 (S1 と S4)、`check_lin_parts` の取り出しの手順 (S3 と S2) | 名前と数は S4 に、`check_lin_parts` の手順は S2 の 1 章の文に従う (S3 の `blocks.len()` の上限は使わない) | 後で直された方を採る |
| Miri の予算を書くタスク (S2 は W6、S4 は W7) | 最終の値は Miri (g) を足す T-7 で書く | 9本目の `trusted` のテストがそこで入る |
| 起動の条件 (S4 は「G-S1-2 と同じ」と書くが、`limits.workers ≥ 2` と監視のスレッドも求める) | 条件は4つ (N ≥ 2、根がない、`RootSet` がない、監視のスレッドが動いている) である。G-S1-2 と G-S1-6 を合わせたものとして書く | 言い方の誤りで、中身は同じ |

## 6. 実装の制約と約束

- 段の進め方は `docs/future/roadmap.md` の「進め方」に従う。`docs/` には段の名前と経緯を書かない。`spec/` の節は規則を入れるコミットで書き、コードのコメントはその節を引く。コードとコメントは `docs/plans/` を引かない (D-1〜D-30、T-x を書かない)
- 日本語の文 (コメント、文書) は `natural-japanese` のスキルに従う
- 各タスクの終わりに、次の確認を通す。
  - S (すべてのタスク): `cargo nextest run` → `0 failed`。`cargo clippy --all-targets -- -D warnings` → 終了コード 0。`cargo fmt --check` → 終了コード 0
  - G (`eml_rt` を変えるタスク。`crates/eml_rt/CLAUDE.md`): `cargo verus verify -p eml_rt` → 最後の行が `verification results:: <n> verified, 0 errors` で、`warning:` の行がない。`verusfmt --verus-only --check $(git ls-files 'crates/eml_rt/src/*.rs')` → 終了コード 0。`nix develop .#miri -c env MIRIFLAGS="-Zmiri-strict-provenance" /usr/bin/time -p cargo miri test -p eml_rt trusted::` → 通り、`user` が `crates/eml_rt/CLAUDE.md` の Stacked Borrows の予算以下。同じものに `-Zmiri-tree-borrows` を足す → 通り、`user` が Tree Borrows の予算以下
  - X (VM と CLI を変えるタスク): `nix develop .#cross -c cargo clippy --target x86_64-pc-windows-msvc --workspace --exclude eml_tls --all-targets --no-default-features --features eml_cli/run,eml_test_support/run -- -D warnings` → 終了コード 0。`cargo clippy -p eml_cli --no-default-features --features types -- -D warnings` → 終了コード 0。`cargo nextest run --profile timing -j 1` → `0 failed`
  - IR (T-1〜T-13): `cargo build --release -p eml_cli` の後、F ∈ {empty, loop, fib, par_tree, list} について `nix shell nixpkgs#valgrind -c valgrind --tool=cachegrind --cache-sim=no --cachegrind-out-file=/dev/null target/release/eml run --sched sequential bench/F.em` の `I refs` を読む (T-14 からは `--workers 1` を付ける)。親のコミットと比べて、`(loop − empty)` と `(fib − empty)` の変化がどちらも 0.1% 以下であること。`(par_tree − empty)` と `(list − empty)` の差はコミットのメッセージに書く。T-2 と T-4 では `list` の増加が 1 回の解放あたり 2 命令以下 (100,001 回の解放で +200,002 以下) であること。`list` は T-2 で初めて測る。`bench/list.em` がなければ、S4 の 7 章の `list` の計測に使ったプログラムを `bench/` に足すかを T-2 で決め、足さないなら `list` の行を省いて理由を書く
- Miri の予算: 新しい `trusted` のテストは9本である。Tree Borrows は 220 + 9 × 5 = 265 秒、Stacked Borrows は仮に 220 秒とし、静かな 4 CPU のコンテナで測り直す。`trusted` のテストを足すタスクは、測った値を `crates/eml_rt/CLAUDE.md` と testing.md の「Miri」に書く。最終の値は T-7 で書く
- テストの変更の種類 (段全体)
  - 種類 1 (成否の変更): なし
  - 種類 2 (期待値の変更): T-1 の単体テストの名前と期待 (`an_rc2_object_under_a_resent_lin_object_keeps_the_senders_owner` → `an_rc2_object_under_a_resent_lin_object_is_an_error`。共有の後に `WrongOwner` になる代わりに、`share` が `SharedReachesLocal` を返す。理由: 手で組んだ形が `lin` の部分の不変条件に反するため)。T-2〜T-7 の `TRUSTED` の行と Miri の予算。T-4 の `Stats` の2つの欄。T-8 の `Control` の呼び方。T-10 の `sched.rs` の2つの単体テストと `api.rs` の `an_injected_driver_waits_at_most_10_ms_at_a_time` (名前と期待を S4 の W10 の行のとおりに変える。理由: 監視のスレッドが注入した driver の run でも始まり、待ちを 10 ms で切らなくなるため)。T-17 の benchmarks.md の表。見出しを変えたときの `citations.rs` のスナップショット
  - 種類 3 (機械的な追随): `Limits { .. }` のリテラル (12 ファイル 29 か所、T-8)、`Finished { .. }` のリテラル (T-11)、歩きの `let mut` と `over_budget` の呼び手 (T-4)、数え直しから `check_owned` への置き換え (T-3)、2つのヒープを使うテストの `with_control(shared)` (T-4)、`Scheduler::new`、`Monitor::start`、`execute_in` の引数 (T-10、T-11、T-15)、`driver_mut()` の 31 か所 (T-10)

## 7. 実装のタスク

1つのタスクが1つのコミットである。「S4」の列は、付録の単位の表 (S4 の 5 章「Units」) の行で、そこにファイル、`spec/` の節、テストの名前、完了の条件の細部がある。本文に書いた変更は、付録の行より優先する。

| ID | タスク | S4 | 基づく決定 | 依存 | 確認 |
|---|---|---|---|---|---|
| T-0 | この計画を入れる | W0 | — | — | — |
| T-1 | `share` の R1 と `lin` の部分の不変条件 | W1 | D-10 | T-0 | S、G、IR |
| T-2 | ページの頭の分割、`RemoteSink`、所有の判定、T6 と T14 (`page_sink_of`) | W2 | D-5、D-9 | T-0 | S、G、IR |
| T-3 | `WorkerIds` と `check_owned` | W6 (前へ移す) | D-7 | T-2 | S、G、IR |
| T-4 | 別のスレッドからの解放、`reclaim`、`over_budget`、`Stats` の2つの欄 | W3 | D-5、D-28 | T-3 | S、G、IR |
| T-5 | ほかのヒープの物体の受け付け、`PageMap`、歩きの規則、T0 の書き直し、T15 | W4 | D-6、D-9 | T-4、T-1 | S、G、IR |
| T-6 | `check_lin_parts` (R2) | W5 | D-10、D-6 | T-5 | S、G、IR |
| T-7 | N 個のヒープの回復と最終の Miri の予算 | W7 | D-8 | T-6 | S、G、IR |
| T-8 | `Control` の worker ごとの止める語と `END` | W8 | D-3 | T-7 | S、G、IR |
| T-9 | `eml run` を worker 0 のスレッドで動かす、シグナルの塞ぎ、スレッドの作り方の一覧 | W9 | D-2、D-26 | T-0 | S、X、IR |
| T-10 | `Run`、受け箱、監視のスレッドの変更、待ちの上限 | W10 | D-3、D-20 | T-8、T-9 | S、X、IR |
| T-11 | worker の起動の仕組みと止める手順 (配置はまだしない) | W11 | D-2、D-4、D-22、D-24、D-30 | T-10 | S、X、IR |
| T-12 | 配置と N 個のヒープの run の終わり | W12 | D-1、D-4、D-8、D-20、D-21、D-23、D-27、D-28 | T-11 | S、X、IR |
| T-13 | `RunStats` の合算 | W13 | D-25 | T-12 | S、X、IR |
| T-14 | `--workers` と `EML_WORKERS` | W14 | D-2 | T-13 | S、X |
| T-15 | UI テストの N = 2 と 8 | W15 | D-1 | T-14 | S、X |
| T-16 | ThreadSanitizer の CI の job | W16 | D-9 | T-12、T-5 | S |
| T-17 | 段を閉じる | W17 | すべて | T-0〜T-16 | 下の完了の条件 |

### T-0: この計画を入れる

- 目的: roadmap の「進め方」のとおり、承認した計画を段の最初のコミットにする
- 対象: `docs/plans/2026-10-31-worker-threads.md` と `docs/plans/2026-10-31-worker-threads/` の4つの付録
- 実装者に任せること: なし
- 完了の条件: `git show --stat HEAD` → この5つのファイルだけ

### T-1: `share` の R1 と `lin` の部分の不変条件

- 目的: `share` の制限を不変条件と検査に置き換える (D-10)
- 作業: S3 の 5 章「Commit 1」と S4 の W1 の行に従う。`ShareError::SharedLocal` を足し、`share_walk` の最後の `else` の枝で `check_lin && on_own && rc > 1` のときに返し、`share` で `HeapError::SharedReachesLocal` にする。Verus の述語と ensures は S3 の 5 章に従う。「Ok を返す」向きの証明が1コミットで閉じなければ、S3 の代わりの手 (向きを弱め、object-model.md:688 の「モデルが合っていても起きる誤り」に `SharedReachesLocal` を足す) を使い、コミットのメッセージに書く
- spec: object-model.md の「`share`」(259 行の項目を不変条件の文に替え、331 行の表の行に足す)、「報告する誤り」。testing.md:104 と :136。status.md:142 の項目を消す。roadmap.md:31 の制限の文を消す
- 実装者に任せること: Verus の述語の形 (S3 の 5 章の条件を満たす範囲で)
- 完了の条件:
  - `cargo nextest run -p eml_rt -E 'test(/an_rc2_object_under_a_resent_lin_object_is_an_error$/) | test(/resending_after_marking_an_rc2_node_under_the_lin_part_is_ok$/)'` → 3 本が通る
  - コミットの前に `git status --porcelain` → 新しい `.snap` は `integration__ui__run@par__poly_lin_resend_shared_elements.em.snap` の1つだけ
  - `grep -rn "「\`share\`」の制限\|前の持ち主のまま残る\|前の持ち主に残る" docs crates` → 出力なし
- 落とし穴: 新しい UI テスト `par/poly_lin_resend_shared_elements.em` は、S3 の 7 章の `p1.em` (付録には本文がない) を元に書く。期待する出力は `(0, 0, "ab12", "ab12", "ab12ab12", "ab12")` である。同じ形のプログラムを書き直し、出力が違えば、その理由を確かめてから期待を決める

### T-2: ページの頭の分割と `RemoteSink`

- 目的: ほかのスレッドが読む欄と持ち主が書く欄を分ける (D-5)
- 作業: S2 の U1 と S4 の W2 の行。`PageHeader` 24 バイト、`PageState` 16 バイト、`remote` の語、`FIRST_BLOCK = 48`、`RemoteSink` (この時点では `count` と `large`)、`Pages::owns` を `page_sink_of` の比較に替える、`freeze` で `sink` を null にする、T6 の行と T14 の `page_sink_of`、`RemoteSink: Sync` の assert
- spec: object-model.md の「ページ」「生存の列挙」「凍結する」「`unsafe` の核」
- 実装者に任せること: なし
- 完了の条件:
  - `cargo nextest run -p eml_rt -E 'test(trust::)'` → 6 本が通る
  - `git diff --stat HEAD~1 -- crates/eml_vm/tests/bench.rs` → 出力なし (S2 は 48 バイトの頭でスナップショットが変わらないことを確かめてある)
  - IR の `list` の行を初めて測り、コミットのメッセージに書く

### T-3: `WorkerIds` と `check_owned`

- 目的: 別のスレッドからの解放を入れる前に、検査モードの数を run の表へ移す (D-7、5.3 節の1行目)
- 作業: S2 の U4 と S4 の W6 の行。ただし、2スレッドの `trusted` のテストは T-4 に回す。`Control::check_owned` の `NotQuiescent` は、この時点では `count != 0 || large != 0` で返し、`pushing` は T-4 で足す。VM の検査モードの `check_heap` は `check_invariants` と `check_owned(&[heap])` にする
- spec: object-model.md の「論理的な worker ID」、multicore.md の「論理的な worker ID」
- 実装者に任せること: S4 の W6 にある Miri (f) が、T-4 や T-5 の機能を要るかの判断。要るなら、それを入れるタスクに移し、Miri の予算はそのタスクで書く
- 完了の条件: S と G。`cargo nextest run -p eml_rt -E 'test(workers::)'` → 通る

### T-4: 別のスレッドからの解放と `reclaim`

- 目的: ほかのヒープのページの物体を、そのページの遠隔のリストに返す (D-5、D-28)
- 作業: S2 の U2 と S4 の W3 の行。`remote_push`、`remote_large_push`、`remote_take`、`reclaim`、`reclaim_if_due`、`Heap::over_budget`、`Drop` で `reclaim`、`pushing` と `NotQuiescent`、ページを歩く操作を `&mut self` にして先に `reclaim`、`Stats.remote_frees` と `Stats.reclaimed`、VM の包み `Ctx::{reclaim, reclaim_if_due, check_invariants, leaks}`、`Machine.check_heap: fn(&mut Ctx<M>)`。`check_owned` に `pushing != 0` を足す。T-3 から回した2スレッドのテスト `reclaim_changes_no_owner_count_and_no_work_counter` もここで足す
- spec: object-model.md の新しい節「別のスレッドからの解放」(「確保器」の下)、「実行の制御」の注、T14 の行。testing.md の「Miri」「単体テスト」。`crates/eml_rt/CLAUDE.md` の予算。multicore.md の「別のスレッドからの解放」
- 完了の条件: `cargo nextest run -p eml_rt -E 'test(the_size_of_heap_error_stays_at_twelve_bytes) | test(reclaim_changes_no_work_counter) | test(reclaim_changes_no_owner_count_and_no_work_counter)'` → 4 本が通る。IR で `list` の増加が 1 回の解放あたり 2 命令以下

### T-5: ほかのヒープの物体の受け付けと `PageMap`

- 目的: 同じ run のほかのヒープの物体を受け付け、検査モードで知らないページを報告する (D-6、D-9)
- 作業: S2 の U3 と S4 の W4 の行。`ForeignLend`、3段の `PageMap` (`Control` の中)、`UnknownPage`、`check_reach` と `freezable` のほかのヒープの子の規則、T0 の書き直し、T15、`PageMap: Send + Sync` の assert、S3 の2スレッドの Miri のテスト `a_lin_part_moves_to_another_thread_and_back`
- spec: object-model.md の「核の操作」「報告する誤り」「検査モード」、object-model.md:394 の項目を消す、multicore.md:327
- 実装者に任せること: `PageMap` の葉の大きさ。Tree Borrows の `trusted::` の `user` の増加を、検査モードの `trusted` のテストの数で割って 0.2 秒未満にする。超えたら、葉を 256 語にして4段目を足す。測った値をコミットのメッセージに書く
- 完了の条件: `cargo nextest run -p eml_rt -E 'test(trust::)'` → 6 本が通る

### T-6: `check_lin_parts` (R2)

- 作業: S4 の W5 の行。取り出しの手順は S2 の 1 章「Foreign children in checked walks」の文に従い、`seen` の集まりで終わらせる
- spec: object-model.md:668 の項目
- 完了の条件: `cargo nextest run -p eml_rt -E 'test(/check_invariants_finds_an_rc2_object_in_a_lin_part$/)'` → 1 本が通る

### T-7: N 個のヒープの回復と最終の Miri の予算

- 作業: S2 の U5 と S4 の W7 の行。`recover` は登録したどのページからも写す。破棄の順番の規則。Miri (g) に、ヒープ 0 の共有の物体の `lin` の子がヒープ 1 にあるときに `check_invariants` が `Ok` を返すことを足す
- spec: object-model.md の「回復」(「根のある run は worker を作らないので、ヒープ 1..N-1 に生き残る物体はない」)、multicore.md:328
- 完了の条件: G の Miri が Tree Borrows で 265 秒以下で通り、測った Stacked Borrows の予算を `crates/eml_rt/CLAUDE.md` と testing.md に書いてある

### T-8: `Control` の worker ごとの止める語

- 作業: S1 の U1 と S4 の W8 の行。`Limits.workers` (既定 1)、`Control.words`、番号を取る API、`END`、`end_run`、`clear_end`、`stop_reason` の `END` の枝、`Ctx.word`、`Ctx::for_worker(control, host, k)`
- 実装者に任せること: `Ctx.word` の形。まず `Arc<AtomicU32>` を Verus で試し、通らなければ `Box<[AtomicU32]>` と番号の形 (S1 が Verus で確かめた形) にする。どちらでも IR を通すこと
- spec: object-model.md の「実行の制御」
- 完了の条件: `cargo nextest run -p eml_rt -E 'test(every_word_sees_interrupt) | test(end_run_sets_end_in_every_word_and_stop_stays_clear) | test(stop_reason_puts_end_below_memory)'` → 通る。IR を通る

### T-9: worker 0 のスレッドとシグナルの塞ぎ

- 作業: S1 の U2 と S4 の W9 の行。`execute` を `eml-worker-0` で動かし、host は join の間塞ぐ。`eml_io::Masked` の公開、`spawn_scoped_masked`、`CaptureHost` の読み手を塞ぐ、スレッドの作り方の一覧のテスト
- spec: vm.md の「上限と割り込み」(worker 0 のスレッド、host の塞ぎ、スレッドの作り方の規則)
- 完了の条件: `cargo nextest run -p eml_vm --test integration -E 'test(spawns::) | test(signal::)'` → 通る。`cargo nextest run -p eml_cli --test integration -E 'test(cli::)'` → 通る
- 落とし穴: スレッドの作り方の一覧は、何に対して呼ぶかで数える。`Command::spawn` (プロセス) と `#[cfg(all(test, ...))]` のモジュールの中は数えない (S4 の 1.5 節)

### T-10: `Run`、受け箱、監視のスレッド

- 作業: S1 の U3 と S4 の W10 の行。run ごとの `Run` と `Slot`、worker 0 だけの形の待ちの手順 (D-20)、監視のスレッドの起動の規則と刻みの規則 (D-3)、`wait_deadline` は監視のスレッドが動いているか `Virtual` の時計なら上限なし、`Scheduler.driver: Option`、`const _` の assert (`Run`、`JoinNode`、`Outcome`)
- spec: vm.md の「タスクとスケジュール」「時計と driver」
- テストの変更: 種類 2 の3本 (6 章)。S4 の W10 の行に、変わらないことを確かめるテストの名前がある
- 完了の条件: `cargo nextest run -p eml_vm --lib -E 'test(monitor::) | test(sched::)'` → 通る。`grep -rn "WAIT_CAP_MS" crates/eml_vm/src` → `wait_deadline`、定数、そのコメントだけ

### T-11: worker の起動と止める手順

- 作業: S4 の W11 の行。`Workers` の状態と起動の条件 (5.3 節の最後の行の4つ)、`Exit::Spawn` での遅延の起動 (D-22)、`run_worker` のループ (この時点では待つだけ)、worker の終わり方 (止めるビットと `wake_all`、`fatal`、panic)、`end_run` と join と `clear_end`、`settle`、`Ctx` を返す、join の後にヒープ 1..N-1 を落とす、`Finished.workers`、作れなかったときの扱い (D-30)
- spec: vm.md の「タスクとスケジュール」(worker、起動の条件、根のあるセッションは1つの worker で動くこと、止める手順、panic)
- 完了の条件: `cargo nextest run -p eml_vm --test integration -E 'test(workers::)'` → 4 本が通る (`a_run_without_par_starts_no_worker`、`the_first_fork_starts_the_workers`、`code_with_roots_starts_no_worker`、`an_interrupt_ends_a_run_with_idle_workers`)。`cargo nextest run -p eml_vm --lib -E 'test(workers::)'` → 通る (`a_fatal_wins_over_any_result_of_worker_0` を含む)

### T-12: 配置と N 個のヒープの run の終わり

- 作業: S4 の W12 の行。ただし `a_child_runs_on_another_worker` は、統計の比較を含めず、出力が N = 1 と同じことと `placed ≥ 1` を確かめる (5.3 節)
- spec: vm.md の「`par` の fork と join」(配置、`Joined`、「子の誤り」は S1 の 5 章の差し替えの文)、「タスクとスケジュール」(I1〜I4)、「時計と driver」(`in_flight` のあるデッドロック、時間を進める条件)、「誤り、止めること、終わり」(N 個のヒープの run の終わり)、「追記」、「並列化への備え」の項目 4
- 完了の条件: `cargo nextest run -p eml_vm --test integration -E 'test(workers::)'` → 15 本が通る。`cargo nextest run -p eml_vm --lib -E 'test(workers::tests::the_parent_frees_a_child_result_remotely)'` → 1 本が通る。`cargo nextest run -p eml_vm --test integration -E 'test(par::) | test(miri::)'` → 通る
- 実装者に任せること: `miri::par_on_two_workers` を足すか。Tree Borrows で 10 秒以内に収まれば足し、収まらなければ T-17 で roadmap の「`miri.rs` のテストの1つあたりの Miri の費用」に書く

### T-13: `RunStats` の合算

- 作業: S4 の W13 の行。`a_child_runs_on_another_worker` に「統計が N = 1 と同じ」を足す
- spec: vm.md の「`RunStats`」(数は足し、山は最大)
- 完了の条件: `cargo nextest run -p eml_vm --test integration -E 'test(workers::) | test(bench::)'` → 通り、`workers::` は 16 本。`git diff --stat HEAD~1 -- crates/eml_vm/tests/bench.rs` → 出力なし

### T-14: `--workers` と `EML_WORKERS`

- 作業: S4 の W14 の行。`CLAUDE.md` の Commands の `run` の行に `--workers N` を足す
- spec: stdlib.md の「起動」
- 完了の条件:
  - `timeout 20 cargo run -p eml_cli -- run --workers 2 tests/ui/run/par/both.em` → 標準出力が `--workers 1` とバイト単位で同じ、終了コード 0
  - `EML_WORKERS=0 cargo run -p eml_cli -- run tests/ui/run/par/both.em; echo $?` → `2`
  - `cargo run -p eml_cli -- run --workers 0 tests/ui/run/par/both.em; echo $?` → `2`
  - `cargo run -p eml_cli -- repl --workers 2; echo $?` → `2`
  - `timeout 20 cargo run -p eml_cli -- run --debug-heap --workers 8 tests/ui/run/par/nested.em; echo $?` → `0`
  - `cargo nextest run -p eml_cli --test integration -E 'test(cli::workers_flag_is_accepted_and_rejected) | test(api::repl_limits_run_one_worker)'` → 2 本が通る

### T-15: UI テストの N = 2 と 8

- 作業: S4 の W15 の行。`par/` だけを、検査モード、逐次の方針、仮想の時計で N = 2 と 8 でも流す
- spec: testing.md の「UI テスト」「seed で流す」
- 完了の条件: `EML_UI_FILTER=/par/ cargo nextest run -p eml_cli --test integration -E 'test(=ui::run) | test(=ui::run_fail)'` → 2 本が通る。`cargo insta test -p eml_cli --check` → 保留なし。`cargo nextest run --profile timing -j 1 -p eml_cli --test integration -E 'test(=ui::run) | test(=ui::run_fail)'` → 通る。`ui::run` の実時間の前後をコミットのメッセージに書く (見込みは +0.2 秒以下)

### T-16: ThreadSanitizer の CI の job

- 作業: `.github/workflows/ci.yml` に x86_64 Linux の `tsan` の job を足す。2つの手順で、`eml_rt trusted::` と `eml_vm --test integration -- workers:: signal:: preempt::` を流す。testing.md の「CI」と新しい「ThreadSanitizer」の節、`CLAUDE.md` の Commands にコマンドの行を足す
- 完了の条件:
  - `nix shell nixpkgs#shellcheck -c nix run nixpkgs#actionlint -- .github/workflows/ci.yml` → 終了コード 0
  - `nix develop .#miri -c env RUSTFLAGS=-Zsanitizer=thread TSAN_OPTIONS=halt_on_error=1 CARGO_TARGET_DIR=target/tsan cargo test -Zbuild-std --target x86_64-unknown-linux-gnu -p eml_rt trusted::` → 終了コード 0
  - 同じ形で `-p eml_vm --test integration -- workers:: signal:: preempt::` → 終了コード 0

### T-17: 段を閉じる

- 作業: roadmap の「進め方」の手順 1〜9。付け足すことは次のとおり
  - 手順 2: `bench/scripts.sh`、`bench/run.sh`、`bench/rt.sh` の表を入れ替える (macOS)。`bench/rt.sh` で、局所の解放が 1 回あたり 2 命令以下しか増えていないことと、`dup` と `decref` の行が変わらないことを確かめる。`par_tree` を `--workers 1` と `2` で比べる見出しを足す
  - 手順 3: status.md の `par` の制限 (140 行の項目を、空いている worker に置くことと REPL が N = 1 であることに直す)
  - 手順 4: architecture.md (`workers.rs`、worker 0 のスレッド、遅延の起動、`spawn_scoped_masked`、worker 1..N-1 に driver がないこと)
  - 手順 5: roadmap から段を消し、S1 の 5 章の「Revised roadmap.md text」の `async-threads` と `par-steal` の文を入れる。「その後の項目」に、S1 の3つ (REPL の複数の worker、`eml_sched`、配置の粒度)、S2 の汚れたページの列と `rt-concurrent-proofs`、T-12 で `miri::par_on_two_workers` を見送ったならその項目を足す
  - multicore.md を S1 の 5 章の行の一覧と S2 の不変条件 5、「テスト」に合わせて直す
  - 手順 7: `CLAUDE.md` の Architecture の「The VM is single-threaded but keeps the constraints...」の不変条件と、`eml_vm` の行 (`par` を1つの worker で動かすという記述) を直す。`crates/eml_rt/CLAUDE.md` の Miri の予算が T-7 の値であることを確かめる
- 完了の条件: `grep -rn "worker-threads" docs/spec docs/implementation` → 出力なし。`ls docs/plans` → この段のファイルがない

## 8. 実行の順番

```
T-0 ─┬─ T-1 ─────────────────────┐
     ├─ T-2 ─ T-3 ─ T-4 ─ T-5 ─ T-6 ─ T-7 ─ T-8 ─┐
     └─ T-9 ───────────────────────────────────── T-10 ─ T-11 ─ T-12 ─ T-13 ─ T-14 ─ T-15 ─ T-17
                                                                   └─ T-16 (T-5 の後) ──────┘
```

- T-1 は T-5 より前に入れる (T-5 の Miri のテストが R1 を前提にする)
- T-9 は `eml_rt` の列 (T-1〜T-8) と並べて進めてよい。T-10 は T-8 と T-9 の両方を待つ
- コミットは表の順に1列に並べる。並べて作った場合も、上の依存を守って積む

## 9. 確かめ方

- 基準のコミットで: `cargo nextest run` → `0 failed`。`cargo verus verify -p eml_rt` → `0 errors`。IR の5本を測り、3 章の値と同じことを確かめる (違えば、以後の比較の基準を測った値にし、報告に書く)
- 全体の確認 (T-17 の前):
  - 6 章の S、G、X をすべて通す
  - `cargo nextest run --profile timing -j 1` → `0 failed`
  - `EML_UI_FILTER=/par/ cargo nextest run -p eml_cli --test integration -E 'test(=ui::run) | test(=ui::run_fail)'` → 通る
  - T-16 の2つの TSan のコマンド → 終了コード 0
  - `timeout 20 cargo run -p eml_cli -- run --debug-heap --workers 8 tests/ui/run/par/nested.em` → 終了コード 0
- 利用者が手で確かめること
  - 4 CPU 以上の機械で `cargo build --release -p eml_cli` の後、`time target/release/eml run --workers 1 bench/par_tree.em` と `--workers 2` を比べる。成功の条件ではなく記録である。2 のほうが遅ければ、配置の粒度の項目の「見直す条件」に当たる
  - macOS で T-17 の `bench/*.sh`

## 実行中の決定

実装の途中で計画を変えた決定を、ここに書く。本文のほかの所や付録と食い違うときは、ここが優先する。

- E-1 (T-4、利用者の決定): `reclaim_if_due` は検証したコード (確保器と `Heap::over_budget`) の中では呼ばない。確保の枠は「新しい物体を足すほかはモデルを変えない」と約束しており、その中で遠隔に解放した物体をモデルから外すと、`eml_rt` の確保の枠のほとんどを弱めることになるためである。代わりに次のようにする
  - 取り込む地点は VM だけにする。待つ前、タスクの切り替え、join の後、run の終わりの検査の前で `Ctx::reclaim_if_due` を呼び、ページを歩く操作と `Drop` は今までどおり先に `reclaim` を呼ぶ
  - ほかのヒープのページにある大きい物体を解放したスレッドは、その大きさを `Control.bytes` から atomic に引く。持ち主は次の取り込みでページを OS に返し、もう一度は引かない。これで、上限の判定がスレッドの順番に左右されない
  - これに合わせて G-S2-3 (持ち主でないスレッドは `Control.bytes` に触れない) は「`Control.bytes` の遠隔の大きい物体の返金のほかは」と読み替え、G-S2-6 (ページを取るたびに `reclaim_if_due`) は上の VM の地点に読み替える。Miri (b) と (e) はこの形で確かめる (e は「次の `reclaim_if_due` が取り込む」)

- E-2 (T-5、利用者の決定。Fable の助言 `T-5-advice-1` による): ほかのヒープの物体を証明の模型に入れない。S2 の 1 章の `Core::has` の3つ目の枝と `ForeignLend` (貸し出しの token) は入れず、`free_remote` は T-4 のまま証明の上では届かない道にしておく。1つのヒープの逐次の証明は、ほかのヒープが持つ物体の ghost の状態を今の値に保てない (持ち主の模型は移した物体について古いままで、受け手が作る権限はそれと重なる) ので、貸し出しの token は信頼を ghost の形に写すだけで、信頼そのものは減らないためである。代わりに次のようにする
  - T14 の行に次を書く。ほかのヒープの語を受けた操作は自分の物体と同じコードを走らせ、違うのは解放の道 (持ち主の `RemoteSink` に積む) だけであること。コードはブロックのメモリと公開前に書いたページの頭の欄だけを読み、持ち主の `PageState` やページの一覧を読まず、自分の数だけを書き換えること。そのため証明は「run のすべてのヒープのブロックを1つのヒープと見て、各ブロックの権限をそのときのメモリの中身とする」読み方で成り立ち、その読み方は (i) rc > 0 の物体に触れるスレッドは一度に1つ、(ii) rc < 0 の物体は `rc` だけを T2 で書く、(iii) ヒープはページに積まれる解放より長く生きる、の間成り立つこと。移した物体についてヒープの ghost は古く、その物体についての証明の主張は空になること (`remote_take` の集合は今の中身に対して約束するので、移した物体の古い辺が、ほかのヒープが持つ物体を取り込む集合に入れることはない。T-4 のレビューの B-2 はこれで閉じる)。裏付けは Miri (d)、(f)、S3 の2スレッドのテスト、TSan (T-16)、N = 2 と 8 の検査モード (T-15) であること
  - 検査モードの歩き (`check_reach`、`freezable`、T-6 の `check_lin_parts`、T-7 の `recover`) は、ほかのヒープの子の前置きの状態と rc と `lin` ビットを T14 の `foreign_header` で読む。静止点でだけ呼ぶ exec の読みで、追跡する権限を返さない。`child_ok` の仕様には `foreign(c) && foreign_alive(c)` の枝を足し、`foreign(c)` は `Control` の登録したページの ghost の集合 (`pages@`) で定める。`reach_ok` は walk.rs の外に出ない
  - `PageMap` の引きには「登録してあり、まだ外していないページに真を返す」という値の約束を1つ置く (T15 の行)。検査モードの操作の `Ok <==> readable` を保つためである。自分のページと付けた塊のページがみな `pages@` にあることを不変条件に入れる。別の `Control` で凍結した塊を付ける経路があれば `attach` で登録し、なければないことを T-5 で確かめる
  - 証明の模型にほかのヒープの物体を入れることは、`rt-concurrent-proofs` と同じ項目として T-17 で roadmap の「その後の項目」に書く (見直す条件は D-9 と同じ)
  - T-5 は1コミットにする。Verus の作業が大きすぎるときに限り、(a) `PageMap`、`UnknownPage`、T15、Miri (f) と、(b) 歩きの規則、T0 と T14、Miri (d)、S3 の2スレッドのテストの2つに分け、分けたことをここに書き足す
  - S2 の 1 章と U3、S4 の W4、本文の T-5 と D-6 のうち `ForeignLend` と `Core::has` の3つ目の枝に触れる文は、この決定が優先する
- E-3 (T-4 のレビュー): `recover` が先に `reclaim` を呼ぶことは T-7 で入れる。`live_objects` と `leaks` は `Result` を返さないので `NotQuiescent` を返せない (doc に書く)。遠隔の解放の 256 回のしきい値は Miri ではなく単体テストで確かめる (Miri の1テスト 5 秒の規則のため)

- E-4 (T-5 のレビュー): 別の `Control` で凍結した塊のページは、`attach` で登録せず、`region_known` が付けた塊の鎖をたどって、塊を凍結した `Control` の表で確かめる (`attach` のたびに鎖を歩くと 1 秒の規則を超え、塊を落とした後に登録が古くなるため)。`freezable` は、ほかのヒープのページの生きた子をどれも `NotImmortal` で断る (塊が指してよいのは自分と祖先だけであるため)。G-S2-13 の `freezable` への当てはめは、これで読み替える。T-6 の `check_lin_parts` は、ほかのヒープのブロックの走査する語を読む T14 の読み手を1つ足して下る (オーケストレータの判断。E-2 と同じく静止点での exec の読みで、追跡する権限を返さない)

- E-5 (T-5 の修正のレビュー): E-2 の `Control` の ghost の集合 `pages@` の代わりに、ページごとの登録の token (`Registered`。ページの番地と `Control` の番号を持ち、`PagePerm.reg` に置く) で `PageMap` の約束を表す。`Control` は信頼する数え手から run をまたいで一意な番号を受ける。`foreign_child` は `foreign_view(..) is Some` で定める。塊のページの token は凍結のときに捨て、塊のページは `region_known` で確かめる (E-4)

## 10. 危険と未決の項目

| ID | 項目 | 影響 | いつ誰が決めるか | 決まらないときの既定 |
|---|---|---|---|---|
| Q-1 | T-1 の Verus の「Ok を返す」向きの証明が1コミットで閉じるか | T-1 | 実装者が T-1 で | S3 の代わりの手 (T-1 に書いた) |
| Q-2 | `Ctx.word` を `Arc<AtomicU32>` にできるか | T-8 | 実装者が T-8 で | `Box<[AtomicU32]>` と番号 |
| Q-3 | `PageMap` の Miri の費用 | T-5 | 実装者が T-5 で | 葉を 256 語にして4段目を足す |
| Q-4 | `dup` と `decref` の速い道の後退を、Linux の IR では見ない | T-2〜T-7 | T-17 の `bench/rt.sh` で | 後退していれば T-17 を止めて報告する |
| Q-5 | worker k のスレッドを作れなかったときの扱いは付録になく、本文 D-30 で決めた | T-11 | 実装者が T-11 で、問題があれば報告 | D-30 のとおり |
| Q-6 | Stacked Borrows の Miri の予算 | T-4〜T-7 | 実装者が静かな 4 CPU のコンテナで測る | 測った値の 1.2 倍を書く |
| Q-7 | `--stats` に `workers` と `placed` を出すか | T-13 | — | 出さない |
| Q-8 | `REMOTE_RECLAIM` の値 | T-4 | — | 256 |
| Q-9 | worker 0 も配置の先 (`idle`) になるか | T-12 | — | なる (S1 の C3) |
| Q-10 | IR の 0.1% の幅 | T-1〜T-13 | 測った揺れが幅を超えたとき、実装者が報告する | 0.1% |

## 付録外の記録: 検討で変わったこと

蒸し返さないために、検討の中で変わった主な点だけを書く。

- 段の範囲: 「`par` の子はこの段ではまだ親の worker で動く」を外し、右の子の配置を入れた。土台だけでは端から端まで確かめられないためである (利用者がこの判断を設計に任せた)
- worker は run の最初の `par` で遅延して作り、worker 1..N-1 には driver を作らない。`par` のないプログラムにスレッドとヒープの費用をかけないためである (レビューの指摘)
- 監視のスレッドが止めるビットを写す multicore.md の形を、書く側がすべての語に書く形に替えた
- 監視のスレッドの刻みの規則は、ドアベルのある run に限った。ドアベルのない監視のスレッドは、刻まずに眠ると止めるビットに気づけない (利用者の決定)
- G-S1-4 の確かめを Linux の cachegrind にした。`bench/run.sh` は macOS だけで動き、S2 は解放の道に1回の読みと分岐を許すためである (利用者の決定)
- Host の書き出しを検査より前に置く今の順番を保った (利用者の決定)
- 根のあるセッションは worker を作らない。S1 の最初の設計では、成功した run の後に REPL の根が落としたヒープを指しうることを見落としていた (Fable の裁定、利用者の決定)
- 型の assert は、crate の中の型を名指しできる `src/workers.rs` の `const _` に置いた (利用者の決定)
- 大きい物体の遠隔の返金を遅らせると、上限の判定がスレッドの順番に左右されるので、`over_budget` の前に取り込む (レビューの指摘)
- `remote_take` は許可を作り直さず、持ち主の許可の表から取り除く。前の案は `assume_new` で許可を2つにしていた (レビューの指摘)
- 検査モードのページの表は、1 MiB の表が Miri で遅すぎたので、3段の基数の表にした (S2 の測定)
- `share` の制限に、速いモードで歩いて印を付ける形は採らなかった。その形は型の検査を通るプログラムでは作れない (S3 の調査)
