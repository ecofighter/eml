-- pipe でやりとりする子 (docs/spec/stdlib.md の「`Proc`」)。eml は1度に1本の pipe しか読み書きせず、pipe の buffer は
-- OS が持つ (ふつう 64 KiB)。詰まらないように、次の規則で読み書きする
-- - 入力を読み終えてから出力を書く子 (`sort`、`wc`) には、書き終えて `close_stdin` してから読む。入力の量に上限はない
-- - 読みながら書く子 (`cat`、`tr`、`grep`) に buffer より多く書いてから読むと、子は出力を書けず、eml は入力を書けず、
--   両方が止まる。そういう子には buffer より少なく書くか、入力を分けて書くたびに読む
-- - `wait` の前に標準出力を読み切る。読まずに `wait` すると、buffer より多く出力する子で止まる
-- - 止まったときは Ctrl-C で終われる。処理系はこの状態を検出しない
import Proc
import Bytes

feed : Int -> Proc.Child -> <IO, Panic> Proc.Child
feed n c = if n == 0 then c else feed (n - 1) (Proc.write_line "line \{n}" c)

show_lines : Proc.Child -> <IO, Panic> Proc.Child
show_lines c =
  match Proc.read_line c with
    | (None, c) -> c
    | (Some line, c) ->
      println line
      show_lines c

main : Unit -> <IO, Panic> Unit
main () =
  let c = feed 10 (Proc.spawn `sort`)
  let c = show_lines (Proc.close_stdin c)
  println "sort \{Proc.wait c}"
  -- `cat` には buffer より少なく書くので、書いてから読んでも詰まらない
  let c = Proc.write "a " (Proc.spawn `cat`)
  let c = Proc.write_bytes (Bytes.of_string "b") c
  let c = Proc.close_stdin c
  let (bytes, c) = Proc.read_chunk (Bytes.empty ()) c
  let (rest, c) = Proc.read_all_bytes c
  println "\{show (Bytes.append bytes rest)}"
  println "cat \{Proc.wait c}"
  -- `merge_stderr` は子の標準エラーも標準出力の pipe に入れる
  let c = Proc.spawn (Proc.merge_stderr `sh -c 'read x; echo "got $x"; echo oops >&2'`)
  let c = Proc.write_line "hello" c
  let (first, c) = Proc.read_line_bytes c
  let (all, c) = Proc.read_all c
  println "\{show first} \{show all}"
  println "sh \{Proc.wait c}"
