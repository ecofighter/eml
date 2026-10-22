-- `<IO>` だけの row の関数は yield しない。`File` を持った段の下で無いファイルを開くと、IO の失敗が `try_io` へ
-- 中断し、巻き戻しが各段の `File` を1回ずつ閉じる。
import Fs

missing : Unit -> <IO> Int
missing () =
  let f = Fs.open "input.txt"
  Fs.close (Fs.open "missing.txt")
  Fs.close f
  0

outer : Unit -> <IO, Panic> Int
outer () =
  let f = Fs.open "input.txt"
  let r = missing () + 1
  Fs.close f
  r

main : Unit -> <IO, Panic> Unit
main () =
  match try_io outer with
    | Ok r -> println "ok \{r}"
    | Err e -> println "outer: \{e}"
