//! 型、row、Kind の検査 (docs/spec/types.md)。

mod carry;
mod check;
mod context;
mod data;
mod diverge;
mod dump;
mod exhaustive;
mod kind;
mod resolve;
mod scc;
mod shape;
mod store;
mod table;
mod ty;
mod uniform;
mod usage;

use std::collections::HashMap;

use eml_diagnostics::{Diagnostic, FileId, Label, SourceFiles, TextRange, TextSize};
use eml_hir::{
    ConstructorId, EffectId, ExprId, Function, FunctionId, ItemMap, LocalId, PatId, Program,
    ValueItem, VarId,
};
use la_arena::ArenaMap;

use crate::kind::eval;
use crate::kind::problem::KindScheme;
use crate::shape::Shape;

pub use dump::{dump, dump_linear};
pub use resolve::{Resolution, resolve, satisfies};
pub use store::{
    EffectLabel, Lin, RowTail, RowType, Substitution, TypeId, TypeKind, TypeNames, TypeStore,
};
pub use uniform::{InstanceNode, Uniform};

pub mod codes {
    use eml_diagnostics::ErrorCode;

    pub const LINEAR_VALUE_MISUSED: ErrorCode = ErrorCode(3001);
    pub const LINEAR_VALUE_USED_TWICE: ErrorCode = ErrorCode(3002);
    pub const LINEAR_VALUE_NOT_CONSUMED: ErrorCode = ErrorCode(3003);
    pub const LINEAR_VALUE_DISCARDED: ErrorCode = ErrorCode(3004);
    pub const CONTINUATION_NOT_HANDLED: ErrorCode = ErrorCode(3005);
    pub const LINEAR_VALUE_KEPT_ACROSS_MULTI: ErrorCode = ErrorCode(3006);
    pub const VAR_KIND: ErrorCode = ErrorCode(3007);
    pub const ARRAY_VAR_MISUSED: ErrorCode = ErrorCode(3008);
    pub const TYPE_MISMATCH: ErrorCode = ErrorCode(2001);
    pub const EFFECT_NOT_IN_ROW: ErrorCode = ErrorCode(2002);
    pub const MISSING_MAIN: ErrorCode = ErrorCode(2003);
    pub const INVALID_MAIN_TYPE: ErrorCode = ErrorCode(2004);
    pub const INFINITE_TYPE: ErrorCode = ErrorCode(2005);
    pub const NO_INSTANCE: ErrorCode = ErrorCode(2006);
    pub const AMBIGUOUS_CONSTRAINT: ErrorCode = ErrorCode(2009);
    pub const LINEAR_INSTANCE_HEAD: ErrorCode = ErrorCode(2010);
    pub const METHOD_KIND_MISMATCH: ErrorCode = ErrorCode(2011);
    pub const CONSTRAINED_POLYMORPHIC_RECURSION: ErrorCode = ErrorCode(2012);
    pub const FIELD_NEEDS_KNOWN_TYPE: ErrorCode = ErrorCode(2013);
    pub const NO_SUCH_FIELD: ErrorCode = ErrorCode(2014);
    pub const INPUT_EFFECT: ErrorCode = ErrorCode(2015);
    pub const INPUT_LINEAR: ErrorCode = ErrorCode(2016);
    pub const BINDING_AMBIGUOUS: ErrorCode = ErrorCode(2017);
    pub const INPUT_CHANGES_UNIFORM: ErrorCode = ErrorCode(2018);
    pub const VAR_ESCAPES: ErrorCode = ErrorCode(2019);
    pub const ROW_BOUND_EXCEEDED: ErrorCode = ErrorCode(2020);
    pub const NON_EXHAUSTIVE_MATCH: ErrorCode = ErrorCode(4001);
    pub const NON_EXHAUSTIVE_EQUATION: ErrorCode = ErrorCode(4002);
    pub const REFUTABLE_PATTERN: ErrorCode = ErrorCode(4003);
    /// 重大度は Warning である (docs/spec/diagnostics.md の「割り当て済みの番号」)。
    pub const UNREACHABLE_ARM: ErrorCode = ErrorCode(4004);
    /// 重大度は Warning である (docs/spec/diagnostics.md の「割り当て済みの番号」)。
    pub const UNREACHABLE_EQUATION: ErrorCode = ErrorCode(4005);
}

/// 型付き HIR。HIR は複製せず、型を別テーブルに持つ (docs/implementation/architecture.md)。
/// 宣言の結果を宣言ごとに持つのは、クエリ化と REPL で、宣言ごとに結果を使い回せるようにするため
/// (docs/implementation/architecture.md の「`eml_types` の内部」)。
#[derive(Debug)]
pub struct TypedProgram {
    /// 宣言と本体の型がすべて指す、プログラムに1つの型の表。
    pub types: TypeStore,
    /// シグネチャのある関数、クラスのメソッド、操作、コンストラクタの型。
    pub decls: HashMap<ValueItem, DeclType>,
    /// シグネチャと等式の両方がある関数だけを含む。
    pub bodies: ItemMap<Function, BodyTypes>,
    /// 単相化で一様にする型変数の位置。
    pub uniform: Uniform,
    /// REPL の入力の関数ごとの結果。セッションのモジュールの入力の関数だけを含む。問い合わせの入力は含まない。
    pub inputs: ItemMap<Function, InputType>,
    /// `:type` の問い合わせの型 (`Show a => a -> String`)。問い合わせの検査が型の文字列まで届いたときだけ `Some` で、
    /// 後で報告する誤り (線形性、網羅性、E2012) があっても `Some` になりうる。誤りの判断は呼び出し側が診断で行う
    /// (docs/spec/types.md の「REPL の入力」)。
    pub query: Option<String>,
}

/// REPL の入力の関数の検査の結果
/// (docs/spec/types.md の「REPL の入力」)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputType {
    /// 入力の式の型。入力の関数の型 `Unit -> <IO> T` の `T` である。
    pub ty: TypeId,
    /// 式の入力の結果の表示の仕方。`let` の入力は `Unit` である。
    pub display: InputDisplay,
}

/// 式の入力の結果の表示の仕方 (docs/spec/types.md の「REPL の入力」)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputDisplay {
    /// 何も表示しない。
    Unit,
    /// 型が `Show` を持つ。値だけを書く。
    Value,
    /// 型が `Show` を持たない。値の後に ` : ` と型を書く。中身は診断と同じ形の型の表示である。
    Typed(String),
}

/// 1つの宣言の型検査の結果。
#[derive(Debug)]
pub struct DeclType {
    /// 後の段階が読む型。検査の最後に1回だけ書き出しておく。矢印は線形性を持ち、スキームで定数に決まらない宣言自身の
    /// 変数は `Var(i)` で残る (docs/implementation/architecture.md の「`eml_types` の内部」)。
    pub ty: TypeId,
    /// 後の段階は Kind を読まない (docs/implementation/architecture.md の「`eml_types` の内部」) ので、外からは読めなくする。
    pub(crate) shape: Shape,
    pub(crate) kinds: KindScheme,
    /// `kinds` だけから決まる事実。`kind_values` などが問い合わせのたびに求め直さないよう、1回だけ求めて持つ。
    pub(crate) facts: eval::SchemeFacts,
    /// 本体の型の `Var(shape.lin_vars + k)` が表す、宣言自身の変数の集合 `joins[k]`。値はその変数の値の結びである
    /// (docs/spec/types.md の「宣言の Kind 変数の番号」)。
    pub(crate) joins: Vec<Vec<u32>>,
}

impl TypedProgram {
    /// 宣言の `Var(i)` の、型引数 `args` (`Shape::rigids` の順) の instance での値。`i < lin_vars` は宣言自身の変数、
    /// それ以降は `joins` の結びである。値は `Unr`、`Lin`、`Poly` のどれかになる
    /// (docs/spec/types.md の「instance での評価」)。`args` は閉じた型でなければならない (`TypeStore::linearity` を読むので、
    /// 宣言に対して相対的な型は渡せない)。
    pub fn kind_values(&self, decl: ValueItem, args: &[TypeId], store: &TypeStore) -> Vec<Lin> {
        let declared = &self.decls[&decl];
        let shape = &declared.shape;
        debug_assert_eq!(args.len(), shape.rigids.len(), "type arguments of {decl:?}");
        // 矢印の値は呼び出し側が決めるので、型引数から決まらない変数の `base` は `Poly` である
        let mut base = vec![Lin::Poly; shape.lin_vars];
        for ((_, mu), &arg) in shape.rigids.iter().zip(args) {
            base[mu.index()] = store.linearity(arg);
        }
        let mut values = eval::values(shape.lin_vars, |var| base[var], &declared.facts);
        let joins: Vec<Lin> = declared
            .joins
            .iter()
            .map(|set| {
                set.iter()
                    .map(|&var| values[var as usize])
                    .fold(Lin::Unr, Lin::join)
            })
            .collect();
        values.extend(joins);
        values
    }

    /// 宣言 `decl` を型引数 `args` (`Instantiation::args` の順) で具体化する代入 (docs/spec/core-ir.md の「変換の規則」)。
    /// 型変数の名前は宣言の形から取るので、名前と型引数の順は食い違わない。`args` は閉じた型でなければならない
    /// (`kind_values`)。
    pub fn substitution(
        &self,
        decl: ValueItem,
        args: &[TypeId],
        store: &TypeStore,
    ) -> Substitution {
        Substitution::new(
            self.rigid_names(decl, args),
            self.kind_values(decl, args, store),
        )
    }

    /// Kind 変数を残す `substitution`。型引数が宣言に対して相対的で、Kind の値を求められないときに使う。
    pub fn partial_substitution(&self, decl: ValueItem, args: &[TypeId]) -> Substitution {
        Substitution::partial(self.rigid_names(decl, args))
    }

    fn rigid_names(&self, decl: ValueItem, args: &[TypeId]) -> Vec<(String, TypeId)> {
        let rigids = &self.decls[&decl].shape.rigids;
        debug_assert_eq!(args.len(), rigids.len(), "type arguments of {decl:?}");
        rigids
            .iter()
            .map(|(name, _)| name.clone())
            .zip(args.iter().copied())
            .collect()
    }

    /// 関数の一様な位置 `position` の型変数の Kind。一様な位置の鍵は `Int` の呼び出し元にも `FileIn` の呼び出し元にも
    /// 使われるので、スキームで `Unr` に固定されていなければ `Poly` にする
    /// (docs/spec/types.md の「`Flexible` と `OpVar` の Kind」)。
    pub fn uniform_kind(&self, function: FunctionId, position: usize) -> Lin {
        let declared = &self.decls[&ValueItem::Function(function)];
        let mu = declared.shape.rigids[position].1.index();
        if declared.facts.fixed_unr[mu] {
            Lin::Unr
        } else {
            Lin::Poly
        }
    }

    /// 単相化で宣言の本体の型を写す必要があるか。型変数を持つか、定数に畳まれない宣言自身の線形性の変数を持つときに
    /// 写す。`joins` の変数も宣言自身の変数なので、これで足りる
    /// (docs/spec/core-ir.md の「変換の規則」)。
    pub fn needs_substitution(&self, decl: ValueItem) -> bool {
        let declared = &self.decls[&decl];
        let shape = &declared.shape;
        if !shape.rigids.is_empty() {
            return true;
        }
        let facts = &declared.facts;
        (0..shape.lin_vars).any(|var| !facts.fixed_unr[var] && !facts.forced_lin[var])
    }
}

/// `mask` の1つの項目。呼び出しの間に起きた `effect` の操作は、外側の `effect` の handler のうち、内側から数えて
/// `keep` 個を残し、続く `skip` 個を飛ばす (docs/spec/effects.md の「handler の意味」)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MaskItem {
    pub effect: EffectId,
    pub keep: u32,
    pub skip: u32,
}

#[derive(Debug, Default)]
pub struct BodyTypes {
    pub exprs: ArenaMap<ExprId, TypeId>,
    pub locals: ArenaMap<LocalId, TypeId>,
    /// パターンが受けた値の型。`bind_pat` が束縛するパターンを記録するので、コンストラクタとタプルの引数のパターンは
    /// どれも入る。ラムダの引数の外側にある注釈のパターンは入らない (check/body.rs の `bind_param`)。Core IR が、
    /// handler の節の引数の変数と、`unpack` と `switch` で作るフィールドの変数の Repr を決めるのに使う。
    pub pats: ArenaMap<PatId, TypeId>,
    /// 式の中のトップレベルの item への参照ごとの具体化。キーは参照を表す `ExprKind::Path` の式と、コンストラクタを指す
    /// レコードの構築 (`ExprKind::Record`) の式である。局所変数の参照、パターンのコンストラクタ、handler の節の操作、
    /// シグネチャのない参照は記録しない
    /// (docs/implementation/architecture.md の「`eml_types` の内部」)。
    pub instantiations: ArenaMap<ExprId, Instantiation>,
    /// 呼び出しの矢印ごとの `mask` (docs/spec/types.md の「推論」)。キーは呼び出しの式と矢印の番号である。項目は
    /// エフェクトが rest に初めて現れた順に並ぶ。Core IR が、エフェクトの番号の順に並べ直して呼び出しに付ける。
    pub masks: HashMap<(ExprId, usize), Vec<MaskItem>>,
    /// 射影 (`ExprKind::Field`) が指すフィールド。型検査が射影の時点の型から引いた結果である
    /// (docs/spec/types.md の「フィールドの解決」)。
    pub fields: ArenaMap<ExprId, FieldTarget>,
    /// 更新 (`ExprKind::Update`) が作り直すコンストラクタとフィールド (docs/spec/types.md の「フィールドの解決」)。
    pub updates: ArenaMap<ExprId, Update>,
    /// 射影と更新が捨てるフィールドの番号と型。射影は取り出さない残りのフィールド、更新は上書きされる古いフィールドで
    /// ある。線形性の検査は、これらを `_` で受けた値と同じに扱う。
    pub discarded: ArenaMap<ExprId, Vec<(u32, TypeId)>>,
    /// 本体が動く row (関数の型の、引数の数番目の矢印の row) の分類。引数のない値は `Never` である。
    pub body_yield: RowYield,
    /// 宣言の型の最初の矢印の row の分類。関数型でなければ `Never` である。入口を `()` で呼ぶ関数 `entry$` の印に
    /// 使う。等式に引数のない入口 (`main = fn () -> ..`) では `body_yield` が使えず、書き出した型の rigid な末尾は名前
    /// でしか区別できないので、ここで分類しておく (docs/spec/core-ir.md の「`yields` の印」)。
    pub first_arrow_yield: RowYield,
    /// ラムダの本体が動く row (ラムダで最後にたどった矢印の row) の分類。キーはラムダの式である。
    pub lambda_yields: ArenaMap<ExprId, RowYield>,
    /// 呼び出しの矢印の row の分類。キーは `masks` と同じく、呼び出しの式と矢印の番号である。
    pub arrow_yields: HashMap<(ExprId, usize), RowYield>,
    /// 戻らない呼び出しの前で暗黙に手放す変数。キーは記録の対象の戻らない呼び出しの式、値はその経路のどこでも消費
    /// しない局所変数で、番号の順に並ぶ。型や HIR の誤りがある本体では記録しない
    /// (docs/spec/linearity.md の「線形性の検査パス」)。
    pub implicit_drops: HashMap<ExprId, Vec<LocalId>>,
    /// `var` の変数の型と分類。型は宣言の本体の中の型で、instance の型引数で写す前のものである
    /// (docs/spec/types.md の「`var` の変数」)。
    pub vars: ArenaMap<VarId, VarType>,
    /// 添字のノード (`ExprKind::Index`) ごとの読み方 (docs/spec/types.md の「`var` の変数」)。
    pub indexes: ArenaMap<ExprId, IndexTarget>,
}

/// `var` の変数の分類。推論を終えた後の変数の型で、型検査が決める (docs/spec/types.md の「`var` の変数」)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VarClass {
    /// 値を複製して読む変数。型の Kind は `Unr` である。
    Unr,
    /// `MutArray` を持つ配列の変数。
    Array,
}

/// `var` の変数の型と分類。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VarType {
    pub ty: TypeId,
    pub class: VarClass,
}

/// 添字 `e[i]` の読み方 (docs/spec/types.md の「`var` の変数」)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexTarget {
    /// `Array` の値を `Std.Array.get` で読む。式の添字と、その時点で型が `Array _` だった変数の添字である。
    Array,
    /// 配列の変数の要素を読む。
    ArrayVar,
}

/// 更新が作り直すコンストラクタ。`T` は型検査の中では表の型で、書き出した後は `TypeId` である。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Update<T = TypeId> {
    pub ctor: ConstructorId,
    /// 書いたフィールドの宣言の中の番号。ソースの順に並ぶ。
    pub written: Vec<u32>,
    /// すべてのフィールドの、型引数で具体化した型。宣言の順に並ぶ。Core IR は、残すフィールドを受ける変数の Repr を
    /// これで決める。配置の Repr で受けると、型変数のフィールドが `tobj` のまま後のパターンの変数に渡るためである
    /// (docs/spec/core-ir.md の「変換の規則」)。
    pub fields: Vec<T>,
}

/// 射影が指すフィールド。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldTarget {
    /// コンストラクタが1つの `data` の、宣言の中の番号のフィールド。
    Constructor { ctor: ConstructorId, field: u32 },
    /// 要素が `arity` 個のタプルの、`index` 番目の要素。
    Tuple { arity: u32, index: u32 },
}

/// 1つの参照の具体化。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Instantiation {
    pub decl: ValueItem,
    /// `Shape::rigids` の順 (関数はシグネチャに最初に現れた順、操作はエフェクトの型引数が先、コンストラクタは `data` の
    /// 頭の型引数の順) に並べた型引数。矢印は線形性を持つ。row 変数と宣言の Kind 変数そのものは持たない。
    pub args: Vec<TypeId>,
    /// 参照先のシグネチャの row 変数 (`generics.row_vars` の順) を具体化した row の分類。シグネチャが row 変数を
    /// 持たなければ空である。
    pub rows: Vec<RowYield>,
}

/// 具体化した row が yield しうるか。`Var(i)` は、参照を含む定義の i 番目の row 変数 (`generics.row_vars` の順)
/// に従う (docs/spec/types.md の「row の具体化の記録」)。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum RowYield {
    /// ラベルを含まず、閉じているか、末尾が型検査で最後まで決まらなかった row 変数である。
    #[default]
    Never,
    /// ラベルを含むか、末尾が参照を含む定義のものでない rigid な row 変数である。
    Always,
    /// ラベルを含まず、末尾が参照を含む定義の i 番目の row 変数である。
    Var(u32),
}

/// `files` は、E3003 の fix の字下げをソースから求めるのに使う (docs/implementation/diagnostics.md の「線形性の診断」)。
pub fn check(program: &Program, files: &SourceFiles) -> (TypedProgram, Vec<Diagnostic>) {
    check::check_module(program, files)
}

/// 入口のモジュールに `main` がないこと。`eml check` では検査せず、`eml run` だけが報告する (docs/spec/types.md の
/// 「推論」)。import した `main` は実行を始める関数にならないので、メッセージは入口のモジュールの定義を求める。
pub fn missing_main(file: FileId) -> Diagnostic {
    Diagnostic::error(
        codes::MISSING_MAIN,
        "the entry module does not define `main`",
        Label::new(
            file,
            TextRange::empty(0.into()),
            "`eml run` starts the program from `main`",
        ),
    )
    .with_help("add `main : Unit -> <IO> Unit` and an equation `main () = ...`")
}

/// REPL の入力が、前の入力の関数か instance の一様な位置を変えたこと。前の入力の関数は、すでに型引数を決めたコードとして
/// `CodeBase` にあり、同じ名前で違うコードを足せないので、入力を受け付けない
/// (docs/spec/modules.md の「REPL のセッション」)。REPL だけが前の入力の位置を
/// 知っているので、`eml_cli` がこの関数で作る。
pub fn input_changes_uniform(file: FileId, len: TextSize, name: &str) -> Diagnostic {
    Diagnostic::error(
        codes::INPUT_CHANGES_UNIFORM,
        format!("this input changes how `{name}` from an earlier input is compiled"),
        Label::new(
            file,
            TextRange::up_to(len),
            "this input makes a type variable of it grow without bound",
        ),
    )
    .with_note(
        "a class instance in this input adds a polymorphic recursion that passes through the earlier definition",
    )
    .with_help("start a new session to use this input")
}

/// 単体テストのための `Program`。Prelude と、`text` を入口にしたモジュールを変換する。
#[cfg(test)]
pub(crate) fn test_program(text: &str) -> eml_hir::Program {
    test_program_with_files(text).0
}

/// `test_program` と、そのソース。診断を作るテストが使う。
#[cfg(test)]
pub(crate) fn test_program_with_files(text: &str) -> (eml_hir::Program, SourceFiles) {
    let (loaded, _) = eml_hir::load("test.em", text, &eml_hir::MemorySource(&[]));
    let (def_map, _) = eml_hir::def_map(&loaded);
    let program = eml_hir::lower(&def_map, &loaded.modules).0;
    (program, loaded.files)
}

/// 単体テストのための REPL のセッションの `Program`。`inputs` を入力 1, 2, … として読む。
#[cfg(test)]
pub(crate) fn test_session(inputs: &[&str]) -> (eml_hir::Program, SourceFiles) {
    let inputs: Vec<eml_hir::SessionInput> = inputs
        .iter()
        .zip(1..)
        .map(|(&text, number)| eml_hir::SessionInput {
            number,
            text,
            form: eml_hir::InputForm::Typed,
        })
        .collect();
    let (loaded, _, _) = eml_hir::load_session(
        eml_hir::STD,
        &inputs,
        None,
        &eml_hir::MemorySource(&[]),
        &eml_hir::ImportCache::default(),
    );
    let (def_map, _) = eml_hir::def_map(&loaded);
    let program = eml_hir::lower(&def_map, &loaded.modules).0;
    (program, loaded.files)
}

/// 単体テストのための、最後に `:type` の問い合わせを置いたセッションの `Program`。
#[cfg(test)]
pub(crate) fn test_query(inputs: &[&str], query: &str) -> (eml_hir::Program, SourceFiles) {
    let number = inputs.len() as u32 + 1;
    let mut session: Vec<eml_hir::SessionInput> = inputs
        .iter()
        .zip(1..)
        .map(|(&text, number)| eml_hir::SessionInput {
            number,
            text,
            form: eml_hir::InputForm::Typed,
        })
        .collect();
    session.push(eml_hir::SessionInput {
        number,
        text: query,
        form: eml_hir::InputForm::Query,
    });
    let (loaded, _, _) = eml_hir::load_session(
        eml_hir::STD,
        &session,
        None,
        &eml_hir::MemorySource(&[]),
        &eml_hir::ImportCache::default(),
    );
    let (def_map, _) = eml_hir::def_map(&loaded);
    let program = eml_hir::lower(&def_map, &loaded.modules).0;
    (program, loaded.files)
}
