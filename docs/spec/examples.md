# プログラム例

位置づけ: 説明。

本番の構文で書いたプログラム例を集める。標準ライブラリの API は [標準ライブラリ](stdlib.md) が定める。今の実装で動く例は `tests/ui/run/scripts/` の5本 (`wc`、`grep`、`logstat`、`csv2jsonl`、`deploy`) にある。ここの例には、今の std にない関数とモジュール (`Fs.read_lines`、`Option`、`Proc.spawn` など) や、実装していない構文 (コマンドリテラル) を使うものがあり、構文を示すためのものである。

## ログの集計 (全体)

import、リストのパターン、補間、複数行の文字列、名前付きのレコードと、実装していないコマンドリテラルを含む。

```haskell
#!/usr/bin/env eml
-- | ログを集計して、レポートを書く
import Env
import Fs
import List (filter, filter_map, length, map)
import String
import Report.Format (render, Style(..))

data Level =
  | Info
  | Warn
  | Error

data Entry =
  | Entry { level : Level, msg : String }

effect Fail where
  never fail : String -> a

parse_entry : String -> Option Entry
parse_entry line =
  match String.split_once " " line with
    | Some ("INFO", msg)  -> Some Entry { level = Info, msg }
    | Some ("WARN", msg)  -> Some Entry { level = Warn, msg }
    | Some ("ERROR", msg) -> Some Entry { level = Error, msg }
    | _ -> None

is_error : Level -> Bool
is_error Error = True
is_error _ = False

try : (Unit -> <Fail | e> a) -> <IO | e> Option a
try action =
  handle action () with
    | ctl fail msg ->
        eprintln "error: \{msg}"
        None
    | return x -> Some x

summarize : String -> <IO, Fail, Panic> Unit
summarize path =
  let entries = Fs.read_lines path |> filter_map parse_entry
  let errors = entries |> filter (fn e -> is_error e.level) |> map (.msg)
  if length errors > 10 then fail "too many errors"
  let commit = read `git rev-parse --short HEAD`
  Fs.write_text "report.md" """
    # Report for \{commit}
    \{render Markdown errors}
    """

main : Unit -> <IO, Panic> Unit
main () =
  match Env.args () with
    | [path] ->
        match try (fn () -> summarize path) with
          | Some () -> println "done"
          | None -> exit 1
    | _ -> eprintln "usage: summarize LOG"
```

## grep (線形なファイル)

補間を含む。

```haskell
import FileIn
import List (each, filter)
import String (lines)

grep : String -> String -> <IO, Panic> Unit
grep pat path =
  let f = FileIn.open path
  let (text, f) = FileIn.read_all f
  FileIn.close f
  lines text
    |> filter (String.contains pat)
    |> each (fn line -> println "match: \{line}")
```

## 状態 (パラメータ付き handler)

```haskell
effect State s where
  get : Unit -> s
  put : s -> Unit

run_state : s -> (Unit -> <State s | e> a) -> <e> (a, s)
run_state init action =
  handle action () from init with
    | fn get () st -> (st, st)
    | fn put st2 _ -> ((), st2)
    | return x st -> (x, st)

counter : Unit -> <State Int, Panic> Int
counter () =
  let n = get ()
  put (n + 1)
  n
```

`put` の節で `_` により古い状態を捨てるので、`s` に `Unr` の制約が付く ([線形性](linearity.md))。

## デプロイ (エフェクト、`use`、コマンドリテラル)

リストのリテラル、補間と、実装していないコマンドリテラルを含む。

`Fail` と `try` は、上のログの集計の例と同じものを使う。

```haskell
import Env
import Option

effect Ask where
  ask : String -> String

with_env : (Unit -> <Ask, IO | e> a) -> <IO | e> a
with_env action =
  handle action () with
    | fn ask key -> Option.default "" (Env.get key)

deploy : List String -> <Ask, Fail, IO> Unit
deploy files =
  let host = ask "HOST"
  if host == "" then fail "HOST is not set"
  let dest = "\{host}:/srv/app"
  let code = run `rsync -a --delete \{..files} \{dest}`
  if code != 0 then fail "rsync failed"

main : Unit -> <IO> Unit
main () =
  use with_env
  match try (fn () -> deploy ["dist/app", "dist/assets"]) with
    | Some () -> println "deployed"
    | None -> exit 1
```

## 子プロセスとパイプ (線形なハンドルの束)

名前付きのレコードと、実装していないコマンドリテラルを含む。

```haskell
data Child =
  | Child { proc : Process, stdin : Stdin, stdout : Stdout }

sort_lines : List String -> <IO> String
sort_lines xs =
  let Child { proc, stdin, stdout } = Proc.spawn `sort`
  let stdin = Proc.write stdin (String.join "\n" xs)
  Proc.close_stdin stdin
  let (stdout, out) = Proc.read_all stdout
  Proc.close_stdout stdout
  Proc.wait proc
  out
```

## データ変換 (名前付きのレコード)

リストのパターン、補間、名前付きのレコードを含む。

```haskell
data Person =
  | Person { name : String, age : Int, email : Option String }

parse_row : List String -> Option Person
parse_row cols =
  match cols with
    | [name, age, email] -> Some Person { name, age = parse_int age, email = non_empty email }
    | _ -> None

adults : List Person -> List String
adults people =
  people
    |> filter (fn p -> p.age >= 18)
    |> map (fn p -> "\{p.name} <\{Option.default "-" p.email}>")
```
