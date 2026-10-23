-- A command literal builds a `Cmd` from its words without running anything. Quotes and holes join a word, `''` is an
-- empty argument, and `\{..xs}` spreads a list into separate arguments.
main : Unit -> <IO> Unit
main () =
  let dir = "out dir"
  let xs = ["a b", "c"]
  let none : List String = []
  let c = `git log -n 3 --out=\{dir} '' 'x y'z \{..xs} end \{..none}`
  println (show c)
  println (display c)
  println (show c.args)
  println (show (`ls` == `ls`))
  println (show (`ls` == `ls -l`))
  let prog = "echo"
  println (show `\{prog} \{..xs}`)
