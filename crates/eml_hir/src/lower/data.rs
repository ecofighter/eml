//! `data` の宣言の変換 (docs/spec/declarations.md の「`data` と `type`」)。名前の表と重複の判定は `DefMap` が持つ。

use eml_diagnostics::{Diagnostic, FileId, Label, TextRange};
use eml_extern::ExternType;
use la_arena::Arena;

use super::ItemLowering;
use super::types::{TypeLowering, Vars};
use crate::codes;
use crate::def_map::Resolved;
use crate::hir::{
    Constructor, FieldName, Generics, ItemId, TypeDef, TypeDefKind, TypeVarDecl, ValueItem,
};
use crate::item_tree::{DataItem, Fixity};

impl ItemLowering<'_> {
    /// 型の名前と型引数を置く。フィールドの型は `lower_constructors` が、すべての型を置いた後に変換する。
    pub(super) fn declare_data(&mut self, items: &[DataItem], types: &mut Arena<TypeDef>) {
        for (k, item) in items.iter().enumerate() {
            let mut generics = Generics::default();
            // extern の型に書いた型引数はパーサが E0011 にした (docs/spec/declarations.md の「`extern`」)。それでも型引数として置き、使う位置の型引数の数 (E1015) と
            // 型検査の Kind を書いたとおりにそろえて、誤りを連鎖させない
            for (name, _) in &item.params {
                generics.type_vars.alloc(TypeVarDecl { name: name.clone() });
            }
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
            let id = ItemId::new(
                self.module,
                types.alloc(TypeDef {
                    name: item.name.clone(),
                    generics,
                    types: Arena::new(),
                    kind,
                }),
            );
            debug_assert_eq!(id, self.def_map.type_id(self.module, k));
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
            let ty = self.def_map.type_id(self.module, k);
            for (j, constructor) in item.constructors.iter().enumerate() {
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
                let mut lowering = TypeLowering {
                    file: self.file,
                    types: &mut def.types,
                    generics: &mut def.generics,
                    items: self.resolver,
                    vars: Vars::Data,
                    public_item: item.public.then_some(constructor.name.as_str()),
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
                let id = self.def_map.constructor_id(self.module, k, j);
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
/// (docs/superpowers/specs/2026-10-10-s6c-records-design.md の「診断」)。
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
