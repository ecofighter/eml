# 診断の出し方

位置づけ: 手引き。

番号ごとの診断が指す場所 (primary と secondary)、メッセージと note の言い方、help と fix の文言と付ける条件を書く。番号の意味、データ構造、診断の順、番号の範囲、連鎖する診断の抑止は [診断](../spec/diagnostics.md) で定める。

## 番号の置き場所

E0xxx は `eml_syntax::codes` (E0004 だけは `eml_diagnostics`)、E1xxx は `eml_hir::codes`、E2xxx、E3xxx、E4xxx は `eml_types::codes` に置く。

## 番号ごとの出し方

番号の意味は [診断](../spec/diagnostics.md) の「割り当て済みの番号」が定める。この表は、表示の決まりがある番号について、今の行の全体を写したものである。

| 番号 | 定数 | 内容 |
|---|---|---|
| E0012 | `NEEDS_PARENS` | 括弧の要る形を括弧なしで書いた。`if`、`match`、`handle`、`fn`、`let ... in` を引数や演算の項の位置に書いた場合と、`drop` を引数の位置に書いた場合である ([文法](../spec/grammar.md) の「文法上の補足」) |
| E1001 | `UNDEFINED_NAME` | 未定義の値の名前。修飾した名前と import の並びでは、メッセージにモジュールの名前を書く (「in module `Report.Csv`」) |
| E1002 | `UNDEFINED_TYPE` | 未定義の型の名前、未定義のエフェクトの名前、本体の注釈に書いたシグネチャにない型変数と row 変数。修飾した名前と import の並びでは、E1001 と同じくメッセージにモジュールの名前を書く |
| E1004 | `MISSING_SIGNATURE` | シグネチャのない等式。シグネチャの追加を提案する help を付ける |
| E1013 | `MISSING_CLAUSE` | 節のない操作がある。操作の節が1つもない handler も含む。primary は `handle` で、節の追加を help で示す |
| E1017 | `DUPLICATE_BINDING` | 1つのパターン、または1つの等式の引数の並び、ラムダの引数の並び、あるいは handler の節の引数の並び (操作の引数と `k`) の中で、同じ変数名を2回束縛した。2つ目の束縛を primary、1つ目を secondary にする |
| E1018 | `NON_CONSECUTIVE_EQUATIONS` | 同じ名前の等式が連続していない (間に別の item がある)。離れた等式の関数名を primary、直前の等式の並びの最後の等式の関数名を secondary にする。等式をシグネチャの直後にまとめるよう help で伝える |
| E1019 | `SIGNATURE_NOT_ADJACENT` | シグネチャと最初の等式が隣り合っていない。最初の等式の関数名を primary、シグネチャの関数名を secondary にする。等式をシグネチャの直後に置くよう help で伝える |
| E1020 | `EQUATION_ARITY_MISMATCH` | 等式ごとに引数の個数が違う。個数の違う等式の関数名を primary、最初の等式の関数名を secondary にする |
| E1021 | `DUPLICATE_FIXITY` | 同じ演算子への2回目の fixity の宣言。1つの宣言に同じ演算子を2回並べた場合を含む。2回目の宣言の演算子を primary、1回目を secondary にする |
| E1022 | `FIXITY_WITHOUT_DEFINITION` | このモジュールで定義していない演算子への fixity の宣言。Prelude の演算子の fixity を変えようとした場合を含む。宣言の演算子を指す |
| E1023 | `INVALID_SECTION` | 優先順位の合わないセクション (`(* a + b)` や `(+ a + b)`)。セクション全体を指し、被演算子を括弧で囲むよう help で伝える |
| E1024 | `USE_AT_END_OF_BLOCK` | ブロックの最後の文が `use` である (包む残りがない)。`use` の文を指す |
| E1025 | `MISSING_CONSTRUCTORS` | ユーザーのモジュールの `data` にコンストラクタがない (`=` のない `data`)。`data` の名前を指す。`=` のない `data` は `Prelude` の intrinsic の型だけに使う |
| E1026 | `MODULE_NOT_FOUND` | import したモジュールのファイルがない、または読めない (UTF-8 でない、IO の誤り)。読めない理由をメッセージに書く。import のモジュールのパスを指す |
| E1027 | `IMPORT_CYCLE` | import の循環。循環を閉じる import を指し、循環の経路を note で示す |
| E1028 | `AMBIGUOUS_NAME` | 修飾しない名前、または合流した修飾子の名前が、別々の定義を指して曖昧である。使った位置を primary にし、候補の import を secondary にする |
| E1029 | `PRIVATE_NAME` | ユーザーのモジュールの `pub` でない名前を、修飾か import の並びで使った。名前を primary にし、定義を secondary にする |
| E1030 | `RESERVED_MODULE` | 修飾子が `Prelude` になる import、`import Main`、入口のファイルを指す import。import を指す |
| E1031 | `UNKNOWN_QUALIFIER` | 修飾子がどの import にもない。2つ以上のセグメントの修飾子 (`Report.Csv.parse`) を含む。修飾子の全体を指し、そのモジュールを import していれば、使える修飾子 (`Csv.parse`) を help で示す |
| E1032 | `PRIVATE_IN_PUBLIC` | `pub` の item の型に、同じモジュールの `pub` でない型かエフェクトが現れた ([モジュールと名前解決](../spec/modules.md) の「公開の範囲」)。非公開の型かエフェクトの名前を primary にし、その定義を secondary にして、`pub` を付けるよう help で伝える |
| E2001 | `TYPE_MISMATCH` | 型の不一致。メッセージとラベルは制約の由来ごとに変える ([型と Kind](../spec/types.md))。呼び出しの row のエフェクトの型引数が今の row と一致しないときも E2001 にし、呼び出しを primary にする |
| E2002 | `EFFECT_NOT_IN_ROW` | シグネチャの row に含まれないエフェクトを起こした。シグネチャの矢印を指し、row を足す help を付ける。ラムダの本体の場合は、エフェクトを起こした場所を primary、ラムダの期待する型の由来 (シグネチャの引数の型や型の明示) を secondary にする |
| E2005 | `INFINITE_TYPE` | 無限の型 (単一化の occurs check)。row のラベルの型引数を通して、型変数か row 変数が自分自身の中に現れる場合を含む。呼び出しの row で起きたときは、呼び出しを指す |
| E2006 | `NOT_COMPARABLE` | `==` か `!=` で、`Int`、`String`、`Bool` のどれでもない型の値を比べた。演算子を primary にし、比べようとした型をメッセージに出す。note で比べられる型を示す |
| E2008 | `MASK_CONFLICT` | 呼び出し先が自分で起こすエフェクトを、同じ呼び出しで row 変数のために飛ばす必要がある。primary は呼び出しの式で、メッセージには、呼び出し先がそのエフェクトを自分で起こし、row 変数を通して外側の handler にも渡すことを書く。今の row がラムダの row でなくシグネチャの row で、シグネチャの本体の矢印の row が row 変数の手前にそのエフェクトを並べているときだけ、その矢印を secondary にして、そのエフェクトを並べていることを示す。余ったのが囲む handle の足したラベルだけなら、シグネチャの row は余りに関わらないので secondary を付けない。note で、呼び出し自身のそのエフェクトはいちばん内側の handler に届くが、row 変数を通るほうはその handler を飛ばさなければならないことを示す |
| E3001 | `LINEAR_VALUE_MISUSED` | 線形な値の誤った使い方のうち、E3002〜E3005 に当たらないもの (関数への受け渡し、型の単一化、ラムダや節の捕獲)。違反した Kind の制約の由来を指す |
| E3002 | `LINEAR_VALUE_USED_TWICE` | 線形な値を、ある経路で2回以上使った。2回目に使った位置を primary、1回目を secondary にする |
| E3003 | `LINEAR_VALUE_NOT_CONSUMED` | 線形な値を、ある経路で使わなかった。束縛した位置を primary、使わなかった枝、省いた `else`、またはスコープの終わりを secondary にする。どの経路でも使わないうちに同じブロックの後の `let` で隠されたときは、スコープの終わりではなく隠した束縛を secondary にし、隠す前に `drop` するよう help で伝える。help で `drop` を提案し、使わなかった経路がブロックなら、その最後の文の前に `drop x` の行を入れる fix を付ける。`drop x` を入れる位置で同じ名前の後の束縛が見えているときは、fix を付けない |
| E3004 | `LINEAR_VALUE_DISCARDED` | 線形な値を `_` で受けた。パターンを指す。状態のある handler で省いた `return` の節が `Lin` の状態を捨てた場合は、`from` の初期値を指し、状態を受ける `return` の節を書くよう help で伝える。fix は付けない |
| E3005 | `CONTINUATION_NOT_HANDLED` | `once` の操作の節の `k` を、ある経路で呼びも `drop` もしなかった。節を primary、`k` の束縛を secondary にし、すべての経路で `k` を呼ぶか `drop k` するよう help で伝える |
| E3006 | `LINEAR_VALUE_KEPT_ACROSS_MULTI` | 線形な値を持ったまま、`multi` の操作を起こしうる呼び出し、`handle` をまたいだ (持ち越し規則)。同じ値は、呼び出しの位置が最も前の1件だけを報告する。持ち越した値が handler の状態のときは、`from` の初期値を secondary にする。呼んだ関数のスキームを通る持ち越しの違反は、そのスキームに残した組ごとに1件で、呼んだ関数の中で位置が最も前の持ち越しを指す |

シグネチャに関する E1xxx の診断には、シグネチャの追加を提案する help を付ける ([宣言](../spec/declarations.md))。

## 型エラー

制約の由来 (引数の位置、`if` の各枝、型注釈など) をもとに、「expected / found」と、その根拠になった場所を示すラベルを出す ([型と Kind](../spec/types.md))。

単一化の失敗が矢印の線形性の食い違いだけによるときは、書き出す型に線形性が出ないので、expected と found が同じ表示になる。このときの E2001 には、どちらの関数型が1回しか呼べず、どちらが何度でも呼べるのかは言わずに、2つの関数型は呼べる回数が違う、という note を付ける。矢印の食い違いはどちら側が線形でも起こり、単一化は最初に失敗した子で止まるので、片方を名指しすると誤るため。

## 線形性の診断

| 誤り | 指す場所 |
|---|---|
| 二重使用 | 1回目に消費した場所と、2回目に使った場所 |
| 消費されていない | 束縛した場所とスコープの終わり。同じブロックの後の `let` で隠されたときは、隠した束縛。help と fix で `drop x` の追加を提案する |
| `_` で `Lin` の値を受けた | そのパターン。help で、変数に束縛して `drop` するよう提案する。省いた `return` の節が `Lin` の状態を捨てたときは、`from` の初期値 |
| `multi` の呼び出しをまたぐ | primary はその呼び出し、または `handle` で、起こしうる `multi` の操作を書く。secondary は値 (変数の束縛、途中の値の部分式、`return` の節) と、`multi` と宣言した操作。値が handler の状態のときは、`from` の初期値を値の secondary にする。help で、呼び出しの前に使い終えるよう提案する |
| 呼んだ関数が `multi` の呼び出しをまたがせる | primary は呼んだ関数を参照した位置。secondary は、呼んだ関数の中で値をまたがせている呼び出し、またはその先の関数の参照 (1段だけ) |
| 継続の扱い忘れ | `k` を呼びも `drop` もしていない handler の節 |
| 射影・更新で `Lin` な値を捨てる | 射影または更新の式。help と fix で、分解パターンへの書き換えを提案する ([直積型とレコード](../spec/records.md)) |

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
