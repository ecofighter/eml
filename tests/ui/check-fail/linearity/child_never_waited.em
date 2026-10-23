-- `Child` は線形で、`Proc.wait` で待つか `drop` で手放して、ちょうど1回使う (docs/spec/stdlib.md の「`Proc`」)
import Proc

main : Unit -> <IO, Panic> Unit
main () =
  let c = Proc.spawn `cat`
  println "never waited"
