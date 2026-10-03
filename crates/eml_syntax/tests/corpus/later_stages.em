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
