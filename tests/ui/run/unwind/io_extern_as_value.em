-- 値として渡した `FileIn.open` の `$externN` は yield しない。2つ目のファイルが無いと、IO の失敗が `try_io` へ
-- 巻き戻し、途中まで作ったリストの `File` を閉じる。
import FileIn
import List

main : Unit -> <IO> Unit
main () =
  match try_io (fn () -> List.each FileIn.close (List.map FileIn.open ["input.txt", "missing.txt"])) with
    | Ok () -> println "ok"
    | Err e -> println "Err \{e}"
