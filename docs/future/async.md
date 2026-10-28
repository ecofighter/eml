# 非同期の設計

位置づけ: 将来の設計。

std の `Async` (タスク、スコープ、取り消し、時間) と、I/O の driver、タスクの間の channel、ネットワークとシグナルをまとめる。1つの worker の上のタスク、暗黙のスコープと名前付きのスコープ、`await`、`try_all`、取り消しと `shield`、操作の予算の規則は、[標準ライブラリ](../spec/stdlib.md) の「`Std.Async`」と [VM](../spec/vm.md) の「`Async` のタスク」にある。この文書は、その上に足す時間、channel、I/O、複数の worker の上のタスクなどの設計を書く。worker は [マルチコア対応の設計](multicore.md) の「スレッドと worker」に、スケジューラは [マルチコア対応の設計](multicore.md) の「スケジューラ」に、スレッドをまたぐ RC は [マルチコア対応の設計](multicore.md) の「スレッドをまたぐ RC」にあり、この文書はその上で利用者から見える意味と、std とランタイムの境目を書く。C の関数をタスクの中で待つ形は [FFI と拡張の設計](ffi.md) の「長く走る呼び出し」に、ネイティブのコードでタスクが切り替わる所は [ネイティブ化の設計](native.md) の「並列の実行」にある。

## 目的と範囲

- 目的: 多数の接続を1つのプロセスで受けるネットワークのサーバを eml で書けるようにする。接続ごとにタスクを1つ作り、1つの接続の失敗 (IO の失敗、`Panic`、タスクごとの上限) をその接続の中にとどめる
- 範囲: `Async` のエフェクト、スコープとタスク、失敗と `exit`、取り消し、時間、channel と共有の状態、I/O の driver、`Net` と `Signal`、非同期のファイルとパイプ、`compute`、テスト
- 範囲の外: worker のループ、受け箱、救済、横取り、スレッドをまたぐ RC、データ並列の `par`。worker のループ、受け箱、救済、横取りは [マルチコア対応の設計](multicore.md) の「スケジューラ」に、スレッドをまたぐ RC は [マルチコア対応の設計](multicore.md) の「スレッドをまたぐ RC」に、`par` は [マルチコア対応の設計](multicore.md) の「並列と並行の API」にある
- 範囲の外: TLS、HTTP、時刻。下の「ネットワークとシグナル」と「未決の論点と将来の拡張」に、後で足す形を書く
- 並行処理のための新しい構文は足さない。タスクはラムダと `use` で書く

## 確定した設計判断

| # | 領域 | 決定 |
|---|---|---|
| 1 | エフェクト | `Async` は普通の eml のエフェクトで、extern のエフェクトにしない。待ちの生の操作の結果はいつも `Wake` の値を持ち、ランタイムとの ABI として公開して文書に書く |
| 2 | 入口 | `Async.run : (Unit -> <IO, Panic, Async> a) -> <IO, Panic> a` が根のスコープで、`main` では `use Async.run` と書く。VM では呼んだ側を待たせる組み込みの上に作る |
| 3 | スコープ | nursery の形にする。子が失敗したら、その時点でスコープのほかのタスクをすべて取り消す。`Task a` は `Lin` で、捨てると取り消しになる |
| 4 | 分離 | 接続ごとの分離は、タスクの中で `Async.try_all` を使って行う。失敗は `Failure` の `IOFailed` か `Panicked` で受ける |
| 5 | 取り消し | 取り消しは `Wake.Cancelled` で届け、std の包みが IO の失敗 `Prelude.Cancelled` として投げる。タスクは自分のフレームとスコープの後始末を通って巻き戻る。継続を捨てる強い取り消しは `exit`、致命的な誤り、run の終わり、回復のときだけにする。柔らかい確かめの名前は `check_cancelled` と `cancel_requested` である |
| 6 | `exit` | `exit` は自分のタスクだけを巻き戻し、`Control` の終わりの欄に終了コードを書いて終わりのビットを立てる。ほかのタスクは巻き戻さず、ヒープを落とすときに `Lin` の資源を後始末する |
| 7 | タスクごとの上限 | タスクの中で深さ、スタック、メモリの柔らかい上限を超えると、そのタスクの IO の失敗 `ResourceLimit` になる。メモリの硬い上限は今と同じく run 全体を止める |
| 8 | 失敗の種類 | `IOErrorKind` は `Cancelled`、`TimedOut`、`ConnectionReset`、`AddrInUse`、`ResourceLimit` を持つ。extern はこの5種類を出さず、出すのは `fail_io` だけである ([標準ライブラリ](../spec/stdlib.md) の「`try_io`」) |
| 9 | 時間 | 時間は今からの長さを `Int` のミリ秒で表す。名前を付けたスコープを期限付きで待つ関数は `Async.drain_within` である |
| 10 | I/O | std の API は OS によらず完了型にする。buffer は所有ごと渡して返してもらい、受け付け、待ち、取り出しの3段で行う。`IoDriver` の trait を `eml_rt` に置き、`eml_io` が実装する。本物の driver は `compio-driver` の上に作り、unix は polling、Windows は IOCP を使う |
| 11 | ハンドルと worker | ハンドルは最初に受け付けた worker の I/O の driver に縛る。別の worker からの受け付けはその worker の受け箱を通して回す |
| 12 | 読みの buffer | 読みの buffer の容量はソケットが読めるようになってから確保する。IOCP ではまず0バイトの受信を出す |
| 13 | ブロックする呼び出し | ブロックする呼び出しのプールはヒープを持たない。`blocking` の行は型に `<IO, Async>` を要する。std の今の同期の行はタスクの中でその場で動き、非同期のファイルとパイプは I/O の driver で作る |
| 14 | channel | `std/Chan` は容量付きの MPMC の1つの型で、閉じた channel への送りは値を送り手に返す。複数の channel の `select` は入れない |
| 15 | 共有の状態 | 共有の状態は持ち主のタスクの後ろに `Chan` を置く形 (actor) で書く。利用者に見せる atomic な操作は `Atomic.Counter` だけである |
| 16 | 受け手の決まらない `Lin` の値 | MPMC の `Chan` と `Task` の結果で送る `Lin` の値は、途中の持ち主の ID を付けて送る |
| 17 | Rust の側が持つ語 | channel の buffer、タスクのセルの結果、スコープの失敗の欄、join の節点、プールの仕事の記録は、run の登録表に載せる |
| 18 | シグナル | シグナルの handler は `Control` のビットを立てて監視のスレッドを起こし、監視のスレッドが `Signal.wait` で待つタスクへ完了として届ける。待つタスクがなければ、`INTERRUPT` は今と同じく致命的で、`TERMINATE` は同じ止め方で run を止めて 143 で終わる |
| 19 | `Net` のハンドル | `TcpListener` と `TcpStream` は `Lin` の `Foreign` で、破棄処理がソケットを閉じる |
| 20 | 決定性 | `Async` の UI テストはどの seed でも同じ出力を返す |
| 21 | 操作の予算 | 待たずに済む速い道が 128 回続いたら、タスクはほかのタスクに worker を渡す |
| 22 | タスクの論理的な ID | 検査モードでは、`par` の子と同じく、タスクごとに新しい論理的な worker ID を割り当てる |

## `Async` のエフェクト

```haskell
pub abstract effect Async where
  await_cell     : Cell -> Wake                  -- タスクかスコープのセルが終わるまで待つ
  await_io       : OpId -> (Wake, OpId)          -- I/O の driver の操作が終わるまで待つ
  await_io_until : OpId -> Int -> (Wake, OpId)   -- 同じく待ち、今から Int ミリ秒が過ぎたら操作を取り消す
  await_send     : Cell -> Wake                  -- channel に枠が空くか閉じるまで待つ
  await_recv     : Cell -> Wake                  -- channel に品物が来るか閉じるまで待つ
  await_time     : Int -> Wake                   -- 今から Int ミリ秒待つ
  await_drain    : Scope -> Wake                 -- スコープに属するタスクがなくなるまで待つ
  reschedule     : Unit -> Wake                  -- 実行待ちに戻る (`yield_now` と操作の予算)

pub data Wake = | Woke | Cancelled
```

- このうち `await_cell`、`await_time`、`await_drain`、`reschedule`、`await_send`、`await_recv` の6つは実装してある ([標準ライブラリ](../spec/stdlib.md) の「`Std.Async`」)
- `Async` は extern のエフェクトにしない。extern のエフェクトは yield しうるラベルでなく ([型と Kind](../spec/types.md) の「row の具体化の記録」)、`Async` の操作は継続を取り出して待つためである
- 生の操作は eml とランタイムの間の ABI である。`Async` は抽象エフェクトで、名前は見えるが操作は見えない。操作はランタイムの ABI として文書に書く。利用者は下の節の包みの関数を呼ぶ。`Cell` は登録表の添字を持つ `Unr` の即値 (`Repr::Int`) で、`OpId` はランタイムの token の `Foreign` である。どれも std の中で使う。`OpId` は `Lin` で、待つ間はランタイムが預かり、再開するときに `Wake` と組にして返す
- ランタイムが名前で呼ぶ入口は、単相の `start_task : (Unit -> <IO, Async> Unit) -> <IO> Unit` と `resume_task : (once Wake -> <IO> Unit) -> Wake -> <IO> Unit` である。単相なので、名前で ABI が決まり、instance の名前が正式な名前のままになる。`start_task` は、生の操作ごとに節を1つ持つ handler をタスクの本体にかぶせる。節は継続 `k` をランタイムに預けて (待ちに入れるか、実行待ちに戻す) そのまま戻る。ランタイムは後で `resume_task` で `k` に `Wake` の値を渡し、タスクを再開する ([マルチコア対応の設計](multicore.md) の「スケジューラ」)。タスクの本体は std の底の関数で包む。底は `Panic` と IO の失敗を捕まえ、結果を値のまま、引数を預かる extern (`finish_ok`) に渡す。`await` と `run` は預けた値を `take_result` と `run_result` で1回受け取る。`Lin` の結果も thunk に包まずに渡せる ([標準ライブラリ](../spec/stdlib.md) の「`Std.Async`」)
- 生の操作の結果はどれも `Wake` を持つ (`Wake` そのものか、`Wake` と預けた token の組)。そのため、ランタイムはどの待ちのタスクにも即値の `Cancelled` (`await_io` と `await_io_until` では預かった token との組) を渡して再開でき、型が合う。これは std の約束で、テストで確かめる
- 待ちうる包みの関数はどれも取り消しの地点である。取り消しはここで届く ([標準ライブラリ](../spec/stdlib.md) の「取り消し」)
- 操作の予算: 待たずに済む速い道 (channel の送りと受け取り、すぐに終わる完了) は、`check_cancelled` と同じくタスクのセルの予算 ([標準ライブラリ](../spec/stdlib.md) の「操作の予算」) を使う。予算を使い切ると速い道は待つ必要があると答え、包みは `reschedule` を通ってほかのタスクに worker を渡す。`try_send` と `try_recv` は予算を使っても譲らず、`send`、`recv`、`check_cancelled` が譲る。ネイティブのコードも同じ核の入口で予算を使う。予算は worker の止める語に触れないので、VM の横取り (`PREEMPT`) とは別に働く。VM の横取りは CPU を使い続けるバイトコードを抑え、予算は VM でもネイティブでも速い道の連続を抑える

## スコープとタスク

`run`、`start`、`spawn`、`await`、`yield_now`、名前付きのスコープ (`scope`、`start_in`、`spawn_in`、`cancel_scope`)、`cancel` と `Task a` は [標準ライブラリ](../spec/stdlib.md) の「`Std.Async`」と「スコープ」にあり、セルの形は [VM](../spec/vm.md) の「セルと登録表」にある。ここには、複数の worker とネイティブのコードのために足すものを書く。

- タスクの閉じた row `<IO, Panic, Async>` は `Heap h` も除く
- 複数の worker では、タスクのセルは今の持ち主の worker を持つ。取り消しのビットとスコープの旗は atomic にし、スコープの属するタスクと入れ子のスコープの一覧は mutex で守る。表現を替えるだけで、印を付ける手順と受け箱の処理の呼び口は変えない ([VM](../spec/vm.md) の「取り消しの届け方」、[マルチコア対応の設計](multicore.md) の「`Async` のタスク」)
- `cancel`、`drop`、スコープの取り消しは、取り消しの知らせをタスクのセルの持ち主の worker の受け箱へ送る。`Task a` はほかの `Lin` の値と同じく `Chan` で送れる
- ネイティブのコードでは、`Async.run` はマシンのスタックの上で worker のループを入れ子にする。呼んだ側が完了の後に再開するという約束は VM と同じである ([VM](../spec/vm.md) の「待ちの2種類」、[ネイティブ化の設計](native.md) の「並列の実行」)
- ハンドルにスコープの型引数は付けない。付けると `run_heap` のようなランク2の特別な形が要るためである。スコープの外へ出た `Task a` は、取り消されたものでもありうる

## 失敗と `exit`

`Failure`、`try_all`、`fail_io` と、1つの worker の上の失敗の規則は [標準ライブラリ](../spec/stdlib.md) の「`Std.Async`」と「`fail_io`」にあり、`IOErrorKind` の種類は同じ文書の「`try_io`」にある。ここには、複数の worker のために足す規則と、接続ごとの分離の例を書く。

- 親の `try_io` が子の失敗を捕まえるのは、`try_io` が `await` か `scope` を囲むときである

```haskell
probe : String -> <IO, Panic, Async> Result Failure Int    -- あいさつの最初の塊のバイト数
probe host =
  Async.try_all (fn () ->
    let conn = Net.connect host 7
    let (chunk, conn) = Net.read_chunk (Bytes.empty ()) (Net.write_line "hello" conn)
    Net.close conn
    Bytes.length chunk)

probe_all : List String -> <IO, Panic> List (Option (Result Failure Int))
probe_all hosts =
  use Async.run
  let tasks = List.map (fn h -> Async.start (fn () -> Async.timeout 5000 (fn () -> probe h))) hosts
  List.map Async.await tasks
```

`try_all` は `probe` の中にあるので、1つの接続の失敗は `Err` として返り、スコープの失敗にならない。`timeout` は `try_all` の外にあるので、期限を過ぎた接続は `None`、失敗した接続は `Err` になる。

- worker が複数のときも、`exit n` はどのタスクで呼んでもプログラムを終える。底が `Control` の終わりの欄に終了コードを書いた後 (最初の書き手が勝つ)、終わりのビットを立てる。各 worker は次の安全点で自分の入口へ戻り、自分のスレッドの上で I/O の driver を空にしてから終わる。driver を空にするとは、操作を取り消し、実行中のプールの仕事も含めて完了がすべて届くまで時間の上限なしに待つことである。host のスレッドはすべての worker を待ち、その後、ヒープを落とし (ファイナライザが書き出して閉じる)、Host を書き出し、`n & 255` で終わる ([マルチコア対応の設計](multicore.md) の「止める手順」)。1つの worker の手順は [VM](../spec/vm.md) の「`Async` のタスク」の「終わり」にある
- 実行時エラー (ヒープの誤り、上限、`Signal.wait Interrupt` で待つタスクのない割り込み) は今と同じく run 全体を止め、1回だけ報告する ([標準ライブラリ](../spec/stdlib.md) の「実行時エラー」)

### タスクごとの上限

1つの接続が深さかメモリの上限に届くと、サーバ全体が止まる。外から止められることになるので、タスクごとの柔らかい上限を置く。

- すべての安全点と、末尾でないすべての呼び出しの位置を巻き戻しの地点にする。VM では巻き戻しの表の行がデータなので、実行時の費用はない。ネイティブのコードでは yield の地点でない末尾でない呼び出しの位置ごとに旗の確かめが1つ増える ([ネイティブ化の設計](native.md) の「タスクごとの上限」)。この費用は受け入れる
- タスクの中で深さかスタックの上限を安全点で超えたときは、その安全点でそのタスクの IO の失敗 `ResourceLimit` を起こす。`try_all` はこれを捕まえる。タスクの外では今と同じく run 全体を止める
- `Control` にメモリの柔らかい上限を置く (既定は `max_bytes` の 7/8)。タスクのセルはそのタスクが動く間に確保したバイト数を数え、確保器の遅い道で足す。柔らかい上限を超えると、監視のスレッドが、動いているか実行待ちのタスクのうち数のいちばん大きいものに旗を立て、そのタスクの次の安全点で `ResourceLimit` を起こす
- メモリの硬い上限は今と同じく run 全体を止める
- 残る危険: 待ちのタスクが持ち続けるメモリは、どのタスクにも数えない。柔らかい上限と硬い上限の間で起きる1回の大きな確保は、今と同じく致命的である。数のいちばん大きいタスクが原因のタスクとは限らない

## 取り消し

取り消しの規則 (2つの所、届け方、std の包み、続く取り消し、`shield`、`check_cancelled` と `cancel_requested`) は [標準ライブラリ](../spec/stdlib.md) の「取り消し」と [VM](../spec/vm.md) の「取り消しの届け方」にある。ここには、時間、I/O、複数の worker のために足すものを書く。

- I/O の待ちでは、ランタイムは預かった token と組にした `Cancelled` を継続に渡し、完了が届いてから再開する (下の「待ちを外す」)。待ちを外すのも、取り消しの地点でビットと旗を読むのも、セルの持ち主の worker の上である
- 待たない後始末 (数を減らす、`Chan.close`、`try_send`、`eprintln`、ハンドルを閉じる) は、`try_all` の handler の中でそのまま走る。待つ必要のある後始末 (満ちた channel で登録の actor に知らせる、別れの書き込み) は `Async.shield` で包む。`shield` は利用者が終わりを約束する範囲で、スコープはそれを待つ
- `finally` も、利用者が定義する破棄処理も置かない。破棄処理は eml のコードを動かさない ([線形性](../spec/linearity.md) の「破棄処理」)。`Lin` の規則と `try_all` と `shield` で、サーバの後始末 (接続の数を減らす、channel を閉じる、登録の更新) は書ける
- 取り消しは協調的で、CPU を使い続けるコードを止めない。`compute` の周りの `timeout` は `par` の仕事が終わったときに戻る (`par` の取り消しは後で足す。[マルチコア対応の設計](multicore.md) の「未決の論点と将来の拡張」)。サーバでは CPU を多く使う仕事を `compute` に入れる。`compute` はタスクを待たせるので、worker はほかの接続を受け続ける。待たずに `compute` も使わないネイティブのタスクは、終わるまで worker を持ち続ける。そのときは、監視のスレッドが詰まりを見つけた後で、空いた worker が同じ worker のほかのタスクを救済する

### 待ちを外す

取り消しで再開する前に、ランタイムは待ちの理由ごとに待ちを外す。外すのは、取り消しを処理するときのタスクのセルの持ち主の worker で、その受け箱の lock の下で行う。救済で持ち主が変わると、外す務めも新しい持ち主へ移る ([マルチコア対応の設計](multicore.md) の「スケジューラ」)。セルの待ち、実行待ち、時間の待ちの外し方は [VM](../spec/vm.md) の「取り消しの届け方」にある。

待つ継続はどの待ちでも待つタスクのセルが持ち、待ちの登録 (時間の輪、channel の待ち手、I/O の操作の記録、プールの仕事の記録) はタスクのセルの番号だけを持つ。タスクのセルの待ちの欄は、何を待っているかを名指す。核の持たない登録の待ちでは、受け箱の処理が待ちの種類を実行系に返し、実行系は自分の登録を外してから、タスクのセルから継続を取り出して `Cancelled` で再開する。時間と channel の待ちではすぐに取り出し、I/O の操作とプールの仕事の待ちでは完了の後に取り出す。取り消し中の状態は、待ちの欄が残ったまま取り消しのビットが立っている形で表し、状態の種類は足さない。

- 時間の待ち: 時間の輪は核の登録表にあるので、受け箱の処理が実行系を通さずに輪の項を消す ([VM](../spec/vm.md) の「時間の待ち」)
- channel の待ち: channel の lock の下で、待つ側の項をその channel の待ち手の列から外す ([VM](../spec/vm.md) の「channel」)。外した項は起こさない。待ち合わせの `send` の値は送り手のフレームに残り、巻き戻しが落とす。これは後始末で、漏れではない
- I/O の操作の待ち: タスクを取り消し中の状態にし、ハンドルを縛った worker の I/O の driver に操作の取り消しを頼む (`CancelIoEx`、io_uring の `ASYNC_CANCEL`、epoll と kqueue では登録を外す)。完了 (取り消した状態か、競合したときはデータ) が届いてから `Cancelled` で再開する。取り出しは buffer とハンドルを返し、巻き戻しがそれを落とす。カーネルが書きうる間は buffer を放さない。取り消し中のタスクは完了の前に再開しない
- プールの仕事の待ち: 仕事は止められないので、I/O の操作と同じく完了を待ってから再開する (下の「I/O の driver」の表)

channel の待ちでも、外すことと、すでに出た起こしが競合しうる。規則はセルの待ちと同じで、継続をタスクのセルから先に取り出した側が勝つ。外すのが先なら `Cancelled` で再開する。起こしがすでに出ていれば外すのは負け、`Woke` で再開して channel の操作をやり直す。続く取り消しが次の取り消しの地点で投げる。両方が起きることも、どちらも起きないこともない。I/O の操作とプールの仕事の待ちには、この競合はない。データが競合して届いても、完了の後でいつも `Cancelled` で再開する。

### スコープの取り消し

- 費用は属するタスクの数に比例する。worker をまたぐのは受け箱を通る知らせだけで、ヒープの語は渡らない。「失敗した時点で」は相手の worker の受け箱の遅れの分だけ遅れる
- `scope` の本体を動かすタスクが I/O の driver の操作 (`accept` など) で待っていれば、その操作を取り消してから再開する。そのため、接続のタスクが失敗した、待つだけのサーバもすぐに止まる。接続のタスクを `try_all` で包めば止まらない

### 強い取り消し

- 継続を捨てる強い取り消しは `exit`、致命的な誤り、run の終わりに待ちのまま残ったタスク、回復のときだけにする。そのときは run を止める手順で全 worker が止まり、ヒープを落とす。受け箱、待ちの集まり、I/O の driver の表にある継続の語はヒープとともに放され、ファイナライザがファイルとソケットを閉じる
- I/O の driver の操作の `Lin` の token の破棄処理は、その操作を持ち主のないものとして記録する。driver は完了のとき、または driver を閉じるとき (取り消して、限られた時間だけ完了を待った後) に buffer を放す

### 却下した案

- 強い取り消しを主な仕組みにする案: スコープの終わりの手順と、`Lin` でない後始末をすべて飛ばす
- `Async` の `never` の操作で取り消す案: 動くが、利用者が扱えない2つ目の捕まえ方になる
- 利用者が定義する破棄処理か `finally`: 破棄処理で eml のコードを動かさない規則に反する。`try_all` と `shield` で足りる
- `scope` が本体の `Cancelled` を吸収する案: `scope` は値を返す型で、取り消された本体は返す値を持たない。取り消しを吸収するのは、`Option` か `Either` を返す `timeout` と `race` である

## 時間

`sleep_ms`、`timeout`、`race`、`drain_within` は [標準ライブラリ](../spec/stdlib.md) の「時間」にある。ここには I/O の期限を書く。

- 時間は今からの長さを表す `Int` のミリ秒である。`sleep_ms`、`timeout`、`_within` の関数、生の操作の `await_time` と `await_io_until` はどれもこれを受ける。時刻を扱う `Time` のモジュールは後で足す
- 時間の待ちは、worker ごとの核の登録表にある時間の輪に登録し、満期は driver の時計で見る ([VM](../spec/vm.md) の「時間の待ち」)。テストでは仮想の時計を使う (下の「テストと決定性」)
- 期限付きの読み (`_within` の版) は、期限が来ても接続を閉じない。受け付けた後に `await_io_until` で待つと、ランタイムは待つときに時間を登録し、期限で操作を取り消す。ここでの取り消しはタスクの取り消しではない。完了が届いたら `Woke` で再開し、完了の状態が `TimedOut` なら取り出しは `None` とハンドルを返す

```haskell
-- (a) 何も来ない接続を閉じる: 接続ごと取り消す
read_or_close : TcpStream -> <IO, Panic, Async> Option (Bytes, TcpStream)
read_or_close conn = Async.timeout 10000 (fn () -> Net.read_chunk (Bytes.empty ()) conn)

-- (b) 要求の期限: ストリームを残して 408 を返す
read_request : TcpStream -> <IO, Panic, Async> TcpStream
read_request conn =
  match Net.read_chunk_within 10000 (Bytes.empty ()) conn with
    | (Some req, conn) -> handle req conn
    | (None, conn) -> Net.write_line "408 Request Timeout" conn
```

(a) は本体のタスクを取り消し、巻き戻しが捕まえた `Lin` の `conn` を落として閉じる。(b) は期限で操作だけを取り消し、ハンドルが戻るので何も閉じない。

## チャネルと共有の状態

```haskell
pub extern data Chan a                                  -- Unr のハンドル。1つの型で MPMC
pub new      : Int -> <IO> Chan a                       -- 容量。0 は待ち合わせ
pub send     : a -> Chan a -> <IO, Async> SendResult a
pub recv     : Chan a -> <IO, Async> Option a           -- None: 閉じて空になった
pub try_send : a -> Chan a -> <IO> TrySendResult a      -- 待たない
pub try_recv : Chan a -> <IO> TryRecvResult a           -- 待たない
pub close    : Chan a -> <IO> Unit                      -- 何度呼んでもよい。待つ側をすべて起こす
pub merge    : List (Chan a) -> <IO, Async> Chan a          -- 元ごとに転送のタスクを1つと、終わりを集めるタスクを1つ、いちばん内側のスコープに作る
pub data SendResult a    = | Sent | Rejected a          -- Rejected: 閉じていた。値が戻る
pub data TrySendResult a = | Accepted | Full a | Refused a
pub data TryRecvResult a = | Got a | Empty | Drained
```

- コンストラクタの名前はモジュールの中で別々にする (同じモジュールの `data` の間で名前を使い回すと E1003 になる)
- `Chan` は Go の形で、閉じるのは明示した `close` で行う。`Unr` のハンドルは「送り手がすべて落ちた」を知らせられないためである。閉じた channel への `send` は `Rejected` で値を送り手に返すので、API が `Lin` の値を黙って消費することはない
- 中身の Kind は問わない。`Unr` の値は送るときに印を付け、受けるときに移し替えの歩きで受ける ([マルチコア対応の設計](multicore.md) の「移し戻し」)。送り手が持ち続ける値は共有のままで、reuse しない。`Lin` の値は移す ([マルチコア対応の設計](multicore.md) の「`Lin` の物体の所有者の移し替え」)
- 受け手の決まらない `Lin` の値 (MPMC の `Chan` と `Task` の結果) は、途中の持ち主の ID を付けて送る ([マルチコア対応の設計](multicore.md) の「`Lin` の物体の所有者の移し替え」)。送り手は、値を預かる構造の ID (タスクの結果はタスクの ID、channel の品物は channel のセルの ID) を途中の持ち主にして移し替えをする。この ID は `fork_worker` で出し、構造とともに退かせる。`lin` の物体から rc がすべて 1 の道で届く物体だけが途中の持ち主になり、最初の rc > 1 の物体から下は印付け (rc < 0) に切り替える。受け手の歩きは、rc > 0 で持ち主が途中の ID の物体を受け取って (持ち主を受け手にして) 下り、rc == -1 の物体は今までどおり裏返し、ほかのものでは止まる。持ち主の ID は検査モードにだけある ([マルチコア対応の設計](multicore.md) の「論理的な worker ID」)。速いモードでも送り手の印付けは行い、受け手の裏返しの歩きは変わらない
- 検査モードでは、`par` の子と同じく、タスクごとに新しい論理的な worker ID を割り当てる。run 全体の生きている数の表はタスクとともに大きくなる。速いモードには影響しない
- channel は `close` で放す資源で、閉じて空になった時点でその場で放す。閉じないままの channel と、閉じても品物の残った channel は run の終わりに放し、残った品物をそのとき落とす ([標準ライブラリ](../spec/stdlib.md) の「`Std.Chan`」)。落とした継続が捕まえた `Lin` の値と同じく、これは後始末で、飛ばした後始末ではない
- channel の物体は数えた参照を持つ Rust の構造で、run の登録表に載る。漏れの検査と回復は登録表をたどる。キューはまず mutex の MPMC にし、ロックフリーのキューには後でランタイムの中だけで替える。送り手は直接積み、待っている側はその worker の受け箱を通して起こす
- 容量付きなので、送り手は満ちたときに待つ (back-pressure)。上限のない channel は大きな容量で作る
- `send` と `recv` は取り消しの地点である。速い道 (`try_*` と、`send` と `recv` の中の速い道) は extern で、操作の予算を数える。待つ道は `Std.Async` の `chan_wait_send` と `chan_wait_recv` を通す
- 接続の数の上限は容量 n の `Chan Unit` で書く。`send` で枠を取り (満ちていれば待つ)、`try_recv` で枠を返す (下の「ネットワークとシグナル」の例)
- 1回だけの channel は別に作らない (`Task a` と `await`、または `Chan.new 1` で書く)。broadcast と watch も作らない。複数の channel の `select` は入れない。競わせるのは `race` と `timeout` で、集めるのは `merge` で書く
- 共有して書き換える状態は持ち主のタスクの後ろに `Chan` を置く形 (actor) で書く。`std/Atomic` は `Counter` だけを持つ (`new : Int -> <IO> Counter`、`add : Int -> Counter -> <IO> Int`、`load`、`store`。`Unr`。CAS はない)。atomic な `Cell` も mutex も作らない
- 利用者にメモリモデルは見せない。ロックフリーのキューとほかの atomic な構造は、ランタイムの中だけに置く。`Chan` と `Atomic.Counter` は `IO` の下にあり、`IO` を持たない `par` の子からは届かない

## I/O の driver

### 受け付け、待ち、取り出し

std の非同期の操作はどれも次の3段で行う。extern は yield しないので ([エフェクトと handler](../spec/effects.md) の「組み込みの `IO`」)、待つのは eml の側の生の操作である。

1. 受け付け (`submit_*`): buffer とハンドルを I/O の driver へ移し、`Lin` の token `OpId` を返す extern
2. 待ち: 生の操作 `await_io` で、操作が終わるまでタスクを待たせる。token は `Wake` と組で戻る
3. 取り出し (`take_*`): token を消費して、結果 (buffer とハンドル) を返すか、操作の IO の失敗を投げる extern。取り消した後も buffer とハンドルを返す。取り消したことは `Wake` で分かる

ソケット、シグナル、`Gui.next_event` ([FFI と拡張の設計](ffi.md) の「GUI」)、ブロックする呼び出しのプールの仕事は、どれもこの形で待つ。時間は `await_time` で、`compute` は join の節点のセルでの `await_cell` で待つ。

```haskell
pub read_chunk : Bytes -> TcpStream -> <IO, Async> (Bytes, TcpStream)
read_chunk buf s =
  let (w, op) = await_io (submit_read buf s)   -- submit_read : Bytes -> TcpStream -> <IO> OpId
  let (buf, s) = take_read op                   -- take_read : OpId -> <IO> (Bytes, TcpStream)
  match w with
    | Woke -> (buf, s)
    | Cancelled ->
      drop s                                    -- ハンドルの破棄処理が閉じる
      fail_io Prelude.Cancelled "task was cancelled"   -- 戻らない呼び出し
```

### 所有ごと渡す buffer

- buffer は値で渡す `Bytes` である。受け付けは語を受け取り、一意でなければ (rc ≠ 1。共有の印の付いた rc < 0 の物体を含む) 新しい一意な物体に写す。カーネルが書くのは、参照がちょうど1つの物体だけで、その参照は登録表の操作の記録が持つ。driver は語を見ず、記録が貸す `RawBuf` だけを見る
- 操作の記録は worker の登録表に載り、語は長さを進めて戻る。記録の語は公開しないので、共有の印が付かず、一意なまま残る
- 読みでは、受け付けは buffer を預かるだけで、容量を足すのはソケットが読めるようになってからにする。kqueue と epoll では読めると分かってから容量を `length + 64 KiB` にし (容量が足りなければ一意な物体を確保し直す)、`(payload + length, 64 KiB)` をカーネルに渡す。IOCP ではまず0バイトの受信を出し、それが終わってから容量を足して受信する。そのため、何も来ない接続は読みの buffer を持たない
- `write_bytes` も同じく buffer を移し (一意でなければ写す)、完了の後で放す。buffer は返さない
- Perceus の reuse には頼らない。上の `read_chunk` を繰り返すと、1回ごとに 64 KiB の物体を1つ確保する (`Bytes.empty ()` の容量は 0 で、extern が増やす)

### `IoDriver` と `eml_io`

- `IoDriver` の trait は `eml_rt` の中で `Host` の隣に置く。今は `verus!` の外の安全なつなぎのコードで、`external_body` も信頼の一覧の行も持たない ([VM](../spec/vm.md) の「時計と driver」)。`eml_rt` の extern が driver を要るまでは実行系が持ち、`Ctx` には入れない。`eml_rt` の extern が I/O を持ち、`Ctx` が driver を持つときに、`external_body` と信頼の一覧の行を足す
- 新しい crate `eml_io` が実装する。本物の driver は worker ごとに1つで、その worker の待ちと起こしを受け持つ。テストの driver は仮想の時計、メモリの上のストリーム、seed で決まる完了の順を持つ。どちらも同じ trait の後ろにあり、時計の選び (`Clock`) で選ぶ。driver は作ったスレッドから動かないので、`Session::run` は driver そのものでなく、`Copy + Send` の `Clock` と seed を渡し、各 worker が自分のスレッドの上で driver を作る
- driver のどの待ちも worker を起こす知らせと監視のスレッドの起こしで戻る。今の VM は起こしを使わず、1回の待ちを `IDLE_TICK` で切って代える ([VM](../spec/vm.md) の「時計と driver」)
- 本物の driver は `compio-driver` と `compio-buf` の上に作る。unix は polling (epoll と kqueue)、Windows は IOCP を使い、io_uring は使わない。単独で使えること、スレッドごとに driver を持てること、別のスレッドから起こせる待ちを持つこと、プールの仕事の完了が待ちを起こすことは確かめた。版と、上げるときに確かめることは [アーキテクチャ](../implementation/architecture.md) の「外部 crate」にある
- Windows では、ソケットを IOCP のポートに結び付けてから (`Proactor::attach`) `Recv` と `Send` を出す。今の driver は Windows でハンドルを作らないので結び付けもしない。Windows のソケットを扱うときに足す
- 完了は持ち主の worker の driver の完了の列に届き、集める地点で登録表の操作の記録に当てる。完了の中身は保留の表が持つ数えた参照で、ハンドルを縛った worker とタスクの worker が違えば、普通の移し替えの歩きでタスクへ渡る
- REPL の回復は実行中の操作を取り消して完了を待ちきってから写す

### ハンドルと worker

- 利用者から見える規則はない。ランタイムはハンドルを最初に受け付けた worker の I/O の driver にそのハンドルを縛る。`accept` が返したソケットは縛らずに作り、子のタスクが最初に読む worker に縛る。そのため、`spawn` から最初の操作までの間にタスクが動いても費用はない
- 救済でタスクが別の worker に動いた後の受け付けは、縛った worker の受け箱を通して回し、完了はタスクのセルの持ち主へ届ける ([マルチコア対応の設計](multicore.md) の「スケジューラ」)。これは正しさのために要る
- 救済は回した受け付けを処理しない。縛った worker の driver が要るためである。詰まった worker へ回した受け付けは、その worker が戻るまで待つ。これは遅れで、正しさの問題ではない
- 1つの listener の accept は1つのタスクで行う。`SO_REUSEPORT` で複数のタスクが accept する形は後で足す

### その場で動く同期の行

- extern は待てず、row に `Async` のない行は yield できない。そのため、std の今の同期の行 (`FileIn`、`Stdin`、`Proc`) はタスクの中でその場で動き、その worker を止める。その間、同じ worker のほかのタスクは、監視のスレッドが詰まりを見つけた後で救済される
- サーバがときどきファイルを読むにはこれで足りるが、パイプを汲み続けるには向かない。非同期のファイルとパイプは I/O の driver で作る (下の「ファイルとパイプ」)
- 利用者の `extern "C"` の `blocking` の行は、型に `<IO, Async>` を要し、3段に下ろす。受け付けは C の関数を包んだ仕事をプールに載せ、待ち、取り出しが結果を返す。`Async` のない行に `blocking` を書くと検査の誤りになる ([FFI と拡張の設計](ffi.md) の「長く走る呼び出し」)。std の同期の行には `blocking` を付けない

### ブロックする呼び出しのプール

- プールは各 worker の I/O の driver の一部で、仕事は driver に出す操作 (`Op::Job`) である。仕事の記録は run の登録表の操作の記録で、I/O の操作と同じ待ちの仕組みで待つ
- プールはヒープを持たない。プールのスレッドは RC に触れず、確保もしない。std の内部の仕事は、入力を写して所有して受け取り、結果を値で返す。カーネルに貸す記憶 (`RawBuf`) は driver を持つスレッドから出さないので、仕事には渡さない
- 仕事の記録が渡した語を数えた参照で持つ。`Unr` の引数には印を付けない。`Mut*Array` の引数は記録へ移す。記録は run の登録表に載り、仕事が終わるまで run が生かしておく。プールの側は記録を通してだけ語に届く
- プールは入力の中身を読み、移した `Mut*Array` の中身を書き、スカラー、ハンドル、移した語、Rust のデータを返す。eml の物体は、取り出しが持ち主の worker の上で作る
- 完了は持ち主の worker の driver の完了の列を通して届く。これがプールの書き込みと持ち主の読みの順を決める
- DNS の名前の解決 (`getaddrinfo`) も、`connect` の中で同じ3段のプールの仕事にする
- 戻りの `Mut*Array` と `Lin` の物体は、取り出しが移し戻しで受ける ([マルチコア対応の設計](multicore.md) の「移し戻し」)
- 場合ごとの扱いは次のとおりである

| 場合 | 語を持つもの | 扱い |
|---|---|---|
| 普通の完了 | 登録表の仕事の記録 | 完了がタスクのセルの持ち主へ届き、取り出しが語を戻して記録を放す |
| 仕事の途中でタスクが取り消された | 記録 | 仕事は止められない。タスクは完了を待ってから取り消しで再開し、巻き戻しが戻った語を落とす |
| 待っている間にタスクが救済された | 記録。待ちの理由から届く | 救済する worker は、元の worker の lock の下で、記録の語を継続と同じく扱う。記録が持つ印のない `Unr` の語には、ここで元の worker を送り手として印を付け、`Lin` の語は移し替えにする。変わるのはヘッダだけで、プールは中身にだけ触れるので競合しない |
| `exit` か致命的な誤り | 記録 | host のスレッドが実行中の仕事を待ってからヒープを落とす。2回目の Ctrl-C は今と同じく 130 で終わる |
| REPL の回復 | 記録 | 回復は I/O の driver を空にするときと同じく実行中の仕事を待ってから写す |
| 持ち主が入力をその場で書き換える | なし | 起きない。記録が印のない参照を持つので rc が 2 以上になり、`is_unique` が成り立たないので、書き込みは写す |
| 検査モード | なし | プールの中身の書き込みは所有者を確かめない。その場で動く C の呼び出しが `Mut*Array` を書くときと同じで、宣言を信頼する |

## ネットワークとシグナル

```haskell
pub extern data TcpListener                                                      -- Lin
pub extern data TcpStream                                                        -- Lin
pub listen            : String -> Int -> <IO> TcpListener                         -- ホストとポート (0 は空いているポート)
pub local_port        : TcpListener -> (Int, TcpListener)
pub accept            : TcpListener -> <IO, Async> (TcpStream, TcpListener)       -- ECONNABORTED と EINTR は中でやり直す
pub accept_within     : Int -> TcpListener -> <IO, Async> (Option TcpStream, TcpListener)
pub close_listener    : TcpListener -> <IO> Unit
pub connect           : String -> Int -> <IO, Async> TcpStream                   -- 名前の解決をプールで行い、非同期に接続する
pub read_chunk        : Bytes -> TcpStream -> <IO, Async> (Bytes, TcpStream)      -- 最大 64 KiB を足す。何も足さなければ EOF
pub read_chunk_within : Int -> Bytes -> TcpStream -> <IO, Async> (Option Bytes, TcpStream)
pub read_line         : TcpStream -> <IO, Panic, Async> (Option String, TcpStream)
pub read_line_within  : Int -> TcpStream -> <IO, Panic, Async> (Option (Option String), TcpStream)
pub write             : String -> TcpStream -> <IO, Async> TcpStream
pub write_bytes       : Bytes -> TcpStream -> <IO, Async> TcpStream               -- buffer をすべて書く
pub write_line        : String -> TcpStream -> <IO, Async> TcpStream
pub shutdown_write    : TcpStream -> <IO> TcpStream                               -- FIN。読みは続けられる
pub close             : TcpStream -> <IO> Unit                                    -- drop と同じ
pub peer_addr         : TcpStream -> (String, TcpStream)
pub set_nodelay       : Bool -> TcpStream -> <IO> TcpStream
```

- 名前はハンドルの規則 ([標準ライブラリ](../spec/stdlib.md) の「ハンドル」) に従い、ハンドルを後に、結果を先に置く。`read_line` はストリームごとの buffer の上に eml で書く
- 誤りは IO の失敗で、種類 `Cancelled`、`TimedOut`、`ConnectionReset`、`AddrInUse` を使う。EPIPE は `ConnectionReset` にする
- `TcpListener` と `TcpStream` は `Lin` の `Foreign` で、破棄処理がソケットを閉じる。取り消したタスクの巻き戻しがハンドルを落とすので、取り消しの道で `close` を書かなくてよい
- `accept` は ECONNABORTED と EINTR を中でやり直す。EMFILE は IO の失敗で、accept するタスクが捕まえて待ってからやり直せる

```haskell
import Async
import Net
import Chan
import Signal

session : TcpStream -> <IO, Panic, Async> Unit
session conn =
  match Net.read_line_within 10000 conn with          -- 要求の期限。ストリームは戻る
    | (Some (Some line), conn) -> session (Net.write_line (answer line) conn)
    | (_, conn) -> Net.close conn                     -- 取り消されたときは破棄処理が conn を閉じる

acceptor : Async.Scope -> Chan Unit -> TcpListener -> <IO, Panic, Async> Unit
acceptor conns slots l =
  let _ = Chan.send () slots                          -- 生きている接続が 10000 なら待つ
  let (conn, l) = Net.accept l
  Async.spawn_in conns (fn () ->
    let _ = Async.try_all (fn () -> session conn)     -- IO の失敗、Panic、ResourceLimit はここで止まる
    let _ = Chan.try_recv slots
    ())
  acceptor conns slots l                              -- 取り消されたときは巻き戻しが l を落とす

main : Unit -> <IO, Panic> Unit
main () =
  use Async.run
  let slots = Chan.new 10000
  use conns <- Async.scope
  let _ = Async.race (fn () -> acceptor conns slots (Net.listen "0.0.0.0" 8080))
                     (fn () -> Signal.wait Terminate)
  Async.drain_within 30000 conns
```

`SIGTERM` が来ると `race` が accept するタスクを取り消し、`drain_within` が実行中の接続を最大 30 秒待ち、残りを取り消して終わるまで待つ。接続のスコープはその後で終わりの手順を通る。`session` を `try_all` で包まないと、1つの接続の失敗がスコープの失敗になり、サーバが止まる。

### シグナル

- `std/Signal` は `data Signal = | Interrupt | Terminate` と `wait : Signal -> <IO, Async> Unit` を持つ。`Terminate` は `SIGTERM` で、Windows では `CTRL_CLOSE_EVENT` と `CTRL_SHUTDOWN_EVENT` である
- シグナルの handler は async-signal-safe のまま、`Control` のビットを立て (Ctrl-C は `INTERRUPT`、`SIGTERM` は新しい `TERMINATE`)、自己パイプに1バイト書いて監視のスレッドを起こす ([マルチコア対応の設計](multicore.md) の「スレッドと worker」)。Windows ではコンソールの handler が同じビットを立て、監視のスレッドを起こす。ビットが立ったのを見た監視のスレッドは、そのシグナルを `Signal.wait` で待つタスクが登録されていればビットを下ろし、待つタスクのセルの持ち主へ完了として届ける。登録されていなければ、終わりの欄に致命的な誤りを書いて run を止める。`INTERRUPT` では今と同じである ([VM](../spec/vm.md) の「上限と割り込み」)。`TERMINATE` も同じ止め方で run を止め、終了コードは 143 にする。シェルが SIGTERM で終わった子に返す値で、SIGINT の 130 と同じ考え方である
- `signalfd` と `EVFILT_SIGNAL` は使わない。どちらもすべてのスレッドでシグナルを止める必要があり、今の handler とブロックする読みの EINTR の道と食い違う
- 2回目の Ctrl-C は今と同じく `_exit(130)` である。handler がビットを立ててから監視のスレッドが下ろすまでの間 (高々監視の刻み1つ) の2回目の Ctrl-C も終わる。これは文書にある2回の Ctrl-C の意味と同じである

### 後で足すもの

- UDP、`SO_REUSEPORT` で複数のタスクが accept する形
- 非同期の名前の解決。既定では `getaddrinfo` をプールで動かす。`resolve` も同じ形で持つ
- TLS: Rust のプラグイン (`eml_tls`、rustls の上) にする。形は GUI のプラグインと同じである ([FFI と拡張の設計](ffi.md) の「拡張」)
- HTTP: `Net` と `Chan` の上のライブラリにする

## ファイルとパイプ

- 非同期のファイルと子のプロセスの標準入出力は、I/O の driver の上で、`<IO, Async>` の row を持つ操作として作る。どれも受け付け、待ち、取り出しの3段で行う。子の標準入出力は driver のハンドルにする。Unix では poll できるパイプ、Windows では overlapped の名前付きパイプである
- これで、今ブロックする std の行では書けない3つのことができる。パイプ (`.|`。子の標準出力を次の子の標準入力へ eml で汲む)、`run` と `output` の標準入力を文字列で与えること (`Proc.output_with`)、`Child` の標準エラーの pipe を読み分けることである。今の規則は [標準ライブラリ](../spec/stdlib.md) の「`Child`」にある
- 1つのタスクがファイルを読んでも、同じ worker の別のタスクの組のやりとりは止まらない

## `compute`

```haskell
pub compute : (Unit -> <never e> a) -> <IO, Async | e> a
```

- `compute f` は `f` を、子が1つの `par` の仕事として動かし、終わるまで呼んだタスクを待たせる。その間、worker はほかのタスクを動かす。仕事は `par` の盗みと手伝いのもとで動く
- 子の row は `<never e>` である ([マルチコア対応の設計](multicore.md) の「row の Kind」)。結果の row が `e` を持つので、子の `never` の操作 (`Never` のラベルの `Panic` と `Fail x` を含む) は、join の後で呼んだタスクの中で起こし直す。`par` が親で起こし直すのと同じである
- 中身は子が1つの `par` の節点、join の節点のセルでの `await_cell` (持ち主の受け箱での起こし)、結果の移し替えの歩き、子が中断したときの起こし直しである
- 取り消しは join の後で届く。`par` の取り消しを入れるまで、`compute` の周りの `timeout` は仕事が終わってから戻る
- タスクの中の `Par.map` などは今までどおり普通の手伝いで動き、タスクを待たせない

## テストと決定性

- `async/` の UI テストを seed で流す規則は [テスト戦略](../implementation/testing.md) の「seed で流す」にある。I/O の待ちを持つテストは、seed で完了の順を決めるテストの driver の上で流す。どの seed でも同じ出力を返すことを求めるので、ネイティブのコードでも同じになる
- テストの driver は、メモリの上の `TcpStream` の組 (仮想のホストの上の `listen` と `connect`) と、テストの hook で起こす仮想のシグナルを持つ。時間の待ちは `Clock` が決め、仮想の時計の規則は [VM](../spec/vm.md) の「時計と driver」にある
- 本物のソケットのテストは `tests/ui/run/net/` に少しだけ置く。ループバックでポート 0 に bind して `local_port` で読み、期限を使わない (期限は仮想の時計の上だけで試す)。接続は一度に1つにするか、順によらない出力にする。N = 1 で流し、複数の worker の組 (N が 2 と 8) では順によらないものだけを流す。Windows の CI は同じ組を IOCP の上で流す
- 意味のテスト: 取り消したタスクの `TcpStream` が閉じること (検査モードの漏れの検査で確かめる)、失敗した子がほかのタスクと `accept` で待つタスクをすぐに取り消すこと、`try_all` で `Panic` を分けたサーバが動き続けること、`shield` の中の別れの `send` が取り消しの下でも終わること、テストの driver で取り消した読みが投げる前に buffer を返すこと、`read_chunk_within` が使えるストリームとともに `(None, s)` を返すこと、`Signal.wait Interrupt` が試しの Ctrl-C を取り消しに変えること
- 救済を強いるスケジュール (`--sched rescue-always`。N は 2 以上) で、検査モードの漏れと所有者の検査を流す
- `RunStats` に `completions` を足す (channel の欄 `channel_ops`、`channel_waits`、`channels_swept` はある)。スナップショットは実装した欄と同じく逐次のスケジュール (`--sched sequential`) で固定する ([VM](../spec/vm.md) の「`RunStats`」)

## 未決の論点と将来の拡張

- I/O の driver の crate: `compio-driver` を使う。macOS か Windows で compio の中の理由 (IOCP の時間付きの待ち、起こし、プールの不具合、ビルドの失敗、kqueue の `PollOnce` の振る舞い) で使えないと分かったら、同じ trait の後ろで、`polling` と手で書く IOCP (`windows-sys`) の自前の driver に替える。API は変わらない。mio は Windows で AFD の readiness を真似るので採らない。io_uring を使うかは、ネットワークの計測で readiness の道が目立ったときに決める
- 受け付けをスレッドをまたいで安全に出せる backend (IOCP、SQ を lock する io_uring) では、受け箱を通す回り道を省いてよい。既定は backend ごとに `eml_io` が決める
- `write_bytes` が buffer を返して使い回す形: 既定は返さない。計測で確保が目立ったときに考える
- 2つ目からの失敗をまとめて報告する形 (MultiError): 既定は最初の失敗が勝ち、ほかは数えるだけにする
- 複数の channel の `select`: 既定は入れない。`race`、`timeout`、`merge` で書く
- broadcast と watch の channel: 既定は作らない
- `Counter` のほかの `Atomic`: 既定は作らない
- タスクを置く worker を選ぶ API (`spawn_here` など): 既定は作らない。スレッドに縛られた仕事は `Gui.run_on_main` で書く ([FFI と拡張の設計](ffi.md) の「GUI」)
- channel のキュー: 既定は mutex で、ロックフリーには後でランタイムの中だけで替える
- `Lin` の closer token: 既定は作らない。`Chan.owned : Int -> <IO> (Chan a, ChanOwner)` のように `Lin` の `Foreign` の `ChanOwner` を足し、その `Drop` が受け箱に閉じる知らせを積む形である。`TaskHandle` と同じ仕組みなので、`eml_rt` は変わらず、送受信に費用を足さない。これで足りなければ、`Chan a` を `Unr` の `Foreign` のハンドルにする。channel の Rust の構造は変わらず、ハンドルの読みと放す引き金だけが変わる ([ロードマップ](roadmap.md) の「非同期」)
- `Time` のモジュール: 上の「時間」にある
- UDP、`SO_REUSEPORT`、TLS のプラグイン、`Http`、非同期の名前の解決: 上の「ネットワークとシグナル」の「後で足すもの」にある
