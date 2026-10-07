-- A file kept alive across a `never` operation is released when the operation aborts the handled body.
effect Fail where
  never fail : Unit -> a

checked : Fs.File -> <Fail, IO> Unit
checked f =
  fail ()
  Fs.close f

main : Unit -> <IO> Unit
main () =
  handle checked (Fs.open "input.txt") with
    | fail () -> println "aborted"
