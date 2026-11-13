//! 利用者のCの関数の呼び出し (docs/spec/native.mdの「Cの関数の呼び出し」)。
use cranelift_codegen::ir::condcodes::IntCC;
use cranelift_codegen::ir::{Block, InstBuilder, Value, types};
use cranelift_module::{FuncId, Module};
use eml_extern::{CFnId, CInt, CParam, CRet, Purity};
use eml_lir::{Atom, BlockId, Loc, VarId};

use super::consts::Datum;
use super::function::FnLower;
use super::unwind::Point;
use crate::mem;

/// 範囲の確かめの後の、Cの関数の引数。
#[derive(Clone, Copy)]
enum CArg {
    /// そのまま渡すスカラー。`Float`、`Int`と、32ビットに狭めた`int32_t`の値である。
    Value(Value),
    /// 借りた物体。貸すエントリがその位置をCに渡す。
    Lent(Lend, Value),
}

/// 借りた物体をCに貸すエントリ。
#[derive(Clone, Copy)]
enum Lend {
    Text,
    Unboxed,
    UnboxedMut,
    Handle,
}

impl Lend {
    fn entry(self) -> &'static str {
        match self {
            Lend::Text => "eml_rt_text_payload",
            Lend::Unboxed => "eml_rt_unboxed_ptr",
            Lend::UnboxedMut => "eml_rt_unboxed_ptr_mut",
            Lend::Handle => "eml_x_c_handle_ptr",
        }
    }
}

/// 失敗を上げうる文の巻き戻し。文の冷たい道はどれも同じ位置で上げ、文の1つの後始末へ進む。
struct Unwind<'a> {
    site: u32,
    purity: Purity,
    cleanup: Block,
    point: Point<'a>,
    filled: bool,
}

impl<'a, M: Module> FnLower<'a, '_, M> {
    /// Cの関数`id`の文`let var = extern f(args)`。まず範囲の確かめをすべて済ませ、次に借りた物体を貸して関数を呼び、
    /// 最後に結果を値にする。
    pub(super) fn c_row(
        &mut self,
        block: BlockId,
        position: usize,
        var: VarId,
        id: CFnId,
        args: &[Atom],
        at: Option<Loc>,
    ) {
        let program = self.callees.program;
        let function = &program.c_functions[id.0 as usize];
        let row = function.row();
        let mut unwind = row.faults.then(|| Unwind {
            site: self.site(at),
            purity: row.purity,
            cleanup: self.cold_block(),
            point: Point::from(self.point(block, position)),
            filled: false,
        });
        let values: Vec<Value> = args
            .iter()
            .zip(row.params)
            .map(|(&arg, &repr)| self.atom(arg, repr))
            .collect();
        let mut c_args = Vec::with_capacity(values.len());
        for (k, (&param, &v)) in function.params.iter().zip(&values).enumerate() {
            let c_arg = match param {
                CParam::Int(CInt::I32) => {
                    let low = self.b.ins().ireduce(types::I32, v);
                    let back = self.b.ins().sextend(types::I64, low);
                    let out = self.b.ins().icmp(IntCC::NotEqual, back, v);
                    self.narrow(&mut unwind, &function.name, k, CInt::I32, v, out);
                    CArg::Value(low)
                }
                CParam::Int(CInt::SizeT) => {
                    let out = self.b.ins().icmp_imm_s(IntCC::SignedLessThan, v, 0);
                    self.narrow(&mut unwind, &function.name, k, CInt::SizeT, v, out);
                    CArg::Value(v)
                }
                CParam::Int(CInt::I64) | CParam::Float => CArg::Value(v),
                CParam::Text => CArg::Lent(Lend::Text, v),
                CParam::UnboxedArray => CArg::Lent(Lend::Unboxed, v),
                CParam::MutUnboxedArray => CArg::Lent(Lend::UnboxedMut, v),
                CParam::Handle(_) => CArg::Lent(Lend::Handle, v),
                CParam::NoArg => continue,
            };
            c_args.push(c_arg);
        }
        let result = self.lend_and_call(self.imports.c_functions[id.0 as usize], &c_args);
        let returned = || result.expect("a C function with a result returns it");
        let value = match function.ret {
            CRet::Void => self.b.ins().iconst(types::I64, mem::UNIT),
            CRet::Int(CInt::I32) => self.b.ins().sextend(types::I64, returned()),
            CRet::Int(CInt::I64 | CInt::SizeT) | CRet::Float => returned(),
            CRet::Handle(handle) => {
                let name = self.datum_word(Datum::String(function.name.clone()), types::I64);
                let dtor = match self.imports.c_dtors[handle.0 as usize] {
                    Some(dtor) => {
                        let dtor = self.func_ref(dtor);
                        self.b.ins().func_addr(types::I64, dtor)
                    }
                    None => self.b.ins().iconst(types::I64, 0),
                };
                let w = self
                    .call_entry("eml_x_c_handle_new", &[name, dtor, returned()])
                    .expect("a handle is a word");
                let fault = mem::fault(&mut self.b, self.ctx);
                let raise = self.cold_block();
                let hot = self.b.create_block();
                self.b.ins().brif(fault, raise, &[], hot, &[]);
                self.b.switch_to_block(raise);
                self.raise_to_cleanup(&mut unwind);
                self.b.switch_to_block(hot);
                w
            }
        };
        self.bind(var, value);
    }

    /// Cの関数`name`の引数`k` (0から数える) の範囲の確かめ。`out`が真なら、冷たい道で範囲のpanicを置いて上げる。
    /// builderは熱い続きに置いて戻る。
    fn narrow(
        &mut self,
        unwind: &mut Option<Unwind<'a>>,
        name: &str,
        k: usize,
        int: CInt,
        v: Value,
        out: Value,
    ) {
        let cold = self.cold_block();
        let hot = self.b.create_block();
        self.b.ins().brif(out, cold, &[], hot, &[]);
        self.b.switch_to_block(cold);
        let name = self.datum_word(Datum::String(name.to_string()), types::I64);
        // 引数はVMと同じく、引数のない関数のUnitの引数も含めて1から数える
        let arg = self.b.ins().iconst(types::I64, k as i64 + 1);
        let code = self.b.ins().iconst(types::I64, int.code());
        self.call_entry("eml_x_c_narrow", &[name, arg, code, v]);
        self.raise_to_cleanup(unwind);
        self.b.switch_to_block(hot);
    }

    /// 今のブロックで置いた失敗を上げ、文の後始末へ進む。後始末は初めて使うときに埋める。
    fn raise_to_cleanup(&mut self, unwind: &mut Option<Unwind<'a>>) {
        let unwind = unwind
            .as_mut()
            .expect("a C function that can raise has an unwind point");
        self.raise(unwind.site, unwind.purity, unwind.cleanup);
        if !std::mem::replace(&mut unwind.filled, true) {
            self.fill_cleanup(unwind.cleanup, &unwind.point);
        }
    }

    /// 借りた物体をCに貸し、Cの関数`callee`を呼ぶ。呼び手は範囲の確かめをすべて済ませてあるので、最初の貸すエントリから
    /// 呼び出しまでの間に冷たい道は出ない。本体は、貸すエントリの呼び出しと`callee`の呼び出しのほかに何も出さない。
    /// 貸すエントリは借りた物体をヒープから1度読むだけで、確保も解放もせず、参照の数も変えない。`eml_x_c_handle_ptr`は
    /// 中身が`CHandle`であることも確かめ、違えば内部の誤りを記録し、Cが走る前に致命的な誤りの道からrunを終える。間に安全点も巻き戻しの地点もないので、
    /// 貸した位置はどれもCが戻るまで有効である。文は物体の引数をどれも借りるので、呼び手の参照が呼び出しの間それを
    /// 生かし、`MutUnboxedArray`は一意でこのworkerのものである。これで、ポインタのエントリと`eml_x_c_handle_ptr`の
    /// 約束を守る。`eml_x_c_handle_ptr`は、ポインタをCに渡す者に、Cがそのポインタを使う間ハンドルを生かしておくことを
    /// 求める (docs/spec/native.mdの「Cの関数の呼び出し」、docs/spec/object-model.mdの「`unsafe` の核」のT16)。
    fn lend_and_call(&mut self, callee: FuncId, args: &[CArg]) -> Option<Value> {
        let mut values = Vec::with_capacity(args.len());
        for &arg in args {
            values.push(match arg {
                CArg::Value(v) => v,
                CArg::Lent(lend, w) => self
                    .call_entry(lend.entry(), &[w])
                    .expect("a lending entry returns a position"),
            });
        }
        let callee = self.func_ref(callee);
        let call = self.b.ins().call(callee, &values);
        self.b.inst_results(call).first().copied()
    }
}
