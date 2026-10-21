//! 直接だけ呼ばれる、値として生まれた関数を型どおりの ABI に戻す (docs/spec/core-ir.md の「直接だけ呼ばれる持ち上げた関数」)。

use std::collections::HashMap;

use crate::{Program, Rhs, Stmt};
use eml_lir::{Atom, FnIdx};

/// translate が持ち上げた値として生まれる関数と、その持ち主の番号。持ち主は、名前の `$` の前の部分を名前に持つ、
/// 内部でない関数のうち最も長いものである。translate は持ち上げた関数を `<根>$…` と名付け、根の instance と一緒に
/// 出すので、持ち上げた関数があれば持ち主も translate の出力にある。包む関数 (`$con$`、`$op$`、`$cont$`) は
/// どこからでも参照されるので、名前を `$` で始めて持ち主に当たらないようにしてある (docs/spec/core-ir.md の
/// 「直接だけ呼ばれる持ち上げた関数」)。optimize の前の、translate の出力に使う。
pub(crate) fn lifted(program: &Program) -> Vec<(FnIdx, FnIdx)> {
    let roots: HashMap<&str, FnIdx> = program
        .functions
        .iter()
        .enumerate()
        .filter(|(_, function)| !function.internal)
        .map(|(index, function)| (function.name.as_str(), FnIdx(index as u32)))
        .collect();
    program
        .functions
        .iter()
        .enumerate()
        .filter(|(_, function)| function.internal && function.split.is_some())
        .filter_map(|(index, function)| {
            let root = function
                .name
                .match_indices('$')
                .rev()
                .find_map(|(at, _)| roots.get(&function.name[..at]))?;
            Some((FnIdx(index as u32), *root))
        })
        .collect()
}

/// optimize の終わりに、一族のコードに値の参照 (`&f` と `closure f`) が残らない、値として生まれた関数を、型どおりの
/// ABI (`split` なし) にする。一族は2種類ある。
///
/// - 持ち主の一族 (持ち主と、同じ持ち主の持ち上げた関数)。持ち上げた関数があれば一族はそろって translate の出力に
///   ある
/// - handler への特殊化の末尾の経路の形の複製の一族 (`handlers` が返す。`f{H}` と、それを作るときに作った関数)。
///   `k` として渡す継続の関数 `f{H}$hN` を、節の写しの複製が直接呼ぶようになると、値の参照が残らない
///
/// どちらの一族のコードも名前で決まるので、決め方も名前だけで決まる。一族の外の関数 (持ち主の複製や、持ち上げた
/// 関数を鍵にする複製) に残る値の参照は、boxing が作る `$uniform` の版が受ける (docs/spec/lir.md の「関数の ABI」)。
/// 到達できない関数を落とす前に呼ぶ。
pub(crate) fn type_direct_only(
    program: &mut Program,
    lifted: &[(FnIdx, FnIdx)],
    continued: &[Vec<FnIdx>],
) {
    let mut families: HashMap<FnIdx, Vec<FnIdx>> = HashMap::new();
    for &(function, root) in lifted {
        families
            .entry(root)
            .or_insert_with(|| vec![root])
            .push(function);
    }
    let families = families
        .into_values()
        .map(|members| (members[1..].to_vec(), members))
        .chain(continued.iter().map(|members| {
            let candidates = members
                .iter()
                .copied()
                .filter(|&member| program.functions[member.0 as usize].split.is_some())
                .collect();
            (candidates, members.clone())
        }));
    let mut typed: Vec<FnIdx> = Vec::new();
    for (candidates, members) in families {
        let mut referenced = vec![false; program.functions.len()];
        for &member in &members {
            for_each_value_reference(program, member, |target| {
                referenced[target.0 as usize] = true;
            });
        }
        typed.extend(
            candidates
                .into_iter()
                .filter(|candidate| !referenced[candidate.0 as usize]),
        );
    }
    for function in typed {
        program.functions[function.0 as usize].split = None;
    }
}

/// 関数のコードが値として参照する関数。直接の呼び出しの相手は値の参照ではない。
fn for_each_value_reference(program: &Program, function: FnIdx, mut f: impl FnMut(FnIdx)) {
    let mut atom = |atom: Atom| {
        if let Atom::Fn(target) = atom {
            f(target);
        }
    };
    for block in &program.functions[function.0 as usize].blocks {
        for stmt in &block.stmts {
            let Stmt::Let { var: _, rhs } = stmt else {
                continue;
            };
            if let Rhs::MakeClosure(target, _) = rhs {
                atom(Atom::Fn(*target));
            }
            rhs.for_each_atom(&mut atom);
        }
        block.term.for_each_atom(&mut atom);
    }
}
