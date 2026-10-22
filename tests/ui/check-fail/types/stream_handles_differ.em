-- E2001: the handle of one stream is not the handle of another
import Stderr
import Stdout

main : Unit -> <IO, Panic> Unit
main () =
  let err = Stderr.open ()
  Stdout.close err
