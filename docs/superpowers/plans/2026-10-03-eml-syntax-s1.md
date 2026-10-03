# eml 本番の構文 S1 (字句・レイアウト・M1 の文法) 実装計画

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 構文設計 spec §10 の段階 S1 を実装する。新しい lexer、レイアウト段、M1 の機能の文法 (宣言、式、パターン、型と row) を `eml_syntax` に入れ、暫定構文の文法を捨てる。

**Architecture:** lexer は logos で単純なトークンを切り出し、文字列・コメント・演算子の分類などを手書きの層で扱う。lexer と parser の間に新しいレイアウト段を置き、trivia を除いたトークン列に幅 0 の仮想トークン (`LAYOUT_OPEN` / `LAYOUT_SEP` / `LAYOUT_CLOSE`) を挿入する。parser は仮想トークンを読んでもイベントを出さないので、rowan の木は lossless のまま。文法は `grammar/` の下に items / types / patterns / expressions のモジュールで書く。S2・S3 の構文は字句と文法の置き場所だけを用意し、使うと E0004 (未対応) を出して回復する。

**Tech Stack:** Rust (edition 2024)、logos 0.16、rowan 0.16、text-size 1.1、insta 1.49。新しい外部 crate は追加しない

**Spec:** [docs/superpowers/specs/2026-10-03-eml-syntax-design.md](../specs/2026-10-03-eml-syntax-design.md) (以下「spec」。§3 字句、§4 レイアウト規則、§5 文法、§6〜§7 の構文の部分、§10 実装への影響)。全体の方針は [言語設計 spec](../specs/2026-10-03-eml-language-design.md) §3、§8

## Global Constraints

- S1 は `eml_syntax` だけの作業。ほかに触るのは UI テストのソースとスナップショット、`CLAUDE.md`、言語設計 spec の文言だけ (spec §10「S1 は `eml_syntax` だけの作業になる」)
- 構文の差し替えは `grammar/`、`SyntaxKind`、lexer、AST ラッパ、新しいレイアウト段に閉じる (spec §10)
- CST は常に lossless。仮想トークンは parser の入力にだけ現れ、イベントにも rowan の木にも入らない (spec §4)
- エラーで止まらない。lexer・レイアウト段・parser の診断はすべて集めて返し、壊れた入力でも必ず `SOURCE_FILE` の木を作る。パニックしない
- 各段階は純粋な関数。グローバルな可変状態は持たない
- 字句・構文の診断の番号は E0xxx。番号は `eml_syntax::codes` に置く
- トークンの種類 (`EOF` まで) は 128 未満に収める (`TokenSet` が `u128`)
- 既存のテストは合意済みの仕様なので変更しない。例外は言語設計 spec §8 の合意済みの例外 (暫定構文で書いたテストのソースの機械的な書き換え) で、この計画では各タスクで書き換えるテストと理由を明示する
- テストを先に書く (TDD)。スナップショットは `insta`
- 各タスクの終わりに `cargo test`、`cargo clippy --all-targets`、`cargo fmt --check` が通ること
- コードのコメントとドキュメントは日本語、診断のメッセージは英語 (既存の慣習)

### この計画で決めた、spec にない細部

spec が決めていない (または実装の都合で読み替えた) 細部を、次のように決めた。後で変える場合は spec に書き戻す。

- **診断の番号**:

  | 番号 | 意味 | 出す段 |
  |---|---|---|
  | E0001 | 認識できない文字 (既存) | lexer |
  | E0002 | 閉じていない文字列 (既存)。複数行・raw の文字列とコマンドリテラルにも使う | lexer |
  | E0003 | 項目が来るべき位置に別のもの (既存、メッセージを `expected an item` に変える) | parser |
  | E0004 | まだ対応していない構文 (既存) | lexer / parser |
  | E0005 | 閉じていないブロックコメント | lexer |
  | E0006 | インデントのタブ | レイアウト段 |
  | E0007 | 数値のリテラルの形が不正 (`0xZZ` など) | lexer |
  | E0008 | 不正なエスケープ (`\q`、範囲外の `\u{...}`) | lexer |
  | E0009 | 字下げしたブロックが必要 | レイアウト段 |
  | E0010 | `.` の前後の空白 | parser |
  | E0011 | 一般の構文エラー (`expected ...`) | parser |
  | E0012 | 括弧で囲む必要がある式 (`f match x with ...` など) | parser |

- **S2・S3 の構文の S1 での扱い**: 次のものは、字句と文法の置き場所だけを作り、使うと E0004 を出す。回復できるよう、まとまりごとに読み飛ばす

  | 構文 | S1 での扱い |
  |---|---|
  | 浮動小数、文字のリテラル | lexer は `FLOAT` / `CHAR` にする。parser がリテラルとして読んだ位置で E0004 (`t.0.1` の `0.1` で誤って出さないため) |
  | 複数行の文字列 `"""`、raw 文字列 `r"..."`、コマンドリテラル | lexer はそれぞれ1つのトークン (`MULTILINE_STRING` / `RAW_STRING` / `COMMAND`) にする。parser がリテラルとして読んだ位置で E0004 |
  | 文字列の補間 `\{...}` | lexer が補間の穴を対応する `}` まで読み飛ばし (入れ子の文字列も読む)、E0004。文字列は1つの `STRING` のまま |
  | レコード `{...}`、リスト `[...]` | parser が対応する閉じ括弧までを `ERROR` ノードにまとめ、E0004 |
  | `type` の宣言、`pub` | parser は文法どおりに読み、キーワードの位置に E0004 |
  | `import` | parser が E0004 を出し、次の項目まで読み飛ばす |

- **文字列は S1 では1つの `STRING` トークンのまま**にする。spec §10 の `STRING_START` / `STRING_TEXT` / ... への分割は、補間と一緒に S2 で行う。S1 の lexer はエスケープの検査 (E0008) まで行う
- **raw 文字列は行をまたげる** (Rust と同じ)。コマンドリテラルは行をまたげない。どちらも S1 では E0004 なので、S2・S3 で見直してよい
- **`t.0.1` の分割は lexer で行う**: spec §5 は「parser がフィールドアクセスの位置で分割する」とするが、`.` の直後 (空白なし) の `数字.数字` の形の `FLOAT` を、lexer が `INT` `DOT` `INT` に分ける。そのほかの位置の `FLOAT` は分けない。観察できる結果 (木と診断) は spec と同じで、sink にトークンの分割を持ち込まずに済む
- **文字列の外の `\`** は、S1 では使い道がないので E0001 (認識できない文字) とする
- **E0009 はレイアウト段が出す**。開始トークンで行が終わり、次の行が深くない (ファイルの終わりを含む) とき、E0009 を出して、開始トークンの直後に空のブロック (`LAYOUT_OPEN` `LAYOUT_CLOSE`) を挿入する。parser は空のブロックを黙って受け入れるので、診断は重ならない。次の行の先頭が閉じ括弧の場合も「深くない」とみなす (`(fn x ->` の直後の行が `)` だけ、など)
- **parser の診断の重複を抑える**: parser は `ERROR_TOKEN` の位置に診断を出さない (lexer が報告済み)。直前の parser の診断と同じ位置には、診断を出さない
- **前置の `-` は `OP_SEQ` の中のトークンとして平たく置く** (`-a * b` は `OP_SEQ [MINUS, a, *, b]`)。負号のノードは作らない。HIR が fixity と一緒に `negate` として組み直す (spec §7)。演算子の位置 (列の先頭か、別の演算子の直後) にある `MINUS` が前置の負号である
- **中置のコンストラクタの両辺は `btype`**: spec §5 の文法は `type_atom CONOP type_atom` だが、spec §6 の例 `| a :: List a` の右辺は `btype` なので、例に合わせる
- **`->` と `=` で行が終わる型**: レイアウト段がブロックを開くので、型の `->` の右側と `type` の宣言の `=` の右側では、`LAYOUT_OPEN 型 LAYOUT_CLOSE` も1つの型として読む (`f : Int ->` で改行して続ける書き方のため)
- **`let ... in` の `=` の右側は `body`** として読む (ブロックの `let` と同じ関数で読むため)。`in` が続けば `LET_EXPR` になる
- **fixity の優先順位が 0〜9 の1桁でなければ、parser が E0011** を出す
- **将来の予約語** `forall` `class` `instance` はキーワードのトークンにし、項目の位置で使うと E0011 (`reserved for future use`)
- **空の項目** (`;;` や、ブロックの先頭の `;`) は黙って読み飛ばす
- **トップレベルの閉じていない文字列** (`"abc`) は、lexer の E0002 と parser の E0003 の両方を出す (別の問題なので両方を報告する。M1 スケルトンで保留した論点の決定)
- **閉じ忘れた括弧の中では改行が意味を持たない** (spec §4 規則 2 のまま)。そのため、閉じ忘れた `(` より後ろの項目は、括弧の中身として読まれる。回復の質の改善は、spec の変更が必要なので S1 では行わない

## Review Focus

spec が明示していないが、利用者が実際に踏みやすい入力と、そのときに期待される振る舞い。各行のテストは、担当するタスクに入れてある。

1. **編集の途中で切れたソース**: エディタで入力している途中のような、任意の位置で切れたファイルでも、パニックや無限ループをせず、lossless な木と診断を返す (Task 8 の `every_prefix_of_the_corpus_parses`、`every_line_deletion_of_the_corpus_parses`)
2. **F# / Haskell の癖で `|` を `match` と同じ列に書く**: 「字下げしたブロックが必要」(E0009) が `with` の位置に出て、どう直せばよいかが分かる (Task 6 の `arms_at_the_column_of_match_need_indentation`)
3. **CRLF の改行とタブ**: Windows で書いたファイルでも、レイアウトの結果が LF のファイルと同じになる。インデントのタブは E0006 になるが、レイアウトは続ける (Task 2 の `crlf_lines_lay_out_like_lf`、`tab_in_indentation_is_an_error`)
4. **関数合成の癖で `f . g` と書く**: `.` の前後の空白として E0010 になり、`>>` を使うよう案内する (Task 5 の `dot_with_spaces_is_an_error`)
5. **閉じ忘れた括弧**: パニックせずに `expected `)`` を報告して終わる (Task 2 の `unclosed_bracket_suspends_layout_to_eof`、Task 5 の `unclosed_paren_is_reported`)

---

### Task 1: `SyntaxKind` の差し替えと lexer の書き直し

**Files:**
- Modify: `crates/eml_syntax/src/syntax_kind.rs` (トークンの種類を全面的に置き換える。logos の derive を外す)
- Modify: `crates/eml_syntax/src/lexer.rs` (全体を置き換える)
- Modify: `crates/eml_syntax/src/lib.rs` (`codes` に番号を足す、`NOT_YET_SUPPORTED_LABEL`)
- Modify: `crates/eml_syntax/tests/lexer.rs` (全体を置き換える。暫定構文のテストを書き換え、新しいテストを足す)
- Modify: `crates/eml_syntax/tests/parser.rs` (暫定構文の字句に依存する3件を機械的に書き換える)
- Modify: `crates/eml_syntax/src/parser.rs` (単体テスト2件のコメントを `//` から `--` に書き換える)
- Modify: `tests/ui/check-fail/unexpected_character.em`、`tests/ui/check-fail/multiple_errors.em`、`tests/ui/run/comments_only.em`
- Modify: `crates/eml_cli/tests/snapshots/ui__check_fail@unexpected_character.em.snap`、`crates/eml_cli/tests/snapshots/ui__check_fail@multiple_errors.em.snap`

**Interfaces:**
- Consumes: なし
- Produces:
  - `SyntaxKind` のトークン: trivia `WHITESPACE` `COMMENT` `BLOCK_COMMENT` `SHEBANG`、リテラル `INT` `FLOAT` `CHAR` `STRING` `MULTILINE_STRING` `RAW_STRING` `COMMAND`、`LIDENT` `UIDENT` `UNDERSCORE`、キーワード `DATA_KW` … `INSTANCE_KW` (下のコード)、区切り `L_PAREN` `R_PAREN` `L_BRACK` `R_BRACK` `L_BRACE` `R_BRACE` `COMMA` `SEMICOLON`、予約記号 `EQ` `PIPE` `COLON` `DOT` `THIN_ARROW` `LEFT_ARROW` `DOT2`、演算子 `OP` `CONOP` `MINUS`、parser が付け替える `L_ANGLE` `R_ANGLE`、仮想トークン `LAYOUT_OPEN` `LAYOUT_SEP` `LAYOUT_CLOSE`、`ERROR_TOKEN` `EOF`。ノードは `SOURCE_FILE` `ERROR` だけ (Task 4 で足す)
  - `SyntaxKind::is_trivia(self) -> bool` (4 種類の trivia)
  - `pub fn lex(file: FileId, text: &str) -> (Vec<Token>, Vec<Diagnostic>)` (シグネチャは既存のまま)
  - `pub(crate) fn lexer::operator_kind(op: &str) -> SyntaxKind` (演算子の文字の並びを `EQ` / `PIPE` / `COLON` / `DOT` / `THIN_ARROW` / `LEFT_ARROW` / `DOT2` / `MINUS` / `CONOP` / `OP` に分類する。Task 3 がトークンの分割で使う)
  - `codes::UNTERMINATED_BLOCK_COMMENT` (E0005)、`codes::INVALID_NUMBER` (E0007)、`codes::INVALID_ESCAPE` (E0008)
  - `pub(crate) const NOT_YET_SUPPORTED_LABEL: &str = "this is implemented in a later stage"` (`lib.rs`。E0004 のラベル)

暫定構文の字句で書いた既存のテストは、言語設計 spec §8 の合意済みの例外として、次のように機械的に書き換える (意味は変えない)。

| テスト | 書き換え | 理由 |
|---|---|---|
| `tests/lexer.rs` の `all_keywords` | 新しいキーワードの一覧にする。`true` / `false` が識別子になったことは別のテストにする | キーワードが変わった (spec §3) |
| `tests/lexer.rs` の `operators_use_longest_match` | `operators_and_reserved_symbols` に置き換える | 演算子はユーザー定義の `OP` になった |
| `tests/lexer.rs` の `literals_and_comments`、`crlf_line_endings_are_whitespace`、`comment_at_end_of_file_without_newline` | `//` を `--` にする | 行コメントが `--` になった。`//` は演算子 |
| `tests/lexer.rs` の `unexpected_characters_are_merged_into_one_error` | `$@` を `€€` にする | `$` と `@` は演算子の文字になった (spec §10 の指示) |
| `tests/parser.rs` の `trivia_only_file_has_no_errors` | `//` を `--` にする | 同上 |
| `tests/parser.rs` の `lexer_errors_are_not_reported_twice` | `$` を `€` にし、E0003 の位置を 2 から 4 にする (`€` は 3 バイト) | 同上 |
| `tests/parser.rs` の `recovery_resumes_at_item_keywords` | `?` を `€` にする | `?` は演算子の文字になった |
| `src/parser.rs` の `trivia_inside_root_goes_to_the_enclosing_node`、`lookahead_skips_trivia_and_reports_eof` | `//` を `--` にする | 同上 |
| UI テストの `unexpected_character.em`、`multiple_errors.em` | `$` と `@` を `€` にし、コメントを `--` にする | 同上 (spec §10 の指示) |
| UI テストの `comments_only.em` | コメントを `--` にし、「ブロックコメントはない」という文を、入れ子のブロックコメントの例に置き換える | 文の内容が事実でなくなった。期待値 (出力なし) は変わらない |

- [ ] **Step 1: lexer のテストを書き換える**

`crates/eml_syntax/tests/lexer.rs` を次の内容で置き換える。

```rust
use eml_diagnostics::{SourceFiles, render};
use eml_syntax::lex;

/// トークン列を1行1トークンで表示する。
fn dump(text: &str) -> String {
    let mut files = SourceFiles::new();
    let file = files.add("test.em", text);
    let (tokens, diagnostics) = lex(file, text);
    let mut out = String::new();
    for token in &tokens {
        out.push_str(&format!(
            "{:?}@{:?} {:?}\n",
            token.kind, token.range, &text[token.range]
        ));
    }
    let joined: String = tokens.iter().map(|token| &text[token.range]).collect();
    assert_eq!(joined, text, "tokens must cover the whole text");
    if !diagnostics.is_empty() {
        out.push_str("---\n");
        out.push_str(&render(&diagnostics, &files));
    }
    out
}

/// trivia を除いたトークンの種類。
fn kinds(text: &str) -> Vec<String> {
    let mut files = SourceFiles::new();
    let file = files.add("test.em", text);
    let (tokens, _) = lex(file, text);
    let joined: String = tokens.iter().map(|token| &text[token.range]).collect();
    assert_eq!(joined, text, "tokens must cover the whole text");
    tokens
        .iter()
        .filter(|token| !token.kind.is_trivia())
        .map(|token| format!("{:?}", token.kind))
        .collect()
}

/// 診断を `E0001@2..4 message` の形で並べる。
fn diags(text: &str) -> Vec<String> {
    let mut files = SourceFiles::new();
    let file = files.add("test.em", text);
    let (_, diagnostics) = lex(file, text);
    diagnostics
        .iter()
        .map(|d| format!("{}@{:?} {}", d.code, d.primary.range, d.message))
        .collect()
}

#[test]
fn keywords_and_identifiers() {
    insta::assert_snapshot!(dump("fn fnord let _ _x x1 Some IO"), @r#"
    FN_KW@0..2 "fn"
    WHITESPACE@2..3 " "
    LIDENT@3..8 "fnord"
    WHITESPACE@8..9 " "
    LET_KW@9..12 "let"
    WHITESPACE@12..13 " "
    UNDERSCORE@13..14 "_"
    WHITESPACE@14..15 " "
    LIDENT@15..17 "_x"
    WHITESPACE@17..18 " "
    LIDENT@18..20 "x1"
    WHITESPACE@20..21 " "
    UIDENT@21..25 "Some"
    WHITESPACE@25..26 " "
    UIDENT@26..28 "IO"
    "#);
}

#[test]
fn all_keywords() {
    let text = "data type effect where pub import as infixl infixr infix \
                let in if then else match with handle from resume drop return \
                never once multi use fn forall class instance";
    assert_eq!(
        kinds(text),
        [
            "DATA_KW", "TYPE_KW", "EFFECT_KW", "WHERE_KW", "PUB_KW", "IMPORT_KW", "AS_KW",
            "INFIXL_KW", "INFIXR_KW", "INFIX_KW", "LET_KW", "IN_KW", "IF_KW", "THEN_KW",
            "ELSE_KW", "MATCH_KW", "WITH_KW", "HANDLE_KW", "FROM_KW", "RESUME_KW", "DROP_KW",
            "RETURN_KW", "NEVER_KW", "ONCE_KW", "MULTI_KW", "USE_KW", "FN_KW", "FORALL_KW",
            "CLASS_KW", "INSTANCE_KW",
        ]
    );
}

#[test]
fn true_and_false_are_identifiers() {
    assert_eq!(kinds("true false True"), ["LIDENT", "LIDENT", "UIDENT"]);
}

#[test]
fn identifiers_may_contain_primes() {
    assert_eq!(kinds("x' go'' A'b"), ["LIDENT", "LIDENT", "UIDENT"]);
}

#[test]
fn operators_and_reserved_symbols() {
    let text = "( ) [ ] { } , ; = | : . -> <- .. - :: |> <> == => !$@ -->";
    assert_eq!(
        kinds(text),
        [
            "L_PAREN", "R_PAREN", "L_BRACK", "R_BRACK", "L_BRACE", "R_BRACE", "COMMA",
            "SEMICOLON", "EQ", "PIPE", "COLON", "DOT", "THIN_ARROW", "LEFT_ARROW", "DOT2",
            "MINUS", "CONOP", "OP", "OP", "OP", "OP", "OP", "OP",
        ]
    );
}

#[test]
fn literals_and_comments() {
    insta::assert_snapshot!(dump("42 \"a\\n\\\"b\" -- note\n-1"), @r#"
    INT@0..2 "42"
    WHITESPACE@2..3 " "
    STRING@3..11 "\"a\\n\\\"b\""
    WHITESPACE@11..12 " "
    COMMENT@12..19 "-- note"
    WHITESPACE@19..20 "\n"
    MINUS@20..21 "-"
    INT@21..22 "1"
    "#);
}

#[test]
fn dashes_start_a_comment_unless_an_operator_character_follows() {
    insta::assert_snapshot!(dump("x -- note\n---- banner\n-- | doc\ny"), @r#"
    LIDENT@0..1 "x"
    WHITESPACE@1..2 " "
    COMMENT@2..9 "-- note"
    WHITESPACE@9..10 "\n"
    COMMENT@10..21 "---- banner"
    WHITESPACE@21..22 "\n"
    COMMENT@22..30 "-- | doc"
    WHITESPACE@30..31 "\n"
    LIDENT@31..32 "y"
    "#);
    assert_eq!(kinds("a --> b --| c"), ["LIDENT", "OP", "LIDENT", "OP", "LIDENT"]);
}

#[test]
fn block_comments_nest() {
    insta::assert_snapshot!(dump("a {- x {- y -} z -} b"), @r#"
    LIDENT@0..1 "a"
    WHITESPACE@1..2 " "
    BLOCK_COMMENT@2..19 "{- x {- y -} z -}"
    WHITESPACE@19..20 " "
    LIDENT@20..21 "b"
    "#);
}

#[test]
fn unterminated_block_comment_is_an_error() {
    assert_eq!(
        diags("a {- x {- y -}"),
        ["E0005@2..4 unterminated block comment"]
    );
    assert_eq!(kinds("a {- x {- y -}"), ["LIDENT"]);
}

#[test]
fn shebang_is_trivia_only_at_the_start_of_the_file() {
    insta::assert_snapshot!(dump("#!/usr/bin/env eml run\nx"), @r#"
    SHEBANG@0..22 "#!/usr/bin/env eml run"
    WHITESPACE@22..23 "\n"
    LIDENT@23..24 "x"
    "#);
    assert_eq!(kinds("\u{feff}#!x\ny"), ["LIDENT"]);
    assert_eq!(kinds("x\n#!y"), ["LIDENT", "ERROR_TOKEN", "OP", "LIDENT"]);
}

#[test]
fn number_forms() {
    assert_eq!(
        kinds("123 1_000 0xff 0o17 0b1010 1.5 1e9 2.5e-3"),
        ["INT", "INT", "INT", "INT", "INT", "FLOAT", "FLOAT", "FLOAT"]
    );
}

#[test]
fn malformed_numbers_are_errors() {
    assert_eq!(
        diags("0xZZ 12ab 0x"),
        [
            "E0007@0..4 invalid number literal `0xZZ`",
            "E0007@5..9 invalid number literal `12ab`",
            "E0007@10..12 invalid number literal `0x`",
        ]
    );
    assert_eq!(kinds("0xZZ 12ab 0x"), ["INT", "INT", "INT"]);
}

#[test]
fn character_literals() {
    assert_eq!(kinds(r"'a' '\n' x'"), ["CHAR", "CHAR", "LIDENT"]);
}

#[test]
fn float_right_after_a_dot_is_split_into_field_indices() {
    insta::assert_snapshot!(dump("t.0.1"), @r#"
    LIDENT@0..1 "t"
    DOT@1..2 "."
    INT@2..3 "0"
    DOT@3..4 "."
    INT@4..5 "1"
    "#);
    assert_eq!(kinds("x . 0.1"), ["LIDENT", "DOT", "FLOAT"]);
    assert_eq!(kinds("1.5"), ["FLOAT"]);
}

#[test]
fn valid_escapes_have_no_errors() {
    assert!(diags(r#""\n\t\r\\\"\0\u{1F600}""#).is_empty());
}

#[test]
fn invalid_escapes_are_errors() {
    assert_eq!(
        diags(r#""a\qb""#),
        ["E0008@2..4 unknown escape sequence `\\q`"]
    );
    assert_eq!(
        diags(r#""\u{110000}""#),
        ["E0008@1..11 invalid unicode escape `\\u{110000}`"]
    );
}

#[test]
fn unterminated_string_becomes_string_with_error() {
    insta::assert_snapshot!(dump("\"abc\nx"), @r#"
    STRING@0..4 "\"abc"
    WHITESPACE@4..5 "\n"
    LIDENT@5..6 "x"
    ---
    [E0002] Error: unterminated string literal
       ╭─[ test.em:1:1 ]
       │
     1 │ "abc
       │ ──┬─  
       │   ╰─── missing closing `"`
    ───╯
    "#);
}

#[test]
fn backslash_at_end_of_line_gives_only_the_unterminated_error() {
    assert_eq!(diags("\"abc\\\nx"), ["E0002@0..5 unterminated string literal"]);
    assert_eq!(kinds("\"abc\\\nx"), ["STRING", "LIDENT"]);
}

#[test]
fn unterminated_string_does_not_swallow_carriage_return() {
    assert_eq!(diags("\"abc\r\nx"), ["E0002@0..4 unterminated string literal"]);
    assert_eq!(kinds("\"abc\r\nx"), ["STRING", "LIDENT"]);
}

#[test]
fn interpolation_is_skipped_with_not_yet_supported() {
    let text = r#""a\{f "x"} b" c"#;
    assert_eq!(
        diags(text),
        ["E0004@2..10 string interpolation is not supported yet"]
    );
    assert_eq!(kinds(text), ["STRING", "LIDENT"]);
}

#[test]
fn later_stage_literals_are_single_tokens() {
    insta::assert_snapshot!(dump("\"\"\"\n  a\n  \"\"\" r\"x\" r#\"y\"z\"# `ls -l`"), @r#"
    MULTILINE_STRING@0..13 "\"\"\"\n  a\n  \"\"\""
    WHITESPACE@13..14 " "
    RAW_STRING@14..18 "r\"x\""
    WHITESPACE@18..19 " "
    RAW_STRING@19..27 "r#\"y\"z\"#"
    WHITESPACE@27..28 " "
    COMMAND@28..35 "`ls -l`"
    "#);
}

#[test]
fn unterminated_later_stage_literals_are_errors() {
    assert_eq!(diags("`ls"), ["E0002@0..3 unterminated command literal"]);
    assert_eq!(diags("r#\"abc"), ["E0002@0..3 unterminated raw string"]);
    assert_eq!(diags("\"\"\"abc"), ["E0002@0..3 unterminated multi-line string"]);
}

#[test]
fn unexpected_characters_are_merged_into_one_error() {
    insta::assert_snapshot!(dump("a €€ b é"), @r#"
    LIDENT@0..1 "a"
    WHITESPACE@1..2 " "
    ERROR_TOKEN@2..8 "€€"
    WHITESPACE@8..9 " "
    LIDENT@9..10 "b"
    WHITESPACE@10..11 " "
    ERROR_TOKEN@11..13 "é"
    ---
    [E0001] Error: unexpected character `€€`
       ╭─[ test.em:1:3 ]
       │
     1 │ a €€ b é
       │   ─┬  
       │    ╰── not valid in eml source
    ───╯
    [E0001] Error: unexpected character `é`
       ╭─[ test.em:1:8 ]
       │
     1 │ a €€ b é
       │        ┬  
       │        ╰── not valid in eml source
    ───╯
    "#);
}

#[test]
fn unexpected_character_messages_escape_and_truncate() {
    assert_eq!(diags("a\u{1}b"), ["E0001@1..2 unexpected character `\\u{1}`"]);
    assert_eq!(
        diags(&"€".repeat(20)),
        ["E0001@0..60 unexpected character `€€€€€€€€€€€€€€€€…`"]
    );
}

#[test]
fn backslash_outside_strings_is_unexpected() {
    assert_eq!(diags("a \\ b"), ["E0001@2..3 unexpected character `\\`"]);
}

#[test]
fn crlf_line_endings_are_whitespace() {
    insta::assert_snapshot!(dump("a\r\nb -- c\r\n"), @r#"
    LIDENT@0..1 "a"
    WHITESPACE@1..3 "\r\n"
    LIDENT@3..4 "b"
    WHITESPACE@4..5 " "
    COMMENT@5..9 "-- c"
    WHITESPACE@9..11 "\r\n"
    "#);
}

#[test]
fn byte_order_mark_is_whitespace() {
    insta::assert_snapshot!(dump("\u{feff}fn"), @r#"
    WHITESPACE@0..3 "\u{feff}"
    FN_KW@3..5 "fn"
    "#);
}

#[test]
fn comment_at_end_of_file_without_newline() {
    insta::assert_snapshot!(dump("x -- end"), @r#"
    LIDENT@0..1 "x"
    WHITESPACE@1..2 " "
    COMMENT@2..8 "-- end"
    "#);
}
```

- [ ] **Step 2: テストが失敗することを確認する**

Run: `cargo test -p eml_syntax --test lexer`
Expected: コンパイルは通るが、`all_keywords`、`operators_and_reserved_symbols`、`block_comments_nest` など多数が FAIL (古い lexer は `--` や `{-` を知らない)

- [ ] **Step 3: `SyntaxKind` を置き換える**

`crates/eml_syntax/src/syntax_kind.rs` の `use logos::Logos;` と、enum の定義全体を次のものに置き換える (`EmlLanguage` 以下とテストは変えない)。

```rust
/// トークンと構文ノードの種類。トークン (`EOF` まで) を先に並べ、`TokenSet` が 128 ビットに収まるようにする。
/// 字句の規則は構文設計 spec §3 に従う。字句解析は `lexer` が行う。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u16)]
#[allow(non_camel_case_types)]
pub enum SyntaxKind {
    // trivia
    WHITESPACE = 0,
    /// `--` から行末まで。`-- |` のドキュメントコメントも字句としてはこれ。
    COMMENT,
    /// 入れ子にできる `{- -}`。
    BLOCK_COMMENT,
    /// ファイルの先頭の `#!` の行。
    SHEBANG,

    // リテラルと識別子
    INT,
    /// 浮動小数。S1 では使うと E0004。
    FLOAT,
    /// 文字。S1 では使うと E0004。
    CHAR,
    /// 通常の文字列。S1 では補間を含めて1つのトークン。
    STRING,
    /// `"""` の複数行の文字列。S1 では使うと E0004。
    MULTILINE_STRING,
    /// `r"..."` / `r#"..."#`。S1 では使うと E0004。
    RAW_STRING,
    /// バッククォートのコマンドリテラル。S1 では使うと E0004。
    COMMAND,
    LIDENT,
    UIDENT,
    UNDERSCORE,

    // キーワード
    DATA_KW,
    TYPE_KW,
    EFFECT_KW,
    WHERE_KW,
    PUB_KW,
    IMPORT_KW,
    AS_KW,
    INFIXL_KW,
    INFIXR_KW,
    INFIX_KW,
    LET_KW,
    IN_KW,
    IF_KW,
    THEN_KW,
    ELSE_KW,
    MATCH_KW,
    WITH_KW,
    HANDLE_KW,
    FROM_KW,
    RESUME_KW,
    DROP_KW,
    RETURN_KW,
    NEVER_KW,
    ONCE_KW,
    MULTI_KW,
    USE_KW,
    FN_KW,
    /// 将来のために予約する。
    FORALL_KW,
    CLASS_KW,
    INSTANCE_KW,

    // 区切り記号
    L_PAREN,
    R_PAREN,
    L_BRACK,
    R_BRACK,
    L_BRACE,
    R_BRACE,
    COMMA,
    SEMICOLON,

    // 予約記号 (演算子にならない)
    EQ,
    PIPE,
    COLON,
    DOT,
    THIN_ARROW,
    LEFT_ARROW,
    DOT2,

    // 演算子
    /// ユーザーが定義できる演算子。
    OP,
    /// `:` で始まる、コンストラクタの演算子。
    CONOP,
    /// `-`。中置の引き算と、前置の負号の両方に使う。
    MINUS,

    /// row の `<` と `>`。lexer は `OP` にし、parser が型の中で付け替える (分割することもある)。
    L_ANGLE,
    R_ANGLE,

    /// レイアウト段の仮想トークン。parser の入力にだけ現れ、木には入らない。
    LAYOUT_OPEN,
    LAYOUT_SEP,
    LAYOUT_CLOSE,

    /// 字句として認識できない文字の並び。
    ERROR_TOKEN,
    /// 入力の終わり。パーサの中でだけ使い、木には現れない。
    EOF,

    // ノード
    SOURCE_FILE,
    ERROR,

    #[doc(hidden)]
    __LAST,
}

impl SyntaxKind {
    pub fn is_trivia(self) -> bool {
        matches!(
            self,
            SyntaxKind::WHITESPACE
                | SyntaxKind::COMMENT
                | SyntaxKind::BLOCK_COMMENT
                | SyntaxKind::SHEBANG
        )
    }

    fn from_raw(raw: u16) -> SyntaxKind {
        assert!(raw < SyntaxKind::__LAST as u16, "invalid SyntaxKind {raw}");
        // SAFETY: `SyntaxKind` は `repr(u16)` で、0 から `__LAST` まで値が連続している。
        unsafe { std::mem::transmute::<u16, SyntaxKind>(raw) }
    }
}
```

- [ ] **Step 4: 番号とラベルを `lib.rs` に足す**

`crates/eml_syntax/src/lib.rs` の `codes` を次のものに置き換え、その下に定数を足す。

```rust
/// 字句・構文の診断の番号 (E0xxx)。
pub mod codes {
    use eml_diagnostics::ErrorCode;

    pub const UNEXPECTED_CHARACTER: ErrorCode = ErrorCode(1);
    pub const UNTERMINATED_STRING: ErrorCode = ErrorCode(2);
    pub const EXPECTED_ITEM: ErrorCode = ErrorCode(3);
    pub const NOT_YET_SUPPORTED: ErrorCode = ErrorCode(4);
    pub const UNTERMINATED_BLOCK_COMMENT: ErrorCode = ErrorCode(5);
    pub const INVALID_NUMBER: ErrorCode = ErrorCode(7);
    pub const INVALID_ESCAPE: ErrorCode = ErrorCode(8);
}

/// E0004 (まだ対応していない構文) のラベル。
pub(crate) const NOT_YET_SUPPORTED_LABEL: &str = "this is implemented in a later stage";
```

- [ ] **Step 5: lexer を書き直す**

`crates/eml_syntax/src/lexer.rs` を次の内容で置き換える。

```rust
//! 字句解析 (構文設計 spec §3)。logos で単純なトークンを切り出し、文字列・コメント・演算子の分類などを
//! 手書きの層で扱う。トークン列をつなげると元のテキストに戻る (lossless)。

use eml_diagnostics::{Diagnostic, ErrorCode, FileId, Label, TextRange, TextSize};
use logos::Logos;

use crate::SyntaxKind::{self, *};
use crate::{NOT_YET_SUPPORTED_LABEL, codes};

/// 字句解析の結果の1トークン。trivia (空白とコメント) も含む。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Token {
    pub kind: SyntaxKind,
    pub range: TextRange,
}

/// テキストをトークン列に分ける。認識できない文字の並びは1つの `ERROR_TOKEN` にまとめ、診断を1件出す。
pub fn lex(file: FileId, text: &str) -> (Vec<Token>, Vec<Diagnostic>) {
    let mut lexer = Lexer {
        file,
        text,
        pos: 0,
        tokens: Vec::new(),
        diagnostics: Vec::new(),
    };
    lexer.run();
    let Lexer {
        tokens,
        mut diagnostics,
        ..
    } = lexer;
    for token in tokens.iter().filter(|token| token.kind == ERROR_TOKEN) {
        diagnostics.push(Diagnostic::error(
            codes::UNEXPECTED_CHARACTER,
            format!("unexpected character `{}`", printable(&text[token.range])),
            Label::new(file, token.range, "not valid in eml source"),
        ));
    }
    diagnostics.sort_by_key(|diagnostic| diagnostic.primary.range.start());
    (tokens, diagnostics)
}

/// 演算子の文字の並びを分類する。予約記号 (spec §3) は演算子にならない。
pub(crate) fn operator_kind(op: &str) -> SyntaxKind {
    match op {
        "=" => EQ,
        "|" => PIPE,
        ":" => COLON,
        "." => DOT,
        "->" => THIN_ARROW,
        "<-" => LEFT_ARROW,
        ".." => DOT2,
        "-" => MINUS,
        _ if op.starts_with(':') => CONOP,
        _ => OP,
    }
}

/// logos で切り出す単純なトークン。キーワードや演算子の種類は、切り出した後で決める。
#[derive(Logos, Debug, Clone, Copy, PartialEq, Eq)]
enum Raw {
    #[regex(r"[ \t\r\n\u{FEFF}]+")]
    Whitespace,
    #[regex(r"[a-z_][A-Za-z0-9_']*")]
    Lower,
    #[regex(r"[A-Z][A-Za-z0-9_']*")]
    Upper,
    /// 整数、指数だけの浮動小数 (`1e9`)、形の不正な数値 (`0xZZ`)。種類は `number_kind` で決める。
    #[regex(r"[0-9][0-9A-Za-z_]*")]
    Number,
    #[regex(r"[0-9][0-9_]*\.[0-9][0-9_]*([eE][+-]?[0-9][0-9_]*)?")]
    #[regex(r"[0-9][0-9_]*[eE][+-][0-9][0-9_]*")]
    Float,
    #[regex(r"'([^'\\\n]|\\[^\n]|\\u\{[0-9A-Fa-f]*\})'")]
    Char,
    #[regex(r"[!$%&*+\-./<=>?@^|~:]+")]
    Op,
    #[token("(")]
    LParen,
    #[token(")")]
    RParen,
    #[token("[")]
    LBrack,
    #[token("]")]
    RBrack,
    #[token("{")]
    LBrace,
    #[token("}")]
    RBrace,
    #[token(",")]
    Comma,
    #[token(";")]
    Semicolon,
}

struct Lexer<'a> {
    file: FileId,
    text: &'a str,
    /// 次に読むバイト位置。常に文字の境界にある。
    pos: usize,
    tokens: Vec<Token>,
    diagnostics: Vec<Diagnostic>,
}

impl Lexer<'_> {
    fn run(&mut self) {
        // shebang はファイルの先頭 (BOM の直後を含む) にだけ書ける。
        let shebang_at = if self.text.starts_with('\u{feff}') {
            '\u{feff}'.len_utf8()
        } else {
            0
        };
        let text = self.text;
        while self.pos < text.len() {
            let rest = &text[self.pos..];
            if self.pos == shebang_at && rest.starts_with("#!") {
                self.push(SHEBANG, self.pos + line_len(rest));
            } else if rest.starts_with("{-") {
                self.block_comment();
            } else if rest.starts_with("\"\"\"") {
                self.multiline_string();
            } else if rest.starts_with('"') {
                self.string();
            } else if let Some(hashes) = raw_string_hashes(rest) {
                self.raw_string(hashes);
            } else if rest.starts_with('`') {
                self.command();
            } else {
                self.simple();
            }
        }
    }

    /// `self.pos` から `end` までを1つのトークンにする。連続する `ERROR_TOKEN` は1つにまとめる。
    fn push(&mut self, kind: SyntaxKind, end: usize) {
        let range = self.range(self.pos, end);
        match self.tokens.last_mut() {
            Some(last) if kind == ERROR_TOKEN && last.kind == ERROR_TOKEN => {
                last.range = last.range.cover(range);
            }
            _ => self.tokens.push(Token { kind, range }),
        }
        self.pos = end;
    }

    fn range(&self, start: usize, end: usize) -> TextRange {
        TextRange::new(TextSize::new(start as u32), TextSize::new(end as u32))
    }

    fn error(
        &mut self,
        code: ErrorCode,
        message: impl Into<String>,
        start: usize,
        end: usize,
        label: impl Into<String>,
    ) {
        let range = self.range(start, end);
        self.diagnostics.push(Diagnostic::error(
            code,
            message,
            Label::new(self.file, range, label),
        ));
    }

    /// logos で1トークンを切り出す。
    fn simple(&mut self) {
        let text = self.text;
        let rest = &text[self.pos..];
        let mut raw = Raw::lexer(rest);
        let result = raw.next().expect("the rest of the text is not empty");
        // 認識できない文字は1文字ずつ進める (文字の境界を保つため)。
        let len = match result {
            Ok(_) => raw.span().end,
            Err(()) => rest.chars().next().map_or(1, char::len_utf8),
        };
        let slice = &rest[..len];
        let kind = match result {
            Ok(Raw::Whitespace) => WHITESPACE,
            Ok(Raw::Lower) if slice == "_" => UNDERSCORE,
            Ok(Raw::Lower) => keyword(slice).unwrap_or(LIDENT),
            Ok(Raw::Upper) => UIDENT,
            Ok(Raw::Number) => number_kind(slice).unwrap_or_else(|| {
                self.error(
                    codes::INVALID_NUMBER,
                    format!("invalid number literal `{slice}`"),
                    self.pos,
                    self.pos + len,
                    "not a valid number",
                );
                INT
            }),
            Ok(Raw::Float) => {
                if self.split_field_index(len) {
                    return;
                }
                FLOAT
            }
            Ok(Raw::Char) => CHAR,
            Ok(Raw::Op) if slice.len() >= 2 && slice.bytes().all(|b| b == b'-') => {
                // `-` が2つ以上並び、その後ろに演算子の文字が続かなければ、行コメント (Haskell と同じ)。
                self.push(COMMENT, self.pos + line_len(rest));
                return;
            }
            Ok(Raw::Op) => operator_kind(slice),
            Ok(Raw::LParen) => L_PAREN,
            Ok(Raw::RParen) => R_PAREN,
            Ok(Raw::LBrack) => L_BRACK,
            Ok(Raw::RBrack) => R_BRACK,
            Ok(Raw::LBrace) => L_BRACE,
            Ok(Raw::RBrace) => R_BRACE,
            Ok(Raw::Comma) => COMMA,
            Ok(Raw::Semicolon) => SEMICOLON,
            Err(()) => ERROR_TOKEN,
        };
        self.push(kind, self.pos + len);
    }

    /// `t.0.1` の `0.1` のように、空白なしの `.` の直後にある `数字.数字` を、`INT` `DOT` `INT` に分ける。
    fn split_field_index(&mut self, len: usize) -> bool {
        let text = self.text;
        let start = self.pos;
        let float = &text[start..start + len];
        let after_dot = matches!(
            self.tokens.last(),
            Some(last) if last.kind == DOT && last.range.end() == TextSize::new(start as u32)
        );
        let Some((left, right)) = float.split_once('.') else {
            return false;
        };
        if !after_dot || !right.bytes().all(|b| b.is_ascii_digit() || b == b'_') {
            return false;
        }
        self.push(INT, start + left.len());
        self.push(DOT, start + left.len() + 1);
        self.push(INT, start + len);
        true
    }

    /// 通常の文字列 `"..."`。行をまたげない。閉じていなければ行末までを `STRING` にする。
    fn string(&mut self) {
        let text = self.text;
        let start = self.pos;
        let mut i = start + 1;
        let mut terminated = false;
        while let Some(c) = text[i..].chars().next() {
            match c {
                '"' => {
                    i += 1;
                    terminated = true;
                    break;
                }
                '\n' => break,
                '\r' if text[i + 1..].starts_with('\n') => break,
                '\\' => i = self.escape(i),
                _ => i += c.len_utf8(),
            }
        }
        if !terminated {
            self.error(
                codes::UNTERMINATED_STRING,
                "unterminated string literal",
                start,
                i,
                "missing closing `\"`",
            );
        }
        self.push(STRING, i);
    }

    /// `i` にある `\` から始まるエスケープを読み、その次の位置を返す。不正なら E0008 を出す。
    fn escape(&mut self, i: usize) -> usize {
        let text = self.text;
        let Some(c) = text[i + 1..].chars().next() else {
            return i + 1;
        };
        match c {
            // 行末の `\` は文字列の外に出ない。文字列は閉じていない扱いになり、E0002 だけを出す。
            '\n' | '\r' => i + 1,
            'n' | 't' | 'r' | '\\' | '"' | '0' => i + 2,
            'u' => self.unicode_escape(i),
            '{' => self.interpolation(i),
            _ => {
                let end = i + 1 + c.len_utf8();
                self.error(
                    codes::INVALID_ESCAPE,
                    format!("unknown escape sequence `\\{c}`"),
                    i,
                    end,
                    "not a valid escape",
                );
                end
            }
        }
    }

    /// `\u{XXXX}`。16進で1〜6桁の、Unicode のスカラー値であること。
    fn unicode_escape(&mut self, i: usize) -> usize {
        let text = self.text;
        let rest = &text[i + 2..];
        let line = &rest[..line_len(rest)];
        let (end, valid) = match line.strip_prefix('{').and_then(|inner| inner.find('}')) {
            Some(close) => {
                let hex = &line[1..close + 1];
                let valid = (1..=6).contains(&hex.len())
                    && hex.bytes().all(|b| b.is_ascii_hexdigit())
                    && u32::from_str_radix(hex, 16)
                        .ok()
                        .and_then(char::from_u32)
                        .is_some();
                (i + 2 + close + 2, valid)
            }
            None => (i + 2, false),
        };
        if !valid {
            self.error(
                codes::INVALID_ESCAPE,
                format!("invalid unicode escape `{}`", &text[i..end]),
                i,
                end,
                "expected `\\u{` followed by 1 to 6 hex digits and `}`",
            );
        }
        end
    }

    /// 補間 `\{...}` は S2 で実装する。S1 では対応する `}` まで読み飛ばし、E0004 を出す。
    /// 穴の中の文字列も読み飛ばすので、`"\{f "x"}"` の内側の `"` で外側の文字列が終わらない。
    fn interpolation(&mut self, i: usize) -> usize {
        let text = self.text;
        let mut j = i + 2;
        let mut depth = 1;
        while let Some(c) = text[j..].chars().next() {
            match c {
                '\n' => break,
                '{' => {
                    depth += 1;
                    j += 1;
                }
                '}' => {
                    depth -= 1;
                    j += 1;
                    if depth == 0 {
                        break;
                    }
                }
                '"' => j = skip_simple_string(text, j),
                _ => j += c.len_utf8(),
            }
        }
        self.error(
            codes::NOT_YET_SUPPORTED,
            "string interpolation is not supported yet",
            i,
            j,
            NOT_YET_SUPPORTED_LABEL,
        );
        j
    }

    /// 複数行の文字列 `"""..."""` は S2 で実装する。S1 では閉じの `"""` までを1つのトークンにする。
    fn multiline_string(&mut self) {
        let text = self.text;
        let start = self.pos;
        let end = match text[start + 3..].find("\"\"\"") {
            Some(offset) => start + 3 + offset + 3,
            None => {
                self.error(
                    codes::UNTERMINATED_STRING,
                    "unterminated multi-line string",
                    start,
                    start + 3,
                    "missing closing `\"\"\"`",
                );
                text.len()
            }
        };
        self.push(MULTILINE_STRING, end);
    }

    /// raw 文字列 `r"..."` / `r#"..."#` は S2 で実装する。S1 では閉じまでを1つのトークンにする。行をまたげる。
    fn raw_string(&mut self, hashes: usize) {
        let text = self.text;
        let start = self.pos;
        let open_len = 1 + hashes + 1;
        let close = format!("\"{}", "#".repeat(hashes));
        let end = match text[start + open_len..].find(&close) {
            Some(offset) => start + open_len + offset + close.len(),
            None => {
                self.error(
                    codes::UNTERMINATED_STRING,
                    "unterminated raw string",
                    start,
                    start + open_len,
                    format!("missing closing `{close}`"),
                );
                text.len()
            }
        };
        self.push(RAW_STRING, end);
    }

    /// コマンドリテラルは S3 で実装する。S1 では閉じのバッククォートまでを1つのトークンにする。行はまたげない。
    fn command(&mut self) {
        let text = self.text;
        let start = self.pos;
        let mut i = start + 1;
        let mut terminated = false;
        while let Some(c) = text[i..].chars().next() {
            match c {
                '`' => {
                    i += 1;
                    terminated = true;
                    break;
                }
                '\n' => break,
                '\r' if text[i + 1..].starts_with('\n') => break,
                '\\' => {
                    i += 1;
                    if let Some(next) = text[i..].chars().next().filter(|&next| next != '\n') {
                        i += next.len_utf8();
                    }
                }
                _ => i += c.len_utf8(),
            }
        }
        if !terminated {
            self.error(
                codes::UNTERMINATED_STRING,
                "unterminated command literal",
                start,
                i,
                "missing closing backtick",
            );
        }
        self.push(COMMAND, i);
    }

    /// 入れ子にできるブロックコメント `{- ... -}`。
    fn block_comment(&mut self) {
        let text = self.text;
        let start = self.pos;
        let mut i = start + 2;
        let mut depth = 1;
        while i < text.len() {
            let rest = &text[i..];
            if rest.starts_with("{-") {
                depth += 1;
                i += 2;
            } else if rest.starts_with("-}") {
                depth -= 1;
                i += 2;
                if depth == 0 {
                    break;
                }
            } else {
                i += rest.chars().next().map_or(1, char::len_utf8);
            }
        }
        if depth > 0 {
            self.error(
                codes::UNTERMINATED_BLOCK_COMMENT,
                "unterminated block comment",
                start,
                start + 2,
                "missing closing `-}`",
            );
        }
        self.push(BLOCK_COMMENT, i);
    }
}

/// 行末 (`\n` または `\r\n`) の手前までの長さ。
fn line_len(rest: &str) -> usize {
    let end = rest.find('\n').unwrap_or(rest.len());
    if rest[..end].ends_with('\r') {
        end - 1
    } else {
        end
    }
}

/// 補間の穴の中の文字列を読み飛ばす。`j` は開きの `"` の位置。閉じの `"` の次 (なければ行末) を返す。
fn skip_simple_string(text: &str, j: usize) -> usize {
    let mut k = j + 1;
    while let Some(c) = text[k..].chars().next() {
        match c {
            '"' => return k + 1,
            '\n' => return k,
            '\\' => {
                k += 1;
                if let Some(next) = text[k..].chars().next().filter(|&next| next != '\n') {
                    k += next.len_utf8();
                }
            }
            _ => k += c.len_utf8(),
        }
    }
    k
}

/// `r"` または `r#..#"` で始まっていれば、`#` の数を返す。
fn raw_string_hashes(rest: &str) -> Option<usize> {
    let after_r = rest.strip_prefix('r')?;
    let hashes = after_r.bytes().take_while(|&b| b == b'#').count();
    after_r[hashes..].starts_with('"').then_some(hashes)
}

/// `Raw::Number` の種類。整数 (10進、`0x`、`0o`、`0b`) なら `INT`、`1e9` の形なら `FLOAT`、不正なら `None`。
fn number_kind(number: &str) -> Option<SyntaxKind> {
    let digits = |body: &str, radix: u32| {
        body.chars().any(|c| c != '_') && body.chars().all(|c| c == '_' || c.is_digit(radix))
    };
    let radix_body = [("0x", 16), ("0o", 8), ("0b", 2)]
        .into_iter()
        .find_map(|(prefix, radix)| number.strip_prefix(prefix).map(|body| (body, radix)));
    if let Some((body, radix)) = radix_body {
        return digits(body, radix).then_some(INT);
    }
    if digits(number, 10) {
        return Some(INT);
    }
    let (mantissa, exponent) = number.split_once(['e', 'E'])?;
    (digits(mantissa, 10) && digits(exponent, 10)).then_some(FLOAT)
}

fn keyword(ident: &str) -> Option<SyntaxKind> {
    Some(match ident {
        "data" => DATA_KW,
        "type" => TYPE_KW,
        "effect" => EFFECT_KW,
        "where" => WHERE_KW,
        "pub" => PUB_KW,
        "import" => IMPORT_KW,
        "as" => AS_KW,
        "infixl" => INFIXL_KW,
        "infixr" => INFIXR_KW,
        "infix" => INFIX_KW,
        "let" => LET_KW,
        "in" => IN_KW,
        "if" => IF_KW,
        "then" => THEN_KW,
        "else" => ELSE_KW,
        "match" => MATCH_KW,
        "with" => WITH_KW,
        "handle" => HANDLE_KW,
        "from" => FROM_KW,
        "resume" => RESUME_KW,
        "drop" => DROP_KW,
        "return" => RETURN_KW,
        "never" => NEVER_KW,
        "once" => ONCE_KW,
        "multi" => MULTI_KW,
        "use" => USE_KW,
        "fn" => FN_KW,
        "forall" => FORALL_KW,
        "class" => CLASS_KW,
        "instance" => INSTANCE_KW,
        _ => return None,
    })
}

/// 診断のメッセージに埋め込むため、制御文字をエスケープし、長い並びを切り詰める。
fn printable(snippet: &str) -> String {
    const MAX_CHARS: usize = 16;
    let mut out = String::new();
    for c in snippet.chars().take(MAX_CHARS) {
        if c.is_control() {
            out.extend(c.escape_default());
        } else {
            out.push(c);
        }
    }
    if snippet.chars().count() > MAX_CHARS {
        out.push('…');
    }
    out
}
```

logos がパターンの重なりを報告した場合 (`Number` と `Float` の共通の接頭辞など) は、`#[regex(..., priority = N)]` で優先順位を付けて解消する。最長一致の結果はテストで確かめる。

- [ ] **Step 6: lexer のテストが通ることを確認する**

Run: `cargo test -p eml_syntax --test lexer`
Expected: PASS (すべて)

- [ ] **Step 7: 暫定構文の字句に依存する parser のテストを書き換える**

`crates/eml_syntax/tests/parser.rs` の3件を、次のように書き換える (ほかは変えない)。

```rust
#[test]
fn trivia_only_file_has_no_errors() {
    insta::assert_snapshot!(dump("-- only a comment\n\n"), @r#"
    SOURCE_FILE@0..19
      COMMENT@0..17 "-- only a comment"
      WHITESPACE@17..19 "\n\n"
    "#);
}
```

```rust
#[test]
fn lexer_errors_are_not_reported_twice() {
    let text = "€ x";
    let mut files = SourceFiles::new();
    let file = files.add("test.em", text);
    let (_, diagnostics) = parse(file, text);
    let codes: Vec<String> = diagnostics.iter().map(|d| d.code.to_string()).collect();
    // `€` は字句解析の E0001 だけ。続く `x` は項目ではないので E0003 を1件出す。
    assert_eq!(codes, ["E0001", "E0003"]);
    assert_eq!(u32::from(diagnostics[1].primary.range.start()), 4);
}
```

`recovery_resumes_at_item_keywords` の `let text = ...;` の行だけを次のものにする。

```rust
    let text = "fn main () : Unit { } € type T { } effect E { }";
```

`crates/eml_syntax/src/parser.rs` の単体テストで、`trivia_inside_root_goes_to_the_enclosing_node` の入力を `" a -- c\n b "` に、スナップショットの `COMMENT@3..7 "// c"` を `COMMENT@3..7 "-- c"` にする。`lookahead_skips_trivia_and_reports_eof` の入力を `"a  -- c\n b"` にする。

- [ ] **Step 8: UI テストのソースとスナップショットを書き換える**

`tests/ui/check-fail/unexpected_character.em`:

```
-- `€` is not a valid character in eml source.
€
```

`tests/ui/check-fail/multiple_errors.em`:

```
-- Independent errors are all reported in one run.
€
foo
€
```

`tests/ui/run/comments_only.em`:

```
-- A file with only comments and blank lines.

{- Block comments {- nest -} too. -}
```

`crates/eml_cli/tests/snapshots/ui__check_fail@unexpected_character.em.snap`:

```
---
source: crates/eml_cli/tests/ui.rs
expression: rendered
input_file: tests/ui/check-fail/unexpected_character.em
---
[E0001] Error: unexpected character `€`
   ╭─[ check-fail/unexpected_character.em:2:1 ]
   │
 2 │ €
   │ ┬  
   │ ╰── not valid in eml source
───╯
```

`crates/eml_cli/tests/snapshots/ui__check_fail@multiple_errors.em.snap`:

```
---
source: crates/eml_cli/tests/ui.rs
expression: rendered
input_file: tests/ui/check-fail/multiple_errors.em
---
[E0001] Error: unexpected character `€`
   ╭─[ check-fail/multiple_errors.em:2:1 ]
   │
 2 │ €
   │ ┬  
   │ ╰── not valid in eml source
───╯
[E0003] Error: expected an item (`fn`, `type`, or `effect`)
   ╭─[ check-fail/multiple_errors.em:3:1 ]
   │
 3 │ foo
   │ ─┬─  
   │  ╰─── not the start of an item
───╯
[E0001] Error: unexpected character `€`
   ╭─[ check-fail/multiple_errors.em:4:1 ]
   │
 4 │ €
   │ ┬  
   │ ╰── not valid in eml source
───╯
```

スナップショットのファイルは、行末の空白 (`┬` の行の後ろの2つの空白) を含めて、既存のファイルの `$` / `@` を `€` に置き換えるだけにする。

- [ ] **Step 9: 全体のテストと lint を通す**

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check`
Expected: すべて PASS、警告なし。`cargo insta` の未承認のスナップショットが残っていないこと (`cargo insta pending-snapshots` が空)

- [ ] **Step 10: Commit**

```bash
git add crates/eml_syntax crates/eml_cli/tests/snapshots tests/ui
git commit -m "Rewrite the lexer for the final syntax"
```

### Task 2: レイアウト段

**Files:**
- Create: `crates/eml_syntax/src/layout.rs` (レイアウト段と、その単体テスト)
- Modify: `crates/eml_syntax/src/lib.rs` (`mod layout;`、`codes` に E0006 / E0009 を足す)

**Interfaces:**
- Consumes: `lex` の `Token` 列 (trivia を含む)、`SyntaxKind` (Task 1)
- Produces:
  - `pub(crate) fn layout::layout(file: FileId, text: &str, tokens: &[Token]) -> (Vec<Token>, Vec<Diagnostic>)`。trivia を除いたトークン列に、`LAYOUT_OPEN` / `LAYOUT_SEP` / `LAYOUT_CLOSE` を挿入して返す。仮想トークンの範囲は幅 0 で、`OPEN` / `SEP` は行の先頭のトークンの開始位置、閉じ括弧の前の `CLOSE` はその括弧の開始位置、ファイルの終わりの `CLOSE` はテキストの末尾、E0009 の回復で入れる空のブロックは開始トークンの終わりの位置
  - `codes::TAB_INDENTATION` (E0006)、`codes::EXPECTED_INDENTED_BLOCK` (E0009)

規則は spec §4 のとおり。この計画の決定として、開始トークンで行が終わり、次の行が深くない (ファイルの終わり、または次の行の先頭が閉じ括弧の場合を含む) ときは、E0009 を出して空のブロックを入れる。`;` はそのまま parser に渡す (parser が `LAYOUT_SEP` と同じに扱う)。

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_syntax/src/layout.rs` を、テストだけを持つ形で作る (実装は Step 3)。

```rust
//! レイアウト段 (構文設計 spec §4)。

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SyntaxKind::*;
    use crate::lexer::lex;
    use eml_diagnostics::SourceFiles;

    /// レイアウト段の出力を、トークンのテキストと `<OPEN>` / `<SEP>` / `<CLOSE>` を空白で区切って並べる。
    /// 診断は `E0009@2..3` の形で返す。
    fn dump(text: &str) -> (String, Vec<String>) {
        let mut files = SourceFiles::new();
        let file = files.add("test.em", text);
        let (tokens, _) = lex(file, text);
        let (out, diagnostics) = layout(file, text, &tokens);
        let shown: Vec<String> = out
            .iter()
            .map(|token| match token.kind {
                LAYOUT_OPEN => "<OPEN>".to_string(),
                LAYOUT_SEP => "<SEP>".to_string(),
                LAYOUT_CLOSE => "<CLOSE>".to_string(),
                _ => text[token.range].to_string(),
            })
            .collect();
        let diagnostics = diagnostics
            .iter()
            .map(|d| format!("{}@{:?}", d.code, d.primary.range))
            .collect();
        (shown.join(" "), diagnostics)
    }

    fn layout_of(text: &str) -> String {
        let (shown, diagnostics) = dump(text);
        assert!(diagnostics.is_empty(), "unexpected diagnostics: {diagnostics:?}");
        shown
    }

    #[test]
    fn top_level_items_are_separated() {
        assert_eq!(layout_of("a = 1\nb = 2"), "a = 1 <SEP> b = 2");
    }

    #[test]
    fn first_item_has_no_separator() {
        assert_eq!(layout_of("\n\na = 1"), "a = 1");
    }

    #[test]
    fn starter_at_end_of_line_opens_a_block() {
        assert_eq!(
            layout_of("f =\n  a\n  b\ng"),
            "f = <OPEN> a <SEP> b <CLOSE> <SEP> g"
        );
    }

    #[test]
    fn deeper_line_without_starter_continues_the_line() {
        assert_eq!(
            layout_of("x =\n  lines s\n    |> f\n  y"),
            "x = <OPEN> lines s |> f <SEP> y <CLOSE>"
        );
    }

    #[test]
    fn starter_in_the_middle_of_a_line_opens_nothing() {
        assert_eq!(layout_of("x = if c then a else b"), "x = if c then a else b");
    }

    #[test]
    fn dedent_closes_several_blocks() {
        assert_eq!(
            layout_of("f =\n  g =\n    a\nh"),
            "f = <OPEN> g = <OPEN> a <CLOSE> <CLOSE> <SEP> h"
        );
    }

    #[test]
    fn end_of_file_closes_all_blocks() {
        assert_eq!(
            layout_of("f =\n  g =\n    a"),
            "f = <OPEN> g = <OPEN> a <CLOSE> <CLOSE>"
        );
    }

    #[test]
    fn then_and_else_at_the_column_of_if_get_separators() {
        assert_eq!(
            layout_of("f =\n  if c then\n    a\n  else\n    b"),
            "f = <OPEN> if c then <OPEN> a <CLOSE> <SEP> else <OPEN> b <CLOSE> <CLOSE>"
        );
    }

    #[test]
    fn where_and_with_open_blocks() {
        assert_eq!(
            layout_of("effect E where\n  op : A"),
            "effect E where <OPEN> op : A <CLOSE>"
        );
        assert_eq!(
            layout_of("f = match x with\n  | A -> 1\n  | B -> 2"),
            "f = match x with <OPEN> | A -> 1 <SEP> | B -> 2 <CLOSE>"
        );
    }

    #[test]
    fn newlines_inside_brackets_mean_nothing() {
        assert_eq!(layout_of("f = (a\nb)\ng"), "f = ( a b ) <SEP> g");
    }

    #[test]
    fn closing_bracket_closes_blocks_opened_inside() {
        assert_eq!(
            layout_of("f = map (fn x ->\n    x) xs"),
            "f = map ( fn x -> <OPEN> x <CLOSE> ) xs"
        );
    }

    #[test]
    fn semicolon_is_passed_through() {
        assert_eq!(layout_of("a = 1; b = 2"), "a = 1 ; b = 2");
    }

    #[test]
    fn comments_do_not_affect_layout() {
        assert_eq!(
            layout_of("f = -- c\n  a {- x -}\n  b -- d"),
            "f = <OPEN> a <SEP> b <CLOSE>"
        );
    }

    #[test]
    fn lines_inside_a_multi_line_string_are_not_line_starts() {
        assert_eq!(
            layout_of("s =\n  \"\"\"\nx\n  \"\"\"\nt = 1"),
            "s = <OPEN> \"\"\"\nx\n  \"\"\" <CLOSE> <SEP> t = 1"
        );
    }

    #[test]
    fn byte_order_mark_takes_no_column() {
        assert_eq!(layout_of("\u{feff}a = 1\nb = 2"), "a = 1 <SEP> b = 2");
    }

    #[test]
    fn crlf_lines_lay_out_like_lf() {
        assert_eq!(
            layout_of("f =\r\n  a\r\n  b\r\ng"),
            "f = <OPEN> a <SEP> b <CLOSE> <SEP> g"
        );
    }

    #[test]
    fn missing_indented_block_is_an_error_with_an_empty_block() {
        assert_eq!(
            dump("f =\ng"),
            (
                "f = <OPEN> <CLOSE> <SEP> g".to_string(),
                vec!["E0009@2..3".to_string()]
            )
        );
        assert_eq!(
            dump("f ="),
            ("f = <OPEN> <CLOSE>".to_string(), vec!["E0009@2..3".to_string()])
        );
    }

    #[test]
    fn closing_bracket_on_the_next_line_is_not_a_block() {
        assert_eq!(
            dump("f = (fn x ->\n    )"),
            (
                "f = ( fn x -> <OPEN> <CLOSE> )".to_string(),
                vec!["E0009@10..12".to_string()]
            )
        );
    }

    #[test]
    fn block_inside_brackets_must_be_deeper_than_the_enclosing_block() {
        assert_eq!(
            dump("f =\n  g (fn x ->\n  y)"),
            (
                "f = <OPEN> g ( fn x -> <OPEN> <CLOSE> y ) <CLOSE>".to_string(),
                vec!["E0009@14..16".to_string()]
            )
        );
    }

    #[test]
    fn tab_in_indentation_is_an_error() {
        assert_eq!(
            dump("f =\n\ta"),
            ("f = <OPEN> a <CLOSE>".to_string(), vec!["E0006@4..5".to_string()])
        );
    }

    #[test]
    fn tab_after_the_first_token_is_fine() {
        assert_eq!(layout_of("a =\t1"), "a = 1");
    }

    #[test]
    fn unbalanced_closing_bracket_does_not_panic() {
        assert_eq!(layout_of("a = 1)\nb"), "a = 1 ) <SEP> b");
    }

    #[test]
    fn unclosed_bracket_suspends_layout_to_eof() {
        // spec §4 規則 2 のまま。閉じ忘れた括弧の後ろの行は、括弧の中身として続く。
        assert_eq!(layout_of("a = (1\nb = 2"), "a = ( 1 b = 2");
    }

    #[test]
    fn virtual_tokens_are_empty_ranges_at_line_starts_and_eof() {
        let text = "f =\n  a\ng\nh =\n  b";
        let mut files = SourceFiles::new();
        let file = files.add("test.em", text);
        let (tokens, _) = lex(file, text);
        let (out, _) = layout(file, text, &tokens);
        let virtuals: Vec<String> = out
            .iter()
            .filter(|token| matches!(token.kind, LAYOUT_OPEN | LAYOUT_SEP | LAYOUT_CLOSE))
            .map(|token| format!("{:?}@{:?}", token.kind, token.range))
            .collect();
        assert_eq!(
            virtuals,
            [
                "LAYOUT_OPEN@6..6",
                "LAYOUT_CLOSE@8..8",
                "LAYOUT_SEP@8..8",
                "LAYOUT_SEP@10..10",
                "LAYOUT_OPEN@16..16",
                "LAYOUT_CLOSE@17..17",
            ]
        );
    }
}
```

`crates/eml_syntax/src/lib.rs` に `mod layout;` を足す (`mod lexer;` の次)。

- [ ] **Step 2: テストが失敗することを確認する**

Run: `cargo test -p eml_syntax layout`
Expected: コンパイルエラー (`layout` 関数がない)

- [ ] **Step 3: レイアウト段を実装する**

`crates/eml_syntax/src/lib.rs` の `codes` に足す。

```rust
    pub const TAB_INDENTATION: ErrorCode = ErrorCode(6);
    pub const EXPECTED_INDENTED_BLOCK: ErrorCode = ErrorCode(9);
```

`crates/eml_syntax/src/layout.rs` の先頭のコメントの下 (テストの前) に、実装を書く。

```rust
//! レイアウト段 (構文設計 spec §4)。trivia を除いたトークン列に、幅 0 の仮想トークン
//! `LAYOUT_OPEN` (ブロックの開始) / `LAYOUT_SEP` (項目の区切り) / `LAYOUT_CLOSE` (ブロックの終了) を挿入する。
//! 仮想トークンは parser の入力にだけ現れ、木には入らない。

use eml_diagnostics::{Diagnostic, FileId, Label, TextRange, TextSize};

use crate::SyntaxKind::{self, *};
use crate::codes;
use crate::lexer::Token;

/// 文脈のスタックの要素。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Context {
    /// 基準列 n のブロック。
    Block(u32),
    /// 括弧の内側。改行は意味を持たない。
    Bracket,
}

/// 行の最後にあると、ブロックを開くトークン (spec §4 規則 3)。
const BLOCK_STARTERS: [SyntaxKind; 6] = [EQ, THIN_ARROW, WITH_KW, THEN_KW, ELSE_KW, WHERE_KW];

/// trivia でないトークンと、それが行の先頭にあるかどうか、その列 (0 始まり、文字数)。
struct Item {
    token: Token,
    line_start: bool,
    column: u32,
}

pub(crate) fn layout(file: FileId, text: &str, tokens: &[Token]) -> (Vec<Token>, Vec<Diagnostic>) {
    let mut diagnostics = Vec::new();
    let items = scan_lines(file, text, tokens, &mut diagnostics);
    let mut out = Vec::with_capacity(items.len() * 2);
    let mut stack = vec![Context::Block(0)];
    // 今のブロックに、まだトークンが1つもないか。最初の項目の前には SEP を入れない。
    let mut at_block_start = true;
    for (i, item) in items.iter().enumerate() {
        let start = item.token.range.start();
        if item.line_start {
            let mut opened = false;
            if i > 0 && BLOCK_STARTERS.contains(&items[i - 1].token.kind) {
                // 前の行は開始トークンで終わっている (規則 3)。
                if item.column > enclosing_indent(&stack) && !is_closing_bracket(item.token.kind) {
                    out.push(virtual_token(LAYOUT_OPEN, start));
                    stack.push(Context::Block(item.column));
                    opened = true;
                } else {
                    missing_block(file, text, items[i - 1].token, &mut out, &mut diagnostics);
                }
            }
            if !opened {
                // 規則 1。一番上が Bracket なら何もしない (規則 2)。
                while let Some(&Context::Block(n)) = stack.last() {
                    if item.column < n && stack.len() > 1 {
                        out.push(virtual_token(LAYOUT_CLOSE, start));
                        stack.pop();
                    } else {
                        if item.column == n && !at_block_start {
                            out.push(virtual_token(LAYOUT_SEP, start));
                        }
                        break;
                    }
                }
            }
        }
        match item.token.kind {
            L_PAREN | L_BRACK | L_BRACE => {
                out.push(item.token);
                stack.push(Context::Bracket);
            }
            kind if is_closing_bracket(kind) => {
                // 規則 4。対応する Bracket より上のブロックをすべて閉じる。対応する開き括弧がなければ何もしない。
                if stack.contains(&Context::Bracket) {
                    while let Some(Context::Block(_)) = stack.last() {
                        out.push(virtual_token(LAYOUT_CLOSE, start));
                        stack.pop();
                    }
                    stack.pop();
                }
                out.push(item.token);
            }
            _ => out.push(item.token),
        }
        at_block_start = false;
    }
    let eof = TextSize::of(text);
    if let Some(last) = items.last() {
        if BLOCK_STARTERS.contains(&last.token.kind) {
            missing_block(file, text, last.token, &mut out, &mut diagnostics);
        }
    }
    // 規則 6。ファイル全体の Block(0) は閉じない。
    while stack.len() > 1 {
        if let Some(Context::Block(_)) = stack.pop() {
            out.push(virtual_token(LAYOUT_CLOSE, eof));
        }
    }
    (out, diagnostics)
}

/// trivia でないトークンについて、行の先頭かどうかと列を求める。インデントのタブを報告する。
fn scan_lines(
    file: FileId,
    text: &str,
    tokens: &[Token],
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<Item> {
    let mut items = Vec::new();
    // ファイルの先頭も行の先頭とみなす。
    let mut newline_seen = true;
    for token in tokens {
        if token.kind.is_trivia() {
            if text[token.range].contains('\n') {
                newline_seen = true;
            }
            continue;
        }
        let start = usize::from(token.range.start());
        let line_begin = text[..start].rfind('\n').map_or(0, |i| i + 1);
        let prefix = &text[line_begin..start];
        // BOM は列に数えない。タブは1列に数える (タブ自体はエラー)。
        let column = prefix.chars().filter(|&c| c != '\u{feff}').count() as u32;
        if newline_seen {
            report_tab(file, prefix, line_begin, diagnostics);
        }
        items.push(Item {
            token: *token,
            line_start: newline_seen,
            column,
        });
        newline_seen = false;
    }
    items
}

/// 行の先頭の空白 (インデント) にタブがあれば、最初のタブの位置に E0006 を出す。
fn report_tab(file: FileId, prefix: &str, line_begin: usize, diagnostics: &mut Vec<Diagnostic>) {
    let indent_len = prefix
        .find(|c: char| !matches!(c, ' ' | '\t' | '\u{feff}'))
        .unwrap_or(prefix.len());
    if let Some(offset) = prefix[..indent_len].find('\t') {
        let at = TextSize::new((line_begin + offset) as u32);
        diagnostics.push(Diagnostic::error(
            codes::TAB_INDENTATION,
            "tab used for indentation",
            Label::new(file, TextRange::at(at, TextSize::new(1)), "indent with spaces"),
        ));
    }
}

/// 規則 3 の「字下げしたブロックが必要」。E0009 を出し、開始トークンの直後に空のブロックを入れる。
/// parser は空のブロックを黙って受け入れるので、同じ問題を二重に報告しない。
fn missing_block(
    file: FileId,
    text: &str,
    starter: Token,
    out: &mut Vec<Token>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    diagnostics.push(Diagnostic::error(
        codes::EXPECTED_INDENTED_BLOCK,
        format!("expected an indented block after `{}`", &text[starter.range]),
        Label::new(
            file,
            starter.range,
            "the next line must be indented more than the enclosing block",
        ),
    ));
    let end = starter.range.end();
    out.push(virtual_token(LAYOUT_OPEN, end));
    out.push(virtual_token(LAYOUT_CLOSE, end));
}

/// 括弧の内側にいても、一番近いブロックの基準列を返す。
fn enclosing_indent(stack: &[Context]) -> u32 {
    stack
        .iter()
        .rev()
        .find_map(|context| match context {
            Context::Block(n) => Some(*n),
            Context::Bracket => None,
        })
        .unwrap_or(0)
}

fn is_closing_bracket(kind: SyntaxKind) -> bool {
    matches!(kind, R_PAREN | R_BRACK | R_BRACE)
}

fn virtual_token(kind: SyntaxKind, at: TextSize) -> Token {
    Token {
        kind,
        range: TextRange::empty(at),
    }
}
```

この時点では `layout` を呼ぶのはテストだけなので、`dead_code` の警告が出る場合は `mod layout;` に `#[allow(dead_code)] // Task 3 で parse から使う` を付ける (Task 3 で外す)。

- [ ] **Step 4: テストが通ることを確認する**

Run: `cargo test -p eml_syntax layout`
Expected: PASS (すべて)

- [ ] **Step 5: 全体のテストと lint を通す**

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check`
Expected: すべて PASS

- [ ] **Step 6: Commit**

```bash
git add crates/eml_syntax/src/layout.rs crates/eml_syntax/src/lib.rs
git commit -m "Add the layout stage that inserts virtual block tokens"
```

### Task 3: parser の仕組みの拡張と、レイアウト段の接続

**Files:**
- Modify: `crates/eml_syntax/src/parser.rs` (`Parser` の構造と補助関数、`Marker` の drop bomb、単体テスト)
- Modify: `crates/eml_syntax/src/sink.rs` (`Event::TokenPrefix`、`precede` 後の `abandon` に耐える)
- Modify: `crates/eml_syntax/src/syntax_kind.rs` (`is_virtual`)
- Modify: `crates/eml_syntax/src/lib.rs` (`parse` にレイアウト段をつなぐ)
- Modify: `crates/eml_syntax/src/grammar/mod.rs` (暫定の文法が仮想トークンの位置に E0003 を出さないようにする1行だけ。Task 4 で全体を置き換える)

**Interfaces:**
- Consumes: `layout::layout` (Task 2)、`lexer::operator_kind` (Task 1)
- Produces (`crate::parser`):
  - `Parser::new(file: FileId, text: &'t str, tokens: Vec<Token>) -> Parser<'t>` (`tokens` はレイアウト段の出力。trivia を含まない)
  - `nth(&self, n) -> SyntaxKind`、`current()`、`at(kind)`、`at_ts(TokenSet)`、`at_eof()`、`at_sep()` (`LAYOUT_SEP` か `SEMICOLON`)
  - `current_text(&self) -> &'t str` (仮想トークンと EOF では `""`)、`current_range()`
  - `touches_prev(&self) -> bool` (直前のトークンの終わりと今のトークンの始まりが同じ位置)、`touches_next(&self) -> bool`
  - `bump_any()` (仮想トークンならイベントを出さずに進む)、`bump(kind)`、`eat(kind) -> bool`、`bump_remap(kind)` (今のトークンを別の種類として木に入れる)、`split_first_char(kind)` (2文字以上の演算子の先頭の1文字を `kind` として読み、残りを今のトークンにする)
  - `error(code, message, label)`: 今のトークンの位置に診断を出す。今のトークンが `ERROR_TOKEN` なら出さない。直前の parser の診断と同じ開始位置なら出さない
  - `start() -> Marker`、`Marker::complete(self, p, kind) -> CompletedMarker`、`Marker::abandon(self, p)`、`CompletedMarker::precede(self, p) -> Marker`。`Marker` は完了も放棄もせずに捨てるとパニックする
  - 前進せずに `nth` を `STEP_LIMIT` (1_000_000) 回呼ぶとパニックする (`the parser seems stuck`)
- Produces: `SyntaxKind::is_virtual(self) -> bool`

M1 スケルトンで保留した `precede` / `abandon` の安全性の問題をここで直す: `abandon` はイベントを取り除かず `Tombstone` のまま残す (後に積んだノードを誤って指さないため)。sink は `forward_parent` が `Tombstone` を指していたら、そこで親の連鎖を止める。

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_syntax/src/parser.rs` の `mod tests` の `run` を次のものに置き換える (レイアウト段を通すため。テストの意味は変えない)。

```rust
    /// テキストを字句解析とレイアウト段に通し、`grammar` でパースして木の表示を返す。
    /// 仕組みだけを試すための小さな文法を渡す。
    fn run(text: &str, grammar: impl FnOnce(&mut Parser)) -> (String, Vec<Diagnostic>) {
        let mut files = SourceFiles::new();
        let file = files.add("test.em", text);
        let (tokens, _) = lex(file, text);
        let (input, _) = layout(file, text, &tokens);
        let mut p = Parser::new(file, text, input);
        grammar(&mut p);
        let (events, diagnostics) = p.finish();
        let tree = SyntaxNode::new_root(build_tree(text, &tokens, events));
        assert_eq!(tree.text().to_string(), text, "tree must be lossless");
        (debug_tree(&tree), diagnostics)
    }
```

`use crate::lexer::lex;` の次に `use crate::layout::layout;` を足す。既存の5件のテストは変えない。次のテストを `mod tests` に足す。

```rust
    #[test]
    fn virtual_tokens_make_no_tree_tokens() {
        let (tree, _) = run("f =\n  a", |p| {
            let root = p.start();
            let mut virtuals = 0;
            while !p.at_eof() {
                if p.current().is_virtual() {
                    virtuals += 1;
                }
                p.bump_any();
            }
            assert_eq!(virtuals, 2);
            root.complete(p, SOURCE_FILE);
        });
        insta::assert_snapshot!(tree, @r#"
        SOURCE_FILE@0..7
          LIDENT@0..1 "f"
          WHITESPACE@1..2 " "
          EQ@2..3 "="
          WHITESPACE@3..6 "\n  "
          LIDENT@6..7 "a"
        "#);
    }

    #[test]
    fn semicolon_is_a_separator() {
        run("a; b", |p| {
            let root = p.start();
            p.bump(LIDENT);
            assert!(p.at_sep());
            p.bump_any();
            p.bump(LIDENT);
            root.complete(p, SOURCE_FILE);
        });
    }

    #[test]
    fn split_first_char_divides_an_operator_token() {
        let (tree, _) = run("<>->", |p| {
            let root = p.start();
            p.split_first_char(L_ANGLE);
            assert_eq!(p.current(), OP);
            assert_eq!(p.current_text(), ">->");
            p.split_first_char(R_ANGLE);
            assert_eq!(p.current(), THIN_ARROW);
            p.bump(THIN_ARROW);
            root.complete(p, SOURCE_FILE);
        });
        insta::assert_snapshot!(tree, @r#"
        SOURCE_FILE@0..4
          L_ANGLE@0..1 "<"
          R_ANGLE@1..2 ">"
          THIN_ARROW@2..4 "->"
        "#);
    }

    #[test]
    fn bump_remap_changes_the_tree_kind() {
        let (tree, _) = run("<", |p| {
            let root = p.start();
            p.bump_remap(L_ANGLE);
            root.complete(p, SOURCE_FILE);
        });
        insta::assert_snapshot!(tree, @r#"
        SOURCE_FILE@0..1
          L_ANGLE@0..1 "<"
        "#);
    }

    #[test]
    fn touching_tokens() {
        run("a.b c", |p| {
            let root = p.start();
            assert!(!p.touches_prev());
            assert!(p.touches_next());
            p.bump(LIDENT);
            assert!(p.touches_prev());
            assert!(p.touches_next());
            p.bump(DOT);
            assert!(!p.touches_next());
            p.bump(LIDENT);
            p.bump(LIDENT);
            root.complete(p, SOURCE_FILE);
        });
    }

    #[test]
    fn precede_can_be_chained() {
        let (tree, _) = run("a b c", |p| {
            let root = p.start();
            let first = p.start();
            p.bump_any();
            let first = first.complete(p, ERROR);
            let second = first.precede(p);
            p.bump_any();
            let second = second.complete(p, ERROR);
            let third = second.precede(p);
            p.bump_any();
            third.complete(p, ERROR);
            root.complete(p, SOURCE_FILE);
        });
        insta::assert_snapshot!(tree, @r#"
        SOURCE_FILE@0..5
          ERROR@0..5
            ERROR@0..3
              ERROR@0..1
                LIDENT@0..1 "a"
              WHITESPACE@1..2 " "
              LIDENT@2..3 "b"
            WHITESPACE@3..4 " "
            LIDENT@4..5 "c"
        "#);
    }

    #[test]
    fn abandoning_a_preceded_marker_keeps_the_child() {
        let (tree, _) = run("a b", |p| {
            let root = p.start();
            let first = p.start();
            p.bump_any();
            let first = first.complete(p, ERROR);
            let outer = first.precede(p);
            outer.abandon(p);
            let next = p.start();
            p.bump_any();
            next.complete(p, ERROR);
            root.complete(p, SOURCE_FILE);
        });
        insta::assert_snapshot!(tree, @r#"
        SOURCE_FILE@0..3
          ERROR@0..1
            LIDENT@0..1 "a"
          WHITESPACE@1..2 " "
          ERROR@2..3
            LIDENT@2..3 "b"
        "#);
    }

    #[test]
    fn abandoning_a_marker_that_is_not_the_last_event() {
        let (tree, _) = run("a b", |p| {
            let root = p.start();
            let outer = p.start();
            let inner = p.start();
            p.bump_any();
            inner.complete(p, ERROR);
            outer.abandon(p);
            p.bump_any();
            root.complete(p, SOURCE_FILE);
        });
        insta::assert_snapshot!(tree, @r#"
        SOURCE_FILE@0..3
          ERROR@0..1
            LIDENT@0..1 "a"
          WHITESPACE@1..2 " "
          LIDENT@2..3 "b"
        "#);
    }

    #[test]
    #[should_panic(expected = "marker must be completed or abandoned")]
    fn forgotten_marker_panics() {
        run("a", |p| {
            let _forgotten = p.start();
        });
    }

    #[test]
    #[should_panic(expected = "the parser seems stuck")]
    fn parser_without_progress_panics() {
        run("a", |p| {
            loop {
                p.current();
            }
        });
    }

    #[test]
    fn errors_at_the_same_position_are_reported_once() {
        let (_, diagnostics) = run("a", |p| {
            let root = p.start();
            p.error(ErrorCode(9999), "first", "here");
            p.error(ErrorCode(9998), "second", "here");
            p.bump_any();
            root.complete(p, SOURCE_FILE);
        });
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].message, "first");
    }

    #[test]
    fn no_error_is_reported_at_an_error_token() {
        let (_, diagnostics) = run("€", |p| {
            let root = p.start();
            p.error(ErrorCode(9999), "expected something", "here");
            p.bump_any();
            root.complete(p, SOURCE_FILE);
        });
        assert!(diagnostics.is_empty());
    }
```

- [ ] **Step 2: テストが失敗することを確認する**

Run: `cargo test -p eml_syntax --lib parser`
Expected: コンパイルエラー (`Parser::new` の引数、`split_first_char` などがない)

- [ ] **Step 3: `SyntaxKind::is_virtual` を足す**

`crates/eml_syntax/src/syntax_kind.rs` の `impl SyntaxKind` に足す。

```rust
    /// レイアウト段の仮想トークンか。parser は読んでもイベントを出さない。
    pub fn is_virtual(self) -> bool {
        matches!(
            self,
            SyntaxKind::LAYOUT_OPEN | SyntaxKind::LAYOUT_SEP | SyntaxKind::LAYOUT_CLOSE
        )
    }
```

- [ ] **Step 4: `Parser` を書き換える**

`crates/eml_syntax/src/parser.rs` の、`mod tests` より前の部分を次のものに置き換える。

```rust
//! rust-analyzer と同じイベント方式のパーサ。文法の規則は `grammar` に置き、ここは仕組みだけを持つ。
//! 入力はレイアウト段の出力で、仮想トークン (`LAYOUT_*`) を含み、trivia を含まない。

use std::cell::Cell;

use eml_diagnostics::{Diagnostic, ErrorCode, FileId, Label, TextRange, TextSize};

use crate::SyntaxKind;
use crate::lexer::{Token, operator_kind};
use crate::token_set::TokenSet;

/// 前進せずに先読みできる回数の上限。文法の誤りによる無限ループを検出する。
const STEP_LIMIT: u32 = 1_000_000;

/// パーサが出すイベント。`sink::build_tree` がこれを rowan の木に組み立てる。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Event {
    /// ノードの開始。`forward_parent` は、`precede` で後から作った親ノードの `Start` までの距離。
    Start {
        kind: SyntaxKind,
        forward_parent: Option<u32>,
    },
    /// trivia でないトークン (の残り全部) を1つ進める。
    Token { kind: SyntaxKind },
    /// trivia でないトークンの先頭の `len` バイトだけを、`kind` のトークンとして進める。
    TokenPrefix { kind: SyntaxKind, len: TextSize },
    Finish,
    /// まだ種類の決まっていない `Start`、または取り消したノード。
    Tombstone,
}

pub(crate) struct Parser<'t> {
    file: FileId,
    text: &'t str,
    /// レイアウト段の出力。
    tokens: Vec<Token>,
    pos: usize,
    events: Vec<Event>,
    diagnostics: Vec<Diagnostic>,
    steps: Cell<u32>,
}

impl<'t> Parser<'t> {
    pub(crate) fn new(file: FileId, text: &'t str, tokens: Vec<Token>) -> Parser<'t> {
        Parser {
            file,
            text,
            tokens,
            pos: 0,
            events: Vec::new(),
            diagnostics: Vec::new(),
            steps: Cell::new(0),
        }
    }

    pub(crate) fn finish(self) -> (Vec<Event>, Vec<Diagnostic>) {
        (self.events, self.diagnostics)
    }

    /// `n` 個先のトークンの種類。入力の終わりを越えたら `EOF`。
    pub(crate) fn nth(&self, n: usize) -> SyntaxKind {
        let steps = self.steps.get();
        assert!(steps < STEP_LIMIT, "the parser seems stuck at token {}", self.pos);
        self.steps.set(steps + 1);
        self.tokens
            .get(self.pos + n)
            .map_or(SyntaxKind::EOF, |token| token.kind)
    }

    pub(crate) fn current(&self) -> SyntaxKind {
        self.nth(0)
    }

    pub(crate) fn at(&self, kind: SyntaxKind) -> bool {
        self.current() == kind
    }

    pub(crate) fn at_ts(&self, set: TokenSet) -> bool {
        set.contains(self.current())
    }

    pub(crate) fn at_eof(&self) -> bool {
        self.at(SyntaxKind::EOF)
    }

    /// 項目の区切り。`;` はレイアウトの `SEP` と同じに扱う (spec §4 規則 5)。
    pub(crate) fn at_sep(&self) -> bool {
        matches!(
            self.current(),
            SyntaxKind::LAYOUT_SEP | SyntaxKind::SEMICOLON
        )
    }

    /// 今のトークンのテキスト。仮想トークンと入力の終わりでは空。
    pub(crate) fn current_text(&self) -> &'t str {
        let text = self.text;
        self.tokens
            .get(self.pos)
            .map_or("", |token| &text[token.range])
    }

    /// 今のトークンの位置。入力の終わりでは、テキストの末尾の空の範囲。
    pub(crate) fn current_range(&self) -> TextRange {
        self.tokens
            .get(self.pos)
            .map_or(TextRange::empty(TextSize::of(self.text)), |token| token.range)
    }

    /// 直前のトークンと今のトークンの間に、空白やコメントがないか。
    pub(crate) fn touches_prev(&self) -> bool {
        match (self.pos.checked_sub(1), self.tokens.get(self.pos)) {
            (Some(prev), Some(current)) => {
                self.tokens[prev].range.end() == current.range.start()
            }
            _ => false,
        }
    }

    /// 今のトークンと次のトークンの間に、空白やコメントがないか。
    pub(crate) fn touches_next(&self) -> bool {
        match (self.tokens.get(self.pos), self.tokens.get(self.pos + 1)) {
            (Some(current), Some(next)) => current.range.end() == next.range.start(),
            _ => false,
        }
    }

    pub(crate) fn bump_any(&mut self) {
        let kind = self.current();
        assert_ne!(kind, SyntaxKind::EOF, "cannot bump past the end of input");
        if !kind.is_virtual() {
            self.events.push(Event::Token { kind });
        }
        self.advance();
    }

    pub(crate) fn bump(&mut self, kind: SyntaxKind) {
        assert!(
            self.at(kind),
            "expected {kind:?}, found {:?}",
            self.current()
        );
        self.bump_any();
    }

    pub(crate) fn eat(&mut self, kind: SyntaxKind) -> bool {
        if self.at(kind) {
            self.bump_any();
            true
        } else {
            false
        }
    }

    /// 今のトークンを `kind` として木に入れる (型の中の `<` を `L_ANGLE` にする、など)。
    pub(crate) fn bump_remap(&mut self, kind: SyntaxKind) {
        assert!(!self.at_eof() && !self.current().is_virtual());
        self.events.push(Event::Token { kind });
        self.advance();
    }

    /// 今のトークン (2文字以上の演算子) の先頭の1文字を `kind` として読み、残りを今のトークンにする。
    /// 型の中で `<>` や `>->` を分けて読むのに使う (spec §5)。
    pub(crate) fn split_first_char(&mut self, kind: SyntaxKind) {
        let token = self.tokens[self.pos];
        let one = TextSize::new(1);
        assert!(token.range.len() > one, "cannot split a one-character token");
        self.events.push(Event::TokenPrefix { kind, len: one });
        let rest = TextRange::new(token.range.start() + one, token.range.end());
        self.tokens[self.pos] = Token {
            kind: operator_kind(&self.text[rest]),
            range: rest,
        };
        self.steps.set(0);
    }

    fn advance(&mut self) {
        self.pos += 1;
        self.steps.set(0);
    }

    pub(crate) fn start(&mut self) -> Marker {
        let pos = self.events.len() as u32;
        self.events.push(Event::Tombstone);
        Marker::new(pos)
    }

    /// 今のトークンの位置に診断を出す。トークンは進めない。`label` はその位置に付ける説明。
    /// `ERROR_TOKEN` は字句解析で報告済みなので、その位置には出さない。
    /// 直前の診断と同じ位置にも出さない (1つの誤りから連鎖する診断を抑える)。
    pub(crate) fn error(
        &mut self,
        code: ErrorCode,
        message: impl Into<String>,
        label: impl Into<String>,
    ) {
        if self.at(SyntaxKind::ERROR_TOKEN) {
            return;
        }
        let range = self.current_range();
        if self
            .diagnostics
            .last()
            .is_some_and(|last| last.primary.range.start() == range.start())
        {
            return;
        }
        self.diagnostics.push(Diagnostic::error(
            code,
            message,
            Label::new(self.file, range, label),
        ));
    }
}

/// 開始したノード。`complete` か `abandon` で必ず閉じる。閉じずに捨てるとパニックする。
#[must_use]
pub(crate) struct Marker {
    pos: u32,
    done: bool,
}

impl Marker {
    fn new(pos: u32) -> Marker {
        Marker { pos, done: false }
    }

    pub(crate) fn complete(mut self, p: &mut Parser, kind: SyntaxKind) -> CompletedMarker {
        self.done = true;
        match &mut p.events[self.pos as usize] {
            slot @ Event::Tombstone => {
                *slot = Event::Start {
                    kind,
                    forward_parent: None,
                }
            }
            _ => unreachable!("marker must point at a Tombstone event"),
        }
        p.events.push(Event::Finish);
        CompletedMarker { pos: self.pos }
    }

    /// ノードを作らない。イベントは `Tombstone` のまま残す (取り除くと、`precede` で指された位置に
    /// 後のノードが入り、親子関係が壊れるため)。
    pub(crate) fn abandon(mut self, _p: &mut Parser) {
        self.done = true;
    }
}

impl Drop for Marker {
    fn drop(&mut self) {
        if !self.done && !std::thread::panicking() {
            panic!("marker must be completed or abandoned");
        }
    }
}

pub(crate) struct CompletedMarker {
    pos: u32,
}

impl CompletedMarker {
    /// 完了したノードの外側に、新しい親ノードを開始する (演算子の列やフィールドアクセスの左辺などに使う)。
    pub(crate) fn precede(self, p: &mut Parser) -> Marker {
        let parent = p.start();
        match &mut p.events[self.pos as usize] {
            Event::Start { forward_parent, .. } => *forward_parent = Some(parent.pos - self.pos),
            _ => unreachable!("completed marker must point at a Start event"),
        }
        parent
    }
}
```

既存の単体テストの `Parser::new(file, &tokens, TextSize::of(text))` は Step 1 の `run` に置き換わっている。`#[allow(dead_code)]` の付いていた `bump` / `eat` / `abandon` / `precede` の属性とコメント (`// 後の段階の文法で使う。`) は消す。

- [ ] **Step 5: sink を直す**

`crates/eml_syntax/src/sink.rs` を次のように変える。

1. `use` に `use eml_diagnostics::{TextRange, TextSize};` を足す。
2. `forward_parent` の連鎖をたどる `match` の `_ => unreachable!(...)` を、次のものに置き換える。

```rust
                        // 放棄した親ノード。連鎖はここで終わる。
                        Event::Tombstone => None,
                        _ => unreachable!("forward_parent must point at a Start or Tombstone event"),
```

3. `Event::Token { kind } => { ... }` の次に足す。

```rust
            Event::TokenPrefix { kind, len } => {
                builder.eat_trivia();
                builder.token_prefix(kind, len);
            }
```

4. `Builder` に `offset: TextSize` (今のトークンのうち、すでに木に入れたバイト数) を足し、`build_tree` の初期化で `offset: TextSize::new(0)` にする。`token` を置き換え、`token_prefix` を足す。

```rust
    /// 今のトークンの残り全部を、`kind` として木に入れる。
    fn token(&mut self, kind: SyntaxKind) {
        let range = self.tokens[self.next].range;
        let start = range.start() + self.offset;
        self.inner.token(
            EmlLanguage::kind_to_raw(kind),
            &self.text[TextRange::new(start, range.end())],
        );
        self.next += 1;
        self.offset = TextSize::new(0);
    }

    /// 今のトークンの先頭の `len` バイトだけを、`kind` として木に入れる。
    fn token_prefix(&mut self, kind: SyntaxKind, len: TextSize) {
        let range = self.tokens[self.next].range;
        let start = range.start() + self.offset;
        self.inner.token(
            EmlLanguage::kind_to_raw(kind),
            &self.text[TextRange::at(start, len)],
        );
        self.offset += len;
    }
```

`eat_trivia` は変えない (トークンの途中では、今のトークンが trivia でないので、何も読まずに止まる)。

- [ ] **Step 6: `parse` にレイアウト段をつなぐ**

`crates/eml_syntax/src/lib.rs` の `parse` を次のものに置き換え、`mod layout;` に付けた `#[allow(dead_code)]` があれば外す。`use eml_diagnostics::{Diagnostic, FileId, TextSize};` の `TextSize` が使われなくなったら消す。

```rust
pub fn parse(file: FileId, text: &str) -> (Parse, Vec<Diagnostic>) {
    let (tokens, mut diagnostics) = lex(file, text);
    let (input, layout_diagnostics) = layout::layout(file, text, &tokens);
    diagnostics.extend(layout_diagnostics);
    let mut parser = parser::Parser::new(file, text, input);
    grammar::source_file(&mut parser);
    let (events, parse_diagnostics) = parser.finish();
    diagnostics.extend(parse_diagnostics);
    diagnostics.sort_by_key(|diagnostic| diagnostic.primary.range.start());
    let green = sink::build_tree(text, &tokens, events);
    (Parse { green }, diagnostics)
}
```

`crates/eml_syntax/src/grammar/mod.rs` (暫定の文法。Task 4 で置き換える) の `stray_tokens` の条件 `if !reported && !p.at(ERROR_TOKEN) {` を、`if !reported && !p.at(ERROR_TOKEN) && !p.current().is_virtual() {` にする。仮想トークンの位置 (次の行の先頭) に E0003 を出すと、`multiple_errors.em` の診断の範囲が変わるため。

- [ ] **Step 7: テストが通ることを確認する**

Run: `cargo test -p eml_syntax`
Expected: PASS (すべて)

- [ ] **Step 8: 全体のテストと lint を通す**

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check`
Expected: すべて PASS。UI テストのスナップショットは変わらない

- [ ] **Step 9: Commit**

```bash
git add crates/eml_syntax
git commit -m "Feed the layout stage into the parser and harden markers"
```

### Task 4: 文法の骨組み、型、宣言

**Files:**
- Modify: `crates/eml_syntax/src/syntax_kind.rs` (ノードの種類を足す)
- Modify: `crates/eml_syntax/src/lib.rs` (`codes` に E0010 / E0011 を足す)
- Modify: `crates/eml_syntax/src/grammar/mod.rs` (全体を置き換える)
- Create: `crates/eml_syntax/src/grammar/items.rs`
- Create: `crates/eml_syntax/src/grammar/types.rs`
- Create: `crates/eml_syntax/tests/common/mod.rs` (文法のテストの補助)
- Create: `crates/eml_syntax/tests/declarations.rs`
- Modify: `crates/eml_syntax/tests/parser.rs` (暫定構文の2件を書き換える)
- Modify: `crates/eml_cli/tests/snapshots/ui__check_fail@stray_tokens.em.snap`、`crates/eml_cli/tests/snapshots/ui__check_fail@multiple_errors.em.snap` (E0003 のメッセージ)

**Interfaces:**
- Consumes: Task 3 の `Parser` の API
- Produces:
  - ノードの種類 (下の Step 3 の一覧。式・パターンのノードもここで定義し、Task 5〜7 が使う)
  - `codes::SPACE_AROUND_DOT` (E0010)、`codes::SYNTAX_ERROR` (E0011)
  - `grammar` の共通の補助 (`grammar/mod.rs`、`pub(super)` 相当で子モジュールから使う): `block_of(p, expected: &str, item: impl FnMut(&mut Parser) -> bool)`、`close_block(p)`、`skip_to_sep(p, in_block: bool)`、`expect(p, kind) -> bool`、`describe(p) -> String`、`not_yet_supported(p, message: &str)`、`unsupported_group(p, message: &str)` (ノードは作らない)、`qcon(p)`、`dot(p)`
  - `grammar::types`: `type_(p) -> bool`、`btype(p) -> bool`、`type_atom(p) -> bool`、`at_type_atom_start(p) -> bool`、`type_or_block(p)`
  - `grammar::items`: `at_item_start(p) -> bool`、`item(p)` (Task 5 で等式を足す)
  - テストの補助 `tests/common/mod.rs`: `shape(text) -> String`、`diagnostics(text) -> Vec<String>` (`E0011 1:5 message` の形)、`lines(&[&str]) -> String`

この段階では、等式 (`f x = ...`) はまだ項目として認識しない (Task 5)。`foo` のような小文字の名前だけの行は E0003 のまま。

暫定構文で書いた既存のテストは、次のように書き換える (言語設計 spec §8 の合意済みの例外)。

| テスト | 書き換え | 理由 |
|---|---|---|
| `tests/parser.rs` の `stray_tokens_are_one_error_until_the_next_item` | E0003 のメッセージを `expected an item` にする | 項目のキーワードが `fn` / `type` / `effect` ではなくなった |
| `tests/parser.rs` の `recovery_resumes_at_item_keywords` | `recovery_resumes_at_the_next_item` に置き換える。暫定構文の項目 (`fn main () ...`) を、本番の構文のシグネチャにする。「1つの項目の中のエラーが、次の項目の解析に影響しない」ことを確かめる意図は同じ | 暫定構文の項目がなくなった。項目ごとの E0004 も消える |
| UI テストの `stray_tokens.em`、`multiple_errors.em` のスナップショット | E0003 のメッセージの行だけを変える | 同上 |

- [ ] **Step 1: テストの補助を作る**

`crates/eml_syntax/tests/common/mod.rs`:

```rust
//! 文法のテストの補助。範囲と trivia を省いた木の形と、診断の一覧を表示する。
#![allow(dead_code)]

use eml_diagnostics::SourceFiles;
use eml_syntax::{SyntaxElement, SyntaxNode, parse};

/// 木の形 (ノードと、trivia でないトークン) を表示し、`---` の後に診断を並べる。
/// 木が元のテキストに戻ることも確認する。
pub fn shape(text: &str) -> String {
    let (root, diagnostics) = parse_text(text);
    let mut out = String::new();
    write_node(&mut out, &root, 0);
    if !diagnostics.is_empty() {
        out.push_str("---\n");
        for line in diagnostics {
            out.push_str(&line);
            out.push('\n');
        }
    }
    out
}

/// 診断を `E0011 1:5 message` の形で並べる。行と列は 1 始まりで、列は文字数で数える。
pub fn diagnostics(text: &str) -> Vec<String> {
    parse_text(text).1
}

/// テキストの行を改行でつなぐ。
pub fn lines(lines: &[&str]) -> String {
    lines.join("\n")
}

fn parse_text(text: &str) -> (SyntaxNode, Vec<String>) {
    let mut files = SourceFiles::new();
    let file = files.add("test.em", text);
    let (parse, diagnostics) = parse(file, text);
    let root = parse.syntax();
    assert_eq!(root.text().to_string(), text, "tree must be lossless");
    let shown = diagnostics
        .iter()
        .map(|d| {
            let offset = u32::from(d.primary.range.start()) as usize;
            let before = &text[..offset];
            let line = before.matches('\n').count() + 1;
            let column = before.rsplit('\n').next().unwrap_or("").chars().count() + 1;
            format!("{} {line}:{column} {}", d.code, d.message)
        })
        .collect();
    (root, shown)
}

fn write_node(out: &mut String, node: &SyntaxNode, depth: usize) {
    out.push_str(&format!("{}{:?}\n", "  ".repeat(depth), node.kind()));
    for child in node.children_with_tokens() {
        match child {
            SyntaxElement::Node(child) => write_node(out, &child, depth + 1),
            SyntaxElement::Token(token) if !token.kind().is_trivia() => {
                out.push_str(&format!(
                    "{}{:?} {:?}\n",
                    "  ".repeat(depth + 1),
                    token.kind(),
                    token.text()
                ));
            }
            SyntaxElement::Token(_) => {}
        }
    }
}
```

- [ ] **Step 2: 失敗するテストを書く**

`crates/eml_syntax/tests/declarations.rs`:

```rust
mod common;

use common::{diagnostics, lines, shape};

#[test]
fn signature_with_function_type() {
    insta::assert_snapshot!(shape("len : List a -> Int"), @r#"
    SOURCE_FILE
      SIGNATURE
        LIDENT "len"
        COLON ":"
        FN_TYPE
          APP_TYPE
            UIDENT "List"
            VAR_TYPE
              LIDENT "a"
          THIN_ARROW "->"
          PATH_TYPE
            UIDENT "Int"
    "#);
}

#[test]
fn qualified_type_names() {
    insta::assert_snapshot!(shape("x : Option.Option Int"), @r#"
    SOURCE_FILE
      SIGNATURE
        LIDENT "x"
        COLON ":"
        APP_TYPE
          UIDENT "Option"
          DOT "."
          UIDENT "Option"
          PATH_TYPE
            UIDENT "Int"
    "#);
}

#[test]
fn effect_row_with_effects_and_tail() {
    insta::assert_snapshot!(shape("f : a -> <IO, State s | e> b"), @r#"
    SOURCE_FILE
      SIGNATURE
        LIDENT "f"
        COLON ":"
        FN_TYPE
          VAR_TYPE
            LIDENT "a"
          THIN_ARROW "->"
          EFFECT_ROW
            L_ANGLE "<"
            EFFECT
              UIDENT "IO"
            COMMA ","
            EFFECT
              UIDENT "State"
              VAR_TYPE
                LIDENT "s"
            PIPE "|"
            LIDENT "e"
            R_ANGLE ">"
          VAR_TYPE
            LIDENT "b"
    "#);
}

#[test]
fn empty_row_is_split_from_one_operator_token() {
    insta::assert_snapshot!(shape("g : Unit -> <> Unit"), @r#"
    SOURCE_FILE
      SIGNATURE
        LIDENT "g"
        COLON ":"
        FN_TYPE
          PATH_TYPE
            UIDENT "Unit"
          THIN_ARROW "->"
          EFFECT_ROW
            L_ANGLE "<"
            R_ANGLE ">"
          PATH_TYPE
            UIDENT "Unit"
    "#);
}

#[test]
fn row_variable_alone() {
    insta::assert_snapshot!(shape("h : Unit -> <e> Unit"), @r#"
    SOURCE_FILE
      SIGNATURE
        LIDENT "h"
        COLON ":"
        FN_TYPE
          PATH_TYPE
            UIDENT "Unit"
          THIN_ARROW "->"
          EFFECT_ROW
            L_ANGLE "<"
            LIDENT "e"
            R_ANGLE ">"
          PATH_TYPE
            UIDENT "Unit"
    "#);
}

#[test]
fn operator_signature() {
    insta::assert_snapshot!(shape("(</>) : Path -> String -> Path"), @r#"
    SOURCE_FILE
      SIGNATURE
        L_PAREN "("
        OP "</>"
        R_PAREN ")"
        COLON ":"
        FN_TYPE
          PATH_TYPE
            UIDENT "Path"
          THIN_ARROW "->"
          FN_TYPE
            PATH_TYPE
              UIDENT "String"
            THIN_ARROW "->"
            PATH_TYPE
              UIDENT "Path"
    "#);
}

#[test]
fn arrows_at_the_end_of_lines_continue_the_type() {
    insta::assert_snapshot!(shape(&lines(&["f : Int ->", "  Int ->", "    Int"])), @r#"
    SOURCE_FILE
      SIGNATURE
        LIDENT "f"
        COLON ":"
        FN_TYPE
          PATH_TYPE
            UIDENT "Int"
          THIN_ARROW "->"
          FN_TYPE
            PATH_TYPE
              UIDENT "Int"
            THIN_ARROW "->"
            PATH_TYPE
              UIDENT "Int"
    "#);
}

#[test]
fn tuple_and_parenthesized_types() {
    insta::assert_snapshot!(shape("p : (Int, (String -> Int))"), @r#"
    SOURCE_FILE
      SIGNATURE
        LIDENT "p"
        COLON ":"
        TUPLE_TYPE
          L_PAREN "("
          PATH_TYPE
            UIDENT "Int"
          COMMA ","
          PAREN_TYPE
            L_PAREN "("
            FN_TYPE
              PATH_TYPE
                UIDENT "String"
              THIN_ARROW "->"
              PATH_TYPE
                UIDENT "Int"
            R_PAREN ")"
          R_PAREN ")"
    "#);
}

#[test]
fn data_declarations() {
    let text = lines(&[
        "data Option a =",
        "  | None",
        "  | Some a",
        "data Color = | Red | Green",
        "data List a =",
        "  | Nil",
        "  | a :: List a",
    ]);
    insta::assert_snapshot!(shape(&text), @r#"
    SOURCE_FILE
      DATA_ITEM
        DATA_KW "data"
        UIDENT "Option"
        LIDENT "a"
        EQ "="
        ALT
          PIPE "|"
          UIDENT "None"
        ALT
          PIPE "|"
          UIDENT "Some"
          VAR_TYPE
            LIDENT "a"
      DATA_ITEM
        DATA_KW "data"
        UIDENT "Color"
        EQ "="
        ALT
          PIPE "|"
          UIDENT "Red"
        ALT
          PIPE "|"
          UIDENT "Green"
      DATA_ITEM
        DATA_KW "data"
        UIDENT "List"
        LIDENT "a"
        EQ "="
        ALT
          PIPE "|"
          UIDENT "Nil"
        ALT
          PIPE "|"
          VAR_TYPE
            LIDENT "a"
          CONOP "::"
          APP_TYPE
            UIDENT "List"
            VAR_TYPE
              LIDENT "a"
    "#);
}

#[test]
fn constructor_without_leading_pipe_is_an_error() {
    insta::assert_snapshot!(shape("data T = A | B"), @r#"
    SOURCE_FILE
      DATA_ITEM
        DATA_KW "data"
        UIDENT "T"
        EQ "="
        ALT
          UIDENT "A"
        ALT
          PIPE "|"
          UIDENT "B"
    ---
    E0011 1:10 expected `|` before the constructor
    "#);
}

#[test]
fn effect_declaration() {
    let text = lines(&[
        "effect State s where",
        "  get : Unit -> s",
        "  never fail : String -> a",
    ]);
    insta::assert_snapshot!(shape(&text), @r#"
    SOURCE_FILE
      EFFECT_ITEM
        EFFECT_KW "effect"
        UIDENT "State"
        LIDENT "s"
        WHERE_KW "where"
        OP_DECL
          LIDENT "get"
          COLON ":"
          FN_TYPE
            PATH_TYPE
              UIDENT "Unit"
            THIN_ARROW "->"
            VAR_TYPE
              LIDENT "s"
        OP_DECL
          NEVER_KW "never"
          LIDENT "fail"
          COLON ":"
          FN_TYPE
            PATH_TYPE
              UIDENT "String"
            THIN_ARROW "->"
            VAR_TYPE
              LIDENT "a"
    "#);
}

#[test]
fn effect_operations_must_be_on_indented_lines() {
    insta::assert_snapshot!(shape("effect E where op : A"), @r#"
    SOURCE_FILE
      EFFECT_ITEM
        EFFECT_KW "effect"
        UIDENT "E"
        WHERE_KW "where"
      ERROR
        LIDENT "op"
        COLON ":"
        UIDENT "A"
    ---
    E0011 1:16 expected the operations on indented lines after `where`
    "#);
}

#[test]
fn fixity_declarations() {
    let text = lines(&["infixr 5 </>, ++", "infixl 6 -", "infix 4 ::"]);
    insta::assert_snapshot!(shape(&text), @r#"
    SOURCE_FILE
      FIXITY_ITEM
        INFIXR_KW "infixr"
        INT "5"
        OP "</>"
        COMMA ","
        OP "++"
      FIXITY_ITEM
        INFIXL_KW "infixl"
        INT "6"
        MINUS "-"
      FIXITY_ITEM
        INFIX_KW "infix"
        INT "4"
        CONOP "::"
    "#);
}

#[test]
fn precedence_out_of_range_is_an_error() {
    assert_eq!(
        diagnostics("infixl 10 +"),
        ["E0011 1:8 precedence must be an integer from 0 to 9"]
    );
}

#[test]
fn pub_and_type_are_parsed_but_not_supported_yet() {
    insta::assert_snapshot!(shape("pub type Person = (String, Int)"), @r#"
    SOURCE_FILE
      TYPE_ITEM
        PUB_KW "pub"
        TYPE_KW "type"
        UIDENT "Person"
        EQ "="
        TUPLE_TYPE
          L_PAREN "("
          PATH_TYPE
            UIDENT "String"
          COMMA ","
          PATH_TYPE
            UIDENT "Int"
          R_PAREN ")"
    ---
    E0004 1:1 `pub` is not supported yet
    E0004 1:5 `type` declarations are not supported yet
    "#);
}

#[test]
fn import_and_records_are_skipped_as_not_supported_yet() {
    insta::assert_snapshot!(shape("import Report.Csv\nt : { name : String }"), @r#"
    SOURCE_FILE
      ERROR
        IMPORT_KW "import"
        UIDENT "Report"
        DOT "."
        UIDENT "Csv"
      SIGNATURE
        LIDENT "t"
        COLON ":"
        ERROR
          L_BRACE "{"
          LIDENT "name"
          COLON ":"
          UIDENT "String"
          R_BRACE "}"
    ---
    E0004 1:1 `import` is not supported yet
    E0004 2:5 records are not supported yet
    "#);
}

#[test]
fn errors_in_one_declaration_do_not_affect_the_next() {
    let text = lines(&["x : Int ->", "y : Int", "z : ) Int", "w : Int"]);
    insta::assert_snapshot!(shape(&text), @r#"
    SOURCE_FILE
      SIGNATURE
        LIDENT "x"
        COLON ":"
        FN_TYPE
          PATH_TYPE
            UIDENT "Int"
          THIN_ARROW "->"
      SIGNATURE
        LIDENT "y"
        COLON ":"
        PATH_TYPE
          UIDENT "Int"
      SIGNATURE
        LIDENT "z"
        COLON ":"
      ERROR
        R_PAREN ")"
        UIDENT "Int"
      SIGNATURE
        LIDENT "w"
        COLON ":"
        PATH_TYPE
          UIDENT "Int"
    ---
    E0009 1:9 expected an indented block after `->`
    E0011 3:5 expected a type
    "#);
}

#[test]
fn dot_with_spaces_in_a_qualified_name_is_an_error() {
    insta::assert_snapshot!(shape("f : Foo . Bar"), @r#"
    SOURCE_FILE
      SIGNATURE
        LIDENT "f"
        COLON ":"
        PATH_TYPE
          UIDENT "Foo"
          DOT "."
          UIDENT "Bar"
    ---
    E0010 1:9 unexpected whitespace around `.`
    "#);
}

#[test]
fn reserved_keywords_are_errors() {
    assert_eq!(
        diagnostics("class Foo"),
        ["E0011 1:1 `class` is reserved for future use"]
    );
}

#[test]
fn lone_lowercase_name_is_not_an_item() {
    assert_eq!(diagnostics("foo"), ["E0003 1:1 expected an item"]);
}

#[test]
fn stray_unterminated_string_reports_both_problems() {
    assert_eq!(
        diagnostics("\"abc"),
        [
            "E0002 1:1 unterminated string literal",
            "E0003 1:1 expected an item"
        ]
    );
}
```

`crates/eml_syntax/tests/parser.rs` の `stray_tokens_are_one_error_until_the_next_item` のスナップショットの診断の部分を、次のものにする。

```
    ---
    [E0003] Error: expected an item
       ╭─[ test.em:1:1 ]
       │
     1 │ 1 2 3
       │ ┬  
       │ ╰── not the start of an item
    ───╯
```

`recovery_resumes_at_item_keywords` を、次のテストに置き換える。

```rust
#[test]
fn recovery_resumes_at_the_next_item() {
    // 1つの項目の中のエラーは、次の行の項目の解析に影響しない。
    let text = "a : Int -> )\nb : Int\nc : Int";
    let mut files = SourceFiles::new();
    let file = files.add("test.em", text);
    let (parse, diagnostics) = parse(file, text);
    let codes: Vec<String> = diagnostics.iter().map(|d| d.code.to_string()).collect();
    assert_eq!(codes, ["E0011"]);
    assert_eq!(u32::from(diagnostics[0].primary.range.start()), 11);
    let kinds: Vec<String> = parse
        .syntax()
        .children()
        .map(|node| format!("{:?}", node.kind()))
        .collect();
    assert_eq!(kinds, ["SIGNATURE", "ERROR", "SIGNATURE", "SIGNATURE"]);
}
```

- [ ] **Step 3: テストが失敗することを確認する**

Run: `cargo test -p eml_syntax --test declarations --test parser`
Expected: FAIL (宣言の文法がない。ノードの種類も未定義なのでスナップショットが一致しない)

- [ ] **Step 4: ノードの種類と番号を足す**

`crates/eml_syntax/src/syntax_kind.rs` の `// ノード` の `SOURCE_FILE, ERROR,` の後に足す (`__LAST` の前)。

```rust
    // 項目
    SIGNATURE,
    EQUATION,
    DATA_ITEM,
    /// `data` の1つの選択肢 (`| Some a`、`| a :: List a`)。
    ALT,
    TYPE_ITEM,
    EFFECT_ITEM,
    /// エフェクトの1つの操作の宣言。
    OP_DECL,
    FIXITY_ITEM,

    // 文
    /// 字下げしたブロック。仮想トークンは木に入らないので、子は文だけ。
    BLOCK,
    LET_STMT,
    USE_STMT,
    EXPR_STMT,

    // 式
    IF_EXPR,
    MATCH_EXPR,
    MATCH_ARM,
    HANDLE_EXPR,
    /// handler の操作の節 (`| op x k -> e`)。
    OP_CLAUSE,
    /// handler の `return` の節。
    RETURN_CLAUSE,
    LAMBDA_EXPR,
    /// `let p = e in e2`。
    LET_EXPR,
    /// 演算子の列。被演算子と演算子のトークンを平たく並べる。前置の `-` もトークンとして入る (spec §7)。
    OP_SEQ,
    /// 関数適用。最初の子が関数、残りが引数。
    APP_EXPR,
    RESUME_EXPR,
    DROP_EXPR,
    /// `e.name`、`e.0`。
    FIELD_EXPR,
    /// 変数、コンストラクタ、修飾された名前。
    PATH_EXPR,
    LITERAL,
    UNIT_EXPR,
    PAREN_EXPR,
    TUPLE_EXPR,
    /// `(e : T)`。
    ANNOT_EXPR,
    /// `(+)`。
    OP_REF,
    /// `(1 +)`。
    LEFT_SECTION,
    /// `(+ 1)`。
    RIGHT_SECTION,
    /// `(.name)`。
    FIELD_SECTION,

    // パターン
    WILDCARD_PAT,
    BIND_PAT,
    CON_PAT,
    LITERAL_PAT,
    UNIT_PAT,
    PAREN_PAT,
    TUPLE_PAT,
    /// `x :: rest`。
    INFIX_CON_PAT,
    /// ラムダの引数の `(x : Int)`。
    ANNOT_PAT,

    // 型
    PATH_TYPE,
    VAR_TYPE,
    APP_TYPE,
    FN_TYPE,
    PAREN_TYPE,
    TUPLE_TYPE,
    EFFECT_ROW,
    EFFECT,
```

`crates/eml_syntax/src/lib.rs` の `codes` に足す。

```rust
    pub const SPACE_AROUND_DOT: ErrorCode = ErrorCode(10);
    pub const SYNTAX_ERROR: ErrorCode = ErrorCode(11);
```

- [ ] **Step 5: 文法の共通部分を書く**

`crates/eml_syntax/src/grammar/mod.rs` を次の内容で置き換える。

```rust
//! 本番の構文の文法 (構文設計 spec §5)。構文の差し替えは原則としてこのモジュールの中で行う。
//!
//! 入力はレイアウト段の出力で、仮想トークン `LAYOUT_OPEN` / `LAYOUT_SEP` / `LAYOUT_CLOSE` を含む。
//! spec の `block(x)` は `block_of` で読む。エラーからの回復は、同じ深さの `SEP`
//! (ブロックの中なら `CLOSE` も) まで読み飛ばすのを基本にする (spec §4 のエラー回復)。

mod items;
mod types;

use crate::SyntaxKind::{self, *};
use crate::parser::{Marker, Parser};
use crate::token_set::TokenSet;
use crate::{NOT_YET_SUPPORTED_LABEL, codes};

pub(crate) fn source_file(p: &mut Parser) {
    let m = p.start();
    loop {
        while p.at_sep() {
            p.bump_any();
        }
        if p.at_eof() {
            break;
        }
        if p.at(LAYOUT_CLOSE) {
            // 項目の中で読み残したブロックの終わり。トップレベルでは読み捨てる。
            p.bump_any();
            continue;
        }
        if items::at_item_start(p) {
            items::item(p);
        } else {
            stray_tokens(p);
        }
        if !p.at_sep() && !p.at_eof() && !p.at(LAYOUT_CLOSE) {
            p.error(
                codes::SYNTAX_ERROR,
                format!("unexpected {}", describe(p)),
                "expected the end of the item",
            );
            let m = p.start();
            skip_to_sep(p, false);
            m.complete(p, ERROR);
        }
    }
    m.complete(p, SOURCE_FILE);
}

/// 項目の外にあるトークンの並びを、次の項目まで1つの `ERROR` ノードにまとめる。
/// 診断は1件だけ出す。`ERROR_TOKEN` は字句解析で報告済みなので、それ以外のトークンの位置に出す。
fn stray_tokens(p: &mut Parser) {
    let m = p.start();
    let mut reported = false;
    let mut depth = 0u32;
    while !p.at_eof() && !(depth == 0 && p.at_sep()) {
        if !reported && !p.at(ERROR_TOKEN) && !p.current().is_virtual() {
            p.error(codes::EXPECTED_ITEM, "expected an item", "not the start of an item");
            reported = true;
        }
        match p.current() {
            LAYOUT_OPEN => depth += 1,
            LAYOUT_CLOSE => depth = depth.saturating_sub(1),
            _ => {}
        }
        p.bump_any();
    }
    m.complete(p, ERROR);
}

/// 同じ深さの `SEP` まで (`in_block` なら、今のブロックの `LAYOUT_CLOSE` の手前まで) トークンを読み飛ばす。
/// 途中のブロックは丸ごと読み飛ばす。ノードは作らないので、呼び出し側が `ERROR` で包む。
fn skip_to_sep(p: &mut Parser, in_block: bool) {
    let mut depth = 0u32;
    while !p.at_eof() {
        match p.current() {
            LAYOUT_SEP | SEMICOLON if depth == 0 => break,
            LAYOUT_CLOSE if depth == 0 && in_block => break,
            LAYOUT_OPEN => depth += 1,
            LAYOUT_CLOSE => depth = depth.saturating_sub(1),
            _ => {}
        }
        p.bump_any();
    }
}

/// `block(x) ::= OPEN x (SEP x)* CLOSE` を読む。今のトークンは `LAYOUT_OPEN`。
/// `item` は項目を1つ読む。今の位置から項目を始められなければ、何も読まずに偽を返す。
/// レイアウト段の回復で作った空のブロック (`OPEN` の直後の `CLOSE`) は、黙って受け入れる (E0009 は報告済み)。
fn block_of(p: &mut Parser, expected: &str, mut item: impl FnMut(&mut Parser) -> bool) {
    p.bump(LAYOUT_OPEN);
    if p.eat(LAYOUT_CLOSE) {
        return;
    }
    let mut attempted = false;
    loop {
        while p.at_sep() {
            p.bump_any();
        }
        if p.at(LAYOUT_CLOSE) || p.at_eof() {
            break;
        }
        attempted = true;
        if !item(p) {
            p.error(
                codes::SYNTAX_ERROR,
                format!("expected {expected}"),
                format!("found {}", describe(p)),
            );
        }
        if !p.at_sep() && !p.at(LAYOUT_CLOSE) && !p.at_eof() {
            p.error(
                codes::SYNTAX_ERROR,
                format!("unexpected {}", describe(p)),
                "expected a new line or the end of the block",
            );
            let m = p.start();
            skip_to_sep(p, true);
            m.complete(p, ERROR);
        }
    }
    if !attempted {
        p.error(
            codes::SYNTAX_ERROR,
            format!("expected {expected}"),
            format!("found {}", describe(p)),
        );
    }
    p.eat(LAYOUT_CLOSE);
}

/// 1つの要素だけを持つブロック (型の途中で改行したときなど) の終わりを読む。
/// 余分なトークンがあれば、ブロックの終わりまでを `ERROR` にまとめる。
fn close_block(p: &mut Parser) {
    if !p.at(LAYOUT_CLOSE) && !p.at_eof() {
        p.error(
            codes::SYNTAX_ERROR,
            format!("unexpected {}", describe(p)),
            "expected the end of the indented block",
        );
        let m = p.start();
        let mut depth = 0u32;
        while !p.at_eof() && !(depth == 0 && p.at(LAYOUT_CLOSE)) {
            match p.current() {
                LAYOUT_OPEN => depth += 1,
                LAYOUT_CLOSE => depth -= 1,
                _ => {}
            }
            p.bump_any();
        }
        m.complete(p, ERROR);
    }
    p.eat(LAYOUT_CLOSE);
}

/// `kind` があれば読み進める。なければ診断を出して偽を返す。
fn expect(p: &mut Parser, kind: SyntaxKind) -> bool {
    if p.eat(kind) {
        return true;
    }
    p.error(
        codes::SYNTAX_ERROR,
        format!("expected {}", token_name(kind)),
        format!("found {}", describe(p)),
    );
    false
}

fn token_name(kind: SyntaxKind) -> &'static str {
    match kind {
        EQ => "`=`",
        COLON => "`:`",
        COMMA => "`,`",
        PIPE => "`|`",
        THIN_ARROW => "`->`",
        LEFT_ARROW => "`<-`",
        R_PAREN => "`)`",
        WHERE_KW => "`where`",
        WITH_KW => "`with`",
        THEN_KW => "`then`",
        IN_KW => "`in`",
        UIDENT => "a capitalized name",
        LIDENT => "a lowercase name",
        INT => "an integer",
        _ => "a token",
    }
}

/// 今のトークンを、診断のラベルのために説明する。
fn describe(p: &Parser) -> String {
    match p.current() {
        EOF => "the end of the file".to_string(),
        LAYOUT_SEP => "a new line".to_string(),
        LAYOUT_OPEN => "an indented block".to_string(),
        LAYOUT_CLOSE => "the end of the block".to_string(),
        _ => format!("`{}`", p.current_text()),
    }
}

/// 今のトークンの位置に E0004 (まだ対応していない構文) を出す。
fn not_yet_supported(p: &mut Parser, message: &str) {
    p.error(codes::NOT_YET_SUPPORTED, message, NOT_YET_SUPPORTED_LABEL);
}

/// まだ対応していない括弧の構文 (`[...]`、`{...}`) に E0004 を出し、対応する閉じ括弧まで読み飛ばす。
/// ノードは作らないので、呼び出し側が `ERROR` で包む。
fn unsupported_group(p: &mut Parser, message: &str) {
    not_yet_supported(p, message);
    let open = p.current();
    let close = if open == L_BRACK { R_BRACK } else { R_BRACE };
    let mut depth = 0u32;
    while !p.at_eof() {
        let kind = p.current();
        p.bump_any();
        if kind == open {
            depth += 1;
        } else if kind == close {
            depth -= 1;
            if depth == 0 {
                break;
            }
        }
    }
}

/// qcon ::= (UIDENT '.')* UIDENT
fn qcon(p: &mut Parser) {
    while p.at(UIDENT) && p.nth(1) == DOT && p.nth(2) == UIDENT {
        p.bump(UIDENT);
        dot(p);
    }
    p.bump(UIDENT);
}

/// 修飾とフィールドアクセスの `.` を読む。前後に空白があれば E0010 を出す (spec §5)。
fn dot(p: &mut Parser) {
    if !p.touches_prev() || !p.touches_next() {
        p.error(
            codes::SPACE_AROUND_DOT,
            "unexpected whitespace around `.`",
            "write `.` without spaces; compose functions with `>>`",
        );
    }
    p.bump(DOT);
}
```

- [ ] **Step 6: 型の文法を書く**

`crates/eml_syntax/src/grammar/types.rs`:

```rust
//! 型と row (spec §5 の type / btype / type_atom / row)。

use super::*;

const TYPE_ATOM_START: TokenSet = TokenSet::new(&[UIDENT, LIDENT, L_PAREN, L_BRACE]);

pub(super) fn at_type_atom_start(p: &Parser) -> bool {
    p.at_ts(TYPE_ATOM_START)
}

/// type ::= btype ('->' row? type)?
pub(super) fn type_(p: &mut Parser) -> bool {
    let m = p.start();
    if !btype(p) {
        m.abandon(p);
        p.error(
            codes::SYNTAX_ERROR,
            "expected a type",
            format!("found {}", describe(p)),
        );
        return false;
    }
    if p.at(THIN_ARROW) {
        p.bump(THIN_ARROW);
        arrow_result(p);
        m.complete(p, FN_TYPE);
    } else {
        m.abandon(p);
    }
    true
}

/// `->` の右側。`->` で行が終わると、レイアウト段がブロックを開くので、その中の1つの型として読む。
fn arrow_result(p: &mut Parser) {
    let in_block = p.eat(LAYOUT_OPEN);
    if in_block && p.eat(LAYOUT_CLOSE) {
        // レイアウト段の回復で作った空のブロック。E0009 は報告済み。
        return;
    }
    if at_angle(p, '<') {
        effect_row(p);
    }
    type_(p);
    if in_block {
        close_block(p);
    }
}

/// `type` の宣言の `=` の右側。`=` で行が終われば、ブロックの中の1つの型として読む。
pub(super) fn type_or_block(p: &mut Parser) {
    let in_block = p.eat(LAYOUT_OPEN);
    if in_block && p.eat(LAYOUT_CLOSE) {
        return;
    }
    type_(p);
    if in_block {
        close_block(p);
    }
}

/// btype ::= qcon type_atom* | type_atom
pub(super) fn btype(p: &mut Parser) -> bool {
    if !p.at(UIDENT) {
        return type_atom(p);
    }
    let m = p.start();
    qcon(p);
    let mut args = 0;
    while at_type_atom_start(p) {
        type_atom(p);
        args += 1;
    }
    m.complete(p, if args == 0 { PATH_TYPE } else { APP_TYPE });
    true
}

/// type_atom ::= qcon | LIDENT | '(' type ')' | '(' type (',' type)+ ')' | レコード (S2)
pub(super) fn type_atom(p: &mut Parser) -> bool {
    let m = p.start();
    let kind = match p.current() {
        UIDENT => {
            qcon(p);
            PATH_TYPE
        }
        LIDENT => {
            p.bump(LIDENT);
            VAR_TYPE
        }
        L_PAREN => {
            p.bump(L_PAREN);
            type_(p);
            let mut count = 1;
            while p.eat(COMMA) {
                type_(p);
                count += 1;
            }
            expect(p, R_PAREN);
            if count == 1 { PAREN_TYPE } else { TUPLE_TYPE }
        }
        L_BRACE => {
            unsupported_group(p, "records are not supported yet");
            ERROR
        }
        _ => {
            m.abandon(p);
            return false;
        }
    };
    m.complete(p, kind);
    true
}

/// row ::= '<' '>' | '<' LIDENT '>' | '<' effect (',' effect)* ('|' LIDENT)? '>'
fn effect_row(p: &mut Parser) {
    let m = p.start();
    eat_angle(p, '<', L_ANGLE);
    if !at_angle(p, '>') {
        if p.at(LIDENT) {
            p.bump(LIDENT);
        } else {
            effect(p);
            while p.eat(COMMA) {
                effect(p);
            }
            if p.eat(PIPE) {
                expect(p, LIDENT);
            }
        }
    }
    if !eat_angle(p, '>', R_ANGLE) {
        p.error(
            codes::SYNTAX_ERROR,
            "expected `>`",
            format!("found {}", describe(p)),
        );
    }
    m.complete(p, EFFECT_ROW);
}

/// effect ::= qcon type_atom*
fn effect(p: &mut Parser) {
    if !p.at(UIDENT) {
        p.error(
            codes::SYNTAX_ERROR,
            "expected an effect",
            format!("found {}", describe(p)),
        );
        return;
    }
    let m = p.start();
    qcon(p);
    while at_type_atom_start(p) {
        type_atom(p);
    }
    m.complete(p, EFFECT);
}

/// 今のトークンが `c` で始まる演算子か。型の中では `<>` や `>->` を分けて読む (spec §5)。
fn at_angle(p: &Parser, c: char) -> bool {
    p.at(OP) && p.current_text().starts_with(c)
}

fn eat_angle(p: &mut Parser, c: char, kind: SyntaxKind) -> bool {
    if !at_angle(p, c) {
        return false;
    }
    if p.current_text().len() == 1 {
        p.bump_remap(kind);
    } else {
        p.split_first_char(kind);
    }
    true
}
```

- [ ] **Step 7: 宣言の文法を書く**

`crates/eml_syntax/src/grammar/items.rs`:

```rust
//! 項目 (spec §5 の item / decl、§6)。

use super::*;

/// 項目を始めるキーワード。将来の予約語も、ここで受けてエラーにする。
const ITEM_KEYWORDS: TokenSet = TokenSet::new(&[
    DATA_KW, TYPE_KW, EFFECT_KW, INFIXL_KW, INFIXR_KW, INFIX_KW, IMPORT_KW, PUB_KW, FORALL_KW,
    CLASS_KW, INSTANCE_KW,
]);

/// fixity の宣言に書ける演算子。
const DECLARABLE_OPERATORS: TokenSet = TokenSet::new(&[OP, CONOP, MINUS]);

pub(super) fn at_item_start(p: &Parser) -> bool {
    p.at_ts(ITEM_KEYWORDS) || (p.at(LIDENT) && p.nth(1) == COLON) || at_operator_signature(p)
}

/// `(OP) : type` の形のシグネチャか。
fn at_operator_signature(p: &Parser) -> bool {
    p.at(L_PAREN) && matches!(p.nth(1), OP | MINUS) && p.nth(2) == R_PAREN
}

/// item ::= 'pub'? decl | equation | import_item
pub(super) fn item(p: &mut Parser) {
    let m = p.start();
    if p.at(PUB_KW) {
        not_yet_supported(p, "`pub` is not supported yet");
        p.bump(PUB_KW);
    }
    match p.current() {
        DATA_KW => data_item(p, m),
        TYPE_KW => type_item(p, m),
        EFFECT_KW => effect_item(p, m),
        INFIXL_KW | INFIXR_KW | INFIX_KW => fixity_item(p, m),
        IMPORT_KW => import_item(p, m),
        FORALL_KW | CLASS_KW | INSTANCE_KW => reserved_item(p, m),
        LIDENT if p.nth(1) == COLON => signature(p, m),
        _ if at_operator_signature(p) => signature(p, m),
        _ => {
            // `pub` の後ろに項目がない
            p.error(codes::EXPECTED_ITEM, "expected an item", "not the start of an item");
            skip_to_sep(p, false);
            m.complete(p, ERROR);
        }
    }
}

/// signature ::= var ':' type 、var ::= LIDENT | '(' OP ')'
fn signature(p: &mut Parser, m: Marker) {
    if p.at(LIDENT) {
        p.bump(LIDENT);
    } else {
        p.bump(L_PAREN);
        p.bump_any();
        p.bump(R_PAREN);
    }
    if expect(p, COLON) {
        types::type_(p);
    }
    m.complete(p, SIGNATURE);
}

/// data_item ::= 'data' UIDENT LIDENT* '=' alts
fn data_item(p: &mut Parser, m: Marker) {
    p.bump(DATA_KW);
    expect(p, UIDENT);
    while p.at(LIDENT) {
        p.bump(LIDENT);
    }
    if expect(p, EQ) {
        alts(p);
    }
    m.complete(p, DATA_ITEM);
}

/// alts ::= block(alt) | alt+
fn alts(p: &mut Parser) {
    if p.at(LAYOUT_OPEN) {
        block_of(p, "a constructor starting with `|`", alt);
        return;
    }
    let mut first = true;
    // 最初の選択肢だけは、`|` を書き忘れた形 (`= A | B`) も受けて診断する。
    while p.at(PIPE) || (first && p.at(UIDENT)) {
        alt(p);
        first = false;
    }
    if first {
        p.error(
            codes::SYNTAX_ERROR,
            "expected a constructor starting with `|`",
            format!("found {}", describe(p)),
        );
    }
}

/// alt ::= '|' UIDENT type_atom* | '|' btype CONOP btype
fn alt(p: &mut Parser) -> bool {
    if !p.at(PIPE) && !types::at_type_atom_start(p) {
        return false;
    }
    let m = p.start();
    if !p.eat(PIPE) {
        p.error(
            codes::SYNTAX_ERROR,
            "expected `|` before the constructor",
            "each constructor starts with `|`",
        );
    }
    if p.at(UIDENT) && p.nth(1) != CONOP {
        p.bump(UIDENT);
        while types::at_type_atom_start(p) {
            types::type_atom(p);
        }
    } else {
        if !types::btype(p) {
            p.error(
                codes::SYNTAX_ERROR,
                "expected a constructor",
                format!("found {}", describe(p)),
            );
        }
        if p.eat(CONOP) {
            if !types::btype(p) {
                p.error(
                    codes::SYNTAX_ERROR,
                    "expected a type",
                    format!("found {}", describe(p)),
                );
            }
        } else {
            p.error(
                codes::SYNTAX_ERROR,
                "expected a constructor name or an infix constructor",
                format!("found {}", describe(p)),
            );
        }
    }
    m.complete(p, ALT);
    true
}

/// type_item ::= 'type' UIDENT LIDENT* '=' type 。S2 で実装するので、読んだうえで E0004 を出す。
fn type_item(p: &mut Parser, m: Marker) {
    not_yet_supported(p, "`type` declarations are not supported yet");
    p.bump(TYPE_KW);
    expect(p, UIDENT);
    while p.at(LIDENT) {
        p.bump(LIDENT);
    }
    if expect(p, EQ) {
        types::type_or_block(p);
    }
    m.complete(p, TYPE_ITEM);
}

/// effect_item ::= 'effect' UIDENT LIDENT* 'where' block(op_decl)
fn effect_item(p: &mut Parser, m: Marker) {
    p.bump(EFFECT_KW);
    expect(p, UIDENT);
    while p.at(LIDENT) {
        p.bump(LIDENT);
    }
    if expect(p, WHERE_KW) {
        if p.at(LAYOUT_OPEN) {
            block_of(p, "an operation signature", op_decl);
        } else {
            p.error(
                codes::SYNTAX_ERROR,
                "expected the operations on indented lines after `where`",
                format!("found {}", describe(p)),
            );
        }
    }
    m.complete(p, EFFECT_ITEM);
}

/// op_decl ::= ('never' | 'once' | 'multi')? LIDENT ':' type
fn op_decl(p: &mut Parser) -> bool {
    if !matches!(p.current(), NEVER_KW | ONCE_KW | MULTI_KW | LIDENT) {
        return false;
    }
    let m = p.start();
    if !p.at(LIDENT) {
        p.bump_any();
    }
    expect(p, LIDENT);
    if expect(p, COLON) {
        types::type_(p);
    }
    m.complete(p, OP_DECL);
    true
}

/// fixity_item ::= ('infixl' | 'infixr' | 'infix') INT OP (',' OP)*
fn fixity_item(p: &mut Parser, m: Marker) {
    p.bump_any();
    if p.at(INT) {
        let text = p.current_text();
        if !(text.len() == 1 && text.as_bytes()[0].is_ascii_digit()) {
            p.error(
                codes::SYNTAX_ERROR,
                "precedence must be an integer from 0 to 9",
                "out of range",
            );
        }
        p.bump(INT);
    } else {
        p.error(
            codes::SYNTAX_ERROR,
            "expected a precedence from 0 to 9",
            format!("found {}", describe(p)),
        );
    }
    loop {
        if p.at_ts(DECLARABLE_OPERATORS) {
            p.bump_any();
        } else {
            p.error(
                codes::SYNTAX_ERROR,
                "expected an operator",
                format!("found {}", describe(p)),
            );
            break;
        }
        if !p.eat(COMMA) {
            break;
        }
    }
    m.complete(p, FIXITY_ITEM);
}

/// import は S2 で実装する。E0004 を出し、次の項目まで読み飛ばす。
fn import_item(p: &mut Parser, m: Marker) {
    not_yet_supported(p, "`import` is not supported yet");
    skip_to_sep(p, false);
    m.complete(p, ERROR);
}

fn reserved_item(p: &mut Parser, m: Marker) {
    p.error(
        codes::SYNTAX_ERROR,
        format!("`{}` is reserved for future use", p.current_text()),
        "not usable yet",
    );
    skip_to_sep(p, false);
    m.complete(p, ERROR);
}
```

- [ ] **Step 8: テストが通ることを確認する**

Run: `cargo test -p eml_syntax`
Expected: PASS (すべて)

- [ ] **Step 9: UI テストのスナップショットを更新する**

`crates/eml_cli/tests/snapshots/ui__check_fail@stray_tokens.em.snap` と `ui__check_fail@multiple_errors.em.snap` の `[E0003] Error: expected an item (`fn`, `type`, or `effect`)` の行を `[E0003] Error: expected an item` にする (ほかの行は変えない)。

Run: `cargo test -p eml_cli --test ui`
Expected: PASS

- [ ] **Step 10: 全体のテストと lint を通す**

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check`
Expected: すべて PASS

- [ ] **Step 11: Commit**

```bash
git add crates/eml_syntax crates/eml_cli/tests/snapshots
git commit -m "Parse declarations and types in the final syntax"
```

### Task 5: パターン、等式、基本の式

**Files:**
- Create: `crates/eml_syntax/src/grammar/patterns.rs`
- Create: `crates/eml_syntax/src/grammar/expressions.rs`
- Modify: `crates/eml_syntax/src/grammar/mod.rs` (`mod expressions; mod patterns;`)
- Modify: `crates/eml_syntax/src/grammar/items.rs` (等式と演算子の定義を項目として読む)
- Create: `crates/eml_syntax/tests/expressions.rs`

**Interfaces:**
- Consumes: Task 4 の共通の補助 (`block_of`、`expect`、`describe`、`not_yet_supported`、`unsupported_group`、`qcon`、`dot`、`skip_to_sep`) と `types::type_`
- Produces:
  - `grammar::patterns`: `pattern(p) -> bool`、`apat(p) -> bool`、`param(p) -> bool` (`(pat : type)` を許す apat)、`at_apat_start(p) -> bool`、`at_apat_start_at(p, n) -> bool`、`apat_len(p) -> Option<usize>`
  - `grammar::expressions`: `body(p)`、`expr(p) -> bool` (何も読めなければ診断を出さずに偽)。内部の `stmt`、`op_expr(p, section: bool) -> OpExpr`、`operand`、`app`、`postfix`、`atom`、`paren_expr` は Task 6・7 が拡張する
  - 等式のノード: `EQUATION` の子は、関数の定義なら `LIDENT` (名前のトークン)、引数のパターン、`EQ`、本体 (`BLOCK` か式)。演算子の定義なら、パターン、演算子のトークン (`OP` / `MINUS`)、パターン、`EQ`、本体

この段階の式は、演算子の列、前置の `-`、関数適用、フィールドアクセス、名前、リテラル、括弧 (単位、タプル、型の明示、セクション) と、ブロックの `let` と式文。`if` / `match` / `fn` / `use` / `let ... in` (式の位置) は Task 6、`handle` / `resume` / `drop` は Task 7。

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_syntax/tests/expressions.rs`:

```rust
mod common;

use common::{diagnostics, lines, shape};

#[test]
fn equation_with_a_constructor_pattern() {
    insta::assert_snapshot!(shape("len Nil = 0"), @r#"
    SOURCE_FILE
      EQUATION
        LIDENT "len"
        CON_PAT
          UIDENT "Nil"
        EQ "="
        LITERAL
          INT "0"
    "#);
}

#[test]
fn parameter_patterns() {
    insta::assert_snapshot!(shape("f (Some (x, _)) (y :: rest) 0 -1 \"s\" () = x"), @r#"
    SOURCE_FILE
      EQUATION
        LIDENT "f"
        PAREN_PAT
          L_PAREN "("
          CON_PAT
            UIDENT "Some"
            TUPLE_PAT
              L_PAREN "("
              BIND_PAT
                LIDENT "x"
              COMMA ","
              WILDCARD_PAT
                UNDERSCORE "_"
              R_PAREN ")"
          R_PAREN ")"
        PAREN_PAT
          L_PAREN "("
          INFIX_CON_PAT
            BIND_PAT
              LIDENT "y"
            CONOP "::"
            BIND_PAT
              LIDENT "rest"
          R_PAREN ")"
        LITERAL_PAT
          INT "0"
        LITERAL_PAT
          MINUS "-"
          INT "1"
        LITERAL_PAT
          STRING "\"s\""
        UNIT_PAT
          L_PAREN "("
          R_PAREN ")"
        EQ "="
        PATH_EXPR
          LIDENT "x"
    "#);
}

#[test]
fn block_body_with_let_and_expression_statements() {
    let text = lines(&["main () =", "  let x = 1", "  let n : Int = x", "  println x"]);
    insta::assert_snapshot!(shape(&text), @r#"
    SOURCE_FILE
      EQUATION
        LIDENT "main"
        UNIT_PAT
          L_PAREN "("
          R_PAREN ")"
        EQ "="
        BLOCK
          LET_STMT
            LET_KW "let"
            BIND_PAT
              LIDENT "x"
            EQ "="
            LITERAL
              INT "1"
          LET_STMT
            LET_KW "let"
            BIND_PAT
              LIDENT "n"
            COLON ":"
            PATH_TYPE
              UIDENT "Int"
            EQ "="
            PATH_EXPR
              LIDENT "x"
          EXPR_STMT
            APP_EXPR
              PATH_EXPR
                LIDENT "println"
              PATH_EXPR
                LIDENT "x"
    "#);
}

#[test]
fn operator_sequence_is_flat_with_prefix_minus() {
    insta::assert_snapshot!(shape("x = -a + b * c"), @r#"
    SOURCE_FILE
      EQUATION
        LIDENT "x"
        EQ "="
        OP_SEQ
          MINUS "-"
          PATH_EXPR
            LIDENT "a"
          OP "+"
          PATH_EXPR
            LIDENT "b"
          OP "*"
          PATH_EXPR
            LIDENT "c"
    "#);
}

#[test]
fn minus_after_a_function_is_subtraction() {
    insta::assert_snapshot!(shape("y = f -1 :: xs"), @r#"
    SOURCE_FILE
      EQUATION
        LIDENT "y"
        EQ "="
        OP_SEQ
          PATH_EXPR
            LIDENT "f"
          MINUS "-"
          LITERAL
            INT "1"
          CONOP "::"
          PATH_EXPR
            LIDENT "xs"
    "#);
}

#[test]
fn application_field_access_and_qualified_names() {
    insta::assert_snapshot!(shape("y = String.split_once \" \" line.text t.0.1"), @r#"
    SOURCE_FILE
      EQUATION
        LIDENT "y"
        EQ "="
        APP_EXPR
          PATH_EXPR
            UIDENT "String"
            DOT "."
            LIDENT "split_once"
          LITERAL
            STRING "\" \""
          FIELD_EXPR
            PATH_EXPR
              LIDENT "line"
            DOT "."
            LIDENT "text"
          FIELD_EXPR
            FIELD_EXPR
              PATH_EXPR
                LIDENT "t"
              DOT "."
              INT "0"
            DOT "."
            INT "1"
    "#);
}

#[test]
fn sections() {
    insta::assert_snapshot!(shape("s = ((+), (+ 1), (1 +), (.name), (- 1), (-))"), @r#"
    SOURCE_FILE
      EQUATION
        LIDENT "s"
        EQ "="
        TUPLE_EXPR
          L_PAREN "("
          OP_REF
            L_PAREN "("
            OP "+"
            R_PAREN ")"
          COMMA ","
          RIGHT_SECTION
            L_PAREN "("
            OP "+"
            LITERAL
              INT "1"
            R_PAREN ")"
          COMMA ","
          LEFT_SECTION
            L_PAREN "("
            LITERAL
              INT "1"
            OP "+"
            R_PAREN ")"
          COMMA ","
          FIELD_SECTION
            L_PAREN "("
            DOT "."
            LIDENT "name"
            R_PAREN ")"
          COMMA ","
          PAREN_EXPR
            L_PAREN "("
            OP_SEQ
              MINUS "-"
              LITERAL
                INT "1"
            R_PAREN ")"
          COMMA ","
          OP_REF
            L_PAREN "("
            MINUS "-"
            R_PAREN ")"
          R_PAREN ")"
    "#);
}

#[test]
fn annotation_and_unit() {
    insta::assert_snapshot!(shape("a = ((x : Int), ())"), @r#"
    SOURCE_FILE
      EQUATION
        LIDENT "a"
        EQ "="
        TUPLE_EXPR
          L_PAREN "("
          ANNOT_EXPR
            L_PAREN "("
            PATH_EXPR
              LIDENT "x"
            COLON ":"
            PATH_TYPE
              UIDENT "Int"
            R_PAREN ")"
          COMMA ","
          UNIT_EXPR
            L_PAREN "("
            R_PAREN ")"
          R_PAREN ")"
    "#);
}

#[test]
fn deeper_lines_continue_the_expression() {
    let text = lines(&["t =", "  lines s", "    |> map f", "    |> sum"]);
    insta::assert_snapshot!(shape(&text), @r#"
    SOURCE_FILE
      EQUATION
        LIDENT "t"
        EQ "="
        BLOCK
          EXPR_STMT
            OP_SEQ
              APP_EXPR
                PATH_EXPR
                  LIDENT "lines"
                PATH_EXPR
                  LIDENT "s"
              OP "|>"
              APP_EXPR
                PATH_EXPR
                  LIDENT "map"
                PATH_EXPR
                  LIDENT "f"
              OP "|>"
              PATH_EXPR
                LIDENT "sum"
    "#);
}

#[test]
fn dot_with_spaces_is_an_error() {
    insta::assert_snapshot!(shape("x = a . b"), @r#"
    SOURCE_FILE
      EQUATION
        LIDENT "x"
        EQ "="
        FIELD_EXPR
          PATH_EXPR
            LIDENT "a"
          DOT "."
          LIDENT "b"
    ---
    E0010 1:7 unexpected whitespace around `.`
    "#);
}

#[test]
fn operator_definition() {
    insta::assert_snapshot!(shape("dir </> name = join dir name"), @r#"
    SOURCE_FILE
      EQUATION
        BIND_PAT
          LIDENT "dir"
        OP "</>"
        BIND_PAT
          LIDENT "name"
        EQ "="
        APP_EXPR
          PATH_EXPR
            LIDENT "join"
          PATH_EXPR
            LIDENT "dir"
          PATH_EXPR
            LIDENT "name"
    "#);
}

#[test]
fn top_level_pattern_bindings_are_errors() {
    insta::assert_snapshot!(shape("(a, b) = p"), @r#"
    SOURCE_FILE
      ERROR
        TUPLE_PAT
          L_PAREN "("
          BIND_PAT
            LIDENT "a"
          COMMA ","
          BIND_PAT
            LIDENT "b"
          R_PAREN ")"
        EQ "="
        LIDENT "p"
    ---
    E0011 1:8 top-level pattern bindings are not allowed
    "#);
    assert_eq!(
        diagnostics("x :: rest = xs"),
        ["E0011 1:3 top-level pattern bindings are not allowed"]
    );
}

#[test]
fn later_stage_literals_are_not_supported_yet() {
    assert_eq!(
        diagnostics("x = (1.5, 'c', [1], r\"raw\", \"\"\"m\"\"\", `ls`)"),
        [
            "E0004 1:6 floating-point literals are not supported yet",
            "E0004 1:11 character literals are not supported yet",
            "E0004 1:16 lists are not supported yet",
            "E0004 1:21 raw strings are not supported yet",
            "E0004 1:29 multi-line strings are not supported yet",
            "E0004 1:38 command literals are not supported yet",
        ]
    );
}

#[test]
fn missing_operand_does_not_affect_the_next_item() {
    insta::assert_snapshot!(shape("a = 1 +\nb = 2"), @r#"
    SOURCE_FILE
      EQUATION
        LIDENT "a"
        EQ "="
        OP_SEQ
          LITERAL
            INT "1"
          OP "+"
      EQUATION
        LIDENT "b"
        EQ "="
        LITERAL
          INT "2"
    ---
    E0011 2:1 expected an expression
    "#);
}

#[test]
fn let_in_as_a_statement() {
    let text = lines(&["f x =", "  let y = x in y"]);
    insta::assert_snapshot!(shape(&text), @r#"
    SOURCE_FILE
      EQUATION
        LIDENT "f"
        BIND_PAT
          LIDENT "x"
        EQ "="
        BLOCK
          EXPR_STMT
            LET_EXPR
              LET_KW "let"
              BIND_PAT
                LIDENT "y"
              EQ "="
              PATH_EXPR
                LIDENT "x"
              IN_KW "in"
              PATH_EXPR
                LIDENT "y"
    "#);
}

#[test]
fn body_on_an_unindented_line_gets_an_empty_block() {
    insta::assert_snapshot!(shape("f x =\ng = 1"), @r#"
    SOURCE_FILE
      EQUATION
        LIDENT "f"
        BIND_PAT
          LIDENT "x"
        EQ "="
        BLOCK
      EQUATION
        LIDENT "g"
        EQ "="
        LITERAL
          INT "1"
    ---
    E0009 1:5 expected an indented block after `=`
    "#);
}

#[test]
fn unclosed_paren_is_reported() {
    assert_eq!(diagnostics("a = f (1\nb = 2"), ["E0011 2:3 expected `)`"]);
}

#[test]
fn leftover_tokens_in_a_statement_are_skipped_to_the_next_line() {
    assert_eq!(
        diagnostics(&lines(&["f =", "  g x)", "  h"])),
        ["E0011 2:6 unexpected `)`"]
    );
}

#[test]
fn error_token_in_an_expression_is_reported_once() {
    assert_eq!(
        diagnostics("z = f € x"),
        ["E0001 1:7 unexpected character `€`"]
    );
}
```

- [ ] **Step 2: テストが失敗することを確認する**

Run: `cargo test -p eml_syntax --test expressions`
Expected: FAIL (等式が項目として認識されず、E0003 になる)

- [ ] **Step 3: パターンの文法を書く**

`crates/eml_syntax/src/grammar/patterns.rs`:

```rust
//! パターン (spec §5 の pat / cpat / apat)。

use super::*;

pub(super) fn at_apat_start(p: &Parser) -> bool {
    at_apat_start_at(p, 0)
}

/// `n` 個先から apat を始められるか。`-` は整数が続くときだけ (負の数のリテラル)。
pub(super) fn at_apat_start_at(p: &Parser, n: usize) -> bool {
    match p.nth(n) {
        UNDERSCORE | LIDENT | UIDENT | INT | STRING | CHAR | L_PAREN | L_BRACK | L_BRACE => true,
        MINUS => p.nth(n + 1) == INT,
        _ => false,
    }
}

/// 今の位置から始まる apat のトークン数。apat でなければ `None`。
/// 括弧は対応する閉じ括弧までを数える (項目の種類を決める先読みに使う)。
pub(super) fn apat_len(p: &Parser) -> Option<usize> {
    match p.current() {
        UNDERSCORE | LIDENT | INT | STRING | CHAR => Some(1),
        MINUS if p.nth(1) == INT => Some(2),
        UIDENT => {
            let mut n = 1;
            while p.nth(n) == DOT && p.nth(n + 1) == UIDENT {
                n += 2;
            }
            Some(n)
        }
        L_PAREN | L_BRACK | L_BRACE => {
            let mut depth = 0u32;
            let mut n = 0;
            loop {
                match p.nth(n) {
                    L_PAREN | L_BRACK | L_BRACE => depth += 1,
                    R_PAREN | R_BRACK | R_BRACE => {
                        depth -= 1;
                        if depth == 0 {
                            return Some(n + 1);
                        }
                    }
                    EOF => return None,
                    _ => {}
                }
                n += 1;
            }
        }
        _ => None,
    }
}

/// pat ::= cpat (CONOP pat)?  (右結合)
pub(super) fn pattern(p: &mut Parser) -> bool {
    let m = p.start();
    if !cpat(p) {
        m.abandon(p);
        return false;
    }
    if p.at(CONOP) {
        p.bump(CONOP);
        if !pattern(p) {
            p.error(
                codes::SYNTAX_ERROR,
                "expected a pattern",
                format!("found {}", describe(p)),
            );
        }
        m.complete(p, INFIX_CON_PAT);
    } else {
        m.abandon(p);
    }
    true
}

/// cpat ::= qcon apat+ | apat
fn cpat(p: &mut Parser) -> bool {
    if !p.at(UIDENT) {
        return apat(p);
    }
    let m = p.start();
    qcon(p);
    while at_apat_start(p) {
        apat(p);
    }
    m.complete(p, CON_PAT);
    true
}

pub(super) fn apat(p: &mut Parser) -> bool {
    apat_with(p, false)
}

/// ラムダの引数。`(pat : type)` も書ける (spec §5 の param)。
pub(super) fn param(p: &mut Parser) -> bool {
    apat_with(p, true)
}

/// apat ::= '_' | LIDENT | qcon | literal | '-' INT | '(' ')' | '(' pat ')' | '(' pat (',' pat)+ ','? ')'
///        | リスト (S2) | レコード (S2)
fn apat_with(p: &mut Parser, annotated: bool) -> bool {
    let m = p.start();
    let kind = match p.current() {
        UNDERSCORE => {
            p.bump_any();
            WILDCARD_PAT
        }
        LIDENT => {
            p.bump_any();
            BIND_PAT
        }
        UIDENT => {
            qcon(p);
            CON_PAT
        }
        INT | STRING => {
            p.bump_any();
            LITERAL_PAT
        }
        CHAR => {
            not_yet_supported(p, "character literals are not supported yet");
            p.bump_any();
            LITERAL_PAT
        }
        MINUS if p.nth(1) == INT => {
            p.bump(MINUS);
            p.bump(INT);
            LITERAL_PAT
        }
        L_PAREN => paren_pat(p, annotated),
        L_BRACK => {
            unsupported_group(p, "lists are not supported yet");
            ERROR
        }
        L_BRACE => {
            unsupported_group(p, "records are not supported yet");
            ERROR
        }
        _ => {
            m.abandon(p);
            return false;
        }
    };
    m.complete(p, kind);
    true
}

fn paren_pat(p: &mut Parser, annotated: bool) -> SyntaxKind {
    p.bump(L_PAREN);
    if p.eat(R_PAREN) {
        return UNIT_PAT;
    }
    if !pattern(p) {
        p.error(
            codes::SYNTAX_ERROR,
            "expected a pattern",
            format!("found {}", describe(p)),
        );
    }
    let kind = if annotated && p.eat(COLON) {
        types::type_(p);
        ANNOT_PAT
    } else if p.at(COMMA) {
        let mut count = 1;
        while p.eat(COMMA) {
            if p.at(R_PAREN) && count >= 2 {
                break;
            }
            if !pattern(p) {
                p.error(
                    codes::SYNTAX_ERROR,
                    "expected a pattern",
                    format!("found {}", describe(p)),
                );
                break;
            }
            count += 1;
        }
        TUPLE_PAT
    } else {
        PAREN_PAT
    };
    expect(p, R_PAREN);
    kind
}
```

- [ ] **Step 4: 式の文法を書く**

`crates/eml_syntax/src/grammar/expressions.rs`:

```rust
//! 文と式 (spec §5 の body / stmt / expr / op_expr / app / postfix / atom)。

use super::*;
use crate::parser::CompletedMarker;

/// 中置の位置に置ける演算子。`-` は前置の負号にもなる。
const OPERATORS: TokenSet = TokenSet::new(&[OP, CONOP, MINUS]);

/// atom を始められるトークン。`ERROR_TOKEN` も atom として読み、診断は出さない (字句解析で報告済み)。
const ATOM_START: TokenSet = TokenSet::new(&[
    INT, FLOAT, CHAR, STRING, MULTILINE_STRING, RAW_STRING, COMMAND, LIDENT, UIDENT, L_PAREN,
    L_BRACK, L_BRACE, ERROR_TOKEN,
]);

/// body ::= block(stmt) | expr
pub(super) fn body(p: &mut Parser) {
    if p.at(LAYOUT_OPEN) {
        let m = p.start();
        block_of(p, "a statement", stmt);
        m.complete(p, BLOCK);
        return;
    }
    if !expr(p) {
        p.error(
            codes::SYNTAX_ERROR,
            "expected an expression",
            format!("found {}", describe(p)),
        );
    }
}

/// stmt ::= 'let' pat (':' type)? '=' body | 'use' (pat '<-')? expr | expr
fn stmt(p: &mut Parser) -> bool {
    match p.current() {
        LET_KW => {
            let_stmt(p);
            true
        }
        _ => {
            let m = p.start();
            if expr(p) {
                m.complete(p, EXPR_STMT);
                true
            } else {
                m.abandon(p);
                false
            }
        }
    }
}

/// ブロックの `let`。後ろに `in` が続けば、1行の形の `let ... in` 式 (spec §7) として式文にする。
fn let_stmt(p: &mut Parser) {
    let m = p.start();
    let_head_and_body(p);
    if p.eat(IN_KW) {
        if !expr(p) {
            p.error(
                codes::SYNTAX_ERROR,
                "expected an expression",
                format!("found {}", describe(p)),
            );
        }
        let let_expr = m.complete(p, LET_EXPR);
        let_expr.precede(p).complete(p, EXPR_STMT);
    } else {
        m.complete(p, LET_STMT);
    }
}

/// `let pat (: type)? = body` の部分。
fn let_head_and_body(p: &mut Parser) {
    p.bump(LET_KW);
    if !patterns::pattern(p) {
        p.error(
            codes::SYNTAX_ERROR,
            "expected a pattern",
            format!("found {}", describe(p)),
        );
    }
    if p.eat(COLON) {
        types::type_(p);
    }
    if expect(p, EQ) {
        body(p);
    }
}

/// expr。何も読めなければ、診断を出さずに偽を返す。
pub(super) fn expr(p: &mut Parser) -> bool {
    op_expr(p, false) != OpExpr::Nothing
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OpExpr {
    Nothing,
    Expr,
    /// `(a +)` の左セクション。被演算子と演算子は、呼び出し側のノードの直下に平たく残る。
    LeftSection,
}

/// op_expr ::= operand (OP operand)* 。CST では `OP_SEQ` に平たく並べる (spec §7)。
/// 前置の `-` も `OP_SEQ` の中のトークンとして置き、HIR が `negate` として組み直す。
/// 演算子が1つもなければ `OP_SEQ` を作らない。
/// `section` が真なら、括弧の直下の `(a +)` を検出して `LeftSection` を返す。
fn op_expr(p: &mut Parser, section: bool) -> OpExpr {
    let m = p.start();
    let mut operands = 0;
    let mut has_operator = false;
    loop {
        while p.at(MINUS) {
            p.bump(MINUS);
            has_operator = true;
        }
        if !operand(p) {
            if has_operator {
                p.error(
                    codes::SYNTAX_ERROR,
                    "expected an expression",
                    format!("found {}", describe(p)),
                );
            }
            break;
        }
        operands += 1;
        if !p.at_ts(OPERATORS) {
            break;
        }
        if section && operands == 1 && p.nth(1) == R_PAREN {
            p.bump_any();
            m.abandon(p);
            return OpExpr::LeftSection;
        }
        p.bump_any();
        has_operator = true;
    }
    if has_operator {
        m.complete(p, OP_SEQ);
        OpExpr::Expr
    } else {
        m.abandon(p);
        if operands > 0 {
            OpExpr::Expr
        } else {
            OpExpr::Nothing
        }
    }
}

/// operand ::= app | lambda (Task 6)
fn operand(p: &mut Parser) -> bool {
    if p.at_ts(ATOM_START) {
        app(p);
        true
    } else {
        false
    }
}

/// app ::= postfix+ 。引数が1つ以上あれば `APP_EXPR` にする。
fn app(p: &mut Parser) {
    let m = p.start();
    postfix(p);
    let mut args = 0;
    while p.at_ts(ATOM_START) {
        postfix(p);
        args += 1;
    }
    if args > 0 {
        m.complete(p, APP_EXPR);
    } else {
        m.abandon(p);
    }
}

/// postfix ::= atom ('.' (LIDENT | INT))*
fn postfix(p: &mut Parser) -> bool {
    let Some(mut lhs) = atom(p) else {
        return false;
    };
    while p.at(DOT) && matches!(p.nth(1), LIDENT | INT) {
        let m = lhs.precede(p);
        dot(p);
        p.bump_any();
        lhs = m.complete(p, FIELD_EXPR);
    }
    true
}

fn atom(p: &mut Parser) -> Option<CompletedMarker> {
    let m = p.start();
    let kind = match p.current() {
        INT | STRING => {
            p.bump_any();
            LITERAL
        }
        kind @ (FLOAT | CHAR | MULTILINE_STRING | RAW_STRING | COMMAND) => {
            not_yet_supported(p, unsupported_literal_message(kind));
            p.bump_any();
            LITERAL
        }
        LIDENT | UIDENT => {
            qname(p);
            PATH_EXPR
        }
        L_PAREN => paren_expr(p),
        L_BRACK => {
            unsupported_group(p, "lists are not supported yet");
            ERROR
        }
        L_BRACE => {
            unsupported_group(p, "records are not supported yet");
            ERROR
        }
        ERROR_TOKEN => {
            p.bump_any();
            ERROR
        }
        _ => {
            m.abandon(p);
            return None;
        }
    };
    Some(m.complete(p, kind))
}

fn unsupported_literal_message(kind: SyntaxKind) -> &'static str {
    match kind {
        FLOAT => "floating-point literals are not supported yet",
        CHAR => "character literals are not supported yet",
        MULTILINE_STRING => "multi-line strings are not supported yet",
        RAW_STRING => "raw strings are not supported yet",
        _ => "command literals are not supported yet",
    }
}

/// qvar | qcon ::= (UIDENT '.')* (LIDENT | UIDENT)
fn qname(p: &mut Parser) {
    while p.at(UIDENT) && p.nth(1) == DOT && matches!(p.nth(2), UIDENT | LIDENT) {
        p.bump(UIDENT);
        dot(p);
    }
    p.bump_any();
}

/// `(` で始まる atom。単位、括弧、タプル、型の明示、演算子の参照、セクション。
fn paren_expr(p: &mut Parser) -> SyntaxKind {
    p.bump(L_PAREN);
    if p.eat(R_PAREN) {
        return UNIT_EXPR;
    }
    if p.at_ts(OPERATORS) && p.nth(1) == R_PAREN {
        p.bump_any();
        p.bump(R_PAREN);
        return OP_REF;
    }
    if p.at(OP) || p.at(CONOP) {
        // 右セクション。`(- 1)` は負の数なので、ここには来ない (spec §7)。
        p.bump_any();
        while p.at(MINUS) {
            p.bump(MINUS);
        }
        if !operand(p) {
            p.error(
                codes::SYNTAX_ERROR,
                "expected an expression",
                format!("found {}", describe(p)),
            );
        }
        expect(p, R_PAREN);
        return RIGHT_SECTION;
    }
    if p.at(DOT) && p.nth(1) == LIDENT {
        // `(.name)`。`.` と名前の間だけを詰める。
        if !p.touches_next() {
            p.error(
                codes::SPACE_AROUND_DOT,
                "unexpected whitespace around `.`",
                "write `.name` without spaces",
            );
        }
        p.bump(DOT);
        p.bump(LIDENT);
        expect(p, R_PAREN);
        return FIELD_SECTION;
    }
    match op_expr(p, true) {
        OpExpr::LeftSection => {
            expect(p, R_PAREN);
            return LEFT_SECTION;
        }
        OpExpr::Nothing => p.error(
            codes::SYNTAX_ERROR,
            "expected an expression",
            format!("found {}", describe(p)),
        ),
        OpExpr::Expr => {}
    }
    if p.eat(COLON) {
        types::type_(p);
        expect(p, R_PAREN);
        return ANNOT_EXPR;
    }
    if p.at(COMMA) {
        let mut count = 1;
        while p.eat(COMMA) {
            if p.at(R_PAREN) && count >= 2 {
                break;
            }
            if !expr(p) {
                p.error(
                    codes::SYNTAX_ERROR,
                    "expected an expression",
                    format!("found {}", describe(p)),
                );
                break;
            }
            count += 1;
        }
        expect(p, R_PAREN);
        return TUPLE_EXPR;
    }
    expect(p, R_PAREN);
    PAREN_EXPR
}
```

`crates/eml_syntax/src/grammar/mod.rs` の `mod items;` の前に `mod expressions;`、後に `mod patterns;` を足す (アルファベット順に `expressions` / `items` / `patterns` / `types`)。

- [ ] **Step 5: 等式を項目として読む**

`crates/eml_syntax/src/grammar/items.rs` の `at_item_start` を次のものに置き換え、補助の関数を足す。

```rust
pub(super) fn at_item_start(p: &Parser) -> bool {
    p.at_ts(ITEM_KEYWORDS)
        || (p.at(LIDENT) && p.nth(1) == COLON)
        || at_operator_signature(p)
        || at_equation(p)
        || at_operator_equation(p)
}

/// equation ::= LIDENT apat* '=' body 。`LIDENT OP` で始まる行は演算子の定義 (spec §5)。
fn at_equation(p: &Parser) -> bool {
    p.at(LIDENT)
        && (p.nth(1) == EQ || (p.nth(1) != MINUS && patterns::at_apat_start_at(p, 1)))
}

/// equation ::= apat OP apat '=' body 。apat の後ろが `=` や `::` なら、パターンによる束縛 (エラー)。
fn at_operator_equation(p: &Parser) -> bool {
    patterns::apat_len(p).is_some_and(|len| matches!(p.nth(len), OP | MINUS | CONOP | EQ))
}
```

`item` の `match` の、`_ if at_operator_signature(p) => signature(p, m),` の次に足す。

```rust
        _ if at_equation(p) => equation(p, m),
        _ if at_operator_equation(p) => operator_equation(p, m),
```

ファイルの末尾に足す。

```rust
/// equation ::= LIDENT apat* '=' body
fn equation(p: &mut Parser, m: Marker) {
    p.bump(LIDENT);
    while patterns::at_apat_start(p) {
        patterns::apat(p);
    }
    if expect(p, EQ) {
        expressions::body(p);
    }
    m.complete(p, EQUATION);
}

/// equation ::= apat OP apat '=' body (演算子の定義)。トップレベルのパターンによる束縛はエラー (spec §5)。
fn operator_equation(p: &mut Parser, m: Marker) {
    patterns::apat(p);
    if p.at(OP) || p.at(MINUS) {
        p.bump_any();
        if !patterns::apat(p) {
            p.error(
                codes::SYNTAX_ERROR,
                "expected a pattern",
                format!("found {}", describe(p)),
            );
        }
        if expect(p, EQ) {
            expressions::body(p);
        }
        m.complete(p, EQUATION);
    } else {
        p.error(
            codes::SYNTAX_ERROR,
            "top-level pattern bindings are not allowed",
            "define a value by name instead, as in `x = ...`",
        );
        skip_to_sep(p, false);
        m.complete(p, ERROR);
    }
}
```

- [ ] **Step 6: テストが通ることを確認する**

Run: `cargo test -p eml_syntax`
Expected: PASS (すべて。Task 4 の `lone_lowercase_name_is_not_an_item` も E0003 のまま)

- [ ] **Step 7: 全体のテストと lint を通す**

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check`
Expected: すべて PASS

- [ ] **Step 8: Commit**

```bash
git add crates/eml_syntax
git commit -m "Parse equations, patterns, and basic expressions"
```

### Task 6: `if`、`match`、ラムダ、`let ... in`、`use`

**Files:**
- Modify: `crates/eml_syntax/src/grammar/expressions.rs`
- Modify: `crates/eml_syntax/src/lib.rs` (`codes` に E0012 を足す)
- Create: `crates/eml_syntax/tests/control.rs`

**Interfaces:**
- Consumes: Task 5 の `expressions` の内部 (`expr`、`op_expr`、`operand`、`app`、`paren_expr`、`stmt`、`let_head_and_body`、`body`)、`patterns::{pattern, param, at_apat_start}`、`block_of`
- Produces:
  - `codes::NEEDS_PARENS` (E0012)
  - `expressions` の内部の `branches(p, expected: &str, branch: fn(&mut Parser) -> bool)` (`with` の後ろの枝の並び。Task 7 の handler も使う)、`NEEDS_PARENS: TokenSet` (Task 7 で `HANDLE_KW` を足す)
  - ノード: `IF_EXPR` (`IF_KW` 条件 `THEN_KW` 本体 [`ELSE_KW` 本体])、`MATCH_EXPR` (`MATCH_KW` 式 `WITH_KW` `MATCH_ARM`*)、`MATCH_ARM` (`PIPE` パターン `THIN_ARROW` 本体)、`LAMBDA_EXPR` (`FN_KW` パターン+ `THIN_ARROW` 本体)、`LET_EXPR`、`USE_STMT` (`USE_KW` [パターン `LEFT_ARROW`] 式)

`if` / `match` / `let ... in` は atom ではない (spec §5)。関数の引数や演算の項の位置に書くと E0012 を出し、そのうえで式として読む (回復のため)。`fn` は演算の項と、最後の引数の位置に書ける。

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_syntax/tests/control.rs`:

```rust
mod common;

use common::{diagnostics, lines, shape};

#[test]
fn if_with_else_on_separate_lines() {
    let text = lines(&["f c =", "  if c then", "    a", "  else", "    b"]);
    insta::assert_snapshot!(shape(&text), @r#"
    SOURCE_FILE
      EQUATION
        LIDENT "f"
        BIND_PAT
          LIDENT "c"
        EQ "="
        BLOCK
          EXPR_STMT
            IF_EXPR
              IF_KW "if"
              PATH_EXPR
                LIDENT "c"
              THEN_KW "then"
              BLOCK
                EXPR_STMT
                  PATH_EXPR
                    LIDENT "a"
              ELSE_KW "else"
              BLOCK
                EXPR_STMT
                  PATH_EXPR
                    LIDENT "b"
    "#);
}

#[test]
fn if_without_else() {
    insta::assert_snapshot!(shape("f c = if c then g x"), @r#"
    SOURCE_FILE
      EQUATION
        LIDENT "f"
        BIND_PAT
          LIDENT "c"
        EQ "="
        IF_EXPR
          IF_KW "if"
          PATH_EXPR
            LIDENT "c"
          THEN_KW "then"
          APP_EXPR
            PATH_EXPR
              LIDENT "g"
            PATH_EXPR
              LIDENT "x"
    "#);
}

#[test]
fn else_if_chain_on_one_line() {
    insta::assert_snapshot!(shape("f = if a then x else if b then y else z"), @r#"
    SOURCE_FILE
      EQUATION
        LIDENT "f"
        EQ "="
        IF_EXPR
          IF_KW "if"
          PATH_EXPR
            LIDENT "a"
          THEN_KW "then"
          PATH_EXPR
            LIDENT "x"
          ELSE_KW "else"
          IF_EXPR
            IF_KW "if"
            PATH_EXPR
              LIDENT "b"
            THEN_KW "then"
            PATH_EXPR
              LIDENT "y"
            ELSE_KW "else"
            PATH_EXPR
              LIDENT "z"
    "#);
}

#[test]
fn else_on_the_next_line_after_a_one_line_then() {
    let text = lines(&["main () =", "  if s then a", "  else b"]);
    insta::assert_snapshot!(shape(&text), @r#"
    SOURCE_FILE
      EQUATION
        LIDENT "main"
        UNIT_PAT
          L_PAREN "("
          R_PAREN ")"
        EQ "="
        BLOCK
          EXPR_STMT
            IF_EXPR
              IF_KW "if"
              PATH_EXPR
                LIDENT "s"
              THEN_KW "then"
              PATH_EXPR
                LIDENT "a"
              ELSE_KW "else"
              PATH_EXPR
                LIDENT "b"
    "#);
}

#[test]
fn missing_then_is_an_error() {
    assert_eq!(diagnostics("f = if c 1"), ["E0011 1:11 expected `then`"]);
}

#[test]
fn match_with_indented_arms() {
    let text = lines(&[
        "f b =",
        "  match b with",
        "    | True -> 1",
        "    | False ->",
        "        g x",
        "        0",
    ]);
    insta::assert_snapshot!(shape(&text), @r#"
    SOURCE_FILE
      EQUATION
        LIDENT "f"
        BIND_PAT
          LIDENT "b"
        EQ "="
        BLOCK
          EXPR_STMT
            MATCH_EXPR
              MATCH_KW "match"
              PATH_EXPR
                LIDENT "b"
              WITH_KW "with"
              MATCH_ARM
                PIPE "|"
                CON_PAT
                  UIDENT "True"
                THIN_ARROW "->"
                LITERAL
                  INT "1"
              MATCH_ARM
                PIPE "|"
                CON_PAT
                  UIDENT "False"
                THIN_ARROW "->"
                BLOCK
                  EXPR_STMT
                    APP_EXPR
                      PATH_EXPR
                        LIDENT "g"
                      PATH_EXPR
                        LIDENT "x"
                  EXPR_STMT
                    LITERAL
                      INT "0"
    "#);
}

#[test]
fn match_on_one_line() {
    insta::assert_snapshot!(shape("g b = match b with | True -> 1 | False -> 0"), @r#"
    SOURCE_FILE
      EQUATION
        LIDENT "g"
        BIND_PAT
          LIDENT "b"
        EQ "="
        MATCH_EXPR
          MATCH_KW "match"
          PATH_EXPR
            LIDENT "b"
          WITH_KW "with"
          MATCH_ARM
            PIPE "|"
            CON_PAT
              UIDENT "True"
            THIN_ARROW "->"
            LITERAL
              INT "1"
          MATCH_ARM
            PIPE "|"
            CON_PAT
              UIDENT "False"
            THIN_ARROW "->"
            LITERAL
              INT "0"
    "#);
}

#[test]
fn later_arms_on_one_line_belong_to_the_inner_match() {
    insta::assert_snapshot!(shape("h = match a with | X -> match b with | Y -> 1 | Z -> 2"), @r#"
    SOURCE_FILE
      EQUATION
        LIDENT "h"
        EQ "="
        MATCH_EXPR
          MATCH_KW "match"
          PATH_EXPR
            LIDENT "a"
          WITH_KW "with"
          MATCH_ARM
            PIPE "|"
            CON_PAT
              UIDENT "X"
            THIN_ARROW "->"
            MATCH_EXPR
              MATCH_KW "match"
              PATH_EXPR
                LIDENT "b"
              WITH_KW "with"
              MATCH_ARM
                PIPE "|"
                CON_PAT
                  UIDENT "Y"
                THIN_ARROW "->"
                LITERAL
                  INT "1"
              MATCH_ARM
                PIPE "|"
                CON_PAT
                  UIDENT "Z"
                THIN_ARROW "->"
                LITERAL
                  INT "2"
    "#);
}

#[test]
fn arms_at_the_column_of_match_need_indentation() {
    assert_eq!(
        diagnostics(&lines(&["f b =", "  match b with", "  | True -> 1"])),
        [
            "E0009 2:11 expected an indented block after `with`",
            "E0011 3:3 expected a statement",
        ]
    );
}

#[test]
fn arm_without_pipe_is_an_error() {
    assert_eq!(
        diagnostics("f b = match b with True -> 1"),
        ["E0011 1:20 expected an arm starting with `|`"]
    );
    assert_eq!(
        diagnostics(&lines(&["f b =", "  match b with", "    True -> 1"])),
        ["E0011 3:5 expected `|` before the arm"]
    );
}

#[test]
fn trailing_lambda_with_a_block_body() {
    let text = lines(&["f = each items fn item ->", "  println item"]);
    insta::assert_snapshot!(shape(&text), @r#"
    SOURCE_FILE
      EQUATION
        LIDENT "f"
        EQ "="
        APP_EXPR
          PATH_EXPR
            LIDENT "each"
          PATH_EXPR
            LIDENT "items"
          LAMBDA_EXPR
            FN_KW "fn"
            BIND_PAT
              LIDENT "item"
            THIN_ARROW "->"
            BLOCK
              EXPR_STMT
                APP_EXPR
                  PATH_EXPR
                    LIDENT "println"
                  PATH_EXPR
                    LIDENT "item"
    "#);
}

#[test]
fn lambda_parameters() {
    insta::assert_snapshot!(shape("g = map (fn (x : Int) (a, b) -> x) xs"), @r#"
    SOURCE_FILE
      EQUATION
        LIDENT "g"
        EQ "="
        APP_EXPR
          PATH_EXPR
            LIDENT "map"
          PAREN_EXPR
            L_PAREN "("
            LAMBDA_EXPR
              FN_KW "fn"
              ANNOT_PAT
                L_PAREN "("
                BIND_PAT
                  LIDENT "x"
                COLON ":"
                PATH_TYPE
                  UIDENT "Int"
                R_PAREN ")"
              TUPLE_PAT
                L_PAREN "("
                BIND_PAT
                  LIDENT "a"
                COMMA ","
                BIND_PAT
                  LIDENT "b"
                R_PAREN ")"
              THIN_ARROW "->"
              PATH_EXPR
                LIDENT "x"
            R_PAREN ")"
          PATH_EXPR
            LIDENT "xs"
    "#);
}

#[test]
fn lambda_as_an_operand() {
    insta::assert_snapshot!(shape("h = xs |> each fn l -> println l"), @r#"
    SOURCE_FILE
      EQUATION
        LIDENT "h"
        EQ "="
        OP_SEQ
          PATH_EXPR
            LIDENT "xs"
          OP "|>"
          APP_EXPR
            PATH_EXPR
              LIDENT "each"
            LAMBDA_EXPR
              FN_KW "fn"
              BIND_PAT
                LIDENT "l"
              THIN_ARROW "->"
              APP_EXPR
                PATH_EXPR
                  LIDENT "println"
                PATH_EXPR
                  LIDENT "l"
    "#);
}

#[test]
fn lambda_needs_a_parameter() {
    assert_eq!(diagnostics("f = fn -> 1"), ["E0011 1:8 expected a parameter"]);
}

#[test]
fn let_in_inside_parentheses() {
    insta::assert_snapshot!(shape("f = (let x = 1 in x)"), @r#"
    SOURCE_FILE
      EQUATION
        LIDENT "f"
        EQ "="
        PAREN_EXPR
          L_PAREN "("
          LET_EXPR
            LET_KW "let"
            BIND_PAT
              LIDENT "x"
            EQ "="
            LITERAL
              INT "1"
            IN_KW "in"
            PATH_EXPR
              LIDENT "x"
          R_PAREN ")"
    "#);
}

#[test]
fn use_statements() {
    let text = lines(&[
        "main () =",
        "  use with_env",
        "  use tmp <- with_temp_dir",
        "  build tmp",
    ]);
    insta::assert_snapshot!(shape(&text), @r#"
    SOURCE_FILE
      EQUATION
        LIDENT "main"
        UNIT_PAT
          L_PAREN "("
          R_PAREN ")"
        EQ "="
        BLOCK
          USE_STMT
            USE_KW "use"
            PATH_EXPR
              LIDENT "with_env"
          USE_STMT
            USE_KW "use"
            BIND_PAT
              LIDENT "tmp"
            LEFT_ARROW "<-"
            PATH_EXPR
              LIDENT "with_temp_dir"
          EXPR_STMT
            APP_EXPR
              PATH_EXPR
                LIDENT "build"
              PATH_EXPR
                LIDENT "tmp"
    "#);
}

#[test]
fn control_expressions_must_be_parenthesized_as_arguments_and_operands() {
    assert_eq!(
        diagnostics("f = g match x with | A -> 1"),
        ["E0012 1:7 `match` expression must be parenthesized here"]
    );
    assert_eq!(
        diagnostics("x = 1 + if c then 2 else 3"),
        ["E0012 1:9 `if` expression must be parenthesized here"]
    );
}
```

- [ ] **Step 2: テストが失敗することを確認する**

Run: `cargo test -p eml_syntax --test control`
Expected: FAIL

- [ ] **Step 3: 番号を足す**

`crates/eml_syntax/src/lib.rs` の `codes` に足す。

```rust
    pub const NEEDS_PARENS: ErrorCode = ErrorCode(12);
```

- [ ] **Step 4: 式の文法を拡張する**

`crates/eml_syntax/src/grammar/expressions.rs` を次のように変える。

1. `ATOM_START` の定義の後に足す。

```rust
/// atom ではないので、引数や演算の項の位置では括弧が要る式 (spec §5)。
const NEEDS_PARENS: TokenSet = TokenSet::new(&[IF_KW, MATCH_KW, LET_KW]);
```

2. `stmt` の `match` に、`LET_KW` の腕の次に足す。

```rust
        USE_KW => {
            use_stmt(p);
            true
        }
```

3. `expr` を置き換える。

```rust
/// expr。何も読めなければ、診断を出さずに偽を返す。
pub(super) fn expr(p: &mut Parser) -> bool {
    match p.current() {
        IF_KW => if_expr(p),
        MATCH_KW => match_expr(p),
        LET_KW => let_expr(p),
        _ => return op_expr(p, false) != OpExpr::Nothing,
    }
    true
}
```

4. `operand` を置き換える。

```rust
/// operand ::= app | lambda 。括弧の要る式は E0012 を出して読む。
fn operand(p: &mut Parser) -> bool {
    if p.at_ts(ATOM_START) {
        app(p);
    } else if p.at(FN_KW) {
        lambda(p);
    } else if p.at_ts(NEEDS_PARENS) {
        needs_parens(p);
    } else {
        return false;
    }
    true
}
```

5. `app` を置き換える。

```rust
/// app ::= postfix+ lambda? 。引数が1つ以上あれば `APP_EXPR` にする。
fn app(p: &mut Parser) {
    let m = p.start();
    postfix(p);
    let mut args = 0;
    loop {
        if p.at_ts(ATOM_START) {
            postfix(p);
            args += 1;
        } else if p.at(FN_KW) {
            // 最後の引数のラムダは、括弧なしで書ける (spec §7)。
            lambda(p);
            args += 1;
            break;
        } else if p.at_ts(NEEDS_PARENS) {
            needs_parens(p);
            args += 1;
            break;
        } else {
            break;
        }
    }
    if args > 0 {
        m.complete(p, APP_EXPR);
    } else {
        m.abandon(p);
    }
}
```

6. `paren_expr` の `match op_expr(p, true) {` を、次のものに置き換える (腕は変えない)。

```rust
    let inner = if p.at_ts(NEEDS_PARENS) {
        expr(p);
        OpExpr::Expr
    } else {
        op_expr(p, true)
    };
    match inner {
```

7. ファイルの末尾に足す。

```rust
/// 括弧の要る式を、引数や演算の項の位置で見つけた。E0012 を出し、回復のためにそのまま式として読む。
fn needs_parens(p: &mut Parser) {
    p.error(
        codes::NEEDS_PARENS,
        format!("`{}` expression must be parenthesized here", p.current_text()),
        "wrap it in parentheses",
    );
    expr(p);
}

/// 'if' expr 'then' body ('else' body)?
fn if_expr(p: &mut Parser) {
    let m = p.start();
    p.bump(IF_KW);
    if !expr(p) {
        p.error(
            codes::SYNTAX_ERROR,
            "expected an expression",
            format!("found {}", describe(p)),
        );
    }
    skip_sep_before(p, THEN_KW);
    if expect(p, THEN_KW) {
        body(p);
        skip_sep_before(p, ELSE_KW);
        if p.eat(ELSE_KW) {
            body(p);
        }
    }
    m.complete(p, IF_EXPR);
}

/// `if` と同じ列に書いた `then` / `else` の前の `SEP` を1つ読み飛ばす (spec §4、Haskell の DoAndIfThenElse)。
fn skip_sep_before(p: &mut Parser, kind: SyntaxKind) {
    if p.at_sep() && p.nth(1) == kind {
        p.bump_any();
    }
}

/// 'match' expr 'with' arms
fn match_expr(p: &mut Parser) {
    let m = p.start();
    p.bump(MATCH_KW);
    if !expr(p) {
        p.error(
            codes::SYNTAX_ERROR,
            "expected an expression",
            format!("found {}", describe(p)),
        );
    }
    if expect(p, WITH_KW) {
        branches(p, "an arm starting with `|`", match_arm);
    }
    m.complete(p, MATCH_EXPR);
}

/// `with` の後ろの枝の並び。字下げしたブロックか、同じ行に並べた枝 (spec §7)。
/// 同じ行の形では、枝の本体は次の `|` の手前で終わる (`|` 単独は演算子ではないため)。
fn branches(p: &mut Parser, expected: &str, branch: fn(&mut Parser) -> bool) {
    if p.at(LAYOUT_OPEN) {
        block_of(p, expected, branch);
        return;
    }
    if !p.at(PIPE) {
        p.error(
            codes::SYNTAX_ERROR,
            format!("expected {expected}"),
            format!("found {}", describe(p)),
        );
        return;
    }
    while p.at(PIPE) {
        branch(p);
    }
}

/// arm ::= '|' pat '->' body 。ブロックの中で `|` を書き忘れた枝も、診断したうえで読む。
fn match_arm(p: &mut Parser) -> bool {
    if !p.at(PIPE) && !patterns::at_apat_start(p) {
        return false;
    }
    let m = p.start();
    if !p.eat(PIPE) {
        p.error(
            codes::SYNTAX_ERROR,
            "expected `|` before the arm",
            "each arm starts with `|`",
        );
    }
    if !patterns::pattern(p) {
        p.error(
            codes::SYNTAX_ERROR,
            "expected a pattern",
            format!("found {}", describe(p)),
        );
    }
    if expect(p, THIN_ARROW) {
        body(p);
    }
    m.complete(p, MATCH_ARM);
    true
}

/// lambda ::= 'fn' param+ '->' body
fn lambda(p: &mut Parser) {
    let m = p.start();
    p.bump(FN_KW);
    let mut params = 0;
    while patterns::at_apat_start(p) {
        patterns::param(p);
        params += 1;
    }
    if params == 0 {
        p.error(
            codes::SYNTAX_ERROR,
            "expected a parameter",
            format!("found {}", describe(p)),
        );
    }
    if expect(p, THIN_ARROW) {
        body(p);
    }
    m.complete(p, LAMBDA_EXPR);
}

/// 式の位置の 'let' pat (':' type)? '=' expr 'in' expr
fn let_expr(p: &mut Parser) {
    let m = p.start();
    let_head_and_body(p);
    if expect(p, IN_KW) && !expr(p) {
        p.error(
            codes::SYNTAX_ERROR,
            "expected an expression",
            format!("found {}", describe(p)),
        );
    }
    m.complete(p, LET_EXPR);
}

/// 'use' (pat '<-')? expr 。脱糖は HIR で行う (spec §7)。
fn use_stmt(p: &mut Parser) {
    let m = p.start();
    p.bump(USE_KW);
    if has_left_arrow(p) {
        if !patterns::pattern(p) {
            p.error(
                codes::SYNTAX_ERROR,
                "expected a pattern",
                format!("found {}", describe(p)),
            );
        }
        expect(p, LEFT_ARROW);
    }
    if !expr(p) {
        p.error(
            codes::SYNTAX_ERROR,
            "expected an expression",
            format!("found {}", describe(p)),
        );
    }
    m.complete(p, USE_STMT);
}

/// 文の終わりまでに、括弧とブロックの外の `<-` があるか (`use p <- e` の形か)。
fn has_left_arrow(p: &Parser) -> bool {
    let mut depth = 0u32;
    let mut n = 0;
    loop {
        match p.nth(n) {
            LEFT_ARROW if depth == 0 => return true,
            L_PAREN | L_BRACK | L_BRACE | LAYOUT_OPEN => depth += 1,
            R_PAREN | R_BRACK | R_BRACE | LAYOUT_CLOSE => {
                if depth == 0 {
                    return false;
                }
                depth -= 1;
            }
            LAYOUT_SEP | SEMICOLON if depth == 0 => return false,
            EOF => return false,
            _ => {}
        }
        n += 1;
    }
}
```

- [ ] **Step 5: テストが通ることを確認する**

Run: `cargo test -p eml_syntax`
Expected: PASS (すべて)

- [ ] **Step 6: 全体のテストと lint を通す**

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check`
Expected: すべて PASS

- [ ] **Step 7: Commit**

```bash
git add crates/eml_syntax
git commit -m "Parse if, match, lambdas, let-in, and use"
```

### Task 7: handler、`resume`、`drop`

**Files:**
- Modify: `crates/eml_syntax/src/grammar/expressions.rs`
- Create: `crates/eml_syntax/tests/handlers.rs`

**Interfaces:**
- Consumes: Task 6 の `branches`、`NEEDS_PARENS`、`app`、`operand`、`expr`
- Produces: ノード `HANDLE_EXPR` (`HANDLE_KW` 式 [`FROM_KW` 式] `WITH_KW` 節*)、`OP_CLAUSE` (`PIPE` `LIDENT` パターン* `THIN_ARROW` 本体)、`RETURN_CLAUSE` (`PIPE` `RETURN_KW` パターン+ `THIN_ARROW` 本体)、`RESUME_EXPR` / `DROP_EXPR` (キーワード、続く postfix と最後の引数のラムダ)。節の引数の個数と `resume` の引数の個数は HIR で検査する (spec §5、§7)

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_syntax/tests/handlers.rs`:

```rust
mod common;

use common::{diagnostics, lines, shape};

#[test]
fn handler_with_operation_and_return_clauses() {
    let text = lines(&[
        "try action =",
        "  handle action () with",
        "    | fail _ -> None",
        "    | return x -> Some x",
    ]);
    insta::assert_snapshot!(shape(&text), @r#"
    SOURCE_FILE
      EQUATION
        LIDENT "try"
        BIND_PAT
          LIDENT "action"
        EQ "="
        BLOCK
          EXPR_STMT
            HANDLE_EXPR
              HANDLE_KW "handle"
              APP_EXPR
                PATH_EXPR
                  LIDENT "action"
                UNIT_EXPR
                  L_PAREN "("
                  R_PAREN ")"
              WITH_KW "with"
              OP_CLAUSE
                PIPE "|"
                LIDENT "fail"
                WILDCARD_PAT
                  UNDERSCORE "_"
                THIN_ARROW "->"
                PATH_EXPR
                  UIDENT "None"
              RETURN_CLAUSE
                PIPE "|"
                RETURN_KW "return"
                BIND_PAT
                  LIDENT "x"
                THIN_ARROW "->"
                APP_EXPR
                  PATH_EXPR
                    UIDENT "Some"
                  PATH_EXPR
                    LIDENT "x"
    "#);
}

#[test]
fn parameterized_handler() {
    let text = lines(&[
        "run_state init action =",
        "  handle action () from init with",
        "    | get () k st -> resume k st st",
        "    | put st2 k _ -> resume k () st2",
        "    | return x st -> (x, st)",
    ]);
    insta::assert_snapshot!(shape(&text), @r#"
    SOURCE_FILE
      EQUATION
        LIDENT "run_state"
        BIND_PAT
          LIDENT "init"
        BIND_PAT
          LIDENT "action"
        EQ "="
        BLOCK
          EXPR_STMT
            HANDLE_EXPR
              HANDLE_KW "handle"
              APP_EXPR
                PATH_EXPR
                  LIDENT "action"
                UNIT_EXPR
                  L_PAREN "("
                  R_PAREN ")"
              FROM_KW "from"
              PATH_EXPR
                LIDENT "init"
              WITH_KW "with"
              OP_CLAUSE
                PIPE "|"
                LIDENT "get"
                UNIT_PAT
                  L_PAREN "("
                  R_PAREN ")"
                BIND_PAT
                  LIDENT "k"
                BIND_PAT
                  LIDENT "st"
                THIN_ARROW "->"
                RESUME_EXPR
                  RESUME_KW "resume"
                  PATH_EXPR
                    LIDENT "k"
                  PATH_EXPR
                    LIDENT "st"
                  PATH_EXPR
                    LIDENT "st"
              OP_CLAUSE
                PIPE "|"
                LIDENT "put"
                BIND_PAT
                  LIDENT "st2"
                BIND_PAT
                  LIDENT "k"
                WILDCARD_PAT
                  UNDERSCORE "_"
                THIN_ARROW "->"
                RESUME_EXPR
                  RESUME_KW "resume"
                  PATH_EXPR
                    LIDENT "k"
                  UNIT_EXPR
                    L_PAREN "("
                    R_PAREN ")"
                  PATH_EXPR
                    LIDENT "st2"
              RETURN_CLAUSE
                PIPE "|"
                RETURN_KW "return"
                BIND_PAT
                  LIDENT "x"
                BIND_PAT
                  LIDENT "st"
                THIN_ARROW "->"
                TUPLE_EXPR
                  L_PAREN "("
                  PATH_EXPR
                    LIDENT "x"
                  COMMA ","
                  PATH_EXPR
                    LIDENT "st"
                  R_PAREN ")"
    "#);
}

#[test]
fn handler_on_one_line_with_drop() {
    insta::assert_snapshot!(shape("h = handle f () with | ask key k -> drop k"), @r#"
    SOURCE_FILE
      EQUATION
        LIDENT "h"
        EQ "="
        HANDLE_EXPR
          HANDLE_KW "handle"
          APP_EXPR
            PATH_EXPR
              LIDENT "f"
            UNIT_EXPR
              L_PAREN "("
              R_PAREN ")"
          WITH_KW "with"
          OP_CLAUSE
            PIPE "|"
            LIDENT "ask"
            BIND_PAT
              LIDENT "key"
            BIND_PAT
              LIDENT "k"
            THIN_ARROW "->"
            DROP_EXPR
              DROP_KW "drop"
              PATH_EXPR
                LIDENT "k"
    "#);
}

#[test]
fn resume_is_an_operand() {
    insta::assert_snapshot!(shape("y = resume k 1 + 2"), @r#"
    SOURCE_FILE
      EQUATION
        LIDENT "y"
        EQ "="
        OP_SEQ
          RESUME_EXPR
            RESUME_KW "resume"
            PATH_EXPR
              LIDENT "k"
            LITERAL
              INT "1"
          OP "+"
          LITERAL
            INT "2"
    "#);
}

#[test]
fn malformed_clauses_are_errors() {
    assert_eq!(
        diagnostics("h = handle f () with | return -> 1"),
        ["E0011 1:31 expected a pattern"]
    );
    assert_eq!(
        diagnostics("h = handle f () with | 1 -> 2"),
        ["E0011 1:24 expected a lowercase name"]
    );
}

#[test]
fn drop_needs_an_argument() {
    assert_eq!(diagnostics("h = drop"), ["E0011 1:9 expected an expression"]);
}

#[test]
fn handle_must_be_parenthesized_as_an_argument() {
    assert_eq!(
        diagnostics("f = g handle x with | return y -> y"),
        ["E0012 1:7 `handle` expression must be parenthesized here"]
    );
}
```

- [ ] **Step 2: テストが失敗することを確認する**

Run: `cargo test -p eml_syntax --test handlers`
Expected: FAIL

- [ ] **Step 3: 式の文法を拡張する**

`crates/eml_syntax/src/grammar/expressions.rs` を次のように変える。

1. `NEEDS_PARENS` に `HANDLE_KW` を足す: `TokenSet::new(&[IF_KW, MATCH_KW, HANDLE_KW, LET_KW])`
2. `expr` の `match` に `HANDLE_KW => handle_expr(p),` を足す (`MATCH_KW` の腕の次)。
3. `operand` の最初の条件を `if p.at_ts(ATOM_START) || p.at(RESUME_KW) || p.at(DROP_KW) {` にする。
4. `app` を置き換える。

```rust
/// app ::= ('resume' | 'drop')? postfix+ lambda?
/// `resume` / `drop` があればそのノードに、なければ引数が1つ以上あるときだけ `APP_EXPR` にする。
fn app(p: &mut Parser) {
    let m = p.start();
    let keyword = match p.current() {
        RESUME_KW => Some(RESUME_EXPR),
        DROP_KW => Some(DROP_EXPR),
        _ => None,
    };
    if keyword.is_some() {
        p.bump_any();
        if !postfix(p) {
            p.error(
                codes::SYNTAX_ERROR,
                "expected an expression",
                format!("found {}", describe(p)),
            );
        }
    } else {
        postfix(p);
    }
    let mut args = 0;
    loop {
        if p.at_ts(ATOM_START) {
            postfix(p);
            args += 1;
        } else if p.at(FN_KW) {
            // 最後の引数のラムダは、括弧なしで書ける (spec §7)。
            lambda(p);
            args += 1;
            break;
        } else if p.at_ts(NEEDS_PARENS) {
            needs_parens(p);
            args += 1;
            break;
        } else {
            break;
        }
    }
    match keyword {
        Some(kind) => {
            m.complete(p, kind);
        }
        None if args > 0 => {
            m.complete(p, APP_EXPR);
        }
        None => m.abandon(p),
    }
}
```

5. ファイルの末尾に足す。

```rust
/// 'handle' expr ('from' expr)? 'with' clauses
fn handle_expr(p: &mut Parser) {
    let m = p.start();
    p.bump(HANDLE_KW);
    if !expr(p) {
        p.error(
            codes::SYNTAX_ERROR,
            "expected an expression",
            format!("found {}", describe(p)),
        );
    }
    if p.eat(FROM_KW) && !expr(p) {
        p.error(
            codes::SYNTAX_ERROR,
            "expected the initial state",
            format!("found {}", describe(p)),
        );
    }
    if expect(p, WITH_KW) {
        branches(p, "a clause starting with `|`", handler_clause);
    }
    m.complete(p, HANDLE_EXPR);
}

/// clause ::= '|' LIDENT apat* '->' body | '|' 'return' apat+ '->' body
fn handler_clause(p: &mut Parser) -> bool {
    if !p.at(PIPE) {
        return false;
    }
    let m = p.start();
    p.bump(PIPE);
    let kind = if p.eat(RETURN_KW) {
        if !patterns::at_apat_start(p) {
            p.error(
                codes::SYNTAX_ERROR,
                "expected a pattern",
                format!("found {}", describe(p)),
            );
        }
        RETURN_CLAUSE
    } else {
        expect(p, LIDENT);
        OP_CLAUSE
    };
    while patterns::at_apat_start(p) {
        patterns::apat(p);
    }
    if expect(p, THIN_ARROW) {
        body(p);
    }
    m.complete(p, kind);
    true
}
```

- [ ] **Step 4: テストが通ることを確認する**

Run: `cargo test -p eml_syntax`
Expected: PASS (すべて)

- [ ] **Step 5: 全体のテストと lint を通す**

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check`
Expected: すべて PASS

- [ ] **Step 6: Commit**

```bash
git add crates/eml_syntax
git commit -m "Parse handlers, resume, and drop"
```

### Task 8: コーパスと、壊れた入力に対する頑健性

**Files:**
- Create: `crates/eml_syntax/tests/corpus/s1.em` (S1 の構文だけで書いたプログラム)
- Create: `crates/eml_syntax/tests/corpus/later_stages.em` (spec §12 の「ログの集計」の例をそのまま)
- Create: `crates/eml_syntax/tests/corpus.rs`

**Interfaces:**
- Consumes: `eml_syntax::parse`、`tests/common` の `diagnostics`
- Produces: `tests/corpus/s1.em` (Task 9 の AST のテストも使う)

このタスクは実装を足さない。テストが失敗した場合は、Task 4〜7 の文法の誤りなので、そのタスクの文法を直す (テストやコーパスを変えて通さない)。コーパスに書いた構文が spec §5 の文法に合っていないと判断した場合は、作業を止めて報告する。

- [ ] **Step 1: S1 のコーパスを書く**

`crates/eml_syntax/tests/corpus/s1.em`:

```
#!/usr/bin/env eml run
-- | S1 の構文をひととおり使うプログラム
{- ブロックコメントは {- 入れ子に -} できる -}

infixr 5 ::
infixl 6 +, -

data List a =
  | Nil
  | a :: List a

data Option a = | None | Some a

effect State s where
  get : Unit -> s
  put : s -> Unit

effect Fail where
  never fail : String -> a

effect Choose where
  multi choose : Unit -> Bool

len : List a -> Int
len Nil = 0
len (_ :: rest) = 1 + len rest

(<+>) : Int -> Int -> Int
a <+> b = a * 2 + b

try : (Unit -> <Fail | e> a) -> <e> Option a
try action =
  handle action () with
    | fail _ -> None
    | return x -> Some x

run_state : s -> (Unit -> <State s | e> a) -> <e> (a, s)
run_state init action =
  handle action () from init with
    | get () k st -> resume k st st
    | put st2 k _ -> resume k () st2
    | return x st -> (x, st)

counter : Unit -> <State Int> Int
counter () =
  let n = get ()
  put (n + 1)
  n

first : (Unit -> <Choose | e> a) -> <e> a
first action =
  handle action () with | choose () k -> resume k True

classify : Int -> String
classify n =
  match n with
    | 0 -> "zero"
    | -1 -> "minus one"
    | _ ->
        if n > 0 then "positive"
        else "negative"

main : Unit -> <IO> Unit
main () =
  let f = open "data.txt"
  let (f, text) = read_all f
  close f
  let total =
    lines text
      |> map parse_int
      |> fold (+) 0
  if total > 100 then
    println "big"
  else
    println (show_int (total : Int))
  each (Cons 1 Nil) fn x ->
    println (show_int (x <+> 1)) -- 行末のコメント
  let pair = (total, classify total)
  println pair.1
  let r = run_state 0 (fn () -> counter ())
  println (show_int (r.0 + negate 1))
  let inc = (+ 1)
  let half = (/ 2)
  let name_of = (.name)
  use tmp <- with_temp_dir
  println tmp
```

- [ ] **Step 2: 後の段階の構文を含むコーパスを作る**

spec §12 の「ログの集計 (全体)」のコードブロックの中身を、そのまま `crates/eml_syntax/tests/corpus/later_stages.em` にする。中身は次のとおり (spec の 688〜740 行目)。

```
#!/usr/bin/env eml run
-- | ログを集計して、レポートを書く
import Report.Format (render, Style(..))

data Level =
  | Info
  | Warn
  | Error

type Entry = { level : Level, msg : String }

effect Fail where
  never fail : String -> a

parse_entry : String -> Option Entry
parse_entry line =
  match String.split_once " " line with
    | Some ("INFO", msg)  -> Some { level = Info, msg }
    | Some ("WARN", msg)  -> Some { level = Warn, msg }
    | Some ("ERROR", msg) -> Some { level = Error, msg }
    | _ -> None

is_error : Level -> Bool
is_error Error = True
is_error _ = False

try : (Unit -> <Fail | e> a) -> <IO | e> Option a
try action =
  handle action () with
    | fail msg ->
        eprintln "error: \{msg}"
        None
    | return x -> Some x

summarize : String -> <IO, Fail> Unit
summarize path =
  let entries = Fs.read_lines path |> filter_map parse_entry
  let errors = entries |> filter (fn e -> is_error e.level) |> map (.msg)
  if length errors > 10 then fail "too many errors"
  let commit = read `git rev-parse --short HEAD`
  Fs.write_text "report.md" """
    # Report for \{commit}
    \{render Markdown errors}
    """

main : Unit -> <IO> Unit
main () =
  match Env.args () with
    | [path] ->
        match try (fn () -> summarize path) with
          | Some () -> println "done"
          | None -> exit 1
    | _ -> eprintln "usage: summarize LOG"
```

- [ ] **Step 3: テストを書く**

`crates/eml_syntax/tests/corpus.rs`:

```rust
mod common;

use common::diagnostics;
use eml_diagnostics::SourceFiles;
use eml_syntax::parse;

const S1: &str = include_str!("corpus/s1.em");
const LATER_STAGES: &str = include_str!("corpus/later_stages.em");

#[test]
fn s1_corpus_has_no_diagnostics() {
    assert_eq!(diagnostics(S1), Vec::<String>::new());
}

#[test]
fn s1_corpus_items() {
    let mut files = SourceFiles::new();
    let file = files.add("s1.em", S1);
    let (parse, _) = parse(file, S1);
    let kinds: Vec<String> = parse
        .syntax()
        .children()
        .map(|node| format!("{:?}", node.kind()))
        .collect();
    assert_eq!(
        kinds,
        [
            "FIXITY_ITEM", "FIXITY_ITEM", "DATA_ITEM", "DATA_ITEM", "EFFECT_ITEM", "EFFECT_ITEM",
            "EFFECT_ITEM", "SIGNATURE", "EQUATION", "EQUATION", "SIGNATURE", "EQUATION",
            "SIGNATURE", "EQUATION", "SIGNATURE", "EQUATION", "SIGNATURE", "EQUATION",
            "SIGNATURE", "EQUATION", "SIGNATURE", "EQUATION", "SIGNATURE", "EQUATION",
        ]
    );
}

#[test]
fn later_stage_corpus_reports_only_not_yet_supported() {
    // S2・S3 の構文は E0004 だけを出し、ほかの診断を連鎖させない。
    let found = diagnostics(LATER_STAGES);
    assert!(!found.is_empty());
    for line in &found {
        assert!(line.starts_with("E0004 "), "unexpected diagnostic: {line}");
    }
    let mut messages: Vec<&str> = found
        .iter()
        .map(|line| line.splitn(3, ' ').nth(2).unwrap())
        .collect();
    messages.sort();
    messages.dedup();
    assert_eq!(
        messages,
        [
            "`import` is not supported yet",
            "`type` declarations are not supported yet",
            "command literals are not supported yet",
            "lists are not supported yet",
            "multi-line strings are not supported yet",
            "records are not supported yet",
            "string interpolation is not supported yet",
        ]
    );
}

/// 編集の途中のような、任意の位置で切れたソースでも、パニックせずに lossless な木を返す
/// (lossless の確認は `diagnostics` の中で行う)。
#[test]
fn every_prefix_of_the_corpus_parses() {
    for corpus in [S1, LATER_STAGES] {
        for (end, _) in corpus.char_indices() {
            diagnostics(&corpus[..end]);
        }
        diagnostics(corpus);
    }
}

/// 1行を消したソースでも、パニックせずに lossless な木を返す。
#[test]
fn every_line_deletion_of_the_corpus_parses() {
    for corpus in [S1, LATER_STAGES] {
        let lines: Vec<&str> = corpus.lines().collect();
        for skip in 0..lines.len() {
            let text: Vec<&str> = lines
                .iter()
                .enumerate()
                .filter(|(i, _)| *i != skip)
                .map(|(_, line)| *line)
                .collect();
            diagnostics(&text.join("\n"));
        }
    }
}
```

- [ ] **Step 4: テストを実行する**

Run: `cargo test -p eml_syntax --test corpus`
Expected: PASS。失敗したら、上の注意のとおり文法を直す (どの構文で失敗したかを `shape` で調べ、担当のタスクのテストに同じ形の小さなテストを足してから直す)

- [ ] **Step 5: 全体のテストと lint を通す**

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check`
Expected: すべて PASS

- [ ] **Step 6: Commit**

```bash
git add crates/eml_syntax/tests
git commit -m "Add syntax corpora and robustness tests"
```

### Task 9: 型付き AST ラッパ

**Files:**
- Modify: `crates/eml_syntax/src/ast.rs` (全体を置き換える)
- Create: `crates/eml_syntax/tests/ast.rs`

**Interfaces:**
- Consumes: Task 4 のノードの種類、Task 8 の `tests/corpus/s1.em`
- Produces (`eml_syntax::ast`。HIR の実装計画が使う):
  - ノードごとの構造体 (すべて `AstNode` を実装): `SourceFile` `Signature` `Equation` `DataItem` `Alt` `TypeItem` `EffectItem` `OpDecl` `FixityItem` `Block` `LetStmt` `UseStmt` `ExprStmt` `IfExpr` `MatchExpr` `MatchArm` `HandleExpr` `OpClause` `ReturnClause` `LambdaExpr` `LetExpr` `OpSeq` `AppExpr` `ResumeExpr` `DropExpr` `FieldExpr` `PathExpr` `Literal` `UnitExpr` `ParenExpr` `TupleExpr` `AnnotExpr` `OpRef` `LeftSection` `RightSection` `FieldSection` `WildcardPat` `BindPat` `ConPat` `LiteralPat` `UnitPat` `ParenPat` `TuplePat` `InfixConPat` `AnnotPat` `PathType` `VarType` `AppType` `FnType` `ParenType` `TupleType` `EffectRow` `Effect`
  - enum: `Item` (`Signature` / `Equation` / `DataItem` / `TypeItem` / `EffectItem` / `FixityItem`)、`Stmt` (`LetStmt` / `UseStmt` / `ExprStmt`)、`Expr` (`Block` と式のノード 20 種)、`Pat` (パターンのノード 9 種)、`Type` (型のノード 6 種)
  - アクセサ: `SourceFile::items() -> AstChildren<Item>`、`Signature::name() -> Option<SyntaxToken>`、`Signature::ty() -> Option<Type>`、`Equation::name() -> Option<SyntaxToken>`、`Equation::params() -> AstChildren<Pat>`、`Equation::body() -> Option<Expr>`、`Block::stmts() -> AstChildren<Stmt>`、`OpSeq::elements() -> impl Iterator<Item = OpSeqElement>`、`Literal::token() -> Option<SyntaxToken>`、`PathExpr::segments() -> impl Iterator<Item = SyntaxToken>`
  - `pub enum OpSeqElement { Operand(Expr), Operator(SyntaxToken) }`

アクセサは、HIR への変換で必要になったものから足していく。S1 では、上の基本的なものだけを用意する (YAGNI)。

- [ ] **Step 1: 失敗するテストを書く**

`crates/eml_syntax/tests/ast.rs`:

```rust
use eml_diagnostics::SourceFiles;
use eml_syntax::SyntaxKind::{self, *};
use eml_syntax::ast::{Expr, Item, OpSeqElement, Pat, SourceFile, Stmt, Type};
use eml_syntax::parse;
use rowan::ast::AstNode;

fn source(text: &str) -> SourceFile {
    let mut files = SourceFiles::new();
    let file = files.add("test.em", text);
    let (parse, diagnostics) = parse(file, text);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    parse.tree()
}

fn first_equation(file: &SourceFile) -> eml_syntax::ast::Equation {
    file.items()
        .find_map(|item| match item {
            Item::Equation(equation) => Some(equation),
            _ => None,
        })
        .expect("an equation")
}

#[test]
fn items_and_their_names() {
    let file = source("len : List a -> Int\nlen Nil = 0\n(<+>) : A\na <+> b = a\ndata T = | A");
    let items: Vec<String> = file
        .items()
        .map(|item| match item {
            Item::Signature(signature) => format!("signature {}", signature.name().unwrap().text()),
            Item::Equation(equation) => format!("equation {}", equation.name().unwrap().text()),
            Item::DataItem(_) => "data".to_string(),
            other => format!("{other:?}"),
        })
        .collect();
    assert_eq!(
        items,
        ["signature len", "equation len", "signature <+>", "equation <+>", "data"]
    );
}

#[test]
fn signature_type() {
    let file = source("f : Int -> Int");
    let Some(Item::Signature(signature)) = file.items().next() else {
        panic!("expected a signature");
    };
    assert!(matches!(signature.ty(), Some(Type::FnType(_))));
}

#[test]
fn equation_parameters_and_body() {
    let file = source("f x (a, b) = x");
    let equation = first_equation(&file);
    let params: Vec<SyntaxKind> = equation.params().map(|pat| pat.syntax().kind()).collect();
    assert_eq!(params, [BIND_PAT, TUPLE_PAT]);
    assert!(matches!(equation.body(), Some(Expr::PathExpr(_))));
    assert!(matches!(equation.params().next(), Some(Pat::BindPat(_))));
}

#[test]
fn block_statements() {
    let file = source("main () =\n  let x = 1\n  use f\n  g x");
    let Some(Expr::Block(block)) = first_equation(&file).body() else {
        panic!("expected a block body");
    };
    let stmts: Vec<&str> = block
        .stmts()
        .map(|stmt| match stmt {
            Stmt::LetStmt(_) => "let",
            Stmt::UseStmt(_) => "use",
            Stmt::ExprStmt(_) => "expr",
        })
        .collect();
    assert_eq!(stmts, ["let", "use", "expr"]);
}

#[test]
fn operator_sequence_elements_keep_prefix_minus() {
    let file = source("x = -a + b");
    let Some(Expr::OpSeq(seq)) = first_equation(&file).body() else {
        panic!("expected an operator sequence");
    };
    let elements: Vec<String> = seq
        .elements()
        .map(|element| match element {
            OpSeqElement::Operand(expr) => format!("operand {:?}", expr.syntax().kind()),
            OpSeqElement::Operator(token) => format!("operator {}", token.text()),
        })
        .collect();
    assert_eq!(
        elements,
        [
            "operator -",
            "operand PATH_EXPR",
            "operator +",
            "operand PATH_EXPR"
        ]
    );
}

#[test]
fn literals_and_paths() {
    let file = source("x = Foo.bar 1");
    let Some(Expr::AppExpr(app)) = first_equation(&file).body() else {
        panic!("expected an application");
    };
    let parts: Vec<Expr> = app.syntax().children().filter_map(Expr::cast).collect();
    let Expr::PathExpr(path) = &parts[0] else {
        panic!("expected a path");
    };
    let segments: Vec<String> = path.segments().map(|token| token.text().to_string()).collect();
    assert_eq!(segments, ["Foo", "bar"]);
    let Expr::Literal(literal) = &parts[1] else {
        panic!("expected a literal");
    };
    assert_eq!(literal.token().unwrap().text(), "1");
}

#[test]
fn every_node_in_the_corpus_has_an_ast_type() {
    let file = source(include_str!("corpus/s1.em"));
    // enum に入らない、ほかのノードの部品になるノード。
    let parts = [
        SOURCE_FILE, ALT, OP_DECL, MATCH_ARM, OP_CLAUSE, RETURN_CLAUSE, EFFECT_ROW, EFFECT,
    ];
    for node in file.syntax().descendants() {
        let kind = node.kind();
        let covered = Item::can_cast(kind)
            || Stmt::can_cast(kind)
            || Expr::can_cast(kind)
            || Pat::can_cast(kind)
            || Type::can_cast(kind)
            || parts.contains(&kind);
        assert!(covered, "{kind:?} has no AST type");
    }
}
```

- [ ] **Step 2: テストが失敗することを確認する**

Run: `cargo test -p eml_syntax --test ast`
Expected: コンパイルエラー (`ast::Item` などがない)

- [ ] **Step 3: AST ラッパを書く**

`crates/eml_syntax/src/ast.rs` を次の内容で置き換える。

```rust
//! 型付き AST ラッパ。rowan の木の上の薄い型付きの見方を提供する。
//! ノードごとの構造体と、項目・文・式・パターン・型の enum を持つ。
//! アクセサは、HIR への変換で必要になったものから足していく。

use rowan::NodeOrToken;
use rowan::ast::{AstChildren, AstNode, support};

use crate::{EmlLanguage, SyntaxKind, SyntaxNode, SyntaxToken};

/// ノードの種類1つに対応する構造体を定義する。
macro_rules! ast_node {
    ($($(#[$meta:meta])* $name:ident => $kind:ident,)*) => {$(
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash)]
        pub struct $name {
            syntax: SyntaxNode,
        }

        impl AstNode for $name {
            type Language = EmlLanguage;

            fn can_cast(kind: SyntaxKind) -> bool {
                kind == SyntaxKind::$kind
            }

            fn cast(syntax: SyntaxNode) -> Option<Self> {
                Self::can_cast(syntax.kind()).then_some($name { syntax })
            }

            fn syntax(&self) -> &SyntaxNode {
                &self.syntax
            }
        }
    )*};
}

/// いくつかのノードの構造体をまとめる enum を定義する。
macro_rules! ast_enum {
    ($(#[$meta:meta])* $name:ident { $($variant:ident),* $(,)? }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash)]
        pub enum $name {
            $($variant($variant),)*
        }

        impl AstNode for $name {
            type Language = EmlLanguage;

            fn can_cast(kind: SyntaxKind) -> bool {
                $($variant::can_cast(kind))||*
            }

            fn cast(syntax: SyntaxNode) -> Option<Self> {
                $(if $variant::can_cast(syntax.kind()) {
                    return $variant::cast(syntax).map($name::$variant);
                })*
                None
            }

            fn syntax(&self) -> &SyntaxNode {
                match self {
                    $($name::$variant(node) => node.syntax(),)*
                }
            }
        }
    };
}

ast_node! {
    /// ファイル全体。
    SourceFile => SOURCE_FILE,
    Signature => SIGNATURE,
    Equation => EQUATION,
    DataItem => DATA_ITEM,
    Alt => ALT,
    TypeItem => TYPE_ITEM,
    EffectItem => EFFECT_ITEM,
    OpDecl => OP_DECL,
    FixityItem => FIXITY_ITEM,
    Block => BLOCK,
    LetStmt => LET_STMT,
    UseStmt => USE_STMT,
    ExprStmt => EXPR_STMT,
    IfExpr => IF_EXPR,
    MatchExpr => MATCH_EXPR,
    MatchArm => MATCH_ARM,
    HandleExpr => HANDLE_EXPR,
    OpClause => OP_CLAUSE,
    ReturnClause => RETURN_CLAUSE,
    LambdaExpr => LAMBDA_EXPR,
    LetExpr => LET_EXPR,
    OpSeq => OP_SEQ,
    AppExpr => APP_EXPR,
    ResumeExpr => RESUME_EXPR,
    DropExpr => DROP_EXPR,
    FieldExpr => FIELD_EXPR,
    PathExpr => PATH_EXPR,
    Literal => LITERAL,
    UnitExpr => UNIT_EXPR,
    ParenExpr => PAREN_EXPR,
    TupleExpr => TUPLE_EXPR,
    AnnotExpr => ANNOT_EXPR,
    OpRef => OP_REF,
    LeftSection => LEFT_SECTION,
    RightSection => RIGHT_SECTION,
    FieldSection => FIELD_SECTION,
    WildcardPat => WILDCARD_PAT,
    BindPat => BIND_PAT,
    ConPat => CON_PAT,
    LiteralPat => LITERAL_PAT,
    UnitPat => UNIT_PAT,
    ParenPat => PAREN_PAT,
    TuplePat => TUPLE_PAT,
    InfixConPat => INFIX_CON_PAT,
    AnnotPat => ANNOT_PAT,
    PathType => PATH_TYPE,
    VarType => VAR_TYPE,
    AppType => APP_TYPE,
    FnType => FN_TYPE,
    ParenType => PAREN_TYPE,
    TupleType => TUPLE_TYPE,
    EffectRow => EFFECT_ROW,
    Effect => EFFECT,
}

ast_enum! {
    /// トップレベルの項目。
    Item { Signature, Equation, DataItem, TypeItem, EffectItem, FixityItem }
}

ast_enum! {
    /// ブロックの中の文。
    Stmt { LetStmt, UseStmt, ExprStmt }
}

ast_enum! {
    /// 式。本体の位置の字下げしたブロック (`Block`) も式として扱う。
    Expr {
        Block, IfExpr, MatchExpr, HandleExpr, LambdaExpr, LetExpr, OpSeq, AppExpr, ResumeExpr,
        DropExpr, FieldExpr, PathExpr, Literal, UnitExpr, ParenExpr, TupleExpr, AnnotExpr, OpRef,
        LeftSection, RightSection, FieldSection,
    }
}

ast_enum! {
    /// パターン。
    Pat {
        WildcardPat, BindPat, ConPat, LiteralPat, UnitPat, ParenPat, TuplePat, InfixConPat,
        AnnotPat,
    }
}

ast_enum! {
    /// 型。
    Type { PathType, VarType, AppType, FnType, ParenType, TupleType }
}

impl SourceFile {
    pub fn items(&self) -> AstChildren<Item> {
        support::children(&self.syntax)
    }
}

impl Signature {
    /// 名前のトークン (`LIDENT`、または `(OP)` の演算子)。
    pub fn name(&self) -> Option<SyntaxToken> {
        name_token(&self.syntax)
    }

    pub fn ty(&self) -> Option<Type> {
        support::child(&self.syntax)
    }
}

impl Equation {
    /// 定義する名前のトークン。関数なら `LIDENT`、演算子の定義なら演算子。
    pub fn name(&self) -> Option<SyntaxToken> {
        name_token(&self.syntax)
    }

    /// 引数のパターン。演算子の定義では左辺と右辺。
    pub fn params(&self) -> AstChildren<Pat> {
        support::children(&self.syntax)
    }

    pub fn body(&self) -> Option<Expr> {
        support::child(&self.syntax)
    }
}

impl Block {
    pub fn stmts(&self) -> AstChildren<Stmt> {
        support::children(&self.syntax)
    }
}

/// 演算子の列の要素。前置の `-` は、列の先頭か別の演算子の直後にある `Operator` として現れる (spec §7)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpSeqElement {
    Operand(Expr),
    Operator(SyntaxToken),
}

impl OpSeq {
    pub fn elements(&self) -> impl Iterator<Item = OpSeqElement> {
        self.syntax
            .children_with_tokens()
            .filter_map(|element| match element {
                NodeOrToken::Node(node) => Expr::cast(node).map(OpSeqElement::Operand),
                NodeOrToken::Token(token)
                    if matches!(
                        token.kind(),
                        SyntaxKind::OP | SyntaxKind::CONOP | SyntaxKind::MINUS
                    ) =>
                {
                    Some(OpSeqElement::Operator(token))
                }
                NodeOrToken::Token(_) => None,
            })
    }
}

impl Literal {
    /// リテラルのトークン (`INT`、`STRING` など)。
    pub fn token(&self) -> Option<SyntaxToken> {
        self.syntax.first_token()
    }
}

impl PathExpr {
    /// 名前の部分 (`Foo.bar` なら `Foo` と `bar`)。
    pub fn segments(&self) -> impl Iterator<Item = SyntaxToken> {
        self.syntax
            .children_with_tokens()
            .filter_map(NodeOrToken::into_token)
            .filter(|token| matches!(token.kind(), SyntaxKind::UIDENT | SyntaxKind::LIDENT))
    }
}

/// 直接の子のトークンのうち、最初の名前 (`LIDENT`) か演算子。
fn name_token(node: &SyntaxNode) -> Option<SyntaxToken> {
    node.children_with_tokens()
        .filter_map(NodeOrToken::into_token)
        .find(|token| {
            matches!(
                token.kind(),
                SyntaxKind::LIDENT | SyntaxKind::OP | SyntaxKind::MINUS
            )
        })
}
```

- [ ] **Step 4: テストが通ることを確認する**

Run: `cargo test -p eml_syntax --test ast`
Expected: PASS

- [ ] **Step 5: 全体のテストと lint を通す**

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check`
Expected: すべて PASS (`eml_hir` は `ast::SourceFile` だけを使うので、そのまま通る)

- [ ] **Step 6: Commit**

```bash
git add crates/eml_syntax
git commit -m "Add typed AST wrappers for the final syntax"
```

### Task 10: UI テスト、ドキュメント、最終確認

**Files:**
- Create: `tests/ui/check-fail/tab_indentation.em`、`tests/ui/check-fail/missing_indented_block.em`
- Create: `crates/eml_cli/tests/snapshots/ui__check_fail@tab_indentation.em.snap`、`ui__check_fail@missing_indented_block.em.snap` (`cargo insta review` で承認する)
- Modify: `CLAUDE.md` (Architecture と Syntax の節)
- Modify: `docs/superpowers/specs/2026-10-03-eml-language-design.md` (§3 の回復の同期点の文)

**Interfaces:**
- Consumes: Task 1〜9 のすべて
- Produces: なし

- [ ] **Step 1: レイアウトの誤りの UI テストを足す**

タブを含むので、`printf` で作る。

```bash
printf -- '-- Indentation must use spaces, not tabs.\nanswer =\n\t42\n' > tests/ui/check-fail/tab_indentation.em
printf -- '-- The body after `=` on the next line must be indented.\nanswer =\nother = 42\n' > tests/ui/check-fail/missing_indented_block.em
```

- [ ] **Step 2: UI テストを実行し、スナップショットを確かめて承認する**

Run: `cargo insta test -p eml_cli --test ui` の後に `cargo insta review`
Expected: 新しいスナップショットが2つできる。`tab_indentation` は `[E0006] Error: tab used for indentation` が `check-fail/tab_indentation.em:3:1` に1件だけ、`missing_indented_block` は `[E0009] Error: expected an indented block after `=`` が `check-fail/missing_indented_block.em:2:8` に1件だけであることを確かめてから承認する。それ以外の診断が出ていたら承認せず、Task 2 のレイアウト段を調べる

- [ ] **Step 3: `CLAUDE.md` を更新する**

`CLAUDE.md` を次のように変える。

1. Architecture の図の `eml_syntax` の行を、`eml_syntax       logos lexer, layout stage, event-based parser, rowan CST, typed AST wrappers` にする。
2. 箇条書きの「The parser follows rust-analyzer ...」の項目を、次のものに置き換える。

```markdown
- The parser follows rust-analyzer (event stream -> rowan tree built in `sink.rs`). A layout stage (`layout.rs`) between the lexer and the parser inserts virtual `LAYOUT_OPEN` / `LAYOUT_SEP` / `LAYOUT_CLOSE` tokens (syntax spec §4); the parser emits no events for them, so the CST is always lossless. Grammar rules are confined to `eml_syntax/src/grammar/`.
```

3. 「The project is at the milestone-1 skeleton stage ...」の項目を、次のものに置き換える。

```markdown
- `eml_syntax` implements stage S1 of the final syntax (syntax spec §10); S2/S3 constructs (records, modules, interpolation, command literals, ...) are lexed and parsed far enough to report E0004. The later stages (hir / types / core_ir / interp) are still stubs.
```

4. Syntax の節の本文を、次のものに置き換える。

```markdown
The final syntax is defined in `docs/superpowers/specs/2026-10-03-eml-syntax-design.md` (lexical rules, layout rule, grammar, desugaring); the provisional syntax in language-design spec §7 is obsolete. When a syntax choice is undecided, lean toward Haskell conventions.
```

- [ ] **Step 4: 言語設計 spec の回復の同期点を更新する**

`docs/superpowers/specs/2026-10-03-eml-language-design.md` の §3「エラーが出ても止まらない」の最初の項目を、次のものに置き換える (構文設計 spec §11 の「§3 エラー回復」の行)。

```markdown
- パーサは、壊れた入力に対して `ERROR` ノードを作って処理を続ける。レイアウト段の `SEP` / `CLOSE` を回復の同期点にし、トップレベルでは列 0 の `SEP` を最も強い同期点にする ([構文設計 spec](2026-10-03-eml-syntax-design.md) §4)
```

- [ ] **Step 5: 最終確認**

Run:

```bash
cargo test
cargo clippy --all-targets
cargo fmt --check
cargo run -p eml_cli -- check crates/eml_syntax/tests/corpus/s1.em; echo "exit=$?"
cargo run -p eml_cli -- check crates/eml_syntax/tests/corpus/later_stages.em; echo "exit=$?"
nix build
```

Expected: テスト・clippy・fmt はすべて通る。`s1.em` の検査は診断なしで `exit=0`。`later_stages.em` は E0004 だけが表示されて `exit=1`。`nix build` が成功する

- [ ] **Step 6: Commit**

```bash
git add tests/ui crates/eml_cli/tests/snapshots CLAUDE.md docs/superpowers/specs/2026-10-03-eml-language-design.md
git commit -m "Add layout UI tests and document the final syntax stage"
```
