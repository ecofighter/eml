//! コード生成器のシグネチャとCraneliftのフラグ (docs/spec/native.md の「`eml_enter`」)。
use cranelift_codegen::ir::{AbiParam, Signature, types};
use cranelift_codegen::isa::{CallConv, OwnedTargetIsa};
use cranelift_codegen::settings::{self, Configurable};
use cranelift_jit::{ArenaMemoryProvider, JITBuilder};
use cranelift_module::{FuncId, Linkage, Module, ModuleError, default_libcall_names};
use eml_nrt::{Entry, Kind};

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

/// エントリをimportとして宣言する。importのリンケージを書くのはここだけである。
pub fn declare_entry<M: Module>(module: &mut M, entry: &Entry) -> Built<FuncId> {
    let sig = entry_signature(entry, module.isa().default_call_conv());
    Ok(module.declare_function(entry.symbol, Linkage::Import, &sig)?)
}
