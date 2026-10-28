-- 1つの worker の上でタスクを動かす (docs/spec/stdlib.md の「`Std.Async`」)。`Async` の生の操作、ランタイムが名前で
-- 呼ぶ入口、extern はランタイムとの ABI なので、形を変えるときはランタイムも合わせて変える

-- ランタイムが待たせたタスクを再開するときに渡す値。ランタイムは宣言の順のタグ (`Woke` = 0、`Cancelled` = 1) で作る
pub data Wake =
  | Woke
  | Cancelled

-- タスクの生の操作。どれも once である。
-- 操作はランタイムとの ABI で、利用者は包みの関数を呼ぶ (docs/spec/effects.md の「抽象エフェクト」)
pub abstract effect Async where
  -- `Cell` が指すタスクのセルが終わるか、スコープのセルが閉じるまで待つ
  await_cell : Cell -> Wake
  -- 実行待ちの列の後ろへ回る
  reschedule : Unit -> Wake
  -- 指定したミリ秒だけ眠る。0 以下は `reschedule` と同じく列の後ろへ回る
  await_time : Int -> Wake
  -- スコープに属するタスクがなくなるまで待つ。スコープは閉じない
  await_drain : Scope -> Wake
  -- channel に枠が空くか、閉じるまで待つ (docs/spec/stdlib.md の「`Std.Chan`」)
  await_send : Cell -> Wake
  -- channel に品物が来るか、閉じるまで待つ
  await_recv : Cell -> Wake

-- 登録表の添字 `(generation << 32) | index` を持つ即値である。Kind は `Unr`
pub extern data Cell
-- ランタイムのタスクのハンドルを中身に持つ。Kind はいつも `Lin`
pub extern data Task a
-- タスクの失敗。`await` と `run` は、IO の失敗を種類と文言のまま、Panic を文言のまま起こし直す
pub data Failure =
  | IOFailed IOError
  | Panicked String
  deriving (Show)

pub try_all : (Unit -> <IO, Panic | e> a) -> <IO | e> Result Failure a
try_all action =
  match try_io (fn () -> try_panic action) with
    | Ok (Ok x) -> Ok x
    | Ok (Err m) -> Err (Panicked m)
    | Err e -> Err (IOFailed e)

-- ランタイムが名前で呼ぶ入口。単相に保つ (名前で ABI が決まり、instance の名前が正式な名前のままになるため)
start_task : (Unit -> <IO, Async> Unit) -> <IO> Unit
start_task f =
  handle f () with
    | ctl await_cell c k -> park c k
    | ctl reschedule () k -> ready k
    | ctl await_time ms k -> park_time ms k
    | ctl await_drain s k -> park_drain s k
    | ctl await_send c k -> park_send c k
    | ctl await_recv c k -> park_recv c k
    | return () -> ()

resume_task : (once Wake -> <IO> Unit) -> Wake -> <IO> Unit
resume_task k w = k w

-- タスクの底。値は預かる行にそのまま渡す (docs/spec/declarations.md の「`extern`」)
bottom : (Unit -> <IO, Panic, Async> a) -> Unit -> <IO, Async> Unit
bottom f () =
  match try_all f with
    | Ok x -> finish_ok x
    | Err (IOFailed (IOError { kind = k, message = m })) -> finish_io k m
    | Err (Panicked m) -> finish_panic m

pub run : (Unit -> <IO, Panic, Async> a) -> <IO, Panic> a
run f = run_result (run_raw (bottom f))

-- 置き場所の決まり。`run` の位置 (`<std>/Async.em:行:桁`) は実行時エラーの UI テストと REPL のスナップショットに出る。
-- そのため `run` より上には `Async` の操作の宣言と `start_task` の節だけを置き、関数は `shield` より下に足す。
-- extern は extern の並びの末尾に足す。

-- 名前付きのスコープのセルの番号を持つ即値で、`Cell` と同じ形である。Kind は `Unr` で、子のタスクにも渡せる。閉じて
-- 離れたスコープの番号と前の run の番号は、閉じたスコープとして扱う (docs/spec/stdlib.md の「スコープ」)
pub extern data Scope

-- 本体の結果、失敗、取り消しのどれで終わっても、子がすべて終わるまで待ってから戻る。最初に記録した失敗が本体の結果より
-- 先になる (docs/spec/stdlib.md の「スコープ」)
pub scope : (Scope -> <IO, Panic, Async> a) -> <IO, Panic, Async> a
scope f =
  let s = open_scope ()
  match try_all (fn () -> f s) with
    | Ok x ->
      -- 最初の失敗を起こし直せば、`x` は巻き戻しが落とす
      wait_closed s
      x
    | Err e ->
      fail_scope s e
      wait_closed s
      reraise e

pub start : (Unit -> <IO, Panic, Async> a) -> <IO, Async> Task a
start f = start_raw (bottom f)

-- ハンドルを落とすと取り消しになるので、`drop` でなく `detach` で手放す
pub spawn : (Unit -> <IO, Panic, Async> Unit) -> <IO, Async> Unit
spawn f = detach (start_raw (bottom f))

pub start_in : Scope -> (Unit -> <IO, Panic, Async> a) -> <IO, Panic, Async> Task a
start_in s f = start_in_raw s (bottom f)

pub spawn_in : Scope -> (Unit -> <IO, Panic, Async> Unit) -> <IO, Panic, Async> Unit
spawn_in s f = detach (start_in_raw s (bottom f))

pub await : Task a -> <IO, Panic, Async> a
await t =
  let (c, t) = task_cell t
  match await_cell c with
    | Woke -> take_result t
    | Cancelled ->
      -- 待つ側が取り消されたので、待っていたタスクも取り消す。`Task a` は `Lin` で、ほかに待つ者はいない
      drop t
      raise_cancelled ()

pub cancel : Task a -> <IO> Unit
cancel t = cancel_raw t

pub cancel_scope : Scope -> <IO> Unit
cancel_scope s = cancel_scope_raw s

pub yield_now : Unit -> <IO, Async> Unit
yield_now () =
  match reschedule () with
    | Woke -> ()
    | Cancelled -> raise_cancelled ()

-- 待たない取り消しの地点。速い道として予算を1つ使い、使い切ったらほかのタスクに worker を渡す
-- (docs/spec/stdlib.md の「操作の予算」)
pub check_cancelled : Unit -> <IO, Async> Unit
check_cancelled () =
  if cancel_point () then raise_cancelled ()
  else if spend_budget () then yield_now ()
  else ()

-- 取り消しの地点が投げるときだけ真を返す。`shield` の中では、今のスコープを今の `shield` の中で自分が開いたときだけ、
-- その旗を読む
pub cancel_requested : Unit -> <IO> Bool
cancel_requested () = cancel_point ()

-- 中の取り消しの地点は、ビットと、`shield` の外で開いたスコープの旗を見ない。中で開いたスコープには外の取り消しが
-- 届かない。外で立っているビットか旗は、抜けた後の最初の地点で投げる
pub shield : (Unit -> <IO, Panic, Async> a) -> <IO, Panic, Async> a
shield f =
  shield_enter ()
  match try_all f with
    | Ok x ->
      shield_leave ()
      x
    | Err e ->
      shield_leave ()
      reraise e

-- `ms` ミリ秒待つ取り消しの地点。0 以下なら実行待ちの列の後ろへ回るだけである (docs/spec/stdlib.md の「時間」)
pub sleep_ms : Int -> <IO, Async> Unit
sleep_ms ms =
  match await_time ms with
    | Woke -> ()
    | Cancelled -> raise_cancelled ()

-- 本体は呼んだタスクで動かし、期限は子のスコープに置いたタスクが旗で届ける。本体を `scope` で包むのは、本体が
-- 始めたタスクにも期限を届け、終わるまで待つためである (docs/spec/stdlib.md の「時間」)
pub timeout : Int -> (Unit -> <IO, Panic, Async> a) -> <IO, Panic, Async> Option a
timeout ms f =
  let s = open_scope ()
  let timer = deadline s ms
  let r = try_all (fn () -> scope (fn _ -> f ()))
  -- 残る期限のタスクを取り消す。旗はスコープを離れるまで呼んだタスクにも届くが、この後に取り消しの地点はない
  cancel_scope_raw s
  wait_closed s
  let late = expired timer
  match r with
    | Ok x -> Some x
    -- 外から取り消された呼び手は、期限が過ぎていても `None` で取り消しを飲み込まない
    | Err e -> if late && cancelled e && not (cancel_point ()) then None else reraise e

-- 過ぎた期限は、タスクを始めずにすぐ旗を立てる
deadline : Scope -> Int -> <IO, Panic, Async> Option (Task Unit)
deadline s ms =
  if ms <= 0 then
    cancel_scope_raw s
    None
  else Some (start_in s (fn () ->
    sleep_ms ms
    cancel_scope_raw s))

-- 期限のタスクが眠り切って終わっていれば、期限は過ぎている
expired : Option (Task Unit) -> <IO> Bool
expired timer = match timer with
  | None -> True
  | Some t -> match try_all (fn () -> take_result t) with
    | Ok () -> True
    | Err _ -> False

-- 2つの本体を子のスコープのタスクで競わせ、shield の中で両方の終わりを待つ。本体の失敗はスコープの最初の失敗
-- として `wait_closed` が起こし直す (docs/spec/stdlib.md の「時間」)
pub race : (Unit -> <IO, Panic, Async> a) -> (Unit -> <IO, Panic, Async> b) -> <IO, Panic, Async> Either a b
race f g =
  let s = open_scope ()
  let ta = start_in s (arm s f)
  let tb = start_in s (arm s g)
  wait_closed s
  match try_all (fn () -> take_result ta) with
    | Ok x ->
      drop tb
      Left x
    | Err e -> match try_all (fn () -> take_result tb) with
      | Ok y -> Right y
      | Err _ -> reraise e

-- 値を返したときにまだ取り消されていなければ、勝って旗を立てる。負けた側の値は巻き戻しが落とす
arm : Scope -> (Unit -> <IO, Panic, Async> a) -> Unit -> <IO, Panic, Async> a
arm s f () =
  let x = scope (fn _ -> f ())
  if claim_scope s then x else raise_cancelled ()

-- 待ちを `timeout` で区切り、期限が来たら呼んだタスク自身が残ったタスクを取り消し、空になるまで待ち直す。期限が触る
-- のは呼んだタスクの待ちだけなので、戻った後に期限が `s` に触れることはない (docs/spec/stdlib.md の「時間」)
pub drain_within : Int -> Scope -> <IO, Async> Unit
drain_within ms s =
  -- `timeout` の期限のタスクは種類 `Cancelled` でしか失敗せず、`drained` は `Panic` を持たないので、`Err` は届かない
  match try_panic (fn () -> timeout ms (fn () -> drained s)) with
    | Ok (Some ()) -> ()
    | Ok None ->
      cancel_members s
      drained s
    | Err _ -> ()

drained : Scope -> <IO, Async> Unit
drained s =
  match await_drain s with
    | Woke -> ()
    | Cancelled -> raise_cancelled ()

cancelled : Failure -> Bool
cancelled e = match e with
  | IOFailed (IOError { kind = k, message = _ }) -> k == Prelude.Cancelled
  | Panicked _ -> False

-- 取り消しを受けた包みが投げる IO の失敗。`Wake` の `Cancelled` と名前が重なるので、種類は修飾して書く
-- (docs/spec/stdlib.md の「取り消し」)
raise_cancelled : Unit -> <IO> a
raise_cancelled () = fail_io Prelude.Cancelled "task was cancelled"

-- `try_all` で分けた失敗を、種類と文言のまま起こし直す
reraise : Failure -> <IO, Panic> a
reraise e = match e with
  | IOFailed (IOError { kind = k, message = m }) -> fail_io k m
  | Panicked m -> panic m

-- 本体の失敗をスコープに記録し、子をすべて取り消す。種類 `Cancelled` は記録せず、旗だけを立てる
fail_scope : Scope -> Failure -> <IO> Unit
fail_scope s e = match e with
  | IOFailed (IOError { kind = k, message = m }) -> fail_scope_io s k m
  | Panicked m -> fail_scope_panic s m

-- 閉じる印を付け、shield の中でスコープのセルを待ってから、閉じたスコープを離れる。shield の中の待ちはいつも `Woke`
-- で戻る。最初の失敗は `leave_scope` が起こし直す
wait_closed : Scope -> <IO, Panic, Async> Unit
wait_closed s =
  let c = close_scope s
  shield_enter ()
  let _ = await_cell c
  shield_leave ()
  leave_scope s

-- `Std.Chan` の待ち。`Async` の操作は外のモジュールから perform できないので、ここで包んで公開する
-- (docs/spec/effects.md の「抽象エフェクト」)。`Cell` を作る公開の関数はないので、利用者が呼んでも渡せる値はない
pub chan_wait_send : Cell -> <IO, Async> Wake
chan_wait_send c = await_send c

pub chan_wait_recv : Cell -> <IO, Async> Wake
chan_wait_recv c = await_recv c

-- ランタイムの口。生の行を `pub` にせず、結果の型の `a` は `start`、`start_in`、`run`、`await` のシグネチャだけが結ぶ。
-- 処理系はこの対応を確かめない。表で預かる印を付ける引数は、`start_raw`、`run_raw`、`ready`、`finish_ok`、
-- `take_result`、`run_result`、`detach`、`cancel_raw` の引数、`park`、`park_time`、`park_drain`、`park_send`、
-- `park_recv`、`start_in_raw` の2つ目の引数、`task_cell` の引数 (`task_cell` だけは借りて預かる) である。預かる引数の
-- 型変数は `Unr` に固定せず、その矢印には閉じた row を書ける (docs/spec/stdlib.md の「`Std.Async`」)
extern start_raw        : (Unit -> <IO, Async> Unit) -> <IO> Task a
extern run_raw          : (Unit -> <IO, Async> Unit) -> <IO> Task a
extern start_in_raw     : Scope -> (Unit -> <IO, Async> Unit) -> <IO, Panic> Task a
extern park             : Cell -> (once Wake -> <IO> Unit) -> <IO> Unit
extern ready            : (once Wake -> <IO> Unit) -> <IO> Unit
extern finish_ok        : a -> <IO> Unit
extern finish_io        : IOErrorKind -> String -> <IO> Unit
extern finish_panic     : String -> <IO> Unit
extern task_cell        : Task a -> (Cell, Task a)
extern take_result      : Task a -> <IO, Panic> a
extern run_result       : Task a -> <IO, Panic> a
extern detach           : Task a -> <IO> Unit
extern cancel_raw       : Task a -> <IO> Unit
extern open_scope       : Unit -> <IO> Scope
extern close_scope      : Scope -> <IO> Cell
extern leave_scope      : Scope -> <IO, Panic> Unit
extern cancel_scope_raw : Scope -> <IO> Unit
extern fail_scope_io    : Scope -> IOErrorKind -> String -> <IO> Unit
extern fail_scope_panic : Scope -> String -> <IO> Unit
extern shield_enter     : Unit -> <IO> Unit
extern shield_leave     : Unit -> <IO> Unit
extern cancel_point     : Unit -> <IO> Bool
extern spend_budget     : Unit -> <IO> Bool
extern park_time        : Int -> (once Wake -> <IO> Unit) -> <IO> Unit
extern claim_scope      : Scope -> <IO> Bool
extern park_drain       : Scope -> (once Wake -> <IO> Unit) -> <IO> Unit
extern cancel_members   : Scope -> <IO> Unit
extern park_send        : Cell -> (once Wake -> <IO> Unit) -> <IO> Unit
extern park_recv        : Cell -> (once Wake -> <IO> Unit) -> <IO> Unit
