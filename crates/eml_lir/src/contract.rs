//! box の挿入と Perceus の間の縮約のパス (docs/spec/lir.md の「縮約」)。使われない純粋な `let` を消し、その後で
//! すべてのブロックに末尾呼び出しの規則を当てる。末尾呼び出しを作るのはこのパスだけである。RC の命令がまだないので、
//! 所有権を扱わずに書き換えられる。

use crate::boxing::tail_position_call;
use crate::{
    Atom, Block, Call, EffectInfo, LirFn, Mark, OpRet, Program, Rhs, Stmt, Term, VarId, operation,
};

pub fn contract(program: &mut Program) {
    let ret_marks: Vec<Vec<Mark>> = program
        .functions
        .iter()
        .map(|function| function.rets.iter().map(|&(_, mark)| mark).collect())
        .collect();
    for function in &mut program.functions {
        remove_dead_lets(function);
        let rets: Vec<Mark> = function.rets.iter().map(|&(_, mark)| mark).collect();
        for block in &mut function.blocks {
            tail_call(block, &rets, &ret_marks, &program.effects);
        }
        // 1回のパスで不動点に達することを確かめる (docs/spec/lir.md の「縮約」)。確かめるパスも IR を書き換えうるので、
        // その副作用を `debug_assert!` の式に隠さない
        if cfg!(debug_assertions) {
            let removed = remove_dead_lets(function);
            assert!(
                !removed,
                "one backward pass removes every dead binding in `{}`",
                function.name
            );
        }
    }
}

/// `let x = <呼び出し>` の後の終端が `return x` なら、その2つを末尾呼び出しにする。複数の値を返す呼び出しは、
/// `let (x.0, .., x.n) = <呼び出し>` の後の `return (x.0, .., x.n)` (同じ並び、同じ順) を同じく末尾呼び出しにする
/// (docs/spec/lir.md の「縮約」)。結果の Repr が関数の結果と
/// 互換でなくてもよい。結果の変換は VM の変換のフレームが行い、末尾呼び出しの間で合成する
/// (docs/spec/lir.md の「末尾の位置の呼び出し」)。印の互換は推移的でないので、
/// x を通してつながっていた結果の印 (関数値の適用なら `poly`) と関数の戻り値の印を、値ごとに直接つないでも互換かを
/// 確かめる。値の数が違う形 (構造体と箱の間) の印は変換のフレームが受け渡すので比べない。`never` の操作の節の
/// 呼び出しは戻らないので、どの戻り値の印とも互換とする (docs/spec/lir.md の「縮約」と「Kind の印」)。
fn tail_call(block: &mut Block, rets: &[Mark], ret_marks: &[Vec<Mark>], effects: &[EffectInfo]) {
    let returned: &[Atom] = match &block.term {
        Term::Return(atom) => std::slice::from_ref(atom),
        Term::ReturnMany(atoms) => atoms,
        _ => return,
    };
    let (vars, call): (&[VarId], &Call) = match block.stmts.last() {
        Some(Stmt::Let {
            var,
            rhs: Rhs::Call(call),
        }) => (std::slice::from_ref(var), call),
        Some(Stmt::LetMany { vars, call }) => (vars, call),
        _ => return,
    };
    if vars.len() != returned.len()
        || vars
            .iter()
            .zip(returned)
            .any(|(&var, &atom)| atom != Atom::Var(var))
    {
        return;
    }
    let result_marks = match call {
        Call::Direct(target, _) => Some(ret_marks[target.0 as usize].clone()),
        Call::Apply { .. } => Some(vec![Mark::Poly; vars.len()]),
        Call::Clause {
            effect,
            op,
            callee: _,
            args: _,
        } => {
            let operation = operation(effects, *effect, *op);
            match operation.ret {
                OpRet::Repr(_) => Some(vec![operation.ret_mark]),
                OpRet::Never => None,
            }
        }
    };
    let compatible = result_marks.is_none_or(|marks| {
        marks.len() != rets.len()
            || marks
                .iter()
                .zip(rets)
                .all(|(&mark, &ret)| mark.compatible(ret))
    });
    if !compatible {
        return;
    }
    let call = match block.stmts.pop() {
        Some(Stmt::Let {
            var: _,
            rhs: Rhs::Call(call),
        })
        | Some(Stmt::LetMany { vars: _, call }) => call,
        _ => unreachable!("the last statement was checked above"),
    };
    block.term = Term::TailCall(call);
}

/// 使われない純粋な `let` を消し、1つでも消したかを返す。定義は使う位置を支配し、辺は番号の大きい
/// ブロックへ向かうので、ブロックを後ろから、文を後ろから見れば、`let` を見る時点でその変数の使用はすべて見終わって
/// いる。関数全体の使用の数を消すたびに減らせば、1回のパスで、消した `let` だけが使っていた束縛も消える。
fn remove_dead_lets(function: &mut LirFn) -> bool {
    let mut uses = vec![0usize; function.vars.len()];
    let mut count = |atom: Atom| {
        if let Atom::Var(var) = atom {
            uses[var.0 as usize] += 1;
        }
    };
    for block in &function.blocks {
        for stmt in &block.stmts {
            stmt.for_each_atom(&mut count);
        }
        block.term.for_each_atom(&mut count);
    }
    let mut removed = false;
    let vars = &function.vars;
    for block in function.blocks.iter_mut().rev() {
        let mut keep = vec![true; block.stmts.len()];
        let tail = tail_position_call(block).map(|(at, _)| (at, bound(&block.stmts[at])));
        for (position, stmt) in block.stmts.iter_mut().enumerate().rev() {
            match stmt {
                Stmt::Let { var, rhs } if uses[var.0 as usize] == 0 && pure(rhs) => {
                    keep[position] = false;
                    rhs.for_each_atom(|atom| {
                        if let Atom::Var(used) = atom {
                            uses[used.0 as usize] -= 1;
                        }
                    });
                }
                // 構造体の `unbox` は箱を読むだけの純粋な `let` である。使われない葉は `_` にして読まない。葉がどれも
                // 使われない `unbox` は、箱が `unr` か、箱が末尾の位置の呼び出しの結果で、その呼び出しの後ろにあれば
                // 消す。後者は開くパスの覗き穴が残すもので、消すと末尾の位置の呼び出しの形が保たれる
                // (docs/spec/lir.md の「縮約」)。ほかの `unr` でない箱の `unbox` は残す。Perceus はこの文で箱を知り、
                // 箱を手放す所を `decref` (V1 が拒む) でなく `drop` にする。末尾の位置の呼び出しの結果の箱は、呼び出しの
                // 結果として返すので、手放す所がない
                Stmt::UnboxStruct { value, fields, .. } => {
                    for field in fields.iter_mut() {
                        if field.is_some_and(|var| uses[var.0 as usize] == 0) {
                            *field = None;
                            removed = true;
                        }
                    }
                    if fields.iter().all(Option::is_none)
                        && (vars[value.0 as usize].mark == Mark::Unr
                            || tail.as_ref().is_some_and(|(at, results)| {
                                position > *at && results.contains(value)
                            }))
                    {
                        keep[position] = false;
                        uses[value.0 as usize] -= 1;
                    }
                }
                _ => {}
            }
        }
        if keep.contains(&false) {
            let mut keep = keep.into_iter();
            block.stmts.retain(|_| keep.next() == Some(true));
            removed = true;
        }
    }
    removed
}

/// 呼び出しの文が束縛する変数。
fn bound(stmt: &Stmt) -> Vec<VarId> {
    match stmt {
        Stmt::Let { var, .. } => vec![*var],
        Stmt::LetMany { vars, .. } => vars.clone(),
        _ => Vec::new(),
    }
}

/// 消してもよい右辺。値を作るだけで、エフェクトも実行時エラーも起こさない。extern は表の行が消せるもの
/// (`FunctionRow::removable`) だけである。
/// `con` と `closure` が所有権を受け取る値が `unr` なら、消すと Perceus がその値の生存の終わりに `decref` を入れるので、
/// 解放が早まるだけである。translate が開いた `unr` でない変数の `con` は、消さなければならない。その `lin` の引数は
/// 葉も読むので、残すと Perceus が `lin` の `con` を `decref` し、V1 が拒む。縮約は使われない純粋な `let` をすべて消し、
/// いつも Perceus より前に走るので、Perceus の入力にこの `con` は残らない (docs/spec/lir.md の「縮約」)。`box` を消すと
/// 確保が1つ減るだけで、`unbox` は値を読むだけなので、どちらも評価の順を変えない。token を受ける `con` は、消すと
/// token を手放す命令がなくなるので消さない。
pub(crate) fn pure(rhs: &Rhs) -> bool {
    match rhs {
        Rhs::Con {
            ctor: _,
            args: _,
            reuse,
        } => reuse.is_none(),
        Rhs::ConstString(_) | Rhs::MakeClosure(_, _) | Rhs::Box(_) | Rhs::Unbox(_) => true,
        Rhs::Extern {
            ext,
            args: _,
            at: _,
        } => ext.row().removable(),
        Rhs::Prim(prim, _) => prim.pure(),
        Rhs::Call(_) | Rhs::Drop(_) => false,
    }
}
