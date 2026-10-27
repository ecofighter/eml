//! モジュールごとのスコープ表 (docs/implementation/architecture.md の「`eml_hir` の内部」)。全モジュールの
//! `ItemTree` から、item の ID、名前の表、定義に付く fixity、lang item、extern の索引を作る。名前を解決しなくても
//! 決まるものだけを読むので、item の変換より先に作れる。

use eml_diagnostics::{Diagnostic, ErrorCode, FileId, Label, TextRange, TextSize};
use eml_extern::{ExternEffect, ExternType};
use eml_syntax::{Parse, ast};
use la_arena::{Idx, RawIdx};
use rustc_hash::{FxHashMap, FxHashSet};

use crate::codes;
use crate::hir::{ExternIndex, LangItems, ParamSort};
use crate::item_tree::{Fixity, ImportName, InputItemKind, ItemTree, duplicate, first_item_start};
use crate::load::{ImportTarget, Loaded, LoadedModule, STD_ROOT};
use crate::names::{Definer, DisplayNames};
use crate::program::{
    BindingId, ClassId, ConstructorId, EffectId, FunctionId, ItemId, ModuleId, ModuleOrigin,
    OperationId, TypeDefId, TypeItem, ValueItem,
};

/// 名前の参照。修飾子は1つのセグメントとは限らない (2つ以上は E1031)。
#[derive(Debug, Clone, Copy)]
pub enum NameRef<'a> {
    Plain(&'a str),
    Qualified { qualifier: &'a str, name: &'a str },
}

/// 名前を引いた結果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolved<T> {
    Found(T),
    /// 診断を出さずに誤りにする。理由は fixity の求め方だけが見分ける (`Resolver::fixity_of`)。
    Silent(Silence),
    NotFound,
    /// E1028。候補の定義を持ち込んだ import の (ファイル、位置)。セッションでは前の入力の import でありうる。
    Ambiguous(Vec<(FileId, TextRange)>),
    /// E1029。ユーザーのモジュールの `pub` でない定義 (ファイル、位置)。
    Private(FileId, TextRange),
    /// E1058。別のモジュールの抽象型のコンストラクタか、抽象エフェクトの操作。
    Abstract(Hidden),
    /// E1031。
    UnknownQualifier,
}

/// 抽象型か抽象エフェクトが隠した部品。E1058 の文言と secondary に使う (docs/spec/modules.md の「公開の範囲」)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hidden {
    /// 部品の定義のファイルと位置。
    pub file: FileId,
    pub range: TextRange,
    /// 部品を持つ型かエフェクトの名前。
    pub owner: String,
    /// 型かエフェクトか。
    pub effect: bool,
    /// 宣言したモジュールの名前。標準ライブラリのモジュールも正式な名前 (`Std.Map`) で書く。
    pub module: String,
}

/// 診断を出さずに誤りにする理由 (docs/implementation/architecture.md の「名前解決の回復」)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Silence {
    /// 重複した宣言の部品だけがある (E1003 で報告済み)。
    Unusable,
    /// 壊れた import から来た名前、import の並びで報告済みの名前、不明な `T(..)` の部分のどれかである。
    Broken,
}

/// 名前を宣言したモジュールの外からの見え方 (docs/spec/modules.md の「公開の範囲」)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Visibility {
    Public,
    Private,
    /// 抽象型のコンストラクタと抽象エフェクトの操作。名前は修飾して引けるが、使うと E1058 になる。
    Abstract,
}

impl Visibility {
    fn of(public: bool) -> Visibility {
        if public {
            Visibility::Public
        } else {
            Visibility::Private
        }
    }

    /// `abstract` を書いた `pub` の型とエフェクトの部品の見え方。
    fn of_part(public: bool, abstract_: bool) -> Visibility {
        match (public, abstract_) {
            (true, true) => Visibility::Abstract,
            (public, _) => Visibility::of(public),
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Definition<T> {
    item: T,
    range: TextRange,
    visibility: Visibility,
    /// 重複した宣言の部品は使えない (規則2)。重複の判定に加えず、同じ名前の使える定義がないときだけ引かれる。
    usable: bool,
}

/// import の並びで修飾なしにした名前の出どころ。
#[derive(Debug, Clone, Copy)]
struct Source<T> {
    /// `None` は、壊れた import の名前か、並びで報告した名前である。どの種類の名前にも「不明」と答える
    /// (docs/implementation/architecture.md の「名前解決の回復」)。
    item: Option<T>,
    /// 名前を持ち込んだ import。E1028 の secondary に使う。
    import: TextRange,
}

#[derive(Debug)]
struct Qualifier {
    name: String,
    target: ImportTarget,
    import: TextRange,
}

/// モジュールの import のスコープ (docs/spec/modules.md の「import」)。
#[derive(Debug, Default)]
struct Imports {
    /// import の順に並べる。合流した修飾子のモジュールを、診断で決まった順に示すため。
    qualifiers: Vec<Qualifier>,
    values: FxHashMap<String, Vec<Source<ValueItem>>>,
    types: FxHashMap<String, Vec<Source<TypeItem>>>,
    /// 部品の分からない `T(..)` か `E(..)` が並びにある。壊れた import の部品と、並びで報告した型の部品は名前が
    /// 分からないので、見つからない値の名前をどれも「不明」として扱う
    /// (docs/implementation/architecture.md の「名前解決の回復」)。
    unknown_parts: bool,
}

/// 値と型の名前空間を同じ手順で引くための口。
trait Namespace: Copy {
    fn definitions(scope: &ModuleScope) -> &FxHashMap<String, Vec<Definition<Self>>>;
    fn imported(imports: &Imports) -> &FxHashMap<String, Vec<Source<Self>>>;
    /// どこにも見つからない名前を、診断を出さずに誤りにするか。
    fn unknown(imports: &Imports) -> bool;
    /// REPL の `let` の束縛か。束縛は、束縛した入力のモジュールからは引かない。右辺の同じ名前は前の版を指すため
    /// である。
    fn is_binding(self) -> bool {
        false
    }
    /// 型、エフェクト、クラスの部品なら、その持ち主。
    fn owner(self, _scope: &ModuleScope) -> Option<TypeItem> {
        None
    }
}

impl Namespace for ValueItem {
    fn definitions(scope: &ModuleScope) -> &FxHashMap<String, Vec<Definition<ValueItem>>> {
        &scope.values
    }

    fn imported(imports: &Imports) -> &FxHashMap<String, Vec<Source<ValueItem>>> {
        &imports.values
    }

    fn unknown(imports: &Imports) -> bool {
        imports.unknown_parts
    }

    fn is_binding(self) -> bool {
        matches!(self, ValueItem::Binding(_))
    }

    fn owner(self, scope: &ModuleScope) -> Option<TypeItem> {
        scope
            .parts
            .iter()
            .find(|(_, parts)| parts.iter().any(|&(_, part)| part == self))
            .map(|(&owner, _)| owner)
    }
}

impl Namespace for TypeItem {
    fn definitions(scope: &ModuleScope) -> &FxHashMap<String, Vec<Definition<TypeItem>>> {
        &scope.types
    }

    fn imported(imports: &Imports) -> &FxHashMap<String, Vec<Source<TypeItem>>> {
        &imports.types
    }

    fn unknown(_: &Imports) -> bool {
        false
    }
}

#[derive(Debug)]
struct ModuleScope {
    name: String,
    origin: ModuleOrigin,
    /// E1029 の secondary が定義を指すのに使う。
    file: FileId,
    /// 名前ごとの定義。ソースの位置の順で、使える定義の最初が名前の定義である (規則1)。
    values: FxHashMap<String, Vec<Definition<ValueItem>>>,
    types: FxHashMap<String, Vec<Definition<TypeItem>>>,
    /// 型引数の種類の並び (E1015、E1048)。重複した型引数は除いてある。
    type_params: FxHashMap<TypeDefId, Vec<ParamSort>>,
    /// `type_params` の各型引数を宣言に書いた位置。E1048 の secondary が指す。
    type_param_ranges: FxHashMap<TypeDefId, Vec<TextRange>>,
    effect_params: FxHashMap<EffectId, usize>,
    /// 定義に付いた fixity、宣言が `pub` か、宣言の演算子の位置。
    fixities: FxHashMap<ValueItem, (Fixity, bool, TextRange)>,
    /// `extern` のシグネチャを持つ関数。handler の節の先頭を extern の関数から引き直すのに使う (E1009)。
    extern_functions: FxHashSet<FunctionId>,
    /// REPL の `let` の束縛か式の入力を実行する関数 `input$N`。`ItemTree` の関数の後ろに置く。
    input_function: Option<FunctionId>,
    /// REPL の `let` の束縛。
    binding: Option<BindingId>,
    /// `T(..)`、`E(..)`、`C(..)` が取り込む部品。型にはコンストラクタ、エフェクトには操作、クラスにはメソッドを、
    /// 宣言の順に並べる。
    parts: FxHashMap<TypeItem, Vec<(String, ValueItem)>>,
    imports: Imports,
    /// E1031 の fix が import を挿入する位置 (`first_item_start`)。import を書けない入力では `None` である。
    import_at: Option<TextSize>,
}

/// プログラム全体の名前の表。
#[derive(Debug)]
pub struct DefMap {
    modules: Vec<ModuleScope>,
    prelude: ModuleId,
    entry: ModuleId,
    /// 読み込んだ標準ライブラリのモジュールの、短い名前 (`Fs`) からの表。Prelude は除く。ユーザーのモジュールが
    /// 標準ライブラリのモジュールを隠したことを示す help が引く。
    loaded_std: FxHashMap<String, ModuleId>,
    /// 埋め込んだ標準ライブラリのモジュールの短い名前すべて (`Loaded::std_names`)。
    std_names: FxHashSet<String>,
    lang: LangItems,
    externs: ExternIndex,
    names: DisplayNames,
}

/// モジュール 0 を Prelude として読む。入口は `Loaded::entry` のファイルのモジュールである。値と型の名前空間の重複
/// (E1003)、fixity の重複 (E1021)、このモジュールにない演算子の fixity (E1022)、import の並びの名前 (E1001、E1002、
/// E1029)、import の循環 (E1027) を出す。
pub fn def_map(loaded: &Loaded) -> (DefMap, Vec<Diagnostic>) {
    let modules = &loaded.modules;
    let entry = modules
        .iter()
        .position(|module| module.tree.file == loaded.entry)
        .expect("the entry file is a loaded module");
    let mut diagnostics = Vec::new();
    let mut scopes: Vec<ModuleScope> = modules
        .iter()
        .enumerate()
        .map(|(index, module)| {
            let mut scope = ModuleScope::new(module_id(index), module);
            scope.check_duplicates(module.tree.file, &mut diagnostics);
            scope.attach_fixities(&module.tree, &mut diagnostics);
            scope
        })
        .collect();
    // 並びの名前は取り込む先のモジュールの表から引くので、すべてのモジュールの表を作ってから組む
    let imports: Vec<Imports> = modules
        .iter()
        .map(|module| Imports::new(module, &scopes, &mut diagnostics))
        .collect();
    for (scope, imports) in scopes.iter_mut().zip(imports) {
        scope.imports = imports;
    }
    check_cycles(modules, &mut diagnostics);
    let lang = lang_items(&scopes[0]);
    let externs = extern_index(&scopes);
    let unit = externs
        .ty(ExternType::Unit)
        .expect("the Prelude declares `Unit`");
    let names = display_names(&scopes, unit);
    let loaded_std = scopes
        .iter()
        .enumerate()
        .filter(|(_, scope)| scope.origin == ModuleOrigin::Std)
        .filter_map(|(index, scope)| {
            let short = scope.name.strip_prefix(STD_ROOT)?.strip_prefix('.')?;
            Some((short.to_string(), module_id(index)))
        })
        .collect();
    (
        DefMap {
            modules: scopes,
            prelude: module_id(0),
            entry: module_id(entry),
            loaded_std,
            std_names: loaded.std_names.iter().cloned().collect(),
            lang,
            externs,
            names,
        },
        diagnostics,
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Visit {
    New,
    Open,
    Done,
}

/// import の循環 (E1027)。モジュールの番号の順に、import を宣言の順に深さ優先でたどり、たどっている途中のモジュールに
/// 戻る import を、循環を閉じる import として報告する。報告した後も名前解決を続ける
/// (docs/implementation/architecture.md の「名前解決の回復」)。import の鎖がいくら長くてもスタックを使い切らないよう、
/// 再帰せずに自前のスタックでたどる。
fn check_cycles(modules: &[LoadedModule], diagnostics: &mut Vec<Diagnostic>) {
    let mut state = vec![Visit::New; modules.len()];
    // たどっている途中のモジュールと、次に見る import の番号。モジュールの並びが循環の経路になる
    let mut path: Vec<(usize, usize)> = Vec::new();
    for start in 0..modules.len() {
        if state[start] != Visit::New {
            continue;
        }
        state[start] = Visit::Open;
        path.push((start, 0));
        while let Some(top) = path.last_mut() {
            let (module, next) = *top;
            top.1 += 1;
            let loaded = &modules[module];
            let (Some(import), Some(&target)) =
                (loaded.tree.imports.get(next), loaded.targets.get(next))
            else {
                path.pop();
                state[module] = Visit::Done;
                continue;
            };
            let ImportTarget::Module(target) = target else {
                continue;
            };
            let target = index(target);
            match state[target] {
                Visit::New => {
                    state[target] = Visit::Open;
                    path.push((target, 0));
                }
                Visit::Open => {
                    let start = path
                        .iter()
                        .position(|&(open, _)| open == target)
                        .expect("an open module is on the path");
                    let cycle: Vec<String> = path[start..]
                        .iter()
                        .map(|&(m, _)| m)
                        .chain([target])
                        .map(|m| format!("`{}`", modules[m].name))
                        .collect();
                    diagnostics.push(
                        Diagnostic::error(
                            codes::IMPORT_CYCLE,
                            format!("importing `{}` makes an import cycle", modules[target].name),
                            Label::new(
                                loaded.tree.file,
                                import.range,
                                "this import closes the cycle",
                            ),
                        )
                        .with_note(format!("the cycle is {}", cycle.join(" -> "))),
                    );
                }
                Visit::Done => {}
            }
        }
    }
}

/// 読み込みの段の番号のモジュールの ID。`lower` もモジュールを同じ順に置く。
pub(crate) fn module_id(index: usize) -> ModuleId {
    ModuleId::from_raw(RawIdx::from(index as u32))
}

/// モジュールの中の局所の番号の item の ID。関数、型、エフェクト、クラスは `ItemTree` の順の番号で、部品は宣言の順の
/// 通し番号である。`lower` も同じ順にアリーナへ置く。
pub(crate) fn item_id<T>(module: ModuleId, local: usize) -> ItemId<T> {
    ItemId::new(module, Idx::from_raw(RawIdx::from(local as u32)))
}

impl ModuleScope {
    /// 局所の番号は `ItemTree` の順に振る。コンストラクタ、操作、メソッドは、宣言の順に通し番号に
    /// する。`lower` も同じ順にアリーナへ置く。
    fn new(module: ModuleId, loaded: &LoadedModule) -> ModuleScope {
        let tree = &loaded.tree;
        let input_function = tree
            .input
            .as_ref()
            .map(|_| item_id(module, tree.functions.len()));
        let binding = tree.input.as_ref().and_then(|input| match &input.kind {
            InputItemKind::Let { name: Some(_), .. } => Some(item_id(module, 0)),
            InputItemKind::Let { name: None, .. }
            | InputItemKind::Expr(_)
            | InputItemKind::Query(_) => None,
        });
        let mut scope = ModuleScope {
            name: loaded.name.clone(),
            origin: loaded.origin,
            file: tree.file,
            values: FxHashMap::default(),
            types: FxHashMap::default(),
            type_params: FxHashMap::default(),
            type_param_ranges: FxHashMap::default(),
            effect_params: FxHashMap::default(),
            fixities: FxHashMap::default(),
            extern_functions: FxHashSet::default(),
            input_function,
            binding,
            parts: FxHashMap::default(),
            imports: Imports::default(),
            import_at: first_item_start(&loaded.parse),
        };
        scope.declare(module, tree);
        scope
    }

    fn declare(&mut self, module: ModuleId, tree: &ItemTree) {
        for (k, data) in tree.data.iter().enumerate() {
            let id = item_id(module, k);
            push(
                &mut self.types,
                &data.name,
                TypeItem::Type(id),
                data.name_range,
                Visibility::of(data.public),
                true,
            );
            self.type_params
                .insert(id, data.params.iter().map(|&(_, _, sort)| sort).collect());
            self.type_param_ranges
                .insert(id, data.params.iter().map(|&(_, range, _)| range).collect());
        }
        for (k, effect) in tree.effects.iter().enumerate() {
            let id = item_id(module, k);
            push(
                &mut self.types,
                &effect.name,
                TypeItem::Effect(id),
                effect.name_range,
                Visibility::of(effect.public),
                true,
            );
            self.effect_params.insert(id, effect.params.len());
        }
        for (k, class) in tree.classes.iter().enumerate() {
            push(
                &mut self.types,
                &class.name,
                TypeItem::Class(item_id(module, k)),
                class.name_range,
                Visibility::of(class.public),
                true,
            );
        }
        for names in self.types.values_mut() {
            names.sort_by_key(|definition| definition.range.start());
        }
        // 重複した宣言 (名前の2つ目以降の定義) の部品は使えない (規則2)
        let duplicates: FxHashSet<TypeItem> = self
            .types
            .values()
            .flat_map(|names| names.iter().skip(1).map(|definition| definition.item))
            .collect();
        for (k, function) in tree.functions.iter().enumerate() {
            let id = item_id(module, k);
            if function
                .signature
                .as_ref()
                .is_some_and(|signature| signature.extern_keyword.is_some())
            {
                self.extern_functions.insert(id);
            }
            push(
                &mut self.values,
                &function.name,
                ValueItem::Function(id),
                function.first_range,
                Visibility::of(function.public),
                true,
            );
        }
        // 部品の局所の番号は、モジュールの中の宣言の順の通し番号である。`lower` も同じ順にアリーナへ置く
        let mut next = 0..;
        for (k, data) in tree.data.iter().enumerate() {
            let owner = TypeItem::Type(item_id(module, k));
            let usable = !duplicates.contains(&owner);
            let visibility = Visibility::of_part(data.public, data.abstract_ctors);
            for constructor in &data.constructors {
                let id = item_id(module, next.next().unwrap());
                self.declare_part(
                    owner,
                    &constructor.name,
                    ValueItem::Constructor(id),
                    constructor.name_range,
                    visibility,
                    usable,
                );
            }
        }
        let mut next = 0..;
        for (k, effect) in tree.effects.iter().enumerate() {
            let owner = TypeItem::Effect(item_id(module, k));
            let usable = !duplicates.contains(&owner);
            // extern のエフェクトの操作 (Prelude の `io_failed`) は、宣言したモジュールの外から見せない
            // (docs/spec/modules.md の「標準ライブラリ」)
            let visibility = match effect.extern_keyword {
                Some(_) => Visibility::Private,
                None => Visibility::of_part(effect.public, effect.abstract_ops),
            };
            for operation in &effect.operations {
                let id = item_id(module, next.next().unwrap());
                self.declare_part(
                    owner,
                    &operation.name,
                    ValueItem::Operation(id),
                    operation.name_range,
                    visibility,
                    usable,
                );
            }
        }
        // `pub class` はメソッドもまとめて公開する
        // (docs/spec/declarations.md の「`pub`」)
        let mut next = 0..;
        for (k, class) in tree.classes.iter().enumerate() {
            let owner = TypeItem::Class(item_id(module, k));
            let usable = !duplicates.contains(&owner);
            for method in &class.methods {
                let id = item_id(module, next.next().unwrap());
                let range = method
                    .signature
                    .as_ref()
                    .map_or(method.first_range, |signature| signature.name_range);
                self.declare_part(
                    owner,
                    &method.name,
                    ValueItem::Method(id),
                    range,
                    Visibility::of(class.public),
                    usable,
                );
            }
        }
        if let (Some(binding), Some(input)) = (self.binding, &tree.input)
            && let InputItemKind::Let {
                name: Some((name, range)),
                ..
            } = &input.kind
        {
            push(
                &mut self.values,
                name,
                ValueItem::Binding(binding),
                *range,
                Visibility::Private,
                true,
            );
        }
        for names in self.values.values_mut() {
            names.sort_by_key(|definition| definition.range.start());
        }
    }

    /// 型、エフェクト、クラスの部品 (コンストラクタ、操作、メソッド) を、名前の表と `T(..)` が取り込む並びに入れる。
    fn declare_part(
        &mut self,
        owner: TypeItem,
        name: &str,
        item: ValueItem,
        range: TextRange,
        visibility: Visibility,
        usable: bool,
    ) {
        push(&mut self.values, name, item, range, visibility, usable);
        self.parts
            .entry(owner)
            .or_default()
            .push((name.to_string(), item));
    }

    /// 名前空間ごとに、使える定義の2つ目以降を E1003 にする (規則1)。
    fn check_duplicates(&self, file: FileId, diagnostics: &mut Vec<Diagnostic>) {
        let mut report = |name: &str, ranges: Vec<TextRange>| {
            if let Some((first, rest)) = ranges.split_first() {
                for again in rest {
                    diagnostics.push(duplicate(file, name, *first, *again));
                }
            }
        };
        for (name, definitions) in &self.types {
            report(
                name,
                definitions
                    .iter()
                    .map(|definition| definition.range)
                    .collect(),
            );
        }
        for (name, definitions) in &self.values {
            report(
                name,
                definitions
                    .iter()
                    .filter(|definition| definition.usable)
                    .map(|definition| definition.range)
                    .collect(),
            );
        }
    }

    /// fixity の宣言を、演算子が解決した先の定義に付ける。同じ定義への2回目の宣言は E1021、このモジュールで定義して
    /// いない演算子への宣言は E1022 にし、どちらも付けない (docs/spec/declarations.md の「fixity」)。
    fn attach_fixities(&mut self, tree: &ItemTree, diagnostics: &mut Vec<Diagnostic>) {
        let file = tree.file;
        for item in &tree.fixities {
            let Some(fixity) = item.fixity else {
                continue;
            };
            for (name, range) in &item.operators {
                let found = self.values.get(name).and_then(|definitions| {
                    definitions
                        .iter()
                        .find(|definition| definition.usable)
                        .or_else(|| definitions.first())
                });
                let Some(definition) = found else {
                    // セッションでは利用者に入力が見え、モジュールは見えない
                    let place = match self.origin {
                        ModuleOrigin::Session(_) => "input",
                        ModuleOrigin::Std | ModuleOrigin::User => "module",
                    };
                    diagnostics.push(Diagnostic::error(
                        codes::FIXITY_WITHOUT_DEFINITION,
                        format!("`{name}` is not defined in this {place}"),
                        Label::new(
                            file,
                            *range,
                            format!(
                                "a fixity declaration needs a definition of its operator in the same {place}"
                            ),
                        ),
                    ));
                    continue;
                };
                if let Some((_, _, first)) = self.fixities.get(&definition.item) {
                    diagnostics.push(
                        Diagnostic::error(
                            codes::DUPLICATE_FIXITY,
                            format!("`{name}` has more than one fixity declaration"),
                            Label::new(file, *range, "declared again here"),
                        )
                        .with_secondary(Label::new(
                            file,
                            *first,
                            "first declared here",
                        )),
                    );
                    continue;
                }
                // 抽象型の中置のコンストラクタは外から使えないので、`pub` の fixity の宣言でも外へ出さない
                let public = item.public && definition.visibility == Visibility::Public;
                self.fixities
                    .insert(definition.item, (fixity, public, *range));
            }
        }
    }

    /// import の並びの小文字の名前と `(op)`。見つからない名前と `pub` でない名前は、並びの位置で報告して `None` を返す。
    /// 重複した宣言の部品だけがある名前は、E1003 で報告済みなので黙って `None` を返す。標準ライブラリの `pub` でない
    /// item は、定義がないものとして扱う (docs/spec/modules.md の「標準ライブラリ」)。
    fn export_value(
        &self,
        name: &str,
        (file, range): (FileId, TextRange),
        diagnostics: &mut Vec<Diagnostic>,
    ) -> Option<ValueItem> {
        let mut unusable = false;
        for definition in self.values.get(name).into_iter().flatten() {
            if !definition.usable {
                unusable = true;
                continue;
            }
            if definition.visibility == Visibility::Public {
                return Some(definition.item);
            }
            if self.origin == ModuleOrigin::Std {
                break;
            }
            diagnostics.push(private_name(
                file,
                range,
                name,
                (self.file, definition.range),
            ));
            return None;
        }
        if !unusable {
            // 本体で使った位置の E1001 と同じく、`(op)` は演算子と呼ぶ。値の名前は文字か `_` で始まる
            let what = if name.starts_with(|c: char| c.is_alphabetic() || c == '_') {
                "value"
            } else {
                "operator"
            };
            diagnostics.push(not_in_module(
                codes::UNDEFINED_NAME,
                file,
                range,
                what,
                name,
                &[&self.name],
            ));
        }
        None
    }

    /// import の並びの大文字の名前。型の名前空間 (型、エフェクト、クラス) だけを見る (docs/spec/modules.md の「import」)。見つかれば、定義と
    /// `pub` かを返す。`pub` でなければ並びの位置で報告する。標準ライブラリの `pub` でない item は、定義がないものとして
    /// 扱う。
    fn export_type(
        &self,
        name: &str,
        (file, range): (FileId, TextRange),
        diagnostics: &mut Vec<Diagnostic>,
    ) -> Option<(TypeItem, bool)> {
        let Some(definition) =
            self.types
                .get(name)
                .and_then(|names| names.first())
                .filter(|definition| {
                    definition.visibility == Visibility::Public || !self.origin.is_std()
                })
        else {
            let diagnostic = not_in_module(
                codes::UNDEFINED_TYPE,
                file,
                range,
                "type or effect",
                name,
                &[&self.name],
            );
            // コンストラクタは型と一緒に `T(..)` で取り込む (docs/spec/modules.md の「import」)
            diagnostics.push(match self.constructor_owner(name) {
                Some(owner) => {
                    diagnostic.with_help(format!("import the constructor with `{owner}(..)`"))
                }
                None => diagnostic,
            });
            return None;
        };
        let public = definition.visibility == Visibility::Public;
        if !public {
            diagnostics.push(private_name(
                file,
                range,
                name,
                (self.file, definition.range),
            ));
        }
        Some((definition.item, public))
    }

    /// 抽象型か抽象エフェクトの部品の定義から、E1058 に書く情報を作る。
    fn hidden<V: Namespace>(&self, definition: &Definition<V>) -> Hidden {
        let owner = definition.item.owner(self);
        let name = owner.and_then(|owner| {
            self.types.iter().find_map(|(name, definitions)| {
                definitions
                    .iter()
                    .any(|definition| definition.item == owner)
                    .then_some(name.clone())
            })
        });
        Hidden {
            file: self.file,
            range: definition.range,
            owner: name.unwrap_or_default(),
            effect: matches!(owner, Some(TypeItem::Effect(_))),
            module: self.name.clone(),
        }
    }

    /// コンストラクタ `name` を持つ型の名前。
    fn constructor_owner(&self, name: &str) -> Option<&str> {
        let constructor = self
            .values
            .get(name)?
            .iter()
            .find(|definition| matches!(definition.item, ValueItem::Constructor(_)))?
            .item;
        let (&owner, _) = self
            .parts
            .iter()
            .find(|(_, parts)| parts.iter().any(|&(_, part)| part == constructor))?;
        self.types
            .iter()
            .find(|(_, definitions)| {
                definitions
                    .iter()
                    .any(|definition| definition.item == owner)
            })
            .map(|(name, _)| name.as_str())
    }
}

impl Imports {
    /// 並びの名前を引けなければ、並びの位置で1回だけ報告し、名前を壊れた印で登録する。本体でその名前を使った位置は、
    /// 診断を出さずに誤りになる (docs/implementation/architecture.md の「名前解決の回復」)。
    fn new(
        module: &LoadedModule,
        scopes: &[ModuleScope],
        diagnostics: &mut Vec<Diagnostic>,
    ) -> Imports {
        let file = module.tree.file;
        let mut imports = Imports::default();
        for (import, &target) in module.tree.imports.iter().zip(&module.targets) {
            // 修飾子が `Prelude` になる import は読み込みの段が E1030 にした。暗黙の修飾子 `Prelude` と合流させないため、
            // 修飾子には登録しない (docs/spec/modules.md の「Prelude」)
            if import.qualifier != scopes[0].name {
                imports.qualifiers.push(Qualifier {
                    name: import.qualifier.clone(),
                    target,
                    import: import.range,
                });
            }
            let target_scope = match target {
                ImportTarget::Module(id) => Some(&scopes[index(id)]),
                ImportTarget::Broken => None,
            };
            for listed in import.list.iter().flatten() {
                match listed {
                    ImportName::Value { name, range } => {
                        let item = target_scope.and_then(|scope| {
                            scope.export_value(name, (file, *range), diagnostics)
                        });
                        push_source(&mut imports.values, name, item, import.range);
                    }
                    ImportName::Type { name, range, all } => {
                        let Some((scope, (item, public))) = target_scope.and_then(|scope| {
                            Some((scope, scope.export_type(name, (file, *range), diagnostics)?))
                        }) else {
                            push_source(&mut imports.types, name, None, import.range);
                            imports.unknown_parts |= *all;
                            continue;
                        };
                        push_source(
                            &mut imports.types,
                            name,
                            public.then_some(item),
                            import.range,
                        );
                        if *all {
                            let mut hidden = false;
                            for (part, value) in scope.parts.get(&item).into_iter().flatten() {
                                // 抽象型と抽象エフェクトの部品は取り込まず、`(..)` で1回だけ報告する
                                // (docs/spec/modules.md の「公開の範囲」)
                                let abstract_ = public
                                    && scope.values.get(part).into_iter().flatten().any(
                                        |definition| {
                                            definition.item == *value
                                                && definition.visibility == Visibility::Abstract
                                        },
                                    );
                                hidden |= abstract_;
                                push_source(
                                    &mut imports.values,
                                    part,
                                    (public && !abstract_).then_some(*value),
                                    import.range,
                                );
                            }
                            if hidden {
                                let at = all_range(&module.parse, *range).unwrap_or(*range);
                                diagnostics.push(abstract_import(file, at, name, item));
                            }
                        }
                    }
                }
            }
        }
        imports
    }
}

fn push_source<T>(
    table: &mut FxHashMap<String, Vec<Source<T>>>,
    name: &str,
    item: Option<T>,
    import: TextRange,
) {
    table
        .entry(name.to_string())
        .or_default()
        .push(Source { item, import });
}

/// import の (ファイル、位置)。
type ImportAt = Option<(FileId, TextRange)>;

/// 同じ定義は、いくつの import が持ち込んでも1つと数える (docs/spec/modules.md の「名前の解決」)。
fn add<T: PartialEq>(found: &mut Vec<(T, ImportAt)>, item: T, import: ImportAt) {
    if !found.iter().any(|(seen, _)| *seen == item) {
        found.push((item, import));
    }
}

/// 壊れていない定義がちょうど1つならそれを使い、2つ以上なら曖昧にする。1つもなく壊れた import があれば、診断を出さずに
/// 誤りにする (docs/implementation/architecture.md の「名前解決の回復」)。どれでもなければ `None` で、呼ぶ側が次を引く。
fn decide<T>(mut found: Vec<(T, ImportAt)>, broken: bool) -> Option<Resolved<T>> {
    match found.len() {
        0 if broken => Some(Resolved::Silent(Silence::Broken)),
        0 => None,
        1 => found.pop().map(|(item, _)| Resolved::Found(item)),
        _ => Some(Resolved::Ambiguous(
            found.into_iter().filter_map(|(_, import)| import).collect(),
        )),
    }
}

fn index(module: ModuleId) -> usize {
    u32::from(module.into_raw()) as usize
}

fn push<T>(
    table: &mut FxHashMap<String, Vec<Definition<T>>>,
    name: &str,
    item: T,
    range: TextRange,
    visibility: Visibility,
    usable: bool,
) {
    table.entry(name.to_string()).or_default().push(Definition {
        item,
        range,
        visibility,
        usable,
    });
}

fn defines_public<V: Namespace>(scope: &ModuleScope, name: &str) -> bool {
    V::definitions(scope).get(name).is_some_and(|definitions| {
        definitions
            .iter()
            .any(|definition| definition.visibility == Visibility::Public)
    })
}

/// 修飾か import の並びで引いた名前が、そのモジュールにない (E1001、E1002)。合流した修飾子では、すべてのモジュールを並べる
/// (docs/spec/modules.md の「名前の解決」)。
pub(crate) fn not_in_module(
    code: ErrorCode,
    file: FileId,
    range: TextRange,
    what: &str,
    name: &str,
    modules: &[&str],
) -> Diagnostic {
    let names: Vec<String> = modules.iter().map(|module| format!("`{module}`")).collect();
    let (place, label) = match names.len() {
        1 => ("module", "not found in this module"),
        _ => ("modules", "not found in these modules"),
    };
    Diagnostic::error(
        code,
        format!(
            "cannot find {what} `{name}` in {place} {}",
            names.join(", ")
        ),
        Label::new(file, range, label),
    )
}

/// 並びの名前 `T(..)` の `(..)` の位置。
fn all_range(parse: &Parse, name: TextRange) -> Option<TextRange> {
    parse
        .items()?
        .filter_map(|item| match item {
            ast::Item::ImportItem(import) => import.list(),
            _ => None,
        })
        .flat_map(|list| list.names())
        .find(|listed| {
            listed
                .name()
                .is_some_and(|name_ref| name_ref.token().text_range() == name)
        })?
        .all_constructors_range()
}

/// 抽象型か抽象エフェクトの `T(..)` (E1058)。型の名前だけは取り込むので、`T` と書けば足りる。
fn abstract_import(file: FileId, range: TextRange, name: &str, owner: TypeItem) -> Diagnostic {
    let parts = match owner {
        TypeItem::Effect(_) => "operation",
        TypeItem::Type(_) | TypeItem::Class(_) => "constructor",
    };
    Diagnostic::error(
        codes::ABSTRACT_MEMBER,
        format!("`{name}(..)` imports no {parts}: `{name}` is abstract"),
        Label::new(file, range, "hidden by `abstract`"),
    )
    .with_help(format!("write `{name}`"))
}

/// ユーザーのモジュールの `pub` でない名前を、修飾か import の並びで使った (E1029)。
pub(crate) fn private_name(
    file: FileId,
    range: TextRange,
    name: &str,
    (definition_file, definition): (FileId, TextRange),
) -> Diagnostic {
    Diagnostic::error(
        codes::PRIVATE_NAME,
        format!("`{name}` is not public"),
        Label::new(file, range, "private to its module"),
    )
    .with_secondary(Label::new(
        definition_file,
        definition,
        "defined here without `pub`",
    ))
}

/// Prelude から、処理系が役割で引く item を名前で引く。`pub` によらない。Prelude は処理系と一緒に配るソースなので、
/// 見つからなければ panic する。
fn lang_items(prelude: &ModuleScope) -> LangItems {
    let type_item = |name: &str| match prelude.types.get(name).and_then(|names| names.first()) {
        Some(definition) => definition.item,
        None => unreachable!("the Prelude declares `{name}`"),
    };
    let ty = |name: &str| match type_item(name) {
        TypeItem::Type(id) => id,
        _ => unreachable!("`{name}` is a type of the Prelude"),
    };
    let class = |name: &str| match type_item(name) {
        TypeItem::Class(id) => id,
        _ => unreachable!("`{name}` is a class of the Prelude"),
    };
    let value = |name: &str| match prelude.values.get(name).and_then(|names| names.first()) {
        Some(definition) => definition.item,
        None => unreachable!("the Prelude declares `{name}`"),
    };
    let constructor = |name: &str| match value(name) {
        ValueItem::Constructor(id) => id,
        _ => unreachable!("`{name}` is a constructor of the Prelude"),
    };
    let function = |name: &str| match value(name) {
        ValueItem::Function(id) => id,
        _ => unreachable!("`{name}` is a function of the Prelude"),
    };
    let method = |name: &str| match value(name) {
        ValueItem::Method(id) => id,
        _ => unreachable!("`{name}` is a method of the Prelude"),
    };
    LangItems {
        bool: ty("Bool"),
        true_ctor: constructor("True"),
        false_ctor: constructor("False"),
        and: function("&&"),
        or: function("||"),
        eq: class("Eq"),
        ord: class("Ord"),
        show: class("Show"),
        ordering: ty("Ordering"),
        list: ty("List"),
        nil: constructor("Nil"),
        cons: constructor("::"),
        display: method("display"),
        show_method: method("show"),
        negate: method("negate"),
        cmd_new: function("cmd_new"),
        cmd_spread: function("cmd_spread"),
    }
}

/// extern の表の行を、標準ライブラリのモジュールの正式な名前で引く。標準ライブラリは使うモジュールだけを読むので、型の
/// 行は読み込んだモジュールが宣言するものだけを持つ。`std/` は処理系と一緒に配るソースなので、Prelude の行の宣言が
/// なければ名前を添えて panic する。行と宣言の対応は `eml_hir` の結合テストが確かめる。
fn extern_index(scopes: &[ModuleScope]) -> ExternIndex {
    // 正式な名前を、モジュールの正式な名前と、そのモジュールの中の名前に分けて引く。演算子の名前は `.` を含みうるので、
    // 最後の `.` では分けない
    fn find<T: Copy, I>(
        scopes: &[ModuleScope],
        canonical: &str,
        table: impl Fn(&ModuleScope) -> &FxHashMap<String, Vec<Definition<T>>>,
        item: impl Fn(T) -> Option<I>,
    ) -> Option<I> {
        scopes
            .iter()
            .filter(|scope| scope.origin == ModuleOrigin::Std)
            .find_map(|scope| {
                let name = canonical
                    .strip_prefix(scope.name.as_str())?
                    .strip_prefix('.')?;
                item(table(scope).get(name)?.first()?.item)
            })
    }
    let types = ExternType::ALL
        .iter()
        .filter_map(|&ty| {
            let canonical = ty.row().name;
            let id = find(
                scopes,
                canonical,
                |scope| &scope.types,
                |item| match item {
                    TypeItem::Type(id) => Some(id),
                    TypeItem::Effect(_) | TypeItem::Class(_) => None,
                },
            );
            assert!(
                id.is_some() || !canonical.starts_with("Prelude."),
                "the standard library does not declare the type `{canonical}`"
            );
            Some((ty, id?))
        })
        .collect();
    let effect = |effect: ExternEffect| {
        let canonical = effect.row().name;
        find(
            scopes,
            canonical,
            |scope| &scope.types,
            |item| match item {
                TypeItem::Effect(id) => Some(id),
                TypeItem::Type(_) | TypeItem::Class(_) => None,
            },
        )
        .unwrap_or_else(|| panic!("the standard library does not declare the effect `{canonical}`"))
    };
    let io = effect(ExternEffect::Io);
    let panic = effect(ExternEffect::Panic);
    ExternIndex { types, io, panic }
}

/// 名前の表から表示名の表を作る。HIR の診断は `lower` の途中で出るので、`Program` より先に作る。重複した宣言の部品も
/// ID を持つので、使えるかによらず入れる。
fn display_names(scopes: &[ModuleScope], unit: TypeDefId) -> DisplayNames {
    let mut types = Vec::new();
    let mut effects = Vec::new();
    let mut constructors = Vec::new();
    let mut classes = Vec::new();
    for scope in scopes {
        let module = match scope.origin {
            ModuleOrigin::Session(number) => Definer::Session(number),
            ModuleOrigin::Std | ModuleOrigin::User => Definer::Module(&scope.name),
        };
        // 抽象型のコンストラクタも名前は外から見える (E1058 が名指す) ので、`pub` の部品と同じに数える
        let definer = |visibility: Visibility| match scope.origin {
            ModuleOrigin::Std if visibility == Visibility::Private => Definer::Hidden(&scope.name),
            _ => module,
        };
        for (name, definitions) in &scope.types {
            for definition in definitions {
                let module = definer(definition.visibility);
                match definition.item {
                    TypeItem::Type(id) => types.push((id, module, name.as_str())),
                    TypeItem::Effect(id) => effects.push((id, module, name.as_str())),
                    TypeItem::Class(id) => classes.push((id, module, name.as_str())),
                }
            }
        }
        for (name, definitions) in &scope.values {
            for definition in definitions {
                if let ValueItem::Constructor(id) = definition.item {
                    constructors.push((id, definer(definition.visibility), name.as_str()));
                }
            }
        }
    }
    DisplayNames::new(types, effects, constructors, classes, unit)
}

impl DefMap {
    pub fn prelude(&self) -> ModuleId {
        self.prelude
    }

    pub fn entry(&self) -> ModuleId {
        self.entry
    }

    pub fn module_name(&self, module: ModuleId) -> &str {
        &self.scope(module).name
    }

    pub fn origin(&self, module: ModuleId) -> ModuleOrigin {
        self.scope(module).origin
    }

    /// 2つのモジュールを、orphan 規則と fixity で同じモジュールとみなすか。セッションのモジュールどうしは、1つの
    /// モジュールとみなす (docs/spec/modules.md の「REPL のセッション」)。
    pub fn same_scope(&self, a: ModuleId, b: ModuleId) -> bool {
        a == b || (self.origin(a).session().is_some() && self.origin(b).session().is_some())
    }

    pub fn lang(&self) -> LangItems {
        self.lang
    }

    pub fn externs(&self) -> &ExternIndex {
        &self.externs
    }

    pub fn display_names(&self) -> &DisplayNames {
        &self.names
    }

    /// `module` の中から名前を引く口。
    pub fn resolver(&self, module: ModuleId) -> Resolver<'_> {
        Resolver {
            def_map: self,
            module,
        }
    }

    /// REPL の入力を実行する関数 `input$N` の ID。
    pub fn input_function(&self, module: ModuleId) -> Option<FunctionId> {
        self.scope(module).input_function
    }

    /// REPL の `let` の束縛の ID と根の番号。根の番号は入力の番号である
    /// (docs/spec/object-model.md の「セッションの根」)。
    pub fn binding(&self, module: ModuleId) -> Option<(BindingId, u32)> {
        let scope = self.scope(module);
        scope.binding.map(|binding| {
            let root = scope
                .origin
                .session()
                .expect("a let input is a session input");
            (binding, root)
        })
    }

    fn scope(&self, module: ModuleId) -> &ModuleScope {
        &self.modules[index(module)]
    }
}

/// モジュールの中から名前を引く口。修飾しない名前は「自分のモジュール → import の並びの名前 → Prelude の `pub` の名前」
/// の順に引き、修飾した名前は修飾子のモジュールだけを引く (docs/spec/modules.md の「名前の解決」)。セッションの
/// モジュールでは、自分のモジュールの次に前の入力のトップレベルを引き、import は入力ごとに新しい入力から順に引く
/// (docs/spec/modules.md の「REPL のセッション」)。
#[derive(Clone, Copy)]
pub struct Resolver<'a> {
    def_map: &'a DefMap,
    module: ModuleId,
}

impl<'a> Resolver<'a> {
    pub fn module(&self) -> ModuleId {
        self.module
    }

    /// HIR の診断が型、エフェクト、コンストラクタを書くときの表示名。
    pub fn names(&self) -> &'a DisplayNames {
        &self.def_map.names
    }

    pub fn externs(&self) -> &'a ExternIndex {
        &self.def_map.externs
    }

    fn own(&self) -> &'a ModuleScope {
        self.def_map.scope(self.module)
    }

    /// 自分が Prelude でなければ Prelude。
    fn prelude(&self) -> Option<&'a ModuleScope> {
        (self.module != self.def_map.prelude).then(|| self.def_map.scope(self.def_map.prelude))
    }

    pub fn value(&self, name: NameRef<'_>) -> Resolved<ValueItem> {
        self.lookup(name, |value: ValueItem| Some(value))
    }

    /// パターンの先頭の名前。コンストラクタだけから引く (規則3)。
    pub fn constructor(&self, name: NameRef<'_>) -> Resolved<ConstructorId> {
        self.lookup(name, |value: ValueItem| match value {
            ValueItem::Constructor(id) => Some(id),
            _ => None,
        })
    }

    /// handler の節の先頭の名前。操作だけから引く (規則3、docs/spec/modules.md の「名前の解決」)。
    pub fn operation(&self, name: NameRef<'_>) -> Resolved<OperationId> {
        self.lookup(name, |value: ValueItem| match value {
            ValueItem::Operation(id) => Some(id),
            _ => None,
        })
    }

    /// handler の節の先頭の名前を、extern の関数だけから引く。操作として見つからなかった名前が、handle できない
    /// extern のエフェクトを起こす関数かを確かめるのに使う (E1009)。引き方は `operation` と同じである。
    pub fn extern_function(&self, name: NameRef<'_>) -> Resolved<FunctionId> {
        self.lookup(name, |value: ValueItem| match value {
            ValueItem::Function(id)
                if self.def_map.scope(id.module).extern_functions.contains(&id) =>
            {
                Some(id)
            }
            _ => None,
        })
    }

    pub fn type_item(&self, name: NameRef<'_>) -> Resolved<TypeItem> {
        self.lookup(name, |item: TypeItem| Some(item))
    }

    /// 制約、instance の頭、`deriving` のクラスの名前。型の名前空間でクラスだけを引く。型かエフェクトに当たったら
    /// 呼び出し側が E1041 にできるよう、`type_item` で引き直す。
    pub fn class(&self, name: NameRef<'_>) -> Resolved<ClassId> {
        self.lookup(name, |item: TypeItem| match item {
            TypeItem::Class(id) => Some(id),
            TypeItem::Type(_) | TypeItem::Effect(_) => None,
        })
    }

    /// 自分のモジュールの `pub` でない型かエフェクトなら、その名前と定義の位置。公開の範囲の検査 (E1032) に使う
    /// (docs/spec/modules.md の「公開の範囲」)。ほかのモジュールの `pub` でない定義は名前で引けないので、自分の
    /// モジュールだけを見ればよい。
    pub fn private_type_item(&self, item: TypeItem) -> Option<(&'a str, TextRange)> {
        // セッションのモジュールはどこからも import されないので、公開の範囲を検査しない
        if self.own().origin.session().is_some() {
            return None;
        }
        self.own().types.iter().find_map(|(name, definitions)| {
            definitions
                .iter()
                .find(|definition| {
                    definition.item == item && definition.visibility == Visibility::Private
                })
                .map(|definition| (name.as_str(), definition.range))
        })
    }

    /// 型の型引数の種類。宣言の順である。
    pub fn type_params(&self, id: TypeDefId) -> &'a [ParamSort] {
        self.def_map
            .scope(id.module)
            .type_params
            .get(&id)
            .map_or(&[], Vec::as_slice)
    }

    /// `index` 番目の型引数を宣言に書いた位置とファイル。
    pub fn type_param_position(&self, id: TypeDefId, index: usize) -> Option<(FileId, TextRange)> {
        let scope = self.def_map.scope(id.module);
        let range = *scope.type_param_ranges.get(&id)?.get(index)?;
        Some((scope.file, range))
    }

    pub fn effect_params(&self, id: EffectId) -> usize {
        self.def_map
            .scope(id.module)
            .effect_params
            .get(&id)
            .copied()
            .unwrap_or(0)
    }

    /// 組み直しに使う fixity。名前を解決した先の定義に付き、別のモジュールの定義の fixity は宣言が `pub` のときだけ効く。
    /// セッションのモジュールどうしは同じモジュールとみなす (`DefMap::same_scope`)。
    /// 宣言がなければ `infixl 9` である (docs/spec/declarations.md の「fixity」)。重複した宣言の部品と、引けなかった名前も
    /// `infixl 9` で組む。曖昧な演算子と壊れた import から来た演算子は `None` で、組み直さない
    /// (docs/implementation/architecture.md の「名前解決の回復」)。
    pub fn fixity_of(&self, resolved: &Resolved<ValueItem>) -> Option<Fixity> {
        let item = match resolved {
            Resolved::Found(item) => *item,
            Resolved::Silent(Silence::Broken) | Resolved::Ambiguous(_) => return None,
            Resolved::Silent(Silence::Unusable)
            | Resolved::NotFound
            | Resolved::Private(..)
            | Resolved::Abstract(_)
            | Resolved::UnknownQualifier => return Some(Fixity::DEFAULT),
        };
        let module = item.module();
        match self.def_map.scope(module).fixities.get(&item) {
            Some((fixity, public, _))
                if self.def_map.same_scope(module, self.module) || *public =>
            {
                Some(*fixity)
            }
            _ => Some(Fixity::DEFAULT),
        }
    }

    /// 名前を引いて `fixity_of` を求める。セクションの被演算子の先読み (`looser_operator`) だけが使う。中置のパターンは
    /// 解決の結果を変換にも使うので、`value` で引いて `fixity_of` に渡す。
    pub fn fixity(&self, op: NameRef<'_>) -> Option<Fixity> {
        self.fixity_of(&self.value(op))
    }

    /// 修飾子が指すモジュールの名前 (`Report.Csv`)。壊れた import は数えない。E1001 と E1002 の「in module」に使う。
    pub fn qualifier_modules(&self, qualifier: &str) -> Vec<&'a str> {
        let mut names: Vec<&'a str> = Vec::new();
        for (target, _) in self.targets(qualifier) {
            if let ImportTarget::Module(module) = target {
                let name = self.def_map.scope(module).name.as_str();
                if !names.contains(&name) {
                    names.push(name);
                }
            }
        }
        names
    }

    /// モジュールの名前 (`Report.Csv`) に、このモジュールの import が与えた修飾子。import の順に並べる。E1031 の help が、
    /// パス全体を書いた修飾子の代わりに使える修飾子を示すのに使う (docs/spec/modules.md の「import」)。
    pub fn qualifiers_of(&self, module: &str) -> Vec<&'a str> {
        let mut names: Vec<&'a str> = Vec::new();
        let bounds = self
            .import_scopes()
            .flat_map(|scope| &scope.imports.qualifiers);
        for bound in bounds {
            let ImportTarget::Module(target) = bound.target else {
                continue;
            };
            if self.def_map.scope(target).name == module && !names.contains(&bound.name.as_str()) {
                names.push(&bound.name);
            }
        }
        names
    }

    /// 修飾子が標準ライブラリのモジュールの短い名前 (`Fs`) か正式な名前 (`Std.Fs`) なら、その短い名前。読み込んで
    /// いないモジュールも含む。E1031 の help と fix が使う (docs/spec/modules.md の「名前の解決」)。
    pub fn std_short_name_of<'q>(&self, qualifier: &'q str) -> Option<&'q str> {
        let short = qualifier
            .strip_prefix(STD_ROOT)
            .and_then(|rest| rest.strip_prefix('.'))
            .unwrap_or(qualifier);
        self.def_map.std_names.contains(short).then_some(short)
    }

    /// このモジュールに import を挿入する位置。import を書けない REPL の入力では `None` である。
    pub fn import_at(&self) -> Option<TextSize> {
        self.own().import_at
    }

    /// 修飾子が指すユーザーのモジュールが同じパスの標準ライブラリのモジュールを隠し、そちらが `name` を `pub` で
    /// 定義しているなら、その標準ライブラリのモジュールの短い名前 (`Fs`)。修飾して引けなかった名前の E1001 と E1002 の
    /// help が使う (docs/spec/modules.md の「標準ライブラリ」)。`types` は型の名前空間で引くか。
    pub fn hidden_std_module(&self, qualifier: &str, name: &str, types: bool) -> Option<&'a str> {
        self.targets(qualifier).into_iter().find_map(|(target, _)| {
            let ImportTarget::Module(module) = target else {
                return None;
            };
            let scope = self.def_map.scope(module);
            if scope.origin != ModuleOrigin::User {
                return None;
            }
            let (short, &std) = self.def_map.loaded_std.get_key_value(&scope.name)?;
            let std = self.def_map.scope(std);
            let defined = if types {
                defines_public::<TypeItem>(std, name)
            } else {
                defines_public::<ValueItem>(std, name)
            };
            defined.then_some(short.as_str())
        })
    }

    fn lookup<V: Namespace, T: PartialEq>(
        &self,
        name: NameRef<'_>,
        kind: impl Fn(V) -> Option<T>,
    ) -> Resolved<T> {
        match name {
            NameRef::Plain(name) => self.plain(name, kind),
            NameRef::Qualified { qualifier, name } => self.qualified(qualifier, name, kind),
        }
    }

    /// 種類の決まった位置では、どの段でも `kind` の種類の定義だけを見る。曖昧さも同じ種類の定義どうしでだけ数える
    /// (docs/spec/modules.md の「名前の解決」)。
    fn plain<V: Namespace, T: PartialEq>(
        &self,
        name: &str,
        kind: impl Fn(V) -> Option<T>,
    ) -> Resolved<T> {
        let own = self.own();
        let mut unusable = false;
        for definition in V::definitions(own).get(name).into_iter().flatten() {
            if definition.item.is_binding() {
                continue;
            }
            let Some(item) = kind(definition.item) else {
                continue;
            };
            if definition.usable {
                return Resolved::Found(item);
            }
            unusable = true;
        }
        // 重複した宣言の部品だけがある名前も、自分のモジュールの名前として import と Prelude の名前を隠す
        // (docs/spec/modules.md の「名前空間」の規則2)
        if unusable {
            return Resolved::Silent(Silence::Unusable);
        }
        // 前の入力のトップレベルは、新しい入力から順に引き、その名前を定義する最も新しいモジュールで止まる。種類で
        // 絞って古い版まで探すと、定義し直した名前の古い版が戻ってくるためである
        // (docs/spec/modules.md の「REPL のセッション」)
        for scope in self.earlier_sessions() {
            let Some(definitions) = V::definitions(scope).get(name) else {
                continue;
            };
            return match definitions.iter().find(|definition| definition.usable) {
                Some(definition) => {
                    kind(definition.item).map_or(Resolved::NotFound, Resolved::Found)
                }
                None => Resolved::Silent(Silence::Unusable),
            };
        }
        // import の並びの名前は入力ごとに引く。曖昧さは1つの入力の import の中でだけ数える
        for scope in self.import_scopes() {
            let mut found = Vec::new();
            let mut broken = false;
            for source in V::imported(&scope.imports).get(name).into_iter().flatten() {
                match source.item.map(&kind) {
                    None => broken = true,
                    Some(None) => {}
                    Some(Some(item)) => add(&mut found, item, Some((scope.file, source.import))),
                }
            }
            if let Some(resolved) = decide(found, broken) {
                return resolved;
            }
        }
        let from_prelude = self.prelude().and_then(|prelude| {
            V::definitions(prelude)
                .get(name)
                .into_iter()
                .flatten()
                .filter(|definition| {
                    definition.usable && definition.visibility == Visibility::Public
                })
                .find_map(|definition| kind(definition.item))
        });
        match from_prelude {
            Some(item) => Resolved::Found(item),
            // 部品の分からない `T(..)` は Prelude の名前を隠さない。名前を知らないので、どの名前を隠すかも決まらない
            None if self.import_scopes().any(|scope| V::unknown(&scope.imports)) => {
                Resolved::Silent(Silence::Broken)
            }
            None => Resolved::NotFound,
        }
    }

    /// 自分より前のセッションのモジュールを、新しい入力から順に並べる。セッションのモジュールは番号 1 から入力の順に
    /// 並ぶ (`Loaded::modules`)。自分がセッションのモジュールでなければ空である。
    fn earlier_sessions(&self) -> impl Iterator<Item = &'a ModuleScope> + 'a {
        let def_map = self.def_map;
        let start = if self.own().origin.session().is_some() {
            index(self.module)
        } else {
            0
        };
        (1..start)
            .rev()
            .map(move |module| &def_map.modules[module])
            .take_while(|scope| scope.origin.session().is_some())
    }

    /// import を引くモジュール。自分と、自分より前のセッションのモジュールを、新しい入力から順に並べる。
    fn import_scopes(&self) -> impl Iterator<Item = &'a ModuleScope> + 'a {
        std::iter::once(self.own()).chain(self.earlier_sessions())
    }

    /// 修飾子のモジュールだけを引く。合流した修飾子では、すべてのモジュールを合わせて引く (docs/spec/modules.md の
    /// 「import」)。
    fn qualified<V: Namespace, T: PartialEq>(
        &self,
        qualifier: &str,
        name: &str,
        kind: impl Fn(V) -> Option<T>,
    ) -> Resolved<T> {
        let targets = self.targets(qualifier);
        if targets.is_empty() {
            return Resolved::UnknownQualifier;
        }
        let mut found = Vec::new();
        let mut broken = false;
        let mut private = None;
        let mut hidden = None;
        let mut unusable = false;
        for (target, import) in targets {
            let ImportTarget::Module(module) = target else {
                broken = true;
                continue;
            };
            let scope = self.def_map.scope(module);
            for definition in V::definitions(scope).get(name).into_iter().flatten() {
                let Some(item) = kind(definition.item) else {
                    continue;
                };
                if !definition.usable {
                    unusable = true;
                    continue;
                }
                match definition.visibility {
                    Visibility::Public => add(&mut found, item, import),
                    // 抽象型と抽象エフェクトの部品は、標準ライブラリのものも「定義がない」とせず E1058 にする
                    // (docs/spec/modules.md の「公開の範囲」)
                    Visibility::Abstract => {
                        if hidden.is_none() {
                            hidden = Some(scope.hidden(definition));
                        }
                    }
                    // 標準ライブラリの `pub` でない item は、定義がないものとして扱う (docs/spec/modules.md の「標準ライブラリ」)
                    Visibility::Private if scope.origin == ModuleOrigin::User => {
                        private.get_or_insert((scope.file, definition.range));
                    }
                    Visibility::Private => {}
                }
                break;
            }
        }
        if let Some(resolved) = decide(found, broken) {
            return resolved;
        }
        if let Some(hidden) = hidden {
            return Resolved::Abstract(hidden);
        }
        match private {
            Some((file, range)) => Resolved::Private(file, range),
            None if unusable => Resolved::Silent(Silence::Unusable),
            None => Resolved::NotFound,
        }
    }

    /// 修飾子が指すモジュールと、それを作った import。`Prelude` はどのモジュールでも使える暗黙の修飾子で、import を
    /// 持たない (docs/spec/modules.md の「Prelude」)。ほかの修飾子は import だけが作る。import していないモジュールは、
    /// ユーザーのものも標準ライブラリのものも修飾子にならない (docs/spec/modules.md の「名前の解決」)。
    fn targets(&self, qualifier: &str) -> Vec<(ImportTarget, ImportAt)> {
        // セッションでは、その修飾子を与える最も新しい入力の import だけを使う
        let mut targets: Vec<(ImportTarget, ImportAt)> = self
            .import_scopes()
            .map(|scope| {
                scope
                    .imports
                    .qualifiers
                    .iter()
                    .filter(|bound| bound.name == qualifier)
                    .map(|bound| (bound.target, Some((scope.file, bound.import))))
                    .collect::<Vec<_>>()
            })
            .find(|targets| !targets.is_empty())
            .unwrap_or_default();
        if self
            .prelude()
            .is_some_and(|prelude| prelude.name == qualifier)
        {
            targets.push((ImportTarget::Module(self.def_map.prelude), None));
        }
        targets
    }
}
