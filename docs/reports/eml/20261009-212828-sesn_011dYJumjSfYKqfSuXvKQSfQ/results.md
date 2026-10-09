# 実験の記録 (コミット 97277f19 のコピー上。マウントされたコードは変更していない)

環境: サンドボックス、rustc 1.97.0、release ビルド。`probe.diff` が `eml_interp` と `eml_runtime` のコピーへの変更 (計測用)。

## E1 サイズ (`heap.rs` の単体テストをコピーに足して取得)
Value=16 B, ObjRef=8 B, Payload=80 B, Frame=80 B, Object=88 B, Slot=96 B, Closure=32 B

## E2 パイプライン全体の時間 (`eml run`/`eml check`, 中央値)
- `main () = println "hi"` (Prelude 69 行 + Fs 7 行): run 2.43 ms, check 2.34 ms
- 関数 N 個の連鎖 (各 `f_i x = f_{i-1} x + 1`): N=10: 3.7/3.9 ms, 100: 5.6/4.7 ms, 400: 11.1/17.0 ms, 1000: 26.8/37.1 ms (check/run)

## E3 実行時エラー後のヒープ (`EML_PROBE=1`, `fault.em`, `fault2.em`)
- `fault.em` (深さ 1000 の非末尾再帰の底で 1/0): 誤りの後に Frame 1002 個、String 1000 個が残る
- `fault2.em` (底の関数が局所の String を持つ): 誤りの後 Frame 1002, String 1001。`decref(cont)` で Frame と退避値は全部解放されるが String 1 個 (誤りを起こした関数の局所変数) が残る

## E4 割り込み (`EML_INTERRUPT_MS=300`, `Machine::transfer` で AtomicBool を見る)
- `spin.em` (末尾再帰の無限ループ): 0.303 s で停止。停止時のヒープは Frame 2 個で、`decref(cont)` で空になる
- `deepinf.em` (非末尾の無限再帰): 0.422 s で停止。Frame 597,878 個と String 597,876 個。`decref(cont)` で空になる
- 同じプログラムを 1 s 走らせた最大 RSS は約 582 MB

## E5 割り込み確認のコスト (3 回ずつ交互の中央値、ノイズを含む)
- fib 30: 変更なし 1.033 s / 確認あり 1.048 s
- loop 1000 万回: 変更なし 2.896 s / 確認あり 2.822 s

## E6 トップレベルの値 (`caf.em`、`dump_example.rs` を `eml_cli/examples/dump.rs` に置いて Core IR を表示)
`x : Int` / `x = 1 + 2` を 2 回使うと、Core IR は `call x()` を 2 回出す (`fn x() -> int` は引数 0 の関数)。トップレベルの値は評価結果を覚えず、参照のたびに再評価される。
