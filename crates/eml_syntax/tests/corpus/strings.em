greet : String -> Int -> String
greet name n =
  let banner = """
    Hello, \{name}!
      count: \{n} \{if n > 1 then "items" else "item"}
    """
  let path = r#"C:\tmp\"quoted"\x"#
  "\{banner}\{path} \{f "nested \{n}"} \u{1F600}"

shell : String -> Cmd
shell dir = `ls -l \{dir} \{..flags}`
