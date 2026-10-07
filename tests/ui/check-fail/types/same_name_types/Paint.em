pub data Color =
  | Red
  | Green

pub name : Color -> String
name c = match c with
  | Red -> "red"
  | Green -> "green"
