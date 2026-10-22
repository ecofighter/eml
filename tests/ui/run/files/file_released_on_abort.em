-- A file kept alive across a `never` operation is released when the operation aborts the handled body.
import FileIn

effect Fail where
  never fail : Unit -> a

checked : FileIn.FileIn -> <Fail, IO> Unit
checked f =
  fail ()
  FileIn.close f

main : Unit -> <IO> Unit
main () =
  handle checked (FileIn.open "input.txt") with
    | ctl fail () -> println "aborted"
