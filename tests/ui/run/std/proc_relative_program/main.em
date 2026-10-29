-- windows: skip (shebang のスクリプトを起動する)
-- `/` を含む相対のプログラムは、子の作業ディレクトリに対して解決する (docs/spec/stdlib.md の「`Proc`」)
import Proc

main : Unit -> <IO, Panic> Unit
main () = println (show (Proc.capture (Proc.cwd "sub" `./run.sh`)))
