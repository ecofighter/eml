-- Multi-line strings drop the indentation of the closing `"""`, the first and last line breaks, and keep deeper
-- indentation, blank lines, escapes and `\"""`. A hole works as in a single-line string.
main : Unit -> <IO> Unit
main () =
  println """
    first
      deeper

    tab\there \"""quoted\"""
    """
  println """
    """
  let name = "eml"
  println """
    Hello, \{name}!
      indented line
    """
  println "after"
