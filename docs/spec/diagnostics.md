# 診断

位置づけ: 規範。各番号の診断が指す場所と、help と fix の文言と付ける条件は [診断の出し方](../implementation/diagnostics.md) にある。

診断のデータ構造、診断の順、番号の範囲、各番号の意味、連鎖する診断の抑止を定める。`eml check` は、構文・名前解決・型・線形性・エフェクト・`match` の網羅性のエラーを、ソース位置を指す診断として表示する。1回の実行で、独立した複数のエラーを報告する。

## データ構造

```rust
struct Diagnostic {
    code: ErrorCode,          // E0001 などの安定した番号
    severity: Severity,       // Error / Warning
    message: String,
    primary: Label,           // (FileId, TextRange, ラベル文)
    secondary: Vec<Label>,
    notes: Vec<String>,
    help: Vec<String>,
    fix: Option<Fix>,
}

struct Fix {
    title: String,            // 何をする fix か。LSP の code action の題名になる
    edits: Vec<TextEdit>,
}
```

- CLI では `ariadne` で表示する。将来の LSP では、同じ構造体から LSP の診断と code action に変換する。
- `Diagnostic`、`FileId`、`SourceFiles` は `eml_diagnostics` に置く。`eml_diagnostics` は `rowan` に依存しない ([コンパイラの構成](../implementation/architecture.md))。
- 番号の定数は、段階ごとの `codes` モジュールに定義する (例: `eml_syntax::codes`)。E0004 (未対応) は、構文、HIR、型検査のどの段階でも同じ意味で使うので、番号とラベルを `eml_diagnostics` に置く (`NOT_YET_SUPPORTED`、`Diagnostic::not_yet_supported`)。
- 補足の情報は、独立した診断ではなく `notes` と `help` に入れる。そのため `Severity` は `Error` と `Warning` の2つだけにする。
- `TextRange` は、読み込み時に先頭の BOM を除いたテキストのバイト位置である ([字句](lexical.md))。

## 診断の順

`eml check` と `eml run` は、診断を (ファイル、primary の開始位置、番号) の順に並べて表示する。3つとも同じなら、段階が出した順を保つ。各段階は診断の順を約束しない。並べ替えは `eml_diagnostics::sort_diagnostics` の1か所で行い、パイプラインを組む `eml_cli::Session` (CLI と結合テストが通る) と、構文の段だけを通す `eml_test_support::parse` がそれを呼ぶ。

## 番号の範囲

`ErrorCode` は段階ごとに番号の範囲を分ける。

| 範囲 | 段階 |
|---|---|
| E0xxx | 字句・構文 |
| E1xxx | 名前解決と HIR での検査 (重複定義、未定義の名前、handler の節と `drop` の引数の個数など) |
| E2xxx | 型・Kind・row |
| E3xxx | 線形性・継続の多重度 |
| E4xxx | パターンの網羅性 |

## 割り当て済みの番号

| 番号 | 名前 | 意味 |
|---|---|---|
| E0001 | `UNEXPECTED_CHARACTER` | eml のソースとして認識できない文字 |
| E0002 | `UNTERMINATED_STRING` | 閉じていない文字列リテラル (複数行の文字列、raw 文字列、コマンドリテラルを含む) |
| E0003 | `EXPECTED_ITEM` | トップレベルで項目の始まりでないトークン |
| E0004 | `NOT_YET_SUPPORTED` | 後の段階で実装する構文 (浮動小数と文字のリテラル、補間、レコードなど) |
| E0005 | `UNTERMINATED_BLOCK_COMMENT` | 閉じていないブロックコメント |
| E0006 | `TAB_INDENTATION` | インデントにタブを使った |
| E0007 | `INVALID_NUMBER` | 不正な数値リテラル |
| E0008 | `INVALID_ESCAPE` | 不正なエスケープ (未知のエスケープ、不正な `\u{...}`) |
| E0009 | `EXPECTED_INDENTED_BLOCK` | 開始トークンの後に字下げしたブロックが必要 |
| E0010 | `SPACE_AROUND_DOT` | `.` の前後の空白 |
| E0011 | `SYNTAX_ERROR` | その他の構文エラー |
| E0012 | `NEEDS_PARENS` | 括弧の要る形を括弧なしで書いた ([文法](grammar.md) の「文法上の補足」) |
| E0013 | `NESTING_TOO_DEEP` | 式・パターン・型の入れ子が深すぎる (256 を超えた。[文法](grammar.md)) |
| E1001 | `UNDEFINED_NAME` | 未定義の値の名前 |
| E1002 | `UNDEFINED_TYPE` | 未定義の型の名前、未定義のエフェクトの名前、本体の注釈に書いたシグネチャにない型変数と row 変数 |
| E1003 | `DUPLICATE_DEFINITION` | 同じ名前空間でのトップレベルの定義の重複 |
| E1004 | `MISSING_SIGNATURE` | シグネチャのない等式 |
| E1005 | `MISSING_EQUATION` | 等式のないシグネチャ。`extern` のシグネチャは対象にならない。どのモジュールでも同じである |
| E1006 | `NON_ASSOCIATIVE_OPERATORS` | 結合しない演算子の並び、優先順位が同じで結合の向きが違う演算子の並び |
| E1007 | `INVALID_OPERATION_SIGNATURE` | 操作のシグネチャの一番外側の `->` に row を書いた。または、シグネチャが関数型でない |
| E1008 | `NEVER_RESULT_NOT_FREE` | `never` の操作の結果の型が、引数に現れない型変数でない |
| E1009 | `UNHANDLEABLE_EFFECT` | handler の節の先頭に、extern のエフェクト (`IO`) を起こす extern の関数 (`println`、`Fs.open` など) の名前を書いた。操作として引けず、extern の関数として引けたときだけである |
| E1010 | `CLAUSE_ARITY` | handler の節の引数の個数の誤り |
| E1011 | `DROP_ARITY` | `drop` の引数の個数の誤り |
| E1012 | `MIXED_EFFECTS_IN_HANDLER` | 1つの handler に別のエフェクトの操作の節が混ざった |
| E1013 | `MISSING_CLAUSE` | 節のない操作がある。操作の節が1つもない handler も含む |
| E1014 | `DUPLICATE_CLAUSE` | 同じ操作の節、または `return` の節が2つある |
| E1015 | `TYPE_ARGUMENT_COUNT` | `data` の型の適用や row の中のエフェクトの型引数の個数が、宣言と違う |
| E1016 | `CONSTRUCTOR_ARITY` | パターンのコンストラクタの引数の個数が、宣言のフィールドの数と違う |
| E1017 | `DUPLICATE_BINDING` | 1つのパターン、または1つの等式の引数の並び、ラムダの引数の並び、あるいは handler の節の引数の並び (操作の引数と `k`) の中で、同じ変数名を2回束縛した |
| E1018 | `NON_CONSECUTIVE_EQUATIONS` | 同じ名前の等式が連続していない (間に別の item がある) |
| E1019 | `SIGNATURE_NOT_ADJACENT` | シグネチャと最初の等式が隣り合っていない |
| E1020 | `EQUATION_ARITY_MISMATCH` | 等式ごとに引数の個数が違う |
| E1021 | `DUPLICATE_FIXITY` | 同じ演算子への2回目の fixity の宣言。1つの宣言に同じ演算子を2回並べた場合を含む |
| E1022 | `FIXITY_WITHOUT_DEFINITION` | このモジュールで定義していない演算子への fixity の宣言。標準ライブラリの演算子の fixity を変えようとした場合を含む |
| E1023 | `INVALID_SECTION` | 優先順位の合わないセクション (`(* a + b)` や `(+ a + b)`) |
| E1024 | `USE_AT_END_OF_BLOCK` | ブロックの最後の文が `use` である (包む残りがない) |
| E1025 | `MISSING_CONSTRUCTORS` | `data` にコンストラクタがない (`=` のない `data`)。`extern data` は対象にならない。どのモジュールでも同じである |
| E1026 | `MODULE_NOT_FOUND` | import したモジュールのファイルがない、または読めない (UTF-8 でない、IO の誤り)。`import Std.Nope` のように標準ライブラリを指す import で `std/` にファイルがない場合を含む |
| E1027 | `IMPORT_CYCLE` | import の循環 |
| E1028 | `AMBIGUOUS_NAME` | 修飾しない名前、または合流した修飾子の名前が、別々の定義を指して曖昧である |
| E1029 | `PRIVATE_NAME` | ユーザーのモジュールの `pub` でない名前を、修飾か import の並びで使った |
| E1030 | `RESERVED_MODULE` | 修飾子が `Prelude` になる import、`import Main`、入口のファイルを指す import、`import Std` (標準ライブラリの根) |
| E1031 | `UNKNOWN_QUALIFIER` | 修飾子がどの import にも、標準ライブラリのモジュールにもない。2つ以上のセグメントの修飾子 (`Report.Csv.parse`) を含む |
| E1032 | `PRIVATE_IN_PUBLIC` | `pub` の item の型に、同じモジュールの `pub` でない型かエフェクトが現れた ([モジュールと名前解決](modules.md) の「公開の範囲」) |
| E1033 | `EXTERN_OUTSIDE_STD` | ユーザーのモジュールに `extern` を書いた。標準ライブラリ (`std/`) のモジュールだけが書ける ([宣言](declarations.md) の「`extern`」) |
| E2001 | `TYPE_MISMATCH` | 型の不一致。呼び出しの row のエフェクトの型引数が今の row と一致しない場合を含む |
| E2002 | `EFFECT_NOT_IN_ROW` | シグネチャの row に含まれないエフェクトを起こした。ラムダの本体の場合を含む |
| E2003 | `MISSING_MAIN` | 入口のモジュールに `main` がない。import した `main` は数えない。`eml run` のときだけ出す |
| E2004 | `INVALID_MAIN_TYPE` | `main` のシグネチャが `Unit -> <IO> Unit` でない |
| E2005 | `INFINITE_TYPE` | 無限の型 (単一化の occurs check)。row のラベルの型引数を通して、型変数か row 変数が自分自身の中に現れる場合を含む |
| E2006 | `NOT_COMPARABLE` | `==` か `!=` で、`Int`、`String`、`Bool` のどれでもない型の値を比べた |
| E2008 | `MASK_CONFLICT` | 呼び出し先が自分で起こすエフェクトを、同じ呼び出しで row 変数のために飛ばす必要がある |
| E3001 | `LINEAR_VALUE_MISUSED` | 線形な値の誤った使い方のうち、E3002〜E3005 に当たらないもの (関数への受け渡し、型の単一化、ラムダや節の捕獲) |
| E3002 | `LINEAR_VALUE_USED_TWICE` | 線形な値を、ある経路で2回以上使った |
| E3003 | `LINEAR_VALUE_NOT_CONSUMED` | 線形な値を、ある経路で使わなかった |
| E3004 | `LINEAR_VALUE_DISCARDED` | 線形な値を `_` で受けた。状態のある handler で省いた `return` の節が `Lin` の状態を捨てた場合を含む |
| E3005 | `CONTINUATION_NOT_HANDLED` | `once` の操作の節の `k` を、ある経路で呼びも `drop` もしなかった |
| E3006 | `LINEAR_VALUE_KEPT_ACROSS_MULTI` | 線形な値を持ったまま、`multi` の操作を起こしうる呼び出し、`handle` をまたいだ (持ち越し規則)。呼んだ関数のスキームを通る持ち越しを含む |
| E4001 | `NON_EXHAUSTIVE_MATCH` | 網羅されていない `match` |
| E4002 | `NON_EXHAUSTIVE_EQUATION` | 網羅されていない等式 |
| E4003 | `REFUTABLE_PATTERN` | 反駁可能な `let` の左辺、ラムダの引数、handler の節の引数と `return` の節の引数のパターン |
| E4004 | `UNREACHABLE_ARM` | 到達しない枝 (Warning) |
| E4005 | `UNREACHABLE_EQUATION` | 到達しない等式 (Warning) |

E2007 は欠番である。もとは `resume` の状態の欄の食い違いに使っていた番号で、ほかの診断には使わない。

E0004 (`NOT_YET_SUPPORTED`) は、まだ実装していない構文に使う。S6 とコマンドリテラルの段で入れる構文と、字句として予約した浮動小数と文字のリテラル (`Float`、`Char`、`Num` の段) である。どの段階でも「後で実装する」という同じ意味なので、番号を分けない。HIR 以降の段階は、対応していない構文を、診断を出さずに無視することはしない。見つけた段階で E0004 を出して回復する。

## 番号を割り当てていない診断

構文の設計で決めた診断のうち、まだ番号を割り当てていないものは次のとおり。それぞれを実装する段階で番号を割り当てる。

| 範囲 | 例 |
|---|---|
| E0xxx | 閉じていない補間 |
| E3xxx | 射影で `Lin` な残りを捨てる、更新で `Lin` な古い値を捨てる |

## 他の言語の書き方へのヒント

他の言語の書き方を前提にした help や Warning は出さない。例えば Haskell 形式の `\x ->` のラムダに対するヒントはない。診断が煩雑になるだけのためである。

## 連鎖する診断の抑止

パーサは壊れた入力に対して `ERROR` ノードを作って処理を続ける。名前解決以降は、エラーが起きた場所に `Error` 型を入れ、`Error` が関わる制約や線形性の検査からは追加の診断を出さない ([コンパイラの構成](../implementation/architecture.md))。使用回数のパスが出す線形性の診断 (E3001〜E3005) は、HIR の誤りがある本体、誤りの跡 (`Missing`) がある本体、型の誤りを報告済みの本体からは出さない。使った回数を正しく数えられないためである。
