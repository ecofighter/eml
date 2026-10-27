# async-cancel の新しいテストと変えるテスト

計画書 `docs/plans/2026-10-27-async-cancel.md` の付録である。各テストの見出しの括弧は、そのテストを入れる task である。プログラムと期待するスナップショットは、`T-1.patch` から `T-5.patch` を基準 `16b8b73` に順に当てた写しで確かめてある。新しい UI テストはどれも、検査モードの seed 1〜100 と、既定のスケジュールの速いモード5回で、出力が1通りだった。

書き方の約束:

- `start` と `spawn` のたびに `spin 40` を置き、どのファイルも少なくとも1回横取りされるようにする ([テスト戦略](../../implementation/testing.md) の「seed で流す」)
- 出力は、1度に1つのタスクだけが書くか、`await`、`scope`、`run` が戻る順か、出力のきっかけになる取り消しの順で並ぶようにする
- 取り消しがどの時点で届いても (始まる前、動いている間、実行待ち、待ち)、出る結果を同じにする。`forever` は取り消さなければ終わらないので、取り消しが届かなければハーネスが 10 秒で止まりとして報告する


## `tests/ui/run/async/drop_cancels.em` (T-3)

```haskell
-- `drop` で落としたハンドルのタスクは取り消される。待っていた `await` は `Cancelled` で戻り、std の包みが種類
-- `Cancelled` の IO の失敗を投げる。待たれていたタスクも、包みがハンドルを落とすので取り消される
-- (docs/spec/stdlib.md の「取り消し」)。
import Async (Async)

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

forever : Unit -> <IO, Async> Int
forever () =
  Async.yield_now ()
  forever ()

victim : Unit -> <IO, Panic, Async> Unit
victim () =
  let u = Async.start forever
  let _ = spin 40
  match try_io (fn () -> Async.await u) with
    | Ok n -> println "unreachable \{show n}"
    | Err e -> println "victim: \{show e.kind}: \{e}"

body : Unit -> <IO, Panic, Async> Unit
body () =
  let t = Async.start victim
  let _ = spin 40
  Async.yield_now ()
  drop t

main : Unit -> <IO, Panic> Unit
main () =
  Async.run body
  println "after run"
```

Expected snapshot body:

```
--- stdout ---\\n{stdout}--- stderr ---\\n{diagnostics}{stderr}{written}{exit}\")"
input_file: tests/ui/run/async/drop_cancels.em
---
--- stdout ---
victim: Cancelled: task was cancelled
after run
--- stderr ---
```

## `tests/ui/run/async/cancel.em` (T-3)

```haskell
-- `cancel` は待っているタスクを取り消し、終わったタスクのハンドルなら残っている `Lin` の結果を落とす。どちらも
-- 検査モードで漏れが出ない (docs/spec/stdlib.md の「取り消し」)。
import Async (Async)
import MutArray

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

forever : Unit -> <IO, Async> Int
forever () =
  Async.yield_now ()
  forever ()

body : Unit -> <IO, Panic, Async> Unit
body () =
  let done = Async.start (fn () -> MutArray.make 3 0)
  let _ = spin 40
  let t = Async.start (fn () ->
    match try_io forever with
      | Ok n -> println "unreachable \{show n}"
      | Err e -> println "t: \{show e.kind}: \{e}")
  let _ = spin 40
  let n = Async.await (Async.start (fn () -> spin 40 + 5))
  println "awaited \{show n}"
  Async.cancel done
  Async.cancel t

main : Unit -> <IO, Panic> Unit
main () =
  Async.run body
  println "after run"
```

Expected snapshot body:

```
--- stdout ---\\n{stdout}--- stderr ---\\n{diagnostics}{stderr}{written}{exit}\")"
input_file: tests/ui/run/async/cancel.em
---
--- stdout ---
awaited 5
t: Cancelled: task was cancelled
after run
--- stderr ---
```

## `tests/ui/run/async/failed_child_cancels_siblings.em` (T-3)

```haskell
-- 失敗した子は、同じスコープのほかのタスクを取り消す。`await` で待つ兄弟も、`yield_now` で回り続ける兄弟も
-- 取り消され、`run` は最初の失敗を起こし直す (docs/spec/stdlib.md の「取り消し」)。
import Async (Async)

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

forever : Unit -> <IO, Async> Int
forever () =
  Async.yield_now ()
  forever ()

waiter : Unit -> <IO, Panic, Async> Unit
waiter () =
  let u = Async.start forever
  let _ = spin 40
  match try_io (fn () -> Async.await u) with
    | Ok n -> println "unreachable \{show n}"
    | Err e -> println "waiter: \{show e.kind}: \{e}"

body : Unit -> <IO, Panic, Async> Unit
body () =
  Async.spawn waiter
  let _ = spin 40
  Async.spawn (fn () ->
    let _ = forever ()
    ())
  let _ = spin 40
  Async.spawn (fn () ->
    let _ = spin 400
    fail_io TimedOut "too slow")
  let _ = spin 40
  ()

main : Unit -> <IO, Panic> Unit
main () =
  match try_io (fn () -> Async.run body) with
    | Ok () -> println "unreachable"
    | Err e -> println "run: \{show e.kind}: \{e}"
```

Expected snapshot body:

```
--- stdout ---\\n{stdout}--- stderr ---\\n{diagnostics}{stderr}{written}{exit}\")"
input_file: tests/ui/run/async/failed_child_cancels_siblings.em
---
--- stdout ---
waiter: Cancelled: task was cancelled
run: TimedOut: too slow
--- stderr ---
```

## `tests/ui/run/async/cancel_persists.em` (T-3)

```haskell
-- 取り消しは続く。最初の `Cancelled` を `try_io` で捕まえても、次の取り消しの地点がまた投げる
-- (docs/spec/stdlib.md の「取り消し」)。
import Async (Async)

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

forever : Unit -> <IO, Async> Int
forever () =
  Async.yield_now ()
  forever ()

stubborn : Unit -> <IO, Panic, Async> Unit
stubborn () =
  let u = Async.start forever
  let _ = spin 40
  match try_io (fn () -> Async.await u) with
    | Ok n -> println "unreachable \{show n}"
    | Err e -> println "first: \{show e.kind}: \{e}"
  match try_io (fn () -> Async.yield_now ()) with
    | Ok () -> println "unreachable"
    | Err e -> println "second: \{show e.kind}: \{e}"

body : Unit -> <IO, Panic, Async> Unit
body () =
  let t = Async.start stubborn
  let _ = spin 40
  Async.cancel t

main : Unit -> <IO, Panic> Unit
main () = Async.run body
```

Expected snapshot body:

```
--- stdout ---\\n{stdout}--- stderr ---\\n{diagnostics}{stderr}{written}{exit}\")"
input_file: tests/ui/run/async/cancel_persists.em
---
--- stdout ---
first: Cancelled: task was cancelled
second: Cancelled: task was cancelled
--- stderr ---
```

## `tests/ui/run/async/nested_run_cancelled.em` (T-3)

```haskell
-- タスクの中の入れ子の `run` の根は、呼んだタスクのスコープの下にある。外のスコープが子の失敗で取り消されると、入れ子の
-- `run` のタスクも取り消され、`run` は `Cancelled` を起こし直す (docs/spec/stdlib.md の「取り消し」)。
import Async (Async)

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

forever : Unit -> <IO, Async> Int
forever () =
  Async.yield_now ()
  forever ()

nested : Unit -> <IO, Panic, Async> Unit
nested () =
  let r = try_io (fn () ->
    Async.run (fn () ->
      let u = Async.start forever
      let _ = spin 40
      Async.await u))
  match r with
    | Ok n -> println "unreachable \{show n}"
    | Err e -> println "inner run: \{show e.kind}: \{e}"

body : Unit -> <IO, Panic, Async> Unit
body () =
  Async.spawn nested
  let _ = spin 40
  Async.spawn (fn () ->
    let _ = spin 400
    fail_io TimedOut "too slow")
  let _ = spin 40
  ()

main : Unit -> <IO, Panic> Unit
main () =
  match try_io (fn () -> Async.run body) with
    | Ok () -> println "unreachable"
    | Err e -> println "run: \{show e.kind}: \{e}"
```

Expected snapshot body:

```
--- stdout ---\\n{stdout}--- stderr ---\\n{diagnostics}{stderr}{written}{exit}\")"
input_file: tests/ui/run/async/nested_run_cancelled.em
---
--- stdout ---
inner run: Cancelled: task was cancelled
run: TimedOut: too slow
--- stderr ---
```

## `tests/ui/run/async/cancel_reaches_nested_run.em` (T-3)

```haskell
-- `cancel t` は、t の中の入れ子の `run` のタスクも取り消す。入れ子の `run` は `Cancelled` を起こし直す
-- (docs/spec/stdlib.md の「取り消し」)。
import Async (Async)

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

forever : Unit -> <IO, Async> Int
forever () =
  Async.yield_now ()
  forever ()

worker : Unit -> <IO, Panic, Async> Unit
worker () =
  let r = try_io (fn () ->
    Async.run (fn () ->
      let u = Async.start forever
      let _ = spin 40
      Async.await u))
  match r with
    | Ok n -> println "unreachable \{show n}"
    | Err e -> println "worker: \{show e.kind}: \{e}"

body : Unit -> <IO, Panic, Async> Unit
body () =
  let t = Async.start worker
  let _ = spin 40
  Async.cancel t

main : Unit -> <IO, Panic> Unit
main () =
  Async.run body
  println "after run"
```

Expected snapshot body:

```
--- stdout ---\\n{stdout}--- stderr ---\\n{diagnostics}{stderr}{written}{exit}\")"
input_file: tests/ui/run/async/cancel_reaches_nested_run.em
---
--- stdout ---
worker: Cancelled: task was cancelled
after run
--- stderr ---
```

## `tests/ui/run/async/cancelled_kind.em` (T-3)

```haskell
-- 種類 `Cancelled` の IO の失敗で終わったタスクは、スコープの失敗にならない。待たないタスクなら報告されず、`await` は
-- 種類と文言のまま起こし直す (docs/spec/stdlib.md の「取り消し」)。
import Async (Async)

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

body : Unit -> <IO, Panic, Async> Unit
body () =
  Async.spawn (fn () ->
    let _ = spin 40
    fail_io Prelude.Cancelled "gave up")
  let _ = spin 40
  let t = Async.start (fn () -> (fail_io Prelude.Cancelled "stopped" : Int))
  let _ = spin 40
  match try_io (fn () -> Async.await t) with
    | Ok n -> println "unreachable \{show n}"
    | Err e -> println "await: \{show e.kind}: \{e}"

main : Unit -> <IO, Panic> Unit
main () =
  Async.run body
  println "after run"
```

Expected snapshot body:

```
--- stdout ---\\n{stdout}--- stderr ---\\n{diagnostics}{stderr}{written}{exit}\")"
input_file: tests/ui/run/async/cancelled_kind.em
---
--- stdout ---
await: Cancelled: stopped
after run
--- stderr ---
```

## `tests/ui/run/async/cancelled_file.em` (T-3)

```haskell
-- 取り消したタスクが持っていた `FileIn` は、巻き戻しが落として閉じる。検査モードで漏れが出ない
-- (docs/spec/stdlib.md の「取り消し」)。
import Async (Async)
import FileIn

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

forever : Unit -> <IO, Async> Int
forever () =
  Async.yield_now ()
  forever ()

reader : Unit -> <IO, Panic, Async> Unit
reader () =
  let f = FileIn.open "cancelled_file.em"
  let u = Async.start forever
  let _ = spin 40
  let n = Async.await u
  FileIn.close f
  println "unreachable \{show n}"

body : Unit -> <IO, Panic, Async> Unit
body () =
  let t = Async.start reader
  let _ = spin 40
  Async.cancel t

main : Unit -> <IO, Panic> Unit
main () =
  Async.run body
  println "after run"
```

Expected snapshot body:

```
--- stdout ---\\n{stdout}--- stderr ---\\n{diagnostics}{stderr}{written}{exit}\")"
input_file: tests/ui/run/async/cancelled_file.em
---
--- stdout ---
after run
--- stderr ---
```

## `tests/ui/run/async/scope_cancel.em` (T-4)

```haskell
-- 子が `cancel_scope` でスコープを取り消すと、待っている本体も旗で取り消され、`Cancelled` が `scope` から出る。
-- `scope` を囲む `try_io` で捕まえれば、スコープを出たタスクはもう取り消されていない (docs/spec/stdlib.md の「スコープ」)。
import Async (Async)

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

forever : Unit -> <IO, Async> Int
forever () =
  Async.yield_now ()
  forever ()

inner : Async.Scope -> <IO, Panic, Async> Int
inner s =
  let u = Async.start forever
  let _ = spin 40
  Async.spawn (fn () ->
    let _ = spin 400
    Async.cancel_scope s)
  let _ = spin 40
  Async.await u

body : Unit -> <IO, Panic, Async> Unit
body () =
  match try_io (fn () -> Async.scope inner) with
    | Ok n -> println "unreachable \{show n}"
    | Err e -> println "scope: \{show e.kind}: \{e}"
  Async.yield_now ()
  println "still running"

main : Unit -> <IO, Panic> Unit
main () = Async.run body
```

Expected snapshot body:

```
--- stdout ---\\n{stdout}--- stderr ---\\n{diagnostics}{stderr}{written}{exit}\")"
input_file: tests/ui/run/async/scope_cancel.em
---
--- stdout ---
scope: Cancelled: task was cancelled
still running
--- stderr ---
```

## `tests/ui/run/async/scope_child_failure_wins.em` (T-4)

```haskell
-- スコープの子の失敗は本体を取り消す。本体の `Cancelled` は記録しないので、`scope` は子の失敗を起こし直す。スコープを
-- 出たタスクは取り消されておらず、続けて動く (docs/spec/stdlib.md の「スコープ」)。
import Async (Async)

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

forever : Unit -> <IO, Async> Int
forever () =
  Async.yield_now ()
  forever ()

inner : Async.Scope -> <IO, Panic, Async> Int
inner s =
  let u = Async.start forever
  let _ = spin 40
  Async.spawn_in s (fn () ->
    let _ = spin 400
    fail_io TimedOut "too slow")
  let _ = spin 40
  Async.await u

body : Unit -> <IO, Panic, Async> Int
body () =
  let r = try_io (fn () -> Async.scope inner)
  match r with
    | Ok n -> println "unreachable \{show n}"
    | Err e -> println "scope: \{show e.kind}: \{e}"
  let t = Async.start (fn () -> spin 40 + 1)
  let _ = spin 40
  Async.await t

main : Unit -> <IO, Panic> Unit
main () = println (show (Async.run body))
```

Expected snapshot body:

```
--- stdout ---\\n{stdout}--- stderr ---\\n{diagnostics}{stderr}{written}{exit}\")"
input_file: tests/ui/run/async/scope_child_failure_wins.em
---
--- stdout ---
scope: TimedOut: too slow
1
--- stderr ---
```

## `tests/ui/run/async/closed_scope.em` (T-4)

```haskell
-- 閉じたスコープの `Scope` を外へ出すと、`spawn_in` と `start_in` は Panic `the scope is closed` を起こし、
-- `cancel_scope` は何もしない (docs/spec/stdlib.md の「スコープ」)。
import Async (Async)

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

body : Unit -> <IO, Panic, Async> Unit
body () =
  let s = Async.scope (fn s ->
    Async.spawn_in s (fn () ->
      let _ = spin 40
      println "in scope")
    let _ = spin 40
    s)
  Async.cancel_scope s
  match try_panic (fn () -> Async.spawn_in s (fn () -> println "unreachable")) with
    | Ok () -> println "unreachable"
    | Err m -> println "panicked: \{m}"

main : Unit -> <IO, Panic> Unit
main () = Async.run body
```

Expected snapshot body:

```
--- stdout ---\\n{stdout}--- stderr ---\\n{diagnostics}{stderr}{written}{exit}\")"
input_file: tests/ui/run/async/closed_scope.em
---
--- stdout ---
in scope
panicked: the scope is closed
--- stderr ---
```

## `tests/ui/run/async/start_in_flagged.em` (T-4)

```haskell
-- 旗を立てたスコープに `start_in` で加えたタスクは、作るときに取り消される。最初の取り消しの地点で投げる
-- (docs/spec/stdlib.md の「スコープ」)。
import Async (Async)

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

inner : Async.Scope -> <IO, Panic, Async> Unit
inner s =
  Async.cancel_scope s
  let t = Async.start_in s (fn () ->
    println "child starts"
    match try_io (fn () -> Async.yield_now ()) with
      | Ok () -> println "unreachable"
      | Err e -> println "child: \{show e.kind}: \{e}")
  let _ = spin 40
  Async.await t

body : Unit -> <IO, Panic, Async> Unit
body () =
  match try_io (fn () -> Async.scope inner) with
    | Ok () -> println "unreachable"
    | Err e -> println "scope: \{show e.kind}: \{e}"

main : Unit -> <IO, Panic> Unit
main () = Async.run body
```

Expected snapshot body:

```
--- stdout ---\\n{stdout}--- stderr ---\\n{diagnostics}{stderr}{written}{exit}\")"
input_file: tests/ui/run/async/start_in_flagged.em
---
--- stdout ---
child starts
child: Cancelled: task was cancelled
scope: Cancelled: task was cancelled
--- stderr ---
```

## `tests/ui/run/async/cancel_reaches_open_scopes.em` (T-4)

```haskell
-- `cancel t` は、t が開いているスコープの子も取り消す。t が子を待つ `scope` の終わりは shield の中の待ちだが、
-- 子が取り消されるので終わる。ビットを立てた後に開くスコープも、開くときに取り消す (docs/spec/stdlib.md の「取り消し」)。
import Async (Async)

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

forever : Unit -> <IO, Async> Int
forever () =
  Async.yield_now ()
  forever ()

worker : Unit -> <IO, Panic, Async> Unit
worker () =
  Async.scope (fn s ->
    Async.spawn_in s (fn () ->
      let _ = forever ()
      ())
    let _ = spin 40
    ())
  match try_io (fn () -> Async.yield_now ()) with
    | Ok () -> println "unreachable"
    | Err e -> println "worker: \{show e.kind}: \{e}"

body : Unit -> <IO, Panic, Async> Unit
body () =
  let t = Async.start worker
  let _ = spin 40
  Async.cancel t

main : Unit -> <IO, Panic> Unit
main () =
  Async.run body
  println "after run"
```

Expected snapshot body:

```
--- stdout ---\\n{stdout}--- stderr ---\\n{diagnostics}{stderr}{written}{exit}\")"
input_file: tests/ui/run/async/cancel_reaches_open_scopes.em
---
--- stdout ---
worker: Cancelled: task was cancelled
after run
--- stderr ---
```

## `tests/ui/run/async/shield.em` (T-4)

```haskell
-- `shield` の中の取り消しの地点は旗を見ず、待ちは最後まで終わる。`shield` を出た後の最初の取り消しの地点が投げる
-- (docs/spec/stdlib.md の「取り消し」)。
import Async (Async)

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

inner : Async.Scope -> <IO, Panic, Async> Unit
inner s =
  Async.cancel_scope s
  let n = Async.shield (fn () ->
    let t = Async.start (fn () -> spin 40 + 7)
    let _ = spin 40
    Async.yield_now ()
    Async.await t)
  println "shielded \{show n}"
  Async.yield_now ()
  println "unreachable"

body : Unit -> <IO, Panic, Async> Unit
body () =
  match try_io (fn () -> Async.scope inner) with
    | Ok () -> println "unreachable"
    | Err e -> println "after shield: \{show e.kind}: \{e}"

main : Unit -> <IO, Panic> Unit
main () = Async.run body
```

Expected snapshot body:

```
--- stdout ---\\n{stdout}--- stderr ---\\n{diagnostics}{stderr}{written}{exit}\")"
input_file: tests/ui/run/async/shield.em
---
--- stdout ---
shielded 7
after shield: Cancelled: task was cancelled
--- stderr ---
```

## `tests/ui/run-fail/async/cancelled_run.em` (T-4)

```haskell
-- `run` の本体の `Cancelled` は、ほかの IO の失敗と同じく `run` から出る。捕まえなければ実行時エラーになる
-- (docs/spec/stdlib.md の「取り消し」)。
import Async (Async)

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

forever : Unit -> <IO, Async> Int
forever () =
  Async.yield_now ()
  forever ()

body : Unit -> <IO, Panic, Async> Int
body () =
  Async.scope (fn s ->
    Async.spawn_in s (fn () ->
      let _ = spin 400
      Async.cancel_scope s)
    let _ = spin 40
    let u = Async.start forever
    let _ = spin 40
    Async.await u)

main : Unit -> <IO, Panic> Unit
main () =
  println "before"
  let n = Async.run body
  println "unreachable \{show n}"
```

Expected snapshot body:

```
--- stdout ---\\n{stdout}--- stderr ---\\n{diagnostics}{stderr}--- runtime error ---\\n{error}\\n{written}\")"
input_file: tests/ui/run-fail/async/cancelled_run.em
---
--- stdout ---
before
--- stderr ---
--- runtime error ---
task was cancelled
  at <std>/Async.em:54:9
```

## `tests/ui/run/async/check_cancelled.em` (T-5)

```haskell
-- 待たない純粋なループも、`check_cancelled` を呼べば取り消せる。予算を使い切るたびにほかのタスクへ worker を渡すので、
-- 取り消すタスクも動ける (docs/spec/stdlib.md の「取り消し」「操作の予算」)。
import Async (Async)

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

busy : Int -> <IO, Panic, Async> Int
busy n =
  Async.check_cancelled ()
  busy (n + 1)

body : Unit -> <IO, Panic, Async> Unit
body () =
  let t = Async.start (fn () ->
    match try_io (fn () -> busy 0) with
      | Ok n -> println "unreachable \{show n}"
      | Err e -> println "busy: \{show e.kind}: \{e}")
  let _ = spin 40
  Async.yield_now ()
  Async.cancel t

main : Unit -> <IO, Panic> Unit
main () = Async.run body
```

Expected snapshot body:

```
--- stdout ---\\n{stdout}--- stderr ---\\n{diagnostics}{stderr}{written}{exit}\")"
input_file: tests/ui/run/async/check_cancelled.em
---
--- stdout ---
busy: Cancelled: task was cancelled
--- stderr ---
```

## `tests/ui/run/async/cancel_requested.em` (T-5)

```haskell
-- `cancel_requested` はスコープの旗も読み、`shield` の中では偽を返す。`check_cancelled` は旗で投げ、スコープを出た
-- 後は偽に戻る (docs/spec/stdlib.md の「取り消し」)。
import Async (Async)

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

inner : Async.Scope -> <IO, Panic, Async> Unit
inner s =
  Async.spawn_in s (fn () ->
    let _ = spin 40
    ())
  let _ = spin 40
  println "before \{show (Async.cancel_requested ())}"
  Async.cancel_scope s
  println "after \{show (Async.cancel_requested ())}"
  let shielded = Async.shield (fn () -> Async.cancel_requested ())
  println "in shield \{show shielded}"
  Async.check_cancelled ()
  println "unreachable"

body : Unit -> <IO, Panic, Async> Unit
body () =
  match try_io (fn () -> Async.scope inner) with
    | Ok () -> println "unreachable"
    | Err e -> println "scope: \{show e.kind}: \{e}"
  println "outside \{show (Async.cancel_requested ())}"

main : Unit -> <IO, Panic> Unit
main () = Async.run body
```

Expected snapshot body:

```
--- stdout ---\\n{stdout}--- stderr ---\\n{diagnostics}{stderr}{written}{exit}\")"
input_file: tests/ui/run/async/cancel_requested.em
---
--- stdout ---
before False
after True
in shield False
scope: Cancelled: task was cancelled
outside False
--- stderr ---
```

## `tests/ui/run/async/await_cancelled.em` (T-5)

```haskell
-- 取り消されたタスクを待つと、`await` は `Cancelled` を起こし直す。待った側は取り消されていないので、
-- `cancel_requested` は偽で、run も失敗しない (docs/spec/stdlib.md の「取り消し」)。
import Async (Async)

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

body : Unit -> <IO, Panic, Async> Unit
body () =
  let t = Async.start (fn () ->
    Async.scope (fn s ->
      Async.cancel_scope s
      Async.yield_now ()))
  let _ = spin 40
  match try_io (fn () -> Async.await t) with
    | Ok () -> println "unreachable"
    | Err e -> println "await: \{show e.kind}: \{e}"
  println "requested \{show (Async.cancel_requested ())}"

main : Unit -> <IO, Panic> Unit
main () =
  Async.run body
  println "after run"
```

Expected snapshot body:

```
--- stdout ---\\n{stdout}--- stderr ---\\n{diagnostics}{stderr}{written}{exit}\")"
input_file: tests/ui/run/async/await_cancelled.em
---
--- stdout ---
await: Cancelled: task was cancelled
requested False
after run
--- stderr ---
```

## `crates/eml_cli/tests/repl/async/stale_scope.repl` (T-4)

```
# 前の入力の run が返した `Scope` は閉じたスコープとして扱う。`cancel_scope` は何もせず、`spawn_in` は、同じ添字に
# 次の run が開いたスコープがあっても、Panic を起こす (docs/spec/stdlib.md の「スコープ」)
> import Async
> let s = Async.run (fn () -> Async.scope (fn s -> s))
> Async.run (fn () -> Async.cancel_scope s)
> Async.run (fn () -> Async.scope (fn t -> Async.spawn_in s (fn () -> println "unreachable")))
> println "next"
```

Expected snapshot body:

```
> import Async
> let s = Async.run (fn () -> Async.scope (fn s -> s))
> Async.run (fn () -> Async.cancel_scope s)
> Async.run (fn () -> Async.scope (fn t -> Async.spawn_in s (fn () -> println "unreachable")))
runtime error: panicked: the scope is closed
  at <std>/Async.em:54:9
> println "next"
next
--- close ---
no leaks
```

With the floor forced to 0 the `spawn_in` line prints `unreachable` (aliasing). With `Scope` as an `Unr` `Foreign` the session breaks at the next runtime error.

## `crates/eml_vm/tests/tasks.rs` (T-3)

```rust
/// 落としたハンドルのタスクが継続で待っていれば、受け箱の処理が待ちを外して `Cancelled` で再開する。実行待ちの列に
/// いる `yield_now` のタスクは、再開するときに `Cancelled` を受ける。逐次の方針の数を固定する。
#[test]
fn a_dropped_handle_cancels_a_parked_task() {
    let src = "\
import Async (Async)
import Async
forever : Unit -> <IO, Async> Int
forever () =
  Async.yield_now ()
  forever ()
main : Unit -> <IO, Panic> Unit
main () =
  Async.run (fn () ->
    let t = Async.start (fn () -> Async.await (Async.start forever))
    Async.yield_now ()
    drop t
    println \"dropped\")
";
    assert_eq!(run_both(src), "dropped\n");
    let (_, stats) = run_stats(src);
    let s = stats.expect("the checked run");
    assert_eq!(
        (s.tasks, s.parks, s.dropped_failures, s.cancels),
        (3, 3, 0, 2)
    );
}
```

## `crates/eml_vm/tests/tasks.rs` (T-5)

```rust
/// `check_cancelled` は 128 回目の呼び出しで予算を使い切り、1回だけ `yield_now` でほかのタスクに worker を渡す。
#[test]
fn the_budget_yields_once_every_128_checks() {
    let src = |n: u32| {
        format!(
            "\
import Async (Async)
import Async
checks : Int -> <IO, Panic, Async> Unit
checks n =
  if n == 0 then ()
  else
    Async.check_cancelled ()
    checks (n - 1)
main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () -> checks {n})
"
        )
    };
    for (n, parks) in [(127, 0), (128, 1), (256, 2)] {
        let (_, stats) = run_stats(&src(n));
        let s = stats.expect("the checked run");
        assert_eq!((s.parks, s.cancels), (parks, 0), "{n} checks");
    }
}
```

## `crates/eml_cli/tests/ui.rs` gate and its test (T-2)

```rust
/// 取り消しの届いた run は `RunStats` を比べない。届いた時点で、それぞれのタスクがした仕事が変わる
/// (docs/implementation/testing.md の「seed で流す」)。
fn compares_stats(checked: &Ran, other: &Ran) -> bool {
    let quiet = |ran: &Ran| ran.finished.is_none_or(|f| f.stats.cancels == 0);
    quiet(checked) && quiet(other)
}

#[test]
fn stats_are_compared_only_without_cancellations() {
    let ran = |cancels: Option<u64>| {
        let finished = cancels.map(|cancels| {
            let mut stats = eml_cli::RunStats::default();
            stats.cancels = cancels;
            Finished {
                stats,
                exit: None,
                preemptions: 1,
            }
        });
        Ran {
            stdout: String::new(),
            stderr: String::new(),
            written: String::new(),
            result: Ok(None),
            finished,
        }
    };
    assert!(compares_stats(&ran(Some(0)), &ran(Some(0))));
    assert!(!compares_stats(&ran(Some(1)), &ran(Some(0))));
    assert!(!compares_stats(&ran(Some(0)), &ran(Some(1))));
    assert!(compares_stats(&ran(None), &ran(None)));
}
```

Call site in `assert_seeds_agree`: `let compare = compare_stats && compares_stats(checked, &ran);` passed to `assert_agrees` instead of `compare_stats`.

## Changed: `tests/ui/run-fail/async/caught_await_still_fails_run.em` (T-3, kind 2 — program changes, snapshot byte-identical)

```haskell
-- `await` の周りで失敗を捕まえても、スコープの失敗は残り `run` が起こし直す。IO の失敗の種類は `await` を通っても
-- 残る。失敗した子が本体も取り消すので、`shield` の中で待ち、待ち始めが子の失敗の前か後かによらず子の失敗を受け取る
-- (docs/spec/stdlib.md の「取り消し」)。
import Async (Async)

spin : Int -> <Panic> Int
spin n = if n == 0 then 0 else spin (n - 1)

body : Unit -> <IO, Panic, Async> Unit
body () =
  let t = Async.start (fn () ->
    let _ = spin 40
    (fail_io TimedOut "too slow" : Int))
  let _ = spin 40
  match try_io (fn () -> Async.shield (fn () -> Async.await t)) with
    | Ok n -> println "ok \{show n}"
    | Err e -> println "caught \{show e.kind}: \{e}"

main : Unit -> <IO, Panic> Unit
main () =
  println "before"
  Async.run body
  println "unreachable"
```

Snapshot unchanged (`before` / `caught TimedOut: too slow` / runtime error `too slow at <std>/Async.em:54:9`). 300 seeds + 50 default fast runs: always `caught TimedOut`. The unshielded original gives `caught Cancelled` for about half the seeds under the new rules.

## Changed: `tests/ui/run/async/drop_handle.em` (T-3, kind 3 — comment only)

```haskell
-- `drop` で落としても、取り消しの地点を通らないタスクは最後まで動く。落としたタスクの `Lin` の結果は終わったときに
-- 放し、検査モードで漏れが出ない (docs/spec/stdlib.md の「取り消し」)。
```
