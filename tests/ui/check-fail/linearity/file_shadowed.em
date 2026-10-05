-- E3003: shadowing a file that was not consumed leaves it unconsumed.
main : Unit -> <IO> Unit
main () =
  let f = open "a.txt"
  let f = open "b.txt"
  close f
