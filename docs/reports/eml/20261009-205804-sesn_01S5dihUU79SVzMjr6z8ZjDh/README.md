# 測定の再現

- `fib.em` (fib 30)、`loop.em` (1000万回の末尾再帰)、`state.em` (handle 中の100万回の再帰)
- `interp-alloc-hack.diff`: `eml_interp` のコピーに当てた差分 (`Rhs::Extern` の引数を固定長配列に、`Env` の `Vec` を使い回す)。元のコードには適用していない
- 実行: `cargo build --release -p eml_cli` の後、`target/release/eml run <file.em>`
- 実行命令数: `valgrind --tool=callgrind target/release/eml run <fib 22 に書き換えたファイル>`
