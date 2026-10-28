-- タスクの間で値を受け渡す channel (docs/spec/stdlib.md の「`Std.Chan`」)。`Chan a` は登録表の添字を持つ `Unr` の
-- 即値で、`close` で放す。送受信の速い道は、枠か品物を確かめる extern (`reserve_send`、`reserve_recv`) と、積むか
-- 取る extern (`push_raw`、`pop_raw`) の組である。`Async` の操作は外のモジュールから perform できないので、待ちは
-- `Std.Async` の `chan_wait_send` と `chan_wait_recv` を通す (docs/spec/effects.md の「抽象エフェクト」)
import Async (Async)

-- 登録表の添字 `(generation << 32) | index` を持つ即値である。Kind は `a` によらず `Unr`
pub extern data Chan a

-- コンストラクタの名前はモジュールの中で重ねられないので、3つの型で別の名前にする
pub data SendResult a =
  | Sent
  | Rejected a
  deriving (Eq, Show)

pub data TrySendResult a =
  | Accepted
  | Full a
  | Refused a
  deriving (Eq, Show)

pub data TryRecvResult a =
  | Got a
  | Empty
  | Drained
  deriving (Eq, Show)

-- 容量 `n` の channel を作る。0 は待ち合わせで、負の数は 0 として扱う
pub new : Int -> <IO> Chan a
new n = new_raw n

-- 枠があれば積み、なければ枠が空くか閉じるまで待つ。閉じていれば値を `Rejected` で返す。取り消しの地点である
pub send : a -> Chan a -> <IO, Async> SendResult a
send x c = if Async.cancel_requested () then raise_cancelled () else send_loop x c True

-- 品物があれば取り、なければ品物が来るか閉じるまで待つ。閉じて空なら `None` を返す。取り消しの地点である
pub recv : Chan a -> <IO, Async> Option a
recv c = if Async.cancel_requested () then raise_cancelled () else recv_loop c True

-- 待たない送り。予算を使うが譲らない (docs/spec/stdlib.md の「操作の予算」)
pub try_send : a -> Chan a -> <IO> TrySendResult a
try_send x c =
  let r = reserve_send c False
  if r == 0 then
    push_raw x c
    Accepted
  else if r == 2 then Refused x
  else Full x

-- 待たない受け取り。予算を使うが譲らない
pub try_recv : Chan a -> <IO> TryRecvResult a
try_recv c =
  let r = reserve_recv c False
  if r == 0 then Got (pop_raw c)
  else if r == 2 then Drained
  else Empty

-- 閉じる。2回目からは何もしない。待っている受け手と送り手をすべて起こし、残った品物は受け手が取り切れる
pub close : Chan a -> <IO> Unit
close c = close_raw c

-- 予約と確定の間は `Int` の比べだけにする。間に呼び出しを置くと、ほかのタスクが確かめた後の channel を変えうるため
-- である (docs/spec/vm.md の「channel」)。起こされた後のやり直しは、予算を使っても譲らず、取り消しも確かめない。
-- 品物か枠があれば、先に操作を終えるためである (docs/spec/stdlib.md の「取り消し」)
send_loop : a -> Chan a -> Bool -> <IO, Async> SendResult a
send_loop x c yields =
  let r = reserve_send c yields
  if r == 0 then
    push_raw x c
    Sent
  else if r == 2 then Rejected x
  else if r == 3 then
    Async.yield_now ()
    send_loop x c True
  else match Async.chan_wait_send (chan_cell c) with
    | Async.Woke -> send_loop x c False
    | Async.Cancelled -> raise_cancelled ()

recv_loop : Chan a -> Bool -> <IO, Async> Option a
recv_loop c yields =
  let r = reserve_recv c yields
  if r == 0 then Some (pop_raw c)
  else if r == 2 then None
  else if r == 3 then
    Async.yield_now ()
    recv_loop c True
  else match Async.chan_wait_recv (chan_cell c) with
    | Async.Woke -> recv_loop c False
    | Async.Cancelled -> raise_cancelled ()

-- `Std.Async` の取り消しの地点と同じ失敗 (docs/spec/stdlib.md の「取り消し」)
raise_cancelled : Unit -> <IO> a
raise_cancelled () = fail_io Prelude.Cancelled "task was cancelled"

-- ランタイムの口。生の行を `pub` にせず、品物の型の `a` は包む関数のシグネチャが結ぶ。`push_raw` の1つ目の引数は
-- 預かる引数で、`pop_raw` がそれを1回返す (docs/spec/declarations.md の「`extern`」)。`reserve_send` と
-- `reserve_recv` は、0 なら確定してよく、1 なら待ち、2 なら閉じている (受け取りは閉じて空)、3 なら先に譲る。3 は
-- 2つ目の引数が真のときだけ返す
extern new_raw      : Int -> <IO> Chan a
extern chan_cell    : Chan a -> Async.Cell
extern reserve_send : Chan a -> Bool -> <IO> Int
extern push_raw     : a -> Chan a -> <IO> Unit
extern reserve_recv : Chan a -> Bool -> <IO> Int
extern pop_raw      : Chan a -> <IO> a
extern close_raw    : Chan a -> <IO> Unit
