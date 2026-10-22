-- grep: 固定の文字列を含む行を出す。`grep [-v] [-n] [-c] PATTERN [FILE...]` で、ファイル名 `-` と、
-- ファイルを書かないときは標準入力を1行ずつ読む。行を流す生産者と、`Yield.fold` で絞り込んで出す消費者に分けて書く
import Yield (Yield(..))
import Env
import FileIn
import List
import Stdin
import String

data Options =
  | Options { invert : Bool, numbered : Bool, count_only : Bool, pattern : String }

-- 先頭のフラグとパターンを読み、残りをファイル名として返す
parse_args : Options -> List String -> <Panic> Option (Options, List String)
parse_args opts args =
  match args with
    | "-v" :: rest -> parse_args { opts | invert = True } rest
    | "-n" :: rest -> parse_args { opts | numbered = True } rest
    | "-c" :: rest -> parse_args { opts | count_only = True } rest
    | first :: rest ->
      if String.starts_with "-" first && first != "-" then None
      else Some ({ opts | pattern = first }, rest)
    | Nil -> None

-- 標準入力を、全体を読まずに1行ずつ流す。行番号は1から数える
stdin_lines : Unit -> <Yield (Int, String), IO, Panic> Unit
stdin_lines () =
  let go n h =
    match Stdin.read_line h with
      | (Some line, h) ->
        yield (n, line)
        go (n + 1) h
      | (None, h) -> Stdin.close h
  go 1 (Stdin.open ())

numbered_lines : List String -> <Yield (Int, String), Panic> Unit
numbered_lines lines =
  let go n rest =
    match rest with
      | Nil -> ()
      | line :: more ->
        yield (n, line)
        go (n + 1) more
  go 1 lines

-- 流れてきた行のうち選ぶものを出し、選んだ数を返す
matching : Options -> String -> (Unit -> <Yield (Int, String), IO, Panic | e> Unit) -> <IO, Panic | e> Int
matching opts prefix lines =
  let step count (n, line) =
    if String.contains opts.pattern line != opts.invert then
      if opts.count_only then ()
      else if opts.numbered then println "\{prefix}\{n}:\{line}"
      else println "\{prefix}\{line}"
      count + 1
    else count
  let count = Yield.fold step 0 lines
  if opts.count_only then println "\{prefix}\{count}" else ()
  count

-- 1つの入力を探し、選んだ数を返す。開けないか読めなければ `None` である
search : Options -> String -> String -> <IO, Panic> Option Int
search opts prefix name =
  let found =
    use try_io
    if name == "-" then matching opts prefix stdin_lines
    else
      let f = FileIn.open name
      let (text, f) = FileIn.read_all f
      FileIn.close f
      use matching opts prefix
      numbered_lines (String.lines text)
  match found with
    | Ok n -> Some n
    | Err e ->
      eprintln "grep: \{e}"
      None

main : Unit -> <IO, Panic> Unit
main () =
  let none = Options { invert = False, numbered = False, count_only = False, pattern = "" }
  match parse_args none (Env.args ()) with
    | None ->
      eprintln "usage: grep [-v] [-n] [-c] PATTERN [FILE...]"
      exit 2
    | Some (opts, names) ->
      let inputs = match names with
        | Nil -> ["-"]
        | _ -> names
      let prefixed = List.length inputs >= 2
      -- 一致した行があったかと、開けない入力があったか
      let step (matched, failed) name =
        match search opts (if prefixed then "\{name}:" else "") name with
          | Some n -> (matched || n > 0, failed)
          | None -> (matched, True)
      let (matched, failed) = List.fold step (False, False) inputs
      if failed then exit 2 else if matched then () else exit 1
