-- 状態が `File` の handler の、その場で動く `fn` 節が、`exit 6` を呼ぶ補助の関数を呼ぶ。巻き戻しは節の入口の
-- フレームが取り出した `File` を閉じ、`Finish` まで届く。
effect Probe where
  probe : Int -> Int

stop : Int -> <IO> Int
stop n =
  println "stop at \{n}"
  exit 6

body : Unit -> <Probe, Panic> Int
body () = probe 1 + probe 2

main : Unit -> <IO, Panic> Unit
main () =
  let r =
    handle body () from Fs.open "input.txt" with
      | fn probe n f -> if n == 2 then (stop n, f) else (n, f)
      | return x f ->
        Fs.close f
        x
  println "never \{r}"
