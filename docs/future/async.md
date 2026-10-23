# 非同期の設計

位置づけ: 将来の設計。

std の `Async` (タスク、スコープ、取り消し、時間) と、I/O の driver、タスクの間の channel、ネットワークとシグナルをまとめる。どれも実装していない。worker、スケジューラ、スレッドをまたぐ RC の仕組みは [マルチコア対応の設計](multicore.md) にあり、この文書はその上で利用者から見える意味と、std とランタイムの境目を書く。C の関数をタスクの中で待つ形は [FFI と拡張の設計](ffi.md) の「長く走る呼び出し」に、ネイティブのコードでタスクが切り替わる所は [ネイティブ化の設計](native.md) の「並列の実行」にある。

## 目的と範囲

- 目的: 多数の接続を1つのプロセスで受けるネットワークのサーバを eml で書けるようにする。接続ごとにタスクを1つ作り、1つの接続の失敗 (IO の失敗、`Panic`、タスクごとの上限) をその接続の中にとどめる
- 範囲: `Async` のエフェクト、スコープとタスク、失敗と `exit`、取り消し、時間、channel と共有の状態、I/O の driver、`Net` と `Signal`、非同期のファイルとパイプ、`compute`、テスト
- 範囲の外: worker のループ、受け箱、救済、横取り、スレッドをまたぐ RC、データ並列の `par`。これらは [マルチコア対応の設計](multicore.md) にある
- 範囲の外: TLS、HTTP、時刻。下の「ネットワークとシグナル」と「未決の論点と将来の拡張」に、後で足す形を書く
- 並行処理のための新しい構文は足さない。タスクはラムダと `use` で書く

## 確定した設計判断

| # | 領域 | 決定 |
|---|---|---|
| 1 | エフェクト | `Async` は普通の eml のエフェクトで、extern のエフェクトにしない。待ちの生の操作の結果はいつも `Wake` の値を持ち、ランタイムとの ABI として公開して文書に書く |
| 2 | 入口 | `Async.run : (Unit -> <IO, Panic, Async> a) -> <IO, Panic> a` が根のスコープで、`main` では `use Async.run` と書く。VM では呼んだ側を待たせる組み込みの上に作る |
| 3 | スコープ | nursery の形にする。子が失敗したら、その時点でスコープのほかのタスクをすべて取り消す。`Task a` は `Lin` で、捨てると取り消しになる |
| 4 | 分離 | 接続ごとの分離は、タスクの中で `Async.try_all` を使って行う。失敗は `Failure` の `IOFailed` か `Panicked` で受ける |
| 5 | 取り消し | 取り消しは `Wake.Cancelled` で届け、std の包みが IO の失敗 `Cancelled` として投げる。タスクは自分のフレームとスコープの後始末を通って巻き戻る。継続を捨てる強い取り消しは `exit`、致命的な誤り、run の終わり、回復のときだけにする。柔らかい確かめの名前は `check_cancelled` と `cancel_requested` である |
| 6 | `exit` | `exit` は自分のタスクだけを巻き戻し、`Control` の終わりの欄に終了コードを書いて止めるビットを立てる。ほかのタスクは巻き戻さず、ヒープを落とすときに `Lin` の資源を後始末する |
| 7 | タスクごとの上限 | タスクの中で深さ、スタック、メモリの柔らかい上限を超えると、そのタスクの IO の失敗 `ResourceLimit` になる。メモリの硬い上限は今と同じく run 全体を止める |
| 8 | 失敗の種類 | `IOErrorKind` に `Cancelled`、`TimedOut`、`ConnectionReset`、`AddrInUse`、`ResourceLimit` を足す |
| 9 | 時間 | 時間は今からの長さを `Int` のミリ秒で表す。名前を付けたスコープを期限付きで待つ関数は `Async.drain_within` である |
| 10 | I/O | std の API は OS によらず完了型にする。buffer は所有ごと渡して返してもらい、受け付け、待ち、取り出しの3段で行う。`IoDriver` の trait を `eml_rt` に置き、`eml_io` が実装する。第一の案は `compio-driver` である |
| 11 | ハンドルと worker | ハンドルは最初に受け付けた worker の I/O の driver に縛る。別の worker からの受け付けはその worker の受け箱を通して回す |
| 12 | 読みの buffer | 読みの buffer の容量はソケットが読めるようになってから確保する。IOCP ではまず0バイトの受信を出す |
| 13 | ブロックする呼び出し | ブロックする呼び出しのプールはヒープを持たない。`blocking` の行は型に `<IO, Async>` を要する。std の今の同期の行はタスクの中でその場で動き、非同期のファイルとパイプは I/O の driver で作る |
| 14 | channel | `std/Chan` は容量付きの MPMC の1つの型で、閉じた channel への送りは値を送り手に返す。複数の channel の `select` は入れない |
| 15 | 共有の状態 | 共有の状態は持ち主のタスクの後ろに `Chan` を置く形 (actor) で書く。利用者に見せる atomic な操作は `Atomic.Counter` だけである |
| 16 | 受け手の決まらない `Lin` の値 | MPMC の `Chan` と `Task` の結果で送る `Lin` の値は、途中の持ち主の ID を付けて送る |
| 17 | Rust の側が持つ語 | channel の buffer、タスクのセルの結果、スコープの失敗の欄、join の節点、プールの仕事の記録は、run の登録表に載せる |
| 18 | シグナル | シグナルの handler は `Control` のビットだけを立て、監視のスレッドが `Signal.wait` で待つタスクへ完了として届ける。待つタスクがなければ、`INTERRUPT` は今と同じく致命的で、`TERMINATE` は同じ止め方で run を止めて 143 で終わる |
| 19 | `Net` のハンドル | `TcpListener` と `TcpStream` は `Lin` の `Foreign` で、破棄処理がソケットを閉じる |
| 20 | 決定性 | `Async` の UI テストはどの seed でも同じ出力を返す |
| 21 | 操作の予算 | 待たずに済む速い道が 128 回続いたら、タスクはほかのタスクに worker を渡す |
| 22 | タスクの論理的な ID | 検査モードでは、`par` の子と同じく、タスクごとに新しい論理的な worker ID を割り当てる |

## `Async` のエフェクト

```haskell
pub effect Async where
  await_cell     : Cell -> Wake                  -- タスクかスコープのセルが終わるまで待つ
  await_io       : OpId -> (Wake, OpId)          -- I/O の driver の操作が終わるまで待つ
  await_io_until : OpId -> Int -> (Wake, OpId)   -- 同じく待ち、今から Int ミリ秒が過ぎたら操作を取り消す
  await_chan     : ChanWait -> Wake              -- channel の片側で待つ
  await_time     : Int -> Wake                   -- 今から Int ミリ秒待つ
  reschedule     : Unit -> Wake                  -- 実行待ちに戻る (`yield_now` と操作の予算)

pub data Wake = | Woke | Cancelled
```

- `Async` は extern のエフェクトにしない。extern のエフェクトは yield しうるラベルでなく ([型と Kind](../spec/types.md) の「row の具体化の記録」)、`Async` の操作は継続を取り出して待つためである
- 生の操作は eml とランタイムの間の ABI である。std の外からも見え、ランタイムの ABI として文書に書く。利用者は下の節の包みの関数を呼ぶ。`Cell`、`OpId`、`ChanWait` はランタイムの token の `Foreign` で、std の中で使う。`OpId` は `Lin` で、待つ間はランタイムが預かり、再開するときに `Wake` と組にして返す
- タスクを始める入口 `start_task : (Unit -> <IO, Panic, Async> a) -> <IO> Unit` は、生の操作ごとに節を1つ持つ handler をタスクの本体にかぶせる。節は継続 `k` をランタイムに預けて (待ちに入れるか、実行待ちに戻す) そのまま戻る。ランタイムは後で `k` に `Wake` の値を渡してタスクを再開する
- 生の操作の結果はどれも `Wake` を持つ (`Wake` そのものか、`Wake` と預けた token の組)。そのため、ランタイムはどの待ちのタスクにも即値の `Cancelled` (`await_io` と `await_io_until` では預かった token との組) を渡して再開でき、型が合う。これは std の約束で、テストで確かめる
- 待ちうる包みの関数はどれも取り消しの地点である。取り消しはここで届く (下の「取り消し」)
- 操作の予算 (128 回): 待たずに済む速い道 (`try_send`、`try_recv`、すぐに終わる完了、`cancel_requested`) は extern で、`Ctx` の予算の数を数える。128 回続くと速い道は待つ必要があると答え、包みは `reschedule` を通ってほかのタスクに worker を渡す。予算の数は worker のループがタスクを渡すときに戻す。予算は止める語に触れないので、VM の横取り (`PREEMPT`) とは別に働く。VM の横取りは CPU を使い続けるバイトコードを抑え、予算は VM でもネイティブでも速い道の連続を抑える

## スコープとタスク

```haskell
pub run          : (Unit -> <IO, Panic, Async> a) -> <IO, Panic> a
pub scope        : (Scope -> <IO, Panic, Async> a) -> <IO, Panic, Async> a
pub extern data Scope
pub spawn        : (Unit -> <IO, Panic, Async> Unit) -> <IO, Async> Unit
pub start        : (Unit -> <IO, Panic, Async> a) -> <IO, Async> Task a
pub spawn_in     : Scope -> (Unit -> <IO, Panic, Async> Unit) -> <IO, Panic, Async> Unit
pub start_in     : Scope -> (Unit -> <IO, Panic, Async> a) -> <IO, Panic, Async> Task a
pub cancel_scope : Scope -> <IO> Unit
pub fail_scope   : Scope -> Failure -> <IO> Unit                                    -- std の中で使う
pub await        : Task a -> <IO, Panic, Async> a
pub cancel       : Task a -> <IO> Unit
pub extern data Task a
pub yield_now    : Unit -> <IO, Async> Unit
```

- タスクの row は `<IO, Panic, Async>` に閉じる。`main` の row に `Async` を足したものである。閉じた row から `Panic` を除くと、タスクは `Int` の足し算も書けない。利用者のエフェクトはタスクのクロージャの中で handle し終える。`Heap h` は閉じた row で除かれる。親の `var` を捕まえたクロージャは型の誤りで ([型と Kind](../spec/types.md) の「範囲の外へ出る閉包」)、タスクの本体の中の `var` は使える
- ランタイムはタスクごとにタスクのセルを持つ。タスクのセルは状態 (実行待ち、待ち、終わり)、結果 (値、失敗、取り消しのどれか)、取り消しのビット、属するスコープ、今の持ち主の worker を持つ。スコープごとのスコープのセルは属するタスク、入れ子のスコープ、生きているタスクの数、取り消しの旗 (スコープの失敗か `cancel_scope` で立つ)、最初の失敗、閉じたかどうかを持つ
- `spawn` と `start` は動的にいちばん内側のスコープに子を加える。`spawn_in` と `start_in` は名前を付けた開いているスコープに加える。`Scope` は開いたスコープの `Unr` のハンドルである。閉じたスコープに `spawn_in` か `start_in` をすると `Panic` になる。閉じたスコープへの `cancel_scope` は何もせず、`drain_within` はすぐに戻る
- `scope f` は今のスコープの子のスコープのセルを開き、それを今のスコープにして、今のタスクの中で `f s` を動かす。その後、生きているタスクの数が 0 になるまで待つ。本体を動かすタスクはそのスコープの中にいる間だけスコープの取り消しを受ける (下の「取り消し」)。タスクは自分のスコープより長く生きない。`run` は根のスコープである
- `Task a` は `a` によらず `Lin` である。`await` はハンドルを消費し、タスクの失敗を投げ直す。`cancel` と `drop` は取り消しの要求をタスクのセルの持ち主の worker へ送り、ハンドルを放す。取り消したタスクは `Cancelled` で終わるが、スコープの失敗にはならない。終わったタスクのハンドルなら、残っている結果を記述子で落とす。`Task a` はほかの `Lin` の値と同じく `Chan` で送れる
- `main` は `use Async.run` と書くと非同期になる。エントリポイントの規則 ([モジュールと名前解決](../spec/modules.md) の「モジュール」) は変えない
- VM では、`Async.run` は呼んだ側を待たせる組み込みの上に作る。ネイティブのコードではマシンのスタックの上で worker のループを入れ子にする。呼んだ側が完了の後に再開するという約束は同じである ([ネイティブ化の設計](native.md) の「並列の実行」)。タスクの中で `run` を呼ぶと、中の根のスコープが空になるまでそのタスクを待たせる。意味は `scope` と同じで、書けるが勧めない
- ハンドルはスコープの外へ出てよい。スコープの型引数は付けない。付けると `run_heap` のようなランク2の特別な形が要るためである。外へ出た `Task a` は終わったか、失敗したか、取り消されたかのどれかで、`await` は値を返すか失敗を投げ直す

スコープの終わりの手順は std の中の eml で書く。そのため、本体が値を返しても、失敗しても、取り消されても、同じ手順が走る。

```haskell
scope f =
  let s = open_scope ()                                 -- 今のスコープの子のセルを開く extern
  let r = try_all (fn () -> f s)                        -- 本体の中断 (IO の失敗、Panic、取り消し) はここで止まる
  let r = match r with
    | Ok x -> Ok x
    | Err e ->
      fail_scope s e                                    -- 本体の失敗を記録し、子をすべて取り消す
      Err e
  wait_closed s                                         -- shield の中でスコープのセルを待ち、スコープを閉じる
  match first_failure s with                            -- 最初に記録した失敗が勝つ
    | Some e -> reraise e                               -- 戻らない呼び出しなので、r を使わなくてよい
    | None ->
      match r with
        | Ok x -> x
        | Err e -> reraise e
```

- `fail_scope s e` はタスクの底と同じことをする。`e` が取り消しでなければスコープのセルに記録し (最初の書き手が勝つ)、取り消しでも、スコープの旗を立てて子をすべて取り消す。そのため結果は `first_failure s` だけで決まり、最後の `match r` は何も記録していないときだけ意味を持つ
- `reraise : Failure -> <IO, Panic> a` は std の中の関数で、`IOFailed e` を `fail_io e.kind e.message` で、`Panicked m` を `panic m` で起こし直す
- `wait_closed` は shield した待ちである。子を待つ間に本体のタスクが取り消されても (`timeout` か `race` の下の入れ子のスコープ)、子はすでに取り消されているので、数が 0 になるまで待ち続ける。その後、タスクは `Cancelled` か最初の失敗を親へ投げ直す。親のない子も、0 にならない数も残らない
- 結果の優先: スコープで最初に記録した失敗 (本体か子かによらず、終わった順) が勝つ。取り消しは失敗として記録しない。記録がなければ本体の結果 (値か本体の取り消し) になる。2つ目からの失敗は報告せずに捨て、`RunStats` の `dropped_failures` に数える

## 失敗と `exit`

```haskell
pub data Failure = | IOFailed IOError | Panicked String
pub try_all : (Unit -> <IO, Panic | e> a) -> <IO | e> Result Failure a   -- try_io と try_panic を重ねる
```

- タスクは中断の境目である。タスクの中の handler が捕まえない IO の失敗か `Panic` はタスクの底に届き、底がタスクのセルに記録する。底はスコープの取り消しの旗を立て、スコープのタスクをすべて取り消す (下の「取り消し」)。種類 `Cancelled` の IO の失敗で終わったタスクは、タスクのセルにだけ記録し、スコープの旗も最初の失敗も変えない
- タスクの捕まえない失敗は、誰かが `await` しているかによらず、起きた時点でスコープの失敗になる。`await` は待つ側へ最初の知らせとして投げ直し、スコープは終わるときに必ず投げ直す。したがって、分離はいつもタスクの中で `try_all` を使って行い、`Result` を返す。`await` の周りで捕まえても、スコープの失敗は止まらない
- 親の `try_io` が子の失敗を捕まえるのは、`try_io` が `await` か `scope` を囲むときである
- `await` は IO の失敗を `fail_io kind message` で、`Panic` を `panic message` で投げ直す。`try_panic` はメッセージだけを見るので ([標準ライブラリ](../spec/stdlib.md) の「`try_panic`」)、これで元の失敗と区別できない
- `fail_io` は Prelude の新しい表の行 `fail_io : IOErrorKind -> String -> <IO> a` で、std の中で使う (引数は `Repr::Enum`、戻らない)。VM では新しい `Fault::Raised { kind, message }` にし、今の `Fault::Io` と同じく `io_failed` の節の入口へ回す ([VM](../spec/vm.md) の「`IO` と `Panic` への中断」)。種類は `IoReason` から写さず、そのまま渡す。`Proc.exited` もこの行を使う形にする
- `IOErrorKind` に `Cancelled`、`TimedOut`、`ConnectionReset`、`AddrInUse`、`ResourceLimit` を足す。EPIPE は `ConnectionReset` にする。どれにも当たらない誤りは今と同じく `Other` である
- Prelude に `pub data Either a b = | Left a | Right b` を足す。Kind は `a` と `b` から普通の規則で決まり、`Either FileIn Int` は `Lin` である

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

- `exit n` はどのタスクで呼んでもプログラムを終える。`exit` は今と同じく自分のタスクのスタックを底まで巻き戻し、底が `Control` の終わりの欄に `exit` と終了コードを書いて (最初の書き手が勝つ)、止めるビットを立てる。各 worker は次の安全点で自分の入口へ戻り、host のスレッドがすべての worker を待つ。実行中のプールの仕事も待つ。その後、I/O の driver を閉じ (操作を取り消して、限られた時間だけ完了を待つ)、ヒープを落とし (ファイナライザが書き出して閉じる)、Host を書き出し、`n & 255` で終わる
- ほかのタスクと待ちのタスクは巻き戻さない。その `Lin` の資源はヒープを落とすときに後始末する。巻き戻したときとの違いは利用者の `try_*` の本体が走らないことだけで、`try_*` は `exit` を捕まえないので、見える違いはない。REPL では `exit` で評価が終わり、ほかの止め方と同じく回復が走る
- 実行時エラー (ヒープの誤り、上限、`Signal.wait Interrupt` で待つタスクのない割り込み) は今と同じく run 全体を止め、1回だけ報告する ([標準ライブラリ](../spec/stdlib.md) の「実行時エラー」)

### タスクごとの上限

1つの接続が深さかメモリの上限に届くと、サーバ全体が止まる。外から止められることになるので、タスクごとの柔らかい上限を置く。

- すべての安全点と、末尾でないすべての呼び出しの位置を巻き戻しの地点にする。VM では巻き戻しの表の行がデータなので、実行時の費用はない。ネイティブのコードでは yield の地点でない末尾でない呼び出しの位置ごとに旗の確かめが1つ増える ([ネイティブ化の設計](native.md) の「タスクごとの上限」)。この費用は受け入れる
- タスクの中で深さかスタックの上限を安全点で超えたときは、その安全点でそのタスクの IO の失敗 `ResourceLimit` を起こす。`try_all` はこれを捕まえる。タスクの外では今と同じく run 全体を止める
- `Control` にメモリの柔らかい上限を置く (既定は `max_bytes` の 7/8)。タスクのセルはそのタスクが動く間に確保したバイト数を数え、確保器の遅い道で足す。柔らかい上限を超えると、監視のスレッドが、動いているか実行待ちのタスクのうち数のいちばん大きいものに旗を立て、そのタスクの次の安全点で `ResourceLimit` を起こす
- メモリの硬い上限は今と同じく run 全体を止める
- 残る危険: 待ちのタスクが持ち続けるメモリは、どのタスクにも数えない。柔らかい上限と硬い上限の間で起きる1回の大きな確保は、今と同じく致命的である。数のいちばん大きいタスクが原因のタスクとは限らない

## 取り消し

- 取り消しは2つの所から来る。1つはタスクのセルの取り消しのビットで、ハンドルの `cancel` か `drop` と、属するスコープの取り消しが立てる。ビットは一度立てたら下ろさない。もう1つはスコープの取り消しの旗で、スコープの失敗と `cancel_scope` が立てる。`scope` の本体を動かすタスクはそのスコープの中にいる間だけ、取り消しの地点でこの旗を読む (タスクの今のスコープを通した atomic の読み1つ)。`scope` が戻るか失敗を投げ直した後は、外のタスクは取り消されたままにならない
- 届け方: ランタイムは取り消したタスクの継続 `k` に `Wake.Cancelled` (I/O の待ちでは預かった token との組) を渡して、タスクを再開する。I/O の待ちでは、完了が届いてから再開する (下の「待ちを外す」)。届く所は2つで、どちらもセルの持ち主の worker の上である。待ちのタスクなら、持ち主の worker が取り消しを処理するときに下の手順で待ちを外し、`Cancelled` を付けて実行待ちに積む。取り消しの地点に来たタスクなら、待ちに入る前にビットと今いるスコープの旗を読み、どちらかが立っていれば待たずに `Cancelled` で戻す。再開は普通の起こしと同じ入口を通り、止めるビットもランタイムによる巻き戻しも使わない
- std の包みは `Cancelled` を受けると、`fail_io` で種類 `Cancelled` (メッセージ `task was cancelled`) の IO の失敗を投げる。失敗はタスクのフレームを巻き戻して `Lin` の値を放し、途中のタスク自身の `try_io` と `try_all` の本体と、スコープの終わりの手順を走らせて、タスクの底まで届く。底はタスクのセルにだけ `Cancelled` を記録し、待つ側とスコープを起こす。スコープの旗も最初の失敗も変えないので、子を1つ取り消してもスコープは失敗しない
- 取り消しは続く。一度届いた後は、`shield` の中を除いて、そのタスクのどの取り消しの地点でも投げ直す。スコープの旗から来た取り消しは、そのスコープの中にいる間だけ続く
- 待たない後始末 (数を減らす、`Chan.close`、`try_send`、`eprintln`、ハンドルを閉じる) は、`try_all` の handler の中でそのまま走る。待つ必要のある後始末 (満ちた channel で登録の actor に知らせる、別れの書き込み) は `Async.shield` で包む。`shield` の中の取り消しの地点は取り消しのビットも旗も見ない。`shield` は利用者が終わりを約束する範囲で、スコープはそれを待つ
- `finally` も、利用者が定義する破棄処理も置かない。破棄処理は eml のコードを動かさない ([線形性](../spec/linearity.md) の「破棄処理」)。`Lin` の規則と `try_all` と `shield` で、サーバの後始末 (接続の数を減らす、channel を閉じる、登録の更新) は書ける

```haskell
pub check_cancelled  : Unit -> <IO, Async> Unit     -- 要求があれば Cancelled を投げる
pub cancel_requested : Unit -> <IO> Bool            -- 取り消しのビットを返す
pub shield           : (Unit -> <IO, Panic, Async> a) -> <IO, Panic, Async> a
```

- `check_cancelled` と `cancel_requested` は取り消しの地点を通らないループのための柔らかい確かめである
- 取り消しは協調的で、CPU を使い続けるコードを止めない。純粋なループの中のタスクは次の取り消しの地点で取り消される。`compute` の周りの `timeout` は `par` の仕事が終わったときに戻る (`par` の取り消しは後で足す。[マルチコア対応の設計](multicore.md) の「未決の論点と将来の拡張」)。サーバでは CPU を多く使う仕事を `compute` に入れる。`compute` はタスクを待たせるので、worker はほかの接続を受け続ける。待たずに `compute` も使わないネイティブのタスクは、終わるまで worker を持ち続ける。そのときは、監視のスレッドが詰まりを見つけた後で、空いた worker が同じ worker のほかのタスクを救済する

### 待ちを外す

取り消しで再開する前に、ランタイムは待ちの理由ごとに待ちを外す。外すのは、取り消しを処理するときのタスクのセルの持ち主の worker で、その受け箱の lock の下で行う。救済で持ち主が変わると、外す務めも新しい持ち主へ移る。

- セルの待ち: 待つ相手のセルの lock の下で、待つ側の登録を消す
- 時間の待ち: 持ち主の worker の時間の輪から登録を消す
- channel の待ち: channel の lock の下で、待つ側の登録を消した印に替える。消した登録には品物を渡さない。待ち合わせの `send` の値は送り手のフレームに残り、巻き戻しが落とす。これは後始末で、漏れではない
- I/O の操作の待ち: タスクを取り消し中の状態にし、ハンドルを縛った worker の I/O の driver に操作の取り消しを頼む (`CancelIoEx`、io_uring の `ASYNC_CANCEL`、epoll と kqueue では登録を外す)。完了 (取り消した状態か、競合したときはデータ) が届いてから `Cancelled` で再開する。取り出しは buffer とハンドルを返し、巻き戻しがそれを落とす。カーネルが書きうる間は buffer を放さない。取り消し中のタスクは完了の前に再開しない
- プールの仕事の待ち: 仕事は止められないので、I/O の操作と同じく完了を待ってから再開する (下の「I/O の driver」の表)

セルの待ちと channel の待ちでは、外すことと、すでに出た起こしが競合しうる。規則は次のとおりである。外すのが先なら `Cancelled` で再開する。起こしがすでに出ていれば外すのは負け、`Woke` で再開して品物か結果を受け取る。続く取り消しが次の取り消しの地点で投げる。両方が起きることも、どちらも起きないこともない。I/O の操作とプールの仕事の待ちには、この競合はない。データが競合して届いても、完了の後でいつも `Cancelled` で再開する。

### スコープの取り消し

- 失敗を記録するタスクの底は、スコープのセルの取り消しの旗を立て、最初の書き手なら失敗を残す。続けて、スコープに属するタスクごとに取り消しのビットを立てて持ち主の worker の受け箱へ取り消しを送り、入れ子のスコープにも同じことをする。`cancel_scope` も同じことをする
- 旗を立てた後に `spawn_in` で加えたタスクは、作るときに取り消す
- 費用は属するタスクの数に比例する。worker をまたぐのは受け箱を通る知らせだけで、ヒープの語は渡らない。「失敗した時点で」は相手の worker の受け箱の遅れの分だけ遅れる
- 本体のタスクはビットを立てずに旗で取り消す。本体がそのスコープの中で待っていれば、持ち主の worker へ知らせを送り、待ちを外して `Cancelled` で再開する。I/O の driver の操作 (`accept` など) で待っていれば、その操作を取り消してから再開する。そのため、接続のタスクが失敗した、待つだけのサーバもすぐに止まる。接続のタスクを `try_all` で包めば止まらない

### 強い取り消し

- 継続を捨てる強い取り消しは `exit`、致命的な誤り、run の終わりに待ちのまま残ったタスク、回復のときだけにする。そのときは run を止める手順で全 worker が止まり、ヒープを落とす。受け箱、待ちの集まり、I/O の driver の表にある継続の語はヒープとともに放され、ファイナライザがファイルとソケットを閉じる
- I/O の driver の操作の `Lin` の token の破棄処理は、その操作を持ち主のないものとして記録する。driver は完了のとき、または driver を閉じるとき (取り消して、限られた時間だけ完了を待った後) に buffer を放す

### 却下した案

- 強い取り消しを主な仕組みにする案: スコープの終わりの手順と、`Lin` でない後始末をすべて飛ばす
- `Async` の `never` の操作で取り消す案: 動くが、利用者が扱えない2つ目の捕まえ方になる
- 利用者が定義する破棄処理か `finally`: 破棄処理で eml のコードを動かさない規則に反する。`try_all` と `shield` で足りる

## 時間

```haskell
pub sleep_ms     : Int -> <IO, Async> Unit
pub timeout      : Int -> (Unit -> <IO, Panic, Async> a) -> <IO, Panic, Async> Option a   -- None: 本体を取り消した
pub race         : (Unit -> <IO, Panic, Async> a) -> (Unit -> <IO, Panic, Async> b)
                 -> <IO, Panic, Async> Either a b                                        -- 負けた側を取り消す
pub drain_within : Int -> Scope -> <IO, Async> Unit
```

- 時間は今からの長さを表す `Int` のミリ秒である。`sleep_ms`、`timeout`、`_within` の関数、生の操作の `await_time` と `await_io_until` はどれもこれを受ける。時刻を扱う `Time` のモジュールは後で足す
- `timeout ms f` は `f` をタスクとして動かし、期限が来たら取り消して `None` を返す。`race f g` は先に終わった側の値を返し、もう一方を取り消す
- `drain_within ms s` は名前を付けたスコープ `s` のタスクが終わるのを最大 `ms` ミリ秒待ち、残っていれば属するタスクを取り消す。`cancel_scope` と違い、呼んだタスクは取り消さない。サーバを穏やかに止めるときに使う (下の「ネットワークとシグナル」)
- 時間の待ちは worker ごとの I/O の driver の時間の輪に登録する。テストでは仮想の時計を使う (下の「テストと決定性」)
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
pub merge    : List (Chan a) -> <IO, Async> Chan a          -- 元ごとに転送のタスクを1つ、いちばん内側のスコープに作る
pub data SendResult a    = | Sent | Rejected a          -- Rejected: 閉じていた。値が戻る
pub data TrySendResult a = | Accepted | Full a | Refused a
pub data TryRecvResult a = | Got a | Empty | Drained
```

- コンストラクタの名前はモジュールの中で別々にする (同じモジュールの `data` の間で名前を使い回すと E1003 になる)
- `Chan` は Go の形で、閉じるのは明示した `close` で行う。`Unr` のハンドルは「送り手がすべて落ちた」を知らせられないためである。閉じた channel への `send` は `Rejected` で値を送り手に返すので、API が `Lin` の値を黙って消費することはない
- 中身の Kind は問わない。`Unr` の値は送るときに印を付け、受けるときに移し替えの歩きで受ける ([マルチコア対応の設計](multicore.md) の「共有の印方式」)。送り手が持ち続ける値は共有のままで、reuse しない。`Lin` の値は移す ([マルチコア対応の設計](multicore.md) の「`Lin` の物体の所有者の移し替え」)
- 受け手の決まらない `Lin` の値 (MPMC の `Chan` と `Task` の結果) は、途中の持ち主の ID を付けて送る。送り手は受け手を予約した途中の ID にして移し替えをする。`Lin` の根から rc がすべて 1 の道で届く物体だけが途中の持ち主になり、最初の rc > 1 の物体から下は印付け (rc < 0) に切り替える。受け手の歩きは、rc > 0 で持ち主が途中の ID の物体を受け取って (持ち主を受け手にして) 下り、rc == -1 の物体は今までどおり裏返し、ほかのものでは止まる。持ち主の ID は検査モードにだけある ([マルチコア対応の設計](multicore.md) の「論理的な worker ID」)。速いモードでも送り手の印付けは行い、受け手の裏返しの歩きは変わらない
- 検査モードでは、`par` の子と同じく、タスクごとに新しい論理的な worker ID を割り当てる。run 全体の生きている数の表はタスクとともに大きくなる。速いモードには影響しない
- channel の最後のハンドルを落とすと、キューに残った品物を記述子で落とす。落とした継続が捕まえた `Lin` の値と同じく、これは後始末で、飛ばした後始末ではない
- channel の物体は数えた参照を持つ Rust の構造で、run の登録表に載る。漏れの検査と回復は登録表をたどる。キューはまず mutex の MPMC にし、ロックフリーのキューには後でランタイムの中だけで替える。送り手は直接積み、待っている側はその worker の受け箱を通して起こす
- 容量付きなので、送り手は満ちたときに待つ (back-pressure)。上限のない channel は大きな容量で作る
- `send` と `recv` は取り消しの地点である。速い道 (`try_*` と、`send` と `recv` の中の速い道) は extern で、操作の予算を数える
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
      fail_io Cancelled "task was cancelled"    -- 戻らない呼び出し
```

### 所有ごと渡す buffer

- buffer は値で渡す `Bytes` である。受け付けは語を受け取り、一意でなければ (rc ≠ 1。共有の印の付いた rc < 0 の物体を含む) 新しい一意な物体に写す。カーネルが書くのは、参照がちょうど1つの物体だけで、その参照は I/O の driver の保留の表が持つ
- 保留の表は worker の登録した根で、語は長さを進めて戻る
- 読みでは、受け付けは buffer を預かるだけで、容量を足すのはソケットが読めるようになってからにする。kqueue と epoll では読めると分かってから容量を `length + 64 KiB` にし (容量が足りなければ一意な物体を確保し直す)、`(payload + length, 64 KiB)` をカーネルに渡す。IOCP ではまず0バイトの受信を出し、それが終わってから容量を足して受信する。そのため、何も来ない接続は読みの buffer を持たない
- `write_bytes` も同じく buffer を移し (一意でなければ写す)、完了の後で放す。buffer は返さない
- Perceus の reuse には頼らない。上の `read_chunk` を繰り返すと、1回ごとに 64 KiB の物体を1つ確保する (`Bytes.empty ()` の容量は 0 で、extern が増やす)

### `IoDriver` と `eml_io`

- `IoDriver` の trait は `eml_rt` の中で `Host` の隣に置く。語の単位の trait で `external_body` にし、信頼の一覧では Host と同じ T11 の行に属する。`Ctx` を通して届く
- 新しい crate `eml_io` が実装する。本物の driver は worker ごとに1つで、その worker の待ちと起こしを受け持つ。テストの driver は仮想の時計、メモリの上のストリーム、seed で決まる完了の順を持つ。どちらも同じ trait の後ろにあり、`eml_test_support` が選ぶ。`Session::run` が実行系を組み立てるときに driver を渡す
- driver のどの待ちも worker を起こす知らせと監視のスレッドの起こしで戻る
- 本物の driver の第一の案は `compio-driver` と `compio-buf` である。IOCP、io_uring、kqueue と epoll (`polling`)、所有ごと渡す buffer を持つ。使えないときの代わりの案は下の「未決の論点と将来の拡張」にある
- 完了は持ち主の worker の受け箱に届く。完了の中身は保留の表が持つ数えた参照で、ハンドルを縛った worker とタスクの worker が違えば、普通の移し替えの歩きでタスクへ渡る
- REPL の回復は実行中の操作を取り消して完了を待ちきってから写す

### ハンドルと worker

- 利用者から見える規則はない。ランタイムはハンドルを最初に受け付けた worker の I/O の driver にそのハンドルを縛る。`accept` が返したソケットは縛らずに作り、子のタスクが最初に読む worker に縛る。そのため、`spawn` から最初の操作までの間にタスクが動いても費用はない
- 救済でタスクが別の worker に動いた後の受け付けは、縛った worker の受け箱を通して回し、完了はタスクのセルの持ち主へ届ける。これは正しさのために要る
- 救済は回した受け付けを処理しない。縛った worker の driver が要るためである。詰まった worker へ回した受け付けは、その worker が戻るまで待つ。これは遅れで、正しさの問題ではない
- 1つの listener の accept は1つのタスクで行う。`SO_REUSEPORT` で複数のタスクが accept する形は後で足す

### その場で動く同期の行

- extern は待てず、row に `Async` のない行は yield できない。そのため、std の今の同期の行 (`FileIn`、`Stdin`、`Proc`) はタスクの中でその場で動き、その worker を止める。その間、同じ worker のほかのタスクは、監視のスレッドが詰まりを見つけた後で救済される
- サーバがときどきファイルを読むにはこれで足りるが、パイプを汲み続けるには向かない。非同期のファイルとパイプは I/O の driver で作る (下の「ファイルとパイプ」)
- 利用者の `extern "C"` の `blocking` の行は、型に `<IO, Async>` を要し、3段に下ろす。受け付けは C の関数を包んだ仕事をプールに載せ、待ち、取り出しが結果を返す。`Async` のない行に `blocking` を書くと検査の誤りになる ([FFI と拡張の設計](ffi.md) の「長く走る呼び出し」)。std の同期の行には `blocking` を付けない

### ブロックする呼び出しのプール

- プールはヒープを持たない。プールのスレッドは RC に触れず、確保もしない
- 仕事の記録が渡した語を数えた参照で持つ。`Unr` の引数には印を付けない。`Mut*Array` の引数は記録へ移す。記録は run の登録表に載り、仕事が終わるまで run が生かしておく。プールの側は記録を通してだけ語に届く
- プールは入力の中身を読み、移した `Mut*Array` の中身を書き、スカラー、ハンドル、移した語、Rust のデータを返す。eml の物体は、取り出しが持ち主の worker の上で作る
- 完了は持ち主の worker の受け箱の mutex を通して届く。これがプールの書き込みと持ち主の読みの順を決める
- DNS の名前の解決 (`getaddrinfo`) も、`connect` の中で同じ3段のプールの仕事にする
- 戻りの `Mut*Array` と `Lin` の物体は、取り出しが移し替えで受ける ([マルチコア対応の設計](multicore.md) の「`Lin` の物体の所有者の移し替え」)
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
- 誤りは IO の失敗で、今の種類に `Cancelled`、`TimedOut`、`ConnectionReset`、`AddrInUse` を足す
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

`SIGTERM` が来ると `race` が accept するタスクを取り消し、`drain_within` が実行中の接続を最大 30 秒待ってから取り消す。接続のスコープはその後で終わりの手順を通る。`session` を `try_all` で包まないと、1つの接続の失敗がスコープの失敗になり、サーバが止まる。

### シグナル

- `std/Signal` は `data Signal = | Interrupt | Terminate` と `wait : Signal -> <IO, Async> Unit` を持つ。`Terminate` は `SIGTERM` で、Windows では `CTRL_CLOSE_EVENT` と `CTRL_SHUTDOWN_EVENT` である
- シグナルの handler は async-signal-safe のまま、`Control` のビットだけを立てる (Ctrl-C は `INTERRUPT`、`SIGTERM` は新しい `TERMINATE`)。Windows ではコンソールの handler が同じビットを立てる。ビットが立ったのを見た監視のスレッドは、そのシグナルを `Signal.wait` で待つタスクが登録されていればビットを下ろし、待つタスクのセルの持ち主へ完了として届ける。登録されていなければ、致命的な止め方として各 worker へ写す。`INTERRUPT` では今と同じである ([VM](../spec/vm.md) の「上限と割り込み」)。`TERMINATE` も同じ止め方で run を止め、終了コードは 143 にする。シェルが SIGTERM で終わった子に返す値で、SIGINT の 130 と同じ考え方である
- `signalfd` と `EVFILT_SIGNAL` は使わない。どちらもすべてのスレッドでシグナルを止める必要があり、今の handler とブロックする読みの EINTR の道と食い違う
- 2回目の Ctrl-C は今と同じく `_exit(130)` である。handler がビットを立ててから監視のスレッドが下ろすまでの間 (高々監視の刻み1つ) の2回目の Ctrl-C も終わる。これは文書にある2回の Ctrl-C の意味と同じである

### 後で足すもの

- UDP、`SO_REUSEPORT` で複数のタスクが accept する形
- 非同期の名前の解決。今は `getaddrinfo` をプールで動かす。`resolve` も同じ形で持つ
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

- N = 1 で seed で決まるスケジュール (`--sched seeded:<n>`) を `tests/ui/run/async/` の UI テストの既定にする。安全点の数による横取り、worker の実行待ちの中からの seed による選択、seed で完了の順を決めるテストの driver を使う。`Async` の UI テストはどの seed でも同じ出力を返さなければならない。そのため、ネイティブのコードでも同じになる
- テストの driver は仮想の時計を持つ。`sleep_ms` と期限はすべてのタスクが待ちになったときだけ仮想の時間を進める。そのため、期限と取り消しのテストはすぐに終わり、マシンの速さによらない (1つのテストを1秒以内に終える規則)。テストの driver はメモリの上の `TcpStream` の組 (仮想のホストの上の `listen` と `connect`) と、テストの hook で起こす仮想のシグナルも持つ
- 本物のソケットのテストは `tests/ui/run/net/` に少しだけ置く。ループバックでポート 0 に bind して `local_port` で読み、期限を使わない (期限は仮想の時計の上だけで試す)。接続は一度に1つにするか、順によらない出力にする。N = 1 で流し、複数の worker の組 (N が 2 と 8) では順によらないものだけを流す。Windows の CI は同じ組を IOCP の上で流す
- 意味のテスト: 取り消したタスクの `FileIn` と `TcpStream` が閉じること (検査モードの漏れの検査で確かめる)、失敗した子がほかのタスクと `accept` で待つタスクをすぐに取り消すこと、`try_all` で `Panic` を分けたサーバが動き続けること、`timeout` の下の入れ子のスコープが親のない子を残さないこと、`shield` の中の別れの `send` が取り消しの下でも終わること、続く取り消しが2つ目の取り消しの地点で投げること、テストの driver で取り消した読みが投げる前に buffer を返すこと、`read_chunk_within` が使えるストリームとともに `(None, s)` を返すこと、取り消した待ち合わせの `send` の値が後始末されて受け手が起きないこと、操作の予算が速い道の 128 回で切り替えること、子の `exit` がその終了コードで run を終えること、`Signal.wait Interrupt` が試しの Ctrl-C を取り消しに変えること
- 救済を強いるスケジュール (`--sched rescue-always`) で、検査モードの漏れと所有者の検査を流す
- `RunStats` に `tasks`、`parks`、`cancels`、`channel_ops`、`completions`、`dropped_failures` (報告せずに捨てた2つ目からの失敗) と、共有にした物体の数を足す。スナップショットは逐次のスケジュール (`--sched sequential`) で固定する ([VM](../spec/vm.md) の「`RunStats`」)

## 未決の論点と将来の拡張

- I/O の driver の crate: 既定は `compio-driver` である。単独で使えること、worker ごとに driver を持てること、別のスレッドから起こせる待ちを持つことは、まだ確かめていない。本物の driver を作る前の試作で決める。使えなければ、`polling` と手で書く IOCP (`windows-sys`) で自前の driver を作る。API は変わらない。mio は Windows で AFD の readiness を真似るので採らない
- 受け付けをスレッドをまたいで安全に出せる backend (IOCP、SQ を lock する io_uring) では、受け箱を通す回り道を省いてよい。既定は backend ごとに `eml_io` が決める
- `write_bytes` が buffer を返して使い回す形: 既定は返さない。計測で確保が目立ったときに考える
- 2つ目からの失敗をまとめて報告する形 (MultiError): 既定は最初の失敗が勝ち、ほかは数えるだけにする
- 複数の channel の `select`: 既定は入れない。`race`、`timeout`、`merge` で書く
- broadcast と watch の channel: 既定は作らない
- `Counter` のほかの `Atomic`: 既定は作らない
- タスクの中の入れ子の `Async.run`: 既定は許し、呼んだタスクを待たせる
- タスクを置く worker を選ぶ API (`spawn_here` など): 既定は作らない。スレッドに縛られた仕事は `Main.run` で書く ([FFI と拡張の設計](ffi.md) の「GUI」)
- channel のキュー: 既定は mutex で、ロックフリーには後でランタイムの中だけで替える
- `Time` のモジュール、UDP、`SO_REUSEPORT`、TLS のプラグイン、`Http`、非同期の名前の解決: 上の「ネットワークとシグナル」の「後で足すもの」にある
