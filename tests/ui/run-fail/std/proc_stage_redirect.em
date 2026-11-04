-- 最後でない段の標準出力のリダイレクトは、子を起動せずに起動の誤りにする (docs/spec/stdlib.md の「`Proc`」)
import Proc ((.|))

main : Unit -> <IO, Panic> Unit
main () =
  println "exit \{Proc.run (Proc.stdout_file "x" `eml-first` .| `eml-second`)}"
