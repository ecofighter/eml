-- E0014: the forms of a multi-line string. The content must start on the line after the opening `"""`, only spaces
-- may precede the closing `"""`, and every line is indented at least as deep as the closing `"""`.
one_line : Unit -> String
one_line () = """oops"""

opening_text : Unit -> String
opening_text () = """ text
  body
  """

short_line : Unit -> String
short_line () =
  """
    fine
  short
    """
