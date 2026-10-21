-- wc: 行、語、バイトの数を数える。`wc [-l] [-w] [-c] [FILE...]` で、ファイル名 `-` と、ファイルを
-- 書かないときは標準入力を読む。数える部分はエフェクトを使わずに書く
data Shown =
  | Shown { lines : Bool, words : Bool, bytes : Bool }
  deriving (Eq)

data Counts =
  | Counts { lines : Int, words : Int, bytes : Int }

-- 先頭のフラグを読み、残りをファイル名として返す。フラグの位置の知らない `-x` は使い方の誤りである
parse_args : Shown -> List String -> <Panic> Option (Shown, List String)
parse_args shown args =
  match args with
    | "-l" :: rest -> parse_args { shown | lines = True } rest
    | "-w" :: rest -> parse_args { shown | words = True } rest
    | "-c" :: rest -> parse_args { shown | bytes = True } rest
    | first :: _ ->
      if String.starts_with "-" first && first != "-" then None else Some (shown, args)
    | Nil -> Some (shown, args)

-- 語の区切りは ASCII の空白 (`String.trim` と同じ6種) である
is_space : Int -> Bool
is_space b = b == 32 || (b >= 9 && b <= 13)

count : String -> <Panic> Counts
count text =
  let n = String.byte_length text
  let go i lines words in_word =
    if i >= n then
      Counts { lines = lines, words = if in_word then words + 1 else words, bytes = n }
    else
      let b = String.byte_at i text
      if is_space b then
        go (i + 1) (if b == 10 then lines + 1 else lines) (if in_word then words + 1 else words) False
      else go (i + 1) lines words True
  go 0 0 0 False

add : Counts -> Counts -> <Panic> Counts
add a b = Counts { lines = a.lines + b.lines, words = a.words + b.words, bytes = a.bytes + b.bytes }

-- 選んだ数を行、語、バイトの順に並べ、名前があれば続ける
format : Shown -> Counts -> Option String -> String
format shown c name =
  let picked = [(shown.lines, c.lines), (shown.words, c.words), (shown.bytes, c.bytes)]
  let fields = List.filter_map (fn (on, n) -> if on then Some (show n) else None) picked
  let named = match name with
    | Some s -> List.append fields [s]
    | None -> fields
  String.join " " named

read_input : String -> <IO> Result IOError String
read_input name =
  use try_io
  if name == "-" then Stdin.read_all ()
  else
    let f = Fs.open name
    let (f, text) = Fs.read_all f
    Fs.close f
    text

-- 名前のある入力を順に数えて出し、合計と、開けない入力があったかを返す
count_files : Shown -> List String -> <IO, Panic> (Counts, Bool)
count_files shown names =
  let go total failed rest =
    match rest with
      | Nil -> (total, failed)
      | name :: more ->
        match read_input name with
          | Ok text ->
            let c = count text
            println (format shown c (Some name))
            go (add total c) failed more
          | Err e ->
            eprintln "wc: \{e}"
            go total True more
  go (Counts { lines = 0, words = 0, bytes = 0 }) False names

main : Unit -> <IO, Panic> Unit
main () =
  let none = Shown { lines = False, words = False, bytes = False }
  match parse_args none (Env.args ()) with
    | None ->
      eprintln "usage: wc [-l] [-w] [-c] [FILE...]"
      exit 2
    | Some (chosen, names) ->
      let shown = if chosen == none then Shown { lines = True, words = True, bytes = True } else chosen
      match names with
        | Nil ->
          match read_input "-" with
            | Ok text -> println (format shown (count text) None)
            | Err e ->
              eprintln "wc: \{e}"
              exit 1
        | _ ->
          let (total, failed) = count_files shown names
          if List.length names >= 2 then println (format shown total (Some "total")) else ()
          if failed then exit 1 else ()
