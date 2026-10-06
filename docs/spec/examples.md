# プログラム例

位置づけ: 説明。

本番の構文で書いたプログラム例を集める。構文を決めるときに、議論しながら書いたものである。どの構文がどのマイルストーンで実装されるかは [ロードマップ](../future/roadmap.md) の「マイルストーンの列」にある。例に出てくる標準ライブラリの関数 (`Fs.read_lines`、`Proc.spawn` など) は、構文を示すためのものである。標準ライブラリの API はまだ設計していない ([標準ライブラリへの申し送り](../future/stdlib.md))。

## ログの集計 (全体)

import (M2)、名前付きのレコード、リストのパターン、補間、複数行の文字列 (M3) とコマンドリテラル (M9) を含む。

```haskell
#!/usr/bin/env eml run
-- | ログを集計して、レポートを書く
import Report.Format (render, Style(..))

data Level =
  | Info
  | Warn
  | Error

type Entry = { level : Level, msg : String }

effect Fail where
  never fail : String -> a

parse_entry : String -> Option Entry
parse_entry line =
  match String.split_once " " line with
    | Some ("INFO", msg)  -> Some { level = Info, msg }
    | Some ("WARN", msg)  -> Some { level = Warn, msg }
    | Some ("ERROR", msg) -> Some { level = Error, msg }
    | _ -> None

is_error : Level -> Bool
is_error Error = True
is_error _ = False

try : (Unit -> <Fail | e> a) -> <IO | e> Option a
try action =
  handle action () with
    | fail msg ->
        eprintln "error: \{msg}"
        None
    | return x -> Some x

summarize : String -> <IO, Fail> Unit
summarize path =
  let entries = Fs.read_lines path |> filter_map parse_entry
  let errors = entries |> filter (fn e -> is_error e.level) |> map (.msg)
  if length errors > 10 then fail "too many errors"
  let commit = read `git rev-parse --short HEAD`
  Fs.write_text "report.md" """
    # Report for \{commit}
    \{render Markdown errors}
    """

main : Unit -> <IO> Unit
main () =
  match Env.args () with
    | [path] ->
        match try (fn () -> summarize path) with
          | Some () -> println "done"
          | None -> exit 1
    | _ -> eprintln "usage: summarize LOG"
```

## grep (線形なファイル)

補間 (M3) を含む。

```haskell
grep : String -> String -> <IO> Unit
grep pat path =
  let f = open path
  let (f, text) = read_all f
  close f
  lines text
    |> filter (String.contains pat)
    |> each fn line -> println "match: \{line}"
```

## 状態 (パラメータ付き handler)

```haskell
effect State s where
  get : Unit -> s
  put : s -> Unit

run_state : s -> (Unit -> <State s | e> a) -> <e> (a, s)
run_state init action =
  handle action () from init with
    | get () k st -> resume k st st
    | put st2 k _ -> resume k () st2
    | return x st -> (x, st)

counter : Unit -> <State Int> Int
counter () =
  let n = get ()
  put (n + 1)
  n
```

`put` の節で `_` により古い状態を捨てるので、`s` に `Unr` の制約が付く ([線形性](linearity.md))。

## デプロイ (エフェクト、`use`、コマンドリテラル)

リストのリテラルと補間 (M3)、コマンドリテラル (M9) を含む。

`Fail` と `try` は、上のログの集計の例と同じものを使う。

```haskell
effect Ask where
  ask : String -> String

with_env : (Unit -> <Ask, IO | e> a) -> <IO | e> a
with_env action =
  handle action () with
    | ask key k -> resume k (Option.default "" (Env.get key))

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

名前付きのレコード (M3) とコマンドリテラル (M9) を含む。

```haskell
type Child = { proc : Process, stdin : Stdin, stdout : Stdout }

sort_lines : List String -> <IO> String
sort_lines xs =
  let { proc, stdin, stdout } = Proc.spawn `sort`
  let stdin = Proc.write stdin (String.join "\n" xs)
  Proc.close_stdin stdin
  let (stdout, out) = Proc.read_all stdout
  Proc.close_stdout stdout
  Proc.wait proc
  out
```

## データ変換 (構造的レコード)

名前付きのレコード、リストのパターン、補間 (M3) を含む。

```haskell
type Person = { name : String, age : Int, email : Option String }

parse_row : List String -> Option Person
parse_row cols =
  match cols with
    | [name, age, email] -> Some { name, age = parse_int age, email = non_empty email }
    | _ -> None

adults : List Person -> List String
adults people =
  people
    |> filter (fn p -> p.age >= 18)
    |> map (fn p -> "\{p.name} <\{Option.default "-" p.email}>")
```
