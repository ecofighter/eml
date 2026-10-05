-- A file kept alive across a `never` operation is released when the operation aborts the handled body.
effect Fail where
  never fail : Unit -> a

checked : File -> <Fail, IO> Unit
checked f =
  fail ()
  close f

main : Unit -> <IO> Unit
main () =
  handle checked (open "input.txt") with
    | fail () -> println "aborted"
