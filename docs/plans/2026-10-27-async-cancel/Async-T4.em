-- 1つの worker の上でタスクを動かす (docs/spec/stdlib.md の「`Std.Async`」)。`Async` の生の操作、ランタイムが名前で
-- 呼ぶ入口、extern はランタイムとの ABI なので、形を変えるときはランタイムも合わせて変える

-- ランタイムが待たせたタスクを再開するときに渡す値。ランタイムは宣言の順のタグ (`Woke` = 0、`Cancelled` = 1) で作る
pub data Wake =
  | Woke
  | Cancelled

-- タスクの生の操作。どちらも once である。
-- 操作はランタイムとの ABI で、利用者は包みの関数を呼ぶ (docs/spec/effects.md の「抽象エフェクト」)
pub abstract effect Async where
  -- `Cell` が指すタスクのセルが終わるか、スコープのセルが閉じるまで待つ
  await_cell : Cell -> Wake
  -- 実行待ちの列の後ろへ回る
  reschedule : Unit -> Wake

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

-- 中の取り消しの地点はビットも旗も見ない。立っているビットか旗は、抜けた後の最初の地点で投げる
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

-- ランタイムの口。生の行を `pub` にせず、結果の型の `a` は `start`、`start_in`、`run`、`await` のシグネチャだけが結ぶ。
-- 処理系はこの対応を確かめない。表で預かる印を付ける引数は、`start_raw`、`run_raw`、`ready`、`finish_ok`、
-- `take_result`、`run_result`、`detach`、`cancel_raw` の引数、`park` と `start_in_raw` の2つ目の引数、`task_cell` の
-- 引数 (`task_cell` だけは借りて預かる) である。預かる引数の型変数は `Unr` に固定せず、その矢印には閉じた row を書ける
-- (docs/spec/stdlib.md の「`Std.Async`」)
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
