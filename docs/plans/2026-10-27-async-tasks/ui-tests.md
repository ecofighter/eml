# async-tasks の計画の付録: UI テストのプログラム

T-7、T-8、T-9c、T-9d、T-14、T-15 が置く UI テストと、T-9c が置く REPL のトランスクリプトの下書きである。`Async` がまだないので、`Async` を使うプログラムの期待する出力は推定である (計画の Q-4)。期待と違ったときは、ランタイムの不具合かプログラムの誤りかを確かめてから、`cargo insta review` で受け入れる。

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
| `types/async_in_par_child.em` | T-14 | `import Std.Par`。`let (a, b) = Par.par (fn () -> Async.yield_now ()) (fn () -> 1)` | `Async` を名指す E2020 |
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

## `abstract` のテスト (T-9c、T-9d)

T-9c の言語のテストは、ユーザーのモジュールの抽象型と抽象エフェクトを使う。T-9d のテストは、std の `Map` と `Async` が抽象になった後に置く。プログラムは下書きで、細部は実装者が決めてよい。期待する誤りの番号と数は変えない。

| ファイル | 置く task | 目的 | 期待 |
|---|---|---|---|
| `check-fail/names/abstract_constructor/` (`Lib.em`、`main.em`) | T-9c | 抽象型のコンストラクタを、外のモジュールの作る式、パターン、import の並びの `T(..)` に書くと誤りになること | E1058 が3つ |
| `check-fail/names/abstract_operation/` (`Lib.em`、`main.em`) | T-9c | 抽象エフェクトの操作を、外のモジュールで perform し、handler の節に書くと誤りになること | E1058 が2つ |
| `check-fail/types/abstract_field/` (`Lib.em`、`main.em`) | T-9c | 抽象型のフィールドを外のモジュールで射影し、更新すると誤りになること。更新のフィールドが2つでも誤りは1つ | E2021 が2つ |
| `check-fail/syntax/abstract_misplaced.em` | T-9c | `abstract` を置けない5つの位置 | E0011 が5つ (1つの位置に1つ)。ほかに、`abstract` を消した宣言が今受ける誤りだけが出る (`extern` の行の E1033、`type` の行の E0004)。`type` の行は E0011 が E0004 より先に出る |
| `run/data/abstract_api.em` | T-9c | 宣言したモジュールの中では抽象型を普通に使えること。非公開の型をフィールドに持つ `pub abstract data` に E1032 を当てないこと。導出した `Show` が中身を表示すること | `42`、`Counter 3` |
| `check-fail/names/async_op_handled.em` | T-9d | `Async` の操作を、利用者の handler の節に書くと誤りになること (`await_cell` と `reschedule`) | E1058 が2つ |
| `check-fail/names/abstract_std_constructor.em` | T-9d | std の抽象型 `Map` のコンストラクタをパターンに書くと誤りになること。診断は `Std.Map` を書く | E1058 が1つ |

check-fail のテストは名前解決の誤りを `names/` に、型検査の誤りを `types/` に、構文の誤りを `syntax/` に置く。`abstract_field` は外のモジュールが要るので、1つのファイルでなくディレクトリにする。

```haskell
-- abstract_constructor/Lib.em
pub abstract data Handle a = | Handle Int
pub make : Int -> Handle a
make n = Handle n
pub get : Handle a -> Int
get (Handle n) = n

-- abstract_constructor/main.em
import Lib (Handle(..))                  -- E1058 (`(..)` の位置)
main : Unit -> <IO> Unit
main () =
  let Lib.Handle n = Lib.make 1          -- E1058 (パターン)
  println (show (Lib.get (Lib.Handle n))) -- E1058 (作る式)

-- abstract_operation/Lib.em
pub abstract effect Ping where
  ping : Unit -> Int
pub twice : Unit -> <Ping> Int
twice () = ping () + ping ()
pub answer : (Unit -> <Ping> Int) -> Int
answer f =
  handle f () with
    | ctl ping () k -> k 21
    | return x -> x

-- abstract_operation/main.em
import Lib
main : Unit -> <IO> Unit
main () =
  println (show (Lib.answer (fn () -> Lib.ping ()))) -- E1058 (perform)
  let n = handle Lib.twice () with
    | ctl Lib.ping () k -> k 0                       -- E1058 (handler の節)
    | return x -> x
  println (show n)

-- abstract_field/Lib.em
pub abstract data Config = | Config { name : String, size : Int }
pub config : String -> Config
config n = Config { name = n, size = 1 }
pub name_of : Config -> String
name_of c = c.name

-- abstract_field/main.em
import Lib
main : Unit -> <IO> Unit
main () =
  let c = Lib.config "a"
  println c.name                         -- E2021 (射影)
  let d = { c | name = "x", size = 2 }   -- E2021 (更新。フィールドが2つでも1つ)
  println (Lib.name_of d)

-- abstract_misplaced.em
abstract data T = | T                    -- E0011 (`pub` がない)
pub abstract f : Int -> Int              -- E0011 (シグネチャ)
f n = n
pub abstract extern data U               -- E0011 (`extern`)。E1033 は今と同じ
abstract pub data V = | V                -- E0011 (`abstract pub` の順)
pub abstract type W = Int                -- E0011 (`type`)。E0004 は今と同じで、E0011 の後に出る
main : Unit -> <IO> Unit
main () = ()

-- abstract_api.em
data Secret = | Secret Int
pub abstract data Sealed = | Sealed Secret
seal : Int -> Sealed
seal n = Sealed (Secret n)
unseal : Sealed -> Int
unseal (Sealed (Secret n)) = n
pub abstract data Counter = | Counter Int
  deriving (Show)
main : Unit -> <IO> Unit
main () =
  println (show (unseal (seal 41) + 1))
  println (show (Counter 3))

-- async_op_handled.em
import Async
main : Unit -> <IO, Panic> Unit
main () =
  Async.run (fn () ->
    let t = Async.start (fn () -> 1)
    let x = handle Async.await t with
      | ctl Async.await_cell c k -> k Async.Woke  -- E1058
      | ctl Async.reschedule () k -> k Async.Woke -- E1058
      | return x -> x
    println (show x))

-- abstract_std_constructor.em
import Map
main : Unit -> <IO> Unit
main () =
  match Map.insert 1 2 Map.empty with
    | Map.Empty -> println "empty"               -- E1058
    | _ -> println "node"
```

`abstract_misplaced.em` の `pub abstract extern data U` は std の外の `extern` なので、E0011 のほかに今と同じ E1033 を受ける。`pub abstract type W = Int` も今と同じ E0004 を受ける。`abstract` が足す誤りは、どの行でも E0011 の1つだけである。

### REPL のトランスクリプト `crates/eml_cli/tests/repl/declarations/abstract_in_session/` (T-9c)

`main.repl` と、import する `Lib.em` を置く。セッションの入力どうしは同じモジュールなので隠さず、import したファイルのモジュールの抽象型は隠すことを確かめる。

- `Lib.em`: `pub abstract data Sealed = | Sealed Int`、`pub make : Int -> Sealed`、`pub peek : Sealed -> Int`
- 入力1: `pub abstract data Box = | Box Int` → 宣言が通る
- 入力2: `match Box 1 with | Box n -> n` → `1` (セッションの入力は同じモジュールなので、コンストラクタが見える)
- 入力3: `import Lib` → 通る
- 入力4: `let Lib.Sealed n = Lib.make 1` → E1058
- 入力5: `Lib.peek (Lib.make 1)` → `1`
