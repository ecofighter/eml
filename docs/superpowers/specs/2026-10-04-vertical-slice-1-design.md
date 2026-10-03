# 縦の貫通 段階1 の設計

位置づけ: 作業用の設計文書。規範に当たる決定は `docs/spec/` と `docs/implementation/` に反映済みで、この文書は段階1の内部構造だけを扱う。段階1を終えたら、残す価値のある内容を `docs/implementation/architecture.md` に移し、この文書は削除する。

## 目的

名前解決以降の段階 (HIR、型検査、Core IR、インタプリタ、ランタイム) を、狭い言語で初めて端から端まで通し、`eml run` で eml のプログラムを実行できるようにする。各段階のデータ構造はマイルストーン1 の全体を見越して作り、後の段階 (`docs/implementation/status.md` の「名前解決以降の実装段階」の 2〜6) は中身を広げるだけで済むようにする。

## 段階1の言語

- 宣言: シグネチャと、それに対応する等式1つ。引数のパターンは変数、`_`、`()` だけ
- 式: 整数と文字列のリテラル、`()`、変数、引数の揃った関数呼び出し、ブロック (式文と `let x = e`)、`if` (`else` の省略を含む)、型の明示 `(e : T)`、括弧
- 演算子: `+ - * / %`、単項の `-`、`== != < <= > >=`、`++`、`&&` `||`、`|>` `<|` (型と意味は `docs/spec/declarations.md` の標準の演算子の表)
- 型: `Int`、`String`、`Bool`、`Unit`、関数型。row は省略 (`<>`)、`<>`、`<IO>` だけ
- 組み込み: `println : String -> <IO> Unit`、`show_int : Int -> String`、`not : Bool -> Bool`、`True`、`False`
- 入口: `main : Unit -> <IO> Unit`

次のものは、見つけた段階で E0004 を出して回復する。

| 構文 | 出す段階 |
|---|---|
| `data`、`type`、`effect`、fixity の宣言、ラムダ、`match`、`handle`、`resume`、`drop`、`use`、`let ... in`、セクション、演算子の参照 `(+)`、タプル、フィールドアクセス、2つ目以降の等式、変数・`_`・`()` 以外のパターン、型変数、row 変数、`IO` 以外のエフェクト名 | `eml_hir` |
| 関数値 (部分適用、関数を引数や戻り値として使う、引数の揃っていない呼び出し) | `eml_types` |

## `eml_hir`

### データ構造

```rust
pub struct Module {
    pub functions: Arena<Function>,
}

pub struct Function {
    pub name: String,
    pub name_ptr: SyntaxNodePtr,     // 診断の位置
    pub signature: TypeRef,          // 型の参照の木 (HIR の型の表現)
    pub signature_ptr: SyntaxNodePtr,
    pub body: Body,
}

pub struct Body {
    pub params: Vec<PatId>,
    pub root: ExprId,
    pub exprs: Arena<Expr>,
    pub pats: Arena<Pat>,
    pub locals: Arena<Local>,        // 束縛した場所。名前と SyntaxNodePtr
}
```

- 本体を関数ごとの `Body` に置くのは、後でクエリ化したときに関数単位で再計算できるようにするためである (rust-analyzer と同じ分け方)。
- 各ノードは `SyntaxNodePtr` を直接持つ。脱糖で作ったノードには、元になった構文 (演算子の列や演算子のトークンを含むノード) のポインタを入れる。
- `Expr` の種類: `Missing` (ERROR ノードや E0004 の後)、`Lit(Int | String | Unit)`、`Path(Res)`、`Call { callee, args }` (適用の列を1つにまとめる)、`If { cond, then, else }`、`Block { stmts, tail }`、`Annot { expr, ty }`。
- `Stmt` の種類: `Let { pat, init }`、`Expr(ExprId)`。
- `Pat` の種類: `Bind(LocalId)`、`Wildcard`、`Unit`、`Missing`。
- `TypeRef` の種類: `Con(Int | String | Bool | Unit)`、`Fn { param, row: RowRef, ret }`、`Error`。`RowRef` は `Omitted | Closed(Vec<EffectRef>)` で、段階1の `EffectRef` は `IO` だけ。

### 名前解決

- 値の名前は `Res = Local(LocalId) | Function(FunctionId) | Builtin(Builtin) | Error` に解決する。スコープは内側から順に、ブロックの `let` (後の `let` が前を隠す)、関数の引数、トップレベルの関数、組み込み。
- 組み込みは、最も外側のスコープに置き、ユーザーの定義で隠せる (Haskell の `Prelude` と同じ)。S2 で本物の `Prelude` モジュールに移すときに作り直さずに済む。
- `Builtin` は `eml_hir::builtin` の enum で、組み込みの名前と演算子をすべて並べる (`Println`、`ShowInt`、`Not`、`True`、`False`、`IntAdd`、`IntSub`、`IntMul`、`IntDiv`、`IntMod`、`IntNeg`、`IntEq` などの比較、`StrConcat`)。型は `eml_types` が、実装は `eml_interp` が、この enum の `match` で与える。
- 型の名前 `Int` / `String` / `Bool` / `Unit` と、エフェクト名 `IO` も、組み込みとして解決する。

### 演算子の列

- `OpSeq` の要素を、標準の演算子の表で precedence climbing により木に組み直す。単項の `-` は、列の先頭か演算子の直後に現れたものを、優先順位 6 の `negate` として扱う。
- 結合しない演算子の並び、優先順位が同じで結合の向きが違う並びは E1006。
- 段階1は fixity の宣言を扱わないので、表は `eml_hir` の中の固定の表である。段階6で宣言を読む形に広げる。
- 脱糖: `a && b` → `if a then b else False`、`a || b` → `if a then True else b`、`x |> f` と `f <| x` → `f x`。`f` が呼び出しなら引数を末尾に足す (`x |> f a` → `f a x`)。

### 宣言の検査

- シグネチャのない等式 (E1004)、等式のないシグネチャ (E1005)、トップレベルの重複 (E1003)。
- 等式がシグネチャの直後にない場合と、2つ目以降の等式は E0004 (段階6で E1xxx の本来の検査にする)。
- 引数の個数とシグネチャの矢印の数の対応は、型検査に任せる。

## `eml_types`

### 型の表現

```rust
enum TyKind {
    Con(TyCon),                      // Int, String, Bool
    Record(RecRow),                  // Unit は閉じた空のレコード {}
    Fn { param: Ty, lin: Mult, row: Row, ret: Ty },
    Var(TyVar),                      // 推論用。Kind Type<μ> を持つ
    Rigid(RigidVar),                 // シグネチャの型変数 (段階2)
    Error,
}
struct Row { labels: Vec<Label>, tail: Option<RowVar> }  // RowVar は Kind Row<σ> を持つ
```

- 型は検査器の中の表に置き、`Ty` (ID) で引く。単一化は自前の union-find で行う。
- Kind の変数 (`μ`、`σ`) と制約 `x ≤ y` を集め、束 (`Unr ≤ Lin`、`Never ≤ Once ≤ Multi`) の上で最小解を求める仕組みを、段階1から置く。段階1では、ほぼすべてが `Unr` / `Once` に落ちる。
- row は scoped labels の書き換えで単一化する。段階1のラベルは `IO` だけで、row 変数はまだ現れない。

### 検査の流れ

1. 各関数のシグネチャを型に変換する。省略した row は `<>`。
2. 本体を、シグネチャの型に対して check モードで検査する。式が起こすエフェクトは、その関数の矢印の row に含まれていなければならない (E2002)。
3. 呼び出しでは、呼ばれる関数の型の矢印をたどり、引数の数が揃っていれば戻り値の型と row を得る。揃っていなければ E0004 (段階2で関数値にする)。
4. `main` があれば、シグネチャが `Unit -> <IO> Unit` であることを確かめる (E2004)。
- Kind の制約を SCC ごとに推論する仕組みは、制約が意味を持つ段階2で入れる。
- `Error` 型は何とでも黙って単一化する。`Error` が関わる箇所からは、追加の診断を出さない。

### 制約の由来

単一化には必ず `Origin` を渡す。種類は、引数の n 番目、`if` の条件と各枝、型の明示、シグネチャの戻り値、最後でない式文 (`Unit` が要る)、演算子の被演算子。E2001 は、この由来で expected / found と根拠の場所のラベルを作る。

### 出力

```rust
pub struct TypedModule {
    pub signatures: ArenaMap<FunctionId, Scheme>,
    pub bodies: ArenaMap<FunctionId, InferenceResult>,  // ExprId / LocalId → 型
    pub main: Option<FunctionId>,
}
```

- HIR は複製しない。`eml_core_ir::lower(&Module, &TypedModule)` に変える。
- `eml_cli::compile` は、`main` が `None` なら E2003 を足す。`check` は足さない。
- 線形性と網羅性の検査パスは、段階4と5で足す。

## `eml_core_ir`

### 形

```rust
pub struct Program {
    pub functions: Vec<CoreFn>,      // FnIdx
    pub main: FnIdx,
    pub strings: Vec<String>,        // 文字列の定数表
}
pub struct CoreFn {
    pub name: String,
    pub params: Vec<VarId>,
    pub vars: Vec<VarInfo>,          // Kind (Unr / Lin) とボックス化の有無
    pub body: CExprId,
    pub exprs: Vec<CExpr>,           // ANF の木をアリーナに置く
}
enum CExpr {
    Let { var: VarId, rhs: Rhs, body: CExprId },
    Switch { scrutinee: VarId, arms: Vec<(Tag, CExprId)> },
    Return(Atom),
    Dup { var: VarId, body: CExprId },
    Decref { var: VarId, body: CExprId },
}
enum Rhs {
    Atom(Atom),
    CallDirect(FnIdx, Vec<Atom>),
    Prim(PrimOp, Vec<Atom>),
    ConstString(StringIdx),          // 定数表から新しい文字列をヒープに作る
    Perform(Op, Vec<Atom>),          // 段階1は IO.println だけ
}
enum Atom { Var(VarId), Int(i64), Unit, Tag(u32) }
```

- 木の各ノードに ID (`CExprId`) を付けるのは、CEK の継続のフレームが「どこから再開するか」を ID で持てるようにするためである。
- `Bool` はタグ (`False` = 0、`True` = 1) で、`if` は `Switch` にする。段階4の `match` も同じ命令に乗せる。
- `show_int` などの組み込みの関数は `Prim` に、`println` は `Perform` に変換する。

### Perceus の `dup` / `decref` の挿入

- 変数を使うことを所有権の移動として扱う。関数の引数とプリミティブの引数は、どれも所有権を受け取る (owned)。
- ANF の上で後ろ向きに生存解析をする。後でまだ使う変数を使う場所の前に `Dup` を入れる。束縛したのに使わない変数と、ある分岐の枝で使わない変数には、その枝の先頭に `Decref` を入れる。
- 対象は `Unr` でボックス化した変数だけである。段階1でボックス化するのは `String` だけ。

### 門

`eml_types` の門を通った HIR だけが来るので、ここでは新しい E0004 を出さない。来るはずのない形は `debug_assert` で検出する。

## `eml_runtime`

- `Heap`: スロットの配列と空きリスト。各スロットは世代番号と、`Option<Object>` を持つ。
- `ObjRef { index: u32, generation: u32 }`。解放したスロットは世代番号を進めて空きリストに戻す。古い `ObjRef` は世代番号の不一致で検出する (常に検査する)。
- `Object { header: Header { rc: AtomicI32, desc: DescId }, payload }`。段階1の `payload` は文字列と継続のフレーム。
- 記述子の表 (`DescId` → 名前、フィールドのレイアウト、`Lin` の破棄処理) の枠を置く。段階1の記述子は文字列とフレームだけ。
- API: `alloc`、`dup`、`decref` (0 になったら作業リストで子をたどって解放)、フィールドの読み出し、`is_unique`、`mark_shared` (呼ぶと未実装の実行時エラー)。
- `debug_heap`: 終了時に生きているオブジェクトを数え、残っていれば記述子ごとの件数を添えてリークとして報告する。

## `eml_interp`

- `Value = Int(i64) | Unit | Tag(u32) | Obj(ObjRef)`。`Copy` で、`Rc` / `RefCell` を使わない。
- CEK 機械の状態: 制御 (関数と `CExprId`)、環境 (今のフレームのスロット)、継続 (フレームオブジェクトの連結リスト)。
- フレームは、戻ったときの束縛先の変数、再開する `CExprId`、関数、環境のスロットを持つランタイムのオブジェクトである。リストの最下部に `IO` の組み込み handler のフレームを置く。
- 変数の読み出しはスロットを空にする move。複製は `Dup` 命令だけで行う。
- `Perform(IO.println)` は、handler を探さずにその場で `OutputSink` に書き、`Unit` を結果にする (`docs/spec/core-ir.md`)。
- `Int` の演算は Rust の checked 演算で行い、オーバーフローとゼロ除算を実行時エラーにする。`/` と `%` は Rust の `checked_div` / `checked_rem` をそのまま使う (商を 0 の方向に切り捨てる)。`Int` の最小値を -1 で割る場合も、オーバーフローとして実行時エラーになる。
- 実行時エラーのメッセージは、段階1では関数名まで含める。ソース上の位置は、必要になったときに Core IR に位置を持たせて付ける。
- ヒープは CEK 機械が `&mut` で所有する。機械の状態は `Send` に保つ。

## `eml_syntax` の変更

- 整数のリテラルが `Int` の最大値を超えたら E0007 にする (`docs/spec/lexical.md`)。
- フィールドアクセスの連鎖の長さを入れ子の深さに数え、E0013 の対象にする (`docs/implementation/status.md`)。
- HIR への変換で要るアクセサを型付き AST ラッパに足す。

## `eml_cli`

- `analyze` を新しいインタフェース (`eml_core_ir::lower(&Module, &TypedModule)`) に合わせる。
- `compile` で E2003 (`main` がない) を足す。
- UI テストに `tests/ui/run-fail/` を足す (`docs/implementation/testing.md`)。
- `run/empty.em` と `comments_only.em` に `main : Unit -> <IO> Unit` と `main () = ()` を足す。

## テスト

| crate | テスト |
|---|---|
| `eml_hir` | HIR の pretty printer による変換結果 (演算子の組み直し、脱糖、名前解決の結果) と、E1xxx と E0004 の診断の inline スナップショット |
| `eml_types` | 関数ごとの `Scheme` と、E2xxx と E0004 の診断のスナップショット |
| `eml_core_ir` | Core IR の pretty printer による、`dup` / `decref` の位置を含むスナップショット |
| `eml_runtime` | 確保と解放、世代番号による解放済みアクセスの検出、リークの数え方、長い連鎖の解放 |
| `eml_interp` / 全体 | UI テスト |

## 完了条件

- `tests/ui/run/` に hello world、再帰 (階乗とフィボナッチ)、`if`、`let`、文字列の連結と `show_int` のプログラムを置き、`debug_heap` を有効にした状態で出力のスナップショットが通る。深い再帰 (例えば 10 万段) で Rust のスタックが溢れないことも確かめる。
- `tests/ui/run-fail/` で、`Int` のオーバーフローとゼロ除算が実行時エラーになる。
- `tests/ui/check-fail/` で、未定義の名前、シグネチャの欠落、等式の欠落、重複定義、結合しない演算子の並び、型の不一致、`main` の型の誤り、row に `<IO>` を書かずに `println` を呼んだ関数を、それぞれ診断として報告する。段階1で未対応の構文が E0004 になる。
- `eml run` で `main` のないファイルが E2003 になり、`eml check` では通る。
- 既存のテストがすべて通る (`check-fail/` のスナップショットに HIR 以降の診断が増える場合は、更新として扱う)。
- `cargo clippy --all-targets` と `cargo fmt` が通る。`docs/implementation/status.md` の段階1を「完了」にする。
