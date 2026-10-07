mod data;
mod effect;
mod expr;
mod handler;
mod ops;
mod section;
mod types;

use eml_diagnostics::{Diagnostic, ErrorCode, FileId, Label, TextRange};
use eml_extern::Extern;
use eml_syntax::{SyntaxNode, SyntaxToken, ast};
use la_arena::{Arena, ArenaMap, Idx};

use crate::codes;
use crate::def_map::{DefMap, NameRef, Resolved, Resolver, module_id, not_in_module, private_name};
use crate::hir::*;
use crate::item_tree::{FunctionItem, ItemTree};
use crate::load::LoadedModule;
use crate::program::{ItemId, Items, Module, ModuleId, ModuleOrigin, Program};
use expr::BodyLowering;
use types::{TypeLowering, Vars};

/// 全モジュールの item と本体を変換する。名前は `def_map` で引き、item はその局所の番号の順にアリーナへ置く
/// (docs/implementation/architecture.md の「`eml_hir` の内部」)。
pub fn lower(def_map: &DefMap, modules: &[LoadedModule]) -> (Program, Vec<Diagnostic>) {
    let mut diagnostics = Vec::new();
    let mut arena = Arena::new();
    for (index, loaded) in modules.iter().enumerate() {
        let module = module_id(index);
        let id = arena.alloc(Module::new(
            loaded.tree.file,
            def_map.module_name(module),
            def_map.origin(module),
        ));
        debug_assert_eq!(id, module);
    }
    // `ItemTree` のポインタは、モジュールごとに1回だけ作った根から解決する
    let roots: Vec<SyntaxNode> = modules.iter().map(|loaded| loaded.parse.syntax()).collect();
    for (index, loaded) in modules.iter().enumerate() {
        let module = module_id(index);
        lower_items(
            def_map,
            module,
            &loaded.tree,
            &roots[index],
            &mut arena[module].items,
            &mut diagnostics,
        );
    }
    let lang = def_map.lang();
    for (index, loaded) in modules.iter().enumerate() {
        let module = module_id(index);
        let bodies = lower_bodies(
            def_map,
            module,
            &loaded.tree,
            &roots[index],
            &mut arena,
            &mut diagnostics,
        );
        arena[module].bodies = bodies;
    }
    (
        Program {
            modules: arena,
            prelude: def_map.prelude(),
            entry: def_map.entry(),
            lang,
            externs: def_map.externs().clone(),
            names: def_map.display_names().clone(),
        },
        diagnostics,
    )
}

/// item を `DefMap` と同じ局所の番号の順に置く。`ItemTree` の順である。
fn lower_items(
    def_map: &DefMap,
    module: ModuleId,
    tree: &ItemTree,
    root: &SyntaxNode,
    items: &mut Items,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let mut lowering = ItemLowering {
        file: tree.file,
        module,
        root,
        def_map,
        resolver: def_map.resolver(module),
        diagnostics,
    };
    lowering.declare_data(&tree.data, &mut items.types);
    lowering.declare_effects(&tree.effects, &mut items.effects);
    lowering.lower_operations(&tree.effects, items);
    lowering.lower_constructors(&tree.data, &mut items.types, &mut items.constructors);
    lowering.lower_functions(&tree.functions, &mut items.functions);
}

/// 1つのモジュールの item を変換する文脈。宣言の変換は、どれもこのモジュールのスコープで名前を引き、このファイルの
/// 診断を足す。
struct ItemLowering<'a> {
    file: FileId,
    module: ModuleId,
    /// `ItemTree` のポインタを解決する、このモジュールの構文木の根。
    root: &'a SyntaxNode,
    def_map: &'a DefMap,
    resolver: Resolver<'a>,
    diagnostics: &'a mut Vec<Diagnostic>,
}

impl ItemLowering<'_> {
    fn lower_functions(&mut self, items: &[FunctionItem], functions: &mut Arena<Function>) {
        for (k, function) in items.iter().enumerate() {
            let FunctionItem {
                name,
                first_range,
                public,
                signature,
                equations,
            } = function;
            let keyword = signature
                .as_ref()
                .and_then(|signature| signature.extern_keyword);
            let kind = match keyword {
                None => FunctionKind::Defined,
                Some(keyword) => {
                    FunctionKind::Extern(self.extern_row(keyword, name, Extern::from_name))
                }
            };
            // extern のシグネチャに続く等式は読み捨てる。E1033 のほかに診断を重ねないため
            let equations = match kind {
                FunctionKind::Defined => equations.as_slice(),
                FunctionKind::Extern(_) => &[],
            };
            if let (Some(signature), None, FunctionKind::Defined) =
                (signature, equations.first(), kind)
            {
                self.diagnostics.push(Diagnostic::error(
                    codes::MISSING_EQUATION,
                    format!("`{name}` has a signature but no equation"),
                    Label::new(
                        self.file,
                        signature.name_range,
                        format!("add an equation for `{name}` after this signature"),
                    ),
                ));
            }
            let signature_name_range = signature.as_ref().map(|signature| signature.name_range);
            let signature = signature.as_ref().map(|signature| {
                let node = signature.ptr.to_node(self.root);
                let range = node.ty().map_or(node.range(), |ty| ty.range());
                let mut types = Arena::new();
                let mut generics = Generics::default();
                let ty = TypeLowering {
                    file: self.file,
                    types: &mut types,
                    generics: &mut generics,
                    items: self.resolver,
                    vars: Vars::Define,
                    public_item: public.then_some(name.as_str()),
                    diagnostics: &mut *self.diagnostics,
                }
                .lower(node.ty(), range);
                Signature {
                    ty,
                    range,
                    types,
                    generics,
                }
            });
            let id = ItemId::new(
                self.module,
                functions.alloc(Function {
                    name: name.clone(),
                    name_range: equations.first().map_or(*first_range, |(_, range)| *range),
                    signature_name_range,
                    equation_ranges: equations.iter().map(|(_, range)| *range).collect(),
                    signature,
                    kind,
                }),
            );
            debug_assert_eq!(id, self.def_map.function_id(self.module, k));
        }
    }

    /// extern の宣言が指す表の行。標準ライブラリでは正式な名前 (`Prelude.+`) で表を引き、ない名前は `std/` の誤りなので
    /// panic する。ユーザーのモジュールでは E1033 を出して `None` にする。宣言は extern として読むので、E1005 や E1025 は
    /// 重ねない。
    fn extern_row<T>(
        &mut self,
        keyword: TextRange,
        name: &str,
        from_name: impl Fn(&str) -> Option<T>,
    ) -> Option<T> {
        match self.def_map.origin(self.module) {
            ModuleOrigin::Std => {
                let canonical = format!("{}.{name}", self.def_map.module_name(self.module));
                let row = from_name(&canonical)
                    .unwrap_or_else(|| panic!("`{canonical}` is not in the extern table"));
                Some(row)
            }
            ModuleOrigin::User => {
                self.diagnostics
                    .push(extern_outside_std(self.file, keyword));
                None
            }
        }
    }
}

/// 本体は、すべてのモジュールの item を置いてから変換する。後ろで定義した関数も、ほかのモジュールの item も引けるように
/// するため。
fn lower_bodies(
    def_map: &DefMap,
    module: ModuleId,
    tree: &ItemTree,
    root: &SyntaxNode,
    modules: &mut Arena<Module>,
    diagnostics: &mut Vec<Diagnostic>,
) -> ArenaMap<Idx<Function>, Body> {
    let mut bodies = ArenaMap::default();
    for (k, function) in tree.functions.iter().enumerate() {
        let id = def_map.function_id(module, k);
        if function.equations.is_empty()
            || modules[module].items.functions[id.local].kind != FunctionKind::Defined
        {
            continue;
        }
        // シグネチャの型変数の表は関数のアリーナの中にあり、本体の変換はほかの item を同じアリーナから読む。
        // そのため、変換の間だけ表を取り出す。シグネチャがなければ、本体の注釈は型変数を引けない
        // (docs/spec/types.md の「推論」)
        let mut generics = modules[module].items.functions[id.local]
            .signature
            .as_mut()
            .map(|signature| std::mem::take(&mut signature.generics))
            .unwrap_or_default();
        let equations: Vec<(ast::Equation, TextRange)> = function
            .equations
            .iter()
            .map(|(ptr, range)| (ptr.to_node(root), *range))
            .collect();
        let body = BodyLowering::new(
            tree.file,
            def_map.resolver(module),
            modules,
            def_map.lang(),
            def_map.externs().negate,
            &mut generics,
            diagnostics,
        )
        .lower_equations(&equations);
        if let Some(signature) = &mut modules[module].items.functions[id.local].signature {
            signature.generics = generics;
        }
        bodies.insert(id.local, body);
    }
    bodies
}

/// E1033。`extern` を外すと E1005 や E1025 になるので、自動の修正にはせず help で示すだけにする。
fn extern_outside_std(file: FileId, keyword: TextRange) -> Diagnostic {
    Diagnostic::error(
        codes::EXTERN_OUTSIDE_STD,
        "`extern` is only allowed in the standard library",
        Label::new(file, keyword, "user modules cannot declare externs"),
    )
    .with_help("remove `extern` and define the declaration in eml")
}

/// 名前の経路の読み方 (docs/spec/modules.md の「名前の解決」)。
pub(super) enum PathName {
    Plain(SyntaxToken),
    /// `Csv.parse`。修飾子は最後より前のセグメントを `.` でつないだもので、2つ以上のセグメントなら E1031 になる。
    Qualified {
        qualifier: String,
        qualifier_range: TextRange,
        name: SyntaxToken,
    },
    /// パーサが報告済み。
    Missing,
}

pub(super) fn path_name(path: Option<ast::Path>) -> PathName {
    let Some(path) = path else {
        return PathName::Missing;
    };
    let segments: Vec<SyntaxToken> = path.segments().map(|segment| segment.token()).collect();
    match segments.as_slice() {
        [] => PathName::Missing,
        [name] => PathName::Plain(name.clone()),
        [qualifier @ .., name] => PathName::Qualified {
            qualifier: qualifier
                .iter()
                .map(|segment| segment.text())
                .collect::<Vec<_>>()
                .join("."),
            qualifier_range: qualifier[0]
                .text_range()
                .cover(qualifier[qualifier.len() - 1].text_range()),
            name: name.clone(),
        },
    }
}

impl PathName {
    /// 最後のセグメント。
    pub(super) fn token(&self) -> Option<&SyntaxToken> {
        match self {
            PathName::Plain(name) | PathName::Qualified { name, .. } => Some(name),
            PathName::Missing => None,
        }
    }

    /// `range` は、修飾子を含む名前の全体の位置である。
    pub(super) fn at(&self, range: TextRange) -> Option<NameUse<'_>> {
        match self {
            PathName::Plain(name) => Some(NameUse::plain(name.text(), range)),
            PathName::Qualified {
                qualifier,
                qualifier_range,
                name,
            } => Some(NameUse {
                name: NameRef::Qualified {
                    qualifier,
                    name: name.text(),
                },
                range,
                qualifier_range: Some(*qualifier_range),
            }),
            PathName::Missing => None,
        }
    }
}

/// 名前の種類。引けなかったときの診断の番号と文言を決める。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum NameKind {
    Value,
    Constructor,
    Operator,
    Operation,
    Type,
    Effect,
}

impl NameKind {
    fn code(self) -> ErrorCode {
        match self {
            NameKind::Type | NameKind::Effect => codes::UNDEFINED_TYPE,
            NameKind::Value | NameKind::Constructor | NameKind::Operator | NameKind::Operation => {
                codes::UNDEFINED_NAME
            }
        }
    }

    fn noun(self) -> &'static str {
        match self {
            NameKind::Value => "value",
            NameKind::Constructor => "constructor",
            NameKind::Operator => "operator",
            NameKind::Operation => "effect operation",
            NameKind::Type => "type",
            NameKind::Effect => "effect",
        }
    }

    fn label(self) -> &'static str {
        match self {
            NameKind::Operation => "not an operation of any effect",
            _ => "not found in this scope",
        }
    }
}

/// 名前を使った位置。
pub(super) struct NameUse<'a> {
    pub name: NameRef<'a>,
    /// 修飾子を含む名前の全体。E1001、E1002、E1028、E1029 が指す。
    pub range: TextRange,
    /// 修飾子の位置。修飾した名前だけが持ち、E1031 が指す。
    pub qualifier_range: Option<TextRange>,
}

impl<'a> NameUse<'a> {
    pub(super) fn plain(name: &'a str, range: TextRange) -> NameUse<'a> {
        NameUse {
            name: NameRef::Plain(name),
            range,
            qualifier_range: None,
        }
    }

    /// 修飾子を除いた名前。
    pub(super) fn last(&self) -> &'a str {
        match self.name {
            NameRef::Plain(name) | NameRef::Qualified { name, .. } => name,
        }
    }

    /// 書いたままの名前 (`Csv.parse`)。
    pub(super) fn written(&self) -> String {
        match self.name {
            NameRef::Plain(name) => name.to_string(),
            NameRef::Qualified { qualifier, name } => format!("{qualifier}.{name}"),
        }
    }
}

/// 名前が見つからない (E1001、E1002)。修飾した名前には、修飾子のモジュールを書く (docs/spec/modules.md の「名前の解決」)。
pub(super) fn not_found(
    items: &Resolver<'_>,
    file: FileId,
    kind: NameKind,
    at: &NameUse<'_>,
) -> Diagnostic {
    match at.name {
        NameRef::Plain(name) => Diagnostic::error(
            kind.code(),
            format!("cannot find {} `{name}`", kind.noun()),
            Label::new(file, at.range, kind.label()),
        ),
        NameRef::Qualified { qualifier, name } => {
            let diagnostic = not_in_module(
                kind.code(),
                file,
                at.range,
                kind.noun(),
                name,
                &items.qualifier_modules(qualifier),
            );
            let types = matches!(kind, NameKind::Type | NameKind::Effect);
            match items.hidden_std_module(qualifier, name, types) {
                Some(short) => diagnostic.with_help(format!(
                    "the standard `{short}` is hidden by your module `{short}`; `import Std.{short} as {}` reaches it",
                    &short[..1]
                )),
                None => diagnostic,
            }
        }
    }
}

/// 名前を引けなかった結果の診断。`Silent` は、重複した宣言の部品 (E1003 で報告済み) か、壊れた import や並びで報告した
/// 名前なので、診断を出さない (docs/implementation/architecture.md の「名前解決の回復」)。
pub(super) fn unresolved<T>(
    items: &Resolver<'_>,
    file: FileId,
    kind: NameKind,
    at: &NameUse<'_>,
    result: Resolved<T>,
) -> Option<Diagnostic> {
    match result {
        Resolved::Found(_) | Resolved::Silent(_) => None,
        Resolved::NotFound => Some(not_found(items, file, kind, at)),
        Resolved::Ambiguous(imports) => Some(ambiguous(file, at, &imports)),
        Resolved::Private(definition_file, definition) => Some(private_name(
            file,
            at.range,
            at.last(),
            (definition_file, definition),
        )),
        Resolved::UnknownQualifier => match at.name {
            NameRef::Qualified { qualifier, name } => Some(unknown_qualifier(
                items,
                file,
                qualifier,
                name,
                at.qualifier_range.unwrap_or(at.range),
            )),
            // 修飾しない名前は修飾子の誤りにならない
            NameRef::Plain(_) => None,
        },
    }
}

/// E1031。修飾子は1つのセグメントなので、パス全体を書いた `Report.Csv.parse` もここに来る。そのモジュールを import して
/// いれば、使える修飾子を help で示す (docs/spec/modules.md の「import」)。標準ライブラリのモジュールの正式な名前
/// (`Std.Fs.close`) を書いたときは、短い名前の修飾子を示す (docs/spec/modules.md の「名前の解決」)。
fn unknown_qualifier(
    items: &Resolver<'_>,
    file: FileId,
    qualifier: &str,
    name: &str,
    range: TextRange,
) -> Diagnostic {
    let diagnostic = Diagnostic::error(
        codes::UNKNOWN_QUALIFIER,
        format!("unknown module qualifier `{qualifier}`"),
        Label::new(file, range, "no import gives this qualifier"),
    );
    match items.qualifiers_of(qualifier).first() {
        Some(usable) => diagnostic.with_help(format!(
            "the import of `{qualifier}` gives the qualifier `{usable}`; write `{usable}.{name}`"
        )),
        None => match items.std_short_name_of(qualifier) {
            Some(short) => diagnostic.with_help(format!(
                "write `{short}.{name}`, or `import {qualifier} as {}`",
                &short[..1]
            )),
            None => diagnostic,
        },
    }
}

/// E1028。secondary は、別々の定義をそれぞれ持ち込んだ import である (docs/spec/modules.md の「名前の解決」)。
pub(super) fn ambiguous(file: FileId, at: &NameUse<'_>, imports: &[TextRange]) -> Diagnostic {
    let diagnostic = Diagnostic::error(
        codes::AMBIGUOUS_NAME,
        format!("`{}` is ambiguous", at.written()),
        Label::new(
            file,
            at.range,
            "this name refers to more than one definition",
        ),
    );
    imports.iter().fold(diagnostic, |diagnostic, &import| {
        diagnostic.with_secondary(Label::new(
            file,
            import,
            "one of the definitions is imported here",
        ))
    })
}
