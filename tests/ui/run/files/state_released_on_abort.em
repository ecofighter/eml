-- A file held as the state of an inner handler is released when an outer `never` operation aborts the body.
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
  handle body () from open "input.txt" with
    | tick () k f -> k () f
    | return x f -> close f

main : Unit -> <IO> Unit
main () =
  handle inner () with
    | fail () -> println "aborted"
