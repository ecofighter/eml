//! `eml_rt`の物体の配置を読む唯一のmodule (docs/spec/object-model.md の「語の位置」)。メモリに触るコード生成をここに置き、
//! 生成したコードが頼る配置の事実を1か所で`eml_rt`に結び付ける。
use cranelift_codegen::ir::condcodes::IntCC;
use cranelift_codegen::ir::{
    GlobalValue, InstBuilder, MemFlagsData, StackSlotData, StackSlotKind, Type, Value, types,
};
use cranelift_frontend::FunctionBuilder;
use eml_lir::Shape;
use eml_rt::rc::{DECREF_FAST_BIAS, DECREF_FAST_SPAN, DUP_FAST_BIAS, DUP_FAST_SPAN};
use eml_rt::{
    CTX_FAULT, CTX_STACK_LIMIT, CTX_STOP, CTX_YIELD_ABORT, CTX_YIELD_TARGET, Kind, Meta, Word,
    layout,
};

// reuseは走査する語が`REUSE_SCANNED_LIMIT`個未満の形だけを組にし、ネイティブのコードはそれを数の語なしで`eml_rt`の上に
// 下ろすので、上限は`eml_rt`の上限以下でなければならない (docs/spec/object-model.md の「ペイロード」)
const _: () = assert!(Shape::REUSE_SCANNED_LIMIT as usize <= layout::INLINE_SCAN_LIMIT);

/// `n`の即値の語。呼ぶ側は、タグなど63ビットに収まる値を渡す。
pub const fn imm_bits(n: i64) -> i64 {
    (n << Word::IMM_SHIFT) | Word::IMM_TAG as i64
}

/// `n`が即値の語に収まるか。`Heap::alloc_int`が即値を選ぶ範囲である。
pub const fn fits_imm(n: i64) -> bool {
    Word::MIN_INT <= n && n <= Word::MAX_INT
}

/// `()`の語。
pub const UNIT: i64 = imm_bits(0);

/// 空のtokenの語。
pub const EMPTY_TOKEN: i64 = 0;

/// 語`w`が即値なら0でない。数えるReprの語は、即値か、物体のヘッダを指す。
pub fn is_imm(b: &mut FunctionBuilder, w: Value) -> Value {
    b.ins().band_imm_u(w, Word::IMM_TAG as i64)
}

/// 即値の語`w`の値。算術シフトで印のビットを落とす。
pub fn imm_value(b: &mut FunctionBuilder, w: Value) -> Value {
    b.ins().sshr_imm_u(w, i64::from(Word::IMM_SHIFT))
}

/// `v`の即値の語。呼ぶ側は、`v`が63ビットに収まることを確かめてある。
pub fn make_imm(b: &mut FunctionBuilder, v: Value) -> Value {
    let shifted = b.ins().ishl_imm_u(v, i64::from(Word::IMM_SHIFT));
    b.ins().bor_imm_u(shifted, Word::IMM_TAG as i64)
}

/// `v`が63ビットに収まる、つまり即値の語にして戻すと`v`になるとき0でない。`Heap::alloc_int`が即値を選ぶ規則と同じである
/// (docs/spec/object-model.md の「語の位置」)。
pub fn imm_round_trips(b: &mut FunctionBuilder, v: Value) -> Value {
    let shifted = b.ins().ishl_imm_u(v, i64::from(Word::IMM_SHIFT));
    let back = b.ins().sshr_imm_u(shifted, i64::from(Word::IMM_SHIFT));
    b.ins().icmp(IntCC::Equal, back, v)
}

/// 物体`obj`の`meta`の語 (`I32`)。
pub fn load_meta(b: &mut FunctionBuilder, obj: Value) -> Value {
    b.ins().load(
        types::I32,
        MemFlagsData::trusted(),
        obj,
        layout::META_BYTE as i32,
    )
}

/// `meta`の語のタグ (`I32`)。下の16ビットである。
pub fn meta_tag(b: &mut FunctionBuilder, meta: Value) -> Value {
    b.ins().band_imm_u(meta, i64::from(Meta::TAG_MASK))
}

/// 物体`obj`のタグ (`I32`)。
pub fn load_tag(b: &mut FunctionBuilder, obj: Value) -> Value {
    let meta = load_meta(b, obj);
    meta_tag(b, meta)
}

/// `meta`の語を持つ関数の値`clo`のエントリのコードのアドレス。コードは0番の走査する語の即値である。その位置は、走査する
/// 語が数の語を要するかで1語ずれ、適用する所は捕まえた値の数を知らないので、`meta`の`scan`のバイトで選ぶ
/// (docs/spec/native.md の「関数の値」)。
pub fn closure_code(b: &mut FunctionBuilder, clo: Value, meta: Value) -> Value {
    let short = layout::slot_byte_offset(layout::data_first(1, false));
    let long = layout::slot_byte_offset(layout::data_first(layout::INLINE_SCAN_LIMIT, false));
    let scan = b.ins().ushr_imm_u(meta, i64::from(Meta::SCAN_SHIFT));
    let scan = b.ins().band_imm_u(scan, i64::from(Meta::SCAN_MASK));
    let extended = b.ins().icmp_imm_u(
        IntCC::Equal,
        scan,
        i64::from(layout::data_scan(layout::INLINE_SCAN_LIMIT, false)),
    );
    let short = b.ins().iconst(types::I64, short as i64);
    let long = b.ins().iconst(types::I64, long as i64);
    let offset = b.ins().select(extended, long, short);
    let address = b.ins().iadd(clo, offset);
    let word = b
        .ins()
        .load(types::I64, MemFlagsData::trusted(), address, 0);
    imm_value(b, word)
}

/// `values`を順に並べたスタックの領域のアドレス。領域は今の関数のフレームにあり、関数が戻るまで生きる。
pub fn spill(b: &mut FunctionBuilder, values: &[Value]) -> Value {
    let bytes = u32::try_from(values.len() * layout::WORD_BYTES).expect("a spill fits in 4 GiB");
    let slot = b.create_sized_stack_slot(StackSlotData::new(
        StackSlotKind::ExplicitSlot,
        bytes,
        layout::WORD_BYTES.trailing_zeros() as u8,
    ));
    for (i, &value) in values.iter().enumerate() {
        let offset = i32::try_from(i * layout::WORD_BYTES).expect("a spill fits in 2 GiB");
        b.ins().stack_store(types::I64, value, slot, offset);
    }
    b.ins().stack_addr(types::I64, slot, 0)
}

/// 物体`obj`が`BigInt`なら0でない。ダミーは`BigInt`で、構造体の箱はいつも`Data`なので、構造体の箱を受ける所ではこれで
/// ダミーを見分ける (docs/spec/object-model.md の「ダミーの値」)。
pub fn is_big_int(b: &mut FunctionBuilder, obj: Value) -> Value {
    let meta = b.ins().load(
        types::I32,
        MemFlagsData::trusted(),
        obj,
        layout::META_BYTE as i32,
    );
    let kind = b
        .ins()
        .band_imm_u(meta, i64::from(Meta::KIND_MASK << Meta::KIND_SHIFT));
    b.ins().icmp_imm_u(
        IntCC::Equal,
        kind,
        i64::from((Kind::BigInt as u32) << Meta::KIND_SHIFT),
    )
}

/// 一意な箱`obj`のtoken。速いモードのtokenは箱を指す語なので、箱の語そのものである。`Heap::into_token`も速いモードでは
/// 何も書かない (docs/spec/object-model.md の「検査モードの token」)。
pub fn token_of_box(_b: &mut FunctionBuilder, obj: Value) -> Value {
    obj
}

/// 空でないtoken`tok`の箱。`token_of_box`の逆で、`Heap::reuse_in_place`と同じくヘッダに何も書かない
/// (docs/spec/object-model.md の「検査モードの token」)。
pub fn box_of_token(_b: &mut FunctionBuilder, tok: Value) -> Value {
    tok
}

/// 走査する語を`scanned`個持つ`Data`の、ペイロードの`position`番目の欄 (`Shape::positions`の位置) のバイトのずれ。
/// 走査する語が多い形は先頭に数の語を持つので、`eml_rt`の`data_first`の分だけずれる。
fn field_offset(scanned: u32, position: u32) -> i32 {
    let first = layout::data_first(scanned as usize, false);
    i32::try_from(layout::slot_byte_offset(first + position as usize))
        .expect("a field offset fits in 32 bits")
}

/// `Data`の物体`obj`の欄を型`ty`で読む。
pub fn load_field(
    b: &mut FunctionBuilder,
    obj: Value,
    scanned: u32,
    position: u32,
    ty: Type,
) -> Value {
    b.ins().load(
        ty,
        MemFlagsData::trusted(),
        obj,
        field_offset(scanned, position),
    )
}

/// `Data`の物体`obj`の欄に`v`を書く。
pub fn store_field(b: &mut FunctionBuilder, obj: Value, scanned: u32, position: u32, v: Value) {
    b.ins().store(
        MemFlagsData::trusted(),
        v,
        obj,
        field_offset(scanned, position),
    );
}

/// `BigInt`か`Float`の箱`obj`の値を型`ty`で読む。
pub fn load_box(b: &mut FunctionBuilder, obj: Value, ty: Type) -> Value {
    let offset = i32::try_from(layout::slot_byte_offset(layout::BOX_VALUE_WORD))
        .expect("the box value offset fits in 32 bits");
    b.ins().load(ty, MemFlagsData::trusted(), obj, offset)
}

/// 読み込み時の値のデータの記号`slot`が持つ語を型`ty`で読む。記号のアドレスはPC相対で、ロードは1つである
/// (docs/spec/native.md の「値と定数」)。
pub fn data_word(b: &mut FunctionBuilder, slot: GlobalValue, ty: Type) -> Value {
    let address = b.ins().symbol_value(types::I64, slot);
    b.ins().load(ty, MemFlagsData::trusted(), address, 0)
}

/// 物体`obj`の数。ヘッダの先頭の`I32`である。
pub fn load_rc(b: &mut FunctionBuilder, obj: Value) -> Value {
    b.ins().load(
        types::I32,
        MemFlagsData::trusted(),
        obj,
        layout::RC_BYTE as i32,
    )
}

/// 物体`obj`の数に`rc`を書く。
pub fn store_rc(b: &mut FunctionBuilder, obj: Value, rc: Value) {
    b.ins()
        .store(MemFlagsData::trusted(), rc, obj, layout::RC_BYTE as i32);
}

/// `dup`が`rc + 1`を書いてよいとき、つまり`rc`が`LOCAL_MAX`未満の局所の数のとき0でない。符号なしの比較1回で決める
/// (docs/spec/object-model.md の「局所の速い道」)。
pub fn dup_is_local(b: &mut FunctionBuilder, rc: Value) -> Value {
    in_span(b, rc, DUP_FAST_BIAS, DUP_FAST_SPAN)
}

/// `decref`が`rc - 1`を書いてよいとき、つまり`rc`が2以上`LOCAL_MAX`未満の局所の数のとき0でない。数が1なら物体を
/// 解放するので、遅い道を通る。
pub fn decref_is_local(b: &mut FunctionBuilder, rc: Value) -> Value {
    in_span(b, rc, DECREF_FAST_BIAS, DECREF_FAST_SPAN)
}

/// 数`rc`が1、つまり呼ぶ側の参照がただ1つの局所の参照なら0でない。共有、不死、張り付いた数は、どれもこの比較で外れる
/// (`Heap::is_unique`の規則)。
pub fn is_one_ref(b: &mut FunctionBuilder, rc: Value) -> Value {
    b.ins().icmp_imm_u(IntCC::Equal, rc, 1)
}

/// `(rc as u32).wrapping_sub(bias) < span`。`bias`より小さい数 (0と共有の負の数) は、引くと符号なしで大きくなるので、
/// 範囲の上の端と同じ比較で外れる。
fn in_span(b: &mut FunctionBuilder, rc: Value, bias: u32, span: u32) -> Value {
    let biased = b.ins().iadd_imm_s(rc, -i64::from(bias));
    b.ins()
        .icmp_imm_u(IntCC::UnsignedLessThan, biased, i64::from(span))
}

/// `ctx`の`yielding.target`の語。yieldも中断もしていない間は`UNIT`である。読むのは`eml$apply$m`の`n < m`の腕の
/// 見張りだけで、最後でない部分から戻るyieldを見つける (docs/spec/native.md の「関数の値」)。
pub fn yield_target(b: &mut FunctionBuilder, ctx: Value) -> Value {
    b.ins().load(
        types::I64,
        MemFlagsData::trusted(),
        ctx,
        CTX_YIELD_TARGET as i32,
    )
}

/// `ctx`のyieldの状態の`abort`のバイト。中断の途中の間だけ0でなく、yieldはそれを立てない (docs/spec/native.md の
/// 「巻き戻しの地点」)。
pub fn yield_abort(b: &mut FunctionBuilder, ctx: Value) -> Value {
    b.ins().load(
        types::I8,
        MemFlagsData::trusted(),
        ctx,
        CTX_YIELD_ABORT as i32,
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

/// スタックポインタが`ctx`の`stack_limit`より下なら0でない。スタックポインタは呼んだ関数が自分のフレームを作った後に
/// 読むので、比べる値はそのフレームを含む (docs/spec/native.md の「安全点」)。
pub fn below_stack_limit(b: &mut FunctionBuilder, ctx: Value) -> Value {
    let sp = b.ins().get_stack_pointer(types::I64);
    let limit = stack_limit(b, ctx);
    b.ins().icmp(IntCC::UnsignedLessThan, sp, limit)
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
