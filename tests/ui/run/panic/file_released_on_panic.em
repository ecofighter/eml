-- `File` を持つフレームを通ってゼロ除算を `try_panic` が捕まえる。巻き戻しが途中のフレームの `File` を閉じるので、
-- 検査モードの漏れの検査も通る。
deep : Int -> Int -> <IO, Panic> Int
deep d n =
  let f = Fs.open "input.txt"
  let r = if n == 0 then 100 / d else deep d (n - 1) + 1
  Fs.close f
  r

main : Unit -> <IO> Unit
main () =
  Fs.write_text "input.txt" "x\n"
  match try_panic (fn () -> deep 0 3) with
    | Ok r -> println "ok \{r}"
    | Err e -> println "caught: \{e}"
  match try_panic (fn () -> deep 4 3) with
    | Ok r -> println "ok \{r}"
    | Err e -> println "caught: \{e}"
