//! REPL の入力の関数の検査 (docs/spec/types.md の「REPL の入力」)。入力は
//! `Unit -> <IO> T` の関数で、`T` をシグネチャなしで、局所の `let` と同じく単相に推論する。入力の関数はほかの関数から
//! 参照されないので、1つだけの SCC として、ほかの関数と同じ道で Kind を解いて書き出す。

use std::collections::{HashMap, HashSet};

use eml_diagnostics::{Diagnostic, Label};
use eml_hir::{ClassId, FunctionId, Generics, InputKind, Program, ValueItem};

use crate::context::Context;
use crate::kind::problem::KindScheme;
use crate::kind::{Provenance, Span};
use crate::shape::{Own, Rigids, closed_shape};
use crate::store::{EffectLabel, Lin, TypeId, TypeKind, TypeStore};
use crate::table::{ArrowLin, Label as RowLabel, Row, Table, Ty};
use crate::ty::Linearity;
use crate::{BodyTypes, InputDisplay, InputType, Resolution, carry, codes, resolve, usage};

use super::body::{BodyCheck, BodyTyping};
use super::report::AmbientSource;
use super::{PendingBody, Signatures};

/// 入力の関数の本体を検査する。`check_body` と同じく、Kind の制約を持った本体を返す。
pub(super) fn check_input<'c>(
    program: &Program,
    context: &'c Context,
    signatures: &Signatures,
    id: FunctionId,
    kind: InputKind,
    types: &mut TypeStore,
) -> (PendingBody<'c>, Vec<Diagnostic>) {
    let function = &program[id];
    let body = program.body(id).expect("an input has a body");
    let mut table = Table::new(context);
    let result = table.fresh_var();
    let io = Row::closed(vec![RowLabel {
        effect: program.io(),
        args: Vec::new(),
    }]);
    let unit = table.unit;
    let own = table.function_with(unit, ArrowLin::Known(Linearity::Unr), io, result);
    let rigids = Rigids::new(&mut table, &Generics::default());
    let file = program.file(id.module);
    table.set_kind_origin(Provenance::Unattributed(Span {
        file,
        range: function.name_range,
    }));
    let mut diagnostics = Vec::new();
    let mut checker = BodyCheck {
        program,
        file,
        function,
        body,
        rigids: &rigids,
        signatures,
        table: &mut table,
        types: &mut *types,
        diagnostics: &mut diagnostics,
        ambient: Row::pure(),
        ambient_source: AmbientSource::Input,
        typing: BodyTyping::default(),
        instances: Vec::new(),
    };
    checker.check_function(own);
    if let InputKind::Let(binding) = kind {
        checker.undecided_binding(binding, result);
    }
    // 式の入力の決まらない型は、制約を解くときに `Unit` とみなす。`let` は決まらない型を誤りにしてあるので、決めない
    checker.solve_constraints(kind == InputKind::Expr);
    let typing = checker.typing;
    let instances = checker.instances;
    default_unsolved(&mut table, &typing, result, kind == InputKind::Expr);
    let reliable = usage::reliable(body, diagnostics.is_empty());
    usage::constrain(program, file, body, &typing, &mut table, types, reliable);
    carry::constrain(program, file, body, &typing, &mut table, reliable);
    (
        PendingBody {
            table,
            typing,
            own: Own {
                ty: own,
                rigids,
                lin: Vec::new(),
                mult: Vec::new(),
            },
            instances,
            reliable,
        },
        diagnostics,
    )
}

/// 決まらずに残った変数を決める。row 変数はどの入力でも `<>` にする。型変数は、式の入力なら `Unit` にする。
/// 制約はもう `Unit` とみなして解いてあるので、書き出す型をそれに合わせる。
fn default_unsolved(table: &mut Table<'_>, typing: &BodyTyping, result: Ty, types: bool) {
    let roots = typing
        .exprs
        .values()
        .chain(typing.locals.values())
        .chain(typing.pats.values())
        .chain(typing.instantiations.values().flat_map(|(_, args, _)| args))
        .copied()
        .chain([result]);
    let (vars, rows) = table.unsolved(roots);
    if types {
        let unit = table.unit;
        for var in vars {
            table
                .unify(var, unit)
                .expect("an unsolved type variable unifies with `Unit`");
        }
    }
    for row in rows {
        table
            .close_row(row)
            .expect("an unsolved row variable closes to `<>`");
    }
}

/// 書き出した入力の関数の本体から、入力の結果をまとめる。入力の関数の型と、`let` の束縛の型を、閉じた形として
/// `signatures` に入れる。後の入力は、それを宣言の型として参照する。
#[expect(clippy::too_many_arguments, reason = "the tables an input adds to")]
pub(super) fn finish_input(
    program: &Program,
    id: FunctionId,
    kind: InputKind,
    body_types: &BodyTypes,
    types: &mut TypeStore,
    signatures: &mut Signatures,
    schemes: &mut HashMap<ValueItem, KindScheme>,
    diagnostics: &mut Vec<Diagnostic>,
) -> InputType {
    let body = program.body(id).expect("an input has a body");
    let ty = body_types.exprs[body.root];
    if !types.contains_error(ty) && types.linearity(ty) == Lin::Lin {
        diagnostics.push(linear_input(program, id, kind, ty, types));
    }
    let function_ty = types.intern(TypeKind::Fn {
        param: types.unit(),
        lin: Lin::Unr,
        effects: vec![EffectLabel {
            id: program.io(),
            args: Vec::new(),
        }],
        tail: None,
        ret: ty,
    });
    signatures
        .functions
        .insert(id, closed_shape(types, function_ty));
    let display = match kind {
        InputKind::Let(binding) => {
            signatures.bindings.insert(binding, closed_shape(types, ty));
            schemes.insert(ValueItem::Binding(binding), KindScheme::default());
            InputDisplay::Unit
        }
        InputKind::Expr if ty == types.unit() => InputDisplay::Unit,
        InputKind::Expr if satisfies(program, types, program.lang.show, ty) => InputDisplay::Show,
        InputKind::Expr => InputDisplay::TypeOnly(types.display(ty, &program.names).to_string()),
    };
    InputType { ty, display }
}

/// 入力の結果が `Lin` の値である。入力をまたいで残せず、表示もできない。
fn linear_input(
    program: &Program,
    id: FunctionId,
    kind: InputKind,
    ty: TypeId,
    types: &TypeStore,
) -> Diagnostic {
    let shown = types.display(ty, &program.names).to_string();
    let file = program.file(id.module);
    match kind {
        InputKind::Let(binding) => {
            let name = &program[binding].name;
            Diagnostic::error(
                codes::INPUT_LINEAR,
                format!("`{name}` cannot keep a value of the linear type `{shown}`"),
                Label::new(file, program[binding].name_range, "bound here"),
            )
            .with_note("only values that may be copied and dropped stay across inputs")
        }
        InputKind::Expr => {
            let body = program.body(id).expect("an input has a body");
            Diagnostic::error(
                codes::INPUT_LINEAR,
                format!("this input leaves a value of the linear type `{shown}`"),
                Label::new(file, body.exprs[body.root].range, "a linear value"),
            )
            .with_help("use up the value inside the input, as in `drop`")
        }
    }
}

/// 閉じた型 `ty` で `class` の制約が解けるか。型検査の `solve` と同じく、instance の文脈は頭の型引数へ写し、タプルは
/// 要素へ進む。型の表は hash-consing なので、同じクラスと同じ型の組は1回だけ調べる。
fn satisfies(program: &Program, types: &TypeStore, class: ClassId, ty: TypeId) -> bool {
    let mut work = vec![(class, ty)];
    let mut seen = HashSet::new();
    while let Some((class, ty)) = work.pop() {
        if !seen.insert((class, ty)) {
            continue;
        }
        match resolve(program, types, class, ty) {
            Resolution::Instance { instance, args } => {
                for constraint in &program[instance].context {
                    let index = u32::from(constraint.var.into_raw()) as usize;
                    work.push((constraint.class, args[index]));
                }
            }
            Resolution::Tuple(elements) => {
                work.extend(elements.into_iter().map(|element| (class, element)));
            }
            Resolution::Given | Resolution::Missing => return false,
        }
    }
    true
}

impl BodyCheck<'_, '_> {
    /// `let` の束縛の型に決まらない型変数が残れば、誤りにする。束縛の型は後の入力の宣言の型になるので、推論で決めずに
    /// 残すと、型の誤りが何入力も後に、原因から離れて出るためである。残った変数は `Error` にして、制約の誤りを
    /// 重ねない。本体にほかの誤りがあれば、その連鎖なので報告しない。
    fn undecided_binding(&mut self, binding: eml_hir::BindingId, ty: Ty) {
        let (vars, _) = self.table.unsolved([ty]);
        if vars.is_empty() {
            return;
        }
        let error = self.table.error;
        for var in vars {
            self.table
                .unify(var, error)
                .expect("an unsolved type variable unifies with `Error`");
        }
        if self.diagnostics.iter().any(Diagnostic::is_error) {
            return;
        }
        let program = self.program;
        let name = &program[binding].name;
        self.diagnostics.push(
            Diagnostic::error(
                codes::BINDING_AMBIGUOUS,
                format!("the type of `{name}` is not decided"),
                Label::new(
                    self.file(),
                    program[binding].name_range,
                    "its type is never decided",
                ),
            )
            .with_help(format!(
                "write the type of `{name}`, as in `let {name} : List Int = …`"
            )),
        );
    }
}

#[cfg(test)]
mod tests {
    use eml_extern::ExternType;
    use eml_hir::{ItemId, Program, ValueItem};

    use super::satisfies;
    use crate::shape::closed_shape;
    use crate::store::{EffectLabel, Lin, TypeKind, TypeStore};
    use crate::{InputDisplay, InputType, TypedProgram};

    #[test]
    fn show_is_satisfied_only_when_every_part_has_an_instance() {
        let program = crate::test_program("data Plain = | Plain");
        let mut types = TypeStore::new(&program);
        let int = types.intern(TypeKind::Con {
            id: program.extern_type(ExternType::Int),
            args: Vec::new(),
        });
        let list = |types: &mut TypeStore, element| {
            types.intern(TypeKind::Con {
                id: program.lang.list,
                args: vec![element],
            })
        };
        let function = types.intern(TypeKind::Fn {
            param: int,
            lin: Lin::Unr,
            effects: Vec::new(),
            tail: None,
            ret: int,
        });
        let plain = program
            .types()
            .find(|(_, ty)| ty.name == "Plain")
            .map(|(id, _)| id)
            .unwrap();
        let plain = types.intern(TypeKind::Con {
            id: plain,
            args: Vec::new(),
        });
        let show = program.lang.show;
        let ints = list(&mut types, int);
        let functions = list(&mut types, function);
        let pair = types.intern(TypeKind::Tuple(vec![int, ints]));
        assert!(satisfies(&program, &types, show, ints));
        assert!(satisfies(&program, &types, show, pair));
        assert!(satisfies(&program, &types, show, types.unit()));
        assert!(!satisfies(&program, &types, show, functions));
        assert!(!satisfies(&program, &types, show, plain));
    }

    #[test]
    fn a_closed_shape_exports_back_to_the_same_type() {
        let program = crate::test_program("");
        let mut types = TypeStore::new(&program);
        let string = types.intern(TypeKind::Con {
            id: program.extern_type(ExternType::String),
            args: Vec::new(),
        });
        let function = types.intern(TypeKind::Fn {
            param: types.unit(),
            lin: Lin::Lin,
            effects: vec![EffectLabel {
                id: program.io(),
                args: Vec::new(),
            }],
            tail: None,
            ret: string,
        });
        let pair = types.intern(TypeKind::Tuple(vec![function, string]));
        let shape = closed_shape(&types, pair);
        assert!(shape.rigids.is_empty() && shape.lin_vars == 0);
        assert_eq!(shape.export(&mut types, None), pair);
    }

    /// セッションを検査し、診断を `E2015 メッセージ` の形で並べる。
    fn session(inputs: &[&str]) -> (Program, TypedProgram, Vec<String>) {
        let (program, files) = crate::test_session(inputs);
        let (typed, diagnostics) = crate::check(&program, &files);
        let shown = diagnostics
            .iter()
            .map(|d| format!("{} {}", d.code, d.message))
            .collect();
        (program, typed, shown)
    }

    /// 最後の入力の結果。
    fn last_input<'a>(program: &Program, typed: &'a TypedProgram) -> &'a InputType {
        &typed.inputs[program
            .input()
            .expect("the last input is an input function")]
    }

    fn shown_input(program: &Program, typed: &TypedProgram) -> (String, InputDisplay) {
        let input = last_input(program, typed);
        let ty = typed.types.display(input.ty, &program.names).to_string();
        (ty, input.display.clone())
    }

    fn binding_type(program: &Program, typed: &TypedProgram, name: &str) -> String {
        let id = program
            .modules
            .iter()
            .flat_map(|(module, m)| {
                m.items
                    .bindings
                    .iter()
                    .map(move |(local, binding)| (ItemId::new(module, local), binding))
            })
            .find(|(_, binding)| binding.name == name)
            .map(|(id, _)| id)
            .expect("the binding exists");
        let ty = typed.decls[&ValueItem::Binding(id)].ty;
        typed.types.display(ty, &program.names).to_string()
    }

    #[test]
    fn a_let_binding_gets_its_inferred_type() {
        let (program, typed, diagnostics) = session(&["let x = 41 + 1", "x * 2"]);
        assert_eq!(diagnostics, Vec::<String>::new());
        assert_eq!(binding_type(&program, &typed, "x"), "Int");
        assert_eq!(
            shown_input(&program, &typed),
            ("Int".to_string(), InputDisplay::Show)
        );
    }

    #[test]
    fn an_expression_of_type_unit_shows_nothing() {
        let (program, typed, diagnostics) = session(&["println \"hi\""]);
        assert_eq!(diagnostics, Vec::<String>::new());
        assert_eq!(
            shown_input(&program, &typed),
            ("Unit".to_string(), InputDisplay::Unit)
        );
    }

    #[test]
    fn leftover_type_variables_of_an_expression_become_unit() {
        // 決まらない要素の型を `Unit` に決めるので、`Show (List Unit)` で表示できる
        let (program, typed, diagnostics) = session(&["[]"]);
        assert_eq!(diagnostics, Vec::<String>::new());
        assert_eq!(
            shown_input(&program, &typed),
            ("List Unit".to_string(), InputDisplay::Show)
        );
    }

    #[test]
    fn a_function_value_is_shown_by_its_type() {
        let (program, typed, diagnostics) = session(&["fn x -> x + 1", "[fn x -> x + 1]"]);
        assert_eq!(diagnostics, Vec::<String>::new());
        // `Show (List a)` があっても、要素の関数型が `Show` を持たないので型だけを表示する
        assert_eq!(
            shown_input(&program, &typed).1,
            InputDisplay::TypeOnly("List (Int -> Int)".to_string())
        );
    }

    #[test]
    fn an_undecided_binding_is_an_error_and_an_annotated_one_is_not() {
        let (_, _, diagnostics) = session(&["let id = fn x -> x"]);
        assert_eq!(diagnostics, ["E2017 the type of `id` is not decided"]);
        let (program, typed, diagnostics) = session(&["let xs : List Int = []"]);
        assert_eq!(diagnostics, Vec::<String>::new());
        assert_eq!(binding_type(&program, &typed, "xs"), "List Int");
    }

    #[test]
    fn an_input_may_perform_only_io() {
        let (_, _, diagnostics) = session(&["effect Ask where\n  ask : Unit -> Int", "ask ()"]);
        assert_eq!(
            diagnostics,
            ["E2015 `ask` performs `Ask`, but an input may perform only `IO`"]
        );
    }

    #[test]
    fn an_input_cannot_leave_or_keep_a_linear_value() {
        let (_, _, diagnostics) = session(&["Fs.open \"x\""]);
        assert_eq!(
            diagnostics,
            ["E2016 this input leaves a value of the linear type `File`"]
        );
        let (_, _, diagnostics) = session(&["let f = Fs.open \"x\""]);
        assert_eq!(
            diagnostics,
            ["E2016 `f` cannot keep a value of the linear type `File`"]
        );
    }

    #[test]
    fn defaulting_reports_classes_without_a_unit_instance_as_ambiguous() {
        // 決めた `Unit` で解けない制約は、利用者が書いていない型なので E2006 でなく E2009 にする
        let (_, _, diagnostics) = session(&[
            "class Named a where\n  name : a -> String",
            "class Make a where\n  make : Unit -> a",
            "name (make ())",
        ]);
        assert_eq!(
            diagnostics,
            [
                "E2009 cannot decide which instance of `Named` to use for `name`",
                "E2009 cannot decide which instance of `Make` to use for `make`",
            ]
        );
    }

    #[test]
    fn a_binding_of_function_type_has_unrestricted_arrows() {
        // 残った矢印の Kind 変数は `Unr` に決まる。束縛の型は後の入力の宣言の型なので、Kind 変数を残さない
        let (program, typed, diagnostics) = session(&["let inc = fn x -> x + 1", "inc 3"]);
        assert_eq!(diagnostics, Vec::<String>::new());
        assert_eq!(binding_type(&program, &typed, "inc"), "Int -> Int");
        let input = last_input(&program, &typed);
        assert_eq!(input.display, InputDisplay::Show);
    }

    #[test]
    fn a_function_definition_reads_a_binding() {
        let (program, typed, diagnostics) =
            session(&["let base = 10", "f : Int -> Int\nf n = n + base", "f 1"]);
        assert_eq!(diagnostics, Vec::<String>::new());
        let f = program
            .functions()
            .find(|(_, function)| function.name == "f")
            .map(|(id, _)| id)
            .unwrap();
        let reads_binding = typed.bodies[f]
            .instantiations
            .values()
            .any(|instantiation| matches!(instantiation.decl, ValueItem::Binding(_)));
        assert!(reads_binding);
    }

    #[test]
    fn a_let_reads_the_previous_version_of_its_name() {
        // 束縛は自分の入力の中では見えないので、右辺の `x` は前の入力の `x` を指す
        let (program, typed, diagnostics) = session(&["let x = 1", "let x = x + 1", "x"]);
        assert_eq!(diagnostics, Vec::<String>::new());
        assert_eq!(
            shown_input(&program, &typed),
            ("Int".to_string(), InputDisplay::Show)
        );
    }

    #[test]
    fn a_main_in_a_session_is_not_checked() {
        let (_, _, diagnostics) = session(&["main : Int\nmain = 1"]);
        assert_eq!(diagnostics, Vec::<String>::new());
    }
}
