//! コード生成器のシグネチャとCraneliftのフラグ (docs/spec/native.md の「`eml_enter`」)。
use cranelift_codegen::ir::{AbiParam, Signature, types};
use cranelift_codegen::isa::{CallConv, OwnedTargetIsa};
use cranelift_codegen::settings::{self, Configurable};
use cranelift_jit::{ArenaMemoryProvider, JITBuilder};
use cranelift_module::{FuncId, Linkage, Module, ModuleError, default_libcall_names};
use eml_clink::c_signature;
use eml_extern::{CFunction, CHandleType};
use eml_lir::{LirFn, OpRet, Program};
use eml_nrt::{Entry, Kind};

use crate::lower::{self, names};

/// Craneliftのmoduleの誤り。大きいので箱に入れて返す。
pub type Built<T> = Result<T, Box<ModuleError>>;

/// emlのコード、adapter、補助の関数、thunkを出すISA。どれもこのフラグでコンパイルし、関数ごとの上書きはない。
/// `pic`はAOTだけが立てる。
pub fn isa(pic: bool) -> Option<OwnedTargetIsa> {
    let mut flags = settings::builder();
    flags.set("opt_level", "speed").ok()?;
    flags
        .set("is_pic", if pic { "true" } else { "false" })
        .ok()?;
    flags.set("preserve_frame_pointers", "true").ok()?;
    flags.set("enable_probestack", "true").ok()?;
    flags.set("probestack_strategy", "inline").ok()?;
    cranelift_native::builder()
        .ok()?
        .finish(settings::Flags::new(flags))
        .ok()
}

/// 1つのJITのmoduleのコードとデータをすべて置く領域の大きさ。aarch64の`bl`の届く範囲なので、moduleの中の呼び出しに
/// veneerは要らない。Windowsは予約の全体をコミットするので、これより大きくしない (docs/spec/native.md の「`eml_enter`」)。
pub const JIT_REGION_BYTES: usize = 128 << 20;

/// emlのコードのJITのmoduleのbuilder。`isa(false)`のISAと、`JIT_REGION_BYTES`の予約した領域1つを持つ。2つをここで一緒に
/// 作るので、`is_pic=false`のPC相対の参照は領域の中に収まる。機械のISAがないときと、予約が失敗したときは誤りを返す。
pub fn jit_builder() -> std::io::Result<JITBuilder> {
    let isa = isa(false).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "Cranelift does not support this machine",
        )
    })?;
    let region =
        ArenaMemoryProvider::new_with_size(JIT_REGION_BYTES).map_err(std::io::Error::other)?;
    let mut builder = JITBuilder::with_isa(isa, default_libcall_names());
    builder.memory_provider(Box::new(region));
    Ok(builder)
}

pub fn clif(kind: Kind) -> types::Type {
    match kind {
        Kind::I64 => types::I64,
        Kind::F64 => types::F64,
    }
}

/// emlの関数のシグネチャ`tail (ctx, 葉...) -> 葉...`。`tail`の規約を書くのはここだけである。
pub fn tail_signature(params: &[Kind], rets: &[Kind]) -> Signature {
    let mut sig = Signature::new(CallConv::Tail);
    sig.params.push(AbiParam::new(types::I64));
    sig.params
        .extend(params.iter().map(|&k| AbiParam::new(clif(k))));
    sig.returns
        .extend(rets.iter().map(|&k| AbiParam::new(clif(k))));
    sig
}

/// 最初の`captures`個の引数を捕まえた関数`function`の値のエントリのシグネチャ`tail (ctx, clo, 残りの引数..) -> 関数の
/// 結果..`。どの関数にも同じ規則で、値として使う関数では一様なシグネチャになり、節の関数ではその操作の
/// `clause_signature`に等しい (docs/spec/native.mdの「関数の値」)。
pub fn closure_entry_signature(function: &LirFn, captures: usize) -> Signature {
    let mut params = vec![Kind::I64];
    params.extend_from_slice(&lower::params(function)[captures..]);
    tail_signature(&params, &lower::rets(function))
}

/// エフェクト`effect`の操作`op`の節の呼び出しのシグネチャ`tail (ctx, clo, ev, 操作の引数..) -> 操作の結果`。
/// `never`の操作の節のエントリは`I64`の語を1つ返す (docs/spec/lir.mdの「`never` の操作」)。
pub fn clause_signature(program: &Program, effect: u32, op: u32) -> Signature {
    let operation = &program.effects[effect as usize].operations[op as usize];
    let mut params = vec![Kind::I64, Kind::I64];
    params.extend(operation.params.iter().map(|&repr| lower::kind(repr)));
    let ret = match operation.ret {
        OpRet::Repr(repr) => lower::kind(repr),
        OpRet::Never => Kind::I64,
    };
    tail_signature(&params, &[ret])
}

/// エントリのシグネチャ。記述子だけから作る (docs/spec/native.md の「記述子の表」)。
pub fn entry_signature(entry: &Entry, call_conv: CallConv) -> Signature {
    let mut sig = Signature::new(call_conv);
    sig.params.push(AbiParam::new(types::I64));
    sig.params
        .extend(entry.params.iter().map(|&k| AbiParam::new(clif(k))));
    sig.returns
        .extend(entry.result.map(|k| AbiParam::new(clif(k))));
    sig
}

/// moduleがimportとして宣言する関数。
pub enum Import<'a> {
    Entry(&'a Entry),
    C(&'a CFunction),
    Dtor(&'a CHandleType),
}

/// importを宣言する。importのリンケージを書くのはここだけである。Cの関数のシグネチャは`eml_clink::c_signature`が、
/// ハンドルのdtorのシグネチャは`(i64)`である (docs/spec/native.md の「Cの関数の呼び出し」)。
pub fn declare_import<M: Module>(module: &mut M, import: Import<'_>) -> Built<FuncId> {
    let call_conv = module.isa().default_call_conv();
    let (name, sig) = match import {
        Import::Entry(entry) => (entry.symbol.to_owned(), entry_signature(entry, call_conv)),
        Import::C(function) => (
            names::c_function(&function.name),
            c_signature(&function.c_signature(), call_conv),
        ),
        Import::Dtor(handle) => {
            let mut sig = Signature::new(call_conv);
            sig.params.push(AbiParam::new(types::I64));
            (names::c_dtor(&handle.name), sig)
        }
    };
    Ok(module.declare_function(&name, Linkage::Import, &sig)?)
}
