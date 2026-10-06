# 診断

位置づけ: 規範。割り当て済みの番号は、段階ごとの `codes` モジュール (`eml_syntax`、`eml_hir`、`eml_types`) にある。どの段階でも使う E0004 だけは `eml_diagnostics` にある。

診断のデータ構造、番号の範囲、各誤りで診断が指す場所を定める。`eml check` は、構文・名前解決・型・線形性・エフェクト・`match` の網羅性のエラーを、ソース位置を指す診断として表示する。1回の実行で、独立した複数のエラーを報告する。

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

`eml check` と `eml run` は、診断を (ファイル、primary の開始位置、番号) の順に並べて表示する。3つとも同じなら、段階が出した順を保つ。各段階は診断の順を約束しない。並べ替えは `eml_diagnostics::sort_diagnostics` の1か所で行い、CLI と結合テストのパイプライン (`eml_test_support`) がそれを呼ぶ。

## 番号の範囲

`ErrorCode` は段階ごとに番号の範囲を分ける。

| 範囲 | 段階 |
|---|---|
| E0xxx | 字句・構文 |
| E1xxx | 名前解決と HIR での検査 (重複定義、未定義の名前、handler の節と `resume` の引数の個数など) |
| E2xxx | 型・Kind・row |
| E3xxx | 線形性・継続の多重度 |
| E4xxx | パターンの網羅性 |

## 割り当て済みの番号

E0xxx は `eml_syntax::codes` (E0004 だけは `eml_diagnostics`)、E1xxx は `eml_hir::codes`、E2xxx は `eml_types::codes` に置く。E3xxx は `eml_types::codes` に置く。E4xxx は `eml_types::codes` に置く。射影と更新の番号は M3 で割り当てる。

| 番号 | 定数 | 内容 |
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
| E0012 | `NEEDS_PARENS` | 括弧の要る式 (`if`、`match`、`handle`、`let`) を、引数や演算の項の位置に括弧なしで書いた |
| E0013 | `NESTING_TOO_DEEP` | 式・パターン・型の入れ子が深すぎる (256 を超えた。[文法](grammar.md)) |
| E1001 | `UNDEFINED_NAME` | 未定義の値の名前 |
| E1002 | `UNDEFINED_TYPE` | 未定義の型の名前、未定義のエフェクトの名前、本体の注釈に書いたシグネチャにない型変数と row 変数 |
| E1003 | `DUPLICATE_DEFINITION` | 同じ名前空間でのトップレベルの定義の重複 |
| E1004 | `MISSING_SIGNATURE` | シグネチャのない等式。シグネチャの追加を提案する help を付ける |
| E1005 | `MISSING_EQUATION` | 等式のないシグネチャ |
| E1006 | `NON_ASSOCIATIVE_OPERATORS` | 結合しない演算子の並び、優先順位が同じで結合の向きが違う演算子の並び |
| E1007 | `INVALID_OPERATION_SIGNATURE` | 操作のシグネチャの一番外側の `->` に row を書いた。または、シグネチャが関数型でない |
| E1008 | `NEVER_RESULT_NOT_FREE` | `never` の操作の結果の型が、引数に現れない型変数でない |
| E1009 | `UNHANDLEABLE_EFFECT` | handler に組み込みの `IO` の操作の節を書いた |
| E1010 | `CLAUSE_ARITY` | handler の節の引数の個数の誤り |
| E1011 | `KEYWORD_ARITY` | `resume` と `drop` の引数の個数の誤り |
| E1012 | `MIXED_EFFECTS_IN_HANDLER` | 1つの handler に別のエフェクトの操作の節が混ざった |
| E1013 | `MISSING_CLAUSE` | 節のない操作がある。操作の節が1つもない handler も含む。primary は `handle` で、節の追加を help で示す |
| E1014 | `DUPLICATE_CLAUSE` | 同じ操作の節、または `return` の節が2つある |
| E1015 | `TYPE_ARGUMENT_COUNT` | `data` の型の適用や row の中のエフェクトの型引数の個数が、宣言と違う |
| E1016 | `CONSTRUCTOR_ARITY` | パターンのコンストラクタの引数の個数が、宣言のフィールドの数と違う |
| E1017 | `DUPLICATE_BINDING` | 1つのパターン、または1つの等式の引数の並び、ラムダの引数の並び、あるいは handler の節の引数の並び (操作の引数と `k`) の中で、同じ変数名を2回束縛した。2つ目の束縛を primary、1つ目を secondary にする |
| E1018 | `NON_CONSECUTIVE_EQUATIONS` | 同じ名前の等式が連続していない (間に別の item がある)。離れた等式の関数名を primary、直前の等式の並びの最後の等式の関数名を secondary にする。等式をシグネチャの直後にまとめるよう help で伝える |
| E1019 | `SIGNATURE_NOT_ADJACENT` | シグネチャと最初の等式が隣り合っていない。最初の等式の関数名を primary、シグネチャの関数名を secondary にする。等式をシグネチャの直後に置くよう help で伝える |
| E1020 | `EQUATION_ARITY_MISMATCH` | 等式ごとに引数の個数が違う。個数の違う等式の関数名を primary、最初の等式の関数名を secondary にする |
| E1021 | `DUPLICATE_FIXITY` | 同じ演算子への2回目の fixity の宣言。1つの宣言に同じ演算子を2回並べた場合を含む。2回目の宣言の演算子を primary、1回目を secondary にする |
| E1022 | `FIXITY_WITHOUT_DEFINITION` | このモジュールで定義していない演算子への fixity の宣言。Prelude の演算子の fixity を変えようとした場合を含む。宣言の演算子を指す |
| E1023 | `INVALID_SECTION` | 優先順位の合わないセクション (`(* a + b)` や `(+ a + b)`)。セクション全体を指し、被演算子を括弧で囲むよう help で伝える |
| E1024 | `USE_AT_END_OF_BLOCK` | ブロックの最後の文が `use` である (包む残りがない)。`use` の文を指す |
| E1025 | `MISSING_CONSTRUCTORS` | ユーザーのモジュールの `data` にコンストラクタがない (`=` のない `data`)。`data` の名前を指す。`=` のない `data` は `Prelude` の intrinsic の型だけに使う |
| E2001 | `TYPE_MISMATCH` | 型の不一致。メッセージとラベルは制約の由来ごとに変える ([型と Kind](types.md))。呼び出しの row のエフェクトの型引数が今の row と一致しないときも E2001 にし、呼び出しを primary にする |
| E2002 | `EFFECT_NOT_IN_ROW` | シグネチャの row に含まれないエフェクトを起こした。シグネチャの矢印を指し、row を足す help を付ける。ラムダの本体の場合は、エフェクトを起こした場所を primary、ラムダの期待する型の由来 (シグネチャの引数の型や型の明示) を secondary にする |
| E2003 | `MISSING_MAIN` | `main` がない。`eml run` のときだけ出す |
| E2004 | `INVALID_MAIN_TYPE` | `main` のシグネチャが `Unit -> <IO> Unit` でない |
| E2005 | `INFINITE_TYPE` | 無限の型 (単一化の occurs check)。row のラベルの型引数を通して、型変数か row 変数が自分自身の中に現れる場合を含む。呼び出しの row で起きたときは、呼び出しを指す |
| E2006 | `NOT_COMPARABLE` | `==` か `!=` で、`Int`、`String`、`Bool` のどれでもない型の値を比べた。演算子を primary にし、比べようとした型をメッセージに出す。note で比べられる型を示す |
| E2007 | `RESUME_STATE_MISMATCH` | `resume` の引数の個数が、`k` の状態の欄と合わない。primary は `resume` の式で、secondary は付けない。状態のある handler の `k` を2引数で再開したときは、次の状態を3つ目の引数で渡すよう help で伝える。状態のない `k` を3引数で再開したときは、3つ目の引数を除くよう help で伝える。fix は2種類ある。状態のない `k` への3引数には、題名 ``remove the state argument`` で、2つ目の引数の終わりから3つ目の引数の終わりまでを消す fix を、つねに付ける。状態のある `k` への2引数には、題名 ``pass the current state `st` `` で、2つ目の引数の後に ` st` を入れる fix を、次の3つをすべて満たすときだけ付ける。(1) `k` の式が、`resume` を囲む操作の節の `k` の変数をそのまま参照している、(2) その節の状態の引数のパターンが、変数の束縛である (型の明示を含んでもよい)、(3) `resume` の位置で、その名前が後の束縛に隠されていない。`resume` 以外の単一化で欄が食い違ったとき (`k` をラムダの引数に渡した後で、状態のある `k` と出会ったときなど) は、食い違いを見つけた式を primary にし、fix を付けない |
| E3001 | `LINEAR_VALUE_MISUSED` | 線形な値の誤った使い方のうち、E3002〜E3005 に当たらないもの (関数への受け渡し、型の単一化、ラムダや節の捕獲)。違反した Kind の制約の由来を指す |
| E3002 | `LINEAR_VALUE_USED_TWICE` | 線形な値を、ある経路で2回以上使った。2回目に使った位置を primary、1回目を secondary にする |
| E3003 | `LINEAR_VALUE_NOT_CONSUMED` | 線形な値を、ある経路で使わなかった。束縛した位置を primary、使わなかった枝、省いた `else`、またはスコープの終わりを secondary にする。どの経路でも使わないうちに同じブロックの後の `let` で隠されたときは、スコープの終わりではなく隠した束縛を secondary にし、隠す前に `drop` するよう help で伝える。help で `drop` を提案し、使わなかった経路がブロックなら、その最後の文の前に `drop x` の行を入れる fix を付ける。`drop x` を入れる位置で同じ名前の後の束縛が見えているときは、fix を付けない |
| E3004 | `LINEAR_VALUE_DISCARDED` | 線形な値を `_` で受けた。パターンを指す。状態のある handler で省いた `return` の節が `Lin` の状態を捨てた場合は、`from` の初期値を指し、状態を受ける `return` の節を書くよう help で伝える。fix は付けない |
| E3005 | `CONTINUATION_NOT_HANDLED` | `once` の操作の節の `k` を、ある経路で `resume` も `drop` もしなかった。節を primary、`k` の束縛を secondary にする |
| E3006 | `LINEAR_VALUE_KEPT_ACROSS_MULTI` | 線形な値を持ったまま、`multi` の操作を起こしうる呼び出し、`resume`、`handle` をまたいだ (持ち越し規則)。同じ値は、呼び出しの位置が最も前の1件だけを報告する。持ち越した値が handler の状態のときは、`from` の初期値を secondary にする。呼んだ関数のスキームを通る持ち越しの違反は、そのスキームに残した組ごとに1件で、呼んだ関数の中で位置が最も前の持ち越しを指す |
| E4001 | `NON_EXHAUSTIVE_MATCH` | 網羅されていない `match` |
| E4002 | `NON_EXHAUSTIVE_EQUATION` | 網羅されていない等式 |
| E4003 | `REFUTABLE_PATTERN` | 反駁可能な `let` の左辺、ラムダの引数、handler の節の引数と `return` の節の引数のパターン |
| E4004 | `UNREACHABLE_ARM` | 到達しない枝 (Warning) |
| E4005 | `UNREACHABLE_EQUATION` | 到達しない等式 (Warning) |

E0004 (`NOT_YET_SUPPORTED`) は、まだ実装していない構文に使う。M2、M3、M9 の構文と、字句として予約した浮動小数と文字のリテラル (M4) である。どの段階でも「後で実装する」という同じ意味なので、番号を分けない。HIR 以降の段階は、対応していない構文を、診断を出さずに無視することはしない。見つけた段階で E0004 を出して回復する。

## 番号を割り当てていない診断

構文の設計で決めた診断のうち、まだ番号を割り当てていないものは次のとおり。それぞれを実装する段階で番号を割り当てる。

| 範囲 | 例 |
|---|---|
| E0xxx | 閉じていない補間 |
| E1xxx | 修飾なしの名前の衝突 |
| E3xxx | 射影で `Lin` な残りを捨てる、更新で `Lin` な古い値を捨てる |

シグネチャに関する E1xxx の診断には、シグネチャの追加を提案する help を付ける ([宣言](declarations.md))。

## 他の言語の書き方へのヒント

他の言語の書き方を前提にした help や Warning は出さない。例えば Haskell 形式の `\x ->` のラムダに対するヒントはない。診断が煩雑になるだけのためである。

## 型エラー

制約の由来 (引数の位置、`if` の各枝、型注釈など) をもとに、「expected / found」と、その根拠になった場所を示すラベルを出す ([型と Kind](types.md))。

## 線形性の診断

| 誤り | 指す場所 |
|---|---|
| 二重使用 | 1回目に消費した場所と、2回目に使った場所 |
| 消費されていない | 束縛した場所とスコープの終わり。同じブロックの後の `let` で隠されたときは、隠した束縛。help と fix で `drop x` の追加を提案する |
| `_` で `Lin` の値を受けた | そのパターン。help で、変数に束縛して `drop` するよう提案する。省いた `return` の節が `Lin` の状態を捨てたときは、`from` の初期値 |
| `multi` の呼び出しをまたぐ | primary はその呼び出し、`resume`、または `handle` で、起こしうる `multi` の操作を書く。secondary は値 (変数の束縛、途中の値の部分式、`return` の節) と、`multi` と宣言した操作。値が handler の状態のときは、`from` の初期値を値の secondary にする。help で、呼び出しの前に使い終えるよう提案する |
| 呼んだ関数が `multi` の呼び出しをまたがせる | primary は呼んだ関数を参照した位置。secondary は、呼んだ関数の中で値をまたがせている呼び出し、またはその先の関数の参照 (1段だけ) |
| 継続の扱い忘れ | `k` に `resume` も `drop` もしていない handler の節 |
| 射影・更新で `Lin` な値を捨てる | 射影または更新の式。help と fix で、分解パターンへの書き換えを提案する ([直積型とレコード](records.md)) |

## 網羅性の診断

| 番号 | 誤り | 重大度 | 指す場所 |
|---|---|---|---|
| E4001 | 網羅されていない `match` | Error | `match` 式。漏れているパターンの例を note で示し、fix で枝の追加を提案する (fix は、どの型にもなる仮置きの式を言語に入れるときに一緒に入れる) |
| E4002 | 網羅されていない等式 | Error | primary はシグネチャの関数名 (シグネチャがなければ最初の等式の関数名)、secondary は各等式の先頭。漏れている引数の並びの例 (`f None _`) を note で示し、fix で最後の等式の後への等式の追加を提案する (fix は、どの型にもなる仮置きの式を言語に入れるときに一緒に入れる) |
| E4004 | 到達しない枝 | Warning | その枝のパターン |
| E4005 | 到達しない等式 | Warning | その等式の引数のパターン |
| E4003 | 反駁可能な `let` / ラムダの引数 / handler の節と `return` の節の引数のパターン | Error | そのパターン |

等式の場合はソースに `match` 式がないので、指す場所を等式に合わせて決めている。handler の節はラムダと同じく持ち上げる関数で、引数のパターンも同じ経路でコンパイルするため、ラムダの引数と同じ E4003 にする。

漏れているパターンの例は、note に最大3つ並べる。3つより多ければ、残りがあることを書き添える。

## 連鎖する診断の抑止

パーサは壊れた入力に対して `ERROR` ノードを作って処理を続ける。名前解決以降は、エラーが起きた場所に `Error` 型を入れ、`Error` が関わる制約や線形性の検査からは追加の診断を出さない ([コンパイラの構成](../implementation/architecture.md))。使用回数のパスが出す線形性の診断 (E3001〜E3005) は、HIR の誤りがある本体、誤りの跡 (`Missing`) がある本体、型の誤りを報告済みの本体からは出さない。使った回数を正しく数えられないためである。
