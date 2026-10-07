# テストの高速化の設計

位置づけ: 作業用の設計文書。作業を終えたら削除する。

## 目的と範囲

`cargo test` を短くする。範囲は、時間に効く変更だけに限る。重複したテストや不要なテストを探す監査は、時間がほとんど縮まないので、別の作業にする。

1か所を直してから `cargo test` を流したときの内訳を、2026-10-07 に macOS で測った。

| 内訳 | 時間 | 原因 |
|---|---|---|
| ビルドとリンク | 約28秒 | テストのバイナリが53個あり、それぞれをリンクする |
| 新しいバイナリの初回起動 | 約135秒 | macOS は、新しくリンクした実行ファイルを最初に起動するときに検査する。1個あたり約3秒かかり、2回目からは0.02秒になる |
| Doc-tests | 約8秒 | doctest は1つもないが、9個の crate のすべてで rustdoc が動く |
| テストの実行 | 約20秒 | 大きな入力を使う少数のテストに偏っている |

初回起動の検査は、ターミナルを macOS の「デベロッパツール」に登録すると止まる。これはリポジトリの外の設定なので、この作業には含めない。

この作業では次の3つを行う。

1. 結合テストを crate ごとに1つのバイナリにまとめ、中身のないテストのバイナリと Doc-tests をなくす
2. 型検査の `eml_types::usage` にある、同じ名前の束縛の数の2乗の時間を直す
3. 検出できることを変えずに縮められる、大きすぎる入力を縮める

## 1. テストのバイナリをまとめる

### 結合テスト

結合テストのある crate はすべて、`Cargo.toml` に次のように書く。

```toml
[package]
autotests = false

[[test]]
name = "integration"
path = "tests/main.rs"
```

`tests/main.rs` は、今の `tests/*.rs` を `mod lexer;` のように宣言するだけのファイルにする。ファイルの場所と名前は変えない。そのため、`docs/` の地図やコメントにある `eml_core_ir/tests/verify.rs` のようなパスはそのまま使える。各ファイルの `mod common;` は `tests/main.rs` に1回だけ書き、各ファイルでは `use crate::common::...` にする。

`tests/` の下に置いても `tests/main.rs` で `mod` を宣言しないファイルは、コンパイルされず、そのテストは流れない。このことを `docs/implementation/testing.md` に書く。

対象の crate は `eml_syntax`、`eml_hir`、`eml_types`、`eml_core_ir`、`eml_interp`、`eml_cli`、`eml_test_support` の7つである。

### 単体テストと Doc-tests

- すべての crate の lib に `doctest = false` を付ける。doc コメントは日本語の説明で、実行する例を書かないためである
- 単体テストのない lib には `test = false` を付ける。対象は `eml_hir`、`eml_cli`、`eml_test_support` の lib と、`eml_cli` の bin (`eml`) である。単体テストを足すときは `test = false` を外す。このことを `docs/implementation/testing.md` に書く

テストのバイナリは53個から13個になる。単体テストが6個 (`eml_diagnostics`、`eml_syntax`、`eml_types`、`eml_core_ir`、`eml_runtime`、`eml_interp`)、結合テストが7個である。

### UI テストのスナップショットの名前

insta は、ファイルのスナップショットの名前の頭にモジュールのパスを付ける。`ui.rs` が `integration` のバイナリの `ui` モジュールになるので、名前は `ui__run@basics__hello.em.snap` から `integration__ui__run@basics__hello.em.snap` に変わる。`crates/eml_cli/tests/snapshots/` の142個のファイルの名前を変え、中身は1バイトも変えない。

### テストの名前とコマンド

1つのテストを流すコマンドは次のように変わる。

```sh
cargo test -p eml_syntax --test integration parser::empty_file
cargo test -p eml_cli --test integration ui::
cargo test --release -p eml_types --test integration scaling:: -- --ignored
cargo test --release -p eml_hir --test integration scaling:: -- --ignored
```

## 2. `eml_types::usage` の2乗の時間を直す

### 原因

`Usage::count` は、変数を経路ごとに1回ちょうど使わなかったとき、`KindReason::NotUsed` を作る。違反になったときの診断の材料なので、その中で `drop_fix` と `unused_path` を呼ぶ。`drop_fix` の `hidden` の判定と、`unused_path` から呼ぶ `shadowed_by` は、どちらも `later_namesakes` で同じ名前のすべての束縛をなめる。`String` のような `Unr` の型では違反にならないが、材料は毎回作る。そのため、1つのブロックで同じ名前を N 回 `let` で束縛すると、N² の時間がかかる。debug ビルドの `eml check` で測ると、4000行で2秒、8000行で8秒かかった。

### 直し方

`constrain` で `by_name` を作るとき、各リストを束縛の範囲の始まりの順に並べる。

- `shadowed_by(local, scope)`: `local` の束縛の終わりより後に始まる最初の位置を二分探索で求め、そこから前へ進む。`scopes` が `scope` に等しい束縛が見つかったら、それを返す
- `drop_fix` の `hidden`: `line.offset` より前に始まる最後の位置を二分探索で求め、そこから `local` の束縛の終わりまで後ろへ戻る。スコープの式の範囲が `line.offset` を含む束縛が見つかったら、隠されていると決める

同じブロックで同じ名前の `let` が続く場合は、どちらも数歩で止まる。結果は今と同じで、診断と fix は変わらない。`later_namesakes` は、この2つから使わなくなれば消す。

### テスト

`crates/eml_types/tests/scaling.rs` に6つ目の形として「1つの関数の本体で、同じ名前の `let` が続く連鎖」を足す。ほかの形と同じく、`SMALL` とその4倍の大きさで型検査の時間の比を測り、6以下なら通る。直す前に release ビルドで流し、比が6を超えて失敗することを確かめる。`docs/implementation/testing.md` の「性能のテスト」の形の一覧にも足す。

`crates/eml_interp/tests/run.rs` の `long_statement_sequence_does_not_overflow_the_stack` は、今は3.2秒かかる。原因はこの2乗の時間なので、入力の大きさ (5000行) は変えない。

## 3. 大きすぎる入力を縮める

まとめた後は、1つのバイナリのテストが並列に流れる。crate ごとの実行時間は、いちばん長い1件でほぼ決まる。そのため、各バイナリで飛び抜けて長いテストだけを縮める。

### `nesting.rs` の入れ子の `let`

`crates/eml_syntax/tests/nesting.rs` の `very_deep_nested_let_blocks_do_not_overflow_the_stack` の段数を 20_000 から 1_000 にする。8.1秒かかっていた。段ごとに字下げが1つ深くなるので、ソースの長さは段数の2乗で伸び、20_000段では約2億文字になる。

ガード (`stmt` の `nested`) を外した worktree で確かめると、300〜2000段では E0013 が1件にならずにアサーションが失敗し、5000段以上ではスタックが溢れた。1_000段は入れ子の上限 (256) の約4倍で、ガードを外せば必ず失敗する。

### `tail_calls.em` のループ

`tests/ui/run/runtime/tail_calls.em` の `loop` の回数を 1_000_000 から 10_000 にし、先頭のコメントの「a million iterations」を「ten thousand iterations」にする。UI の `run` は全ファイルを順に流す1つのテストで、この1件が1.3秒かかっていた。

インタプリタはフレームをヒープに置き、深さの上限を持たない。末尾の呼び出しがフレームを積むようになっても、100万回と1万回のどちらでも失敗しないので、回数を減らしても検出できることは変わらない。

### 変えないもの

- `parser.rs` の先読みの2つのテスト (120万トークン): ステップの上限 `STEP_LIMIT` (100万) を超える長さでないと意味がない。上限を下げるとパーサの振る舞いが変わるので、この作業の範囲の外とする
- `corpus.rs` の `every_prefix_of_the_corpus_parses` (1.9秒) と `verify.rs` の `a_long_run_of_if_statements_is_verified_in_linear_time` (1.1秒): ほかのテストと並列に流れる。縮めると、それぞれ切る位置の網羅と2乗の時間の検出が弱くなる
- `effect_loop.em`、`long_list.em`、`deep_recursion.em`、`closures.em`: Rust の再帰でスタックが溢れないことを確かめる大きさで、合わせて1秒ほどである

## テストの変更の一覧

| 種類 | テスト | 変更 | 理由 |
|---|---|---|---|
| 1 | `crates/eml_cli/tests/snapshots/` の142個 | 名前の頭を `ui__` から `integration__ui__` に変える。中身は変えない | 結合テストを1つのバイナリにまとめ、insta が名前にモジュールのパスを付けるため |
| 1 | `nesting.rs` の `very_deep_nested_let_blocks_do_not_overflow_the_stack` | 段数を 20_000 から 1_000 にする | ソースが段数の2乗で伸びて8秒かかり、1_000段でもガードの抜けを検出できるため |
| 1 | `tests/ui/run/runtime/tail_calls.em` とそのスナップショット | ループを 1_000_000 回から 10_000 回にし、出力の `1000000` を `10000` にする | 深さの上限がないので、回数で検出できることが変わらないため |
| 3 | 結合テストの各ファイル | `mod common;` を消し、`use common::...` を `use crate::common::...` にする | 1つのバイナリのモジュールにするため。期待値は変えない |
| 3 | `eml_types` と `eml_hir` の `scaling.rs` | 先頭のコメントのコマンドを新しいテストの名前にする | 同上 |
| 新規 | `eml_types/tests/scaling.rs` | 同じ名前の `let` の連鎖の形を足す | 2乗の時間を直したことを確かめるため |

ここに挙げたテスト以外の期待値は変えない。

## ドキュメントの更新

- `docs/implementation/testing.md`
  - 「crate の中の置き方」: 結合テストは crate ごとに1つのバイナリ (`tests/main.rs`) にまとめ、新しいファイルは `mod` で宣言すること。単体テストのない lib には `test = false` を付け、足すときは外すこと。lib には `doctest = false` を付けること
  - 「UI テスト」: スナップショットの名前の例を `integration__ui__run@basics__hello.em.snap` にする
  - 「性能のテスト」: `eml_types` の形を6つにする
  - 「よく使うコマンド」: 上の「テストの名前とコマンド」に合わせる
- `CLAUDE.md` の Commands: 1つのテストと UI テストのコマンドを新しい形にする。Testing の節に、結合テストが crate ごとに1つのバイナリであることを1行で足す

## 完了の条件

- `cargo test` がすべて通り、`cargo insta` に保留中のスナップショットがない
- `cargo clippy --all-targets` が警告を出さない
- `cargo test --workspace --no-run` の出す実行ファイルが13個である
- `cargo test --release -p eml_types --test integration scaling:: -- --ignored` と `eml_hir` の同じコマンドが通る
- 1か所を直してからの `cargo test` の時間と、テストの実行だけの時間を測り、作業の前と比べて報告する
