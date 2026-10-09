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
| E1001 | `UNDEFINED_NAME` | 未定義の値の名前。修飾した名前と import の並びでは、メッセージにモジュールの名前を書く (「in module `Report.Csv`」)。修飾子がユーザーのモジュール (`Fs`) を指し、そのモジュールに隠された同じパスの標準ライブラリのモジュールが名前を `pub` で定義しているときは、help「the standard `Fs` is hidden by your module `Fs`; `import Std.Fs as F` reaches it」を付ける |
| E1002 | `UNDEFINED_TYPE` | 未定義の型の名前、未定義のエフェクトの名前、本体の注釈に書いたシグネチャにない型変数と row 変数。修飾した名前と import の並びでは、E1001 と同じくメッセージにモジュールの名前を書く。隠れた標準ライブラリのモジュールの help も E1001 と同じである |
| E1004 | `MISSING_SIGNATURE` | シグネチャのない等式。シグネチャの追加を提案する help を付ける |
| E1009 | `UNHANDLEABLE_EFFECT` | handler の節の先頭の名前が、extern のエフェクトを起こす extern の関数である。見出しは「`IO` cannot be handled」で、エフェクトの名前を入れる。ラベルは「`println` is an extern function with the effect `IO`」である。節の先頭の名前を指す。操作として引けなかったときだけ、extern の関数だけに絞って引き直す |
| E1013 | `MISSING_CLAUSE` | 節のない操作がある。操作の節が1つもない handler も含む。primary は `handle` で、節の追加を help で示す |
| E1017 | `DUPLICATE_BINDING` | 1つのパターン、または1つの等式の引数の並び、ラムダの引数の並び、あるいは handler の節の引数の並び (操作の引数と `k`) の中で、同じ変数名を2回束縛した。2つ目の束縛を primary、1つ目を secondary にする |
| E1018 | `NON_CONSECUTIVE_EQUATIONS` | 同じ名前の等式が連続していない (間に別の item がある)。離れた等式の関数名を primary、直前の等式の並びの最後の等式の関数名を secondary にする。等式をシグネチャの直後にまとめるよう help で伝える。instance の等式はシグネチャを持たないので、help は等式をまとめることだけを伝える |
| E1019 | `SIGNATURE_NOT_ADJACENT` | シグネチャと最初の等式が隣り合っていない。最初の等式の関数名を primary、シグネチャの関数名を secondary にする。等式をシグネチャの直後に置くよう help で伝える |
| E1020 | `EQUATION_ARITY_MISMATCH` | 等式ごとに引数の個数が違う。個数の違う等式の関数名を primary、最初の等式の関数名を secondary にする |
| E1021 | `DUPLICATE_FIXITY` | 同じ演算子への2回目の fixity の宣言。1つの宣言に同じ演算子を2回並べた場合を含む。2回目の宣言の演算子を primary、1回目を secondary にする |
| E1022 | `FIXITY_WITHOUT_DEFINITION` | このモジュールで定義していない演算子への fixity の宣言。標準ライブラリの演算子の fixity を変えようとした場合を含む。宣言の演算子を指す |
| E1023 | `INVALID_SECTION` | 優先順位の合わないセクション (`(* a + b)` や `(+ a + b)`)。セクション全体を指し、被演算子を括弧で囲むよう help で伝える |
| E1024 | `USE_AT_END_OF_BLOCK` | ブロックの最後の文が `use` である (包む残りがない)。`use` の文を指す |
| E1025 | `MISSING_CONSTRUCTORS` | `data` にコンストラクタがない (`=` のない `data`)。`data` の名前を指す。`extern data` は対象にならず、`extern` でなければ標準ライブラリを含むどのモジュールでも出す |
| E1026 | `MODULE_NOT_FOUND` | import したモジュールのファイルがない、または読めない (UTF-8 でない、IO の誤り)。読めない理由をメッセージに書く。import のモジュールのパスを指す。`import Std.Nope` はラベルを「there is no file `<std>/Nope.em`」にし、ユーザーの根は読まない。`import Fs` は、ユーザーの根にファイルが見つからないときだけ `std/` を探す。大文字小文字だけが違うファイルがあるか読めないときは、`std/` へ進まない |
| E1027 | `IMPORT_CYCLE` | import の循環。循環を閉じる import を指し、循環の経路を note で示す |
| E1028 | `AMBIGUOUS_NAME` | 修飾しない名前、または合流した修飾子の名前が、別々の定義を指して曖昧である。使った位置を primary にし、候補の import を secondary にする |
| E1029 | `PRIVATE_NAME` | ユーザーのモジュールの `pub` でない名前を、修飾か import の並びで使った。名前を primary にし、定義を secondary にする |
| E1030 | `RESERVED_MODULE` | 修飾子が `Prelude` になる import、`import Std.Prelude` (別名があってもなくても。別名を選ぶ help は付けない)、`import Main`、入口のファイルを指す import。import を指す。`import Std` はラベルを「`Std` is the root of the standard library」にする。入口のファイルを指す import の検査はユーザーの import にだけかけ、ほかの検査は標準ライブラリの import にもかける (標準ライブラリのモジュールの `import Prelude` も E1030) |
| E1031 | `UNKNOWN_QUALIFIER` | 修飾子がどの import にもない。2つ以上のセグメントの修飾子 (`Report.Csv.parse`) を含む。修飾子の全体を指し、そのモジュールを import していれば、使える修飾子 (`Csv.parse`) を help で示す。import が作る修飾子にないときは、標準ライブラリのモジュールの短い名前 (`Fs` など) を引いてから E1031 にする。短い名前はユーザーのモジュールでだけ使える。修飾子が `Std.Fs` のように標準ライブラリのモジュールの正式な名前なら、`Fs.close` と書くか `import Std.Fs as F` とする help を付ける |
| E1032 | `PRIVATE_IN_PUBLIC` | `pub` の item の型に、同じモジュールの `pub` でない型かエフェクトが現れた ([モジュールと名前解決](../spec/modules.md) の「公開の範囲」)。非公開の型かエフェクトの名前を primary にし、その定義を secondary にして、`pub` を付けるよう help で伝える |
| E1033 | `EXTERN_OUTSIDE_STD` | ユーザーのモジュールに `extern` を書いた。見出しは「`extern` is only allowed in the standard library」で、ラベルを `extern` のキーワードに付ける (``user modules cannot declare externs``)。「`extern` を外す」ことは help で示し、自動の修正にはしない。外すと E1005 や E1025 になるためである。宣言ごとに1つで、宣言を使った位置には重ねない。宣言は extern として読むので、E1005、E1025、`where` がないことの誤りは重ねて出さない。instance の中の `extern` の行も、行ごとに同じ文言で `extern` のキーワードを指す。その行のメソッドは定義したものに数え、E1036 を重ねない |
| E1034 | `ORPHAN_INSTANCE` | 見出しは「an instance of `C` for `T` must be in the module of `C` or of `T`」で、instance の頭を指してラベルを「neither is defined in this module」にする。この instance は置かないので、メソッドの本体の誤りを重ねない |
| E1035 | `DUPLICATE_INSTANCE` | 見出しは「`T` already has an instance of `C`」である。2つ目の instance の頭 (導出なら `deriving` のクラス名) を primary にして「defined again here」、1つ目を secondary にして「first defined here」とする。2つ目は置かない |
| E1036 | `MISSING_METHOD` | 見出しは「the instance of `C` for `T` does not define `m`」で、instance の頭を指してラベルを「`m` has no default」にする。help は「add an equation for `m`」である。定義していないメソッドごとに1つ出す |
| E1037 | `UNKNOWN_METHOD` | 見出しは「`m` is not a method of `C`」で、等式の名前を指してラベルを「not declared in the class」にする |
| E1038 | `NOT_DERIVABLE` | 見出しは「`C` cannot be derived」で、`deriving` に書いたクラスの名前を指してラベルを「only `Eq`, `Ord` and `Show` can be derived」にする |
| E1039 | `INVALID_INSTANCE_HEAD` | 見出しは「an instance head must be a type constructor applied to distinct type variables」である。ラベルは誤りの形ごとに変える。頭の全体を指す「a type variable」「tuples have only built-in instances」「a function type」「`Unit` has only built-in instances」と、頭の型引数を指す「not a type variable」「`a` appears more than once」である。型引数の数の誤りは E1015 で、型の名前がクラスなら E1043 である |
| E1040 | `INVALID_CONSTRAINT` | 誤りの形ごとに見出しとラベルを変える。extern と操作のシグネチャの文脈は「an extern declaration cannot have constraints」「an effect operation cannot have constraints」で、文脈の全体を指して「remove this context」とする。型に現れない型変数への制約は「the constraint on `a` is ambiguous」で、型変数を指して「`a` does not appear in the type」とする。メソッドがクラスの型変数に書いた制約は「a method cannot constrain the class variable `a`」で、制約を指して「remove this constraint」とし、help で「the class already requires `C a`」と示す。上位クラスの文脈は「a superclass constraint must be on the class variable `a`」で、型変数を指して「not the class variable」とする。instance の文脈は「the context of an instance can only constrain the variables of its head」で、型変数を指して「not a variable of the head」とする。クラスの型変数を含まないメソッドは「the method `m` does not mention the class variable `a`」で、シグネチャの型を指して「`a` does not appear in this type」とする。型に現れないことを根拠にするこの誤りと曖昧な制約の誤りは、シグネチャの型に誤り (構文の誤り、解決できない名前、row の誤り) があれば重ねない。誤った型からは型変数が落ちているためである。制約の形の誤りは「a constraint must be a class applied to one type variable」で、制約を指して「not of the form `C a`」とする |
| E1041 | `NOT_A_CLASS` | 見出しは「`X` is not a class」で、名前を指してラベルを「a type, not a class」か「an effect, not a class」にする |
| E1042 | `SUPERCLASS_CYCLE` | 見出しは「the superclasses of `C` make a cycle」で、循環を閉じる辺を持つクラスの名前を指してラベルを「this class closes the cycle」にする。note で循環の経路を示す (「the cycle is `Left` -> `Right` -> `Left`」) |
| E1043 | `CLASS_AS_TYPE` | 見出しは「`C` is a class, not a type」で、名前を指してラベルを「a class cannot be used as a type」にする |
| E2001 | `TYPE_MISMATCH` | 型の不一致。メッセージとラベルは制約の由来ごとに変える ([型と Kind](../spec/types.md))。呼び出しの row のエフェクトの型引数が今の row と一致しないときも E2001 にし、呼び出しを primary にする |
| E2002 | `EFFECT_NOT_IN_ROW` | シグネチャの row に含まれないエフェクトを起こした。シグネチャの矢印を指し、row を足す help を付ける。引数のない関数には、`()` を取る関数にする help を付ける。ただし、引数なしで定義した instance のメソッドと既定のメソッドのシグネチャは、クラスが決めるので変えられない。メソッドのシグネチャに矢印があれば help「define `m` with its parameters, so that it performs `E` when it is called」を付け、なければ help を付けない。引数を書いて定義した instance のメソッドと既定のメソッドには、クラスのシグネチャの row を足す help「the signature of `m` comes from the class `C`; add `E` to its row there, as in `-> <E> ...`」を付ける。クラスが標準ライブラリにあれば書き換えられないので、help を付けない。ラムダの本体の場合は、エフェクトを起こした場所を primary、ラムダの期待する型の由来 (シグネチャの引数の型や型の明示) を secondary にする |
| E2005 | `INFINITE_TYPE` | 無限の型 (単一化の occurs check)。row のラベルの型引数を通して、型変数か row 変数が自分自身の中に現れる場合を含む。呼び出しの row で起きたときは、呼び出しを指す |
| E2006 | `NO_INSTANCE` | 見出しは、解けなかった制約 (葉) について「no instance of `C` for `T`」である。`==` の位置に出る場合も、演算子でなくクラスの言葉で言う。出し方は場所ごとに4つある。(1) 参照の制約: 参照を指し、ラベルを「`name` requires `<根の制約>`」にする。根は参照が求めた制約 (`Show (Option Color)`) である。葉が根と違えば、note「`<根の制約>` needs `<葉の制約>`」を付ける。ラベルは根だけを書くので、note で根から葉への道を示す。葉がシグネチャの型変数で、本体が普通の関数 (instance のメソッドや既定のメソッドでない) なら、help「add `Eq a =>` to the signature of `f`」を付ける。操作ごとの型変数はシグネチャに書けないので、help を付けない。(2) 上位クラスの instance がない: instance の頭を指し、ラベルを「`Ord` requires `Eq`, its superclass」にする。(3) 上位クラスの instance の文脈が導けない: 見出しは「no instance of `C` for `a`」で、頭を指してラベルを「the instance of `Eq` for `T` requires `C a`」にし、help「add `C a` to the context of this instance」を付ける。導出した instance には文脈を書き足せないので、help を「a derived instance cannot have `C a` in its context; write the instance by hand」にする。(4) 導出した instance のフィールド: `deriving` のクラス名を指し、ラベルを「`deriving Show` needs it for a field of `K`」にして、note「the field of type `F` needs `Show F`」を付ける。報告は instance ごとに最初のフィールドの1つで、頭が E2010 なら出さない。制約の表示は、型引数のある型構成子と関数型を括弧で囲む (`Show (Option Color)`) |
| E2008 | `MASK_CONFLICT` | 呼び出し先が自分で起こすエフェクトを、同じ呼び出しで row 変数のために飛ばす必要がある。primary は呼び出しの式で、メッセージには、呼び出し先がそのエフェクトを自分で起こし、row 変数を通して外側の handler にも渡すことを書く。今の row がラムダの row でなくシグネチャの row で、シグネチャの本体の矢印の row が row 変数の手前にそのエフェクトを並べているときだけ、その矢印を secondary にして、そのエフェクトを並べていることを示す。余ったのが囲む handle の足したラベルだけなら、シグネチャの row は余りに関わらないので secondary を付けない。note で、呼び出し自身のそのエフェクトはいちばん内側の handler に届くが、row 変数を通るほうはその handler を飛ばさなければならないことを示す |
| E2009 | `AMBIGUOUS_CONSTRAINT` | 見出しは「cannot decide which instance of `C` to use for `name`」で、参照を指してラベルを「the type here is never decided」にし、help「add a type annotation」を付ける。本体にほかの誤りがあれば出さない。同じ参照の制約に instance がないもの (E2006) があれば、E2006 だけを出す |
| E2010 | `LINEAR_INSTANCE_HEAD` | 見出しは「`T` is linear, so it cannot have an instance of `C`」で、instance の頭 (導出なら `deriving` のクラス名) を指してラベルを「a linear type」にする。note は「the methods of a class may copy or drop their arguments, which a linear value forbids」である。頭が線形な instance のメソッドの本体には、線形性の誤り (E3xxx) を重ねない。本体は頭の値を `Unr` とみなして書いたものだからである |
| E2011 | `METHOD_KIND_MISMATCH` | 見出しは、instance のメソッドなら「`m` in the instance for `T` needs more than the signature of `m` allows」、既定のメソッドなら「the default `m` needs more than the signature of `m` allows」である。定義の最初の等式の名前を指してラベルを「this definition」にする。note は「the signature of a method leaves its own type variables free to be linear, and this definition copies, drops or keeps a value of such a type」である。本体に誤りがあれば出さない |
| E2012 | `CONSTRAINED_POLYMORPHIC_RECURSION` | 見出しは「`name` would need an instance of `C` at infinitely many types」である。`name` は、関数ならその名前、instance なら `Same (Box a)` のようなクラスと頭である。関数ならシグネチャの名前を、instance のメソッドと instance の節点なら instance の頭を指し、ラベルを「a constrained type variable grows on each recursive call」にする。note は「instances are chosen at compile time, so a constraint cannot follow polymorphic recursion」である。大きくなる成分ごとに1つだけ出し、成分のほかの位置を secondary にしてラベルを「also grows here」にする。ほかの成分で示した位置は重ねない |
| E3001 | `LINEAR_VALUE_MISUSED` | 線形な値の誤った使い方のうち、E3002〜E3005 に当たらないもの (関数への受け渡し、型の単一化、ラムダや節の捕獲)。違反した Kind の制約の由来を指す |
| E3002 | `LINEAR_VALUE_USED_TWICE` | 線形な値を、ある経路で2回以上使った。2回目に使った位置を primary、1回目を secondary にする |
| E3003 | `LINEAR_VALUE_NOT_CONSUMED` | 線形な値を、ある経路で使わなかった。束縛した位置を primary、使わなかった枝、省いた `else`、またはスコープの終わりを secondary にする。どの経路でも使わないうちに同じブロックの後の `let` で隠されたときは、スコープの終わりではなく隠した束縛を secondary にし、隠す前に `drop` するよう help で伝える。help で `drop` を提案し、使わなかった経路がブロックで、その最後の文がその行の最初のトークンなら、最後の文の前に、同じ字下げで `drop x` の行を入れる fix を付ける。最後の文の前に同じ行のほかの文やコメントがあるときと、`drop x` を入れる位置で同じ名前の後の束縛が見えているときは、fix を付けない |
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
| E4002 | 網羅されていない等式 | Error | primary はシグネチャの関数名、secondary は各等式の先頭。漏れている引数の並びの例 (`f None _`) を note で示し、fix で最後の等式の後への等式の追加を提案する (fix は、どの型にもなる仮置きの式を言語に入れるときに一緒に入れる) |
| E4004 | 到達しない枝 | Warning | その枝のパターン |
| E4005 | 到達しない等式 | Warning | その等式の引数のパターン |
| E4003 | 反駁可能な `let` / ラムダの引数 / handler の節と `return` の節の引数のパターン | Error | そのパターン |

等式の場合はソースに `match` 式がないので、指す場所を等式に合わせて決めている。handler の節はラムダと同じく持ち上げる関数で、引数のパターンも同じ経路でコンパイルするため、ラムダの引数と同じ E4003 にする。

漏れているパターンの例は、note に最大3つ並べる。3つより多ければ、残りがあることを書き添える。
