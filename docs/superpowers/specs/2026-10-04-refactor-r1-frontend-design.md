# リファクタリング R1: フロントエンドの設計

位置づけ: 作業用の設計文書。R1 を終えたら、残す価値のある内容を `docs/implementation/architecture.md` と `docs/implementation/status.md` に移し、この文書は削除する。

## 目的と範囲

`eml_syntax` の内部と公開 API を整理し、`eml_hir` との境界を型付き AST に寄せる。出発点は [status.md](../../implementation/status.md) の「R1〜R3 で直す項目」の R1 の一覧である。リファクタリング全体の方針 (UI テストの出力は原則として変えない、段階3〜5と S2 の器の形は作り替えるが機能は実装しない) と、テストの変更の運用 ([testing.md](../../implementation/testing.md)) に従う。

この回では、spec を2か所変える。BOM を読み込み時に除くことと、レイアウト規則2の例外を外すことである。どちらもユーザーと合意済みである。

## 1. BOM とレイアウト規則2

### BOM は読み込み時に除く

`SourceFiles::add` が、テキストの先頭の BOM (U+FEFF) を取り除いてから保存する。以後の位置 (`TextRange`、レイアウトの列、診断の行と列) は、すべて BOM を除いたテキストで数える。CST は、保存したテキストに対して lossless である。rustc と同じやり方である。

これで、次の BOM の特別扱いが不要になるので消す。

| 場所 | 消すもの |
|---|---|
| `eml_syntax` の lexer | 空白の正規表現の `\u{FEFF}`。shebang を BOM の直後でも認める処理 |
| `eml_syntax` のレイアウト段 | 列と字下げの計算から BOM を除く処理 (`scan_lines` と `report_tab`) |
| `eml_diagnostics` | `bom_len`。表示 (`render.rs`) と `line_col` での位置のずらし |

ファイルの途中の U+FEFF は、空白ではなく認識できない文字 (E0001) になる。今の lexer は途中の U+FEFF も空白として通していて、[字句](../../spec/lexical.md) の「ファイルの先頭の BOM」だけを読み飛ばすという定めと食い違っている。この変更で spec に合う。

`eml_test_support::parse` は、`eml_syntax::parse` に `files.text(file)` を渡すように直す。lossless の比較も `files.text(file)` に対して行う。今は呼び出し側から受け取った `text` を渡しているので、BOM を除いた後のテキストとずれる。`eml_cli` はすでに `files.text(file)` を渡している。

spec は次のように直す。

- `docs/spec/lexical.md`: 「ファイルの先頭の BOM (U+FEFF) は、ソースを読み込むときに取り除く。字句、レイアウトの列、診断の位置は、BOM を除いたテキストで数える。ファイルの途中の U+FEFF は認識できない文字 (E0001) とする」
- `docs/spec/diagnostics.md`: 「ソースの先頭の BOM は、表示で列に数えない。`TextRange` は BOM を含む元のテキストのバイト位置のままにして、表示の層で BOM を除いて位置をずらす」を、「`TextRange` は、読み込み時に先頭の BOM を除いたテキストのバイト位置である ([字句](lexical.md))」に置き換える

### レイアウト規則2の例外を外す

`docs/spec/layout.md` の規則2から、「ただし、規則 3 で E0009 を出した行 (字下げが足りなかった次の行) には規則 2 を当てない (列 0 の行を除く)。」で始まり「その中の誤りが報告されなくなる」で終わる部分を消す。コード (`layout.rs`) からは `missing` フラグと、列0の特例を消す。

この例外は、括弧の中で行末の `->` などがブロックを開こうとし、次の行の字下げが足りずに E0009 を出した場合だけに効く。正しいプログラムの意味には影響しない。過去の計画で、既存のテストを変えないために足され、後から穴を塞ぐために列0の特例を重ねた経緯がある ([status.md](../../implementation/status.md) の「テストを変えないために曲げた箇所」の2)。

例外を外すと、`f =\n  g (fn x ->\n  y)` のレイアウト段の出力は `f = <OPEN> g ( fn x -> <OPEN> <CLOSE> <SEP> y ) <CLOSE>` になり、レイアウト段の診断は E0009 の1件のままである。プログラム全体では、E0009、`expected )` (E0011) に加えて、`unexpected )` (E0011) と、`y` が未定義である E1001 が出るようになる。例外があっても `expected )` はすでに出ていたので、この例外は連鎖を完全には抑えていなかった。

### この節で変わるテスト

すべて種類1で、この spec の承認を合意とする。

| テスト | 変更 |
|---|---|
| `crates/eml_syntax/tests/lexer.rs` の `shebang_is_trivia_only_at_the_start_of_the_file` のうち `kinds("\u{feff}#!x\ny")` のアサーション | 削除する。BOM の除去は `SourceFiles` の仕事になり、除いた後の `#!x` は通常の先頭の shebang である。代わりに、BOM つきのテキストを `eml_test_support::parse` で構文解析し、shebang が trivia になることを確かめるテストを `tests/parser.rs` に足す |
| `crates/eml_syntax/tests/lexer.rs` の `byte_order_mark_is_whitespace` | 「ファイルの途中の U+FEFF は E0001」を確かめるテストに置き換える。`lex` に `"fn\u{feff}"` を渡し、E0001 が1件出ることを確かめる |
| `crates/eml_syntax/src/layout.rs` の単体テスト `byte_order_mark_takes_no_column` | 削除する。レイアウト段は BOM を見なくなる |
| `crates/eml_diagnostics/src/render.rs` の `byte_order_mark_takes_no_column` と `byte_order_mark_does_not_shift_later_lines` | BOM つきのテキストを `SourceFiles::add` に渡すところは変えない。診断の範囲を、BOM を除いたテキストの位置で作る形にする (`$` はそれぞれバイト位置 4 と 10)。期待する表示 (`a.em:1:5` と `a.em:2:5`) は変えない |
| `crates/eml_diagnostics/src/source.rs` の `line_col_does_not_count_the_bom` | 同じく、BOM を除いた位置で `line_col` を呼ぶ形にする (`b` はバイト位置 1、2行目の `b` はバイト位置 2)。期待する行と列 (`1:2` と `2:1`) は変えない。あわせて、`SourceFiles::add` が先頭の BOM を除き、途中の U+FEFF を残すことを確かめるテストを足す |
| `crates/eml_syntax/src/layout.rs` の `block_inside_brackets_must_be_deeper_than_the_enclosing_block` | 期待するレイアウト段の出力を `"f = <OPEN> g ( fn x -> <OPEN> <CLOSE> <SEP> y ) <CLOSE>"` にする。診断は `["E0009@14..16"]` のまま |

UI テストに BOM を含むファイルはなく、UI テストの出力は変わらない。規則2の例外を外しても、UI テストを含めてほかのテストの出力は変わらないことを確かめてある。

## 2. 括弧と深さの走査を1つにする

### 今の問題

括弧とブロックの深さを数えるループが、`grammar/` の中に7つある。読み飛ばしの `stray_tokens`、`skip_to_sep`、`close_block`、`skip_to_closing` と、先読みの `has_conop_ahead`、`has_left_arrow`、`apat_len` である。括弧の種類の並び `L_PAREN | L_BRACK | L_BRACE` も、レイアウト段とパターンに直書きされている。

写しごとに停止の条件が違う。

- `has_conop_ahead` は、深さ0の `LAYOUT_OPEN` で止まる。`has_left_arrow` は、`LAYOUT_OPEN` を深さに数える。
- `skip_to_closing` は `LAYOUT_SEP` で止まるが `;` では止まらない。`skip_to_sep` はどちらでも止まる。
- `apat_len` は `LAYOUT_SEP` で止まらない。閉じていない `(` があると、レイアウト段が規則2で暗黙に閉じた後も読み続け、遠くの `)` を対応する括弧とみなす。

S2 で補間の `\{` と `}` を括弧として足すときに、すべての写しを直すことになる。

### 設計

- `SyntaxKind` に `is_opening_bracket()` と `is_closing_bracket()` を置く。レイアウト段 (`layout.rs` の `L_PAREN | L_BRACK | L_BRACE` と `is_closing_bracket`)、パーサ、パターン (`at_apat_start_at`) がこれを使う。
- `grammar/scan.rs` に、深さを数える型 `Nesting` を置く。括弧の深さとブロックの深さを別々に持つ。トークンの種類を1つ渡すと、次の4つのどれかを返す。
  - `Enter`: 括弧かブロックに入った
  - `Leave`: 括弧かブロックを抜けた
  - `Inside`: 入れ子の中にいるか、深さ0の普通のトークンである
  - `End`: 今の範囲が終わった。対応する開き括弧のない閉じ括弧、ブロックの外での `LAYOUT_CLOSE`、ブロックの外での `LAYOUT_SEP` のどれかである
  
  `End` の判定は、今の `skip_to_closing` と同じ規則にする。規則2でレイアウト段が括弧を暗黙に閉じると、閉じ括弧のトークンがないまま括弧の深さが戻らない。そのため、ブロックの外の `LAYOUT_SEP` と `LAYOUT_CLOSE` は、括弧の深さにかかわらず範囲の終わりとする。
- 7つの関数は `Nesting` を使い、「何で止まるか」だけを書く形にする。読み飛ばしの関数は `bump_any` で、先読みの関数は `peek` で進める。
- 停止の条件の違いは、必要なものだけを呼び出し側の条件として明示的に残す。
  - `has_conop_ahead` は、深さ0の `PIPE` と `LAYOUT_OPEN` でも止まる。`data` の選択肢の範囲の外に出たことを意味するためである。
  - `skip_to_sep` は `;` でも止まる。`;` はブロックの中の明示的な区切りだからである。
  - `apat_len` は `End` で `None` を返す。閉じていない括弧の後ろを読み続けない。

### 振る舞いへの影響

正しいプログラムの CST は変わらない。コーパスのテストと構文のスナップショットがこれを確かめる。変わりうるのは、閉じていない括弧を含む誤りのある入力での回復の結果だけである。実装の途中で既存のスナップショットや UI テストの出力が変わった場合は、種類1として差分と理由をユーザーに示し、承認を得てから期待値を変える。期待値を推測で先に書くことはしない。

## 3. AST の API、リテラル、公開 API

### AST のラッパに足すもの

HIR が CST の木の構造 (`.syntax()` と子のトークンの走査) に触れずに済むように、型付き AST のラッパに次を足す。識別子と演算子のトークンは、今までどおり `SyntaxToken` で受け取る。トークンを別の型で包み直すのは、R2 で HIR を作り直すときに必要になってから判断する。

- `range(&self) -> TextRange`: すべてのノードの型と、`Expr`、`Pat`、`Type`、`Item`、`Stmt` などの enum に、マクロで生成する。HIR の `.syntax().text_range()` はすべてこれに置き換える。
- `keyword_range(&self) -> TextRange`: ノードの最初のトークンの範囲を返す。トークンがなければノードの範囲を返す。HIR の `lower/mod.rs` と `lower/expr.rs` にある2つの `keyword()` ヘルパを置き換える。
- `Literal::value(&self) -> Option<LiteralValue>`: `LiteralValue` は `Int(i64)` と `String(String)` を持つ enum である。浮動小数、文字、補間を含む文字列、複数行の文字列などの未対応のリテラルと、値が壊れているもの (範囲外の整数、不正なエスケープ、閉じていない文字列) は `None` を返す。どれも字句解析かパーサが報告済みである。

`AppExpr::callee` は、最初の子のノードを `Expr` として読むように直す。今は最初の `Expr` の子を取るので、`f = € x` では `€` の `ERROR` ノードが飛ばされ、`x` が呼ばれるものになる。直した後は、最初の子が `Expr` でなければ `callee` は `None` になり、`args` は2つ目以降の `Expr` の子になる。

### リテラルの値の解釈

`crates/eml_syntax/src/literal.rs` を作り、エスケープの表と `\u{...}` の解釈を置く。lexer の検査 (不正なエスケープの E0008) と、値の取り出し (`Literal::value`) が同じ表を使う。`int_value` と `decode_string` はこのモジュールに移し、crate の外には公開しない。

### 公開 API

- 使われていない `SyntaxNodePtr` の再公開をやめる。
- `int_value` と `decode_string` の公開をやめる (上の「リテラルの値の解釈」)。
- `lex` と `Token` は公開したままにする。lexer は独立した段階の API で、テストのほかに将来の LSP の構文の色分けでも使う見込みがある。
- `eml_hir` の依存から `rowan` を外す。

### E0004 を `eml_diagnostics` に移す

E0004 (未対応) は、構文、HIR、型検査のどの段階でも同じ意味で使う ([診断](../../spec/diagnostics.md))。番号とラベルの文言が `eml_syntax` にあり、`eml_hir` がそれを使って独自の関数を作っている。これを `eml_diagnostics` に移す。

- `eml_diagnostics` に、番号の定数 `NOT_YET_SUPPORTED`、ラベルの文言、`Diagnostic::not_yet_supported(file, range, message)` を置く。
- `eml_syntax::codes::NOT_YET_SUPPORTED`、`eml_syntax::NOT_YET_SUPPORTED_LABEL`、`eml_hir::not_yet_supported` を消し、すべて `eml_diagnostics` のものを使う。
- 診断の番号、メッセージ、ラベルの文言は変えない。

### この節で変わるテスト

- `crates/eml_syntax/tests/literals.rs`: `int_value` と `decode_string` を直接呼ぶ代わりに、リテラルを `eml_test_support::parse` で構文解析し、`Literal::value()` を呼ぶ形にする。期待する値は変えない (種類3)。
- `crates/eml_syntax/tests/ast.rs`: `range`、`keyword_range`、`Literal::value`、`€ x` の `callee` と `args` のテストを足す (新しいテスト)。
- `AppExpr::callee` の修正で既存のスナップショットや UI テストの出力が変わった場合は、種類1として差分を示し、承認を得てから期待値を変える。`€` を含む UI テスト (`check-fail/unexpected_character.em`、`check-fail/multiple_errors.em`) で HIR の診断が変わる可能性がある。

## 4. 残りの項目と、外す項目

### R1 で行うもの

- item の種類の判定を1つにする。`fn item_kind(p: &Parser) -> Option<ItemKind>` を作り、`at_item_start` は `is_some()`、`item` は `match` でこれを使う。S2 で `pub`、`import`、`type` の扱いを変えるときに、直す場所が1か所になる。
- lexer を分ける。`lexer.rs` を、`lexer/mod.rs` (全体の流れ、単純なトークン、コメント) と `lexer/string.rs` (文字列、複数行の文字列、raw 文字列、コマンドリテラル) に分ける。`skip_simple_string` とコマンドリテラルにある、バックスラッシュを読み飛ばす同じ処理は1つにする。
- sink の不変条件を確かめる。パーサが木に入れた仮想でないトークンと、lexer の trivia でないトークンが1対1で対応し、範囲も一致することを、`sink.rs` で `debug_assert` する。
- `eml_syntax::codes` の定数を番号順に並べる。今は E0009 が E0006 と E0007 の間にある。

### 一覧から外すもの

`status.md` の R1 の一覧から外し、理由を残す。

| 項目 | 外す理由 |
|---|---|
| lexer のモードのスタックの器 | モードが1つしかないスタックは中身のない抽象になる。S2 で補間のトークンと一緒に設計する |
| 未対応のリテラルの E0004 を1か所で出す | 補間の E0004 は lexer に残る。補間の字句は S2 で作り直すので、今パーサに寄せても捨てることになる |
| 診断の文言でトークンの名前を引く表を1つにする | `unexpected` と `describe` は、「unexpected line break」と「a new line」のように文の中での役割が違い、重複ではない。まとめると文言が変わるだけになる |
| 入れ子の深さの数え方を1つの書き方にそろえる | `postfix` がフィールドの連鎖を手で数えるのは、連鎖を再帰ではなくループで読むためで、意図した違いである |
| 診断の並べ替えが2回ある | `lex` は `ERROR_TOKEN` の診断を後から足すので並べ替えが要る。`parse` は3つの段階の診断を合わせるので要る。どちらも必要である |
| 使われていない `Diagnostic::fix` と `TextEdit` | spec の [診断](../../spec/diagnostics.md) が定めるデータ構造の一部で、段階4と5の診断で使う |
| `lex` と `Token` をテストのためだけに公開している扱い | 上の「公開 API」のとおり、公開したままにする |

## 文書

| 文書 | 変更 |
|---|---|
| `docs/spec/lexical.md`、`docs/spec/diagnostics.md` | 1の BOM |
| `docs/spec/layout.md` | 1の規則2の例外を消す |
| `docs/implementation/architecture.md` | 「`eml_syntax` の内部構成」に `lexer/`、`literal.rs`、`grammar/scan.rs` を足す。`debug_tree` を「テスト用」と書いている箇所を、公開の木のダンプとして直す。「ソースファイルと位置」に、BOM を読み込み時に除くことを書く。`eml_hir` が型付き AST の API だけを使い、`rowan` に依存しないことを書く |
| `docs/implementation/status.md` | 「リファクタリング」の表で R1 を完了にする。R1 の一覧を消し、外した項目を理由とともに残す。「テストを変えないために曲げた箇所」の2と3を、R1 で直したと書き換える。「完了した作業」に R1 の行を足す |
| `docs/implementation/testing.md` | 「テストの変更の記録」に「リファクタリング R1」の見出しを作り、1と3で変えたテスト (種類1) と、実装の途中で承認を得て変えたテストを記録する |

## 成功の条件

- 括弧とブロックの深さを数える処理が `grammar/scan.rs` の1か所になり、括弧の種類の判定が `SyntaxKind` の1か所になる
- BOM を扱う処理が `SourceFiles::add` の1か所になる。ファイルの途中の U+FEFF は E0001 になる
- `eml_hir` が `rowan` に依存しない。`eml_hir` の中に `.syntax()` の呼び出しと、`SyntaxKind` によるリテラルの判定がない
- 変わったテストは、1と3で挙げたものと、実装の途中で差分を示して承認を得たものだけである
- `cargo test`、`cargo clippy --all-targets`、`cargo fmt --check` が通る
- 上の「文書」の表の変更が済んでいる

## 範囲の外

- S2 と S3 の構文 (補間のトークン列、レコード、モジュールなど) の実装
- 段階3以降の構文 (`handle`、`match`、`data` など) の AST のアクセサ。HIR への変換で必要になったときに足す ([architecture.md](../../implementation/architecture.md) の「`eml_syntax` の内部構成」)
- HIR のデータモデルの作り直し (R2)
