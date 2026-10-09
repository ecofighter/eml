import Shapes (Area(..))

pub data Square = | Square Int

instance Area Square where
  area (Square n) = n * n
