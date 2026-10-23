-- `Proc.capture` の誤りは、起動、読み、終了コードの順に出る (docs/spec/stdlib.md の「`Proc`」)。UTF-8 でない出力を
-- 出して 0 でなく終わった子は、読みの誤りで止まる
import Proc

main : Unit -> <IO, Panic> Unit
main () =
  println "before"
  println (Proc.capture `sh -c 'printf "\\377"; exit 1'`)
