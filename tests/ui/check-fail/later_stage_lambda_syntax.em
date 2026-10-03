-- E0004: lambda parameters and expressions that later stages implement.
main : Unit -> <IO> Unit
main () =
  let swap = fn (a, b) -> (b, a)
  let plus = (+)
  ()
