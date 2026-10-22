import String

main : Unit -> <IO, Panic> Unit
main () =
  println (show 'a')
  println (show '\'')
  println (show '"')
  println (show '\\')
  println (show '\n')
  println (show '\t')
  println (show '\0')
  println (show '\u{1}')
  println (show '\u{7f}')
  println (show '{')
  println (show 'é')
  println (show '\u{1F600}')
  println "[\{'x'}]"
  println (show (String.byte_length (display '\u{1F600}')))
  println (show (String.byte_length (display 'é')))
  println (show (Some 'a'))
  println (show ['a', 'b'])
