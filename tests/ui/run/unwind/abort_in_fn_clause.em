-- 状態が `File` の handler の `fn` 節が、状態を取り出したまま外側の `Fail.to_result` へ中断する。`File` は
-- 中断の値に載せて外側へ渡し、受け取った側が閉じる。巻き戻しは取り出し中の状態に手を触れない。
import Fail (Fail(..))

effect Probe where
  probe : Int -> Int

body : Unit -> <Probe, Fail Fs.File, Panic> Int
body () = probe 1 + probe 2

inner : Unit -> <Fail Fs.File, IO, Panic> Int
inner () =
  handle body () from Fs.open "input.txt" with
    | fn probe n f -> if n == 2 then Fail.fail f else (n, f)
    | return x f ->
      Fs.close f
      x

main : Unit -> <IO, Panic> Unit
main () =
  match Fail.to_result inner with
    | Ok n -> println "ok \{n}"
    | Err f ->
      Fs.close f
      println "aborted"
  println "after"
