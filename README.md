# eml

eml は「線形型 × 代数的エフェクト」の組み合わせを試す、実験的な関数型言語である。構文は Haskell 流を基本にし、F# からいくつかの書き方を借りる。処理系は Rust で書いている。ソースファイルの拡張子は `.em` である。

まだリリースしておらず、開発中である。構文、API、診断は予告なく変わる。

## 特徴

- 関数型、式指向で、イミュータブルがデフォルトである。
- 線形性を Kind で区別する。線形な値 (ファイルや `once` 操作の継続など) は、ちょうど1回使わなければならない。値を捨てるときは `drop` を明示する。
- エフェクトは Koka 方式の Row 多相で扱う。操作ごとに継続の多重度 (`never` / `once` / `multi`) を宣言する。
- 副作用は組み込みの `IO` エフェクトとして管理する。
- 型付きの Core IR を CEK 機械で実行する。メモリは Perceus 方式の参照カウントで管理する。
- 1回の検査で、構文・名前解決・型・線形性・網羅性の誤りをまとめて報告する。

## 例

ユーザー定義のエフェクトと handler の例を示す。

```
effect Ask where
  ask : Unit -> Int

add_two : Unit -> <Ask> Int
add_two () = ask () + ask ()

main : Unit -> <IO> Unit
main () =
  let total =
    handle add_two () with
      | ask () k -> resume k 21
  println (show_int total)
```

```sh
$ eml run ask.em
42
```

ファイルは線形な値なので、閉じ忘れをコンパイル時に検出できる。

```
main : Unit -> <IO> Unit
main () =
  let f = open "input.txt"
  let (f, text) = read_all f
  println text
```

```sh
$ eml check leak.em
[E3003] Error: `f` must be used exactly once, but it is not used
   ╭─[ leak.em:4:8 ]
   │
 4 │   let (f, text) = read_all f
   │        ┬
   │        ╰── `f` is bound here
 5 │   println text
   │               │
   │               ╰─ `f` is not used before the end of this scope
   │
   │ Help: pass `f` to `drop`
   │
   │ Note: linear values, such as files, the continuation of a `once` operation and closures that capture one, must be used exactly once
───╯
```

ほかのプログラム例は [docs/spec/examples.md](docs/spec/examples.md) と [tests/ui/](tests/ui/) にある。

## ビルドと実行

開発環境は Nix flake で用意する。direnv を使う場合は `direnv allow` で devShell に入る。devShell には clippy、rustfmt、cargo-insta が入っている。

```sh
cargo build                                  # すべての crate をビルドする
cargo test                                   # すべてのテストを実行する
cargo run -p eml_cli -- check <file.em>      # 検査だけを行う
cargo run -p eml_cli -- run <file.em>        # 実行する (--debug-heap でヒープの検査を有効にする)
nix build                                    # eml のバイナリをビルドする
```

`eml` の終了コードは、成功で 0、診断または実行時エラーで 1、使い方の誤りで 2 である。

## リポジトリの構成

処理系はバッチ型のパイプラインで、crate の依存は上から下へ一方向に流れる。

| crate | 役割 |
|---|---|
| `eml_cli` | `check` / `run` のコマンド。各段階をつなぐ |
| `eml_interp` | Core IR を CEK 機械で実行する |
| `eml_runtime` | オブジェクトモデル、ヒープ、参照カウント、`debug_heap` の検査 |
| `eml_core_ir` | 型付き HIR から Core IR (ANF 形式) への変換と、`dup` / `decref` の挿入 |
| `eml_types` | Kind、型、row の推論と、線形性、多重度、網羅性の検査 |
| `eml_hir` | CST から HIR への変換と名前解決 |
| `eml_syntax` | lexer、レイアウト段、パーサ、rowan の CST |
| `eml_diagnostics` | 診断のデータ構造と表示 |
| `eml_test_support` | 統合テスト用の補助 (開発時のみ) |

UI テストは [tests/ui/](tests/ui/) に置いている。`run/` は最後まで実行できるプログラム、`run-fail/` は実行時エラーで終わるプログラム、`check-fail/` は検査でエラーになるプログラムである。

## 文書

言語の仕様、処理系の構成、実装の現在地、将来の設計は [docs/](docs/) にまとめている。最初に [docs/README.md](docs/README.md) を読むとよい。

- [docs/overview.md](docs/overview.md): 目的、言語の性格、確定した設計判断
- [docs/spec/](docs/spec/): 実装の規範となる仕様
- [docs/implementation/status.md](docs/implementation/status.md): 実装の現在地
- [docs/future/roadmap.md](docs/future/roadmap.md): 将来の拡張

## ライセンス

[MIT License](LICENSE.txt) で公開している。
