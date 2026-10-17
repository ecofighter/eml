-- `: Lin` と宣言した型は、フィールドが `Unr` でも `Lin` になる。フィールドのない `Token` (Repr は `enum`) を1回だけ
-- 消費し、`Option` に入れ、`once` の操作をまたいで持つ。ハンドルの `Queue` と `Unr` のフィールドを混ぜたレコードを
-- 分解して組み直し、`Unr` のフィールドだけの `Ledger` を射影と更新で使う。どれも検査モードで漏れずに動く
-- (docs/spec/types.md の「`Lin` の宣言」)。
data Token : Lin =
  | Token

data Queue : Lin =
  | Queue (List Int)

data Worker =
  | Worker { queue : Queue, name : String, count : Int }

data Ledger : Lin =
  | Ledger { entries : List Int, owner : String }

spend : Token -> String
spend t =
  match t with
    | Token -> "token spent"

spend_some : Option Token -> String
spend_some o =
  match o with
    | None -> "nothing to spend"
    | Some t -> spend t

effect Ask where
  ask : Unit -> Int

hold : Token -> <Ask> String
hold t =
  let n = ask ()
  "\{spend t} after \{n}"

push : Int -> Queue -> Queue
push x q =
  match q with
    | Queue xs -> Queue (x :: xs)

contents : Queue -> List Int
contents q =
  match q with
    | Queue xs -> xs

work : Worker -> Worker
work w =
  let Worker { queue, name, count } = w
  Worker { queue = push count queue, name = name ++ "!", count = count + 1 }

retire : Worker -> String
retire w =
  match w with
    | Worker { queue = q, name, count = c } -> "\{name} did \{c}: \{show (contents q)}"

is_empty : Ledger -> Bool
is_empty l =
  match l.entries with
    | [] -> True
    | _ -> False

main : Unit -> <IO> Unit
main () =
  let token = Token
  println (spend token)
  println (spend_some (Some Token))
  println (spend_some None)
  println (handle hold Token with | ctl ask () k -> k 7)
  let dropped =
    handle hold Token with
      | ctl ask () k ->
          drop k
          "continuation holding a token dropped"
  println dropped
  let w = Worker { queue = Queue [], name = "w", count = 0 }
  let w = work (work (work w))
  let w = { w | name = "worker" }
  println (retire w)
  let ledger = { Ledger { entries = [], owner = "a" } | owner = "b" }
  println (show (is_empty ledger))
