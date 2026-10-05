# 縦の貫通 段階4a: `data` と `match` の設計

位置づけ: 作業用の設計文書。段階4a を終えたら、残す価値のある内容を `docs/spec/`、`docs/implementation/architecture.md`、`docs/implementation/status.md` に移し、この文書は削除する。

## 目的と範囲

[status.md](../../implementation/status.md) の「名前解決以降の実装段階」の段階4を 4a と 4b に分け、4a を HIR、型検査、網羅性の検査、Core IR、ランタイム、インタプリタの全体に通す。

### 4a に含めるもの

- 型引数を持つ `data` の宣言、中置のコンストラクタ、型の適用 (`Option Int`)
- コンストラクタの値。部分適用と関数値を含む
- `match` と、コンストラクタ・変数・ワイルドカードの入れ子のパターン。パターンは `match`、`let`、ラムダの引数、等式の引数 (等式は1つだけ) に書ける
- 網羅性の検査 (Maranget の usefulness)。網羅されていない `match` と等式、反駁可能な `let` とラムダの引数、到達しない枝
- `Bool` を Prelude の `data Bool = | False | True` にする
- Core IR の join point の引数の並び、`Switch` の枝のフィールドの束縛、決定木によるコンパイル、`simplify` の B2 の作り直し

### 4b に残すもの

- タプル (数字ラベルの閉じたレコード) の式、パターン、型
- `Int` と `String` のリテラルのパターン
- `String` などの等値の扱いの決定。リテラルのパターンは実行時に `String` を比べるので、等値と同じ回で決める

### 後の段階に残すもの

- `Lin` のフィールドを持つ `data` と、それによってつねに `Lin` になる宣言 (段階5)
- 網羅されていない `match` と等式の fix (枝の追加の提案)。段階5の線形性の診断の fix (分解パターンへの書き換え) と一緒に入れる
- 到達しない等式の Warning (複数の等式を入れる段階6)
- 引数を持つコンストラクタの case-of-case。使われない束縛を消すパスと一緒に入れる

### ユーザーと合意済みの決定

- 段階4は 4a と 4b に分ける。分け方は上の範囲のとおり
- 引数のないコンストラクタは即値 `Value::Tag(n)` のままにし、引数を持つコンストラクタはヒープのオブジェクトにする
- `match` の枝のフィールドは `Switch` の枝が束縛する。`Switch` は scrutinee を move で受け取って分解する。射影の命令を別に置く案は、「変数の読み出しは move」の規則に借用の例外が要り、段階5で線形なフィールドを取り出す形とも合わないので採らない
- `match` は決定木にコンパイルし、各枝の本体をつねに join point にする。上から順に試す案は、同じ値を何度も調べ、そのたびに `dup` が増えるので採らない
- B2 は引数のないタグに限る。引数を持つコンストラクタに広げる案は、使われなくなった `let x = Con(...)` を消すパスがないと、箱の確保と `dup` と解放が増えてかえって遅くなるので、後に回す
- 網羅性の診断は note に漏れの例を出し、fix は後に回す

## 1. HIR と名前解決

### item

- `TypeDef` に `generics: Generics` と種類を持たせる。

```rust
pub struct TypeDef {
    pub name: String,
    pub generics: Generics,
    pub kind: TypeDefKind,
}

pub enum TypeDefKind {
    /// `Int` と `String`。
    Builtin,
    Data { constructors: Vec<ConstructorId> },
}
```

- `Module` に `constructors: Arena<Constructor>` を足す。

```rust
pub struct Constructor {
    pub name: String,
    pub range: TextRange,
    pub ty: TypeDefId,
    /// 宣言の順の番号。Core IR のタグになる。
    pub tag: u32,
    /// フィールドの型。宣言の `Generics` で解決する。
    pub fields: Vec<TypeRefId>,
}
```

- フィールドの型の参照は、`TypeDef` の `Generics` と同じ型の参照のアリーナに置く。宣言ごとのアリーナの持ち方は、エフェクトの宣言 (`EffectDef` の `generics` と操作のシグネチャ) に合わせる。
- コンストラクタは値の名前空間 (`ItemScope`) に置き、式の中の名前は `Res::Constructor(ConstructorId)` に解決する。関数や操作と同じ名前のコンストラクタは E1003 にする。型とコンストラクタは別の名前空間なので、同じ名前を持てる ([モジュールと名前解決](../../spec/modules.md))。
- HIR の表示 (`pretty`) では、`Res::Constructor` はコンストラクタの名前をそのまま書く。今の `Res::Builtin(Builtin::True)` の表示 (`True`) と同じになるので、`&&` と `||` の脱糖の表示は変わらない。

### 型の参照

- `TypeRefKind::Con(TypeDefId)` を `Con(TypeDefId, Vec<TypeRefId>)` にし、型の適用 (`Option Int`、`List (Option a)`) を受ける。今の「type applications are not supported yet」の E0004 を外す。
- 型引数の個数が宣言と違えば E1015 を出し、その型を `TypeRefKind::Error` にする。型検査で診断を連鎖させないためである。

### `data` の宣言

- 型引数の名前の重複 (`data P a a`) は E1003 にする (エフェクトの宣言と同じ)。
- フィールドに宣言にない型変数を書いたら E1002 にする。
- フィールドの関数型で row を省略したら `<>` にする ([型と Kind](../../spec/types.md))。
- 中置のコンストラクタ (`| a :+ b`) は、2つのフィールドを持つコンストラクタにする。fixity の宣言は段階6なので、`infixl 9` の既定のままである ([宣言](../../spec/declarations.md))。式とパターンの演算子の列の中の `:+` は、組み直した後にコンストラクタに解決する。
- 宣言の間の再帰と相互再帰を許す。型の名前は、宣言を読む前にすべて `ItemScope` に入れる。

### パターン

```rust
pub enum PatKind {
    Missing,
    Bind(LocalId),
    Wildcard,
    Unit,
    Annot { pat: PatId, ty: TypeRefId },
    Con { ctor: ConstructorId, args: Vec<PatId> },
}
```

- コンストラクタのパターン (`Some x`) と中置のコンストラクタのパターン (`h :+ t`) を `PatKind::Con` にする。今の「constructor patterns are not supported yet」の E0004 を外す。
- 引数の個数がコンストラクタのフィールドの数と違えば、新しい E1016 (`CONSTRUCTOR_ARITY`) を出し、そのパターンを `Missing` にする。未定義のコンストラクタは E1001 にする。
- 1つのパターンの中の変数名の重複 (`Pair x x`) と、等式の引数の名前の重複 (`f x x`) は、新しい E1017 (`DUPLICATE_BINDING`) にする。2つ目の束縛を primary、1つ目を secondary にする。status.md の「段階4: 引数名の重複」の項目をここで片付ける。
- リテラルとタプルのパターンは 4b に残し、今の E0004 のままにする。

### `match`

- `ExprKind::Match { scrutinee: ExprId, arms: Vec<MatchArm> }` を足し、今の「`match` is not supported yet」の E0004 を外す。

```rust
pub struct MatchArm {
    pub pat: PatId,
    pub body: ExprId,
}
```

- 各枝のパターンが束縛する変数は、その枝の本体だけで見える。
- `Body` の走査関数 (子の式、パターンの束縛、ラムダが捕まえる変数) に `Match` と `PatKind::Con` を足す。

### `Bool`

- `prelude.em` に `data Bool = | False | True` を置き、Prelude の変換がシグネチャと `data` の両方を受けるようにする。`Bool` は `builtin_items` の組み込みの型から外し、Prelude の宣言で作る。
- `Builtin::True` と `Builtin::False` をなくし、`LangItems` に `true_ctor` と `false_ctor` を足す。`&&` と `||` の脱糖はこの lang item を使う。
- タグは宣言の順なので、`False` が 0、`True` が 1 になる。Core IR の `FALSE` と `TRUE` と一致することを、lang item を作るときに `assert` で確かめる。

### 診断

| 番号 | 定数 | 内容 |
|---|---|---|
| E1016 | `CONSTRUCTOR_ARITY` | パターンのコンストラクタの引数の個数が、宣言のフィールドの数と違う |
| E1017 | `DUPLICATE_BINDING` | 1つのパターン、または1つの等式の引数の並びで、同じ変数名を2回束縛した |

## 2. 型検査

### 型の表現

- 型の表の `TyShape::Con(TypeDefId)` を `Con(TypeDefId, Vec<Ty>)` にし、書き出す `Type::Con` にも `args: Vec<Type>` を足す。
- 単一化は、ID が同じなら引数を順に単一化し、ID が違えば今の E2001 にする。occurs check は引数の中もたどる。
- 表示は `Option Int`、`List (Option a)`、`Option (Int -> Int)` のように、引数の中の型の適用と関数型を括弧で囲む。

### データ型の Kind

[型と Kind](../../spec/types.md) の「データ型の Kind は、フィールドの Kind の上限 (join) で推論する」を、次の形で実装する。

- `data` の宣言ごとに、Kind に効く型引数の位置を前もって求める。型引数 `a` は、フィールドに直接 `a` と書いたとき、または別の `data` の型の適用の効く引数の位置に `a` を書いたときに効く。再帰する宣言 (`List a`) と相互再帰する宣言があるので、すべての宣言で不動点を求める。
- 関数型の中の型引数は効かない。関数型の Kind はその矢印の線形性で決まる。フィールドに書いた関数型の線形性は、操作の引数の型と同じく `Unr` に固定する。表面の構文で `m` を書けないためである。
- `Table::kind_bounds` は、`Con(id, args)` に対して、効く位置の引数の境界を並べる。今の「`Con` はつねに `Unr`」はこの規則の特別な場合 (効く引数がない) になる。`Option a` の Kind は `a` の Kind、`Color` の Kind は `Unr` である。
- 4a には `Lin` の型がない。宣言がつねに `Lin` になる場合 (`File` のフィールド) の定数の境界は、段階5で足す。

### コンストラクタのスキーム

- コンストラクタのスキームは、宣言の型引数で量化した `Some : a -> Option a` の形にする。矢印の row は `<>`、線形性は `Unr` である。引数のないコンストラクタは `None : Option a` である。
- 式の中のコンストラクタは、関数と同じ経路で具体化する。そのため、部分適用と関数値 (`map Some xs`) は今の検査 (部分適用のクロージャの Kind を含む) がそのまま使える。
- コンストラクタのスキームは、関数のスキームと同じく `TypedModule` に書き出す。Core IR の変換の boxed の判定は、スキームではなく HIR の `TypeDefKind::Data` のコンストラクタのフィールドの数から決める。

### パターン

- パターンの検査は期待する型を受ける。`PatKind::Con` では、コンストラクタのスキームを具体化し、結果の型を期待する型と単一化し、引数のパターンをフィールドの型で検査する。
- 食い違いは E2001 にする。由来にパターンを足し、「このパターンは `Option Int` を期待する位置で `List a` を作るコンストラクタである」の形で、パターンを primary にする。
- 等式の引数のパターンはシグネチャの引数の型で、ラムダの引数のパターンは今の引数の型の推論で、`let` の左辺は右辺の型で検査する。

### `match`

- scrutinee の型を推論し、各枝のパターンをその型で検査する。
- 枝の本体は、`if` の枝と同じく、`match` 式の期待する型で検査する。期待する型がなければ、最初の枝の型に後の枝をそろえる。

### 使用回数のパス

- `match` の枝は、`if` の枝と同じく別の経路として扱う。どの経路でも1回でない変数に `Unr` の制約を出す今の規則がそのまま当てはまる。
- scrutinee の式は、`match` の位置で1回使う。
- 入れ子のパターンの中の `_` と使わない変数は、今の最上位の `_` と同じく、その型に `Unr` の制約を出す。由来は今と同じ `KindReason::Discarded` と、使わない変数の由来である。

## 3. 網羅性の検査

### 置き場所と順番

- `eml_types` に新しいモジュール `exhaustive` を置く。型推論と使用回数のパスの後に、型付き HIR の上で動かし、`eml_types` の検査の診断に加える。
- コンストラクタの集合は、パターンの型 (`BodyTypes::pats`) と scrutinee の型の型構成子から、`TypeDefKind::Data` のコンストラクタの並びを引いて求める。
- 型が `Error` を含むパターンや scrutinee、`Missing` のパターンを含む行列は検査しない。診断を連鎖させないためである。
- Core IR の変換は、網羅されていることを前提にする。どの枝にも当たらない値の受け皿は作らない。

### アルゴリズム

- Maranget の usefulness で、行列の各行が、前の行の全体に対して有用かを調べる。有用でない行は到達しない。ワイルドカードの行がすべての行の後に有用なら、網羅されていない。
- 網羅されていないときは、漏れているパターンの例を作る。例は `Some _`、`Cons _ Nil` の形で書く。note には最大3つを並べ、3つより多ければ「ほか」と書き添える。
- 変数と `_` と `()` は、どれもワイルドカードとして扱う。`Unit` の値は1つしかないためである。型の注釈のパターンは中のパターンとして扱う。

### 検査する位置

| 位置 | 行列 | 網羅されないときの診断 |
|---|---|---|
| `match` | 枝のパターンの列 | E4001 |
| 等式の引数 | 引数のパターンの並びを1行にした行列 | E4002 |
| `let` の左辺、ラムダの引数 | パターン1つの行列 | E4003 |

- 等式は 4a では1つだけなので、引数の並びを1行の行列として調べる。spec の「引数のタプルに対する `match`」と同じ結果になる。複数の等式は段階6で同じ行列に行を足す。
- 到達しない枝 (E4004) は `match` だけで調べる。到達しない等式は段階6で足す。

### 診断

| 番号 | 定数 | 重大度 | 指す場所 |
|---|---|---|---|
| E4001 | `NON_EXHAUSTIVE_MATCH` | Error | `match` のキーワード。漏れている例を note に出す |
| E4002 | `NON_EXHAUSTIVE_EQUATION` | Error | primary はシグネチャの関数名、secondary は等式の先頭 ([診断](../../spec/diagnostics.md) の「網羅性の診断」)。漏れている引数の並びの例 (`f None _`) を note に出す |
| E4003 | `REFUTABLE_PATTERN` | Error | `let` の左辺、またはラムダの引数のパターン。漏れている例を note に出す |
| E4004 | `UNREACHABLE_ARM` | Warning | 到達しない枝のパターン |

- 番号は `eml_types::codes` に置く。
- spec の診断の表には E4003 にあたる行がないので、`docs/spec/diagnostics.md` の「網羅性の診断」に足す。
- Warning だけのプログラムは、今の `has_errors` の判定どおり実行できる。

## 4. Core IR

### 命令

```rust
pub enum CExpr {
    // ...
    Join {
        join: JoinId,
        params: Vec<VarId>,
        captures: Vec<VarId>,
        body: CExprId,
        scope: CExprId,
    },
    Switch {
        scrutinee: Atom,
        arms: Vec<Arm>,
    },
    Jump {
        join: JoinId,
        args: Vec<Atom>,
    },
    // ...
}

pub struct Arm {
    pub tag: u32,
    /// 引数を持つコンストラクタの枝は、すべてのフィールドを順に束縛する。引数のないコンストラクタの枝では空である。
    pub fields: Vec<VarId>,
    pub body: CExprId,
}

pub enum Rhs {
    // ...
    /// 引数を持つコンストラクタの値を作る。`args` の所有権は値に移る。
    Con { tag: u32, args: Vec<Atom> },
}
```

- 引数のないコンストラクタの値は、今の `Bool` と同じく `Atom::Tag(n)` にする。
- `CoreFn::join` は、引数の並びと本体を返す。
- `if` の join point は引数1つ、B2 が作る join point は引数0個になる。
- コンストラクタを部分適用したときと関数値として使ったときは、操作を包む関数と同じく、コンストラクタを包む関数 (`con$Some` の名前) を作り、そのクロージャにする。包む関数は、使われたコンストラクタについてだけ1回作る。

### boxed の判定

- 引数を持つコンストラクタが1つでもある `data` の型の変数は boxed にする。型引数は判定に関わらない。
- 引数を持つコンストラクタのない型 (`Bool`、`Color`) は boxed にしない。
- boxed な変数の値が即値 (`Value::Tag`) のとき、`dup` と `decref` は何もしない。今のインタプリタの動きのままである。

### 所有権の規則

[Core IR](../../spec/core-ir.md) に次の規則を足す。

- `Switch` は scrutinee を1回使う (move)。枝の中でも scrutinee を使うなら、ほかの使用と同じく Perceus が `Switch` の前に `dup` する。
- 枝は、`Switch` の前に所有していた変数から scrutinee を除き、フィールドの変数のうち RC の対象を加えた集合を所有して始まる。使わないフィールドは、Perceus が枝の入口で `decref` する。
- join point の本体は、`captures` と引数のうち RC の対象を1つずつ所有して始まる。`jump` は渡す引数の所有権を渡す。
- `Rhs::Con` は `args` の所有権を受け取る。

### 生存解析、Perceus、verifier

- 生存解析では、枝のフィールドを枝の入口の束縛として、join point の引数を本体の入口の束縛として扱う。
- Perceus は上の所有権の規則で `dup` と `decref` を入れる。scrutinee の使用は move として数え、枝の入口の所有から除く。今の verifier はすでに scrutinee を消費として数えている。今の Perceus でこれまで違いが出なかったのは、scrutinee (`Bool`) が RC の対象でなかったためで、4a で boxed な scrutinee を入れる前に、Perceus の数え方を verifier にそろえる。
- verifier の2つの度合いは、どちらも枝のフィールドを枝の範囲の束縛として、引数の並びを join point の本体の範囲の束縛として扱う。`jump` の引数の数が join point の引数の数と一致することを確かめる。所有権まで確かめる度合いは、上の所有権の規則で釣り合いを数える。

### `match` のコンパイル

コンパイルは `translate/pattern.rs` に置き、join point は `translate/mod.rs` の骨組み (`Exit` と `Binding::Join`) で作る。

- scrutinee の値を変数に束縛し、それを最初の出現 (調べる値の変数) にする。
- 末尾にない `match` は、今の `if` と同じく、値を受ける join point (引数1つ) の範囲に全体を入れる。
- 各枝の本体を join point `arm_i` にする。引数は枝のパターンが束縛する変数で、順は `Body::pat_bindings` の順にする。本体は `match` の渡し先 (`Exit`) に値を渡す。すべての枝の join point を決定木の外側に置くので、決定木のどの葉からも届く。
- 決定木は節の行列で作る。
  1. 行列の最初の行がすべて変数かワイルドカードなら、葉として `jump arm_i(…)` を出す。変数のパターンには、その列の出現を渡す。
  2. そうでなければ、最初の行でコンストラクタのパターンを持ついちばん左の列を選び、その列の出現で `Switch` する。`Switch` は型のすべてのコンストラクタの枝を持つ。
  3. 選んだ列に現れるコンストラクタの枝では、フィールドを新しい出現として束縛し、行列をそのコンストラクタで特殊化して 1 に戻る。
  4. 選んだ列のどの行にも現れないコンストラクタの枝は、すべて同じ「残り」の行列になる。その部分木は `Switch` の直前に引数0個の join point にし、それらの枝から `jump` する。部分木を枝の数だけ複製しないためである。
- `let` の左辺、ラムダの引数、等式の引数のパターンも、枝が1つの `match` として同じ経路でコンパイルする。変数だけのパターンは、今と同じく束縛を作らずに出現を局所変数に対応させる。
- 変換は、枝の join point の `jump` の数を数えない。`jump` が1つの枝は B3 がその位置に戻し、どこからも届かない枝は B4 が消す。

例:

```haskell
f : Option (Option Int) -> Int
f o = match o with
  | Some (Some n) -> n
  | _ -> 0
```

変換の直後の Core IR は次の形になる (変数の番号は省く)。

```
fn f(o) {
  join arm1() { return 0 }
  join arm0(n) { return n }
  join rest0() { jump arm1() }
  switch o {
    #0 -> jump rest0()
    #1(x) ->
      join rest1() { jump arm1() }
      switch x {
        #0 -> jump rest1()
        #1(n) -> jump arm0(n)
      }
  }
}
```

### `simplify`

- B2 は、引数を1つだけ持ち、本体がその引数で `Switch` する join point に、定数のタグ (`Atom::Tag`) を渡す `jump` がある場合に限る。
- 切り出すのは引数のないコンストラクタの枝だけで、引数0個の join point にする。切り出した枝の中では、今と同じく引数をタグの定数に置き換える。
- フィールドを束縛する枝は `Switch` に残し、引数の置き換えもしない。引数を持つコンストラクタの値はタグだけでは決まらないためである。
- `unit_params` はなくなる。B3 は、引数のない join point をそのまま戻し、引数のある join point では引数ごとに `let p = a` を作る。
- B5 は、本体が `Return` か `Jump` で、その値がすべて引数か定数の場合に、引数を位置で置き換えて写す。
- status.md の B2 の性能の注意 (枝の部分木をたどる置き換え) は、そのまま残る。

### 表示

`pretty` は次の形にする。

- `join j0(x, y) [captures] { … }`、`jump j0(a, b)`。引数のないときは `join j1() [] { … }`、`jump j1()`
- `Switch` の枝は、フィールドのない枝が今の `#0 ->`、フィールドのある枝が `#1(h, t) ->`
- `let x = con #1(a, b)`

## 5. ランタイムとインタプリタ

- `Payload::Data { tag: u32, fields: Vec<Value> }` と記述子 `DescId::DATA` を足す。解放するときは、クロージャの引数と同じく、フィールドを1回ずつ `decref` する。
- 分解の関数を `eml_runtime` に置く。一意なオブジェクトならフィールドを取り出して箱を解放し、共有されていればフィールドを `dup` してから箱を `decref` する。どちらでも、呼び出し側はフィールドの参照を1つずつ所有する。`take_or_copy` と同じく、共有の判定をランタイムの1か所にまとめる。
- インタプリタは、`Rhs::Con` で箱を作る。`Switch` は、scrutinee が `Value::Tag` ならそのタグの枝に入り、`Payload::Data` なら分解してフィールドを枝の変数に入れる。枝のフィールドの数とオブジェクトのフィールドの数が違えば `Fault::Internal` にする。`Jump` は引数の並びを順に束縛する。

## 6. テスト

### 新しいテスト

TDD で、実装の前に書く。

| crate | 確かめること |
|---|---|
| `eml_hir` | `data` の item とコンストラクタの解決、中置のコンストラクタ、型の適用、`Bool` を Prelude から作ること、E1003 (コンストラクタと関数の重複、型引数の重複)、E1015、E1016、E1017 |
| `eml_types` | `Option Int` と入れ子の適用の表示、データ型の Kind (効く型引数、再帰する宣言、関数型のフィールド)、パターンの型の E2001、コンストラクタの部分適用、E4001〜E4004 と漏れの例 |
| `eml_core_ir` | 変換の直後の決定木 (入れ子、残りの join point、`let` とラムダと等式の引数のパターン)、B2 がフィールドを持つ枝を残すこと、B3 と B5 の複数の引数、Perceus の枝の所有 (使わないフィールドの `decref`、枝で scrutinee を使うときの `dup`)、verifier の枝の束縛と `jump` の引数の数 |
| `eml_runtime` | 分解の一意と共有の2通り、`Data` の解放とリーク検出 |

### case-of-case (B2) のテスト

B2 の限定が、引数のないタグで効き、フィールドを束縛する枝では何もしないことを確かめる。どれも `crates/eml_core_ir/tests/simplify.rs` に置き、`lower_until(Pass::Simplify)` の IR をスナップショットで見る。

| テスト | 入力の形 | 確かめること |
|---|---|---|
| `a_bool_match_in_a_condition_jumps_straight_to_the_branch` | `if (match o with \| Some _ -> True \| None -> False) then a else b` | `match` の各枝が、`Bool` で分岐し直さずに `then` と `else` の枝へ直接 `jump` する。`&&` と `||` 以外の経路でも B2 が効くことを確かめる |
| `known_tags_of_a_larger_type_jump_straight_to_their_arm` | 末尾にない `if` で `Red`、`Green`、`Blue` のどれかを作り、その値で3つの枝の `match` をする | 2つより多いタグでも、各定数の `jump` がその枝へ直接向かう。`jump` が1つの枝は B3 で戻る |
| `a_mixed_switch_splits_only_the_arms_without_fields` | `match (if c then None else Some x) with \| None -> a \| Some y -> g y` | `None` の枝だけが引数0個の join point になり、定数の `jump` がそこへ直接向かう。`Some y` の枝は `Switch` に残り、`Some` の値を渡す `jump` は元の join point を通る |
| `arms_with_fields_keep_the_join_point_argument` | `let o = if c then None else Some x` の後に `match o with \| None -> a \| Some y -> h o y` | フィールドを束縛する枝の中では、join point の引数をタグの定数に置き換えない |
| `jumps_that_pass_constructed_values_are_left_alone` | `match (if c then Some 1 else Some 2) with \| Some n -> n \| None -> 0` | どの `jump` も `Rhs::Con` で作った変数を渡すので、B2 は join point を変えない。引数を持つコンストラクタの case-of-case を後に回したことを、テストで示す |
| `split_arms_of_a_data_type_use_the_known_tag` | `let c = if b then Red else Green` の後に、`Red` の枝で `c` を使う `match` | 切り出した引数のない枝の中では、引数を `#0` に置き換える。今の `split_arms_use_the_known_tag` の `data` 版 |

Perceus の後の IR は、`crates/eml_core_ir/tests/perceus.rs` に1つ足す。

| テスト | 確かめること |
|---|---|
| `a_split_switch_still_unpacks_the_arm_with_fields` | `a_mixed_switch_splits_only_the_arms_without_fields` と同じ入力で、`Switch` に残った `Some y` の枝が `y` を所有して始まり、使わない値の `decref` が釣り合う |

実行の結果は UI テスト `run/data/case_of_case.em` で確かめる。上の入力の形をまとめ、各枝が `println` で順に出力する。`debug_heap` で、切り出した枝と残った枝のどちらを通っても箱が漏れないことを確かめる。

UI テスト:

- `run/data/`: `Option`、再帰する `List` の長さと和、入れ子のパターン、コンストラクタの部分適用と関数値、中置のコンストラクタ、`let` とラムダと等式の引数の反駁不可能なパターン (`data Box a = | Box a`)、E4004 の Warning (stderr のスナップショットで確かめる)
- `check-fail/exhaustiveness/`: E4001、E4002、E4003
- `check-fail/names/`: E1016、E1017
- `check-fail/types/`: パターンの型の E2001、型の適用の E1015

### 既存のテストの変更

種類1 (振る舞いの変更。ユーザーと合意済み):

| テスト | 変更 | 理由 |
|---|---|---|
| `tests/ui/check-fail/not-yet-supported/data_declarations.em` | 削除する | `data` が E0004 でなくなり、プログラムが通るようになる。後継は `run/data/` のテスト |
| `crates/eml_hir/tests/lower.rs` の `constructs_of_later_stages_are_not_yet_supported` | 入力から `data` の行を除き、`match` の枝をリテラルのパターン (`\| 0 -> x`) に変える | `data` と `match` が E0004 でなくなる。リテラルのパターンは 4b に残る E0004 なので、`match` の行で後の段階の構文を確かめ続けられる |
| `crates/eml_hir/src/builtin.rs` の単体テストの `Builtin::True` を引く行 | 削除する | `Builtin::True` をなくす |

種類2 (内部の表現のスナップショット。この spec の承認で合意とする):

| テスト | 変わる部分 |
|---|---|
| `crates/eml_core_ir/tests/simplify.rs` の `an_arm_reached_twice_stays_a_join_point` | `join j0(u7) [s2]` が `join j0() [s2]` に、`jump j0(())` が `jump j0()` になる |
| `crates/eml_core_ir/tests/simplify.rs` の `join_points_left_without_jumps_are_removed` | `join j1(u13) []` が `join j1() []` に、`jump j1(())` が `jump j1()` になる |

種類3 (期待値を変えない機械的な追随。計画で許す):

- 手で組んだ Core IR (`crates/eml_core_ir/tests/verify.rs`、`crates/eml_interp/tests/`、`eml_test_support` の `ir`) の `param` と `arg` を並びに、`Switch` の枝を `Arm` に書き換える。
- `Type::Con` と `TypeRefKind::Con` に引数の欄を足したことによる、テストの中の組み立ての書き換え。

各変更は `docs/implementation/test-changes.md` に記録する。

## 7. 文書の更新

| 文書 | 更新 |
|---|---|
| `docs/spec/core-ir.md` | 命令の表 (join point の引数の並び、`Switch` の枝のフィールド、コンストラクタの値)、所有権の規則、`simplify` の B2 の限定 |
| `docs/spec/runtime.md` | `Payload::Data` と分解 |
| `docs/spec/types.md` | データ型の Kind の求め方、フィールドの関数型の線形性を `Unr` に固定すること |
| `docs/spec/diagnostics.md` | E1016、E1017、E4001〜E4004。「網羅性の診断」に反駁可能な `let` とラムダの引数の行を足す |
| `docs/implementation/architecture.md` | `eml_types` の `exhaustive`、`eml_core_ir` の `translate/pattern.rs` |
| `docs/implementation/status.md` | 段階4を 4a と 4b に分けた表、各 crate の状況、次の作業の注意点 (下の項目) |

status.md の「次の作業の注意点」は次のように直す。

- 消す: 段階4の `data` の item と `Generics` の項目、boxed な値を `Switch` の scrutinee にするときの所有権の項目、B2 を設計し直す項目、`match` のコンパイルの置き場所と join point の引数の項目、引数名の重複の項目 (どれも 4a で片付く)
- 足す: 引数を持つコンストラクタの case-of-case を、使われない束縛を消すパスと一緒に入れること。網羅されていない `match` と等式の fix を、段階5の線形性の fix と一緒に入れること。フィールドの関数型の線形性を `Unr` に固定したことの見直し (操作の引数の型と同じ扱い)。決定木の列の選び方 (いちばん左) は単純で、行列によっては部分木が大きくなりうること
