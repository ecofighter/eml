# 縦の貫通 段階3a: エフェクトと handler の設計

位置づけ: 作業用の設計文書。段階3a を終えたら、残す価値のある内容を `docs/spec/`、`docs/implementation/architecture.md`、`docs/implementation/status.md` に移し、この文書は削除する。

## 目的と範囲

[status.md](../../implementation/status.md) の「名前解決以降の実装段階」の段階3 (`effect`、`handle`、`resume`、`drop k`) を、3a と 3b の2回に分ける。この文書は 3a の設計である。

3a では、ユーザー定義のエフェクトの `never` と `once` の操作、deep handler、`resume`、`drop` を、HIR、型検査、Core IR、ランタイム、インタプリタの全体に通す。言語の実験の中心である「継続の捕獲、ヒープ上のフレーム、RC」の組み合わせを先に確かめ、各回の大きさを R3a / R3b 程度に収めるために分けた。

### 3a に含めるもの

- `effect` の宣言 (型引数なし)。操作の多重度は `never` と `once`。カリー化した操作も含む
- 操作の呼び出し。引数の揃った呼び出しに加えて、操作を値として参照することと部分適用も含む
- `handle ... with` (deep handler)、入れ子の handler、同じエフェクトの handler の入れ子 (scoped labels により内側の handler が処理する)
- `resume k v` と `drop k`。末尾の位置の `resume` は、末尾呼び出しと同じくフレームを積まない
- 一般の `drop e`。実行時には「所有している参照を1つ手放す」命令で、`drop k` と同じ命令になる。`k` だけに制限すると、かえって特別扱いが要る
- handler に `IO` の操作の節を書いたときのエラー
- Kind の制約の違反を E3001 の診断にする

### 3b に回すもの

- `multi` の操作と multi-shot の再開
- エフェクトの型引数 (`effect State s`)

3a では、どちらも書いたら E0004 にする。

### 他の段階のまま残すもの

- 持ち越し規則と `File` (段階5)
- `from` によるパラメータ付き handler (段階6)。引き続き E0004 にする
- diagnostics.md の「線形性の診断」の表どおりの指し方 (二重使用の2か所、扱い忘れの節など)。3a の E3001 は、変数の束縛を指す一般的な形にとどめる (段階5)

### ユーザーと合意済みの決定

- 段階3を 3a と 3b に分け、範囲は上のとおりにする
- `once` の `k` は `Lin` の Kind で型付けし、誤用は既存の使用回数のパスと Kind の制約で見つける。違反は E3001 にする
- 1つの handler は1つのエフェクトを扱い、そのエフェクトのすべての操作に節が要る (Koka と同じ)。複数のエフェクトは handler を入れ子にして扱う
- `k` は第一級の値で、専用の型を持つ。変数に束縛し、関数に渡し、クロージャで捕まえられる。再開は `resume`、破棄は `drop` だけで行う。型の名前は表面の構文に出さない
- handle の本体と節をクロージャに持ち上げ、`perform` で handler フレームの `next` を切り離し、`resume` でつなぎ直す (案A)。フレームの並びを毎回写す案 (案B) と、本体を同じ関数の中で実行する案 (案C) は採らない
- `drop k` の後始末は所有に任せる。線形性の検査パスのクリーンアップ情報はなくし、spec から削る

## 1. HIR と名前解決

### item

- `Module::effects` の `EffectDef` に、操作の並び (`Vec<OperationId>`) を持たせる。
- 操作は新しいアリーナ `Module::operations` (`OperationId`) に置く。`Operation` は、名前、属するエフェクト、多重度、シグネチャ (`Signature`。型変数は `Generics` に入る)、引数の個数 (シグネチャの一番外側の `->` の数)、範囲を持つ。
- 値の名前空間の `ItemScope` は、関数と操作を両方引けるようにする。`Res` に `Operation(OperationId)` を足す。
- 操作名と関数名の重複、別のエフェクトの操作名との重複は、今の E1003 で報告する。エフェクト名と型名の重複も E1003 にする ([modules.md](../../spec/modules.md) の「名前空間」)。
- 操作のシグネチャの型変数は、操作ごとに暗黙に量化する。
- エフェクトの型引数と `multi` は E0004 にする (3b)。

### 式

```rust
pub enum ExprKind {
    // ...
    /// `effect` は節から決めたエフェクト。決められなかったら `None` で、型検査は連鎖する診断を出さない。
    Handle {
        body: ExprId,
        effect: Option<EffectId>,
        clauses: Vec<OpClause>,
        ret: Option<ReturnClause>,
    },
    Resume { k: ExprId, arg: ExprId },
    Drop(ExprId),
}

pub struct OpClause {
    pub op: OperationId,
    pub params: Vec<PatId>,
    /// `never` の操作の節は `k` を持たない。
    pub k: Option<PatId>,
    pub body: ExprId,
    pub range: TextRange,
}

pub struct ReturnClause {
    pub param: PatId,
    pub body: ExprId,
    pub range: TextRange,
}
```

- 節の引数のパターンは、今のラムダの引数と同じ範囲 (変数、`_`、`()`、型の明示) を扱う。タプルのパターンは段階4なので、今と同じく E0004 にする。
- `Body` の走査関数 (子の式、パターンの束縛、ラムダが捕まえる変数) を、handle の本体と各節に広げる。節の捕獲は、持ち上げと使用回数のパスが使う。

### 節の先頭の名前の解決

節の先頭の名前は、エフェクトの操作だけから引く。ローカルの変数が同じ名前で操作を隠していても、解決には影響しない ([modules.md](../../spec/modules.md) の「名前の解決」)。操作でない名前のときは次のようにする。

- `IO` の操作 (組み込みの表で `IO` の操作と分かるもの) なら E1009 を出す
- それ以外なら E1001 で「エフェクトの操作が見つからない」と報告する

### 診断

| 番号 | 定数 | 内容 |
|---|---|---|
| E1007 | `INVALID_OPERATION_SIGNATURE` | 操作のシグネチャの一番外側の `->` に row を書いた (カリー化した操作では外側のすべての `->` が対象。[declarations.md](../../spec/declarations.md) の「`effect`」)。または、シグネチャが関数型でない。引数のない操作には、エフェクトの row を付ける矢印がないためである |
| E1008 | `NEVER_RESULT_NOT_FREE` | `never` の操作の結果の型が、引数に現れない型変数でない |
| E1009 | `UNHANDLEABLE_EFFECT` | handler に `IO` の操作の節を書いた |
| E1010 | `CLAUSE_ARITY` | 節の引数の個数の誤り。`never` の操作は「操作の引数の個数」、それ以外は「操作の引数の個数 + 1」。`return` の節の引数が2つで `from` がない場合も含む |
| E1011 | `KEYWORD_ARITY` | `resume` と `drop` の引数の個数の誤り。`resume` は2つ、`drop` は1つ。`resume` の3引数の形だけは段階6の構文なので E0004 にする |
| E1012 | `MIXED_EFFECTS_IN_HANDLER` | 1つの handler に、別のエフェクトの操作の節が混ざった |
| E1013 | `MISSING_CLAUSE` | 節のない操作がある。操作の節が1つもない handler もここに含める |
| E1014 | `DUPLICATE_CLAUSE` | 同じ操作の節、または `return` の節が2つある |

- E1008 について。spec は「結果の型を自由な型変数として書く」と定める。`never f : a -> a` のように結果の型変数が引数にも現れると、呼び出した側で「どんな型として使ってもよい」が成り立たないので、これも誤りに含める。
- E1012 では、最初の節の操作のエフェクトを handler のエフェクトとし、それと違う節を primary、最初の節を secondary にする。
- E1013 は、primary を `handle` のキーワードにし、漏れている操作名を note に並べ、節の追加を help で示す。操作を1つも持たないエフェクトは handle できないことになるが、3a ではそのままにする。

## 2. 型検査

### 操作の型

- 操作のスキームは、シグネチャの型の、引数の個数の分だけたどった最後の矢印に、閉じた row `<E>` を付けたものである。`E` はその操作のエフェクトである。
- 呼び出しは、組み込みの関数と同じ経路を通る。値として参照するときも、組み込みと同じく戻り値の側の閉じた row を新しい row 変数で開く ([types.md](../../spec/types.md) の「推論」)。
- `Table::effect_multiplicity` は、今の「`IO` だけ」の仮の実装をやめ、エフェクトの操作の多重度の最大を返す。

### 継続の型

- 型の表に継続の型を足す。持つものは、操作の結果の型 `a`、handle の結果の型 `b`、handle の外側の row `ρ`、線形性の4つである。`once` の操作の `k` の線形性は `Lin` である。
- 診断では `Cont Int Unit <IO>` のように表示する。この名前は表示のためだけのもので、シグネチャには書けない。
- `TypedModule` に書き出すときは、`eml_types::Type` に `Cont` の種類を足す。

### handle の検査

handle 式の期待する型を `τ`、その位置の row を `ρ` とする。

- 本体は row `<E | ρ>` で検査し、型 `σ` を得る。
- `return` の節は、引数が `σ`、本体が `τ`、row が `ρ` である。省略したら `σ = τ` を課す。
- 操作 `op : A1 -> ... -> An -> R` の節では、操作の型変数を、その節だけの rigid な変数で具体化する。handler は、どんな型で呼ばれても動かなければならないためである。引数のパターンは `Ai`、`k` は `Cont R τ ρ`、本体は row `ρ` のもとで `τ` である。
- `effect` が `None` (HIR がエフェクトを決められなかった) なら、本体を末尾が `Error` の row で検査し、handle の型を `Error` にする。連鎖する診断を出さないためである。

### `resume` と `drop`

- `resume k v` は、`k : Cont a b ρ'`、`v : a` で、値の型は `b`、起こすエフェクトは `ρ'` である。`a -<ρ'>-> b` の関数の呼び出しと同じ扱いになる。
- `drop e` は、`e` が任意の型で、値は `Unit` である。

### 使用回数と E3001

- 使用回数のパスでは、handle の本体と `return` の節を、ラムダと同じく「捕獲はその位置での1回の使用、中の使用は別に数える」扱いにする。この2つは高々1回しか動かないので、`Lin` の値も捕まえられる。
- 操作の節は、handler が生きている間に何度も呼ばれうる。そのため、操作の節が捕まえる変数には、使用の回数によらず `Unr` の制約を出す。これにより、内側の handler の操作の節が外側の節の `k` を捕まえて何度も再開することを防ぐ。
- Kind の制約に由来を持たせる。由来は、使用回数のパスが出した制約なら「変数の束縛の範囲と理由 (2回以上使った、使わなかった、`_` で捨てた、操作の節が捕まえた)」、スキームの具体化で複写した制約なら「参照の範囲」、型の単一化で出た制約なら「単一化した式の範囲」である。
- 由来は、型の表が「今の由来」として持ち、制約を作るときに記録する。`Lattice::require` の引数は変えない。本体の検査と使用回数のパスが、制約を作る処理の前後で今の由来を設定する。
- 本体に `Missing` (報告済みの誤りの跡) がある関数では、使用回数のパスは由来を記録しない。E1011 などで捨てた式の中の変数が「使っていない」と数えられ、E3001 が連鎖するのを防ぐためである。由来のない制約が破れても診断は出さない。誤りのあるプログラムは実行しないので、困ることはない。
- `solve_kinds` は、破れた制約の由来を返す。`check` は、今の `debug_assert` の代わりに、それぞれを E3001 (`LINEAR_VALUE_MISUSED`) の診断にする。primary は由来の範囲で、理由をラベルにする。
- E3001 は `eml_types::codes` に置く。E3xxx の範囲は線形性の段階の番号だが、3a では型検査の後の Kind の解決で出すので、型検査の `codes` に置く。段階5で線形性の検査パスを分けるときに置き場所を見直す。

### Core IR への受け渡し

- `TypedModule` に操作のスキームを足す。Core IR の boxed の判定に使う。
- Core IR の `VarInfo::linearity` は 3a では `Unr` のままにする。型検査を通ったプログラムでは `k` はちょうど1回だけ使われるので、Perceus はもともと RC 命令を付けない。`drop k` は `decref` になり、RC が1なので解放される。`Lin` を Core IR に流すのは段階5の作業として status.md に残す。

## 3. Core IR

### 持ち上げ

handle 式の各部分を、ラムダと同じ方法 (捕まえた変数を先頭の引数に持つ関数) で持ち上げ、`MakeClosure` でクロージャにする。

| 部分 | 持ち上げた関数の引数 |
|---|---|
| 本体 | 捕まえた変数、`()` |
| 操作の節 | 捕まえた変数、操作の引数、`k` (`never` なら `k` なし) |
| `return` の節 | 捕まえた変数、`x` |

### 命令

`handle`、`perform`、`resume` を、`Call` の新しい種類として足す。

```rust
pub enum Call {
    Direct(FnIdx, Vec<Atom>),
    Apply(Atom, Vec<Atom>),
    /// 節のクロージャは、エフェクトの操作の順に並べる。`ret` が `None` なら値をそのまま返す。
    Handle {
        effect: u32,
        body: Atom,
        clauses: Vec<Atom>,
        ret: Option<Atom>,
    },
    Perform { effect: u32, op: u32, args: Vec<Atom> },
    Resume { k: Atom, arg: Atom },
}
```

3つとも「制御を移し、いずれ値が戻ってくる」点で呼び出しと同じである。`Call` に入れることで、次のものがそのまま使える。

- `Rhs::Call` の `saved` (後で使う変数の退避)。status.md の注意点にある「`perform` のフレームも後で使う変数だけを退避する」は、これで満たされる
- `CExpr::TailCall` による末尾の位置の扱い。末尾の `resume`、`handle`、`perform` はフレームを積まない
- Perceus と verifier の呼び出しの規則 (引数の所有権は呼び出しに移る、退避した変数の一致)

`IO` の操作は、今の `Rhs::Perform(IoOp, ...)` のままその場で実行する ([core-ir.md](../../spec/core-ir.md) の「インタプリタ」)。ユーザーのエフェクトの `Perform` と紛らわしいので、`Rhs::Io` に改名する。

### その他

- `drop e` は、spec の Core IR の表にある `drop x` を `Rhs::Drop(Atom)` として足す。値は `()` で、Perceus には1回の使用に見える。
- 操作ごとに、引数を受け取って `Perform` を末尾で呼ぶラッパーの関数を作る。操作を値として参照したら、そのラッパーの `MakeClosure` にする。引数の揃った呼び出しは、直接 `Perform` にする。
- verifier は、新しい種類の `Call` を既存の呼び出しと同じ規則で確かめる。加えて、`Handle` の節の数がエフェクトの操作の数と一致することを確かめる。そのため、`Program` にエフェクトごとの操作の表 (名前と、再開できるか) を持たせる。インタプリタも、この表で `never` の操作かどうかを引く。
- 節の引数の型を引くために、`BodyTypes` にパターンの型 (`pats`) を足す。
- 表示は `handle E(body) [clauses] return r`、`perform E.op(args)`、`resume k(v)`、`drop x` のような形にする。細部は計画で決める。

## 4. ランタイムとインタプリタ

### オブジェクト

```rust
pub enum Frame {
    Return { /* 今のまま */ },
    Apply { /* 今のまま */ },
    /// handle の handler。`next` が `None` なのは、継続に捕まえられて外側から切り離されている間である。
    Handler {
        effect: u32,
        clauses: Vec<Value>,
        ret: Option<Value>,
        next: Option<ObjRef>,
    },
    Io,
}

pub enum Payload {
    Str(String),
    Closure(Closure),
    Frame(Frame),
    /// `once` の操作の継続。`top` から `next` をたどった先に `handler` がある。
    Continuation { top: ObjRef, handler: ObjRef },
}
```

- `Frame::Handler` の子は、節のクロージャ、`return` の節、`next` (あれば) である。
- `Payload::Continuation` の記述子は `continuation` にする。所有するのは `top` だけで、`handler` は参照を所有しない。継続を解放すると、`top` から H までの区間が子をたどって解放され、H の `next` が `None` なのでそこで止まる。

### 命令の動き

| 命令 | 動き |
|---|---|
| `Handle` | `Frame::Handler` (`next` は今の継続) を積み、本体のクロージャに `()` を適用する |
| `ret` が handler フレームに届いた | フレームを外して節のクロージャを `decref` し、`return` の節があれば値に適用する。なければ値をそのまま次へ返す |
| `Perform` | 継続の連結リストを先頭から読み (書き換えない)、同じエフェクトの一番内側の handler フレーム H を探す。節のクロージャを `dup` し、H の `next` を取り出して機械の継続にする。`once` の操作は `Continuation { top, handler: H }` を作り、引数と `k` を節に適用する。`never` の操作は区間の先頭を `decref` して解放し、引数だけを節に適用する |
| `Resume` | 継続オブジェクトを `take` し、H の `next` に今の継続を入れ、機械の継続を `top` にして値を返す。末尾でない `resume` では、その前に `saved` の `Return` フレームが積まれている |
| `Drop` | ヒープの値なら `decref` する。継続なら、これで区間がまるごと解放される |

- 3a の継続はどれも `once` なので、区間のフレームは常に一意である。今の `ret` が `take` でフレームを取り出す前提は崩れない。3b の multi-shot では、`Resume` が `take_or_copy` で区間を写す。
- H の `next` の書き換えは、一意なフレームに対してだけ行う。一意なら書き換えても観測できないので、spec の「イミュータブルなフレームの連結リスト」とは Perceus の reuse と同じ理屈で両立する。
- 型検査を通ったプログラムでは、handler の見つからない `Perform` は起きない。起きたら `Fault::Internal` にする。

### 計算量

`Perform` は handler までの区間の長さに比例する。`Resume` と `Drop` (解放そのものを除く) は O(1) である。末尾の `resume` はフレームを積まないので、`perform` と `resume` を10万回繰り返すループでも継続は伸びない。

## 5. テスト、文書、成功の条件

### 変わるテスト

既存のテストで、段階3の構文に E0004 を期待しているものはない。未定義のエフェクトの E1002 のテストは、3a の後もそのまま成り立つ。そのため、種類1 (振る舞い) と種類2 (内部表現のスナップショット) の変更は出ない見込みである。

| テスト | 種類 | 変更 |
|---|---|---|
| `EffectDef` を組み立てるテスト (`eml_types` の `ty.rs` と `table/tests.rs`、`eml_hir` の `lower/scope.rs`) | 3 | `operations: Vec::new()` を足す。期待値は変えない |
| `crates/eml_types/src/table/tests.rs` の `new_table` | 3 | `Table::new` に操作のアリーナを渡す。期待値は変えない |
| `Program` を組み立てるテスト (`eml_core_ir/tests/verify.rs`、`eml_interp/tests/`) | 3 | `effects: Vec::new()` を足す。期待値は変えない |
| `crates/eml_interp/tests/closures.rs` | 3 | `Rhs::Perform` を `Rhs::Io` に改名する。期待値は変えない |
| enum の網羅的な `match` を書いたテスト (`Frame`、`Call`、`Rhs`、`ExprKind` など) | 3 | 新しい種類に追随する。期待値は変えない |

上の表にないテストの期待値が変わった場合は、変えずに止まり、差分と理由をユーザーに示して承認を得る。

### 足すテスト

UI テスト `run/` (どれも `debug_heap` が有効)

| ファイル | 確かめること |
|---|---|
| `effect_abort.em` | `never` の操作で本体を中断する。中断したフレームが退避していた文字列も解放される |
| `effect_resume.em` | `once` の操作の末尾の `resume` と、末尾でない `resume` (再開した後に節が続きを実行する) |
| `effect_deep.em` | deep handler で同じ操作を何度も呼ぶ。同じエフェクトの handler の入れ子では内側が処理する。節の中から外側のエフェクトを起こす |
| `effect_drop_k.em` | `drop k` で、文字列を退避したフレームの区間がリークなく解放される |
| `continuation_values.em` | `k` を関数に渡して再開する。`k` をクロージャで捕まえて再開する |
| `operation_values.em` | 操作を値として参照する。カリー化した操作を部分適用する |
| `effect_loop.em` | 10万回の `perform` と末尾の `resume`。join point をまたぐ `perform` と `saved` の退避も含める |

UI テスト `check-fail/`

| ファイル | 確かめること |
|---|---|
| `handler_clauses.em` | E1010、E1012、E1013、E1014 を1回の実行でまとめて報告する |
| `operation_declarations.em` | E1007、E1008、操作名の重複 (E1003) |
| `handle_io.em` | E1009 |
| `resume_and_drop_arity.em` | E1011 |
| `continuation_misuse.em` | E3001 の3つの理由 (2回再開した、使わなかった、内側の操作の節が捕まえた) |
| `unhandled_effect.em` | handle していないユーザーのエフェクトを `main` が起こす (E2002) |
| `later_stage_effects.em` | `multi`、エフェクトの型引数、`from` が E0004 になる |

crate ごとのテスト

- `eml_hir`: handle、`resume`、`drop` の HIR の表示のスナップショット。節の先頭の名前の解決が、ローカルの変数による隠蔽を見ないこと
- `eml_types`: 操作のスキーム、`k` の型、操作の型変数が節で rigid になること、本体の row に `E` が足されること、Kind の制約の由来
- `eml_core_ir`: 持ち上げた handle の Core IR のスナップショット (新しいテスト)。新しい `Call` の verifier の規則
- `eml_runtime`: 継続オブジェクトの解放が handler フレームで止まること。`Frame::Handler` の子

### レビューで重点的に見るところ

- `perform` のときに退避した変数が、再開した後にも、`drop k` で解放するときにも、ちょうど1回ずつ扱われること (`effect_abort.em`、`effect_drop_k.em`、`effect_loop.em`)
- 継続オブジェクトが所有しない `handler` の参照が、区間を解放した後に使われないこと
- 末尾の `resume` で継続が伸びないこと (`effect_loop.em`)

### 文書

| 文書 | 変更 |
|---|---|
| `docs/spec/effects.md` | handler は1つのエフェクトを扱い、すべての操作に節が要ること。`k` は第一級の値で、型の名前を表面の構文に出さないこと。操作の節が捕まえる変数に `Unr` の制約が付くこと |
| `docs/spec/expressions.md` | 「handler」に、1つのエフェクトとすべての操作の節の規則、節の先頭の名前の解決の誤り (E1009、E1001) を書く |
| `docs/spec/core-ir.md` | 「perform し得る各呼び出しにクリーンアップ情報を付ける」と「`drop k` はクリーンアップ情報に従う」を削り、所有をたどる解放に直す。handle の本体と節の持ち上げ、`Call` の新しい種類、handler フレームと継続オブジェクト、末尾の `resume` を書く |
| `docs/spec/linearity.md` | 「線形性の検査パス」の出力 (クリーンアップ情報) の項を削る。操作の節の捕獲の規則を「基本の規則」に足す |
| `docs/spec/runtime.md` | 継続オブジェクトと、一意なフレームの `next` の書き換えを書く |
| `docs/spec/diagnostics.md` | E1007〜E1014 と E3001 を「割り当て済みの番号」に足し、「番号を割り当てていない診断」から handler の節と `resume` の引数の個数を外す |
| `docs/implementation/architecture.md` | 各 crate の内部に、操作の item、継続の型、Kind の制約の由来、持ち上げ、`Call` の新しい種類、handler フレームと継続オブジェクトを足す |
| `docs/implementation/status.md` | 段階3を 3a と 3b の行に分け、3a を完了にする。「次の作業の注意点」と「spec に反映済みで、実装は後の段階で扱うもの」の段階3の項目を整理し、3b の項目 (multi-shot の `take_or_copy`、エフェクトの型引数、row 変数の多重度の上限のテスト) と段階5の項目 (`Lin` を Core IR に流す、E3001 の指し方) を残す。「完了した作業」に 3a の行を足す |
| `docs/implementation/testing.md` | 種類1と種類2の変更があれば記録する |

### 成功の条件

- `effect` の宣言、操作の呼び出し、`handle`、`resume`、`drop` が E0004 にならず、上の UI テストが通る
- `never` と `once` の操作、deep handler、`drop k` が `debug_heap` を有効にして動き、リークも解放済みアクセスもない
- E1007〜E1014 と E3001 が上の UI テストで報告され、Kind の制約の違反で debug ビルドが panic しない
- `multi`、エフェクトの型引数、`from` は E0004 のままである
- 変わったテストは上の表のものだけである
- `cargo test`、`cargo clippy --all-targets`、`cargo fmt --check` が通る
- 上の「文書」の表の変更が済んでいる
