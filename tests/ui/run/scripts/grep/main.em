-- grep: 固定の文字列を含む行を出す (S12d のスクリプト)。`grep [-v] [-n] [-c] PATTERN [FILE...]` で、ファイル名 `-` と、
-- ファイルを書かないときは標準入力を1行ずつ読む。行を流す生産者と、絞り込んで出す handler に分けて書く
effect Yield a where
  yield : a -> Unit

data Options =
  | Options { invert : Bool, numbered : Bool, count_only : Bool, pattern : String }

-- 先頭のフラグとパターンを読み、残りをファイル名として返す
parse_args : Options -> List String -> Option (Options, List String)
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
stdin_lines : Unit -> <Yield (Int, String), IO> Unit
stdin_lines () =
  let go n =
    match Stdin.read_line () with
      | Some line ->
        yield (n, line)
        go (n + 1)
      | None -> ()
  go 1

numbered_lines : List String -> <Yield (Int, String)> Unit
numbered_lines lines =
  let go n rest =
    match rest with
      | Nil -> ()
      | line :: more ->
        yield (n, line)
        go (n + 1) more
  go 1 lines

-- 流れてきた行のうち選ぶものを出し、選んだ数を返す
matching : Options -> String -> (Unit -> <Yield (Int, String), IO | e> Unit) -> <IO | e> Int
matching opts prefix lines =
  handle lines () from 0 with
    | yield (n, line) k count ->
      if String.contains opts.pattern line != opts.invert then
        if opts.count_only then ()
        else if opts.numbered then println "\{prefix}\{n}:\{line}"
        else println "\{prefix}\{line}"
        k () (count + 1)
      else k () count
    | return _ count ->
      if opts.count_only then println "\{prefix}\{count}" else ()
      count

read_file : String -> <IO> Result IOError String
read_file name =
  use try_io
  let f = Fs.open name
  let (f, text) = Fs.read_all f
  Fs.close f
  text

-- 1つの入力を探し、選んだ数を返す。開けなければ `None` である
search : Options -> String -> String -> <IO> Option Int
search opts prefix name =
  if name == "-" then
    Some (matching opts prefix stdin_lines)
  else
    match read_file name with
      | Ok text ->
        let n =
          use matching opts prefix
          numbered_lines (String.lines text)
        Some n
      | Err e ->
        eprintln "grep: \{e}"
        None

main : Unit -> <IO> Unit
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
