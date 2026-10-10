//! 関数のブロックの列を前から組み立てる (docs/spec/core-ir.md)。ブロックは作った順に番号が付く。
//! `switch` の行き先は `switch` を置くときに作り、ラベルのブロックはそこへ向かう辺がすべて出そろってから作るので、
//! 作った順は前向きの辺だけになる。開いたままのブロック (終端のないブロック) は複数持てる。

use std::mem;

use eml_extern::{Extern, ExternType};

use crate::{Atom, Block, BlockId, CoreFn, Repr, Rhs, Stmt, Term, VarId, VarInfo};

use super::program::Interner;
use super::types::named;

/// 値の渡し先になる、まだブロックになっていない位置。そこへ向かう開いたままのブロックを集め、ラベルを持つ式を
/// 変換し終えたら `resolve` で扱いを決める。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Label(usize);

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

pub(super) struct FnBuilder {
    vars: Vec<VarInfo>,
    blocks: Vec<Slot>,
    /// 文を足すブロック。終端を置くか、ラベルへ向かうと `None` になる。
    current: Option<BlockId>,
    labels: Vec<LabelState>,
}

impl FnBuilder {
    /// 入口のブロックを今のブロックにして始める。
    pub(super) fn new() -> FnBuilder {
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

    pub(super) fn var(&mut self, info: VarInfo) -> VarId {
        self.vars.push(info);
        VarId(self.vars.len() as u32 - 1)
    }

    /// 関数の引数。入口のブロックの引数になる。
    pub(super) fn param(&mut self, info: VarInfo) -> VarId {
        let var = self.var(info);
        self.blocks[BlockId::ENTRY.0 as usize].params.push(var);
        var
    }

    pub(super) fn emit(&mut self, stmt: Stmt) {
        let block = self.current.expect("statements go into an open block");
        self.blocks[block.0 as usize].stmts.push(stmt);
    }

    /// 文字列の定数を新しい変数に束縛する。導出した `Show` と補間が、同じ形で文字列を組むためにここに置く。
    pub(super) fn string(&mut self, strings: &mut Interner, text: &str) -> Atom {
        let id = strings.intern(text);
        self.bind_string(Rhs::ConstString(id))
    }

    /// extern の `++` (`StrConcat`) で2つの文字列をつなぐ。ユーザーが定義した `++` によらない
    /// (docs/spec/core-ir.md の「変換の規則」)。
    pub(super) fn concat(&mut self, left: Atom, right: Atom) -> Atom {
        self.bind_string(Rhs::Extern {
            ext: Extern::StrConcat,
            args: vec![left, right],
            at: None,
        })
    }

    fn bind_string(&mut self, rhs: Rhs) -> Atom {
        let var = self.var(named("s", ExternType::String.row().repr));
        self.emit(Stmt::Let { var, rhs });
        Atom::Var(var)
    }

    /// 今のブロックを終端で閉じる。
    pub(super) fn terminate(&mut self, term: Term) {
        let block = self
            .current
            .take()
            .expect("a terminator closes an open block");
        self.blocks[block.0 as usize].term = Some(term);
    }

    /// 開いたままの新しいブロックを置く。今のブロックは変えない。`switch` の行き先に使う。
    pub(super) fn new_block(&mut self) -> BlockId {
        self.blocks.push(Slot {
            params: Vec::new(),
            stmts: Vec::new(),
            term: None,
        });
        BlockId(self.blocks.len() as u32 - 1)
    }

    /// 開いたままのブロックに戻り、今のブロックにする。
    pub(super) fn reopen(&mut self, block: BlockId) {
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
    pub(super) fn suspend(&mut self) -> BlockId {
        self.current
            .take()
            .expect("only an open block is suspended")
    }

    pub(super) fn new_label(&mut self, params: Vec<VarInfo>) -> Label {
        self.labels.push(LabelState {
            params,
            pending: Vec::new(),
            resolved: false,
        });
        Label(self.labels.len() - 1)
    }

    /// 今のブロックを、`args` を渡して `label` へ向かうブロックにする。ブロックは開いたままで、`resolve` が扱いを
    /// 決めるまで終端を持たない。
    pub(super) fn jump(&mut self, label: Label, args: Vec<Atom>) {
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
    pub(super) fn resolve(&mut self, label: Label) -> Option<Vec<Atom>> {
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

    /// 文がなく `return p` だけのブロック `b(p)` へのすべての `jump b(a)` を `return a` にして `b` を消す。番号の
    /// 大きいブロックから見るので、`return` に変わったブロックが次に消せる形になっても、同じ1回のループで消える。
    /// 消したブロックを詰めて、番号を振り直す。末尾呼び出しはここでは作らず、縮約に任せる (docs/spec/core-ir.md)。
    /// `internal` は内部の関数の印 (`CoreFn::internal`) である。
    pub(super) fn finish(self, name: String, internal: bool, ret: Repr) -> CoreFn {
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
            let forwards = slot.stmts.is_empty()
                && matches!(slot.term, Some(Term::Return(Atom::Var(value))) if slot.params == [value]);
            if !forwards {
                continue;
            }
            removed[index] = true;
            for &source in &jumps_into[index] {
                let Some(Term::Jump { target: _, args }) = blocks[source].term.take() else {
                    unreachable!("the source jumps into the forwarded block");
                };
                let [value] = args[..] else {
                    unreachable!("a jump passes one value per parameter");
                };
                blocks[source].term = Some(Term::Return(value));
            }
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
            vars: self.vars,
            ret,
            blocks,
        }
    }
}
