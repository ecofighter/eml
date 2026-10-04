# リファクタリング R2b-2: 組み込みと ID の設計

位置づけ: 作業用の設計文書。R2b-2 を終えたら、残す価値のある内容を `docs/implementation/architecture.md` と `docs/implementation/status.md` に移し、この文書は削除する。

## 目的と範囲

[status.md](../../implementation/status.md) の「R2b-2 組み込みと ID」の一覧を行う。組み込みの型をユーザーの関数と同じスキームの経路で作り、型とエフェクトを ID で表し、`TypedModule` がスキームを返すようにする。`eml_hir` と `eml_types` を変え、`eml_core_ir` を追随させる。

リファクタリング全体の方針 (UI テストの出力は原則として変えない、段階3〜5の器の形は作り替えるが機能は実装しない) と、テストの変更の運用 ([testing.md](../../implementation/testing.md)) に従う。R2b-2 は言語の振る舞いを変えない。HIR、型、Core IR、UI のスナップショットは、どれも変わらない見込みである。

次のことは、ユーザーと合意済みである。

- 組み込みのシグネチャは、eml のソースで書いた Prelude に置く
- 型構成子とエフェクトは ID だけで表し、型の引数の置き場所は持たせない。引数を持つ `data` と `effect` は段階3と4で、引数の単一化と一緒に入れる
- Core IR への変換の対応 (`PrimOp` などとの対応) は、R3 の「組み込みの変換を R2 の表から引く」に回す

## 1. Prelude と組み込みの表

### Prelude

`crates/eml_hir/src/prelude.em` に、組み込みの関数と演算子のシグネチャを eml のソースとして並べ、`include_str!` で `eml_hir` に埋め込む。

```
println : String -> <IO> Unit
show_int : Int -> String
not : Bool -> Bool
negate : Int -> Int
(+) : Int -> Int -> Int
(-) : Int -> Int -> Int
(*) : Int -> Int -> Int
(/) : Int -> Int -> Int
(%) : Int -> Int -> Int
(==) : Int -> Int -> Bool
(!=) : Int -> Int -> Bool
(<) : Int -> Int -> Bool
(<=) : Int -> Int -> Bool
(>) : Int -> Int -> Bool
(>=) : Int -> Int -> Bool
(++) : String -> String -> String
(>>) : (a -> <e> b) -> (b -> <e> c) -> a -> <e> c
(<<) : (b -> <e> c) -> (a -> <e> b) -> a -> <e> c
```

型は [宣言](../../spec/declarations.md) の標準の演算子の表と、[エフェクト](../../spec/effects.md) の組み込みの `IO` に従う。`>>` と `<<` は、今の `eml_types/src/builtins.rs` が手で組み立てている型と同じである (`f >> g` と `g << f` は、どちらも `fn x -> g (f x)`)。

- `True` と `False` は Prelude に書かない。コンストラクタはシグネチャの構文で書けない。型は lang item の `Bool` から決め、段階4で `data Bool = | False | True` に置き換える。
- `negate` (前置の `-`) は、ユーザーが名前で呼べない組み込みである。今も `negate` という名前は解決できない。Prelude には書くが、`ItemScope` には入れない。

### 組み込みの表

`crates/eml_hir/src/builtin.rs` に、`Builtin` ごとの情報を1つの表にまとめる。

| 列 | 中身 |
|---|---|
| `builtin` | `Builtin` の値 |
| `name` | Prelude と診断での書き方 (`println`、`+`、`negate`、`True` など) |
| `access` | 名前空間での見え方。`Named` (名前で引く値。`println`、`show_int`、`not`、`True`、`False`)、`Operator` (二項演算子として引く。`+` など、`++`、`>>`、`<<`)、`Internal` (名前で引けない。`negate`) |
| `arity` | 実装が受け取る引数の数。部分適用のクロージャの Kind に使う (docs/spec/types.md の「関数型」)。`True` と `False` は 0、`println`、`show_int`、`not`、`negate` は 1、二項演算子は 2、`>>` と `<<` は 3 |

- 今の `Builtin::from_name`、`Builtin::binary_operator`、`Builtin::name` は、この表を引く関数にする。名前の一覧が、今は3つの `match` に分かれている。
- `fixity` は今のまま別にする。`|>`、`<|`、`&&`、`||`、`::` など、組み込みでない演算子も含む構文の表だからである。

### 変換と型検査の経路

- `eml_hir::lower` は、はじめに Prelude を `eml_syntax::parse` で構文解析し、各シグネチャを `Signature` に変換して、`Module::builtins` (`Builtin` から `Signature` への表) に置く。シグネチャの名前は、組み込みの表で `Builtin` に対応づける。
- Prelude の範囲には、`eml_diagnostics` に足す専用の `FileId::PRELUDE` を使う。Prelude の範囲が診断に出ることはない。もし出たら、`SourceFiles` がそのファイルを引けずに panic するので、誤りにすぐ気づける。
- Prelude の構文解析と変換で診断が出ないことを、`debug_assert` と単体テストで確かめる。
- `eml_types::check` は、`Module::builtins` の各シグネチャから、ユーザーの関数と同じ経路でスキームを作る。`Rigids::new`、`lower_signature`、`closure_kinds` (表の `arity`)、`Scheme::new` の順に呼び、作った直後に多相化する (本体がないので、SCC の検査を待たない)。
- 組み込みの値の参照 (`Res::Builtin`) は、そのスキームを `instantiate` する。`True` と `False` は lang item の `Bool` の型にする。
- `crates/eml_types/src/builtins.rs` を消す。

## 2. 型とエフェクトの item、ID、lang item

### HIR

```rust
pub struct Module {
    pub file: FileId,
    pub functions: Arena<Function>,
    /// 型の item。今は組み込みの `Int`、`String`、`Bool`、`Unit` だけ。段階4で `data` を足す。
    pub types: Arena<TypeDef>,
    /// エフェクトの item。今は組み込みの `IO` だけ。段階3で `effect` の宣言を足す。
    pub effects: Arena<EffectDef>,
    /// Prelude のシグネチャ。
    pub builtins: HashMap<Builtin, Signature>,
    pub lang: LangItems,
}

pub struct TypeDef {
    pub name: String,
}

pub struct EffectDef {
    pub name: String,
}

/// 処理系が名前ではなく役割で引く item。
pub struct LangItems {
    pub int: TypeDefId,
    pub string: TypeDefId,
    pub bool: TypeDefId,
    pub unit: TypeDefId,
    pub io: EffectId,
}
```

- 変換のはじめに、組み込みの型とエフェクトを item として登録し、`ItemScope` の型の名前空間に入れる。型の名前空間は、名前を `TypeItem::Type(TypeDefId)` か `TypeItem::Effect(EffectId)` に解決する。型の位置にエフェクトの名前を書いたとき、エフェクトの位置に型の名前を書いたときの診断は、今と同じ「cannot find type」と「cannot find effect」である。
- `BuiltinType` の enum と `builtin_effect` を消す。
- `TypeRefKind::Builtin(BuiltinType)` を `TypeRefKind::Con(TypeDefId)` にする。
- `EffectRef::Io` の enum をやめ、`RowRef` の `effects` を `Vec<EffectId>` にする。
- `main` は名前で探すままにする。`main` は Prelude ではなくユーザーが定義する関数で、[型と Kind](../../spec/types.md) も名前で定めている。
- HIR の pretty は、`module.types` と `module.effects` から名前を引く。表示は今と同じである。

### 型検査

- `table::TyCon` の enum をやめ、`TyShape::Con(TypeDefId)` にする。`Table::new(lang: &LangItems)` が、`int`、`string`、`bool` の型を `Con` で作る。`TypeRefKind::Con(id)` は、`id` が `lang.unit` なら今と同じく空のレコード (`table.unit`) に、それ以外は `TyShape::Con(id)` に変換する。
- 型検査の中の row のラベル (`ty::Effect` の enum) を `EffectId` にする。多重度は、`lang.io` なら `Once` とする ([エフェクト](../../spec/effects.md) の組み込みの `IO`)。段階3で、エフェクトの宣言の操作ごとの多重度に置き換える。

### 外に出す型 (`eml_types::Type`)

- `Type::Int`、`Type::String`、`Type::Bool` を、`Type::Con { id: TypeDefId, name: String }` にする。名前を持たせるのは、`Module` を渡さずに表示するためである。
- 関数型の `effects: Vec<Effect>` を、ID と名前の対 `Vec<EffectLabel>` (`EffectLabel { id: EffectId, name: String }`) にする。
- `check_main` が比べる `Unit -> <IO> Unit` は、lang item から組み立てる。
- 表示は今と同じである。
- `eml_core_ir` の boxed かどうかの判定は、`Type::String` の代わりに `Type::Con { id, .. }` の `id` を `module.lang.string` と比べる。

## 3. `TypedModule` のスキームと `Type::Var`

### スキーム

```rust
pub struct TypedModule {
    /// シグネチャのある関数だけを含む。
    pub signatures: ArenaMap<FunctionId, Scheme>,
    pub bodies: ArenaMap<FunctionId, BodyTypes>,
    pub main: Option<FunctionId>,
}

/// 関数の型と、多相化したときに残った Kind の制約のうち、定数を片側に持つもの。
pub struct Scheme {
    pub ty: Type,
    pub constraints: Vec<KindConstraint>,
}
```

- `TypedModule::kinds` を消す。過去の計画で、既存の表示を変えないために `signatures` の型を変えず、テストの表示のためだけに足したフィールドである ([status.md](../../implementation/status.md) の「テストを変えないために曲げた箇所」の4)。
- `eml_types::dump` は、`Scheme::constraints` から `kinds:` の行を作る。表示は変わらない。
- `eml_core_ir` は `signatures[id].ty` を使う。

### `Type::Var` の区別

`Type::Var(String)` を、`Type::Rigid(String)` (シグネチャの型変数) と `Type::Flexible` (推論で解けなかった変数) の2つにする。row の末尾の `RowTail::Rigid` と `RowTail::Flexible` と同じ分け方である。表示は今と同じ (`a` と `_`) である。

## 4. テスト、文書、成功の条件

### 変わるテスト

| テスト | 種類 | 変更 |
|---|---|---|
| `crates/eml_types/src/table/tests.rs` の単体テスト | 3 | `Effect::Io` と `Table::new()` を、lang item の ID と `Table::new(&lang)` で組み立てる形にする。期待値は変えない |
| `crates/eml_types/src/ty.rs` の `function_types_are_displayed_like_the_surface_syntax` | 3 | `Type::Int` などを `Type::Con { .. }` で組み立てる形にする。期待値は変えない |
| `crates/eml_hir/src/lower/scope.rs` の単体テスト | 3 | `BuiltinType::Int` と `EffectRef::Io` の代わりに、登録した item の ID で確かめる形にする。確かめる中身 (組み込みの値、型、エフェクトの解決と、ユーザーの定義による隠し) は変えない |

スナップショット (HIR、型、Core IR、UI) は変わらない見込みである。上の表にないテストの期待値が変わった場合は、変えずに止まり、差分と理由をユーザーに示して承認を得る。

### 足すテスト

- `eml_hir`: Prelude が診断なしに変換でき、表のすべての `Builtin` (`True` と `False` を除く) のシグネチャが `Module::builtins` にあること
- `eml_hir`: `negate` を名前で引けないこと (`f : Int -> Int`、`f x = negate x` で E1001)
- `eml_types`: 組み込みのスキームを具体化した型が、今の手書きの型と同じ表示になること。`>>` と `<<` を関数値として使う関数 (`compose : (Int -> Int) -> (Int -> Int) -> Int -> Int`、`compose f g = f >> g` など) の推論結果で確かめる

### 文書

| 文書 | 変更 |
|---|---|
| `docs/implementation/architecture.md` | 「`eml_hir` の内部」に、Prelude、組み込みの表、型とエフェクトの item、lang item を足す。「組み込み (`eml_hir::builtin::Builtin`) は名前解決の最も外側のスコープで、…S2 で `Prelude` モジュールに移す」を、Prelude のシグネチャを持つ形に直す。「`eml_types` の内部」に、組み込みのスキーム、`TyShape::Con`、row のラベルの `EffectId`、`TypedModule` のスキームを足す。「`dump` は、スキームに残った Kind の制約…」を `Scheme::constraints` に直す |
| `docs/implementation/status.md` | 「リファクタリング」の表の R2b-2 を完了にする。「テストを変えないために曲げた箇所」の4の「今の負担」を「R2b-2 で `kinds` を除き、スキームを返す形にした」にする。R2b-2 の一覧を、済んだことの1段落にする。「完了した作業」に R2b-2 の行を足す |
| `docs/implementation/testing.md` | 種類1と種類2の変更がなければ、記録は足さない。実装の途中で承認を得て変えたテストがあれば、「リファクタリング R2b-2」の見出しを作って記録する |

### 成功の条件

- 組み込みのシグネチャが `crates/eml_hir/src/prelude.em` の1か所にあり、`crates/eml_types/src/builtins.rs` がない
- `BuiltinType`、`EffectRef::Io`、`ty::Effect` の enum、`TyCon` がなく、型とエフェクトは `TypeDefId` と `EffectId` で表す
- `TypedModule` に `kinds` がなく、`signatures` が `Scheme` を持つ
- `Type::Var` がなく、`Type::Rigid` と `Type::Flexible` がある
- スナップショットが変わらない (変わったものは承認を得たものだけである)
- `cargo test`、`cargo clippy --all-targets`、`cargo fmt --check` が通る
- 上の「文書」の表の変更が済んでいる
