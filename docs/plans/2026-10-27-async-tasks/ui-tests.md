# async-tasks の計画の付録: UI テストのプログラム

T-7、T-8、T-14、T-15 が置く UI テストの下書きである。`Async` がまだないので、`Async` を使うプログラムの期待する出力は推定である (計画の Q-4)。期待と違ったときは、ランタイムの不具合かプログラムの誤りかを確かめてから、`cargo insta review` で受け入れる。

## 共通の約束

- 1行目に、目的を述べて `docs/spec/stdlib.md` の「`Std.Async`」(`async/` 以外は該当する節) を引く日本語のコメントを置く
- `async/` のプログラムは `import Async` を書き、次の `spin` を定義する。`main : Unit -> <IO, Panic> Unit` とする

```haskell
spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)
```

- 横取りの約束: `start` か `spawn` の直後に、始めた側が次の `Async` の操作の前に `spin 40` を呼ぶ。始めたタスクが実行待ちにいる間に seed の予算 (16 以下の安全点) が尽きるので、seed の run ごとに `preemptions > 0` になる。`await` と `yield_now` による切り替えは数えない
- 出力の約束: 一度に出力するタスクは1つだけにし、出力の順は `await` か `run` が戻る順で決める。失敗する run の失敗の文言は1つだけにする。ファイルはテストのディレクトリからの相対パスで開く。これらは harness では確かめず、レビューで確かめる

## `tests/ui/run/async/` (T-14、`exit_in_task.em` だけ T-15)

| ファイル | 目的 | stdout |
|---|---|---|
| `start_await.em` | `start` と `await`。待つ順が終わる順によらないこと。`run` が本体の値を返すこと | `21` |
| `spawn_grandchild.em` | `run` が根のスコープのすべてのタスク (待たない孫を含む) を待つこと | `grandchild`、`after run` |
| `drop_handle.em` | `drop` は取り消さず、落としたタスクの `Lin` の結果を放すこと (検査モードで漏れがない) | `still runs`、`after run` |
| `lin_result.em` | `Lin` の値が `await` と `run` を通ること | `3 7` |
| `handle_outlives_run.em` | 1つ目の `run` が返したハンドルを、後の `run` で待てること | `42` |
| `try_all_isolates.em` | タスクの中の `try_all` が Panic、IO の失敗、`fail_io` を分けること | 下のコメントの5行 |
| `nested_run.em` | タスクの中の入れ子の `run` と、2つ目の `run` | `inner 20`、`outer 21`、`3` |
| `yield_now.em` | `yield_now` が戻り、タスクが終わること | `6 10 15` |
| `exit_in_task.em` | タスクの `exit 3` がプログラムを終えること。本体 (待ち) と `main` (`run` で待つ) がそれぞれ `FileIn` を持つ。検査モードで漏れがない | `before`、`exiting`、終了コード 3 |

```haskell
-- start_await.em
body : Unit -> <IO, Panic, Async> Int
body () =
  let a = Async.start (fn () -> spin 40 + 1)
  let b = Async.start (fn () -> spin 40 + 2)
  let s = spin 40
  let y = Async.await b
  let x = Async.await a
  y * 10 + x + s
main () = println (show (Async.run body))

-- spawn_grandchild.em
body : Unit -> <IO, Panic, Async> Unit
body () =
  Async.spawn (fn () ->
    Async.spawn (fn () ->
      let _ = spin 40
      println "grandchild")
    let _ = spin 40
    ())
  let _ = spin 40
  ()
main () =
  Async.run body
  println "after run"

-- drop_handle.em (import MutArray)
body : Unit -> <IO, Panic, Async> Unit
body () =
  let t = Async.start (fn () ->
    let _ = spin 40
    println "still runs"
    MutArray.make 2 0)
  drop t
  let _ = spin 40
  ()
main () =
  Async.run body
  println "after run"

-- lin_result.em (import MutArray)
main () =
  let a = Async.run (fn () ->
    let t = Async.start (fn () -> MutArray.make 3 7)
    let _ = spin 40
    Async.await t)
  let (n, a) = MutArray.length a
  let (x, a) = MutArray.read a 0
  drop a
  println "\{show n} \{show x}"

-- handle_outlives_run.em
main () =
  let t = Async.run (fn () ->
    let t = Async.start (fn () -> spin 40 + 6)
    let _ = spin 40
    t)
  println (show (Async.run (fn () -> Async.await t * 7)))

-- try_all_isolates.em (import FileIn)
open_missing : Unit -> <IO> Int
open_missing () =
  let f = FileIn.open "missing.txt"
  FileIn.close f
  0
describe : Result Async.Failure Int -> String
describe r = match r with
  | Ok n -> "ok \{show n}"
  | Err (Async.Panicked m) -> "panicked: \{m}"
  | Err (Async.IOFailed e) -> "io failed: \{show e.kind}: \{e}"
body : Unit -> <IO, Panic, Async> Unit
body () =
  let a = Async.start (fn () -> Async.try_all (fn () -> spin 40 + 1))
  let b = Async.start (fn () -> Async.try_all (fn () -> (panic "boom" : Int)))
  let c = Async.start (fn () -> Async.try_all (fn () -> open_missing ()))
  let d = Async.start (fn () -> Async.try_all (fn () -> fail_io TimedOut "too slow"))
  let _ = spin 40
  println (describe (Async.await a))
  println (describe (Async.await b))
  println (describe (Async.await c))
  println (describe (Async.await d))
main () =
  Async.run body
  println "run finished"
-- stdout:
-- ok 1
-- panicked: boom
-- io failed: NotFound: cannot open `missing.txt`: not found
-- io failed: TimedOut: too slow
-- run finished

-- nested_run.em
body : Unit -> <IO, Panic, Async> Unit
body () =
  let t = Async.start (fn () ->
    let n = Async.run (fn () ->
      let c = Async.start (fn () -> spin 40 + 2)
      let _ = spin 40
      Async.await c * 10)
    println "inner \{show n}"
    n + 1)
  let _ = spin 40
  let m = Async.await t
  println "outer \{show m}"
main () =
  Async.run body
  println (show (Async.run (fn () -> 3)))

-- yield_now.em
count : Int -> Int -> <IO, Panic, Async> Int
count n acc = if n == 0 then acc else step n acc
step : Int -> Int -> <IO, Panic, Async> Int
step n acc =
  Async.yield_now ()
  count (n - 1) (acc + n)
body : Unit -> <IO, Panic, Async> Unit
body () =
  let a = Async.start (fn () -> count 3 0)
  let b = Async.start (fn () -> count 4 0)
  let c = Async.start (fn () -> count 5 0)
  let _ = spin 40
  let x = Async.await a
  let y = Async.await b
  let z = Async.await c
  println "\{show x} \{show y} \{show z}"
main () = Async.run body

-- exit_in_task.em (import FileIn)
body : Unit -> <IO, Panic, Async> Unit
body () =
  let f = FileIn.open "exit_in_task.em"
  let x = Async.start (fn () ->
    let _ = spin 40
    println "exiting"
    (exit 3 : Unit))
  let _ = spin 40
  Async.await x
  FileIn.close f
main () =
  let g = FileIn.open "exit_in_task.em"
  println "before"
  Async.run body
  FileIn.close g
  println "unreachable"
```

`exit_in_task.em` のスナップショットは `--- stdout ---\nbefore\nexiting\n--- stderr ---\n--- exit: 3 ---` である。

## `tests/ui/run-fail/async/` (T-14)

`<std>/Async.em:L:C` は `run` の中の `run_result` の呼び出しの位置である。その位置を指すことを確かめてから受け入れる。

| ファイル | 目的 | stdout | 実行時エラー |
|---|---|---|---|
| `await_rethrows_panic.em` | `await` が子の Panic を起こし直す。先に終わった子の失敗が、本体の起こし直しより先に記録される | `before` | `panicked: boom`、`  at <std>/Async.em:L:C` |
| `spawned_failure_fails_run.em` | 待たない子の IO の失敗が `run` で出る。本体の値 `1` は捨てる | `before` | ``cannot open `missing.txt`: not found``、`  at <std>/Async.em:L:C` |
| `caught_await_still_fails_run.em` | `await` の周りで捕まえても、スコープの失敗は止まらない。種類は `await` を通っても残る | `before`、`caught TimedOut: too slow` | `too slow`、`  at <std>/Async.em:L:C` |

```haskell
-- await_rethrows_panic.em
body : Unit -> <IO, Panic, Async> Unit
body () =
  let t = Async.start (fn () ->
    let _ = spin 40
    (panic "boom" : Int))
  let _ = spin 40
  let n = Async.await t
  println "unreachable \{show n}"
main () =
  println "before"
  Async.run body
  println "unreachable"

-- spawned_failure_fails_run.em (import FileIn)
body : Unit -> <IO, Panic, Async> Int
body () =
  Async.spawn (fn () ->
    let _ = spin 40
    let f = FileIn.open "missing.txt"
    FileIn.close f)
  let _ = spin 40
  1
main () =
  println "before"
  let n = Async.run body
  println "unreachable \{show n}"

-- caught_await_still_fails_run.em
body : Unit -> <IO, Panic, Async> Unit
body () =
  let t = Async.start (fn () ->
    let _ = spin 40
    (fail_io TimedOut "too slow" : Int))
  let _ = spin 40
  match try_io (fn () -> Async.await t) with
    | Ok n -> println "ok \{show n}"
    | Err e -> println "caught \{show e.kind}: \{e}"
main () =
  println "before"
  Async.run body
  println "unreachable"
```

## `tests/ui/check-fail/` (T-8、T-14)

| ファイル | 置く task | プログラムの中身 | 期待 |
|---|---|---|---|
| `types/task_captures_var.em` | T-14 | `var n = 0`。`let t = Async.start (fn () -> n <- n + 1)`。`Async.await t`。`println (show n)` | E2002 が2つ |
| `types/async_in_par_child.em` | T-14 | `import Std.Par`。`let (a, b) = Par.par (fn () -> Async.reschedule ()) (fn () -> 1)` | `Async` を名指す E2020 |
| `linearity/task_awaited_twice.em` | T-14 | `let t = Async.start (fn () -> 1)`。`Async.await t` を2回 | E3002 |
| `linearity/either_holding_a_file.em` | T-8 | 使わない `Either FileIn.FileIn Int` | E3003 |

## `async/` の外 (T-7、T-8)

| ファイル | 置く task | 目的 | 期待 |
|---|---|---|---|
| `run/io/fail_io.em` | T-7 | 新しい種類の `Show`。`try_io` が `fail_io` を種類ごと捕まえること。`fail_io` の枝で開いたままの `FileIn` | stdout `[Cancelled, TimedOut, ConnectionReset, AddrInUse, ResourceLimit]`、`ConnectionReset: peer went away`、`Other: bad input`。終了コード 0 |
| `run-fail/io/fail_io_uncaught.em` | T-7 | `try_panic` だけでは `fail_io` を捕まえず (IO は Panic でない)、呼んだ位置の実行時エラーになること | stdout `before`。誤り `gave up`、`  at run-fail/io/fail_io_uncaught.em:L:C` |
| `run/data/prelude_either.em` | T-8 | `Either` の導出した `Show` と `Ord`。`match` が `Either FileIn.FileIn Int` を消費すること | `Left 1`、`Right "x"`、`True`、`0`、`5` |

```haskell
-- fail_io.em (import FileIn)
finish : FileIn.FileIn -> <IO> Int
finish f =
  FileIn.close f
  1
guarded : Bool -> <IO> Int
guarded bad =
  let f = FileIn.open "fail_io.em"
  if bad then fail_io Other "bad input" else finish f
main () =
  println (show [Cancelled, TimedOut, ConnectionReset, AddrInUse, ResourceLimit])
  match try_io (fn () -> (fail_io ConnectionReset "peer went away" : Unit)) with
    | Ok () -> println "ok"
    | Err e -> println "\{show e.kind}: \{e}"
  match try_io (fn () -> guarded True) with
    | Ok n -> println (show n)
    | Err e -> println "\{show e.kind}: \{e}"

-- fail_io_uncaught.em
main () =
  println "before"
  match try_panic (fn () -> (fail_io Other "gave up" : Int)) with
    | Ok n -> println (show n)
    | Err m -> println "caught \{m}"

-- prelude_either.em (import FileIn)
consume : Either FileIn.FileIn Int -> <IO> Int
consume e = match e with
  | Left f ->
    FileIn.close f
    0
  | Right n -> n
main () =
  println (show (Left 1 : Either Int String))
  println (show (Right "x" : Either Int String))
  println (show ((Left 9 : Either Int String) < Right "a"))
  println (show (consume (Left (FileIn.open "prelude_either.em"))))
  println (show (consume (Right 5)))
```
