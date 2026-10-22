-- A file held as the state of an inner handler is released when an outer `never` operation aborts the body.
import FileIn

effect Fail where
  never fail : Unit -> a

effect Tick where
  tick : Unit -> Unit

body : Unit -> <Tick, Fail> Unit
body () =
  tick ()
  fail ()

inner : Unit -> <Fail, IO> Unit
inner () =
  handle body () from FileIn.open "input.txt" with
    | ctl tick () k f -> k () f
    | return x f -> FileIn.close f

main : Unit -> <IO> Unit
main () =
  handle inner () with
    | ctl fail () -> println "aborted"
