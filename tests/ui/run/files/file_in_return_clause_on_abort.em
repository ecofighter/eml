-- A file captured by the `return` clause lives in the clause's closure, which `prompt$` holds and `resume$` captures
-- with the continuation. Operation clauses can capture only unrestricted values, so this is the only way a handler holds
-- a file. Aborting the body drops the continuation and the closure, which releases the file.
effect Fail where
  never fail : Unit -> a

main : Unit -> <IO> Unit
main () =
  let f = Fs.open "input.txt"
  handle fail () with
    | ctl fail () -> println "aborted"
    | return x -> Fs.close f
