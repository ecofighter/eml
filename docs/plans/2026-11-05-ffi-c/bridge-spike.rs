use cranelift_codegen::ir::{types, AbiParam, InstBuilder, MemFlagsData, Signature};
use cranelift_codegen::isa::CallConv;
use cranelift_codegen::settings::{self, Configurable};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{Linkage, Module};
use std::time::Instant;

type Bridge = unsafe extern "C" fn(*const (), *const u64) -> u64;

#[derive(Clone, Copy, PartialEq)]
enum CType { I32, I64, F64 }

fn clif(ct: CType) -> types::Type { match ct { CType::I32 => types::I32, CType::I64 => types::I64, CType::F64 => types::F64 } }

fn make_bridge(module: &mut JITModule, name: &str, params: &[CType], ret: Option<CType>) -> cranelift_module::FuncId {
    let cc = module.isa().default_call_conv();
    let ptr = module.isa().pointer_type();
    let mut bsig = Signature::new(cc);
    bsig.params.push(AbiParam::new(ptr));
    bsig.params.push(AbiParam::new(ptr));
    bsig.returns.push(AbiParam::new(types::I64));
    let mut csig = Signature::new(cc);
    for &p in params {
        let mut ap = AbiParam::new(clif(p));
        if p == CType::I32 { ap = ap.sext(); }
        csig.params.push(ap);
    }
    if let Some(r) = ret { csig.returns.push(AbiParam::new(clif(r))); }
    let id = module.declare_function(name, Linkage::Local, &bsig).unwrap();
    let mut ctx = module.make_context();
    ctx.func.signature = bsig;
    let mut fbc = FunctionBuilderContext::new();
    {
        let mut b = FunctionBuilder::new(&mut ctx.func, &mut fbc);
        let entry = b.create_block();
        b.append_block_params_for_function_params(entry);
        b.switch_to_block(entry);
        b.seal_block(entry);
        let callee = b.block_params(entry)[0];
        let args = b.block_params(entry)[1];
        let sigref = b.import_signature(csig);
        let tr = MemFlagsData::trusted();
        let plain = MemFlagsData::new();
        let mut vals = Vec::new();
        for (k, &p) in params.iter().enumerate() {
            let off = (8 * k) as i32;
            let v = match p {
                CType::I64 => b.ins().load(types::I64, tr, args, off),
                CType::I32 => { let w = b.ins().load(types::I64, tr, args, off); b.ins().ireduce(types::I32, w) }
                CType::F64 => b.ins().load(types::F64, tr, args, off),
            };
            vals.push(v);
        }
        let call = b.ins().call_indirect(sigref, callee, &vals);
        let r = match ret {
            None => b.ins().iconst(types::I64, 0),
            Some(CType::I64) => b.inst_results(call)[0],
            Some(CType::I32) => { let v = b.inst_results(call)[0]; b.ins().sextend(types::I64, v) }
            Some(CType::F64) => { let v = b.inst_results(call)[0]; b.ins().bitcast(types::I64, plain, v) }
        };
        b.ins().return_(&[r]);
        let cfg = module.target_config();
        b.finalize(cfg);
    }
    module.define_function(id, &mut ctx).unwrap();
    module.clear_context(&mut ctx);
    id
}

extern "C" fn add_i32(a: i32, b: i32) -> i32 { a.wrapping_add(b) }
extern "C" fn sum12(a: i32, b: i64, c: i32, d: usize, e: i32, f: i32, g: i32, h: i32, i: i32, j: i32, x: f64, y: f64) -> i64 {
    a as i64 + b + c as i64 + d as i64 + e as i64 + f as i64 + g as i64 + h as i64 + i as i64 + j as i64 + (x + y) as i64
}
extern "C" fn neg_ret() -> i32 { -7 }
extern "C" fn dot(a: *const f64, b: *const f64, n: i64) -> f64 { let (a, b) = unsafe { (std::slice::from_raw_parts(a, n as usize), std::slice::from_raw_parts(b, n as usize)) }; a.iter().zip(b).map(|(x, y)| x * y).sum() }

fn main() {
    let t0 = Instant::now();
    let mut flags = settings::builder();
    flags.set("opt_level", "speed").unwrap();
    flags.set("is_pic", "false").unwrap();
    let isa = cranelift_native::builder().unwrap().finish(settings::Flags::new(flags)).unwrap();
    let builder = JITBuilder::with_isa(isa, cranelift_module::default_libcall_names());
    let mut module = JITModule::new(builder);
    let t_isa = t0.elapsed();

    let f1 = make_bridge(&mut module, "b_i32_i32_i32", &[CType::I32, CType::I32], Some(CType::I32));
    let f2 = make_bridge(&mut module, "b_sum12", &[CType::I32, CType::I64, CType::I32, CType::I64, CType::I32, CType::I32, CType::I32, CType::I32, CType::I32, CType::I32, CType::F64, CType::F64], Some(CType::I64));
    module.finalize_definitions().unwrap();
    let t_first = t0.elapsed();
    let b1: Bridge = unsafe { std::mem::transmute(module.get_finalized_function(f1)) };
    let b2: Bridge = unsafe { std::mem::transmute(module.get_finalized_function(f2)) };
    // garbage upper bits: pass 0xFFFF_FFFF_0000_0003 as "3" (i32) to see sext/ireduce handling
    let args = [0xFFFF_FFFF_0000_0003u64, (-5i64) as u64];
    let r = unsafe { b1(add_i32 as *const (), args.as_ptr()) };
    println!("add_i32(3,-5) = {} (expect -2 as i64 = {})", r as i64, -2i64);
    let args = [1u64, 2, 3, 4, 5, 6, 7, 8, 9, 10, 1.5f64.to_bits(), 2.5f64.to_bits()];
    let r = unsafe { b2(sum12 as *const (), args.as_ptr()) };
    println!("sum12 = {} (expect 59)", r as i64);

    // second round of definitions after finalize (REPL appending)
    let f3 = make_bridge(&mut module, "b_void_i32", &[], Some(CType::I32));
    let f4 = make_bridge(&mut module, "b_dot", &[CType::I64, CType::I64, CType::I64], Some(CType::F64));
    module.finalize_definitions().unwrap();
    let b3: Bridge = unsafe { std::mem::transmute(module.get_finalized_function(f3)) };
    let b4: Bridge = unsafe { std::mem::transmute(module.get_finalized_function(f4)) };
    let r = unsafe { b3(neg_ret as *const (), std::ptr::null()) };
    println!("neg_ret = {} (expect -7)", r as i64);
    let a = [1.0f64, 2.0, 3.0]; let bv = [4.0f64, 5.0, 6.0];
    let args = [a.as_ptr() as u64, bv.as_ptr() as u64, 3];
    let r = unsafe { b4(dot as *const (), args.as_ptr()) };
    println!("dot = {} (expect 32)", f64::from_bits(r));
    // still callable after second finalize
    let args = [3u64, (-5i64) as u64];
    println!("add again = {}", unsafe { b1(add_i32 as *const (), args.as_ptr()) } as i64);

    // multi-thread calls of the same bridge
    let handles: Vec<_> = (0..8).map(|t| std::thread::spawn(move || {
        let mut acc = 0i64;
        for i in 0..100_000i64 { let args = [i as u64, t as u64]; acc += unsafe { b1(add_i32 as *const (), args.as_ptr()) } as i64; }
        acc
    })).collect();
    let sums: Vec<i64> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    println!("threads ok: {:?}", sums.iter().take(2).collect::<Vec<_>>());
    println!("isa {:?}, first finalize {:?}, total {:?}", t_isa, t_first, t0.elapsed());
}
