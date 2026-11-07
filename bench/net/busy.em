-- (e) 計算とI/Oが混ざるrun。同じrunの中の16バイトのechoを`n`往復する2つのタスクと、計算だけを続けるタスク1つを
-- 動かす。計算のタスクは待ちの操作をせず、1歩ごとに呼ぶ`Async.yield_now`でだけ切り替わる。計算のタスクと同じ
-- workerにあるとき、往復が切り替えの間隔をどれだけ待つかを見る (docs/spec/vm.md の「時計と driver」)
-- 引数: n
import Async (Async)
import Async
import Atomic
import Bytes
import Env
import Net
import String

serve : Bytes -> Net.TcpStream -> <IO, Panic, Async> Unit
serve b s =
  let (b, s) = Net.read_chunk (Bytes.truncate 0 b) s
  if Bytes.length b == 0 then Net.close s
  else
    let (b, s) = Net.write_bytes b s
    serve b s

-- `want` バイト集まるまで読む
read_exact : Int -> Bytes -> Net.TcpStream -> <IO, Panic, Async> (Bytes, Net.TcpStream)
read_exact want b s =
  if Bytes.length b >= want then (b, s)
  else
    let (b, s) = Net.read_chunk b s
    read_exact want b s

rounds : Int -> Bytes -> Bytes -> Net.TcpStream -> <IO, Panic, Async> Net.TcpStream
rounds n msg b s =
  if n == 0 then s
  else
    let (msg, s) = Net.write_bytes msg s
    let (b, s) = read_exact 16 (Bytes.truncate 0 b) s
    rounds (n - 1) msg b s

fib : Int -> <Panic> Int
fib k = if k < 2 then k else fib (k - 1) + fib (k - 2)

-- 旗が立つまで計算を続け、回った数を返す
spin : Int -> Atomic.Counter -> <IO, Panic, Async> Int
spin done flag =
  if Atomic.load flag != 0 then done
  else
    let _ = fib 15
    Async.yield_now ()
    spin (done + 1) flag

main : Unit -> <IO, Panic> Unit
main () = Async.run (fn () ->
  let n = int_arg 0 2000
  let flag = Atomic.new 0
  let l = Net.listen "127.0.0.1" 0
  let (port, l) = Net.local_port l
  let server = Async.start (fn () ->
    let (s, l) = Net.accept l
    Net.close_listener l
    serve (Bytes.empty ()) (Net.set_nodelay True s))
  let busy = Async.start (fn () -> spin 0 flag)
  let c = Net.set_nodelay True (Net.connect "127.0.0.1" port)
  Net.close (rounds n (Bytes.make 16 'x'u8) (Bytes.empty ()) c)
  Async.await server
  Atomic.store 1 flag
  let _ = Async.await busy
  println "busy done \{show n}")

-- 引数の読み取り。各プログラムが1ファイルで動くように、同じ定義を写して持つ
int_arg : Int -> Int -> <IO, Panic> Int
int_arg i default =
  match nth i (Env.args ()) with
    | Some s ->
      match String.to_int s with
        | Some n -> n
        | None -> default
    | None -> default

nth : Int -> List String -> <Panic> Option String
nth i xs =
  match xs with
    | Nil -> None
    | x :: rest -> if i == 0 then Some x else nth (i - 1) rest
