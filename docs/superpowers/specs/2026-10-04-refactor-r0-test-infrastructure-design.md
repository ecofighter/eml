# リファクタリング R0: テスト基盤と運用の設計

位置づけ: 作業用の設計文書。R0 を終えたら、残す価値のある内容を `docs/implementation/testing.md` と `docs/implementation/architecture.md` に移し、この文書は削除する。

## 背景

段階3 (エフェクト) に入る前に、これまでの実装で見つかった構成の不合理を、破壊的な変更も含めて整理する。整理は4つのサブプロジェクトに分け、R0 → R1 → R2 → R3 の順に、それぞれ spec、計画、実装の1サイクルで進める。

| 回 | 名前 | 範囲 |
|---|---|---|
| R0 | テスト基盤と運用 | この文書 |
| R1 | フロントエンド | `eml_syntax` と、`eml_diagnostics` の一部 |
| R2 | HIR と型 | `eml_hir`、`eml_types`。組み込みとエフェクトの表現は下流の crate も追随させる |
| R3 | Core IR とランタイム | `eml_core_ir`、`eml_runtime`、`eml_interp`、`eml_cli` |

方針は2つある。1つ目に、言語の観測できる振る舞い (UI テストの出力) は原則として変えない。spec の変更が要る項目は、その回の spec で個別に判断する。2つ目に、段階3〜5に向けた器の形 (エフェクトと型構成子の ID、HIR の item、フレームの種類など) は作り替えるが、機能は実装しない。

R0 を最初に置くのは、後の回でテストを変えるときの手間と危険を先に減らすためである。今はパイプラインを組むヘルパが5か所にあり、R2 と R3 で段階の関数の形が変わると、その全部を直すことになる。

## これまでの問題: テストを変えないために設計を曲げた

過去の計画には、「既存のテストは、このプランで名前を挙げたものだけを変える」という全体の制約があった。この制約は、期待値を変えることと、テストの組み立てを書き換えることを区別していない。そのため、構造体にフィールドを1つ足すだけでも計画の外の変更になり、実装中にテストが壊れそうになると、構造の側を曲げてきた。git の履歴と過去の作業記録から、次の例が見つかった。

| # | 当初の計画 | 実際にしたこと | 今の負担 | 直す回 |
|---|---|---|---|---|
| 1 | `Frame` に種類の enum を持たせる (段階2の設計) | 別のペイロード `Payload::ApplyFrame` を足し、記述子は `FRAME` を流用した。作業記録には「`Frame` を変えると既存の heap テストの書き換えが要るため」とある | 継続のリストが2種類のペイロードからなる。段階3の継続の捕獲、複製、`drop k` がどちらの種類も扱う必要がある | R3 |
| 2 | レイアウト規則2を例外なしで適用する | `block_inside_brackets_must_be_deeper_than_the_enclosing_block` を通すために、E0009 を出した行を除く `missing` フラグを足し、後で列0の例外を重ねた | テストのための例外が、規範の `spec/layout.md` の規則2に書き込まれている | R1 |
| 3 | BOM を読み込み時に除くか、表示だけで直すか | 「既存のテストが変わらない」ほうの表示側の修正を選んだ | BOM の扱いが lexer、レイアウト段、表示の3か所に分かれている | R1 |
| 4 | `TypedModule::signatures` にスキームを持たせる (段階2の設計) | `signatures: Type` を残し、テストの表示のためだけの公開フィールド `kinds` を足した。理由は記録されていない | スキームが後の段階に渡らない | R2 |

`docs/implementation/architecture.md` は、1について「既存の `Frame` を変えずに済むので」とだけ書いていて、テストが理由だったことが残っていない。

## テストの変更の運用

テストの変更を3種類に分け、種類ごとに合意の取り方を決める。

| 種類 | 何が変わるか | 例 | 合意と記録 |
|---|---|---|---|
| 1. 振る舞いの変更 | 言語として観測できる期待値。UI テストの出力、診断の番号と文言、成功か失敗か。テストの削除と移動もここに入れる | E2002 の副ラベルの位置を spec に合わせる | 事前に合意を取り、`testing.md` に理由を記録する |
| 2. 内部表現の変更 | 中間表現のダンプなど、内部の設計を写したスナップショットの期待値 | Core IR に join point を入れて、Core IR のスナップショットが変わる | spec に、変わるテストと理由を列挙する。spec の承認を合意とみなし、`testing.md` に記録する |
| 3. 機械的な追随 | テストの組み立てだけが変わる。スナップショットの文字列と `assert` の値は1文字も変えない | フィールドの追加、改名、ヘルパの差し替え | 計画で、この種類の変更を許すと宣言する。個別の合意は要らず、記録はコミットメッセージで足りる |

この表に、次の規則を加える。

- テストを変えないことを理由に設計を曲げない。テストが壊れると分かったら、その変更が1〜3のどれに当たるかを示して、変更を提案する。
- 計画の全体制約は「期待値は、このプランで名前を挙げたテストだけを変える。期待値を変えない機械的な追随は許す」と書く。
- UI テストは最も強い仕様として扱い、種類1でしか変えない。

この運用は、ユーザーのグローバルな方針「既存のテストを変えない」の例外として、このプロジェクトで合意したものである。`testing.md` と CLAUDE.md の Testing の節にそう書く。

## テスト基盤

### `eml_test_support` crate

開発専用の crate `crates/eml_test_support` を新しく作る。`publish = false` にし、各 crate の `[dev-dependencies]` から使う。

この crate は、テストのためにパイプラインを組む処理と、診断を文字列にする処理を1か所に持つ。各段階で止めて結果を返す関数を置く。

| 関数 | 戻り値 | 中身 |
|---|---|---|
| `source(text)` | `(SourceFiles, FileId)` | `test.em` として登録する |
| `parse(text)` | `Parsed { files, file, parse, diagnostics }` | `source` に続けて構文解析する。木が元のテキストに戻ること (lossless) を必ず確かめる |
| `lower(text)` | `Lowered { files, file, module, diagnostics }` | `parse` に続けて HIR に変換する。構文と HIR の診断を集める |
| `check(text)` | `Checked { files, file, module, typed, diagnostics }` | `lower` に続けて型検査する。構文、HIR、型の診断を集める |
| `core(text)` | `Program` | `check` の結果に診断のエラーがないことを確かめて、Core IR に変換する |
| `run(text)` | `(String, Result<(), RuntimeError>)` | `core` の結果を `debug_heap` を有効にして実行し、出力と実行の結果を返す |
| `execute(program, debug_heap)` | `(String, Result<(), RuntimeError>)` | 手書きの Core IR を実行し、出力と実行の結果を返す |

`diagnostics` は、その段階までに集めた診断を、各段階が返した順に並べたものである。

診断を文字列にする関数は2つ置く。どちらも `(&SourceFiles, &[Diagnostic])` を受け取り、今のヘルパと同じ文字列を出す。

- `short` は `Vec<String>` を返す。1件を `E0001 1:2 message` の1行にする。今の `eml_syntax` と `eml_hir` のテストの形である。
- `full` は `String` を返す。1件を、先頭の行に続けてラベル、note、help を字下げした行にする。今の `eml_types` のテストの形である。

診断を位置の順に並べるかどうかは、今の各ヘルパの挙動に合わせる。`eml_hir` のヘルパは並べ替え、`eml_types` のヘルパは並べ替えない。挙動を変えると、スナップショットの順序が変わるためである。

この crate は、自分が依存する crate の型をそのまま使う。そのため、使ってよいのは各 crate の `tests/` にある結合テストからだけである。`src/` の `#[cfg(test)]` から使うと、テスト対象の crate が2つ別々にリンクされて型が合わなくなる。この制約を crate の doc コメントに書く。

各 crate の段階に固有の表示 (`eml_syntax` の `shape`、`eml_hir::pretty`、`eml_types::dump`、`eml_core_ir::pretty`) は、今の場所に残す。`eml_test_support` は、パイプラインと診断の文字列だけを持つ。

`eml_cli` の `front()` は製品のコードなので残す。パイプラインを組む処理は、`eml_cli` と `eml_test_support` の2か所になる。

### `eml_diagnostics` の行と列

`SourceFiles` に `line_col(file, offset)` を足し、1始まりの行と列を返す。列は文字数で数える。[字句](../../spec/lexical.md) の定めに従い、ファイルの先頭の BOM は列に数えない。

今のテストのヘルパは、行と列を3か所で別々に計算していて、BOM も1列に数えている。BOM のあるファイルで1行目の診断の位置を出しているテストはないので、`line_col` に置き換えても出力は変わらない。

### 出力の取り込みと実行の設定

- `OutputSink::capture()` が返すバッファを `Captured` 型にし、捕まえた出力を `String` で返す `contents()` を持たせる。今は `String::from_utf8(buffer.lock().unwrap().clone()).unwrap()` が、テストに4か所ある。
- `RunConfig` に `with_debug_heap(bool)` を足す。`#[non_exhaustive]` のままにする。

### 各 crate のテストの書き換え

次のテストを `eml_test_support` の関数に書き換える。どれも種類3で、期待値は変えない。

| crate | ファイル | 書き換え |
|---|---|---|
| `eml_syntax` | `tests/common/mod.rs` | `parse` と `short` を使う。`shape`、`helps`、`lines` は残す |
| `eml_syntax` | `tests/parser.rs`、`tests/ast.rs`、`tests/corpus.rs` | ファイルの登録と構文解析を `parse` に替える。`parser.rs` と `corpus.rs` で重複している、トップレベルの子の種類を並べる処理は `common` に1つにする |
| `eml_syntax` | `tests/lexer.rs` | ファイルの登録を `source` に替える。`lex` を直接呼ぶ部分は変えない |
| `eml_hir` | `tests/common/mod.rs` | `lower` と `short` を使う |
| `eml_types` | `tests/common/mod.rs` | `check` と `full` を使う |
| `eml_core_ir` | `tests/common/mod.rs` | `core` を使う |
| `eml_interp` | `tests/run.rs`、`tests/closures.rs` | `run`、`execute`、`Captured::contents` を使う |
| `eml_runtime` | `src/output.rs` の単体テスト | `Captured::contents` を使う。単体テストなので `eml_test_support` は使わない |
| `eml_cli` | `tests/ui.rs`、`tests/api.rs` | `Captured::contents` と `with_debug_heap` を使う。`run` と `run_fail` で重複している、コンパイルして実行する処理を1つの関数にまとめる。UI テストは lib API を通すことに意味があるので、`eml_test_support` は使わない |

### `eml_interp` のテストと UI テストの重複

`eml_interp/tests/run.rs` には、ソースから実行して結果を確かめるテストがあり、多くが UI テストと同じことを確かめている。UI テストは最も強い仕様なので、ソースから実行するテストは UI テストに寄せる。この変更はテストの削除と移動なので、種類1に当たる。この spec の承認を合意とみなし、`testing.md` に記録する。

| テスト | 扱い | 理由 |
|---|---|---|
| `hello_world` | 削除 | `run/hello.em` が確かめている |
| `arithmetic_truncates_toward_zero` | 削除 | `run/operators.em` が同じ式を確かめている |
| `recursion` | 削除 | `run/factorial.em` が `factorial 20` を確かめている |
| `deep_recursion_does_not_overflow_the_stack` | 削除 | `run/deep_recursion.em` が同じプログラムを確かめている |
| `integer_overflow_is_a_runtime_error` | 削除 | `run-fail/integer_overflow.em` が同じプログラムを確かめている |
| `division_by_zero_is_a_runtime_error` | 削除 | `run-fail/division_by_zero.em` が同じプログラムを確かめている |
| `strings_are_freed` | `tests/ui/run/strings_freed_in_branches.em` に移す | `if` の枝の文字列と、使わない引数の文字列の解放は、UI テストにない |
| `and_and_or_short_circuit` | `tests/ui/run/short_circuit.em` に移す | `True && noisy False` で右辺が評価されることは、UI テストにない |
| `debug_heap_reports_leaks` | 残す | 手書きの Core IR が要る |
| `long_statement_sequence_does_not_overflow_the_stack` | 残す | 生成したソースが要る |

移すテストは、ソースを変えずに UI テストのファイルにする。新しいスナップショットの stdout は、元のテストの `assert_eq!` の期待値と同じでなければならない。既存の UI テストのファイルには手を入れない。

## 文書

| 文書 | 変更 |
|---|---|
| `docs/implementation/testing.md` | 「テストの変更の運用」の節を足す。「合意済みの例外」を作業ごとの見出し (S1、縦の貫通 段階1、段階2、R0) に整理し直し、R0 の削除と移動を記録する。テストの置き場所に `eml_test_support` を足す |
| `docs/implementation/architecture.md` | リポジトリの図と crate の説明に `eml_test_support` を足す。`ApplyFrame` を別のペイロードにした理由を「`Frame` を変えると既存の heap テストの書き換えが要るため」に直し、R3 でフレームの種類の enum に直すと書く |
| `docs/implementation/status.md` | 「リファクタリング」の節を作り、R0〜R3 の表と、R1〜R3 で直す項目の一覧 (下の「R1〜R3 で直す項目」) を置く。後の回の spec はこの一覧から作る |
| `CLAUDE.md` | Testing の節に、テストの変更の3種類と、`eml_test_support` の使い方 (結合テストからだけ使う) を書く。英語で書く |

## R1〜R3 で直す項目

R0 の範囲ではないが、`status.md` に置く一覧の中身をここで決めておく。各回の spec を書くときに、この一覧を出発点にし、項目を足したり外したりする。

### R1 フロントエンド

- 括弧と深さを数えるループが約8か所に写されていて、停止条件が食い違っている。走査を1つにし、括弧の種類の集合を `SyntaxKind` の側に置く。S2 の補間 `\{` は、これがないと全部の写しに足すことになる
- リテラルの値の解釈が lexer、パーサ、HIR に分かれていて、エスケープを2回検査している。解釈を1つのモジュールにまとめ、HIR が `SyntaxKind` を見ずに型付き AST から値を受け取るようにする。未対応のリテラルの E0004 は1か所で出す
- lexer に、S2 で使うモードのスタックの器を用意する
- 公開 API を整理する。使われていない `SyntaxNodePtr` を外す。`AstNode` を再公開するか範囲を返すメソッドを持たせ、`eml_hir` を `rowan` に依存させない。`lex` と `Token` をテストのためだけに公開している扱いを見直す
- 複数の段階が使う E0004 の番号と文言を `eml_diagnostics` に移す
- item の種類の判定 (`at_item_start` と `item`) を1つにする
- `AppExpr::callee` を位置で取る。今は最初の `Expr` の子を取るので、`€ x` では `x` が呼ばれるものになる
- 診断の文言でトークンの名前を引く表を1つにする。入れ子の深さの数え方を1つの書き方にそろえる
- BOM の扱いを1か所にする (上の表の3)
- レイアウト規則2の E0009 の例外を見直す (上の表の2)。spec の変更を伴うので、R1 の spec で判断する
- 使われていない `Diagnostic::fix` と `TextEdit` を残すかどうか決める

### R2 HIR と型

- HIR の `Module` に、関数のほかに `data` とエフェクトの item と、値、コンストラクタ、型、エフェクトの名前空間を持たせる。型変数と row 変数の表は `Generics` として、シグネチャ、`data`、エフェクトがそれぞれ持つ。本体の型の注釈は本体のアリーナに置く
- 組み込みの名前、fixity、型、Core IR への変換を1つの表にまとめる。今は `eml_hir`、`eml_types`、`eml_core_ir`、`eml_interp` の約7か所にある。組み込みの型は、ユーザーの関数と同じスキームの経路で作る。`Bool`、`Unit`、`IO`、`main` などは lang item として引く
- エフェクトと型構成子を ID で表す。`EffectRef::Io` と `Effect::Io` の二重の enum と、`TyCon { Int, String, Bool }` をやめる。row のラベルは、エフェクトの ID と型の引数を持つ
- HIR の子の式を辿る関数、パターンが束縛する変数を列挙する関数、ラムダが捕まえる変数を求める関数を HIR に置く。今は5〜7か所で手書きしていて、捕まえる変数は2か所で別々に求めている
- `table.rs` を、単一化、row、スキームの複製、export に分ける。紛らわしい名前を直す (型の形を表す `TyKind` と Kind の衝突、線形性の束から変数を取る `fresh_mult` と多重度の束から取る `fresh_mult_kind`)
- エラーの row を `error_rows` で引き回すのをやめ、row の末尾に `Error` を置く。`status.md` にある既知の誤り2件 (未定義のエフェクトの伝播と、写しどうしで `σ` を共有すること) はここから来ている
- `check.rs` の重複をなくす (check と infer の `If` と `Block`、矢印を辿る3つのループ)。診断を作る処理を別のモジュールに分ける。`perform` という名前を、エフェクトの `perform` と紛れないものにする
- `TypedModule` がスキームを返すようにし、テストの表示のためだけの `kinds` を除く (上の表の4)。`Type::Var` が rigid な変数と解けなかった変数を区別できるようにする
- 表示のために Kind の束を解かない
- spec の判断が要るもの: `x |> f a` の評価順 (今は `x` を最後に評価する)、E2002 の副ラベルの位置

### R3 Core IR とランタイム

- `Rhs::Nested` を join point に替え、末尾の位置の `Switch` と末尾呼び出しを入れる。局所的な分岐のたびにヒープにフレームを積むのをやめる
- Perceus の挿入を独立したパスにし、Core IR の不変条件を確かめる verifier を足す
- boxed かどうかの判定を1か所にする
- 入口の関数を Core IR の側で作り、引数のない `main` の扱いをインタプリタから除く
- `Frame` を種類の enum にする。`ApplyFrame` を別のペイロードにするのをやめ (上の表の1)、番兵の `IO_HANDLER = u32::MAX` と、入れ子の式のフレームを表す `slots: None` をなくす。記述子はペイロードの種類から決める
- 共有されたオブジェクトを複製する手続きをランタイムに置き、参照を `dup` せずに複製する `Clone` をなくす
- 変数のスロットごとの参照の数 (`Owned::refs`) が要るかを確かめ、要らなければ除く
- 組み込みの変換を R2 の表から引く
- `RuntimeError` と `step` の結果に型を付ける。`eml_cli` の `RunResult` が文字列で包み直すのをやめる

## 成功の条件

- `cargo test`、`cargo clippy --all-targets`、`cargo fmt --check` が通る
- 既存のスナップショット (インラインと `snapshots/` のファイル) は1文字も変わらない。増えるスナップショットは、移した2件の UI テストだけである
- 移した2件の UI テストの stdout が、元のテストの期待値と一致する
- パイプラインを組む処理は `eml_cli` と `eml_test_support` の2か所、行と列の計算は `eml_diagnostics` の1か所になる
- 上の「文書」の表の変更が済んでいる

## 範囲の外

- R1〜R3 の項目。R0 は、それらのテストの変更を安く安全にするための準備だけを行う
- UI テストのハーネスを `eml_test_support` に寄せること。UI テストは lib API を通すことに意味がある
- 診断の表示の形式をそろえること。形式を1つにすると、多くのスナップショットが情報の増減なしに変わる
