-- Multi-line strings drop the indentation of the closing `"""`, the first and last line breaks, and keep deeper
-- indentation, blank lines, escapes and `\"""`.
main : Unit -> <IO> Unit
main () =
  println """
    first
      deeper

    tab\there \"""quoted\"""
    """
  println """
    """
  println "after"
