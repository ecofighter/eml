//! コード生成器のABIのテスト (docs/implementation/testing.md の「`eml_native` の検証」)。JITしたコードにホストのABIの
//! エントリのstubを通して入るので、Miriでは流さない。
#![cfg(not(miri))]
use std::sync::Arc;

use cranelift_codegen::ir::{InstBuilder, MemFlagsData, types};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_jit::JITModule;
use cranelift_module::{DataDescription, FuncId, Linkage, Module, ModuleError};
use eml_native::abi::{JIT_REGION_BYTES, declare_entry, jit_builder, tail_signature};
use eml_native::thunk::define_thunk;
use eml_nrt::Kind::{self, F64, I64};
use eml_nrt::{Entry, Fatal, Thunk, detached, enter};
use eml_rt::{Buffering, Control, Ctx, Fast, HostRef, Limits, StdHost};

eml_nrt::entries! {
    const ECHO;

    plain fn eml_t_echo_ii(ctx, a: i64, b: i64) {
        let out = unsafe { (*ctx).runtime() }.cast::<u64>();
        unsafe {
            *out = a as u64;
            *out.add(1) = b as u64;
        }
    }

    plain fn eml_t_echo_ii_i(ctx, a: i64, b: i64) -> i64 {
        let out = unsafe { (*ctx).runtime() }.cast::<u64>();
        unsafe {
            *out = a as u64;
            *out.add(1) = b as u64;
        }
        -2
    }

    plain fn eml_t_echo_iii_i(ctx, a: i64, b: i64, c: i64) -> i64 {
        let out = unsafe { (*ctx).runtime() }.cast::<u64>();
        unsafe {
            *out = a as u64;
            *out.add(1) = b as u64;
            *out.add(2) = c as u64;
        }
        -3
    }

    plain fn eml_t_echo_i(ctx, a: i64) {
        let out = unsafe { (*ctx).runtime() }.cast::<u64>();
        unsafe { *out = a as u64 };
    }

    plain fn eml_t_echo_i_i(ctx, a: i64) -> i64 {
        let out = unsafe { (*ctx).runtime() }.cast::<u64>();
        unsafe { *out = a as u64 };
        -1
    }

    plain fn eml_t_echo_f_i(ctx, a: f64) -> i64 {
        let out = unsafe { (*ctx).runtime() }.cast::<u64>();
        unsafe { *out = a.to_bits() };
        -1
    }

    plain fn eml_t_echo_f_f(ctx, a: f64) -> f64 {
        let out = unsafe { (*ctx).runtime() }.cast::<u64>();
        unsafe { *out = a.to_bits() };
        -1.5
    }

    plain fn eml_t_echo_i_f(ctx, a: i64) -> f64 {
        let out = unsafe { (*ctx).runtime() }.cast::<u64>();
        unsafe { *out = a as u64 };
        -1.25
    }

    plain fn eml_t_echo_if_i(ctx, a: i64, b: f64) -> i64 {
        let out = unsafe { (*ctx).runtime() }.cast::<u64>();
        unsafe {
            *out = a as u64;
            *out.add(1) = b.to_bits();
        }
        -3
    }

    plain fn eml_t_echo_ii_f(ctx, a: i64, b: i64) -> f64 {
        let out = unsafe { (*ctx).runtime() }.cast::<u64>();
        unsafe {
            *out = a as u64;
            *out.add(1) = b as u64;
        }
        -2.25
    }

    plain fn eml_t_echo_iif_i(ctx, a: i64, b: i64, c: f64) -> i64 {
        let out = unsafe { (*ctx).runtime() }.cast::<u64>();
        unsafe {
            *out = a as u64;
            *out.add(1) = b as u64;
            *out.add(2) = c.to_bits();
        }
        -3
    }

    plain fn eml_t_echo_ff_i(ctx, a: f64, b: f64) -> i64 {
        let out = unsafe { (*ctx).runtime() }.cast::<u64>();
        unsafe {
            *out = a.to_bits();
            *out.add(1) = b.to_bits();
        }
        -2
    }

    plain fn eml_t_echo_ff_f(ctx, a: f64, b: f64) -> f64 {
        let out = unsafe { (*ctx).runtime() }.cast::<u64>();
        unsafe {
            *out = a.to_bits();
            *out.add(1) = b.to_bits();
        }
        -2.5
    }

    plain fn eml_t_echo_iiii(ctx, a: i64, b: i64, c: i64, d: i64) {
        let out = unsafe { (*ctx).runtime() }.cast::<u64>();
        for (i, v) in [a, b, c, d].into_iter().enumerate() {
            unsafe { *out.add(i) = v as u64 };
        }
    }

    plain fn eml_t_echo_iiii_i(ctx, a: i64, b: i64, c: i64, d: i64) -> i64 {
        let out = unsafe { (*ctx).runtime() }.cast::<u64>();
        for (i, v) in [a, b, c, d].into_iter().enumerate() {
            unsafe { *out.add(i) = v as u64 };
        }
        -4
    }

    plain fn eml_t_echo_iiiii_i(ctx, a: i64, b: i64, c: i64, d: i64, e: i64) -> i64 {
        let out = unsafe { (*ctx).runtime() }.cast::<u64>();
        for (i, v) in [a, b, c, d, e].into_iter().enumerate() {
            unsafe { *out.add(i) = v as u64 };
        }
        -5
    }

    plain fn eml_t_echo_obj_tobj(ctx, a: obj) -> tobj {
        let out = unsafe { (*ctx).runtime() }.cast::<u64>();
        unsafe { *out = a.bits() as u64 };
        eml_rt::Word::imm(-7)
    }

    plain fn eml_t_echo6(ctx, a: i64, b: f64, c: i64, d: f64, e: i64, f: f64) -> f64 {
        let out = unsafe { (*ctx).runtime() }.cast::<u64>();
        for (i, bits) in [a as u64, b.to_bits(), c as u64, d.to_bits(), e as u64, f.to_bits()]
            .into_iter()
            .enumerate()
        {
            unsafe { *out.add(i) = bits };
        }
        -6.5
    }

    plain fn eml_t_echo9(
        ctx, a: f64, b: i64, c: f64, d: i64, e: f64, f: i64, g: f64, h: i64, i: f64,
    ) -> i64 {
        let out = unsafe { (*ctx).runtime() }.cast::<u64>();
        for (k, bits) in [
            a.to_bits(), b as u64, c.to_bits(), d as u64, e.to_bits(), f as u64, g.to_bits(),
            h as u64, i.to_bits(),
        ]
        .into_iter()
        .enumerate()
        {
            unsafe { *out.add(k) = bits };
        }
        -9
    }

    plain fn eml_t_echo18(
        ctx, a: i64, b: f64, c: i64, d: f64, e: i64, f: f64, g: i64, h: f64, i: i64, j: f64, k: i64,
        l: f64, m: i64, n: f64, o: i64, p: f64, q: i64, r: f64,
    ) -> f64 {
        let out = unsafe { (*ctx).runtime() }.cast::<u64>();
        for (x, bits) in [
            a as u64, b.to_bits(), c as u64, d.to_bits(), e as u64, f.to_bits(), g as u64,
            h.to_bits(), i as u64, j.to_bits(), k as u64, l.to_bits(), m as u64, n.to_bits(),
            o as u64, p.to_bits(), q as u64, r.to_bits(),
        ]
        .into_iter()
        .enumerate()
        {
            unsafe { *out.add(x) = bits };
        }
        -18.5
    }
}

fn ctx() -> Box<Ctx<Fast>> {
    let host = StdHost::new(
        Box::new(std::io::sink()),
        Box::new(std::io::sink()),
        std::path::PathBuf::new(),
        Buffering::Block,
    );
    let control = Arc::new(Control::new(Limits::default()));
    Box::new(Ctx::new(control, HostRef::new(Arc::new(host))))
}

fn module() -> JITModule {
    let mut builder = jit_builder().unwrap();
    for entry in eml_nrt::entries().chain(ECHO) {
        builder.symbol(entry.symbol, (entry.address)());
    }
    JITModule::new(builder)
}

/// `name(ctx, 葉...)`を宣言し、本体を`body`で作る。
fn define(
    m: &mut JITModule,
    name: &str,
    params: &[Kind],
    rets: &[Kind],
    body: impl FnOnce(&mut JITModule, &mut FunctionBuilder, FuncId),
) -> FuncId {
    let sig = tail_signature(params, rets);
    let id = m.declare_function(name, Linkage::Local, &sig).unwrap();
    let mut ctx = m.make_context();
    ctx.func.signature = sig;
    let mut fctx = FunctionBuilderContext::new();
    let mut b = FunctionBuilder::new(&mut ctx.func, &mut fctx);
    let block = b.create_block();
    b.append_block_params_for_function_params(block);
    b.switch_to_block(block);
    body(m, &mut b, id);
    b.seal_all_blocks();
    b.finalize(m.target_config());
    m.define_function(id, &mut ctx).unwrap();
    m.clear_context(&mut ctx);
    id
}

fn thunk(m: &JITModule, id: FuncId) -> Thunk {
    unsafe { std::mem::transmute::<*const u8, Thunk>(m.get_finalized_function(id)) }
}

/// finalizeしたmoduleの領域は、moduleを落としても解放されない。Windowsは予約した領域の全体をコミットするので、
/// テストの終わりに明示して解放する。
fn release(m: JITModule) {
    unsafe { m.free_memory() };
}

fn params(b: &FunctionBuilder) -> Vec<cranelift_codegen::ir::Value> {
    let block = b.current_block().unwrap();
    b.block_params(block).to_vec()
}

/// `thunk`を通して`entry`に入り、`argv`を結果で書き換える。
fn run(ctx: &mut Ctx<Fast>, m: &JITModule, thunk_id: FuncId, entry: FuncId, argv: &mut [u64]) {
    let r = unsafe {
        enter(
            ctx,
            thunk(m, thunk_id),
            m.get_finalized_function(entry),
            argv.as_mut_ptr(),
        )
    };
    assert_eq!(r, Ok(()));
    assert!(detached(ctx));
}

/// 記述子から作った呼び出しが、macroで宣言したRustのエントリに引数を1つずつ渡し、結果を受け取る。
#[test]
fn mixed_int_and_float_arguments_reach_an_entry() {
    for entry in eml_nrt::entries() {
        assert!(
            ECHO.iter()
                .any(|echo| echo.params == entry.params && echo.result == entry.result),
            "no echo entry has the kinds of {}",
            entry.symbol
        );
    }
    let mut m = module();
    let mut callers = Vec::new();
    for entry in ECHO {
        let rets: Vec<Kind> = entry.result.into_iter().collect();
        let callee = declare_entry(&mut m, entry).unwrap();
        let name = format!("call${}", entry.symbol);
        let id = define(&mut m, &name, entry.params, &rets, |m, b, _| {
            let args = params(b);
            let callee = m.declare_func_in_func(callee, b.func);
            let call = b.ins().call(callee, &args);
            let results = b.inst_results(call).to_vec();
            b.ins().return_(&results);
        });
        let th = define_thunk(
            &mut m,
            &format!("thunk${}", entry.symbol),
            entry.params,
            &rets,
        )
        .unwrap()
        .0;
        callers.push((entry, id, th));
    }
    m.finalize_definitions().unwrap();
    let mut ctx = ctx();
    for (entry, id, th) in callers {
        let n = entry.params.len();
        let mut argv: Vec<u64> = entry
            .params
            .iter()
            .enumerate()
            .map(|(i, kind)| match kind {
                I64 => 0x0101_0000_0000_0000u64.wrapping_mul(i as u64 + 1) + i as u64,
                F64 => (i as f64 + 0.25).to_bits(),
            })
            .collect();
        let sent = argv.clone();
        let mut seen = vec![0u64; n];
        ctx.set_runtime(seen.as_mut_ptr().cast());
        let r = unsafe {
            enter(
                &mut *ctx,
                thunk(&m, th),
                m.get_finalized_function(id),
                argv.as_mut_ptr(),
            )
        };
        ctx.set_runtime(std::ptr::null_mut());
        assert_eq!(r, Ok(()), "{}", entry.symbol);
        assert_eq!(seen, sent, "{}", entry.symbol);
        match entry.symbol {
            "eml_t_echo_ii_i" | "eml_t_echo_ff_i" => assert_eq!(argv[0] as i64, -2),
            "eml_t_echo_f_f" => assert_eq!(f64::from_bits(argv[0]), -1.5),
            "eml_t_echo_i_f" => assert_eq!(f64::from_bits(argv[0]), -1.25),
            "eml_t_echo_if_i" => assert_eq!(argv[0] as i64, -3),
            "eml_t_echo_ii_f" => assert_eq!(f64::from_bits(argv[0]), -2.25),
            "eml_t_echo_iif_i" => assert_eq!(argv[0] as i64, -3),
            "eml_t_echo_iii_i" => assert_eq!(argv[0] as i64, -3),
            "eml_t_echo_obj_tobj" => assert_eq!(
                eml_rt::Word::from_bits(argv[0] as usize),
                eml_rt::Word::imm(-7)
            ),
            "eml_t_echo_i_i" | "eml_t_echo_f_i" => assert_eq!(argv[0] as i64, -1),
            "eml_t_echo_ff_f" => assert_eq!(f64::from_bits(argv[0]), -2.5),
            "eml_t_echo_iiii_i" => assert_eq!(argv[0] as i64, -4),
            "eml_t_echo_iiiii_i" => assert_eq!(argv[0] as i64, -5),
            "eml_t_echo6" => assert_eq!(f64::from_bits(argv[0]), -6.5),
            "eml_t_echo9" => assert_eq!(argv[0] as i64, -9),
            "eml_t_echo18" => assert_eq!(f64::from_bits(argv[0]), -18.5),
            _ => {}
        }
        assert!(detached(&ctx));
    }
    release(m);
}

/// 自分への`return_call`はスタックを伸ばさず、スタック渡しの引数が多い関数への`return_call`と、複数の値の戻りが通る。
#[test]
fn tail_calls_and_multiple_results_run() {
    let mut m = module();
    let wide = define(&mut m, "wide", &[I64; 11], &[I64, F64, I64], |_, b, _| {
        let p = params(b);
        let mut sum = p[1];
        for x in &p[2..] {
            sum = b.ins().iadd(sum, *x);
        }
        let half = b.ins().f64const(0.5);
        let one = b.ins().iconst(types::I64, 1);
        b.ins().return_(&[sum, half, one]);
    });
    let count = define(
        &mut m,
        "count",
        &[I64, I64],
        &[I64, F64, I64],
        |m, b, me| {
            let p = params(b);
            let (c, n, acc) = (p[0], p[1], p[2]);
            let done = b.create_block();
            let more = b.create_block();
            b.ins().brif(n, more, &[], done, &[]);
            b.switch_to_block(more);
            let n1 = b.ins().iadd_imm_s(n, -1);
            let a1 = b.ins().iadd(acc, n);
            let me = m.declare_func_in_func(me, b.func);
            b.ins().return_call(me, &[c, n1, a1]);
            b.switch_to_block(done);
            let wide = m.declare_func_in_func(wide, b.func);
            let mut args = vec![c, acc];
            for i in 0..10 {
                args.push(b.ins().iconst(types::I64, i));
            }
            b.ins().return_call(wide, &args);
        },
    );
    let th = define_thunk(&mut m, "thunk$count", &[I64, I64], &[I64, F64, I64])
        .unwrap()
        .0;
    m.finalize_definitions().unwrap();
    let mut ctx = ctx();
    let n = 10_000_000u64;
    let mut argv = [n, 0, 0];
    run(&mut ctx, &m, th, count, &mut argv);
    assert_eq!(argv[0], n * (n + 1) / 2 + 45);
    assert_eq!(f64::from_bits(argv[1]), 0.5);
    assert_eq!(argv[2], 1);
    release(m);
}

/// ネイティブのコード生成器が頼るCraneliftの機能が、どの対象でも動く。localなデータの記号の読み、関数のアドレスを通した
/// `return_call_indirect`、あふれの印を返す演算、スタックポインタの読み、1ページより大きいフレームのinlineのstack probeである。
#[test]
fn the_code_generator_features_run() {
    const WORD: u64 = 0x0123_4567_89ab_cdef;
    const BIG_FRAME: u32 = 64 << 10;
    let mut m = module();

    let data = m
        .declare_data("word", Linkage::Local, false, false)
        .unwrap();
    let mut description = DataDescription::new();
    description.define(WORD.to_ne_bytes().into());
    description.set_align(8);
    m.define_data(data, &description).unwrap();
    let read = define(&mut m, "read", &[], &[I64], |m, b, _| {
        let gv = m.declare_data_in_func(data, b.func);
        let address = b.ins().symbol_value(types::I64, gv);
        let word = b
            .ins()
            .load(types::I64, MemFlagsData::trusted(), address, 0);
        b.ins().return_(&[word]);
    });

    let triple = define(&mut m, "triple", &[I64, I64], &[I64, I64], |_, b, _| {
        let p = params(b);
        let r = b.ins().imul_imm_s(p[1], 3);
        b.ins().return_(&[r, p[2]]);
    });
    // `bounce(ctx, n, x, sp0)`は、nが0でなければ自分のアドレスを通して`bounce(ctx, n - 1, x + 1, sp0)`へ、0なら
    // `triple(ctx, x, sp0 - sp)`へ末尾で跳ぶ。sp0は最初の`bounce`のスタックポインタで、末尾呼び出しがスタックを伸ばさなければ
    // いちばん深い`bounce`のものと等しい
    let bounce = define(
        &mut m,
        "bounce",
        &[I64, I64, I64],
        &[I64, I64],
        |m, b, me| {
            let p = params(b);
            let (c, n, x, given) = (p[0], p[1], p[2], p[3]);
            let sp = b.ins().get_stack_pointer(types::I64);
            let sp0 = b.ins().select(given, given, sp);
            let done = b.create_block();
            let more = b.create_block();
            b.ins().brif(n, more, &[], done, &[]);
            b.switch_to_block(more);
            let me = m.declare_func_in_func(me, b.func);
            let me = b.ins().func_addr(types::I64, me);
            let n1 = b.ins().iadd_imm_s(n, -1);
            let x1 = b.ins().iadd_imm_s(x, 1);
            let sig = b.import_signature(tail_signature(&[I64, I64, I64], &[I64, I64]));
            b.ins().return_call_indirect(sig, me, &[c, n1, x1, sp0]);
            b.switch_to_block(done);
            let grown = b.ins().isub(sp0, sp);
            let triple = m.declare_func_in_func(triple, b.func);
            let triple = b.ins().func_addr(types::I64, triple);
            let sig = b.import_signature(tail_signature(&[I64, I64], &[I64, I64]));
            b.ins().return_call_indirect(sig, triple, &[c, x, grown]);
        },
    );

    let overflow = define(&mut m, "overflow", &[I64, I64], &[I64; 6], |_, b, _| {
        let p = params(b);
        let (x, y) = (p[1], p[2]);
        let mut results = Vec::new();
        for (value, flag) in [
            b.ins().sadd_overflow(x, y),
            b.ins().ssub_overflow(x, y),
            b.ins().smul_overflow(x, y),
        ] {
            results.push(value);
            results.push(b.ins().uextend(types::I64, flag));
        }
        b.ins().return_(&results);
    });

    // 返すのは、スタックポインタと、スタックの枠に置いた局所の変数のアドレスである
    let sp = define(&mut m, "sp", &[], &[I64, I64], |_, b, _| {
        let slot = b.create_sized_stack_slot(cranelift_codegen::ir::StackSlotData::new(
            cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
            8,
            3,
        ));
        let local = b.ins().stack_addr(types::I64, slot, 0);
        let sp = b.ins().get_stack_pointer(types::I64);
        b.ins().return_(&[sp, local]);
    });

    // 64 KiBの枠の両端に触れ、低い端から先に書く。枠が1ページより大きいので、Craneliftはinlineのstack probeを出す
    let big = define(&mut m, "big", &[], &[I64], |_, b, _| {
        let slot = b.create_sized_stack_slot(cranelift_codegen::ir::StackSlotData::new(
            cranelift_codegen::ir::StackSlotKind::ExplicitSlot,
            BIG_FRAME,
            3,
        ));
        let base = b.ins().stack_addr(types::I64, slot, 0);
        let flags = MemFlagsData::trusted();
        let one = b.ins().iconst(types::I64, 1);
        let two = b.ins().iconst(types::I64, 2);
        b.ins().store(flags, one, base, 0);
        b.ins().store(flags, two, base, BIG_FRAME as i32 - 8);
        let low = b.ins().load(types::I64, flags, base, 0);
        let high = b.ins().load(types::I64, flags, base, BIG_FRAME as i32 - 8);
        let sum = b.ins().iadd(low, high);
        b.ins().return_(&[sum]);
    });

    let thunk_read = define_thunk(&mut m, "thunk$read", &[], &[I64]).unwrap().0;
    let thunk_bounce = define_thunk(&mut m, "thunk$bounce", &[I64, I64, I64], &[I64, I64])
        .unwrap()
        .0;
    let thunk_big = define_thunk(&mut m, "thunk$big", &[], &[I64]).unwrap().0;
    let thunk_overflow = define_thunk(&mut m, "thunk$overflow", &[I64, I64], &[I64; 6])
        .unwrap()
        .0;
    let thunk_sp = define_thunk(&mut m, "thunk$sp", &[], &[I64, I64])
        .unwrap()
        .0;
    m.finalize_definitions().unwrap();

    // `big`は、まだ何も深く触れていない小さいスタックの新しいスレッドで最初に流す。Windowsはスタックをガードページ1枚ずつ
    // コミットするので、probeがなければ枠の低い端への書き込みがガードページを飛び越えて落ちる
    let (thunk_big, big) = (thunk(&m, thunk_big), m.get_finalized_function(big) as usize);
    let sum = std::thread::Builder::new()
        .stack_size(4 * BIG_FRAME as usize)
        .spawn(move || {
            let mut ctx = ctx();
            let mut argv = [0u64];
            let r = unsafe { enter(&mut *ctx, thunk_big, big as *const u8, argv.as_mut_ptr()) };
            assert_eq!(r, Ok(()));
            argv[0]
        })
        .unwrap()
        .join()
        .unwrap();
    assert_eq!(sum, 3);

    let mut ctx = ctx();
    let mut argv = [0u64];
    run(&mut ctx, &m, thunk_read, read, &mut argv);
    assert_eq!(argv[0], WORD);
    let code = m.get_finalized_function(read) as usize;
    let word = m.get_finalized_data(data).0 as usize;
    assert!(code.abs_diff(word) < JIT_REGION_BYTES);

    let mut argv = [100_000u64, 5, 0];
    run(&mut ctx, &m, thunk_bounce, bounce, &mut argv);
    assert_eq!(argv[0], 3 * 100_005);
    assert_eq!(argv[1], 0, "return_call_indirect grew the stack");

    let edges = [i64::MAX, i64::MIN, 0, 1, -1];
    for x in edges {
        for y in edges {
            let mut argv = [x as u64, y as u64, 0, 0, 0, 0];
            run(&mut ctx, &m, thunk_overflow, overflow, &mut argv);
            let seen: Vec<(i64, bool)> = argv
                .chunks(2)
                .map(|pair| (pair[0] as i64, pair[1] != 0))
                .collect();
            let expected = vec![
                x.overflowing_add(y),
                x.overflowing_sub(y),
                x.overflowing_mul(y),
            ];
            assert_eq!(seen, expected, "{x} {y}");
        }
    }

    // RustのフレームのアドレスとSPは比べない。AddressSanitizerはRustの局所変数を
    // 偽のスタックに移すので、その差は本当のスタックの深さを表さない
    let mut argv = [0u64, 0];
    run(&mut ctx, &m, thunk_sp, sp, &mut argv);
    let (sp, local) = (argv[0], argv[1]);
    assert!(
        sp <= local && local - sp < 4096,
        "sp {sp:#x}, local {local:#x}"
    );
    release(m);
}

/// 定義してfinalizeする2つ目の回のコードが、1つ目の回のデータを読み、1つ目の回の関数を呼べる。REPLが同じmoduleに足していく形である。
#[test]
fn a_second_round_reaches_the_first() {
    let mut m = module();
    let data = m
        .declare_data("forty", Linkage::Local, false, false)
        .unwrap();
    let mut description = DataDescription::new();
    description.define(40u64.to_ne_bytes().into());
    description.set_align(8);
    m.define_data(data, &description).unwrap();
    let two = define(&mut m, "two", &[], &[I64], |_, b, _| {
        let r = b.ins().iconst(types::I64, 2);
        b.ins().return_(&[r]);
    });
    m.finalize_definitions().unwrap();

    let sum = define(&mut m, "sum", &[], &[I64], |m, b, _| {
        let c = params(b)[0];
        let gv = m.declare_data_in_func(data, b.func);
        let address = b.ins().symbol_value(types::I64, gv);
        let word = b
            .ins()
            .load(types::I64, MemFlagsData::trusted(), address, 0);
        let two = m.declare_func_in_func(two, b.func);
        let call = b.ins().call(two, &[c]);
        let r = b.inst_results(call)[0];
        let r = b.ins().iadd(word, r);
        b.ins().return_(&[r]);
    });
    let th = define_thunk(&mut m, "thunk$sum", &[], &[I64]).unwrap().0;
    m.finalize_definitions().unwrap();
    let mut ctx = ctx();
    let mut argv = [0u64];
    run(&mut ctx, &m, th, sum, &mut argv);
    assert_eq!(argv[0], 42);
    release(m);
}

/// `jit_builder`のmoduleは`JIT_REGION_BYTES`の領域に収まらない定義を`ModuleError::Allocation`で断り、panicしない。
/// 既定のmemory providerなら定義できてしまうので、`jit_builder`が領域を置かなくなるとこのテストが落ちる。
#[test]
fn a_full_region_is_an_error() {
    let mut m = module();
    let data = m
        .declare_data("too_big", Linkage::Local, false, false)
        .unwrap();
    let mut description = DataDescription::new();
    description.define_zeroinit(JIT_REGION_BYTES + 1);
    let r = m.define_data(data, &description);
    assert!(matches!(r, Err(ModuleError::Allocation { .. })), "{r:?}");
}

mod fatal {
    use super::*;

    /// `tail`のフレームをn枚積んでから`eml_fatal`を呼ぶと、`eml_enter`から誤りが返る。
    #[test]
    fn a_fatal_cuts_through_tail_frames() {
        let mut m = module();
        let fatal_entry: &Entry = eml_nrt::entries()
            .find(|e| e.symbol == "eml_fatal")
            .unwrap();
        let fatal = declare_entry(&mut m, fatal_entry).unwrap();
        let deep = define(&mut m, "deep", &[I64], &[I64], |m, b, me| {
            let p = params(b);
            let (c, n) = (p[0], p[1]);
            let zero = b.create_block();
            let more = b.create_block();
            b.ins().brif(n, more, &[], zero, &[]);
            b.switch_to_block(zero);
            let fatal = m.declare_func_in_func(fatal, b.func);
            let code = b.ins().iconst(types::I64, 7);
            let site = b.ins().iconst(types::I64, 9);
            b.ins().call(fatal, &[c, code, site]);
            b.ins()
                .trap(cranelift_codegen::ir::TrapCode::unwrap_user(1));
            b.switch_to_block(more);
            let n1 = b.ins().iadd_imm_s(n, -1);
            let me = m.declare_func_in_func(me, b.func);
            let call = b.ins().call(me, &[c, n1]);
            let r = b.inst_results(call)[0];
            let r1 = b.ins().iadd_imm_s(r, 1);
            b.ins().return_(&[r1]);
        });
        let th = define_thunk(&mut m, "thunk$deep", &[I64], &[I64])
            .unwrap()
            .0;
        m.finalize_definitions().unwrap();
        let mut ctx = ctx();
        for depth in [0u64, 1, 100] {
            let mut argv = [depth];
            let r = unsafe {
                enter(
                    &mut *ctx,
                    thunk(&m, th),
                    m.get_finalized_function(deep),
                    argv.as_mut_ptr(),
                )
            };
            assert_eq!(r, Err(Fatal { code: 7, site: 9 }));
            assert!(detached(&ctx));
        }
        release(m);
    }
}

extern "C" fn import_dtor(_: *mut std::ffi::c_void) {}

extern "C" fn import_wide() -> i64 {
    0x1_FFFF_FFFB
}

extern "C" fn import_add(a: i32, b: f64, c: i32) -> i32 {
    a + (b * 4.0) as i32 + c
}

#[allow(clippy::too_many_arguments)]
extern "C" fn import_sum12(
    a: i32,
    b: i64,
    c: i32,
    d: usize,
    e0: i32,
    e1: i32,
    e2: i32,
    e3: i32,
    e4: i32,
    e5: i32,
    f: f64,
    g: f64,
) -> i64 {
    i64::from(a)
        + 2 * b
        + 3 * i64::from(c)
        + 4 * d as i64
        + 5 * i64::from(e0)
        + 6 * i64::from(e1)
        + 7 * i64::from(e2)
        + 8 * i64::from(e3)
        + 9 * i64::from(e4)
        + 10 * i64::from(e5)
        + 11 * f as i64
        + 12 * g as i64
}

/// `JITBuilder::symbol`に登録した`Linkage::Import`の関数は、`func_addr`でその記号のアドレスを与え、`I32`に`ArgumentExtension`を付けない
/// 直接の`call`はCのABIに従う。Cの関数を呼ぶ下ろしがこの2つに頼るので、どの対象でも成り立つことを確かめる。
#[test]
fn an_import_gives_its_address() {
    use cranelift_codegen::ir::{AbiParam, Signature};

    let mut builder = jit_builder().unwrap();
    let dtor_fn: extern "C" fn(*mut std::ffi::c_void) = import_dtor;
    builder.symbol("eml_cd$M.H", dtor_fn as *const u8);
    builder.symbol(
        "eml_c$M.add",
        import_add as extern "C" fn(i32, f64, i32) -> i32 as *const u8,
    );
    builder.symbol(
        "eml_c$M.wide",
        import_wide as extern "C" fn() -> i64 as *const u8,
    );
    builder.symbol(
        "eml_c$M.sum12",
        import_sum12
            as extern "C" fn(i32, i64, i32, usize, i32, i32, i32, i32, i32, i32, f64, f64) -> i64
            as *const u8,
    );
    let mut m = JITModule::new(builder);

    let c_sig = |m: &JITModule, params: &[cranelift_codegen::ir::Type], ret| {
        let mut sig: Signature = m.make_signature();
        sig.params.extend(params.iter().map(|&t| AbiParam::new(t)));
        sig.returns.push(AbiParam::new(ret));
        sig
    };
    // 呼び出し側の関数。引数と結果の`i32`は`i64`で受け渡し、importとの境目の拡張は`ArgumentExtension`に頼らず生成するコードが自分で行う
    let wrapper = |m: &mut JITModule,
                   name: &str,
                   params: &[cranelift_codegen::ir::Type],
                   ret: cranelift_codegen::ir::Type,
                   body: &dyn Fn(&mut JITModule, &mut FunctionBuilder)| {
        let mut sig = m.make_signature();
        sig.params.extend(
            params
                .iter()
                .map(|&t| AbiParam::new(if t == types::I32 { types::I64 } else { t })),
        );
        sig.returns.push(AbiParam::new(if ret == types::I32 {
            types::I64
        } else {
            ret
        }));
        let id = m.declare_function(name, Linkage::Local, &sig).unwrap();
        let mut ctx = m.make_context();
        ctx.func.signature = sig;
        let mut fctx = FunctionBuilderContext::new();
        let mut b = FunctionBuilder::new(&mut ctx.func, &mut fctx);
        let block = b.create_block();
        b.append_block_params_for_function_params(block);
        b.switch_to_block(block);
        body(m, &mut b);
        b.seal_all_blocks();
        b.finalize(m.target_config());
        m.define_function(id, &mut ctx).unwrap();
        m.clear_context(&mut ctx);
        id
    };

    let void_ptr_sig = {
        let mut sig = m.make_signature();
        sig.params.push(AbiParam::new(types::I64));
        sig
    };
    let dtor_id = m
        .declare_function("eml_cd$M.H", Linkage::Import, &void_ptr_sig)
        .unwrap();
    let add_params = [types::I32, types::F64, types::I32];
    let add_id = m
        .declare_function(
            "eml_c$M.add",
            Linkage::Import,
            &c_sig(&m, &add_params, types::I32),
        )
        .unwrap();
    let sum_params = [
        types::I32,
        types::I64,
        types::I32,
        types::I64,
        types::I32,
        types::I32,
        types::I32,
        types::I32,
        types::I32,
        types::I32,
        types::F64,
        types::F64,
    ];
    let sum_id = m
        .declare_function(
            "eml_c$M.sum12",
            Linkage::Import,
            &c_sig(&m, &sum_params, types::I64),
        )
        .unwrap();

    // プローブは下位32bitが-5で、上位32bitが符号ビットの並びではないi64を返す。`I32`の結果としてimportすると下位32bitだけが意味を持つ。
    // `ArgumentExtension::Sext`はどの対象でも同じように扱われるわけではないので、呼び出し側が`sextend`で結果を明示的に拡張する
    let wide_id = m
        .declare_function("eml_c$M.wide", Linkage::Import, &c_sig(&m, &[], types::I32))
        .unwrap();

    let address = wrapper(&mut m, "address", &[], types::I64, &|m, b| {
        let f = m.declare_func_in_func(dtor_id, b.func);
        let a = b.ins().func_addr(types::I64, f);
        b.ins().return_(&[a]);
    });
    // `i32`の引数は`ireduce`で32bitにしてから渡し、結果は`sextend`で戻す。CのABIは`i32`の引数の上位bitを定めないので、そこは確かめない
    let narrow_and_call = |m: &mut JITModule,
                           b: &mut FunctionBuilder,
                           id: FuncId,
                           kinds: &[cranelift_codegen::ir::Type],
                           ret: cranelift_codegen::ir::Type| {
        let args: Vec<_> = kinds
            .iter()
            .zip(params(b))
            .map(|(&t, v)| {
                if t == types::I32 {
                    b.ins().ireduce(types::I32, v)
                } else {
                    v
                }
            })
            .collect();
        let f = m.declare_func_in_func(id, b.func);
        let call = b.ins().call(f, &args);
        let r = b.inst_results(call)[0];
        let r = if ret == types::I32 {
            b.ins().sextend(types::I64, r)
        } else {
            r
        };
        b.ins().return_(&[r]);
    };
    let add = wrapper(&mut m, "add", &add_params, types::I32, &|m, b| {
        narrow_and_call(m, b, add_id, &add_params, types::I32)
    });
    let sum = wrapper(&mut m, "sum12", &sum_params, types::I64, &|m, b| {
        narrow_and_call(m, b, sum_id, &sum_params, types::I64)
    });
    let wide = wrapper(&mut m, "wide", &[], types::I32, &|m, b| {
        narrow_and_call(m, b, wide_id, &[], types::I32)
    });
    m.finalize_definitions().unwrap();

    let address: extern "C" fn() -> usize =
        unsafe { std::mem::transmute(m.get_finalized_function(address)) };
    assert_eq!(address(), dtor_fn as usize);

    let add: extern "C" fn(i64, f64, i64) -> i64 =
        unsafe { std::mem::transmute(m.get_finalized_function(add)) };
    assert_eq!(add(-5, 0.5, -7), i64::from(import_add(-5, 0.5, -7)));
    assert_eq!(add(-5, 0.5, -7), -10);

    let sum: extern "C" fn(i64, i64, i64, usize, i64, i64, i64, i64, i64, i64, f64, f64) -> i64 =
        unsafe { std::mem::transmute(m.get_finalized_function(sum)) };
    let got = sum(-1, -2, -3, 4, -5, -6, -7, -8, -9, -10, 11.0, 12.0);
    let want = -1 - 4 - 9 + 16 - 25 - 36 - 49 - 64 - 81 - 100 + 121 + 144;
    assert_eq!(got, want);

    let wide: extern "C" fn() -> i64 =
        unsafe { std::mem::transmute(m.get_finalized_function(wide)) };
    assert_eq!(wide(), -5);
    release(m);
}
