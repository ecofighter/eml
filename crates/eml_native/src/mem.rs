//! `eml_rt`の物体の配置を読む唯一のmodule (docs/spec/object-model.md の「語の位置」)。メモリに触るコード生成をここに置き、
//! 生成したコードが頼る配置の事実を1か所で`eml_rt`に結び付ける。
use cranelift_codegen::ir::{InstBuilder, MemFlagsData, Value, types};
use cranelift_frontend::FunctionBuilder;
use eml_lir::Shape;
use eml_rt::{CTX_FAULT, CTX_STACK_LIMIT, CTX_STOP, CTX_YIELD_TARGET, Word, layout};

// reuseは走査する語が`REUSE_SCANNED_LIMIT`個未満の形だけを組にし、ネイティブのコードはそれを数の語なしで`eml_rt`の上に
// 下ろすので、上限は`eml_rt`の上限以下でなければならない (docs/spec/object-model.md の「ペイロード」)
const _: () = assert!(Shape::REUSE_SCANNED_LIMIT as usize <= layout::INLINE_SCAN_LIMIT);

/// `n`の即値の語。呼ぶ側は、タグなど63ビットに収まる値を渡す。
pub const fn imm_bits(n: i64) -> i64 {
    (n << Word::IMM_SHIFT) | Word::IMM_TAG as i64
}

/// `()`の語。
pub const UNIT: i64 = imm_bits(0);

/// 空のtokenの語。
pub const EMPTY_TOKEN: i64 = 0;

/// `ctx`の`yielding.target`の語。yieldも中断もしていない間だけ`UNIT`である (docs/spec/native.md の「巻き戻しの地点」)。
pub fn yield_target(b: &mut FunctionBuilder, ctx: Value) -> Value {
    b.ins().load(
        types::I64,
        MemFlagsData::trusted(),
        ctx,
        CTX_YIELD_TARGET as i32,
    )
}

/// `ctx`のfaultの語。0でないのは、ちょうど失敗を置いている間である (docs/spec/native.md の「externの失敗」)。
pub fn fault(b: &mut FunctionBuilder, ctx: Value) -> Value {
    b.ins()
        .load(types::I64, MemFlagsData::trusted(), ctx, CTX_FAULT as i32)
}

/// `ctx`のworkerの止める語。0でなければ、止めるビットか知らせが立っている (docs/spec/native.md の「安全点」)。
pub fn stop_bits(b: &mut FunctionBuilder, ctx: Value) -> Value {
    let word = b
        .ins()
        .load(types::I64, MemFlagsData::trusted(), ctx, CTX_STOP as i32);
    // 語はほかのスレッドが`Relaxed`で書く`AtomicU32`である。そろった32ビットの普通のロードは、どの対象でも`Relaxed`の
    // ロードと同じ命令になり、入口ごとに1回しか読まないので、ロードをまとめられることもない
    b.ins().load(types::I32, MemFlagsData::trusted(), word, 0)
}

/// `ctx`の`stack_limit`。スタックポインタがこれより下なら、深さの上限を超えている (docs/spec/native.md の「安全点」)。
pub fn stack_limit(b: &mut FunctionBuilder, ctx: Value) -> Value {
    b.ins().load(
        types::I64,
        MemFlagsData::trusted(),
        ctx,
        CTX_STACK_LIMIT as i32,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_words_are_the_runtime_words() {
        assert_eq!(UNIT as usize, Word::UNIT.bits());
        assert_eq!(imm_bits(-5) as usize, Word::imm(-5).bits());
        assert_eq!(
            imm_bits(Word::MAX_INT) as usize,
            Word::imm(Word::MAX_INT).bits()
        );
        assert_eq!(EMPTY_TOKEN as usize, Word::EMPTY.bits());
    }
}
