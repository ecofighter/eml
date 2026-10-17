-- E3006: a file left open before `exit` is dropped just before the call, so it is held across the earlier `multi` operation.
effect Choose where
  multi choose : Unit -> Bool

run : Unit -> <Choose, IO> Unit
run () =
  let f = Fs.open "input.txt"
  let b = choose ()
  exit 2

main : Unit -> <IO> Unit
main () = ()
