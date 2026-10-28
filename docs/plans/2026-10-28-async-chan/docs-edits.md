# async-chan: 直す文書の一覧

計画の本文 ([../2026-10-28-async-chan.md](../2026-10-28-async-chan.md)) の付録である。項目の頭の `(T-n)` は、その箇所を直す作業である。規則を入れるコミットで、その規則の `spec/` の節を書く。「」の中の文は下書きで、入れるときに yomiyasu を当て、前後の文と言い方をそろえる。意味 (規則、数、条件) は変えない。見出しの名前は引用のテスト (`citations::every_cited_heading_exists`) が確かめるので、ここに書いた字のまま入れる。

## docs/spec/stdlib.md

1. (T-3、T-6 で足す) 新しい節 `## \`Std.Chan\``。`### \`Failure\` と \`try_all\`` の後、`## 実行時エラー` の前に置く。
   - 導入: 「`Std.Chan` はタスクの間で値を受け渡す channel のモジュールである。1つの型で、送り手と受け手をいくつでも持てる」。例は `files/tests/ui/run/async/chan_rendezvous.em` の `body` を短くしたもの。
   - API の表 (`| 名前 | 型 | 実装 |`、どの行も `eml`): `new : Int -> <IO> Chan a`、`send : a -> Chan a -> <IO, Async> SendResult a`、`recv : Chan a -> <IO, Async> Option a`、`try_send : a -> Chan a -> <IO> TrySendResult a`、`try_recv : Chan a -> <IO> TryRecvResult a`、`close : Chan a -> <IO> Unit`、`merge : List (Chan a) -> <IO, Async> Chan a` (T-6)。3つの型 (`Sent`/`Rejected a`、`Accepted`/`Full a`/`Refused a`、`Got a`/`Empty`/`Drained`。どれも `deriving (Eq, Show)`)。
   - 箇条 (この順):
     - 「`Chan a` は登録表の添字を持つ `Unr` の即値で、Kind は `a` によらず `Unr` である。channel は `close` で放す資源で、閉じて空になった時点でその場で放す。閉じないままの channel と、閉じても品物の残った channel は、run の終わりに放し、残った品物をそのとき落とす。放した channel と前の run の channel の値は、閉じて空の channel として振る舞う (`Scope` と同じ)。1回だけの受け渡しには `Task a` と `await` を使う。`Task a` は `Lin` で、捨てれば後始末が走るためである」
     - 「`new n` は容量 `n` の channel を作る。0 は待ち合わせで、負の数は 0 として扱う。積めるのは、積んだ品物の数が、容量と待っている受け手の数の和より少ないときである。そのため、容量 0 でも受け手が待っていれば1つ積める」
     - 「`send x c` は、積めれば積んで `Sent` を返し、積めなければ枠が空くか閉じるまで待つ。閉じていれば `x` を `Rejected x` で返す。`recv c` は、品物があれば取って `Some` で返し、なければ品物が来るか閉じるまで待つ。閉じて空なら `None` を返す」
     - 「`try_send` と `try_recv` は待たない。`try_send` は、積めれば `Accepted`、満ちていれば `Full x`、閉じていれば `Refused x` を返す。`try_recv` は、品物があれば `Got x`、空なら `Empty`、閉じて空なら `Drained` を返す。どちらも row に `Async` を持たないので、タスクの外でも呼べる」
     - 「`close c` は何度呼んでもよく、2回目からは何もしない。待っている受け手と送り手をすべて起こす (同じ channel で両方が待つことはない)。積んであった品物は閉じた後も順に取れ、取り切ると `recv` は `None` を返す。起こされた送り手は `Rejected` を受け取る」
     - 「待ち手は側ごとに FIFO で起こす。起こされたタスクより先に、速い道のタスクが品物や枠を取ることがあり、そのときは列の後ろで待ち直す。約束する順は起こす順だけである」
     - 「`send` と `recv` は取り消しの地点である (下の「取り消し」)。取り消しの巻き戻しは `close` を飛ばす。取り消されうるタスクで作った channel は、`try_all` の handler か `shield` の中で `close` する」
     - 「品物の値は、積むときに送り手が印を付け、取るときに受け手が受け取る ([オブジェクトモデル](object-model.md) の「論理的な worker ID」)。断られた値と、待っている間に取り消された送り手の値は channel に入らない。送り手の値のまま戻るか、巻き戻しが落とす」
     - 「待っているタスクのほかに進むものがなくなると、実行時エラー `deadlock` で止まる ([VM](vm.md) の「時計と driver」)」
     - (T-6) 「`merge srcs` は容量 0 の出力を作り、入力ごとに転送のタスクを1つと、終わりを集めて出力を閉じるタスクを1つ、呼んだタスクの今のスコープに `spawn` する。空の並びなら閉じた出力を返し、タスクを作らない。出力の順はスケジュールで決まる。入力がすべて閉じて空になると出力を閉じる。`merge` の結果をそのスコープの外へ出すと、スコープが閉じるときに転送は取り消される。出力を先に閉じると、転送のタスクは次に受け取った品物を落として終わる。閉じない入力を待つ転送は、入力が閉じるかスコープが取り消されるまで待つ。転送の終わりは容量 0 の `Chan Unit` で集め、終わった後にその channel も閉じるので、`merge` は run の終わりまで残る資源を作らない」
     - 「REPL の入力をまたいだ channel は、[モジュールと名前解決](modules.md) の「REPL のセッション」にある」
   - ABI の部分: 「`Std.Chan` の extern の行は次の7行で、どれも `pub` でない。`chan_cell` のほかの6行は `Async` の命令で実行する」。表 `| 行 | 型 | 預かる引数 |` に7行 (`push_raw` は「品物 (1つ目の引数)」、ほかは「なし」)。続けて:
     - 「`reserve_send` と `reserve_recv` は、0 (確定してよい)、1 (待つ)、2 (閉じている。受け取りでは閉じて空)、3 (先に譲る。2つ目の引数が真のときだけ) を返す」
     - 「`push_raw` と `pop_raw` は、0 を返した `reserve_*` の直後に、`Int` を比べるほかは呼び出しを挟まずに呼ぶ。標準ライブラリがこの形を保ち、処理系は確かめない」
     - 「待ちは `Std.Async` の `chan_wait_send` と `chan_wait_recv` (`Cell -> <IO, Async> Wake`) を通す」
     - コードの塊: `files/std/Chan.em` の `send`、`recv`、`send_loop`、`recv_loop` の定義。
2. (T-5) 新しい節 `## \`Std.Atomic\``。`Std.Chan` の直後。表: `new : Int -> <IO> Counter`、`add : Int -> Counter -> <IO> Int`、`load : Counter -> <IO> Int`、`store : Int -> Counter -> <IO> Unit` (どれも extern)。箇条:
   - 「`Counter` は、run をまたいで `Machine` が持つ表の添字を持つ `Unr` の即値である。`add n c` は足した後の値を返し、64 ビットで巻き戻る。`Counter` は放さない。数の少ない共有の状態のためのものである。worker のスレッドを入れるときは、表を `SeqCst` の atomic にする」
   - 「行は `Async` の命令で動くので、`Std.Atomic` は `Std.Async` を読み込む。row は `IO` だけで、タスクの外でも、REPL の入力をまたいでも使え、実行時エラーで捨てた run の `add` も残る」
3. (T-3) 「`Std.Async`」:
   - 意味の箇条の「`Async` は抽象エフェクトで、操作 `await_cell`、`reschedule`、`await_time`、`await_drain` は…」に `await_send`、`await_recv` を足し、「channel の待ちは `chan_wait_send` と `chan_wait_recv` が包む (下の「`Std.Chan`」)」を足す。
   - ABI のコードの塊に、2つの操作を足す。
   - 「生の操作は4つで」を「生の操作は6つで」にし、「`await_send c` は channel `c` に枠が空くか閉じるまで、`await_recv c` は品物が来るか閉じるまで待つ」を足す。
   - 入口の箇条に「`await_send` の節は継続を `park_send` に、`await_recv` の節は継続を `park_recv` に」を足す。
   - 「extern の行は次の27行である」を29行にし、表に `park_send` と `park_recv` (`Cell -> (once Wake -> <IO> Unit) -> <IO> Unit`、預かる引数は「継続 (2つ目の引数)」) を足す。
   - 閉じた row の箇条で、`park_drain` の後に `park_send`、`park_recv` を足す。
   - 新しい箇条: 「`chan_wait_send` と `chan_wait_recv` は `Std.Chan` のための口で、`pub` である。`Async` の操作は外のモジュールから perform できないためである ([エフェクトと handler](effects.md) の「抽象エフェクト」)。`Cell` を返すか渡す公開の関数はないので、利用者は呼べない。`Cell` を公開の関数に出すときは、この2つを見直す。channel でないセルか放したセルを渡すと、すぐ `Woke` で戻る」
4. (T-3) 「extern の境界」: 「預かる行は `Std.Async` の … `park_drain`、`park_send`、`park_recv` の15行と、`Std.Chan` の `push_raw` の1行で、`task_cell` だけは借りて預かる」。
5. (T-3) 「取り消し」: 取り消しの地点の一覧を「`await`、`yield_now`、`sleep_ms`、`drain_within`、`check_cancelled` と、`Std.Chan` の `send` と `recv`」にする。新しい箇条: 「`send` と `recv` は、待つ前と待っている間に取り消しが立てば投げる。起こされた後は、`await` と同じく、まず操作をやり直し (枠か品物があれば送るか受け取る)、次の取り消しの地点が投げる。やり直しで待ち直すときは、待つ前の規則で投げる」。
6. (T-3) 「操作の予算」: 「予算を使う速い道は `check_cancelled` だけで、…」を次にする。「予算を使う速い道は、`check_cancelled` と、`Std.Chan` の `send`、`recv`、`try_send`、`try_recv` の速い道である。`try_send` と `try_recv` の row は `Async` を持たないので、予算を使うだけで譲らず、次に予算を見る `send`、`recv`、`check_cancelled` が譲る。待ちから起こされた直後のやり直しは、予算を使っても譲らない。譲った後のやり直しは予算をもう1つ使う。`cancel_requested` は予算を使わない」。

## docs/spec/vm.md

7. 「セルと登録表」:
   - (T-1) 「受け手の決まらない値の預かり先の操作は `Depot` にまとめる。`open` (`fork_worker`)、`publish` (`share`)、`receive` (`transfer_back`)、`discard` (預かる ID の下の `decref`)、`close` (`retire_worker`) で、タスクの結果と channel の品物が同じ道を通る」
   - (T-3) 「登録表は channel のセルも持つ。channel のセルは、容量、閉じたか、積んだ品物 (数えた参照の語)、待っている送り手と受け手の列 (FIFO)、品物の預かり先 (`Depot`) を持つ。待ち手の列の項はいつも生きていて、`wait` がその channel のセルを指すタスクである。閉じた後は、両方の列がいつも空である」
   - (T-5) 「`Counter` の表 `counters` は `Machine` が持ち、run のたびに `Scheduler` の登録表へ渡して受け取る (generation の数と同じ道)。回復でも戻さない」
8. 「命令」:
   - (T-1) `finish_ok`: 「`finish_ok x` は、先に x を今のタスクの ID に預ける (`share(x, 今のタスクの ID)`)。受け手が今の ID なので所有は動かず、`Unr` の物体に印が付くだけである。タスクの ID が結果の預かる ID になる ([オブジェクトモデル](object-model.md) の「論理的な worker ID」)」
   - (T-3) `park_send c k` と `park_recv c k` の箇条 (`park` と同じく、止めて `parks` と `channel_waits` を数え、取り消しの規則を確かめ、すぐ戻れるなら `Woke` で戻り、待ち手の列に登録する。`park_recv` は登録の後、積めるなら送り手を1つ起こす)。「`park_send` と `park_recv` は、`c` が生きた channel のセルでなければ (放したセル、前の run の値、タスクかスコープのセル)、閉じて空の channel と同じく、すぐ `Woke` で再開する」
   - (T-3) `new_raw`、`reserve_send`、`reserve_recv`、`push_raw`、`pop_raw`、`close_raw` の箇条 (計画の D-3、D-4、D-20)。(T-5) `Atomic` の4行の箇条。
   - (T-3) 「`chan_cell` は `Async` の命令でなく extern の命令で、即値をそのまま写す」
   - (T-3) 「channel と `Counter` の行は、`park_send` と `park_recv` のほかは今のタスクのセルを使わないので、タスクの外でも動く (予算は使わない)」
   - (T-3) 抽象の操作の箇条: 操作を6つにし、節が `park_send`、`park_recv` へ行くことを足す。
9. (T-3) 新しい節 `### channel`。`### 時間の待ち` の後に置く。中身は計画の D-3 と D-4: 積めるかの式と待ち合わせの規則、起こすこと (待ち手を列から外し、`wait` を空にし、`Woke` で再開を積む)、`park_*`、`unpark`、`close_raw`、`settle`、不変条件 W、経路の表、預かり先の呼び出しの表 (D-1)。`close_raw` に「同じ channel で受け手と送り手が同時に待つことはないので、起こす順は列ごとの FIFO だけで決まる」を足す。
10. (T-3) 「終わり」: 「`sweep_all` は channel のセルも放す。残った品物を channel の預かり先の下で落とし、ID を退かせ、`channels_swept` を1つ数える」。(T-1) タスクのセルの文を `Depot::discard` と `Depot::close` の言い方にする。
11. 「`RunStats`」:
    - (T-3) 「欄は次の19で」を22にし、3つの箇条を足す。
      - 「`channel_ops`: `Std.Chan` の速い道が品物を積んだ回数と取った回数の和。登録表の `push_raw` と `pop_raw` が数える」
      - 「`channel_waits`: `send` と `recv` が継続を `park_send`、`park_recv` に渡した回数。すぐ再開する待ちも数え、`parks` にも数える」
      - 「`channels_swept`: run の終わりの `sweep_all` が放した channel の数。閉じなかった channel と、閉じても品物の残った channel を数える。閉じて空になってその場で放した channel は数えない」
    - (T-3) `parks` の箇条に `park_send`、`park_recv` を足す。
    - (T-1) `shared_objects` の箇条: 「…印を付けるときと、タスクの結果を `finish_ok` が預けるときに増える」。(T-3) 「…と、channel の `push_raw` が品物に印を付けるとき」を足す。
    - (T-3) スケジュールによらない欄の段落を次にする。「取り消しが届かず、channel の操作も待ちもない run では、`tasks`、`parks`、`timers`、`dropped_failures`、`shared_objects`、`channels_swept` の欄はスケジュールの方針によらない。channel を使う run では、待つかどうかと、送り手が参照を持ち続けた品物の裏返しが順で変わり、仕事の回数と `parks`、`shared_objects` がそれに従う。そうした run でもスケジュールによらないのは、`tasks`、`timers`、`dropped_failures`、`channels_swept` である。`tasks` と `timers` はプログラムが決め、`dropped_failures` は最初の失敗がどれかによらず、`channels_swept` は run の終わりに閉じていないか品物の残った channel の数で、どれもタスクの順で変わらない」
12. (T-3) 「並列化への備え」: 「言語のキュー (channel など)」の文の後に、計画の D-9 の文を足す。
13. 行の数 (vm.md:239 と、ほかに「`Std.Async` の27行」と書いた所): T-3 で「`Std.Async` の29行と `Std.Chan` の6行 (`chan_cell` を除く)」、T-5 で「…と `Std.Atomic` の4行」にする。

## docs/spec/object-model.md

14. (T-1 でタスクの分、T-3 で channel の分を足す)
    - 「`share`」の最後の箇条: 「VM が `share` を呼ぶのは、`par` の fork、タスクの始め (閉包と根を新しいタスクの ID へ)、タスクの結果を預けるとき (`finish_ok`。受け手は今のタスクの ID で、`Unr` の物体に印を付けるだけである) である」。T-3 で「、channel の送り (受け手は channel のセルの ID)」と、[VM](vm.md) の「channel」への引用を足す。
    - 「`transfer_back`」の最初の箇条: 「`transfer_back(w, from)` は、ID `from` に預けた値 w を、今の worker の局所の値に戻す。`from` は、`par` の join では右の子の ID、タスクの始めでは始めたタスクの ID、タスクの結果では終わったタスクの ID である。値を受け取った後、`retire_worker(from)` の前に呼ぶ (下の「論理的な worker ID」)」。T-3 で「、channel の受け取りでは channel のセルの ID」を足す。
    - 「`transfer_back`」の最後の箇条: 「VM が `transfer_back` を呼ぶのは、`par` の join (右の子の値の語ごと)、タスクの始め (閉包)、タスクの結果の受け取りである」。T-3 で「、channel の受け取り」を足す。
    - 「論理的な worker ID」の `par` の文の後: 「受け手の決まらない値は、値を預かる構造の ID に預ける。タスクの結果はそのタスクの ID である。送り手は `share(値, 預かる ID)` で預け、受け手は `transfer_back(値, 預かる ID)` で受け取る。預けた値は、受け取るか落とすまで預かる ID の持ち物なので、構造が値を手放す道はどれも落とす道を通る。預かる ID の物体が残っていれば、`retire_worker` が `TransferLeak` を返す。誤りと `exit` で捨てた run の ID は、回復が新しいヒープを作るときに表ごと消える」。T-3 で「channel の品物は channel のセルの ID に預ける。channel の ID も `fork_worker` で出し、セルを放すときに、残った品物をその ID の下で落としてから `retire_worker` で退かせる」を足す。
    - 「報告する誤り」の `TransferLeak`: 「退かせる ID (`par` の子、タスク、channel) の物体が残っていること」。T-1 では「channel」を書かず、T-3 で足す。

## docs/spec/declarations.md、types.md、modules.md

15. declarations.md:121: T-3 で「`Std.Par.par` と、`Async` の命令で実行する35個の extern の行 (`Std.Async` の29行と `Std.Chan` の6行) は、その場で実行しない。… `Std.Chan.chan_cell` は即値を写すだけの普通の extern である」。T-5 で39個 (`Std.Atomic` の4行を足す) にする。
16. (T-2) declarations.md:126 の固定の規則の箇条の下に: 「型引数が値の Kind に効かないハンドルの型 (`Std.Chan.Chan a`) の型引数の中の型変数も固定しない。処理系はハンドルの即値だけを読み、中身の値には触れないためである。`Chan a` 自身の Kind は `a` によらず `Unr` である ([型と Kind](types.md) の「Kind」)」
17. (T-2) types.md:23: 「…`MutArray a` は常に `Lin`、`Std.Chan.Chan a` は `a` によらず `Unr` である」
18. modules.md:69 の std の一覧: T-3 で `Chan.em`、T-5 で `Atomic.em` を `Async.em` の後に足す。「REPL のセッション」: T-3 で「`Chan a` の値は束縛に残せるが、前の入力の channel は閉じて空の channel として振る舞う。後の入力の channel とは generation で見分ける」、T-5 で「`Counter` は入力をまたいで使える」。

## docs/implementation

19. (T-4) testing.md 「seed で流す」:
    - `cancels` の文を次にする。「逐次か seed のどちらかの run の `cancels` が 0 でなければ、`RunStats` を比べない。どちらかの run の `channel_ops` か `channel_waits` が 0 でなければ、`tasks`、`timers`、`dropped_failures`、`channels_swept` だけを比べる (`eml_test_support::channel_independent`)。channel を使う run では、待つかどうかと、送り手が参照を持ち続けた品物の裏返しが順で変わり、ほかの欄がそれに従うためである。代わりに、channel を使うプログラムでは仕事の回数の食い違いが見えなくなる。`try_send` と `try_recv` の成否が順で変わるので、`channel_ops` も比べない」
    - `shared_objects` の箇条: 「…始めるときと fork のときと、タスクの結果で印を付ける物体の数である。channel の `push_raw` が品物に付ける印も数えるが、channel を使う run では比べない」
    - `channels_swept` の箇条を足す: 比べる。
    - 書き方の規則: 「`try_send` と `try_recv` の結果と、`merge` の出力の順はスケジュールで変わる。出力に出さず、数か並べ替えで確かめる」
    - 「channel の `RunStats` を固定する単体のテストは `Sched::Sequential` だけで流す」
20. architecture.md:309: T-3 で「`Async` のタスクとスコープと channel の意味 (登録表 `Registry`、セル、受け箱、ハンドル) は `cells.rs` の核が持ち…」、T-5 で「`Counter` の表」を足す。:312 の登録表が数える欄の一覧に、T-3 で `channel_ops`、`channel_waits`、`channels_swept` を足す。
21. (T-7) status.md:
    - :28 に `Chan.em`、`Atomic.em` を足す。
    - :36 に「`Std.Chan` の channel (`send`、`recv`、`try_send`、`try_recv`、`close`、`merge`) と、`Std.Atomic` の `Counter` がある」を足す。
    - :141 を「本物の I/O の driver はない。driver は時間だけを扱う。…」にする (「channel と」を消す)。
    - :143 の取り消しの地点に `send`、`recv` を足す。
    - :149 を「速い道で操作の予算を使うのは、`check_cancelled` と `Std.Chan` の速い道である。`try_send` と `try_recv` は使うだけで譲らない」にする。
    - 新しい箇条: 「閉じない channel は run の終わりまで登録表に残り、残った品物もそのときまで落とさない (`RunStats.channels_swept`。[ロードマップ](../future/roadmap.md) の「非同期」)」
    - 新しい箇条: 「`Counter` の表は `Machine` が持ち、放さない」
22. (T-7) benchmarks.md 「今の数値」: 3つの表を測り直す。
23. (T-7) docs/README.md: stdlib.md の行に `Std.Chan`、`Std.Atomic` を、vm.md の行に「channel」を足す。

## docs/future、ロードマップ、CLAUDE.md (T-7)

24. async.md:49 とコードの塊: `await_chan : ChanWait -> Wake` を `await_send : Cell -> Wake` と `await_recv : Cell -> Wake` にする。:57: 「…`await_drain`、`reschedule`、`await_send`、`await_recv` の6つは実装してある」。
25. async.md:63: 「`try_send` と `try_recv` は予算を使っても譲らず、`send`、`recv`、`check_cancelled` が譲る」。
26. async.md:193、multicore.md:276、:288: 「予約した途中の持ち主の ID」を「値を預かる構造の ID (タスクの結果はタスクの ID、channel の品物は channel のセルの ID)。`fork_worker` で出し、構造とともに退かせる」にする。
27. async.md:195: 最後のハンドルで落とす文を、stdlib.md 「`Std.Chan`」の1つ目の箇条の規則 (close で放す) にする。
28. async.md:198: 待ちが `Std.Async.chan_wait_send` と `chan_wait_recv` を通ることを足す。merge の行のコメントの「いちばん内側のスコープに作る」は残し、「終わりを集めるタスクを1つ足す」を足す。
29. async.md:380: 「`RunStats` に `completions` を足す (channel の欄 `channel_ops`、`channel_waits`、`channels_swept` はある)」。
30. async.md 「未決の論点と将来の拡張」: 「`Lin` の closer token」を足す。中身は計画の D-2 の「後で戻す形」の1。
31. multicore.md:125: 「channel の待ち (待つ側の登録)」を「channel の待ち (channel のセルの待ちと、その channel の待ち手の列の項)」にする。
32. roadmap.md:
    - 「段の列」から `async-chan` の項目を消し、`async-net` の前提から `async-chan` を消す (「前提: `io-driver`」が残る)。
    - 「その後の項目」の「### 非同期」に「最後のハンドルで放す channel」を足し、計画の D-2 の「後に回した理由」と「見直す条件」を入れ子の箇条で書く。
33. CLAUDE.md:
    - 話題の表に行を足す: `| \`Std.Chan\` and \`Std.Atomic\` | docs/spec/stdlib.md の「\`Std.Chan\`」, docs/spec/stdlib.md の「\`Std.Atomic\`」, docs/spec/vm.md の「channel」, docs/spec/object-model.md の「論理的な worker ID」 |`
    - Architecture の `eml_vm` の行: 「`par`, `Async` tasks and channels on one worker」。
