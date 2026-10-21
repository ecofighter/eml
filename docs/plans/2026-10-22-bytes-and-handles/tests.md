# 付録: ハンドルのテストの本文と期待する出力

本文の計画 (`docs/plans/2026-10-22-bytes-and-handles.md`) の T-23〜T-25 が足すテストと、書き換えるテストの正確な形である。どれも排他的なハンドル (D-8) の形で書いてある。列の番号は、ここに書いた本文のとおりにファイルを書いたときのものである。

## `std/Stdin.em` の全文 (T-24)

REPL の transcript の位置 `<std>/Stdin.em:17:19` は、このとおりに書いたときの `read_all` の呼び出しである。

```haskell
-- 標準入力のハンドル (docs/spec/stdlib.md の「`Stdin`」)。
import String
import Bytes

pub extern data Stdin
pub extern open : Unit -> <IO, Panic> Stdin
pub extern close : Stdin -> <IO> Unit
pub extern read_chunk : Bytes -> Stdin -> <IO> (Bytes, Stdin)
pub extern read_all_bytes : Stdin -> <IO> (Bytes, Stdin)
pub extern read_all : Stdin -> <IO> (String, Stdin)
-- 行の終わりを含む1行で、終わりなら空である
extern read_line_bytes_raw : Stdin -> <IO> (Bytes, Stdin)
extern read_line_raw : Stdin -> <IO> (String, Stdin)

pub read_text : Unit -> <IO, Panic> String
read_text () =
  let (text, h) = read_all (open ())
  close h
  text

pub read_line : Stdin -> <IO, Panic> (Option String, Stdin)
read_line h =
  let (line, h) = read_line_raw h
  if String.byte_length line == 0 then (None, h) else (Some (String.trim_line_end line), h)

pub read_line_bytes : Stdin -> <IO, Panic> (Option Bytes, Stdin)
read_line_bytes h =
  let (line, h) = read_line_bytes_raw h
  if Bytes.length line == 0 then (None, h) else (Some (Bytes.trim_line_end line), h)
```

## T-23: `tests/ui/run/files/read_bytes.em`

fixture `tests/ui/run/files/not_utf8.txt` はバイト `ff fe` の2バイトである。`Fs.write_text` は `CaptureHost` のメモリに書き、ディスクには書かない (前例は `tests/ui/run/std/files.em`)。

```haskell
-- `Bytes` reads accept any bytes; `read_chunk` appends to the given `Bytes`
main : Unit -> <IO, Panic> Unit
main () =
  let (b, f) = Fs.read_all_bytes (Fs.open "not_utf8.txt")
  Fs.close f
  println "all \{Bytes.length b}"
  Fs.write_text "lines.txt" "ab\r\nc\n"
  let f = Fs.open "lines.txt"
  let (l, f) = Fs.read_line_bytes f
  match l with
    | Some x -> println "line \{Bytes.length x}"
    | None -> println "none"
  let (rest, f) = Fs.read_chunk (Bytes.of_string ">") f
  println (show (String.from_utf8 rest))
  let (more, f) = Fs.read_chunk (Bytes.empty ()) f
  println "more \{Bytes.length more}"
  Fs.close f
```

期待する stdout はちょうど次の4行で、stderr は空である。3行目の `\n` は、逆斜線と `n` の2文字である。

```
all 2
line 2
Some ">c\n"
more 0
```

## T-24: `tests/ui/run-fail/std/stdin_not_utf8.em` (書き換え、種類 2)

```haskell
main : Unit -> <IO, Panic> Unit
main () =
  let (text, h) = Stdin.read_all (Stdin.open ())
  Stdin.close h
  println text
```

期待値: `--- stdout ---` と `--- stderr ---` は空で、`--- runtime error ---` の後に `the standard input is not valid UTF-8` と `  at run-fail/std/stdin_not_utf8.em:3:19` が続く。

## T-24: `crates/eml_cli/tests/repl/std/script_io.repl` (書き換え、種類 2)

入力は `Env.args ()`、`Stdin.read_text ()`、`Stdin.open ()`、`1 + 1` の4つで、期待する snapshot の本体は次のとおりである。

```
> Env.args ()
[]
> Stdin.read_text ()
runtime error: cannot read the standard input: not available
  at <std>/Stdin.em:17:19
> Stdin.open ()
[E2016] Error: this input leaves a value of the linear type `Stdin`
   ╭─[ <input 3>:1:1 ]
   │
 1 │ Stdin.open ()
   │ ──────┬──────  
   │       ╰──────── a linear value
   │ 
   │ Help: use up the value inside the input, as in `drop`
───╯
> 1 + 1
2
--- close ---
no leaks
```

## T-24: `tests/ui/run/io/stdin_handles.em`

stdin の fixture `stdin_handles.stdin` はバイト列 `a\r\nbc\nd\nrest` (最後に改行がない) である。2つのハンドルは順に開く。

```haskell
-- A `Stdin` handle and a later one read one buffer in program order; `Bytes` and `String` reads mix
main : Unit -> <IO, Panic> Unit
main () =
  let h = Stdin.open ()
  let (first, h) = Stdin.read_line h
  println (show first)
  Stdin.close h
  let g = Stdin.open ()
  let (second, g) = Stdin.read_line_bytes g
  match second with
    | Some b -> println "bytes \{Bytes.length b}"
    | None -> println "none"
  let (rest, g) = Stdin.read_chunk (Bytes.of_string ">") g
  println (show (String.from_utf8 rest))
  let (more, g) = Stdin.read_chunk (Bytes.empty ()) g
  println "more \{Bytes.length more}"
  let (text, g) = Stdin.read_all g
  println (show text)
  let (last, g) = Stdin.read_line g
  println (show last)
  Stdin.close g
```

期待する stdout は次の6行で、stderr は空である。3行目の `\n` は2文字である。`read_chunk` の結果は、14バイトの入力が1回の補充で届くことに依る。`CaptureHost` の `Cursor` がそれを保証する。

```
Some "a"
bytes 2
Some ">d\nrest"
more 0
""
None
```

## T-24: `tests/ui/run-fail/io/stdin_opened_twice.em`

```haskell
main : Unit -> <IO, Panic> Unit
main () =
  let h = Stdin.open ()
  let g = Stdin.open ()
  Stdin.close g
  Stdin.close h
```

期待値: `--- runtime error ---` の後に `the standard input is already open` と `  at run-fail/io/stdin_opened_twice.em:4:11` が続く。

## T-25: `tests/ui/run/io/stdout_handles.em`

```haskell
-- `println` and `Stdout` handles write into one buffer, so the output keeps the program order
main : Unit -> <IO, Panic> Unit
main () =
  let out = Stdout.open ()
  println "one"
  let out = Stdout.write_string "two" out
  let out = Stdout.write (Bytes.of_string ", three\n") out
  let out = out |> Stdout.write_line "four" |> Stdout.flush
  Stdout.close out
  let out = Stdout.open ()
  let out = Stdout.write_line "five" out
  Stdout.close out
  println "six"
  let err = Stderr.open ()
  let err = Stderr.write_string "e" err
  eprintln "1"
  let err = Stderr.write (Bytes.of_string "e2\n") err
  Stderr.close err
```

期待する stdout はバイト列 `one\ntwo, three\nfour\nfive\nsix\n`、stderr は `e1\ne2\n` である。

## T-25: `tests/ui/run/io/try_io_releases_handles.em`

fixture は今ある `tests/ui/run/io/not_utf8.txt` (2バイト) である。

```haskell
-- An IO failure under `try_io` releases the handles on the way: the file whose read failed is closed by the
-- unwinding, and the `Stdout` handle is released without losing what it wrote
read_bad : Stdout.Stdout -> <IO, Panic> Stdout.Stdout
read_bad out =
  let out = Stdout.write_line "before" out
  let (text, f) = Fs.read_all (Fs.open "not_utf8.txt")
  Fs.close f
  Stdout.write_line text out

main : Unit -> <IO, Panic> Unit
main () =
  match try_io (fn () -> Stdout.close (read_bad (Stdout.open ()))) with
    | Ok () -> println "ok"
    | Err e -> println "caught: \{e}"
  let (b, f) = Fs.read_all_bytes (Fs.open "not_utf8.txt")
  Fs.close f
  println "bytes \{Bytes.length b}"
```

期待する stdout は次の3行で、stderr は空である。検査モードの実行は漏れの検査を通る。巻き戻しが `Stdout` の開いた印を下ろすことも、この本文が示す (`main` は `Stdout` をまた開ける)。

```
before
caught: `not_utf8.txt` is not valid UTF-8
bytes 2
```

## T-25: `tests/ui/check-fail/types/stream_handles_differ.em`

```haskell
-- E2001: the handle of one stream is not the handle of another
main : Unit -> <IO, Panic> Unit
main () =
  let err = Stderr.open ()
  Stdout.close err
```

期待値:

```
[E2001] Error: mismatched types
   ╭─[ check-fail/types/stream_handles_differ.em:5:16 ]
   │
 5 │   Stdout.close err
   │   ──────┬───── ─┬─  
   │         ╰─────────── argument 1 of `close`
   │                 │   
   │                 ╰─── expected `Stdout`, found `Stderr`
───╯
```

## T-25: `tests/ui/check-fail/linearity/stdout_not_used.em`

```haskell
-- E3003: a `Stdout` handle must be used
main : Unit -> <IO, Panic> Unit
main () =
  let out = Stdout.open ()
  println "x"
```

期待値:

```
[E3003] Error: `out` must be used exactly once, but it is not used
   ╭─[ check-fail/linearity/stdout_not_used.em:4:7 ]
   │
 4 │   let out = Stdout.open ()
   │       ─┬─  
   │        ╰─── `out` is bound here
 5 │   println "x"
   │              │ 
   │              ╰─ `out` is not used before the end of this scope
   │ 
   │ Help: pass `out` to `drop`
   │ 
   │ Note: linear values, such as files, the continuation of a `once` operation and closures that capture one, must be used exactly once
───╯
```

## T-25: `tests/ui/run-fail/io/stdout_opened_twice.em`

`stdin_opened_twice.em` の `Stdin` を `Stdout` にしたものである。期待値は `the standard output is already open` と `  at run-fail/io/stdout_opened_twice.em:4:11` である。

## 付録 `rust-start/`

`rust-start/` の5つのファイルは、T-1 の Rust の基準の出発点である。試作で測った版で、T-1 の作業 (FILE の引数、フラグ、終了コード) はまだ入っていない。`wcv.rs` は wc の書き方の違いを比べる版で、T-1 の `wc.rs` には、その `one-utf8` の形 (1回の走査で、UTF-8 を確かめる) を使う。
