//! `data` の宣言の変換 (docs/spec/declarations.md の「`data` と `type`」)。名前の表と重複の判定は `DefMap` が持つ。

use eml_diagnostics::{Diagnostic, FileId, Label, TextRange};
use eml_extern::ExternType;
use la_arena::Arena;

use super::ItemLowering;
use super::types::{TypeLowering, Vars};
use crate::codes;
use crate::def_map::{Resolved, item_id};
use crate::hir::{
    Constructor, FieldName, Generics, ItemId, ParamSort, RowVarDecl, TypeDef, TypeDefKind,
    TypeVarDecl, ValueItem,
};
use crate::item_tree::{DataItem, Fixity};

impl ItemLowering<'_> {
    /// 型の名前と型引数を置く。フィールドの型は `lower_constructors` が、すべての型を置いた後に変換する。
    pub(super) fn declare_data(&mut self, items: &[DataItem], types: &mut Arena<TypeDef>) {
        for (k, item) in items.iter().enumerate() {
            let mut generics = Generics::default();
            // extern の型に書いた型引数はパーサが E0011 にした (docs/spec/declarations.md の「`extern`」)。それでも型引数として置き、使う位置の型引数の数 (E1015) と
            // 型検査の Kind を書いたとおりにそろえて、誤りを連鎖させない
            for (name, _, sort) in &item.params {
                let name = name.clone();
                match sort {
                    ParamSort::Type => {
                        generics.type_vars.alloc(TypeVarDecl { name });
                    }
                    ParamSort::Row => {
                        generics.row_vars.alloc(RowVarDecl { name, never: None });
                    }
                }
            }
            let params = item.params.iter().map(|&(_, _, sort)| sort).collect();
            // extern でない `=` のない `data` は、値を作れないので E1025 にする。標準ライブラリでも同じである
            // (docs/spec/declarations.md の「`data` と `type`」)
            let data = TypeDefKind::Data {
                constructors: Vec::new(),
            };
            let kind = match (item.extern_keyword, item.has_constructors) {
                (Some(keyword), _) => {
                    TypeDefKind::Extern(self.extern_row(keyword, &item.name, ExternType::from_name))
                }
                (None, true) => data,
                (None, false) => {
                    self.diagnostics.push(Diagnostic::error(
                        codes::MISSING_CONSTRUCTORS,
                        format!("`{}` has no constructors", item.name),
                        Label::new(
                            self.file,
                            item.name_range,
                            "add constructors after `=`, as in `= | A | B`",
                        ),
                    ));
                    data
                }
            };
            let lin = self.declared_lin(item);
            let id = ItemId::new(
                self.module,
                types.alloc(TypeDef {
                    name: item.name.clone(),
                    generics,
                    params,
                    types: Arena::new(),
                    kind,
                    lin,
                    abstract_ctors: item.public
                        && item.abstract_ctors
                        && item.extern_keyword.is_none(),
                }),
            );
            debug_assert_eq!(id, item_id(self.module, k));
        }
    }

    /// `data` に書いた Kind から `lin` を決める。宣言できるのは `Lin` だけで、`Unr` は既定と同じなので未対応にする
    /// (docs/spec/types.md の「`Lin` の宣言」)。
    fn declared_lin(&mut self, item: &DataItem) -> bool {
        let Some((name, range)) = &item.kind else {
            return false;
        };
        match name.as_str() {
            "Lin" => true,
            "Unr" => {
                self.diagnostics.push(Diagnostic::not_yet_supported(
                    self.file,
                    *range,
                    "declaring a type `Unr` is not supported yet",
                ));
                false
            }
            _ => {
                self.diagnostics.push(
                    Diagnostic::error(
                        codes::UNKNOWN_KIND,
                        format!("`{name}` is not a kind"),
                        Label::new(self.file, *range, "a kind is expected here"),
                    )
                    .with_note("only `Lin` can be declared for a `data` type"),
                );
                false
            }
        }
    }

    /// フィールドの型を変換してコンストラクタを置く。タグは宣言の中の順の番号である。重複した `data` のコンストラクタも
    /// 置く。使えない印は `DefMap` が持つので、使った位置が診断なしで `Missing` になる
    /// (docs/spec/diagnostics.md の「連鎖する診断の抑止」)。
    pub(super) fn lower_constructors(
        &mut self,
        items: &[DataItem],
        types: &mut Arena<TypeDef>,
        constructors: &mut Arena<Constructor>,
    ) {
        for (k, item) in items.iter().enumerate() {
            let ty = item_id(self.module, k);
            for constructor in &item.constructors {
                // `::` は Prelude のリストだけの名前である。2つのモジュールが定義すると、修飾した演算子の構文が
                // ないので、診断にも書けない形になる (docs/spec/declarations.md の
                // 「`data` と `type`」)。宣言はそのまま置き、そのモジュールの使用には誤りを重ねない
                if constructor.name == "::" && self.module != self.def_map.prelude() {
                    self.diagnostics.push(Diagnostic::error(
                        codes::RESERVED_CONSTRUCTOR,
                        "the constructor `::` is reserved for lists",
                        Label::new(
                            self.file,
                            constructor.name_range,
                            "only the Prelude defines `::`",
                        ),
                    ));
                }
                let def = &mut types[ty.local];
                // 抽象型のフィールドは外から見えないので、非公開の型が現れても裏口にならない
                // (docs/spec/modules.md の「公開の範囲」)
                let exposed = item.public && !def.abstract_ctors;
                let mut lowering = TypeLowering {
                    file: self.file,
                    types: &mut def.types,
                    generics: &mut def.generics,
                    items: self.resolver,
                    vars: Vars::Data,
                    public_item: exposed.then_some(constructor.name.as_str()),
                    diagnostics: &mut *self.diagnostics,
                };
                let alt = constructor.ptr.to_node(self.root);
                let (fields, field_names) = match alt.record_fields() {
                    // 型のないフィールドの宣言 (構文の誤り) も `Error` の型で置き、名前と型の番号をそろえる
                    Some(record) => {
                        let mut fields = Vec::new();
                        let mut names: Vec<FieldName> = Vec::new();
                        for decl in record.fields() {
                            let range = decl.range();
                            fields.push(lowering.lower(decl.ty(), range));
                            let name = decl
                                .name()
                                .expect("the parser starts every field declaration at its name")
                                .token();
                            let field = FieldName {
                                name: name.text().to_string(),
                                range: name.text_range(),
                            };
                            if let Some(first) = names.iter().find(|first| first.name == field.name)
                            {
                                lowering.diagnostics.push(duplicate_field(
                                    self.file,
                                    &field.name,
                                    first.range,
                                    field.range,
                                ));
                            }
                            names.push(field);
                        }
                        (fields, Some(names))
                    }
                    None => {
                        let fields = alt
                            .fields()
                            .map(|field| {
                                let range = field.range();
                                lowering.lower(Some(field), range)
                            })
                            .collect();
                        (fields, None)
                    }
                };
                let TypeDefKind::Data {
                    constructors: declared,
                } = &mut def.kind
                else {
                    unreachable!("`declare_data` makes data types")
                };
                let id = item_id(self.module, constructors.len());
                let fixity = constructor.infix.then(|| {
                    self.resolver
                        .fixity_of(&Resolved::Found(ValueItem::Constructor(id)))
                        .unwrap_or(Fixity::DEFAULT)
                });
                let allocated = ItemId::new(
                    self.module,
                    constructors.alloc(Constructor {
                        name: constructor.name.clone(),
                        ty,
                        tag: declared.len() as u32,
                        fields,
                        fixity,
                        field_names,
                    }),
                );
                debug_assert_eq!(allocated, id);
                declared.push(id);
            }
        }
    }
}

/// 同じフィールドの名前を2回書いた (E1045)。宣言、作る式、パターンで同じ文言にする
/// (docs/implementation/diagnostics.md の「番号ごとの出し方」)。
pub(super) fn duplicate_field(
    file: FileId,
    name: &str,
    first: TextRange,
    again: TextRange,
) -> Diagnostic {
    Diagnostic::error(
        codes::DUPLICATE_FIELD,
        format!("the field `{name}` appears more than once"),
        Label::new(file, again, "written again here"),
    )
    .with_secondary(Label::new(file, first, "first written here"))
}
