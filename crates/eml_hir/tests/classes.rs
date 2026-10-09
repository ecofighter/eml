//! クラス、メソッド、instance の HIR (docs/spec/declarations.md の「クラスと instance」)。

use eml_hir::{FunctionKind, InstanceOrigin, MethodImpl};
use eml_test_support::{lower, lower_files, short};

const COLOR: &str = "data Color =\n  | Red\n  | Green\n\n";

fn lines(text: &str) -> Vec<String> {
    let lowered = lower(text);
    short(lowered.files(), &lowered.diagnostics)
}

#[test]
fn a_class_its_methods_and_an_instance_are_lowered() {
    let text = format!(
        "{COLOR}class Same a where\n  same : a -> a -> Bool\n  differ : a -> a -> Bool\n  differ x y = not (same x y)\n\ninstance Same Color where\n  same Red Red = True\n  same Green Green = True\n  same _ _ = False\n\ncheck : Same a => a -> Bool\ncheck x = same x x"
    );
    let lowered = lower(&text);
    assert!(lowered.diagnostics.is_empty(), "{:?}", lowered.diagnostics);
    let program = &lowered.program;
    let (class, def) = program
        .classes()
        .find(|(_, def)| def.name == "Same")
        .expect("the class");
    assert_eq!((def.name.as_str(), def.var.as_str()), ("Same", "a"));
    let names: Vec<&str> = def
        .methods
        .iter()
        .map(|&m| program[m].name.as_str())
        .collect();
    assert_eq!(names, ["same", "differ"]);
    let differ = def.methods[1];
    let default = program[differ].default.expect("`differ` has a default");
    assert_eq!(program[default].kind, FunctionKind::DefaultMethod(differ));
    let (instance, inst) = program
        .instances()
        .find(|(_, inst)| inst.class == class)
        .expect("one instance");
    assert_eq!(inst.class, class);
    assert_eq!(program.instance(class, inst.head), Some(instance));
    let [(method, MethodImpl::Function(function))] = inst.methods.as_slice() else {
        panic!("one method function: {:?}", inst.methods);
    };
    assert_eq!(*method, def.methods[0]);
    assert_eq!(program[*function].name, "Same Color.same");
    assert_eq!(
        program[*function].kind,
        FunctionKind::InstanceMethod(instance, def.methods[0])
    );
    let check = program
        .functions()
        .find(|(_, f)| f.name == "check")
        .map(|(_, f)| f)
        .unwrap();
    let constraints = &check.signature.as_ref().unwrap().constraints;
    assert_eq!(constraints.len(), 1);
    assert_eq!(constraints[0].class, class);
}

#[test]
fn classes_share_the_type_namespace_and_methods_the_value_namespace() {
    assert_eq!(
        lines("data C = | C\n\nclass C a where\n  m : a -> Int"),
        ["E1003 3:7 `C` is defined more than once"]
    );
    assert_eq!(
        lines("m : Int\nm = 1\n\nclass K a where\n  m : a -> Int"),
        ["E1003 5:3 `m` is defined more than once"]
    );
}

#[test]
fn class_parts_are_imported_with_dot_dot() {
    let module = "pub class Size a where\n  size : a -> Int\n";
    let entry = "import M (Size(..))\n\nf : Size a => a -> Int\nf x = size x";
    let lowered = lower_files(entry, &[("M.em", module)]);
    assert!(lowered.diagnostics.is_empty(), "{:?}", lowered.diagnostics);
    let entry = "import M (Size)\n\nf : Size a => a -> Int\nf x = size x\n\ng : Size a => a -> Int\ng x = M.size x";
    let lowered = lower_files(entry, &[("M.em", module)]);
    assert_eq!(
        short(lowered.files(), &lowered.diagnostics),
        ["E1001 4:7 cannot find value `size`"]
    );
}

#[test]
fn an_operator_method_keeps_its_fixity_in_another_module() {
    // 結合しない演算子を2つ並べると E1006 になる。fixity がメソッドに付いて、別のモジュールでも効くことを確かめる
    let module = "pub infix 4 ===\n\npub class Same a where\n  (===) : a -> a -> Bool\n";
    let entry = "import M (Same(..))\n\nf : Same a => a -> Bool\nf x = x === x === x";
    let lowered = lower_files(entry, &[("M.em", module)]);
    let lines = short(lowered.files(), &lowered.diagnostics);
    assert!(
        lines.iter().any(|line| line.starts_with("E1006")),
        "{lines:?}"
    );
}

#[test]
fn instance_declarations_are_checked() {
    let class =
        "class Size a where\n  size : a -> Int\n  big : a -> Bool\n  big x = size x > 9\n\n";
    insta::assert_snapshot!(lines(&format!("{COLOR}{class}instance Size Color where\n  big _ = False\n  small _ = True")).join("\n"), @"
    E1036 10:15 the instance of `Size` for `Color` does not define `size`
    E1037 12:3 `small` is not a method of `Size`
    ");
    insta::assert_snapshot!(lines(&format!("{class}instance Size a where\n  size _ = 0\n\ninstance Size (Int, Int) where\n  size _ = 0\n\ninstance Size Unit where\n  size _ = 0\n\ninstance Size (Option Int) where\n  size _ = 0\n\ndata Pair a b = | Pair a b\n\ninstance Size (Pair a a) where\n  size _ = 0\n\ndata Option a = | None | Some a")).join("\n"), @"
    E1039 6:15 an instance head must be a type constructor applied to distinct type variables
    E1039 9:15 an instance head must be a type constructor applied to distinct type variables
    E1039 12:15 an instance head must be a type constructor applied to distinct type variables
    E1039 15:23 an instance head must be a type constructor applied to distinct type variables
    E1039 20:23 an instance head must be a type constructor applied to distinct type variables
    ");
    insta::assert_snapshot!(lines(&format!("{COLOR}{class}instance Size Color where\n  size _ = 1\n\ninstance Size Color where\n  size _ = 2")).join("\n"), @"E1035 13:15 `Color` already has an instance of `Size`");
}

#[test]
fn an_instance_must_be_in_the_module_of_its_class_or_type() {
    let class = "pub class Size a where\n  size : a -> Int\n";
    let ty = "pub data Color = | Red\n";
    let entry =
        "import C (Size(..))\nimport T (Color(..))\n\ninstance Size Color where\n  size _ = 1";
    let lowered = lower_files(entry, &[("C.em", class), ("T.em", ty)]);
    insta::assert_snapshot!(short(lowered.files(), &lowered.diagnostics).join("\n"), @"E1034 4:15 an instance of `Size` for `Color` must be in the module of `Size` or of `Color`");
    // 型のモジュールに書いた instance は、クラスが別のモジュールでも書ける
    let ty = "import C (Size(..))\n\npub data Color = | Red\n\ninstance Size Color where\n  size _ = 1\n";
    let entry = "import T (Color(..))\n\nmain : Unit -> <IO> Unit\nmain () = ()";
    let lowered = lower_files(entry, &[("C.em", class), ("T.em", ty)]);
    assert!(lowered.diagnostics.is_empty(), "{:?}", lowered.diagnostics);
}

#[test]
fn constraints_are_checked() {
    insta::assert_snapshot!(lines("class Size a where\n  size : a -> Int\n  pair : Size a => a -> Int\n  free : Int -> Int\n\nf : Size b => Int -> Int\nf x = x\n\ng : Int a => a -> a\ng x = x\n\nh : Nope a => a -> a\nh x = x\n\neffect E where\n  op : Size a => a -> Unit").join("\n"), @"
    E1040 3:10 a method cannot constrain the class variable `a`
    E1040 4:10 the method `free` does not mention the class variable `a`
    E1040 6:10 the constraint on `b` is ambiguous
    E1041 9:5 `Int` is not a class
    E1002 12:5 cannot find class `Nope`
    E1040 16:8 an effect operation cannot have constraints
    ");
    insta::assert_snapshot!(lines("class Size a where\n  size : a -> Int\n\nclass Size b => Big a where\n  big : a -> Int\n\nclass A a => B a where\n  b : a -> Int\n\nclass B a => A a where\n  a : a -> Int\n\nk : Size -> Int\nk x = 1").join("\n"), @"
    E1040 4:12 a superclass constraint must be on the class variable `a`
    E1042 10:14 the superclasses of `A` make a cycle
    E1043 13:5 `Size` is a class, not a type
    ");
}

#[test]
fn a_method_variable_named_like_a_head_variable_is_renamed() {
    // 型の表と単相化の代入は型変数を名前で引くので、instance の頭の `b` とメソッドの `b` を別の名前にする
    let text = "data Box a = | Box a\n\nclass Fold f where\n  fold : f -> b -> (b -> Int -> b) -> b\n\ninstance Fold (Box b) where\n  fold (Box _) acc _ = acc";
    let lowered = lower(text);
    assert!(lowered.diagnostics.is_empty(), "{:?}", lowered.diagnostics);
    let program = &lowered.program;
    let (_, function) = program
        .functions()
        .find(|(_, f)| f.name == "Fold Box.fold")
        .unwrap();
    assert!(matches!(function.kind, FunctionKind::InstanceMethod(..)));
    let names: Vec<&str> = function
        .signature
        .as_ref()
        .unwrap()
        .generics
        .type_vars
        .iter()
        .map(|(_, var)| var.name.as_str())
        .collect();
    assert_eq!(names, ["b", "b1"]);
}

#[test]
fn an_instance_body_annotation_sees_only_head_variables() {
    let text = "data Box a = | Box a\n\nclass Fold f where\n  fold : f -> b -> b\n\ninstance Fold (Box a) where\n  fold (Box x) acc = (acc : b)";
    assert_eq!(lines(text), ["E1002 7:29 cannot find type variable `b`"]);
}

#[test]
fn extern_methods_are_only_for_the_standard_library() {
    let text = "class Size a where\n  size : a -> Int\n\ninstance Size Int where\n  extern size";
    assert_eq!(
        lines(text),
        ["E1033 5:3 `extern` is only allowed in the standard library"]
    );
}

#[test]
fn instance_members_are_grouped_by_name() {
    // 離れた等式は E1018、`extern` の行と等式で同じメソッドを2回定義したら、後の定義を E1003 にする
    let text = format!(
        "{COLOR}class Size a where\n  size : a -> Int\n  big : a -> Bool\n\ninstance Size Color where\n  size Red = 1\n  big _ = True\n  size Green = 2\n  extern big\n  extern big"
    );
    assert_eq!(
        lines(&text),
        [
            "E1018 12:3 the equations of `size` are not consecutive",
            "E1003 13:10 `big` is defined more than once",
            "E1003 14:10 `big` is defined more than once",
        ]
    );
}

#[test]
fn deriving_makes_instances_with_a_context_per_parameter() {
    // 文脈は、フィールドに現れる型引数ごとに置く。関数型の中の `b` にも置き、現れない `c` には置かない
    let text = "data Pair a b c = | Pair a (Int -> b) deriving Eq";
    let lowered = lower(text);
    assert!(lowered.diagnostics.is_empty(), "{:?}", lowered.diagnostics);
    let program = &lowered.program;
    let (instance, def) = program
        .instances()
        .find(|(_, def)| program.names.ty(def.head) == "Pair")
        .expect("the derived instance");
    assert_eq!(def.class, program.lang.eq);
    assert!(matches!(def.origin, InstanceOrigin::Derived(_)));
    assert!(def.methods.is_empty());
    assert_eq!(program.instance(program.lang.eq, def.head), Some(instance));
    let context: Vec<(&str, &str)> = def
        .context
        .iter()
        .map(|constraint| {
            (
                program.names.class(constraint.class),
                def.generics.type_vars[constraint.var].name.as_str(),
            )
        })
        .collect();
    assert_eq!(context, [("Eq", "a"), ("Eq", "b")]);
}

#[test]
fn deriving_twice_or_with_a_written_instance_is_a_duplicate() {
    let text = "data Color =\n  | Red\n  | Green\n  deriving (Show, Show)\n\ndata Size = | Size Int deriving Eq\n\ninstance Eq Size where\n  _ == _ = True";
    let found = lines(text);
    assert_eq!(
        found
            .iter()
            .filter(|line| line.starts_with("E1035"))
            .count(),
        2,
        "{found:?}"
    );
}
