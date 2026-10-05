-- E3002: a file is linear, so closing it twice is an error.
main : Unit -> <IO> Unit
main () =
  let f = open "input.txt"
  close f
  close f
