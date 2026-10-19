//! 関数のブロックの列を前から組み立てる (docs/spec/core-ir.md)。ブロックは作った順に番号が付く。
//! `switch` の行き先は `switch` を置くときに作り、ラベルのブロックはそこへ向かう辺がすべて出そろってから作るので、
//! 作った順は前向きの辺だけになる。開いたままのブロック (終端のないブロック) は複数持てる。

use std::mem;

use crate::{Block, CoreFn, Stmt, Term};
use eml_lir::{Atom, BlockId, Mark, Repr, VarId, VarInfo};

/// 値の渡し先になる、まだブロックになっていない位置。そこへ向かう開いたままのブロックを集め、ラベルを持つ式を
/// 変換し終えたら `resolve` で扱いを決める。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Label(usize);

struct LabelState {
    /// ラベルをブロックとして置くときに作る引数。向かうブロックが1本なら作らない。
    params: Vec<VarInfo>,
    /// ラベルへ向かう開いたままのブロックと、そこから渡す値。
    pending: Vec<(BlockId, Vec<Atom>)>,
    resolved: bool,
}

/// 組み立て中のブロック。`term` が `None` のブロックは開いたままである。
struct Slot {
    params: Vec<VarId>,
    stmts: Vec<Stmt>,
    term: Option<Term>,
}

pub(crate) struct FnBuilder {
    vars: Vec<VarInfo>,
    blocks: Vec<Slot>,
    /// 文を足すブロック。終端を置くか、ラベルへ向かうと `None` になる。
    current: Option<BlockId>,
    labels: Vec<LabelState>,
}

impl FnBuilder {
    /// 入口のブロックを今のブロックにして始める。
    pub(crate) fn new() -> FnBuilder {
        let mut builder = FnBuilder {
            vars: Vec::new(),
            blocks: Vec::new(),
            current: None,
            labels: Vec::new(),
        };
        let entry = builder.new_block();
        builder.reopen(entry);
        builder
    }

    pub(crate) fn var(&mut self, info: VarInfo) -> VarId {
        self.vars.push(info);
        VarId(self.vars.len() as u32 - 1)
    }

    pub(crate) fn mark(&self, var: VarId) -> Mark {
        self.vars[var.0 as usize].mark
    }

    pub(crate) fn repr(&self, var: VarId) -> Repr {
        self.vars[var.0 as usize].repr
    }

    /// 関数の引数。入口のブロックの引数になる。
    pub(crate) fn param(&mut self, info: VarInfo) -> VarId {
        let var = self.var(info);
        self.blocks[BlockId::ENTRY.0 as usize].params.push(var);
        var
    }

    pub(crate) fn emit(&mut self, stmt: Stmt) {
        let block = self.current.expect("statements go into an open block");
        self.blocks[block.0 as usize].stmts.push(stmt);
    }

    /// 今のブロックを終端で閉じる。
    pub(crate) fn terminate(&mut self, term: Term) {
        let block = self
            .current
            .take()
            .expect("a terminator closes an open block");
        self.blocks[block.0 as usize].term = Some(term);
    }

    /// 開いたままの新しいブロックを置く。今のブロックは変えない。`switch` の行き先に使う。
    pub(crate) fn new_block(&mut self) -> BlockId {
        self.blocks.push(Slot {
            params: Vec::new(),
            stmts: Vec::new(),
            term: None,
        });
        BlockId(self.blocks.len() as u32 - 1)
    }

    /// 開いたままのブロックに戻り、今のブロックにする。
    pub(crate) fn reopen(&mut self, block: BlockId) {
        debug_assert!(
            self.current.is_none(),
            "the current block is closed or handed to a label first"
        );
        debug_assert!(
            self.blocks[block.0 as usize].term.is_none(),
            "only an open block is reopened"
        );
        self.current = Some(block);
    }

    /// 今のブロックを開いたまま手放す。後で `reopen` で戻る。値をそのまま調べる文脈が、値の分からないブロックを
    /// 集めるのに使う。
    pub(crate) fn suspend(&mut self) -> BlockId {
        self.current
            .take()
            .expect("only an open block is suspended")
    }

    pub(crate) fn new_label(&mut self, params: Vec<VarInfo>) -> Label {
        self.labels.push(LabelState {
            params,
            pending: Vec::new(),
            resolved: false,
        });
        Label(self.labels.len() - 1)
    }

    /// 今のブロックを、`args` を渡して `label` へ向かうブロックにする。ブロックは開いたままで、`resolve` が扱いを
    /// 決めるまで終端を持たない。
    pub(crate) fn jump(&mut self, label: Label, args: Vec<Atom>) {
        let state = &mut self.labels[label.0];
        debug_assert!(!state.resolved, "no jump reaches a resolved label");
        debug_assert_eq!(
            state.params.len(),
            args.len(),
            "a jump passes one value per parameter"
        );
        let block = self.current.take().expect("a jump leaves an open block");
        state.pending.push((block, args));
    }

    /// ラベルへ向かうブロックの数で扱いを決め、ラベルの引数の値を返す (docs/spec/core-ir.md)。
    /// 0 本なら `None` で、今のブロックはない。1 本ならそのブロックに戻り、引数はそのブロックが渡す値になる。2 本以上
    /// なら各ブロックを `jump` で閉じ、ラベルを引数のあるブロックとして置いて今のブロックにする。
    pub(crate) fn resolve(&mut self, label: Label) -> Option<Vec<Atom>> {
        let state = &mut self.labels[label.0];
        state.resolved = true;
        let mut pending = mem::take(&mut state.pending);
        match pending.len() {
            0 => None,
            1 => {
                let (block, args) = pending.pop().expect("one block");
                self.reopen(block);
                Some(args)
            }
            _ => Some(self.place(label, pending)),
        }
    }

    fn place(&mut self, label: Label, pending: Vec<(BlockId, Vec<Atom>)>) -> Vec<Atom> {
        let target = self.new_block();
        for (block, args) in pending {
            self.blocks[block.0 as usize].term = Some(Term::Jump { target, args });
        }
        let params: Vec<VarId> = mem::take(&mut self.labels[label.0].params)
            .into_iter()
            .map(|info| self.var(info))
            .collect();
        let atoms = params.iter().map(|&param| Atom::Var(param)).collect();
        self.blocks[target.0 as usize].params = params;
        self.reopen(target);
        atoms
    }

    /// 文がなく `return p` だけのブロック `b(p)` への `jump b(a)` を `return a` にし、すべての `jump` が変わった `b` を
    /// 消す。番号の大きいブロックから見るので、`return` に変わったブロックが次に消せる形になっても、同じ1回のループで
    /// 消える。消したブロックを詰めて、番号を振り直す。末尾呼び出しはここでは作らず、縮約に任せる (docs/spec/core-ir.md)。
    /// 印の互換は推移的でないので、`a` が定数か、`a` の印が `p` の印と同じときだけ変える
    /// (docs/spec/lir.md の「Kind の印」、docs/spec/core-ir.md の「変換の規則」)。
    /// `internal` は内部の関数の印 (`CoreFn::internal`) である。
    pub(crate) fn finish(
        self,
        name: String,
        internal: bool,
        yields: bool,
        ret: Repr,
        ret_mark: Mark,
    ) -> CoreFn {
        debug_assert!(
            self.labels.iter().all(|label| label.pending.is_empty()),
            "every label is resolved"
        );
        let mut blocks: Vec<Slot> = self.blocks;
        let mut jumps_into: Vec<Vec<usize>> = vec![Vec::new(); blocks.len()];
        for (index, slot) in blocks.iter().enumerate() {
            if let Some(Term::Jump { target, args: _ }) = &slot.term {
                jumps_into[target.0 as usize].push(index);
            }
        }
        let mut removed = vec![false; blocks.len()];
        for index in (1..blocks.len()).rev() {
            let slot = &blocks[index];
            let param = match (&slot.term, &slot.params[..]) {
                (Some(Term::Return(Atom::Var(value))), &[param])
                    if slot.stmts.is_empty() && *value == param =>
                {
                    param
                }
                _ => continue,
            };
            let mark = self.vars[param.0 as usize].mark;
            let mut kept = false;
            for &source in &jumps_into[index] {
                let Some(Term::Jump { target: _, args }) = &blocks[source].term else {
                    unreachable!("the source jumps into the forwarded block");
                };
                let [value] = args[..] else {
                    unreachable!("a jump passes one value per parameter");
                };
                let forwards = match value {
                    Atom::Var(var) => self.vars[var.0 as usize].mark == mark,
                    Atom::Int(_) | Atom::Float(_) | Atom::Unit | Atom::Tag(_) | Atom::Fn(_) => true,
                };
                if forwards {
                    blocks[source].term = Some(Term::Return(value));
                } else {
                    kept = true;
                }
            }
            removed[index] = !kept;
        }
        let mut renumbered = Vec::with_capacity(blocks.len());
        let mut next = 0;
        for &gone in &removed {
            renumbered.push(BlockId(next));
            if !gone {
                next += 1;
            }
        }
        let blocks = blocks
            .into_iter()
            .zip(removed)
            .filter(|(_, gone)| !gone)
            .map(|(slot, _)| {
                let mut term = slot.term.expect("every block is closed");
                term.for_each_successor_mut(|target| *target = renumbered[target.0 as usize]);
                Block {
                    params: slot.params,
                    stmts: slot.stmts,
                    term,
                }
            })
            .collect();
        CoreFn {
            name,
            internal,
            yields,
            clause: None,
            vars: self.vars,
            ret,
            ret_mark,
            blocks,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eml_lir::{Case, CasePattern};

    fn info(name: &str, repr: Repr, mark: Mark) -> VarInfo {
        VarInfo {
            name: name.to_string(),
            repr,
            mark,
        }
    }

    /// `return` だけのブロックへの `jump` を `return` にするのは、実引数が定数か、印が行き先の引数と同じときだけで
    /// ある。この関数の `ret_mark` は `poly` なので、`lin` の `h` を直接返しても互換だが、`h` は転送しない。転送後の
    /// 互換を確かめず、印が同じことだけを条件にする保守的な規則を固定する
    /// (docs/spec/lir.md の「Kind の印」、docs/spec/core-ir.md の「変換の規則」)。
    #[test]
    fn a_jump_is_forwarded_only_when_its_argument_has_the_mark_of_the_parameter() {
        let mut builder = FnBuilder::new();
        let n = builder.param(info("n", Repr::Int, Mark::Unr));
        let same = builder.param(info("s", Repr::TObj, Mark::Poly));
        let other = builder.param(info("h", Repr::TObj, Mark::Lin));
        let label = builder.new_label(vec![info("p", Repr::TObj, Mark::Poly)]);
        let (zero, one, rest) = (
            builder.new_block(),
            builder.new_block(),
            builder.new_block(),
        );
        builder.terminate(Term::Switch {
            scrutinee: Atom::Var(n),
            layout: None,
            cases: vec![
                Case {
                    pattern: CasePattern::Int(0),
                    fields: Vec::new(),
                    target: zero,
                },
                Case {
                    pattern: CasePattern::Int(1),
                    fields: Vec::new(),
                    target: one,
                },
            ],
            default: Some(rest),
        });
        for (block, arg) in [
            (zero, Atom::Var(same)),
            (one, Atom::Var(other)),
            (rest, Atom::Unit),
        ] {
            builder.reopen(block);
            builder.jump(label, vec![arg]);
        }
        let [param] = builder
            .resolve(label)
            .expect("three blocks jump to the label")[..]
        else {
            unreachable!("the label has one parameter");
        };
        builder.terminate(Term::Return(param));
        let function = builder.finish("f".to_string(), false, false, Repr::TObj, Mark::Poly);
        let terms: Vec<&Term> = function.blocks.iter().map(|block| &block.term).collect();
        assert_eq!(terms[1], &Term::Return(Atom::Var(same)));
        assert!(matches!(terms[2], Term::Jump { args, .. } if args == &[Atom::Var(other)]));
        assert_eq!(terms[3], &Term::Return(Atom::Unit));
        assert_eq!(
            function.blocks.len(),
            5,
            "the parameter's block stays for `h`"
        );
    }
}
