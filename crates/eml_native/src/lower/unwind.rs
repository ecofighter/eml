//! 巻き戻しの地点の後のコード、地点の後始末、仮の値、`eml$raise`、関数の位置の行 (docs/spec/native.md の
//! 「巻き戻しの地点」)。
use cranelift_codegen::ir::condcodes::IntCC;
use cranelift_codegen::ir::{Block, InstBuilder, Signature, Value, types};
use cranelift_frontend::FunctionBuilder;
use cranelift_module::Module;
use eml_extern::Purity;
use eml_lir::unwind::{UnwindPoint, Unwinds};
use eml_lir::{BlockId, Loc, Repr, VarId};
use eml_nrt::Kind;
use eml_nrt::image::{NO_FILE, PURITY_EFFECTFUL, PURITY_PANICS, PURITY_PURE, SiteRow};

use super::Imports;
use super::function::FnLower;
use crate::abi::tail_signature;
use crate::mem;

/// 巻き戻しの地点の命令の後に出すもの。`site`は関数自身の位置の行 (`FnRow.sites`) の添字である。
#[derive(Debug, Clone, Copy)]
pub(super) enum After {
    /// 末尾でない呼び出し。呼ばれた側が中断を始めたかを`yielding.target`で確かめる。
    Call,
    /// `faults`の行の呼び出し。faultの語を確かめる。
    Faults { site: u32, purity: Purity },
    /// 戻らない行。`raise`があれば、置いた失敗を上げてから後始末へ進む。
    Never { raise: Option<(u32, Purity)> },
}

/// 地点の命令の直後に持っているもの。
pub(super) struct Point<'a> {
    pub owned: &'a [(VarId, u32)],
    pub tokens: &'a [VarId],
    pub landing: Option<VarId>,
}

impl<'a> From<&'a UnwindPoint> for Point<'a> {
    fn from(point: &'a UnwindPoint) -> Point<'a> {
        Point {
            owned: &point.owned,
            tokens: &point.tokens,
            landing: point.landing,
        }
    }
}

/// `eml$raise(ctx, function, site, purity)`のシグネチャ。
pub(super) fn raise_signature() -> Signature {
    tail_signature(&[Kind::I64, Kind::I64, Kind::I64], &[])
}

/// `eml$raise`の本体。サービスに渡して戻る。
pub(super) fn define_raise<M: Module>(mut b: FunctionBuilder, module: &mut M, imports: &Imports) {
    let entry = b.create_block();
    b.append_block_params_for_function_params(entry);
    b.switch_to_block(entry);
    let args = b.block_params(entry).to_vec();
    let failed = module.declare_func_in_func(imports.entry("eml_nrt_failed"), b.func);
    b.ins().call(failed, &args);
    b.ins().return_(&[]);
    b.seal_all_blocks();
    b.finalize(module.target_config());
}

/// `eml_nrt_failed`の`purity`の語。
fn purity_word(purity: Purity) -> i64 {
    match purity {
        Purity::Pure => PURITY_PURE,
        Purity::Panics => PURITY_PANICS,
        Purity::Effectful => PURITY_EFFECTFUL,
    }
}

impl<'a, M: Module> FnLower<'a, '_, M> {
    /// `block`の`position`番目の文の巻き戻しの地点。
    pub fn point(&self, block: BlockId, position: usize) -> &'a UnwindPoint {
        let unwinds: &'a Unwinds = self.unwinds;
        let points = &unwinds.points;
        let index = points
            .binary_search_by_key(&(block, position), |point| (point.block, point.stmt))
            .unwrap_or_else(|_| {
                panic!(
                    "`{}` has no unwind point at {block:?} {position}",
                    self.function.name
                )
            });
        &points[index]
    }

    /// 関数の位置の行に`at`の行を足し、その添字を返す。失敗を上げる文だけが呼ぶ。
    pub fn site(&mut self, at: Option<Loc>) -> u32 {
        let row = match at {
            Some(at) => SiteRow {
                file: at.file,
                line: at.line,
                column: at.column,
            },
            None => SiteRow {
                file: NO_FILE,
                line: 0,
                column: 0,
            },
        };
        self.sites.push(row);
        u32::try_from(self.sites.len() - 1).expect("a function has fewer than 2^32 sites")
    }

    /// 巻き戻しの地点の命令の直後に置く。`Call`と`Faults`ではbuilderを熱い続きに置き、`Never`では前のブロックのない新しい
    /// ブロックに置いて戻る。
    pub fn after_unwind_point(&mut self, point: &Point, after: After) {
        match after {
            After::Call => {
                let target = mem::yield_target(&mut self.b, self.ctx);
                let unit = self.b.ins().iconst(types::I64, mem::UNIT);
                let aborting = self.b.ins().icmp(IntCC::NotEqual, target, unit);
                let cleanup = self.cold_block();
                let hot = self.b.create_block();
                self.b.ins().brif(aborting, cleanup, &[], hot, &[]);
                self.fill_cleanup(cleanup, point);
                self.b.switch_to_block(hot);
            }
            After::Faults { site, purity } => {
                let fault = mem::fault(&mut self.b, self.ctx);
                let raise = self.cold_block();
                let cleanup = self.cold_block();
                let hot = self.b.create_block();
                self.b.ins().brif(fault, raise, &[], hot, &[]);
                self.fill_raise(raise, site, purity, cleanup);
                self.fill_cleanup(cleanup, point);
                self.b.switch_to_block(hot);
            }
            After::Never { raise } => {
                match raise {
                    Some((site, purity)) => {
                        let block = self.cold_block();
                        let cleanup = self.cold_block();
                        self.b.ins().jump(block, &[]);
                        self.fill_raise(block, site, purity, cleanup);
                        self.fill_cleanup(cleanup, point);
                    }
                    None => {
                        let cleanup = self.cold_block();
                        self.b.ins().jump(cleanup, &[]);
                        self.fill_cleanup(cleanup, point);
                    }
                }
                // LIRの続きはどこからも届かないが、文と終端はそのまま下ろす
                let dead = self.b.create_block();
                self.b.switch_to_block(dead);
            }
        }
    }

    fn cold_block(&mut self) -> Block {
        let block = self.b.create_block();
        self.b.set_cold_block(block);
        block
    }

    /// `eml$raise(ctx, self, site, purity)`を呼び、後始末へ進む。`self`は関数自身のコードのアドレスなので、場所は
    /// 関数のLIRだけで決まる (docs/spec/native.md の「`Image`」)。
    fn fill_raise(&mut self, block: Block, site: u32, purity: Purity, cleanup: Block) {
        self.b.switch_to_block(block);
        let me = self.func_ref(self.id);
        let me = self.b.ins().func_addr(types::I64, me);
        let site = self.b.ins().iconst(types::I64, i64::from(site));
        let purity = self.b.ins().iconst(types::I64, purity_word(purity));
        let raise = self.func_ref(self.imports.raise);
        self.b.ins().call(raise, &[self.ctx, me, site, purity]);
        self.b.ins().jump(cleanup, &[]);
    }

    /// 地点の後始末。持っている参照を数だけ手放し、tokenを捨て、関数の結果の仮の値を返す。
    fn fill_cleanup(&mut self, block: Block, point: &Point) {
        assert!(
            point.landing.is_none(),
            "the native lowering has no landing"
        );
        self.b.switch_to_block(block);
        for &(var, count) in point.owned {
            let word = self.var(var);
            for _ in 0..count {
                self.call_entry("eml_rt_decref", &[word]);
            }
        }
        for &token in point.tokens {
            let word = self.var(token);
            self.call_entry("eml_rt_drop_token", &[word]);
        }
        let function = self.function;
        let values: Vec<Value> = function
            .rets
            .iter()
            .map(|&(repr, _)| placeholder(&mut self.b, repr))
            .collect();
        self.b.ins().return_(&values);
    }
}

/// 中断した呼び出しの結果と、巻き戻すフレームが返す仮の値。呼ぶ側は中断を見て、これを使わない。
pub(super) fn placeholder(b: &mut FunctionBuilder, repr: Repr) -> Value {
    match repr {
        Repr::Float => b.ins().f64const(0.0),
        Repr::Obj | Repr::TObj | Repr::Unit => b.ins().iconst(types::I64, mem::UNIT),
        Repr::Int | Repr::Enum => b.ins().iconst(types::I64, 0),
        Repr::Token => b.ins().iconst(types::I64, mem::EMPTY_TOKEN),
        Repr::Struct(_) => unreachable!("flattening leaves no struct value"),
    }
}
