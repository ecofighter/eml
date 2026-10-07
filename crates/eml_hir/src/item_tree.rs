//! item の収集 (docs/implementation/architecture.md の「`eml_hir` の内部」)。ファイルごとに宣言を集め、名前を解決
//! しなくても判定できる並び方の誤りを出す。名前の表と重複の判定は `DefMap` が行う。

use std::collections::HashMap;

use eml_diagnostics::{Diagnostic, FileId, Label, TextRange};
use eml_syntax::{SyntaxKind, SyntaxToken, ast};

use crate::codes;

/// 1つのファイルのトップレベルの宣言。各並びはソースの順で、モジュールの中の item の局所の番号の順でもある。
#[derive(Debug)]
pub struct ItemTree {
    pub file: FileId,
    /// ソースの順。宣言の後の import (E0011) も含む。読み込みの段が、この順にたどる。
    pub imports: Vec<ImportItem>,
    pub functions: Vec<FunctionItem>,
    pub data: Vec<DataItem>,
    pub effects: Vec<EffectItem>,
    pub fixities: Vec<FixityItem>,
}

/// 同じ名前のシグネチャと等式をまとめたもの。名前で対応づけてから並び方を検査する (docs/spec/declarations.md)。
#[derive(Debug)]
pub struct FunctionItem {
    pub name: String,
    /// 最初に現れたシグネチャか等式の名前の位置。
    pub first_range: TextRange,
    pub public: bool,
    /// (シグネチャ、名前の位置)
    pub signature: Option<(ast::Signature, TextRange)>,
    /// (等式、名前の位置)。ソースの順である。
    pub equations: Vec<(ast::Equation, TextRange)>,
}

#[derive(Debug)]
pub struct DataItem {
    pub name: String,
    pub name_range: TextRange,
    pub public: bool,
    pub syntax: ast::DataItem,
    /// 名前のある選択肢。ソースの順で、コンストラクタの局所の番号の順である。
    pub constructors: Vec<ConstructorItem>,
}

#[derive(Debug)]
pub struct ConstructorItem {
    pub name: String,
    pub name_range: TextRange,
    pub syntax: ast::Alt,
}

#[derive(Debug)]
pub struct EffectItem {
    pub name: String,
    pub name_range: TextRange,
    pub public: bool,
    pub syntax: ast::EffectItem,
    /// 名前のある操作の宣言。ソースの順で、操作の局所の番号の順である。
    pub operations: Vec<OperationItem>,
}

#[derive(Debug)]
pub struct OperationItem {
    pub name: String,
    pub name_range: TextRange,
    pub syntax: ast::OpDecl,
}

#[derive(Debug)]
pub struct FixityItem {
    pub public: bool,
    /// 結合と優先順位が読めなければ `None` (パーサが報告済み)。
    pub fixity: Option<Fixity>,
    /// (演算子、位置)
    pub operators: Vec<(String, TextRange)>,
}

/// モジュールのパス。`Report.Csv` は ["Report", "Csv"]。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ModulePath(pub Vec<String>);

impl ModulePath {
    pub fn dotted(&self) -> String {
        self.0.join(".")
    }

    /// 根からの相対パス。モジュール名とパスは大文字小文字まで一致させる (docs/spec/modules.md の「モジュール」)。
    pub fn file_path(&self) -> String {
        format!("{}.em", self.0.join("/"))
    }

    pub fn last(&self) -> &str {
        self.0.last().expect("a module path has a segment")
    }
}

#[derive(Debug)]
pub struct ImportItem {
    pub path: ModulePath,
    pub path_range: TextRange,
    /// 修飾子 (別名か最後のセグメント) と、その位置 (別名がなければパスの位置)。
    pub qualifier: (String, TextRange),
    pub has_alias: bool,
    pub list: Option<Vec<ImportName>>,
    /// import の全体。
    pub range: TextRange,
}

#[derive(Debug, Clone)]
pub enum ImportName {
    /// 小文字の名前か `(op)`。
    Value { name: String, range: TextRange },
    /// 大文字の名前。`T(..)` / `E(..)` なら `all` が真。
    Type {
        name: String,
        range: TextRange,
        all: bool,
    },
}

/// 演算子の結合の向き (docs/spec/declarations.md の「fixity」)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Assoc {
    Left,
    Right,
    None,
}

/// 演算子の優先順位と結合 (docs/spec/declarations.md の「fixity」)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fixity {
    pub precedence: u8,
    pub assoc: Assoc,
}

impl Fixity {
    /// fixity の宣言がない演算子 (Haskell と同じ)。
    pub const DEFAULT: Fixity = Fixity {
        precedence: 9,
        assoc: Assoc::Left,
    };
}

/// 関数を組み立てる途中の形。シグネチャと等式の item の番号は、並び方の検査だけに使う。
struct Definition {
    name: String,
    first_range: TextRange,
    public: bool,
    /// (item の番号, シグネチャ, 名前の位置)
    signature: Option<(usize, ast::Signature, TextRange)>,
    /// (item の番号, 等式, 名前の位置)
    equations: Vec<(usize, ast::Equation, TextRange)>,
}

/// トップレベルの宣言を集める。E1003 (シグネチャの重複)、E1004、E1018、E1019 と、`type` の E0004 を出す。
/// E1005 は、Prelude の等式のないシグネチャが intrinsic であり、モジュールの種類を知らないここでは決められないので、
/// `lower` が出す。
pub fn item_tree(file: FileId, source: &ast::SourceFile) -> (ItemTree, Vec<Diagnostic>) {
    let mut diagnostics = Vec::new();
    let mut definitions: Vec<Definition> = Vec::new();
    let mut by_name: HashMap<String, usize> = HashMap::new();
    let mut data = Vec::new();
    let mut effects = Vec::new();
    let mut fixities = Vec::new();
    let mut imports = Vec::new();
    for (index, item) in source.items().enumerate() {
        let public = item.pub_keyword().is_some();
        match item {
            ast::Item::Signature(signature) => {
                let Some(name) = value_name(signature.name()) else {
                    continue;
                };
                let range = name.text_range();
                let slot = slot(&mut definitions, &mut by_name, name.text(), range);
                let definition = &mut definitions[slot];
                definition.public |= public;
                match &definition.signature {
                    Some((_, _, first)) => diagnostics.push(
                        Diagnostic::error(
                            codes::DUPLICATE_DEFINITION,
                            format!("`{}` is defined more than once", name.text()),
                            Label::new(file, range, "defined again here"),
                        )
                        .with_secondary(Label::new(
                            file,
                            *first,
                            "first defined here",
                        )),
                    ),
                    None => definition.signature = Some((index, signature, range)),
                }
            }
            ast::Item::Equation(equation) => {
                let Some(name) = value_name(equation.name()) else {
                    continue;
                };
                let range = name.text_range();
                let slot = slot(&mut definitions, &mut by_name, name.text(), range);
                definitions[slot].equations.push((index, equation, range));
            }
            ast::Item::DataItem(item) => {
                // 名前がなければパーサが報告済み
                let Some(name) = item.name().map(|name| name.token()) else {
                    continue;
                };
                let constructors = item
                    .alts()
                    .filter_map(|alt| {
                        // 名前も演算子もなければパーサが報告済み
                        let name = alt.name().or_else(|| alt.operator())?.token();
                        Some(ConstructorItem {
                            name: name.text().to_string(),
                            name_range: name.text_range(),
                            syntax: alt,
                        })
                    })
                    .collect();
                data.push(DataItem {
                    name: name.text().to_string(),
                    name_range: name.text_range(),
                    public,
                    syntax: item,
                    constructors,
                });
            }
            ast::Item::EffectItem(item) => {
                // 名前がなければパーサが報告済み
                let Some(name) = item.name().map(|name| name.token()) else {
                    continue;
                };
                let operations = item
                    .operations()
                    .filter_map(|decl| {
                        // 名前がなければパーサが報告済み
                        let name = decl.name()?.token();
                        Some(OperationItem {
                            name: name.text().to_string(),
                            name_range: name.text_range(),
                            syntax: decl,
                        })
                    })
                    .collect();
                effects.push(EffectItem {
                    name: name.text().to_string(),
                    name_range: name.text_range(),
                    public,
                    syntax: item,
                    operations,
                });
            }
            ast::Item::FixityItem(item) => fixities.push(FixityItem {
                public,
                fixity: fixity_of(&item),
                operators: item
                    .operators()
                    .map(|name| {
                        let token = name.token();
                        (token.text().to_string(), token.text_range())
                    })
                    .collect(),
            }),
            ast::Item::TypeItem(item) => {
                let range = item
                    .type_keyword()
                    .map_or(item.range(), |keyword| keyword.text_range());
                diagnostics.push(Diagnostic::not_yet_supported(
                    file,
                    range,
                    "`type` declarations are not supported yet",
                ));
            }
            ast::Item::ImportItem(item) => imports.extend(import_of(&item)),
        }
    }
    let functions = definitions
        .into_iter()
        .map(|definition| check_order(file, definition, &mut diagnostics))
        .collect();
    (
        ItemTree {
            file,
            imports,
            functions,
            data,
            effects,
            fixities,
        },
        diagnostics,
    )
}

/// 等式は連続し、シグネチャの直後に置く (docs/spec/declarations.md)。離れていても、網羅性の誤りを連鎖させないよう
/// ソースの順に1つの関数として扱う。
fn check_order(
    file: FileId,
    definition: Definition,
    diagnostics: &mut Vec<Diagnostic>,
) -> FunctionItem {
    let Definition {
        name,
        first_range,
        public,
        signature,
        equations,
    } = definition;
    for pair in equations.windows(2) {
        let (previous_index, _, previous_range) = &pair[0];
        let (index, _, range) = &pair[1];
        if *index != previous_index + 1 {
            diagnostics.push(
                Diagnostic::error(
                    codes::NON_CONSECUTIVE_EQUATIONS,
                    format!("the equations of `{name}` are not consecutive"),
                    Label::new(
                        file,
                        *range,
                        "this equation is separated from the ones above",
                    ),
                )
                .with_secondary(Label::new(file, *previous_range, "the previous equation"))
                .with_help(format!(
                    "put every equation of `{name}` together, right after its signature"
                )),
            );
        }
    }
    match (&signature, equations.first()) {
        (None, Some((_, _, range))) => diagnostics.push(
            Diagnostic::error(
                codes::MISSING_SIGNATURE,
                format!("`{name}` has no type signature"),
                Label::new(file, *range, "every top-level definition needs a signature"),
            )
            .with_help(format!(
                "add a signature `{name} : ...` on the line before this equation"
            )),
        ),
        (Some((signature_index, _, signature_range)), Some((equation_index, _, range)))
            if *equation_index != signature_index + 1 =>
        {
            diagnostics.push(
                Diagnostic::error(
                    codes::SIGNATURE_NOT_ADJACENT,
                    format!("the signature of `{name}` is not followed by its equations"),
                    Label::new(
                        file,
                        *range,
                        format!("this equation is not right after the signature of `{name}`"),
                    ),
                )
                .with_secondary(Label::new(file, *signature_range, "the signature is here"))
                .with_help(format!(
                    "move the equations of `{name}` right after its signature"
                )),
            );
        }
        _ => {}
    }
    FunctionItem {
        name,
        first_range,
        public,
        signature: signature.map(|(_, node, range)| (node, range)),
        equations: equations
            .into_iter()
            .map(|(_, node, range)| (node, range))
            .collect(),
    }
}

fn slot(
    definitions: &mut Vec<Definition>,
    by_name: &mut HashMap<String, usize>,
    name: &str,
    range: TextRange,
) -> usize {
    *by_name.entry(name.to_string()).or_insert_with(|| {
        definitions.push(Definition {
            name: name.to_string(),
            first_range: range,
            public: false,
            signature: None,
            equations: Vec::new(),
        });
        definitions.len() - 1
    })
}

/// パスが読めなかった import はパーサが報告済みなので、読み込みもスコープへの登録もしない。
fn import_of(item: &ast::ImportItem) -> Option<ImportItem> {
    let path = item.path()?;
    let path_range = path.range();
    let module = ModulePath(path.segments().map(|segment| segment.text()).collect());
    let alias = item.alias().map(|name| name.token());
    let qualifier = match &alias {
        Some(token) => (token.text().to_string(), token.text_range()),
        None => (module.last().to_string(), path_range),
    };
    let list = item
        .list()
        .map(|list| list.names().filter_map(|name| import_name(&name)).collect());
    Some(ImportItem {
        path: module,
        path_range,
        qualifier,
        has_alias: alias.is_some(),
        list,
        range: item.range(),
    })
}

/// 並びの名前。大文字の名前は型かエフェクトだけを指す (docs/spec/modules.md の「import」)。
fn import_name(name: &ast::ImportName) -> Option<ImportName> {
    if let Some(name_ref) = name.name() {
        let token = name_ref.token();
        let text = token.text().to_string();
        let range = token.text_range();
        return Some(match token.kind() {
            SyntaxKind::UIDENT => ImportName::Type {
                name: text,
                range,
                all: name.all_constructors(),
            },
            _ => ImportName::Value { name: text, range },
        });
    }
    // `:` で始まる演算子はパーサが E0011 にした。中置のコンストラクタは `T(..)` で取り込む (docs/spec/modules.md の「import」)
    let operator = name
        .operator()
        .filter(|token| token.kind() != SyntaxKind::CONOP)?;
    Some(ImportName::Value {
        name: operator.text().to_string(),
        range: operator.text_range(),
    })
}

/// シグネチャと等式の名前。演算子の定義 (`(</>) : …` と `a </> b = …`) は、演算子の文字列を名前にした関数である
/// (docs/spec/declarations.md の「fixity」)。名前がなければパーサが報告済み。
fn value_name(name: Option<ast::Name>) -> Option<SyntaxToken> {
    name.map(|name| name.token()).filter(|token| {
        matches!(
            token.kind(),
            SyntaxKind::LIDENT | SyntaxKind::OP | SyntaxKind::MINUS
        )
    })
}

/// fixity の宣言の結合と優先順位。優先順位の範囲の誤りはパーサが報告済みなので、読めなければ `None` にする。
pub(crate) fn fixity_of(item: &ast::FixityItem) -> Option<Fixity> {
    let assoc = match item.assoc()?.kind() {
        SyntaxKind::INFIXL_KW => Assoc::Left,
        SyntaxKind::INFIXR_KW => Assoc::Right,
        _ => Assoc::None,
    };
    let precedence = item
        .precedence()?
        .text()
        .parse::<u8>()
        .ok()
        .filter(|precedence| *precedence <= 9)?;
    Some(Fixity { precedence, assoc })
}
