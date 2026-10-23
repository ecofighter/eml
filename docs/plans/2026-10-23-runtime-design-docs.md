# マルチコアとネイティブ化の設計を文書に書く段 (runtime-design-docs)

- 作成日: 2026-10-23
- 基準: main @ 4a0fbdf6 (この文書の「背景」と、`ファイル:行` の参照はこのコミットの状態を指す)
- 状態: 計画は確定。未解決の項目は 10 にあり、どれも既定の扱いを決めてある

## 1. 目的と完了の条件

- 目的: マルチコア対応、std の `Async`、ネイティブ化、FFI、GUI の設計を `docs/future/` に書き直し、`docs/future/roadmap.md` の「段の列」に、この段の後に進める29の段を並べる。既存の決定 (今の `docs/future/multicore.md` の12行など) は、下の 5 の決定で置き換える
- 完了の条件: 7 の T-0〜T-8 をそれぞれ1コミットで入れ、各作業の完了の条件と 9 の全体の検証がすべて期待どおりになること

## 2. 範囲

- 範囲に入るもの: `docs/future/` の4つの設計文書 (`native.md`、`ffi.md`、`async.md` を新しく作り、`multicore.md` を書き直す)、`docs/future/roadmap.md`、`docs/overview.md`、`docs/README.md`、`docs/future/stdlib.md`、`CLAUDE.md` の1行、`docs/spec/` の4つの文書の言い回し (規則は変えない)
- 範囲に入らないもの: コード、テスト、std、bench の変更。29の段の実装は、それぞれの段で設計と計画を作ってから行う

## 3. 背景

- 今の `docs/future/multicore.md` は、決定の表12行、`par` の a1 → a2-wait → a2-cancel、閉じた row の `spawn` / `run_scheduler`、スケジューラをエフェクトのライブラリとして書く方針を持つ。この段で全面的に書き直す
- `eml_rt` には共有の印方式の RC (`share`、負の RC、`lin` ビット、所有者の検査) があるが、使うのは単体テストだけである。`par`、スケジューラ、別のスレッドからの解放、`Async` はコードにない
- 継続はヒープのクロージャの連なりである。ライブラリとして書いた協調的なスケジューラ `tests/ui/run/kinds/scheduler.em` は今の言語で動く
- `docs/spec/types.md:24` は、extern のエフェクト (`IO` と `Panic`) の多重度を `Once` と決めている。`Panic` を `Never` に分け直すのは段 `never-rows` の仕事で、この段では spec を変えない
- std の I/O はすべてブロックする。`roadmap.md` には「`Async` が入ったとき」を見直す条件にした項目がある (122〜139行、206行)
- CI はなく (`.github/workflows` がない)、Windows でのビルドもしていない。`rust-toolchain` のファイルもない
- 引用の試験 `cargo nextest run -p eml_cli --test integration -E 'test(citations::)'` が、`docs/` の中のリンクと見出しの引用を確かめる。基準のコミットで4件すべてが通る
- 設計の検討では、5つの事実の調査、4つの設計 (実行系、`Async`、ネイティブ化と FFI、全体の統合)、それぞれの敵対的な査読と改訂を行った。最終の設計は付録のディレクトリ `docs/plans/2026-10-23-runtime-design-docs/` にある (英語)
  - `01-runtime.md`: 実行系とスレッドの模型、`par` の API
  - `02-runtime-addendum.md`: `01` への補遺。`Async` の取り消しの要件への答え
  - `03-async.md`: std の `Async`、スコープ、取り消し、I/O、`Chan`、`Net`
  - `04-native-ffi.md`: ネイティブ化、FFI、生の配列、GUI
  - `05-integration.md`: 3つの設計の食い違いの解決 (表 C1〜C34)、4つの用途の確かめ、この段の文書の構成と確かめのコマンド、29の段の表、後に回す項目
- 付録どうしが食い違うときは、次の順で先にあるものに従う
  1. この文書の 5 の決定と、利用者の答え (5 の D-13 と 10)
  2. `05-integration.md` の 1.1 の表と「Further decisions」
  3. `02-runtime-addendum.md`
  4. `03-async.md` (利用者から見える `Async` の意味。取り消しはこちらが持つ)
  5. `04-native-ffi.md`
  6. `01-runtime.md`
- 付録の中の C1、NU1、M0、REQ-S1-1 のような符号は、付録の中だけの呼び名である。`docs/` には書かない (D-23)

## 4. 前提の環境

| 要るもの | 使う所 | 確かめ方 | ないとき |
|---|---|---|---|
| Nix の devShell (cargo、cargo-nextest) | 全作業の確かめ | `cargo nextest --version` → 版が出る | 止めて利用者に聞く |
| yomiyasu のスキル | 日本語の文書をすべて書くとき (`CLAUDE.md` の規則) | スキルの一覧に `yomiyasu:yomiyasu` がある | 止めて利用者に聞く |
| GNU grep の `-P` を使わないこと | 確かめのコマンド | 7 のコマンドは `-E` だけを使う | なし |

## 5. 決定

### 5.1 長期の方針 (変えるには検討し直しが要る)

実装の途中でこれらと食い違う所が出たら、止めて報告する。詳細は各項の付録の節にある。T-1〜T-5 は、ここに書いた内容を日本語の設計文書に写す。

#### D-1: スケジューラの方式

- 決定:
  - `par` は fork / join で、まだ始まっていない右の子だけを、worker ごとの Chase-Lev の deque から盗ませる (work stealing)。join で待つ worker は、待つ join より深い fork の子だけを手伝う
  - `Async` のタスクは、始まった worker から動かない。待った継続はその worker のキューに戻し、共有の印を付けない。例外は「救済」で、詰まった worker の待ちのタスクと実行待ちのタスクを、監視のスレッドの合図で空いた worker が受け箱の lock の下で移す
  - 横取りは協調的にする。安全点 (`Ctx::poll`) で、worker ごとの止める語の `PREEMPT` ビットを見て切り替える。シグナルによる横取りはしない
  - ランタイムが仕組み (worker、受け箱、deque、待ちと起こし、I/O の駆動の口、プール、予算) を持ち、std (`Std.Async`、`Std.Par`) が意味を持つ
- 理由: 共有の印は外せないので、印の付いた物体は reuse を失い atomic の RC になる。継続をスレッドの間で動かさないことで、サーバの作業の集合は局所のまま保てる。利用者は `par` を `Async` より先に入れると決めた (D-13)
- 却下した案: 始まったタスクも盗む Tokio 型の work stealing (動いた作業の集合がすべて共有になる)、thread-per-core で移動なし (詰まった worker を救えない)、シグナルによる横取り
- 付録: `01-runtime.md` の Q1〜Q3、救済と受け箱の約束
- 文書に残す場所: `docs/future/multicore.md` の「スケジューラ」(T-4)

#### D-2: スレッドと worker の模型

- 決定:
  - OS のスレッド1つが worker 1つで、`Ctx` と `Heap` を1つずつ持つ。worker の数 N の既定は `available_parallelism` で、`--workers` と `EML_WORKERS` で変えられる
  - worker 0 は大きいスタック (既定 1 GiB の予約) で作るスレッドである。プロセスのメインスレッドは「host のスレッド」(ドライバ、シグナル) で、`rt_dedicate_main` で専用の worker M になる
  - 監視のスレッドが run に1本ある (10 ms の刻み、詰まりと過負荷の検出、`Control` のビットの写し、待ちの解除)
  - ブロックする呼び出しのプールは heap を持たない。プールのスレッドは RC に触れず確保もしない。仕事の記録が数えた参照を持ち、run の登録表に載る。完了は持ち主の受け箱を通す (`05-integration.md` の C11 と 5.2 の表)
  - 止める手順: `Control` の終わりの欄に最初の書き手が書き、止めるビットを立てる。各 worker は次の poll で入口へ戻り、host のスレッドが全員を join してから heap を落とす。`exit` もこの手順を使う
- 付録: `01-runtime.md` の Q2、Q5、「Answers to S3 requirements」、`02-runtime-addendum.md`
- 文書に残す場所: `multicore.md` の「スレッドと worker」「止める手順」(T-4)

#### D-3: crate の構成と検証の範囲

- 決定: 依存の順は `eml_rt` (語の単位の操作と、`Host`、`Sched`、`IoDriver` の trait) ← `eml_sched` (スレッド、受け箱、セル、監視、プール、deque、待ち、worker のループが呼ぶ `Exec` の trait) ← `eml_vm` (`Exec` を実装)。`eml_io` は `eml_rt` に依存し `IoDriver` を実装する。`eml_native` は `eml_rt`、`eml_lir`、`eml_sched`、Cranelift に依存する。`eml_cli` はすべてに依存し、`Session::run` が実行系を組み立てるので、`Session` は今と同じく唯一のパイプラインの駆動役である
- 理由: 利用者の答え (スケジューラは Verus の外に置き、Miri と TSan で確かめる)。`eml_rt` は vstd だけに依存したまま保つ
- 文書に残す場所: `multicore.md` の「層の構成」(T-4)。crate を足す段は、閉じるときに `docs/implementation/architecture.md` と `CLAUDE.md` の図を直す

#### D-4: スレッドをまたぐ RC と印付け

- 決定: 共有の印方式は保つ。印付けの表を次のとおりにする
  - fork は捕まえた値に印を付ける
  - join、channel の受け取り、タスクの開始、オフロードの戻りは、移し替えの歩き (`rt_transfer`) で受ける。rc == -1 の物体を rc := 1 に戻して下りる
  - `spawn` と channel の送りは印を付ける
  - `Lin` の物体は印を付けずに所有者を移す
  - 受け手が決まらない `Lin` の値 (MPMC の `Chan`、`Task` の結果) は、途中の持ち主の ID を付けて送る
  - Rust の側が持つ語 (channel の buffer、タスクのセルの結果、join の節点、プールの仕事の記録) は run の登録表に載せる
  - 別のスレッドからの解放は、自前の確保器にページごとの remote free の stack を足して受ける
  - heap は run の間ずっと生きる
- 付録: `01-runtime.md` の Q4 と r2 の査読への答え、`05-integration.md` の C11〜C14
- 文書に残す場所: `multicore.md` の「スレッドをまたぐ RC」(T-4)

#### D-5: `par` の API

- 決定:
  - 利用者が主に使うのは `std/Par` の集まりの関数 (`map`、`mapi`、`tabulate`、`fold`、`reduce`、`for_mut`、`try_map` と、粒度を指定する版、生の配列の版) である。2引数の `par` と逐次の `both` も公開のまま残す。`Par` エフェクト、futures の形、新しい構文は採らない
  - `par` は一つで、すべての子の完了を待つ意味を持つ。子の取り消しは後に回す。a1〜a4 の段の呼び名は文書から消す
  - 子の row は `<never e>` (row 変数の多重度の上限。表面の構文で書ける形を足す) で、`Panic` は `Never` に分け直す。`IO` は `Once` のまま
  - 分け方の木は長さと粒度だけで決まり、盗み方によらない。浮動小数の `fold` も実行ごとに同じ値になる。`try_map` は、見つかった最も左の `Err` より右の fork だけを省く
- 付録: `01-runtime.md` の Q8 と r2 の C-1、C-2、C-8
- 文書に残す場所: `multicore.md` の「並列と並行の API」(T-4)

#### D-6: std の `Async`

- 決定:
  - `Async` は普通の eml のエフェクトで、待ちの生の操作はいつも `Wake` の値を返す
  - 入口は `Async.run : (Unit -> <IO, Panic, Async> a) -> <IO, Panic> a` で、`main` では `use Async.run` と書く
  - スコープは nursery の形にし、子が失敗したら、その時点でスコープのほかのタスクを取り消す。`Task a` は `Lin` で、捨てると取り消しになる
  - 接続ごとの分離は `Async.try_all` で行う (`Failure = IOFailed | Panicked`)
  - 取り消しは `Wake.Cancelled` で届け、std の wrapper が IO の失敗 `Cancelled` として投げる。タスクは自分のフレームとスコープの後始末を通って巻き戻る。継続を捨てる強い取り消しは、`exit`、致命的なエラー、run の終わり、回復のときだけにする
  - 段 `task-limits` で、タスクごとの深さ、スタック、メモリの柔らかい上限を、そのタスクの IO の失敗 `ResourceLimit` にする。メモリの硬い上限は今と同じく run 全体を止める
  - `IOErrorKind` に `Cancelled`、`TimedOut`、`ConnectionReset`、`AddrInUse`、`ResourceLimit` を足す
- 付録: `03-async.md` の Q1〜Q4、Q8、Q9、`02-runtime-addendum.md`、`05-integration.md` の C1〜C5、C31
- 文書に残す場所: `docs/future/async.md` (T-3)

#### D-7: I/O の駆動

- 決定: std の API は OS によらず完了型にする。buffer は所有ごと渡して返してもらう (受け付け → 待ち → 取り出しの3段)。`IoDriver` の trait を `eml_rt` に置き、`eml_io` が実装する。第一の案は `compio-driver` で、使えないときは `polling` と IOCP を自前で書く。ハンドルは最初に登録した worker の駆動に縛り、別の worker からの受け付けは、その worker の受け箱を通して回す。読みの buffer は、ソケットが読めるようになってから確保する (IOCP では、まず0バイトの受信を出す)
- 付録: `03-async.md` の Q5、Q6、`05-integration.md` の「Further decisions」
- 文書に残す場所: `async.md` の「I/O の driver」(T-3)

#### D-8: タスク間のやりとり

- 決定: `std/Chan` は容量付きの MPMC の1つの型で、閉じた channel への送りは値を送り手に返す。共有の状態は、持ち主のタスクの後ろに `Chan` を置く形 (actor) で書く。利用者に見せる atomic な操作は `Atomic.Counter` だけである。ロックフリーのキューとほかの atomic な構造はランタイムの中だけに置く。複数の channel の `select` は入れない (`race`、`timeout`、`Chan.merge` で書く)
- 付録: `03-async.md` の Q7
- 文書に残す場所: `async.md` の「チャネルと共有の状態」(T-3)

#### D-9: ネイティブ化

- 決定:
  - 1つのコード生成器 (`eml_native`、開いた後の LIR → CLIF) を、JIT (`eml run --native`、REPL) と AOT (`eml build`) の両方に使う。コンパイルはプログラム全体を、worker が動いていない間に一度に行う
  - VM とネイティブのコードは混ぜない。VM は参照の実装で、検査モードを持つ唯一の実行系として残す
  - eml の中の呼び出しは Cranelift の `tail` 規約と `return_call` を使い、extern と C は OS の既定の規約で呼ぶ
  - `ctx` はすべての関数の最初の引数にし、スレッドに局所的な変数は使わない
  - 中断は戻り値で伝え、yield の確かめと一緒に旗を確かめる。評価全体を止める誤りは、自前の setjmp / longjmp (`eml_enter` / `eml_fatal`) で行う。extern の失敗は `Ctx.fault` で返す
- 理由: 試作で、`try_call` は呼び出しの位置1つあたり 0.82 ns、旗の確かめは 0.29 ns だった (`04-native-ffi.md` の 7)
- 付録: `04-native-ffi.md` の Q1〜Q3、Q7
- 文書に残す場所: `docs/future/native.md` (T-1)

#### D-10: FFI

- 決定:
  - 利用者のモジュールに `extern "C"` を書けるようにする。属性は `pure`、`blocking`、`symbol "<C の名前>"`
  - C の整数の幅は `Int as int` のように引数の型の後に書く (C の型の名前は `int`、`int32_t`、`int64_t`、`size_t`)。ただの `Int` は `int64_t` である
  - ライブラリは `link on linux "openblas"`、`link on macos framework "Accelerate"` のように OS ごとに書き、CLI の `--link` で上書きできる
  - 配列は先頭のポインタだけを渡し、長さは明示した引数で渡す。`Mut*Array` は消費して `with_arg` の形で返す
  - ハンドルは `extern "C" data … drop "<C の関数>"` で宣言する
  - コールバック、C の構造体、可変長の引数は扱わない
  - VM からの呼び出しは、`eml_native` の FFI のモジュール (段 `ffi-c` が作る) が生成した橋渡しの関数を `Op::Ffi` で呼ぶ
  - `blocking` は `<IO, Async>` を要し、プールへの受け付けと待ちと取り出しに下ろす
- 付録: `04-native-ffi.md` の Q4〜Q6、`05-integration.md` の C10、C21、C22
- 文書に残す場所: `docs/future/ffi.md` (T-2)

#### D-11: 生の要素の配列

- 決定: 新しい物体の種類 `Raw` (番号 6) を足し、`FloatArray` / `MutFloatArray` / `IntArray` / `MutIntArray` を作る。要素は 8 バイトの生の語で、走査しない。並列の書き込みは添字の範囲のプリミティブで行い、`split_at` と `concat` で分けて戻す形は採らない。配列の `scan = 255` は段 `raw-arrays` で入れる
- 付録: `04-native-ffi.md` の Q5、`05-integration.md` の C8、C9
- 文書に残す場所: `native.md` の「生の要素の配列」(T-1)

#### D-12: GUI

- 決定: GUI は FFI のライブラリでなく、Rust のプラグインの crate (`eml_gui`、まず egui) として組み込む。std の `Std.Gui` の行は普通の extern の表の行で、実装だけが `Plugin` の trait に回る。イベントループは worker M (プロセスのメインスレッド) の待ちに載せる。イベントは `Gui.next_event : Window -> <IO, Async> (Event, Window)` で引き、別のタスクの結果は `Gui.post` で送る。GUI のタスクと、その子は M から動かない
- 理由: egui、winit、Slint は C の API を持たない
- 付録: `04-native-ffi.md` の Q8、`05-integration.md` の C6、C7、C19、C20
- 文書に残す場所: `ffi.md` の「拡張」「GUI」(T-2)

#### D-13: 段の順

- 決定: 利用者の答えに従い、次の順にする。CI の段を最初に置く → `par` の核を `Async` より先に入れる → ネットワークサーバへの道 → FFI → GUI → ネイティブ化 → `par` の盗みとデータ並列の性能。29の段の名前、目的、前提は下の表で、各段の範囲、完了の条件、危険、書く spec の節は `05-integration.md` の 1.4 にある

| # | 段 | 目的 | 前提 |
|---|---|---|---|
| 1 | `platform-ci` | 今のテストを3つの OS で通す | — |
| 2 | `never-rows` | `Panic` を `Never` にし、`<never e>` を書けるようにする | — |
| 3 | `par-core` | 1つの worker の上の `par` と `both` | `never-rows` |
| 4 | `async-tasks` | 1つの worker の上のタスク | `par-core` |
| 5 | `async-cancel` | 取り消しと名前付きのスコープ | `async-tasks` |
| 6 | `async-time` | 仮想の時計の上のタイマー | `async-cancel` |
| 7 | `async-chan` | N = 1 の channel | `async-cancel` |
| 8 | `io-driver` | 本物の I/O の駆動とプール | `async-time`、`platform-ci` |
| 9 | `async-net` | macOS と Linux の TCP、シグナル、DNS | `io-driver`、`async-chan` |
| 10 | `net-windows` | IOCP の上のネットのテスト | `async-net` |
| 11 | `worker-threads` | worker のスレッド、worker ごとの heap、スレッドをまたぐ知らせ | `par-core`、`platform-ci` |
| 12 | `async-threads` | worker をまたぐ `Async` | `worker-threads`、`async-net` |
| 13 | `async-rescue` | 詰まった worker からの救済 | `async-threads` |
| 14 | `task-limits` | タスクごとの深さ、スタック、メモリの上限 | `async-threads` |
| 15 | `async-files` | 非同期のファイルとパイプ | `async-net` |
| 16 | `raw-arrays` | 生の要素の配列 | — |
| 17 | `ffi-c` | VM の上の利用者の `extern "C"` | `raw-arrays` |
| 18 | `ffi-blocking` | タスクの中の長い C の呼び出し | `ffi-c`、`io-driver`、`async-cancel` |
| 19 | `main-pinning` | worker M と `Main.run` | `async-threads` |
| 20 | `gui-plugin` | VM の上の egui のプラグイン | `main-pinning` |
| 21 | `borrowed-params` | 借用パラメータ | — |
| 22 | `jit-core` | 一階の部分の JIT | `borrowed-params`、`ffi-c` |
| 23 | `jit-externs` | すべての extern とプラグインの行をネイティブで | `jit-core` |
| 24 | `jit-effects` | 言語の全体をネイティブで | `jit-externs`、`task-limits`、`async-rescue`、`async-files`、`gui-plugin` |
| 25 | `native-aot` | 3つの OS の `eml build` | `jit-effects`、`platform-ci` |
| 26 | `native-inline` | バイトと要素の読みの展開 | `native-aot`、`raw-arrays` |
| 27 | `par-steal` | VM の `par` の子を別の worker で動かす | `worker-threads` |
| 28 | `std-par` | データ並列の集まりの関数 | `par-steal`、`raw-arrays` |
| 29 | `native-par` | 手伝いのあるネイティブの fork / join | `native-inline`、`std-par`、`jit-effects` |

- 文書に残す場所: `roadmap.md` の「段の列」(T-5)

### 5.2 短中期の設計 (理由があれば実装中に変えてよい。変えたらコミットのメッセージに書く)

#### D-20: 4つの設計文書の分け方と章立て

- 決定: 読み手と crate の組が違うので、`native.md`、`ffi.md`、`async.md` を新しく作り、`multicore.md` を書き直す。それぞれの章立て (見出しと1行の中身) は `05-integration.md` の 1.3 に従う。`roadmap.md`、`overview.md`、`README.md`、`future/stdlib.md`、`CLAUDE.md`、spec の言い回しの直し方も同じ節にある
- 却下した案: 1つの文書にまとめる (査読しにくい)、4つの文書を1コミットで入れる (同じ理由)

#### D-21: 残す見出しとリンクの順

- 決定: コードと `spec/` が引いている見出しは、字をそのまま残す。
  - `multicore.md` の見出し: 「VM の並列化の備え」(制約 1、2、5 を残す)、「確定した設計判断」、「共有の印方式」、「`par` の印付け」、「`Lin` の物体の所有者の移し替え」、「論理的な worker ID」、「スレッドに縛られたリソース」、「可変状態」、「row の Kind」、「テスト」、「未決の論点と将来の拡張」
  - `roadmap.md` の見出し: 「進め方」、「段の列」、「その後の項目」、「型システム」、「言語機能と構文」、「処理系」、「REPL」、「マルチコア対応」

  文書は native → ffi → async → multicore → roadmap → 地図 → spec の順に書く。文書がリンクしてよいのは、そのコミットの時点にある見出しだけである。見出しを作るコミットが、先に書いた文書からのリンクを足す
- 理由: 引用の試験を、どのコミットの後でも通すため

#### D-22: 文書に書く追加の決定

`05-integration.md` の 1.1 の表と「Further decisions」の行は、すべて決まっている。T-1〜T-4 はそれを書く。NU1〜NU4 は利用者が既定のとおりに答えた (D-10、D-13、10 の Q-1)。

#### D-23: 符号と段の名前を文書に書かないこと

- 決定: 付録の符号 (C1、G-a1、NU1、M0、M-A2、N3、O-1、REQ-S1、R-S1、S1〜S4) と、この計画の作業の符号 (T-0〜T-8) は `docs/` に書かない。段の名前は `roadmap.md` の「段の列」と `docs/plans/` のファイル名にだけ書く。日付は `docs/future/`、`overview.md`、`README.md`、`CLAUDE.md` に書かない
- 理由: `docs/` は今の状態を書く (`CLAUDE.md` の規則)

## 6. 実装の制約と決まり

- `docs/` の日本語はすべて、書く前に `yomiyasu:yomiyasu` のスキルを読み込み、その規則で書く。コミットのメッセージに、yomiyasu を通したことを書く (`CLAUDE.md`)
- 設計文書には決まったことだけを書き、「仮」「要確認」のような印を書かない。決まっていないことは、各文書の「未決の論点と将来の拡張」に、既定の扱いと一緒に書く (D-22)
- `docs/spec/` は規則を変えない。T-7 は言い回しと引用先だけを直す
- 後に回す項目には、`roadmap.md` の「進め方」に従い、入れ子の箇条で「後に回した理由: …。」と「見直す条件: …。」を置く。後に回す項目の一覧は `05-integration.md` の 1.5 にある
- 1つの作業は1コミットにする。コミットのメッセージの題は、7 の表の題を使う

## 7. 実装の作業

| ID | 作業 | もとの決定 | 依存 | 並行 |
|---|---|---|---|---|
| T-0 | 計画を入れる | — | — | 不可 |
| T-1 | `native.md` を作る | D-9、D-11、D-20、D-21 | T-0 | 不可 |
| T-2 | `ffi.md` を作る | D-10、D-12、D-20、D-21 | T-1 | 不可 |
| T-3 | `async.md` を作る | D-6、D-7、D-8、D-20、D-21 | T-2 | 不可 |
| T-4 | `multicore.md` を書き直す | D-1〜D-5、D-20、D-21 | T-3 | 不可 |
| T-5 | 「段の列」を並べる | D-13、D-20 | T-4 | 不可 |
| T-6 | 地図を新しい設計に向ける | D-20 | T-5 | 不可 |
| T-7 | spec の言い回しを設計に合わせる | D-20 | T-6 | 不可 |
| T-8 | 段を閉じる | — | T-7 | 不可 |

### 各作業で共通の確かめ

シェルで一度、次を定義する。

```sh
SLUGS='runtime-design-docs|platform-ci|never-rows|par-core|async-tasks|async-cancel|async-time|async-chan|io-driver|async-net|net-windows|worker-threads|async-threads|async-rescue|task-limits|async-files|raw-arrays|ffi-c|ffi-blocking|main-pinning|gui-plugin|borrowed-params|jit-core|jit-externs|jit-effects|native-aot|native-inline|par-steal|std-par|native-par'
L='\bC[0-9]{1,2}\b|\bG-[a-d][0-9]\b|\bNU[0-9]\b|\bN[0-7]\b|M-A[0-9]|\bM[0-3]b?\b|\bO-1\b|REQ-S1|R-S1|\bS[1-4]\b'
P='(の|は|で|、|: ) ?a[1-4]\b|a2-(wait|cancel)|\(a[34]\)|a1 →|a1 /'
```

作業 T-k (k = 0〜8) の後で、毎回次を確かめる。基準のコミットでの値は、2026-10-23 に orchestrator が確かめた。

- V1: `cargo nextest run -p eml_cli --test integration -E 'test(citations::)' 2>&1 | grep -c '4 tests run: 4 passed'` → `1`
- V2: `grep -rnwE "$SLUGS" docs CLAUDE.md | grep -v '^docs/future/roadmap.md:' | grep -v '^docs/plans/' | wc -l` → `0`
- V3: `grep -rnE "$L" docs/future docs/overview.md docs/README.md docs/spec CLAUDE.md | wc -l` → `1` (基準の時点でもある `docs/spec/object-model.md:952` の「C1 の制御文字」だけ)
- V4: `grep -rnE '20[0-9]{2}-[0-9]{2}-[0-9]{2}' docs/future docs/overview.md docs/README.md CLAUDE.md | wc -l` → `0`
- V5: `grep -rn 'plans/' docs/future docs/spec docs/overview.md crates std | wc -l` → `2` (`roadmap.md` の「進め方」の2か所)
- V6: `git show --name-only --format= HEAD | sort` → その作業の「変えるファイル」と同じ並び

a1〜a4 の呼び名の残りは、`grep -cE "$P" <ファイル>` で次の数になる。

| 作業の後 | multicore | roadmap | overview | future/stdlib | spec/lir | spec/object-model | spec/vm |
|---|---|---|---|---|---|---|---|
| T-0〜T-3 | 17 | 4 | 1 | 1 | 1 | 3 | 2 |
| T-4 | 0 | 4 | 1 | 1 | 1 | 3 | 2 |
| T-5 | 0 | 0 | 1 | 1 | 1 | 3 | 2 |
| T-6 | 0 | 0 | 0 | 0 | 1 | 3 | 2 |
| T-7、T-8 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |

### T-0: 計画を入れる

- 目的: 段の設計と計画を、段の最初のコミットにする (`roadmap.md` の「進め方」)
- 変えるファイル: `docs/plans/2026-10-23-runtime-design-docs.md` と、付録の `docs/plans/2026-10-23-runtime-design-docs/01-runtime.md`〜`05-integration.md` の5つ
- 作業: この文書と付録をそのまま入れる。題は `Plan runtime design docs`
- 実装者に任せること: なし
- 条件: いつも
- 完了の条件: V1〜V5 が上の値。V6 → 上の6つのファイル

### T-1: `native.md` を作る

- 目的: ネイティブ化の設計 (D-9、D-11) を `docs/future/native.md` に書き、`roadmap.md` の「ネイティブ化」の入れ子の箇条をそこへ移す
- 変えるファイル: `docs/future/native.md` (新規)、`docs/future/roadmap.md`
- 作業: 章立ては `05-integration.md` の 1.3 の「docs/future/native.md」。中身は `04-native-ffi.md` の Q1〜Q3、Q5 の配置と展開、Q7 の表の「keep」「change」「decide」に従う。`roadmap.md` から 170〜173、177〜180、186〜188、190〜194 行の箇条を移し、「ネイティブ化」の項目は `native.md` へのリンクを持つ1行にする。181〜185 行は T-2 まで残す。題は `Add the native design`
- 実装者に任せること: 文の書き方。章立てと決定の中身は変えない
- 条件: いつも
- 完了の条件:
  - `grep -cF` で `'jump threading'`、`'RetN'`、`'MAX_STRUCT_WORDS'`、`'catch_unwind'`、`'scan = 255'` を数える → `docs/future/roadmap.md` ではどれも `0`、`docs/future/native.md` ではどれも1以上
  - `grep -cF 'FnEntry' docs/future/roadmap.md` → `0`
  - `grep -cF 'repr(C)' docs/future/roadmap.md` → `1`
  - `grep -cF '$uniform' docs/future/roadmap.md` → `1`
  - `grep -cF 'read_into' docs/future/roadmap.md` → `1`
  - `grep -cF '](native.md)' docs/future/roadmap.md` → `1`
  - V1〜V5 が上の値。V6 → `docs/future/native.md`、`docs/future/roadmap.md`
- 落とし穴: `native.md` からリンクしてよいのは、`spec/`、`docs/implementation/`、`roadmap.md` の今ある見出しだけである (D-21)

### T-2: `ffi.md` を作る

- 目的: FFI と拡張と GUI の設計 (D-10、D-12) を書く
- 変えるファイル: `docs/future/ffi.md` (新規)、`docs/future/native.md`、`docs/future/roadmap.md`
- 作業: 章立ては `05-integration.md` の 1.3 の「docs/future/ffi.md」。`roadmap.md` の 181〜185 行を移す。`native.md` の「extern の入口」から `ffi.md` へリンクする。題は `Add the FFI and plugin design`
- 実装者に任せること: 文の書き方
- 条件: いつも
- 完了の条件:
  - `grep -cF` で `'int64_t'`、`'Bigarray'`、`'CHandle'`、`'repr(C)'` を数える → `docs/future/roadmap.md` ではどれも `0`、`docs/future/ffi.md` ではどれも1以上
  - `grep -cF '](ffi.md' docs/future/native.md` → 1以上
  - V1〜V5 が上の値。V6 → `docs/future/ffi.md`、`docs/future/native.md`、`docs/future/roadmap.md`

### T-3: `async.md` を作る

- 目的: std の `Async`、I/O、`Chan`、`Net` の設計 (D-6〜D-8) を書く
- 変えるファイル: `docs/future/async.md` (新規)、`docs/future/ffi.md`、`docs/future/native.md`
- 作業: 章立ては `05-integration.md` の 1.3 の「docs/future/async.md」。中身は `03-async.md` を、`05-integration.md` の C1〜C5、C10〜C14、C31、C34 と「Further decisions」で直したものにする。`ffi.md` の「長く走る呼び出し」と `native.md` の「並列の実行」から `async.md` へリンクする。題は `Add the async design`
- 実装者に任せること: 文の書き方
- 条件: いつも
- 完了の条件:
  - `grep -cF '](async.md' docs/future/ffi.md` → 1以上
  - `grep -cF '](async.md' docs/future/native.md` → 1以上
  - V1〜V5 が上の値。V6 → `docs/future/async.md`、`docs/future/ffi.md`、`docs/future/native.md`
- 落とし穴: `async.md` からリンクしてよいのは、`multicore.md` の残す見出し、`native.md`、`ffi.md`、spec だけである

### T-4: `multicore.md` を書き直す

- 目的: 実行系の設計 (D-1〜D-5) で `multicore.md` を書き直す
- 変えるファイル: `docs/future/multicore.md`。新しい見出しへのリンクを足すときは `async.md`、`ffi.md`、`native.md` も
- 作業:
  - 章立ては `05-integration.md` の 1.3 の「docs/future/multicore.md」に従う
  - 決定の表は、`01-runtime.md` の Q7 の判定と、`05-integration.md` の C23〜C27 で直す
  - 新しい行 13〜17 (スレッドの模型、スケジューラ、横取り、長い呼び出し、止める手順) を足す
  - `05-integration.md` の 5.2 の C11 の表は、「印付けの表」に書く
  - 題は `Rewrite the multicore design`
- 実装者に任せること: 文の書き方
- 条件: いつも
- 完了の条件:
  - `grep -cE '^## (スレッドと worker|スケジューラ|止める手順)$' docs/future/multicore.md` → `3`
  - `grep -c '^### 移し戻し$' docs/future/multicore.md` → `1`
  - `grep -cF 'FnEntry' docs/future/multicore.md` → `0`
  - `grep -cF '](multicore.md) の「スケジューラ」' docs/future/async.md` → 1以上
  - `native.md` が先に使った用語を `multicore.md` が定義している: `grep -cF` で `'監視のスレッド'`、`'host のスレッド'`、`'止める語'`、`'専用の worker'`、`'終わりの欄'`、`'救済'`、`'メインの worker'`、`'受け箱'`、`'登録表'`、`'待ちの理由'` を `docs/future/multicore.md` で数える → どれも1以上
  - 「スレッドと worker」が、プロセスのメインスレッドを専用の worker にしたものの名前を「メインの worker」と定め、いつメインの worker になるか (`Main.run` で、GUI を開くときか、始めからか) と、そのとき host のスレッドの仕事 (仕事の終わりを待つこと、止める手順) をどのスレッドがするかを書く。`ffi.md` と `native.md` の同じ語は、そこへのリンクにする
  - `ffi.md` の 5 行の `multicore.md` 全体へのリンクを「スレッドと worker」へ向け直す。`ffi.md` の「`Lin` の物体の所有者の移し替え」へのリンクは、返すときの移し替えが「移し戻し」に移るなら、そこへ向け直す
  - `grep -cF '](multicore.md) の「スレッドと worker」' docs/future/ffi.md` → 1以上
  - a1〜a4 の呼び名の残りが上の表の T-4 の行の数
  - V1〜V5 が上の値。V6 → `docs/future/multicore.md` と、リンクを足した文書
- `async.md` と `ffi.md` が先に書いたことに合わせる (査読で見つかったこと):
  - 次の語も定める: 移し替えの歩き、裏返し、途中の持ち主、止めるビット、監視の刻み、時間の輪、実行待ち、持ち主の worker。`grep -cF` で `'移し替えの歩き'`、`'止めるビット'`、`'監視の刻み'` を数える → どれも1以上
  - `async.md` が「共有の印方式」を引いて移し替えの歩きを述べている箇所は、移し替えを書いた見出し (「`par` の印付け」か「移し戻し」) へ向け直す
  - `async.md` が引く残す見出しに、引かれた中身を置く: 「row の Kind」に書ける `<never e>` と `Never` の印としての `Panic`、「論理的な worker ID」に検査のモードだけの持ち主の ID と、タスクごとの ID と救済の送り手の ID、「`Lin` の物体の所有者の移し替え」に途中の持ち主の規則 (`async.md` の同じ文はそこへのリンクにしてよい)、「未決の論点と将来の拡張」に `par` の取り消し
  - 「スケジューラ」へのリンクは、`async.md` の `start_task` が継続を渡す文か、救済の文か、受け付けの転送の文に置く。`async.md` の `multicore.md` 全体へのリンクは、それぞれに合う見出しへ向け直す
  - 「止める手順」は `async.md` の順番 (worker が入口へ切る、host が worker を join する、プールの仕事を待つ、I/O の driver を閉じる、ヒープを捨てる、host を flush する、`n & 255` で終わる) とシグナルの経路 (待つ者のいない `TERMINATE` は終了コード 143) にそろえる
  - 救済のしきい値、監視の刻み、プールの上限の既定 (`01-runtime.md` の 169 行) は「未決の論点と将来の拡張」に既定として書く (Q-2)
  - 「Rust の側が持つ語」は `async.md` の同じ箇条に join の節を足したものにそろえ、「印付けの表」の C11 の表は `async.md` の「I/O の driver」の表と同じ言葉にする (または、そこへのリンクにする)
- 落とし穴: D-21 の残す見出しの字を変えない。今の「`par` の a1 と協調的マルチタスク」は、「1つの worker での実行」に置き換える

### T-5: 「段の列」を並べる

- 目的: 29の段と後に回す項目を `roadmap.md` に書く (D-13)
- 変えるファイル: `docs/future/roadmap.md`
- 作業: `05-integration.md` の 1.3 の「docs/future/roadmap.md」と 1.4、1.5 に従う
  - 「段の列」の各項目は ``- `段の名前`: 中身。前提: …。設計: [..](..) の「…」`` の形にする
  - `borrowed-params` は今の 219〜224 行の入れ子の箇条を持つ。`async-files` は 122〜124、128〜130、137〜139 行の項目を持つ。`raw-arrays` は 195〜198 行を持つ
  - 「その後の項目」の中で次のものを直す
    - 206 行の見直す条件は「`Std.Async` のタスクの継続の確保が計測で目立ったとき」にする
    - 「JIT (投機的)」の前提は `native-aot` にする
    - 「ループ化」の見直す条件は「ネイティブの展開で巻き上げの不足を測ったとき」にする
    - 40 行 (`Async@lin`) はそのまま残す
  - 「その後の項目」の小見出しは次のようにする
    - 「マルチコア対応」は、後に回す項目だけにする
    - 「非同期」「ネイティブ化」「FFI と拡張」の小見出しを新しく足す。引数を型どおりに受ける版 `f$ret` は 189 行から「ネイティブ化」へ移す
  - 借用パラメータの入れ子の箇条は「段の列」の `borrowed-params` へ移るので、`docs/future/native.md` の「処理系」の Perceus の最適化の項目への2つの引用 (「目的と範囲」と「展開と最適化の前提」) を、`roadmap.md` の「段の列」への引用に直す
  - `f$ret` の項目を「ネイティブ化」の小見出しへ移すとき、前の文脈 (却下した呼び出し規約の案) が `native.md` へ移ったので、`native.md` の「呼び出し規約と入口」へのリンクを添える
  - 「非同期」の小見出しの項目は、`async.md` の「未決の論点と将来の拡張」の後に回す項目ごとに、後に回した理由と見直す条件を書き、`async.md` の既定と食い違わないようにする。「FFI と拡張」も `ffi.md` の同じ節と同じようにする
  - 「マルチコア対応」の今の箇条のうち、左の子を先にする実行時エラーの規則と、失敗したときに種を表示する箇条は `multicore.md` に移った。消してよい。締めの段落 (後に回した項目を並べた段落) は、`05-integration.md` の 1.5 のマルチコアの一覧から書き直す。`multicore.md` の「未決の論点と将来の拡張」にあって 1.5 にない項目 (読むだけの共有、局所的な可変変数、`IO` と大域の状態など) は、`multicore.md` に既定と見直す条件があればそれに合わせて項目にし、なければ項目にしない。private な deque を後に回した理由は `multicore.md` の却下の理由と同じにする
  - この節の行の番号は基準のコミット (4a0fbdf6) のものである。前の作業で動いたので、中身で探す
  - 題は `Lay out the stage sequence`
- 実装者に任せること: 各段の「中身」の文の書き方
- 条件: いつも
- 完了の条件:
  - `` awk '/^## 段の列/{on=1;next} /^## /{on=0} on && /^- `[a-z-]+`:/' docs/future/roadmap.md | wc -l `` → `29`
  - `grep -cE '^## (進め方|段の列|その後の項目)$|^### (型システム|言語機能と構文|診断|処理系|REPL|マルチコア対応|非同期|ネイティブ化|FFI と拡張)$' docs/future/roadmap.md` → `12`
  - 次のコマンドの出力 → `0`
    ```sh
    awk 'function chk(){ if(item!="" && !(r&&c)) print item; item=""; r=c=0 }
         /^#/{chk(); on=($0 ~ /^### (マルチコア対応|非同期|ネイティブ化|FFI と拡張)$/); next}
         on && /^- /{chk(); item=$0}
         on && /後に回した理由/{r=1}
         on && /見直す条件/{c=1}
         END{chk()}' docs/future/roadmap.md | wc -l
    ```
  - ``grep -c 'Async` が入ったとき' docs/future/roadmap.md`` → `0`
  - ``grep -cF '`Async` を作るとき' docs/future/roadmap.md`` → `0`
  - `grep -c '借用パラメータ.*roadmap.md) の「処理系」' docs/future/native.md` → `0`
  - a1〜a4 の呼び名の残りが上の表の T-5 の行の数
  - V1〜V5 が上の値。V6 → `docs/future/native.md`、`docs/future/roadmap.md`
- 落とし穴: `read_into` は「処理系」に残す (`docs/implementation/benchmarks.md:331` が「処理系」を引いている)

### T-6: 地図を新しい設計に向ける

- 目的: 文書の地図と概要を、新しい4つの設計に合わせる
- 変えるファイル: `CLAUDE.md`、`docs/README.md`、`docs/future/stdlib.md`、`docs/overview.md`、`docs/implementation/architecture.md`、`docs/implementation/benchmarks.md`、`docs/implementation/status.md`
- 作業: `05-integration.md` の 1.3 の「docs/overview.md」「docs/README.md」「docs/future/stdlib.md」「CLAUDE.md」に従う。`overview.md` の 31 行と 33 行 (`Panic` と `Never`) は触らない (段 `never-rows` が閉じるときに直す)。あわせて、中身が `native.md` と「段の列」へ移ったための古い参照を直す
  - `benchmarks.md` の 183、330、461、485、491 行の「ロードマップのネイティブ化の項目」の完了の条件と目標を、`native.md` の「目的と範囲」への引用にする。331 行の `read_into` は「処理系」への引用のまま、「、ネイティブ化の項目の」を除く (`read_into` は「処理系」の独立した項目になった)。510 行の「スカラーの要素を生で持つ配列」の項目への引用は、「段の列」の `raw-arrays` への引用にする
  - 「ロードマップのネイティブ化の項目」のような、ロードマップの「ネイティブ化」を指す言い回しは、`benchmarks.md` だけでなく `docs/` 全体で探して直す (`grep -rn 'ネイティブ化の項目' docs`)
  - `status.md` の 46 行のパイプ、文字列の標準入力、`Child` の標準エラーの引用は、`async-files` の段へ移ったので「段の列」に引く
  - `README.md` の `roadmap.md` の行 (37 行) の説明に「段の列」を入れる
  - `overview.md` で「段の列」を引く行は1つにする (完了の条件が数える)
  - `architecture.md` の 10 行の「ネイティブのバックエンドは未定である」を、Cranelift で JIT と AOT を出すことと `native.md` へのリンクに直す
  - `status.md` の 48 行で、借用パラメータとネイティブ化を「その後の項目」でなく「段の列」に引く
  - `overview.md` の a1 と a2 の行 (104 行) を置き換える worker の行は、`multicore.md` の「スレッドと worker」の定義に合わせ、そこへリンクする。`README.md` の `multicore.md` の行 (38 行) の説明は、書き直した `multicore.md` の節 (スレッドと worker、スケジューラ、止める手順など) を挙げる
  - 題は `Point the maps at the new designs`
- 実装者に任せること: 文の書き方
- 条件: いつも
- 完了の条件:
  - `grep -cF 'Cranelift か LLVM' docs/overview.md` → `0`
  - `grep -cE '\]\(future/(async|native|ffi)\.md\)' docs/README.md` → `3`
  - `grep -cF 'roadmap.md) の「段の列」' docs/overview.md` → `1`
  - `git show HEAD -- CLAUDE.md | grep -cE '^[-+][^-+]'` → `2`
  - `grep -cE 'ロードマップ\]\(\.\./future/roadmap\.md\) のネイティブ化の項目|ロードマップの「ネイティブ化」' docs/implementation/benchmarks.md` → `0`
  - `grep -cF '](../future/native.md)' docs/implementation/benchmarks.md` → 1以上
  - `grep -cF 'ネイティブのバックエンドは未定' docs/implementation/architecture.md` → `0`
  - `grep -cF 'roadmap.md) の「段の列」' docs/implementation/status.md` → 1以上
  - a1〜a4 の呼び名の残りが上の表の T-6 の行の数
  - V1〜V5 が上の値。V6 → `CLAUDE.md`、`docs/README.md`、`docs/future/stdlib.md`、`docs/implementation/architecture.md`、`docs/implementation/benchmarks.md`、`docs/implementation/status.md`、`docs/overview.md`

### T-7: spec の言い回しを設計に合わせる

- 目的: spec の中の古い呼び名 (`run_scheduler`、a1、a2-cancel) と、移した中身への引用を直す。規則は変えない
- 変えるファイル: `docs/spec/core-ir.md` (9 行と 13 行の「ネイティブのバックエンドは未定」「Cranelift か LLVM」を Cranelift と `native.md` への引用に、232 行の借用パラメータの引用を「段の列」に)、`docs/spec/expressions.md` (「並行処理」を `Async.run` と `Net` の例に)、`docs/spec/lir.md` (60 行の借用パラメータの引用を「段の列」に、290 行)、`docs/spec/object-model.md` (194 行、358 行、732〜735 行)、`docs/spec/vm.md` (391 行、400 行、402 行)
- 作業: 引用先は、書き直した `multicore.md` の次の見出しにする (行は基準のコミットのもの)
  - `object-model.md` の 194 行: 「別のスレッドからの解放」は「確定した設計判断」の 7 で決まったので、「決めていない」の文を、決まったことへの引用に直す
  - `object-model.md` の 358 行 → 「別のスレッドからの解放」
  - `object-model.md` の 732〜735 行 → 「1つの worker での実行」。行の型は `<>` でなく `<never e>` にする
  - `vm.md` の 400 行 → 「未決の論点と将来の拡張」 (`par` の子の取り消しは、そこの項目である)
  - `vm.md` の 402 行 → 「Rust の側が持つ語」
  - `expressions.md` の 358 行の `run_scheduler` → `Async.run`
  - `lir.md` の 290 行と `vm.md` の 391 行のロードマップの「マルチコア対応」への引用 → `multicore.md` の「1つの worker での実行」
  - `lir.md` の 449 行と 592 行の借用パラメータの引用 (見出しのない `roadmap.md`) → 「段の列」
  - 今の単一スレッドの動作を書いた文 (`vm.md` の 355 行のシグナルの受け方など) は規則なので変えない
  - 題は `Align spec prose with the runtime design`
- 実装者に任せること: 文の書き方
- 条件: いつも
- 完了の条件:
  - `grep -cF 'run_scheduler' docs/spec/expressions.md` → `0`
  - `grep -rcF 'roadmap.md) の「マルチコア対応」' docs/spec | awk -F: '{s+=$2} END{print s}'` → `0`
  - `grep -cE 'ネイティブのバックエンドは未定|Cranelift か LLVM' docs/spec/core-ir.md` → `0`
  - `grep -c '借用パラメータ.*roadmap.md) の「処理系」' docs/spec/core-ir.md docs/spec/lir.md` → 2行とも `:0`
  - a1〜a4 の呼び名の残りがすべて `0`
  - V1〜V5 が上の値。V6 → `docs/spec/core-ir.md`、`docs/spec/expressions.md`、`docs/spec/lir.md`、`docs/spec/object-model.md`、`docs/spec/vm.md`

### T-8: 段を閉じる

- 目的: 「進め方」の閉じる作業を行い、計画を消す
- 変えるファイル: `docs/plans/2026-10-23-runtime-design-docs.md` と付録のディレクトリを消す
- 作業:
  - 「進め方」の閉じる手順は次のように扱う
    - 1 は T-7 で済んでいる
    - 2 は crate、std、bench を変えていないので行わない
    - 3 と 4 は変えるものがない
    - 5 は、この段が「段の列」にないので行わない
    - 6 と 7 は T-6 で済んでいる
    - 8 がこの作業である
  - 消す前に、5 の決定がすべて `docs/` に書いてあることを確かめる
  - 題は `Close runtime design docs`
  - メッセージに次を書く
    - 文書で何をどこへ移したか
    - テストの変更がないこと
    - 計測がないこと
    - 後に回した項目と、利用者が答えた点 (10 の Q-1) を決めた日 (2026-10-23)
- 実装者に任せること: なし
- 条件: いつも
- 完了の条件:
  - `ls docs/plans 2>/dev/null | wc -l` → `0`
  - `cargo nextest run 2>&1 | grep -cE '[1-9][0-9]* failed'` → `0`
  - V1〜V4 が上の値。V5 → `2`

## 8. 実行の順

T-0 → T-1 → T-2 → T-3 → T-4 → T-5 → T-6 → T-7 → T-8 の一列で、並行にできる作業はない。リンクは前の文書の見出しにだけ向かうので (D-21)、順を入れ替えると V1 が失敗する。

## 9. 検証

- 基準 (最初の変更の前): V1〜V5 が 7 の値になる。2026-10-23 に orchestrator が確かめた。全テスト `cargo nextest run` は、基準のコミットですべて通る
- 全体の検証 (T-8 の後): `cargo nextest run` → 失敗 0。V1 → `1`、V2 → `0`、V3 → `1`、V4 → `0`、V5 → `2`。`ls docs/plans 2>/dev/null | wc -l` → `0`
- 利用者の確認: 4つの設計文書と「段の列」を読み、5 の決定と食い違う所がないかを見る

## 10. 危険と未解決の項目

| ID | 項目 | 影響 | 決め方と時期 | 決まらないときの扱い |
|---|---|---|---|---|
| Q-1 | 利用者の答え (2026-10-23)。C の整数の幅は `Int as int`、OS ごとの link は `link on <os>` と `--link`。`task-limits` のネイティブの費用 (末尾でない呼び出しごとの旗の確かめ) は受け入れる。ネイティブ化の段は `gui-plugin` の後 | T-2、T-3、T-5 | 決まっている | — |
| Q-2 | 付録で未解決にした項目 (救済の刻みと閾値、粒度 1024、プールの上限 32、ページの返却なし、`compio-driver` の単独での使い方、egui の閉包のない入れ物 など) | 各段の設計 | それぞれの段の設計で決める | 各文書の「未決の論点と将来の拡張」に、付録の既定の値と一緒に書く |
| Q-3 | Windows での `return_call` の実行と、`cranelift-object` の出力は、まだ試していない (Cranelift の出力だけを確かめた) | `native-aot` | 段 `native-aot` で確かめる | `native.md` の「未決」に、Unix だけ `tail` を使う代わりの案と一緒に書く |
| Q-4 | 新しいエラーの番号 (`<never e>` に合わない row、`blocking` に `Async` がない、FFI の読み込みの失敗) | 各段 | 各段の計画で決める | 文書には番号を書かない |

## 付録: 査読での主な変更

議論のやり直しを防ぐため、査読で結論が変わった点だけを書く。

- 始まったタスクが worker から動かないと、1つの重いタスクが同じ worker の接続をすべて止める → 受け入れ。監視のスレッドと救済の盗みを足した (D-1)
- join での手伝いが際限なく入れ子になる → 受け入れ。手伝えるのは、待つ join より深い fork の子だけにした
- 子の row を `<>` にすると、子は `Int` の足し算もできない (今は `Panic` を持つ)。一方、`Panic` は `Once` なので `Row<Never>` に入らない (`docs/spec/types.md:24`) → `Panic` を `Never` に分け直し、`<never e>` を書けるようにした (D-5)
- 継続を捨てる強い取り消しでは、スコープの後始末と、`Unr` の後始末 (channel を閉じるなど) が走らない → 受け入れ。`Wake.Cancelled` で起こして IO の失敗として投げる形にした (D-6)
- 1つの接続が深さかメモリの上限に届くと、サーバ全体が止まる (外から止められる) → 受け入れ。段 `task-limits` を足した (D-6)
- Cranelift の `try_call` と自前の巻き戻し器 → 測って退けた。戻り値で伝える中断と自前の setjmp / longjmp にした (D-9)
- VM の FFI は Cranelift に頼るのに、VM を「Cranelift がないときの代わり」とも書いていた → 代わりの役を取り下げた。橋渡しの関数は `eml_native` の FFI のモジュールが作る (D-10)
- FFI と GUI がネイティブ化の後ろにあった → 2つは VM だけで動くので、ネイティブ化より前にした (D-13)
