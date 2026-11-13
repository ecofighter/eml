//! `CSignature` ごとの橋渡しの関数 (docs/spec/vm.md の「橋渡しの関数」)。橋渡しは `args` の語だけを読み、`Ctx`、
//! ヒープ、スレッドローカル、記号の名前のどれにも触れない。だから pool のスレッドからもネイティブのコードからも呼べる。

#![allow(unsafe_code)]

use std::collections::HashMap;
use std::sync::Mutex;

use cranelift_codegen::ir::{AbiParam, InstBuilder, MemFlagsData, Signature, types};
use cranelift_codegen::settings::{self, Configurable};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{FuncId, Linkage, Module, default_libcall_names};
use eml_cabi::Bridge;
use eml_extern::{CSignature, CType};

use crate::error::LinkError;

/// 使い終えた module を、プロセスが終わるまで持つ。cranelift-jit が module を落とすときにコードを解放するかどうかに
/// 頼らず、返した `Bridge` がプロセスの間ずっと呼べるようにするためである。`ManuallyDrop` で手放すのでなく、ここに
/// 置いて届く所に残すのは、LeakSanitizer が届かなくなった確保を漏れとして報告するためである。
static RETIRED: Mutex<Vec<JITModule>> = Mutex::new(Vec::new());

fn retire(module: JITModule) {
    RETIRED
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .push(module);
}

/// 作った橋渡しの関数を署名で引く。module は落とさず、明示の解放の関数も呼ばない。`Bridges` を落とすときも、
/// 途中で失敗した module を捨てるときも、`RETIRED` に移す。`Linker` を落としても、作ったコードは残る。
pub struct Bridges {
    module: Option<JITModule>,
    made: HashMap<CSignature, Bridge>,
    next: u32,
}

impl Default for Bridges {
    fn default() -> Bridges {
        Bridges::new()
    }
}

impl Drop for Bridges {
    fn drop(&mut self) {
        if let Some(module) = self.module.take() {
            retire(module);
        }
    }
}

impl Bridges {
    /// C の関数のないプログラムが何も払わないよう、空でない最初の要求まで JIT を作らない。
    pub fn new() -> Bridges {
        Bridges {
            module: None,
            made: HashMap::new(),
            next: 0,
        }
    }

    /// 署名ごとの橋渡しを、入力に並びをそろえて返す。まだない署名をまとめて作り、finalize を1回だけ呼ぶ。
    pub fn get(&mut self, signatures: &[CSignature]) -> Result<Vec<Bridge>, LinkError> {
        let mut missing: Vec<&CSignature> = Vec::new();
        for signature in signatures {
            if !self.made.contains_key(signature) && !missing.contains(&signature) {
                missing.push(signature);
            }
        }
        if !missing.is_empty() {
            let module = match &mut self.module {
                Some(module) => module,
                none => none.insert(new_module()?),
            };
            let ids = match make(module, &missing, &mut self.next) {
                Ok(ids) => ids,
                Err(error) => {
                    // 途中まで定義した関数が次の finalize に混ざらないよう、この module を退かせて次の要求で作り直す。
                    // `RETIRED` に移すので、前に作った橋渡しのコードは残る
                    if let Some(module) = self.module.take() {
                        retire(module);
                    }
                    return Err(error);
                }
            };
            for (signature, id) in missing.into_iter().zip(ids) {
                let code = module.get_finalized_function(id);
                // SAFETY: `define` が `Bridge` と同じ署名 (ポインタ2つを取り i64 を返す、OS の既定の呼び出し規約) で
                // 作り、finalize を済ませた関数である
                let bridge = unsafe { std::mem::transmute::<*const u8, Bridge>(code) };
                self.made.insert(signature.clone(), bridge);
            }
        }
        Ok(signatures
            .iter()
            .map(|signature| self.made[signature])
            .collect())
    }
}

/// `missing` の橋渡しを定義して finalize し、それぞれの関数の番号を返す。
fn make(
    module: &mut JITModule,
    missing: &[&CSignature],
    next: &mut u32,
) -> Result<Vec<FuncId>, LinkError> {
    let mut ids = Vec::with_capacity(missing.len());
    for signature in missing {
        let name = format!("bridge{next}");
        *next += 1;
        ids.push(define(module, &name, signature)?);
    }
    module
        .finalize_definitions()
        .map_err(|_| unsupported("the bridge code could not be made executable"))?;
    Ok(ids)
}

/// 理由は自前の固定の文にする。Cranelift の誤りの文は OS の文言を含みうるためである
/// (docs/spec/vm.md の「ライブラリと記号」)。
fn unsupported(reason: &str) -> LinkError {
    LinkError::Unsupported {
        reason: reason.to_owned(),
    }
}

const NO_ISA: &str = "Cranelift does not support this machine";
const NO_COMPILE: &str = "a bridge could not be compiled";

fn new_module() -> Result<JITModule, LinkError> {
    let mut flags = settings::builder();
    flags
        .set("opt_level", "speed")
        .map_err(|_| unsupported(NO_ISA))?;
    flags
        .set("is_pic", "false")
        .map_err(|_| unsupported(NO_ISA))?;
    let isa = cranelift_native::builder()
        .map_err(|_| unsupported(NO_ISA))?
        .finish(settings::Flags::new(flags))
        .map_err(|_| unsupported(NO_ISA))?;
    Ok(JITModule::new(JITBuilder::with_isa(
        isa,
        default_libcall_names(),
    )))
}

fn clif(ty: CType) -> types::Type {
    match ty {
        CType::I32 => types::I32,
        CType::I64 => types::I64,
        CType::F64 => types::F64,
    }
}

/// `fn(callee, args) -> u64` の本体を作る。slot k は `args` の offset `8*k` の語である。
fn define(module: &mut JITModule, name: &str, signature: &CSignature) -> Result<FuncId, LinkError> {
    let call_conv = module.isa().default_call_conv();
    let pointer = module.isa().pointer_type();
    let mut bridge = Signature::new(call_conv);
    bridge.params.push(AbiParam::new(pointer));
    bridge.params.push(AbiParam::new(pointer));
    bridge.returns.push(AbiParam::new(types::I64));
    let mut callee_signature = Signature::new(call_conv);
    for &param in &signature.params {
        let abi = AbiParam::new(clif(param));
        // 呼び手が `int32_t` の引数を広げて渡すことを求める ABI がある。ほかの ABI でも広げて害はないので、いつも広げる
        callee_signature
            .params
            .push(if param == CType::I32 { abi.sext() } else { abi });
    }
    if let Some(ret) = signature.ret {
        callee_signature.returns.push(AbiParam::new(clif(ret)));
    }

    let id = module
        .declare_function(name, Linkage::Local, &bridge)
        .map_err(|_| unsupported(NO_COMPILE))?;
    let mut context = module.make_context();
    context.func.signature = bridge;
    let mut builder_context = FunctionBuilderContext::new();
    {
        let mut b = FunctionBuilder::new(&mut context.func, &mut builder_context);
        let entry = b.create_block();
        b.append_block_params_for_function_params(entry);
        b.switch_to_block(entry);
        b.seal_block(entry);
        let callee = b.block_params(entry)[0];
        let args = b.block_params(entry)[1];
        let callee_ref = b.import_signature(callee_signature);
        // 呼び手は slot の数だけの、8 で揃った語の列を渡す
        let trusted = MemFlagsData::trusted();
        let mut values = Vec::with_capacity(signature.params.len());
        for (k, &param) in signature.params.iter().enumerate() {
            let offset = i32::try_from(8 * k).expect("a C signature has few parameters");
            values.push(match param {
                CType::I64 => b.ins().load(types::I64, trusted, args, offset),
                // 呼び手に語の上の 32 bit の中身を求めない。下の 32 bit だけを使う
                CType::I32 => {
                    let word = b.ins().load(types::I64, trusted, args, offset);
                    b.ins().ireduce(types::I32, word)
                }
                CType::F64 => b.ins().load(types::F64, trusted, args, offset),
            });
        }
        let call = b.ins().call_indirect(callee_ref, callee, &values);
        let result = match signature.ret {
            None => b.ins().iconst(types::I64, 0),
            Some(CType::I64) => b.inst_results(call)[0],
            Some(CType::I32) => {
                let value = b.inst_results(call)[0];
                b.ins().sextend(types::I64, value)
            }
            Some(CType::F64) => {
                let value = b.inst_results(call)[0];
                b.ins().bitcast(types::I64, MemFlagsData::new(), value)
            }
        };
        b.ins().return_(&[result]);
        b.finalize(module.target_config());
    }
    module
        .define_function(id, &mut context)
        .map_err(|_| unsupported(NO_COMPILE))?;
    module.clear_context(&mut context);
    Ok(id)
}
