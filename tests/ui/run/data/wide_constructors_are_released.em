-- 数える欄が64個を超える構築子と、255個以上で数の語を持つ構築子の値を、ほかの参照があるときとないときに手放す。
-- 残す欄は、最初、真ん中、最後の欄の組み合わせである (docs/spec/native.md の「データ」)。
data Wide =
  | Wide String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String

wide : Int -> <Panic> Wide
wide n = Wide (show (n + 0)) (show (n + 1)) (show (n + 2)) (show (n + 3)) (show (n + 4)) (show (n + 5)) (show (n + 6)) (show (n + 7)) (show (n + 8)) (show (n + 9)) (show (n + 10)) (show (n + 11)) (show (n + 12)) (show (n + 13)) (show (n + 14)) (show (n + 15)) (show (n + 16)) (show (n + 17)) (show (n + 18)) (show (n + 19)) (show (n + 20)) (show (n + 21)) (show (n + 22)) (show (n + 23)) (show (n + 24)) (show (n + 25)) (show (n + 26)) (show (n + 27)) (show (n + 28)) (show (n + 29)) (show (n + 30)) (show (n + 31)) (show (n + 32)) (show (n + 33)) (show (n + 34)) (show (n + 35)) (show (n + 36)) (show (n + 37)) (show (n + 38)) (show (n + 39)) (show (n + 40)) (show (n + 41)) (show (n + 42)) (show (n + 43)) (show (n + 44)) (show (n + 45)) (show (n + 46)) (show (n + 47)) (show (n + 48)) (show (n + 49)) (show (n + 50)) (show (n + 51)) (show (n + 52)) (show (n + 53)) (show (n + 54)) (show (n + 55)) (show (n + 56)) (show (n + 57)) (show (n + 58)) (show (n + 59)) (show (n + 60)) (show (n + 61)) (show (n + 62)) (show (n + 63)) (show (n + 64))

wide_first : Int -> Wide -> <Panic> String
wide_first k v =
  if k > 0 then wide_first (k - 1) v else
    match v with
      | Wide x0 _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ -> "first " ++ x0

wide_last : Int -> Wide -> <Panic> String
wide_last k v =
  if k > 0 then wide_last (k - 1) v else
    match v with
      | Wide _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ x64 -> "last " ++ x64

wide_ends : Int -> Wide -> <Panic> String
wide_ends k v =
  if k > 0 then wide_ends (k - 1) v else
    match v with
      | Wide x0 _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ x32 _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ x64 -> "ends " ++ x0 ++ " " ++ x32 ++ " " ++ x64

data Huge =
  | Huge String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String String

huge : Int -> <Panic> Huge
huge n = Huge (show (n + 0)) (show (n + 1)) (show (n + 2)) (show (n + 3)) (show (n + 4)) (show (n + 5)) (show (n + 6)) (show (n + 7)) (show (n + 8)) (show (n + 9)) (show (n + 10)) (show (n + 11)) (show (n + 12)) (show (n + 13)) (show (n + 14)) (show (n + 15)) (show (n + 16)) (show (n + 17)) (show (n + 18)) (show (n + 19)) (show (n + 20)) (show (n + 21)) (show (n + 22)) (show (n + 23)) (show (n + 24)) (show (n + 25)) (show (n + 26)) (show (n + 27)) (show (n + 28)) (show (n + 29)) (show (n + 30)) (show (n + 31)) (show (n + 32)) (show (n + 33)) (show (n + 34)) (show (n + 35)) (show (n + 36)) (show (n + 37)) (show (n + 38)) (show (n + 39)) (show (n + 40)) (show (n + 41)) (show (n + 42)) (show (n + 43)) (show (n + 44)) (show (n + 45)) (show (n + 46)) (show (n + 47)) (show (n + 48)) (show (n + 49)) (show (n + 50)) (show (n + 51)) (show (n + 52)) (show (n + 53)) (show (n + 54)) (show (n + 55)) (show (n + 56)) (show (n + 57)) (show (n + 58)) (show (n + 59)) (show (n + 60)) (show (n + 61)) (show (n + 62)) (show (n + 63)) (show (n + 64)) (show (n + 65)) (show (n + 66)) (show (n + 67)) (show (n + 68)) (show (n + 69)) (show (n + 70)) (show (n + 71)) (show (n + 72)) (show (n + 73)) (show (n + 74)) (show (n + 75)) (show (n + 76)) (show (n + 77)) (show (n + 78)) (show (n + 79)) (show (n + 80)) (show (n + 81)) (show (n + 82)) (show (n + 83)) (show (n + 84)) (show (n + 85)) (show (n + 86)) (show (n + 87)) (show (n + 88)) (show (n + 89)) (show (n + 90)) (show (n + 91)) (show (n + 92)) (show (n + 93)) (show (n + 94)) (show (n + 95)) (show (n + 96)) (show (n + 97)) (show (n + 98)) (show (n + 99)) (show (n + 100)) (show (n + 101)) (show (n + 102)) (show (n + 103)) (show (n + 104)) (show (n + 105)) (show (n + 106)) (show (n + 107)) (show (n + 108)) (show (n + 109)) (show (n + 110)) (show (n + 111)) (show (n + 112)) (show (n + 113)) (show (n + 114)) (show (n + 115)) (show (n + 116)) (show (n + 117)) (show (n + 118)) (show (n + 119)) (show (n + 120)) (show (n + 121)) (show (n + 122)) (show (n + 123)) (show (n + 124)) (show (n + 125)) (show (n + 126)) (show (n + 127)) (show (n + 128)) (show (n + 129)) (show (n + 130)) (show (n + 131)) (show (n + 132)) (show (n + 133)) (show (n + 134)) (show (n + 135)) (show (n + 136)) (show (n + 137)) (show (n + 138)) (show (n + 139)) (show (n + 140)) (show (n + 141)) (show (n + 142)) (show (n + 143)) (show (n + 144)) (show (n + 145)) (show (n + 146)) (show (n + 147)) (show (n + 148)) (show (n + 149)) (show (n + 150)) (show (n + 151)) (show (n + 152)) (show (n + 153)) (show (n + 154)) (show (n + 155)) (show (n + 156)) (show (n + 157)) (show (n + 158)) (show (n + 159)) (show (n + 160)) (show (n + 161)) (show (n + 162)) (show (n + 163)) (show (n + 164)) (show (n + 165)) (show (n + 166)) (show (n + 167)) (show (n + 168)) (show (n + 169)) (show (n + 170)) (show (n + 171)) (show (n + 172)) (show (n + 173)) (show (n + 174)) (show (n + 175)) (show (n + 176)) (show (n + 177)) (show (n + 178)) (show (n + 179)) (show (n + 180)) (show (n + 181)) (show (n + 182)) (show (n + 183)) (show (n + 184)) (show (n + 185)) (show (n + 186)) (show (n + 187)) (show (n + 188)) (show (n + 189)) (show (n + 190)) (show (n + 191)) (show (n + 192)) (show (n + 193)) (show (n + 194)) (show (n + 195)) (show (n + 196)) (show (n + 197)) (show (n + 198)) (show (n + 199)) (show (n + 200)) (show (n + 201)) (show (n + 202)) (show (n + 203)) (show (n + 204)) (show (n + 205)) (show (n + 206)) (show (n + 207)) (show (n + 208)) (show (n + 209)) (show (n + 210)) (show (n + 211)) (show (n + 212)) (show (n + 213)) (show (n + 214)) (show (n + 215)) (show (n + 216)) (show (n + 217)) (show (n + 218)) (show (n + 219)) (show (n + 220)) (show (n + 221)) (show (n + 222)) (show (n + 223)) (show (n + 224)) (show (n + 225)) (show (n + 226)) (show (n + 227)) (show (n + 228)) (show (n + 229)) (show (n + 230)) (show (n + 231)) (show (n + 232)) (show (n + 233)) (show (n + 234)) (show (n + 235)) (show (n + 236)) (show (n + 237)) (show (n + 238)) (show (n + 239)) (show (n + 240)) (show (n + 241)) (show (n + 242)) (show (n + 243)) (show (n + 244)) (show (n + 245)) (show (n + 246)) (show (n + 247)) (show (n + 248)) (show (n + 249)) (show (n + 250)) (show (n + 251)) (show (n + 252)) (show (n + 253)) (show (n + 254)) (show (n + 255))

huge_first : Int -> Huge -> <Panic> String
huge_first k v =
  if k > 0 then huge_first (k - 1) v else
    match v with
      | Huge x0 _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ -> "first " ++ x0

huge_last : Int -> Huge -> <Panic> String
huge_last k v =
  if k > 0 then huge_last (k - 1) v else
    match v with
      | Huge _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ x255 -> "last " ++ x255

huge_ends : Int -> Huge -> <Panic> String
huge_ends k v =
  if k > 0 then huge_ends (k - 1) v else
    match v with
      | Huge x0 _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ x128 _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ x255 -> "ends " ++ x0 ++ " " ++ x128 ++ " " ++ x255

main : Unit -> <IO, Panic> Unit
main () =
  let v = wide 1000
  println (wide_first 1 v)
  println (wide_last 1 v)
  println (wide_ends 1 v)
  println (wide_first 1 (wide 2000))
  println (wide_last 1 (wide 3000))
  println (wide_ends 1 (wide 4000))
  let v = huge 1000
  println (huge_first 1 v)
  println (huge_last 1 v)
  println (huge_ends 1 v)
  println (huge_first 1 (huge 2000))
  println (huge_last 1 (huge 3000))
  println (huge_ends 1 (huge 4000))
