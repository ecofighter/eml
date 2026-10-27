-- async-tasks の計画の付録: std/Async.em の下書き。
-- T-9 がこの内容で std/Async.em を作る。コメントは実装のときに docs/spec/stdlib.md の「`Std.Async`」の節を引く形に直す。
-- 計画の ID (D-x、T-x) と、この付録のパスはコードに書かない。

-- ランタイムが待たせたタスクを再開するときに渡す値。`Cancelled` は取り消しの段で使う。タグは Woke = 0、Cancelled = 1
pub data Wake =
  | Woke
  | Cancelled

-- タスクの生の操作。どちらも once で、ランタイムとの ABI である
pub effect Async where
  await_cell : Cell -> Wake        -- Cell が指すタスクのセルが終わるか、スコープのセルが閉じるまで待つ
  reschedule : Unit -> Wake        -- 実行待ちの列の後ろへ回る

-- 登録表の添字 `(generation << 32) | index` を持つ即値。Unr、Repr::Int
pub extern data Cell
-- TaskHandle を中身に持つ Foreign。Lin
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

-- タスクの底。値は thunk に包んで渡す (extern の引数は Unr に固定されるので、Lin の結果を直接は渡せない)
bottom : (Unit -> <IO, Panic, Async> a) -> Unit -> <IO, Async> Unit
bottom f () =
  match try_all f with
    | Ok x -> finish_ok (fn () -> x)
    | Err (IOFailed (IOError { kind = k, message = m })) -> finish_io k m
    | Err (Panicked m) -> finish_panic m

pub run : (Unit -> <IO, Panic, Async> a) -> <IO, Panic> a
run f = (run_result (run_raw (bottom f))) ()

pub start : (Unit -> <IO, Panic, Async> a) -> <IO, Async> Task a
start f = start_raw (bottom f)

pub spawn : (Unit -> <IO, Panic, Async> Unit) -> <IO, Async> Unit
spawn f = drop (start_raw (bottom f))

pub await : Task a -> <IO, Panic, Async> a
await t =
  let (c, t) = task_cell t
  let _ = await_cell c
  (take_result t) ()

pub yield_now : Unit -> <IO, Async> Unit
yield_now () =
  match reschedule () with
    | Woke -> ()
    | Cancelled -> ()

-- ランタイムの口。pub にしない。型の `a` は、上の pub の関数のシグネチャだけが結び付ける
extern start_raw    : (Unit -> <IO, Async> Unit) -> <IO> Task a
extern run_raw      : (Unit -> <IO, Async> Unit) -> <IO> Task a
extern park         : Cell -> (once Wake -> <IO> Unit) -> <IO> Unit
extern ready        : (once Wake -> <IO> Unit) -> <IO> Unit
extern finish_ok    : (once Unit -> a) -> <IO> Unit
extern finish_io    : IOErrorKind -> String -> <IO> Unit
extern finish_panic : String -> <IO> Unit
extern task_cell    : Task a -> (Cell, Task a)
extern take_result  : Task a -> <IO, Panic> (once Unit -> a)
extern run_result   : Task a -> <IO, Panic> (once Unit -> a)
