-- `<IO | e>` に `<IO>` が入っても、row の `IO` は1つとして扱う。`try` と `with_env` で包んだ deploy の形が通る。
data Option a =
  | None
  | Some a

effect Fail where
  never fail : String -> a

effect Ask where
  ask : String -> String

try : (Unit -> <Fail | e> a) -> <IO | e> Option a
try action =
  handle action () with
    | fail msg ->
        println ("error: " ++ msg)
        None
    | return x -> Some x

with_env : (Unit -> <Ask, IO | e> a) -> <IO | e> a
with_env action =
  handle action () with
    | ask key k -> k "host"

deploy : Unit -> <Ask, Fail, IO> Unit
deploy () =
  let host = ask "HOST"
  if host == "" then fail "HOST is not set"
  println host

main : Unit -> <IO> Unit
main () =
  use with_env
  match try (fn () -> deploy ()) with
    | Some () -> println "deployed"
    | None -> println "failed"
