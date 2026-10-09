//! item の収集 (docs/implementation/architecture.md の「`eml_hir` の内部」)。ファイルごとに宣言を集め、名前を解決
//! しなくても判定できる並び方の誤りを出す。名前の表と、トップレベルの名前の重複の判定は `DefMap` が行う。シグネチャの重複と
//! 型引数の重複 (E1003) は、ここで報告する。
//!
//! `ItemTree` は構文木のノードを持たない。名前を解決する前に決まる情報 (名前、位置、`pub`、`extern`、型引数) は、
//! ここで取り出して持つ。型やパターンの変換のように resolver の要るものだけを `AstPtr` で指し、`lower` がモジュールの
//! 構文木の根から解決する。ノードを持たないので、`ItemTree` はスレッドをまたげる。

use std::collections::HashMap;

use eml_diagnostics::{Diagnostic, FileId, Label, TextRange};
use eml_syntax::{AstPtr, Parse, SyntaxKind, SyntaxToken, ast};

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
    pub classes: Vec<ClassItem>,
    pub instances: Vec<InstanceItem>,
}

/// 同じ名前のシグネチャと等式をまとめたもの。名前で対応づけてから並び方を検査する (docs/spec/declarations.md)。
#[derive(Debug)]
pub struct FunctionItem {
    pub name: String,
    /// 最初に現れたシグネチャか等式の名前の位置。
    pub first_range: TextRange,
    pub public: bool,
    pub signature: Option<SignatureItem>,
    /// (等式、名前の位置)。ソースの順である。
    pub equations: Vec<(AstPtr<ast::Equation>, TextRange)>,
}

#[derive(Debug)]
pub struct SignatureItem {
    pub ptr: AstPtr<ast::Signature>,
    pub name_range: TextRange,
    /// `extern` はシグネチャにだけ書ける (docs/spec/declarations.md の「`extern`」)。
    pub extern_keyword: Option<TextRange>,
}

#[derive(Debug)]
pub struct DataItem {
    pub name: String,
    pub name_range: TextRange,
    pub public: bool,
    pub extern_keyword: Option<TextRange>,
    /// 型引数 (名前、位置)。重複した名前 (E1003) は最初の1つだけを残す。
    pub params: Vec<(String, TextRange)>,
    /// `=` か選択肢を書いたか (`ast::DataItem::has_constructors`)。
    pub has_constructors: bool,
    /// 名前のある選択肢。ソースの順で、コンストラクタの局所の番号の順である。
    pub constructors: Vec<ConstructorItem>,
    /// `deriving` に書いたクラス (名前、位置)。書いた順で、重複 (E1035) もそのまま持つ。
    pub deriving: Vec<(AstPtr<ast::Path>, TextRange)>,
}

#[derive(Debug)]
pub struct ConstructorItem {
    pub name: String,
    pub name_range: TextRange,
    pub ptr: AstPtr<ast::Alt>,
    /// 演算子の名前の中置のコンストラクタ (`Int :+ Chain`) か。
    pub infix: bool,
}

#[derive(Debug)]
pub struct EffectItem {
    pub name: String,
    pub name_range: TextRange,
    pub public: bool,
    pub extern_keyword: Option<TextRange>,
    /// 型引数 (名前、位置)。重複した名前 (E1003) は最初の1つだけを残す。
    pub params: Vec<(String, TextRange)>,
    /// 名前のある操作の宣言。ソースの順で、操作の局所の番号の順である。
    pub operations: Vec<OperationItem>,
}

#[derive(Debug)]
pub struct OperationItem {
    pub name: String,
    pub name_range: TextRange,
    pub ptr: AstPtr<ast::OpDecl>,
}

#[derive(Debug)]
pub struct ClassItem {
    pub name: String,
    pub name_range: TextRange,
    pub public: bool,
    /// 型変数 (名前、位置)。
    pub var: (String, TextRange),
    pub ptr: AstPtr<ast::ClassItem>,
    /// シグネチャを持つメソッド。シグネチャのない等式は `check_order` が E1004 にして捨てる。
    pub methods: Vec<FunctionItem>,
}

#[derive(Debug)]
pub struct InstanceItem {
    pub ptr: AstPtr<ast::InstanceItem>,
    pub keyword_range: TextRange,
    /// 等式と `extern` の行を名前でまとめたもの。最初に現れた順である。
    pub members: Vec<MemberItem>,
}

#[derive(Debug)]
pub struct MemberItem {
    pub name: String,
    pub name_range: TextRange,
    pub equations: Vec<(AstPtr<ast::Equation>, TextRange)>,
    /// `extern` の行のキーワードの位置。
    pub extern_range: Option<TextRange>,
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
    /// 修飾子 (別名か最後のセグメント)。
    pub qualifier: String,
    pub list: Option<Vec<ImportName>>,
    /// import の全体。
    pub range: TextRange,
    /// パスの後ろに構文の誤りがある。読み込みの段はファイルを読まずに壊れた import にする。
    pub malformed: bool,
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
    /// (item の番号, シグネチャ)
    signature: Option<(usize, SignatureItem)>,
    /// (item の番号, 等式, 名前の位置)
    equations: Vec<(usize, AstPtr<ast::Equation>, TextRange)>,
}

/// シグネチャと等式を名前でまとめる (docs/spec/declarations.md の「シグネチャと等式」)。トップレベルとクラスの
/// ブロックが同じ規則を使う。番号は並びの中の位置で、隣り合っているかの検査だけに使う。
#[derive(Default)]
struct Definitions {
    list: Vec<Definition>,
    by_name: HashMap<String, usize>,
}

impl Definitions {
    fn signature(
        &mut self,
        index: usize,
        signature: &ast::Signature,
        public: bool,
        file: FileId,
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        let Some(name) = value_name(signature.name()) else {
            return;
        };
        let range = name.text_range();
        let definition = self.slot(name.text(), range);
        definition.public |= public;
        match &definition.signature {
            Some((_, first)) => {
                diagnostics.push(duplicate(file, name.text(), first.name_range, range))
            }
            None => {
                definition.signature = Some((
                    index,
                    SignatureItem {
                        ptr: AstPtr::new(signature),
                        name_range: range,
                        extern_keyword: signature
                            .extern_keyword()
                            .map(|keyword| keyword.text_range()),
                    },
                ))
            }
        }
    }

    fn equation(&mut self, index: usize, equation: &ast::Equation) {
        let Some(name) = value_name(equation.name()) else {
            return;
        };
        let range = name.text_range();
        self.slot(name.text(), range)
            .equations
            .push((index, AstPtr::new(equation), range));
    }

    fn finish(self, file: FileId, diagnostics: &mut Vec<Diagnostic>) -> Vec<FunctionItem> {
        self.list
            .into_iter()
            .map(|definition| check_order(file, definition, diagnostics))
            .collect()
    }

    fn slot(&mut self, name: &str, range: TextRange) -> &mut Definition {
        let list = &mut self.list;
        let slot = *self.by_name.entry(name.to_string()).or_insert_with(|| {
            list.push(Definition {
                name: name.to_string(),
                first_range: range,
                public: false,
                signature: None,
                equations: Vec::new(),
            });
            list.len() - 1
        });
        &mut self.list[slot]
    }
}

/// トップレベルの宣言を集める。E1003 (シグネチャと型引数の重複)、E1004、E1018、E1019 と、`type` の E0004 を出す。
/// クラスのブロックのメンバーにも同じ規則を当て、instance のブロックのメンバーには E1003 と E1018 を出す。E1005 は、
/// 関数の種類 (extern かどうか) を決める `lower` が一緒に出す。ポインタを解決する木と取り違えないよう、構文木ではなく
/// `Parse` を受け取る。
pub fn item_tree(file: FileId, parse: &Parse) -> (ItemTree, Vec<Diagnostic>) {
    let mut diagnostics = Vec::new();
    let mut definitions = Definitions::default();
    let mut data = Vec::new();
    let mut effects = Vec::new();
    let mut fixities = Vec::new();
    let mut imports = Vec::new();
    let mut classes = Vec::new();
    let mut instances = Vec::new();
    for (index, item) in parse.tree().items().enumerate() {
        let public = item.pub_keyword().is_some();
        match item {
            ast::Item::Signature(signature) => {
                definitions.signature(index, &signature, public, file, &mut diagnostics)
            }
            ast::Item::Equation(equation) => definitions.equation(index, &equation),
            ast::Item::DataItem(item) => {
                // 名前がなければパーサが報告済み
                let Some(name) = item.name().map(|name| name.token()) else {
                    continue;
                };
                let deriving = item
                    .deriving()
                    .into_iter()
                    .flat_map(|deriving| deriving.classes())
                    .map(|class| (AstPtr::new(&class), class.range()))
                    .collect();
                let constructors = item
                    .alts()
                    .filter_map(|alt| {
                        // 名前も演算子もなければパーサが報告済み
                        let (name, infix) = match alt.name() {
                            Some(name) => (name, false),
                            None => (alt.operator()?, true),
                        };
                        let name = name.token();
                        Some(ConstructorItem {
                            name: name.text().to_string(),
                            name_range: name.text_range(),
                            ptr: AstPtr::new(&alt),
                            infix,
                        })
                    })
                    .collect();
                data.push(DataItem {
                    name: name.text().to_string(),
                    name_range: name.text_range(),
                    public,
                    extern_keyword: item.extern_keyword().map(|keyword| keyword.text_range()),
                    params: params(file, item.params(), &mut diagnostics),
                    has_constructors: item.has_constructors(),
                    constructors,
                    deriving,
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
                            ptr: AstPtr::new(&decl),
                        })
                    })
                    .collect();
                effects.push(EffectItem {
                    name: name.text().to_string(),
                    name_range: name.text_range(),
                    public,
                    extern_keyword: item.extern_keyword().map(|keyword| keyword.text_range()),
                    params: params(file, item.params(), &mut diagnostics),
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
            ast::Item::ClassItem(item) => {
                classes.extend(class_of(file, &item, public, &mut diagnostics))
            }
            ast::Item::InstanceItem(item) => {
                instances.push(instance_of(file, &item, &mut diagnostics))
            }
        }
    }
    let functions = definitions.finish(file, &mut diagnostics);
    (
        ItemTree {
            file,
            imports,
            functions,
            data,
            effects,
            fixities,
            classes,
            instances,
        },
        diagnostics,
    )
}

/// クラスの名前か型変数がなければ、パーサが報告済みなので集めない。`DefMap` と `lower` が同じ並びから番号を振るので、
/// 置けないクラスをここで落とす。
fn class_of(
    file: FileId,
    item: &ast::ClassItem,
    public: bool,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<ClassItem> {
    let name = item
        .name()
        .map(|name| name.token())
        .filter(|token| token.kind() == SyntaxKind::UIDENT)?;
    let var = item
        .var()
        .map(|var| var.token())
        .filter(|token| token.kind() == SyntaxKind::LIDENT)?;
    let mut definitions = Definitions::default();
    for (index, member) in item.members().enumerate() {
        match member {
            ast::ClassMember::Signature(signature) => {
                definitions.signature(index, &signature, false, file, diagnostics)
            }
            ast::ClassMember::Equation(equation) => definitions.equation(index, &equation),
        }
    }
    let methods = definitions
        .finish(file, diagnostics)
        .into_iter()
        .filter(|method| method.signature.is_some())
        .collect();
    Some(ClassItem {
        name: name.text().to_string(),
        name_range: name.text_range(),
        public,
        var: (var.text().to_string(), var.text_range()),
        ptr: AstPtr::new(item),
        methods,
    })
}

/// instance のメンバーを名前でまとめる。等式は連続していなければ E1018 にする。同じメソッドを `extern` の行と
/// 等式の両方で、または `extern` の行2つで定義したら、後の方を E1003 にして捨てる
/// (docs/superpowers/specs/2026-10-10-s5-type-classes-design.md の「文法」)。シグネチャはパーサが E0011 にした。
fn instance_of(
    file: FileId,
    item: &ast::InstanceItem,
    diagnostics: &mut Vec<Diagnostic>,
) -> InstanceItem {
    let mut members: Vec<MemberItem> = Vec::new();
    // メンバーごとの (最後の等式の並びの中の番号, 最後に捨てた等式の番号)
    let mut states: Vec<(Option<usize>, Option<usize>)> = Vec::new();
    for (index, member) in item.members().enumerate() {
        let (name, extern_keyword) = match &member {
            ast::InstanceMember::Equation(equation) => (value_name(equation.name()), None),
            ast::InstanceMember::ExternMethod(line) => {
                (value_name(line.name()), Some(line.keyword_range()))
            }
            ast::InstanceMember::Signature(_) => continue,
        };
        let Some(name) = name else {
            continue;
        };
        let (name, range) = (name.text(), name.text_range());
        let slot = match members.iter().position(|member| member.name == name) {
            Some(slot) => slot,
            None => {
                members.push(MemberItem {
                    name: name.to_string(),
                    name_range: range,
                    equations: Vec::new(),
                    extern_range: None,
                });
                states.push((None, None));
                members.len() - 1
            }
        };
        let member_item = &mut members[slot];
        let (last_equation, last_dropped) = &mut states[slot];
        let conflicts = match extern_keyword {
            Some(_) => member_item.extern_range.is_some() || !member_item.equations.is_empty(),
            None => member_item.extern_range.is_some(),
        };
        if conflicts {
            // 続けて書いた等式は1つの定義なので、その先頭だけを報告する
            let continues = extern_keyword.is_none() && *last_dropped == index.checked_sub(1);
            if !continues {
                diagnostics.push(duplicate(file, name, member_item.name_range, range));
            }
            if extern_keyword.is_none() {
                *last_dropped = Some(index);
            }
            continue;
        }
        match (member, extern_keyword) {
            (_, Some(keyword)) => member_item.extern_range = Some(keyword),
            (ast::InstanceMember::Equation(equation), None) => {
                if let (Some(previous), Some((_, previous_range))) =
                    (*last_equation, member_item.equations.last())
                    && previous + 1 != index
                {
                    diagnostics.push(not_consecutive(file, name, *previous_range, range));
                }
                *last_equation = Some(index);
                member_item.equations.push((AstPtr::new(&equation), range));
            }
            _ => {}
        }
    }
    InstanceItem {
        ptr: AstPtr::new(item),
        keyword_range: item.keyword_range(),
        members,
    }
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
    // extern のシグネチャに続く等式は、lower が診断なしで読み捨てる。E1033 のほかに置き場所の誤りを重ねない
    // (docs/spec/declarations.md の「`extern`」)
    let external = signature
        .as_ref()
        .is_some_and(|(_, signature)| signature.extern_keyword.is_some());
    for pair in equations.windows(2).filter(|_| !external) {
        let (previous_index, _, previous_range) = &pair[0];
        let (index, _, range) = &pair[1];
        if *index != previous_index + 1 {
            diagnostics.push(not_consecutive(file, &name, *previous_range, *range));
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
        (Some((signature_index, signature)), Some((equation_index, _, range)))
            if !external && *equation_index != signature_index + 1 =>
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
                .with_secondary(Label::new(
                    file,
                    signature.name_range,
                    "the signature is here",
                ))
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
        signature: signature.map(|(_, signature)| signature),
        equations: equations
            .into_iter()
            .map(|(_, ptr, range)| (ptr, range))
            .collect(),
    }
}

/// E1018。instance の等式にはシグネチャがないが、文言はトップレベルの関数と同じにする。
fn not_consecutive(file: FileId, name: &str, previous: TextRange, again: TextRange) -> Diagnostic {
    Diagnostic::error(
        codes::NON_CONSECUTIVE_EQUATIONS,
        format!("the equations of `{name}` are not consecutive"),
        Label::new(
            file,
            again,
            "this equation is separated from the ones above",
        ),
    )
    .with_secondary(Label::new(file, previous, "the previous equation"))
    .with_help(format!(
        "put every equation of `{name}` together, right after its signature"
    ))
}

/// `data` と `effect` の型引数。重複した名前は E1003 にして、最初の1つだけを残す。
fn params(
    file: FileId,
    names: impl Iterator<Item = ast::Name>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<(String, TextRange)> {
    let mut params: Vec<(String, TextRange)> = Vec::new();
    for name in names {
        let token = name.token();
        let (text, range) = (token.text(), token.text_range());
        match params.iter().find(|(param, _)| param == text) {
            Some((_, first)) => diagnostics.push(duplicate(file, text, *first, range)),
            None => params.push((text.to_string(), range)),
        }
    }
    params
}

/// 同じ名前空間の定義の重複 (docs/spec/modules.md の「名前空間」)。トップレベルの定義と、宣言の中の型引数に使う。
/// ソースで後に書いた方を primary にする。
pub(crate) fn duplicate(
    file: FileId,
    name: &str,
    first: TextRange,
    again: TextRange,
) -> Diagnostic {
    Diagnostic::error(
        codes::DUPLICATE_DEFINITION,
        format!("`{name}` is defined more than once"),
        Label::new(file, again, "defined again here"),
    )
    .with_secondary(Label::new(file, first, "first defined here"))
}

/// パスが読めなかった import はパーサが報告済みなので、読み込みもスコープへの登録もしない。
fn import_of(item: &ast::ImportItem) -> Option<ImportItem> {
    let path = item.path()?;
    let path_range = path.range();
    let module = ModulePath(path.segments().map(|segment| segment.text()).collect());
    let qualifier = match item.alias() {
        Some(alias) => alias.token().text().to_string(),
        None => module.last().to_string(),
    };
    let list = item
        .list()
        .map(|list| list.names().filter_map(|name| import_name(&name)).collect());
    Some(ImportItem {
        path: module,
        path_range,
        qualifier,
        list,
        range: item.range(),
        malformed: item.is_malformed(),
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
