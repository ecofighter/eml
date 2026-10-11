//! evidence passing の基本操作 (docs/spec/core-ir.md の「基本操作」)。
//! 引数はどれも所有した参照として受け取り、結果に残さないものは手放す。extern の行と同じ約束である。

use eml_core_ir::Prim;
use eml_runtime::{ObjRef, Payload, Value};

use crate::error::Fault;
use crate::runtime::{Runtime, YieldState};

impl Runtime<'_> {
    pub(crate) fn prim(&mut self, prim: &Prim, args: Vec<Value>) -> Result<Value, Fault> {
        match prim {
            Prim::EvvGet => {
                let [] = arguments(args)?;
                self.dup(self.evv)?;
                Ok(self.evv)
            }
            Prim::EvvSet => {
                let [list] = arguments(args)?;
                let old = std::mem::replace(&mut self.evv, list);
                self.decref(old)?;
                Ok(Value::Unit)
            }
            Prim::EvvPush => {
                let [list, evidence] = arguments(args)?;
                let evidence = object(evidence)?;
                Ok(self.alloc(Payload::Evv {
                    evidence,
                    next: list,
                }))
            }
            Prim::EvvMask(effects) => {
                let [list] = arguments(args)?;
                self.mask(effects, list)
            }
            Prim::EvNew(effect) => {
                let [state, outer] = arguments(args)?;
                let size = *self
                    .effect_sizes
                    .get(*effect as usize)
                    .ok_or(Fault::Internal("an evidence of an unknown effect"))?;
                let table = self.heap.alloc(Payload::EvTable {
                    effect: *effect,
                    entries: vec![Value::Unit; size],
                });
                Ok(self.alloc(Payload::Evidence {
                    table,
                    state,
                    outer,
                }))
            }
            Prim::EvSetClause { effect: _, op } => {
                let [evidence, clause] = arguments(args)?;
                let table = self.evidence(evidence)?.table;
                let entry = self.entry(table, *op)?;
                // 入口の欄は handle の場所で一度だけ入れ、その後は変えない
                // (docs/spec/core-ir.md の「evidence」)
                if *entry != Value::Unit {
                    return Err(Fault::Internal("a clause entry is set twice"));
                }
                *entry = clause;
                self.decref(evidence)?;
                Ok(Value::Unit)
            }
            Prim::EvRenew => {
                let [evidence, state] = arguments(args)?;
                let table = self.evidence(evidence)?.table;
                self.dup(Value::Obj(table))?;
                let outer = self.evv;
                self.dup(outer)?;
                let renewed = self.alloc(Payload::Evidence {
                    table,
                    state,
                    outer,
                });
                self.decref(evidence)?;
                Ok(renewed)
            }
            Prim::EvLookup(effect) => {
                let [] = arguments(args)?;
                self.lookup(*effect)
            }
            Prim::EvClause { effect: _, op } => {
                let [evidence] = arguments(args)?;
                let table = self.evidence(evidence)?.table;
                let clause = *self.entry(table, *op)?;
                self.dup(clause)?;
                self.decref(evidence)?;
                Ok(clause)
            }
            Prim::EvEvv => {
                let [evidence] = arguments(args)?;
                let outer = *self.evidence(evidence)?.outer;
                self.dup(outer)?;
                self.decref(evidence)?;
                Ok(outer)
            }
            Prim::EvTakeState => {
                let [evidence] = arguments(args)?;
                let state = std::mem::replace(self.evidence(evidence)?.state, Value::Unit);
                self.decref(evidence)?;
                Ok(state)
            }
            Prim::EvSetState => {
                let [evidence, state] = arguments(args)?;
                let old = std::mem::replace(self.evidence(evidence)?.state, state);
                self.decref(old)?;
                self.decref(evidence)?;
                Ok(Value::Unit)
            }
            Prim::Yielding => {
                let [] = arguments(args)?;
                let yielding = matches!(self.yield_state, YieldState::Yielding { .. });
                Ok(Value::Int(i64::from(yielding)))
            }
            Prim::YieldStatus => {
                let [evidence] = arguments(args)?;
                let obj = object(evidence)?;
                let status = match self.yield_state {
                    YieldState::None => 0,
                    YieldState::Yielding { target, abort, .. } if target == obj => {
                        if abort {
                            2
                        } else {
                            1
                        }
                    }
                    YieldState::Yielding { .. } => 3,
                };
                self.decref(evidence)?;
                Ok(Value::Int(status))
            }
            Prim::YieldTo => self.start_yield(args, false),
            Prim::YieldAbort => self.start_yield(args, true),
            Prim::YieldContTake => {
                let [] = arguments(args)?;
                Ok(std::mem::replace(self.yield_cont()?, Value::Unit))
            }
            Prim::YieldContSet => {
                let [cont] = arguments(args)?;
                let slot = self.yield_cont()?;
                if *slot != Value::Unit {
                    return Err(Fault::Internal(
                        "a yield continuation is set while it holds another",
                    ));
                }
                *slot = cont;
                self.yield_frames += 1;
                Ok(Value::Unit)
            }
            Prim::YieldFinish => {
                let [] = arguments(args)?;
                let YieldState::Yielding {
                    target,
                    clause,
                    cont,
                    abort: _,
                } = std::mem::replace(&mut self.yield_state, YieldState::None)
                else {
                    return Err(Fault::Internal("a yield finishes outside a yield"));
                };
                if cont != Value::Unit {
                    return Err(Fault::Internal(
                        "a yield finishes while its continuation is still held",
                    ));
                }
                self.decref(Value::Obj(target))?;
                Ok(clause)
            }
            Prim::Dummy => {
                let [] = arguments(args)?;
                Ok(Value::Dummy)
            }
        }
    }

    /// 今の `evv` を先頭からたどり、エフェクトが `effect` の最初の evidence を返す。費用は今有効な handler の数に比例し、
    /// 呼び出しの深さにはよらない (docs/spec/core-ir.md の「evidence を探す」)。
    fn lookup(&mut self, effect: u32) -> Result<Value, Fault> {
        let mut list = self.evv;
        while let Some((evidence, next)) = self.node(list)? {
            self.evidence_steps += 1;
            if self.effect_of(evidence)? == effect {
                let evidence = Value::Obj(evidence);
                self.dup(evidence)?;
                return Ok(evidence);
            }
            list = next;
        }
        Err(Fault::Internal("an operation without evidence"))
    }

    /// `effects` の多重集合の各エフェクトについて、`list` の先頭から合う evidence をその数だけ除いたリストを作る。
    /// 除いた最後の節点の前までは写し、その後ろは元のリストを共有する。evidence の物体は写さない
    /// (docs/spec/core-ir.md の「evidence を探す」)。型の付いたプログラムでは、合う evidence がいつも足りる
    /// (docs/spec/effects.md の「健全性」)。`mask` は handle できるエフェクトだけを挙げるので、extern のエフェクトも来ない。
    fn mask(&mut self, effects: &[u32], list: Value) -> Result<Value, Fault> {
        let mut remaining = effects.to_vec();
        let mut kept = Vec::new();
        let mut rest = list;
        while !remaining.is_empty() {
            let Some((evidence, next)) = self.node(rest)? else {
                return Err(Fault::Internal(
                    "a mask skips more evidence than the list holds",
                ));
            };
            self.evidence_steps += 1;
            let effect = self.effect_of(evidence)?;
            match remaining.iter().position(|&masked| masked == effect) {
                Some(index) => {
                    remaining.swap_remove(index);
                }
                None => kept.push(evidence),
            }
            rest = next;
        }
        // `list` を手放すと共有する後ろの部分も解放されうるので、先に参照を増やす
        self.dup(rest)?;
        let mut masked = rest;
        for &evidence in kept.iter().rev() {
            self.dup(Value::Obj(evidence))?;
            masked = self.alloc(Payload::Evv {
                evidence,
                next: masked,
            });
        }
        self.decref(list)?;
        Ok(masked)
    }

    fn start_yield(&mut self, args: Vec<Value>, abort: bool) -> Result<Value, Fault> {
        let [evidence, clause, cont] = arguments(args)?;
        let target = object(evidence)?;
        if !matches!(self.yield_state, YieldState::None) {
            return Err(Fault::Internal("a yield starts during another yield"));
        }
        self.yield_state = YieldState::Yielding {
            target,
            clause,
            cont,
            abort,
        };
        self.yields += 1;
        Ok(Value::Unit)
    }

    fn yield_cont(&mut self) -> Result<&mut Value, Fault> {
        match &mut self.yield_state {
            YieldState::Yielding { cont, .. } => Ok(cont),
            YieldState::None => Err(Fault::Internal(
                "a yield continuation is used outside a yield",
            )),
        }
    }

    /// `evv` のリストの先頭の節点の evidence と次の節点。空のリストなら `None` である。
    fn node(&self, list: Value) -> Result<Option<(ObjRef, Value)>, Fault> {
        let Value::Obj(obj) = list else {
            return match list {
                Value::Unit => Ok(None),
                _ => Err(not_a_list()),
            };
        };
        match self.heap.get(obj).map_err(Fault::Heap)? {
            Payload::Evv { evidence, next } => Ok(Some((*evidence, *next))),
            _ => Err(not_a_list()),
        }
    }

    fn evidence(&mut self, evidence: Value) -> Result<EvidenceFields<'_>, Fault> {
        match self.heap.get_mut(object(evidence)?).map_err(Fault::Heap)? {
            Payload::Evidence {
                table,
                state,
                outer,
            } => Ok(EvidenceFields {
                table: *table,
                state,
                outer,
            }),
            _ => Err(not_an_evidence()),
        }
    }

    fn effect_of(&self, evidence: ObjRef) -> Result<u32, Fault> {
        let Payload::Evidence { table, .. } = self.heap.get(evidence).map_err(Fault::Heap)? else {
            return Err(not_an_evidence());
        };
        match self.heap.get(*table).map_err(Fault::Heap)? {
            Payload::EvTable { effect, .. } => Ok(*effect),
            _ => Err(not_a_table()),
        }
    }

    fn entry(&mut self, table: ObjRef, op: u32) -> Result<&mut Value, Fault> {
        let Payload::EvTable { entries, .. } = self.heap.get_mut(table).map_err(Fault::Heap)?
        else {
            return Err(not_a_table());
        };
        entries.get_mut(op as usize).ok_or(Fault::Internal(
            "an operation that is not in the entry table",
        ))
    }
}

/// evidence の物体の欄。入口の表は共有されるので参照だけを、状態と外側の `evv` は書き換えられるように借りて返す。
struct EvidenceFields<'h> {
    table: ObjRef,
    state: &'h mut Value,
    outer: &'h mut Value,
}

/// 引数の数は verifier が基本操作の表で確かめる。ここでは見張りとして確かめる。
fn arguments<const N: usize>(args: Vec<Value>) -> Result<[Value; N], Fault> {
    args.try_into().map_err(|_| {
        Fault::Internal("a primitive is given a different number of arguments than it takes")
    })
}

fn object(value: Value) -> Result<ObjRef, Fault> {
    match value {
        Value::Obj(obj) => Ok(obj),
        _ => Err(not_an_evidence()),
    }
}

fn not_an_evidence() -> Fault {
    Fault::Internal("an evidence primitive is given a value that is not an evidence")
}

fn not_a_list() -> Fault {
    Fault::Internal("the evidence list has a value that is not a list node")
}

fn not_a_table() -> Fault {
    Fault::Internal("an evidence does not point to an entry table")
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use eml_runtime::OutputSink;

    use super::*;

    /// エフェクト 0 と 1 は、どちらも操作を1つ持つ。
    fn runtime(out: &OutputSink) -> Runtime<'_> {
        Runtime::new(out, Path::new(""), &[], Vec::new(), vec![1, 1])
    }

    /// `evv` の先頭に、エフェクト `effect` の新しい evidence を足す。
    fn push(rt: &mut Runtime<'_>, effect: u32) {
        let evidence = rt
            .prim(&Prim::EvNew(effect), vec![Value::Unit, Value::Unit])
            .unwrap();
        let list = rt.prim(&Prim::EvvGet, Vec::new()).unwrap();
        let list = rt.prim(&Prim::EvvPush, vec![list, evidence]).unwrap();
        rt.prim(&Prim::EvvSet, vec![list]).unwrap();
    }

    fn clear(rt: &mut Runtime<'_>) {
        rt.prim(&Prim::EvvSet, vec![Value::Unit]).unwrap();
    }

    #[test]
    fn lookups_and_masks_count_the_list_nodes_they_visit() {
        let (out, _captured) = OutputSink::capture();
        let mut rt = runtime(&out);
        push(&mut rt, 0);
        push(&mut rt, 1);
        push(&mut rt, 1);
        let found = rt.prim(&Prim::EvLookup(0), Vec::new()).unwrap();
        rt.decref(found).unwrap();
        assert_eq!(rt.evidence_steps, 3);
        let list = rt.prim(&Prim::EvvGet, Vec::new()).unwrap();
        let masked = rt.prim(&Prim::EvvMask(vec![1]), vec![list]).unwrap();
        assert_eq!(rt.evidence_steps, 4);
        rt.decref(masked).unwrap();
        clear(&mut rt);
    }

    /// 型の付いたプログラムでは、`mask` が名前を挙げた数だけ evidence がいつもリストにある (docs/spec/effects.md の
    /// 「健全性」)。足りないのはコンパイラの誤りなので、見つかった分だけを除いて続けず、内部の誤りにする。
    #[test]
    fn a_mask_that_skips_more_evidence_than_the_list_holds_is_an_internal_fault() {
        let (out, _captured) = OutputSink::capture();
        let mut rt = runtime(&out);
        push(&mut rt, 0);
        push(&mut rt, 1);
        let list = rt.prim(&Prim::EvvGet, Vec::new()).unwrap();
        assert_eq!(
            rt.prim(&Prim::EvvMask(vec![1, 1]), vec![list]),
            Err(Fault::Internal(
                "a mask skips more evidence than the list holds"
            ))
        );
    }

    #[test]
    fn yields_and_continuation_frames_are_counted() {
        let (out, _captured) = OutputSink::capture();
        let mut rt = runtime(&out);
        let evidence = rt
            .prim(&Prim::EvNew(0), vec![Value::Unit, Value::Unit])
            .unwrap();
        rt.prim(&Prim::YieldTo, vec![evidence, Value::Unit, Value::Unit])
            .unwrap();
        for _ in 0..2 {
            let cont = rt.prim(&Prim::YieldContTake, Vec::new()).unwrap();
            rt.prim(&Prim::YieldContSet, vec![cont]).unwrap();
        }
        let cont = rt.prim(&Prim::YieldContTake, Vec::new()).unwrap();
        rt.decref(cont).unwrap();
        rt.prim(&Prim::YieldFinish, Vec::new()).unwrap();
        assert_eq!((rt.yields, rt.yield_frames), (1, 2));
        assert_eq!(rt.check_context(), Ok(()));
    }
}
