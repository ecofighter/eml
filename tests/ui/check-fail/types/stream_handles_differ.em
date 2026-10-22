-- E2001: the handle of one stream is not the handle of another
main : Unit -> <IO, Panic> Unit
main () =
  let err = Stderr.open ()
  Stdout.close err
