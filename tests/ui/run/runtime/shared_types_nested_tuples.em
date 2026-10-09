-- 各タプルは前のタプルを2回持つので、型の木の大きさが `let` ごとに2倍になる。型の Kind の境界を集める処理は、
-- 共有する部分を1回だけ訪れなければならない。
ident : a -> a
ident x = x

main : Unit -> <IO> Unit
main () =
  let p0 = 1
  let p1 = (p0, p0)
  let p2 = (p1, p1)
  let p3 = (p2, p2)
  let p4 = (p3, p3)
  let p5 = (p4, p4)
  let p6 = (p5, p5)
  let p7 = (p6, p6)
  let p8 = (p7, p7)
  let p9 = (p8, p8)
  let p10 = (p9, p9)
  let p11 = (p10, p10)
  let p12 = (p11, p11)
  let p13 = (p12, p12)
  let p14 = (p13, p13)
  let p15 = (p14, p14)
  let p16 = (p15, p15)
  let p17 = (p16, p16)
  let p18 = (p17, p17)
  let p19 = (p18, p18)
  let p20 = (p19, p19)
  let p21 = (p20, p20)
  let p22 = (p21, p21)
  let p23 = (p22, p22)
  let p24 = (p23, p23)
  let p25 = (p24, p24)
  let p26 = (p25, p25)
  let p27 = (p26, p26)
  let p28 = (p27, p27)
  let p29 = (p28, p28)
  let p30 = (p29, p29)
  let p31 = (p30, p30)
  let p32 = (p31, p31)
  let p33 = (p32, p32)
  let p34 = (p33, p33)
  let p35 = (p34, p34)
  let p36 = (p35, p35)
  let p37 = (p36, p36)
  let p38 = (p37, p37)
  let p39 = (p38, p38)
  let p40 = (p39, p39)
  println "done"
