-- E1001: the holes of a command literal are ordinary expressions and resolve names as usual.
f : Unit -> Cmd
f () = `ls -l \{undefined_name} \{..other_undefined}`
