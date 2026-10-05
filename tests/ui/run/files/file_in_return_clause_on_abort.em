-- A file captured by the `return` clause lives in the handler frame. Operation clauses can capture only unrestricted
-- values, so this is the only way a handler frame holds a file. Aborting the body releases the frame and the file.
effect Fail where
  never fail : Unit -> a

main : Unit -> <IO> Unit
main () =
  let f = open "input.txt"
  handle fail () with
    | fail () -> println "aborted"
    | return x -> close f
