-- E0004: command literals come with the standard library's `Proc`. The literal is reported once, and its holes are
-- not checked.
list : Unit -> Int
list () =
  let c = `ls -l \{undefined_name}`
  0
