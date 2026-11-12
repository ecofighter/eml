//! LIRをCLIFに下ろす。どの関数を定義するよりも前にすべての関数を宣言するので、本体はプログラムのどの関数も名指せる
//! (docs/spec/native.md の「関数の形」)。
mod call;
mod function;
pub mod names;
pub mod rows;
mod safepoint;
mod unwind;

use std::collections::HashMap;

use cranelift_codegen::Context;
use cranelift_codegen::ir::{ExternalName, Function, Signature, UserFuncName};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_module::{FuncId, Linkage, Module};
use eml_lir::{FnIdx, LirFn, Program, Repr};
use eml_nrt::Kind;
use eml_nrt::image::SiteRow;

use crate::abi::{Built, declare_entry, tail_signature};

pub struct Defined {
    pub id: FuncId,
    pub size: u32,
    /// コンパイルの前のCLIF。求められたときだけ残す。
    pub clif: Option<String>,
    /// 関数の位置の行 (`FnRow.sites`)。失敗を上げる文ごとに1つで、文の順である。
    pub sites: Vec<SiteRow>,
}

/// moduleが定義したもの。プログラムの関数は`Program.functions`の順である。
pub struct Lowered {
    pub functions: Vec<Defined>,
    pub raise: Defined,
}

/// 下ろしたコードが名指す、プログラムによらない関数。
struct Imports {
    entries: HashMap<&'static str, FuncId>,
    raise: FuncId,
}

impl Imports {
    fn entry(&self, symbol: &str) -> FuncId {
        *self
            .entries
            .get(symbol)
            .unwrap_or_else(|| panic!("no runtime entry `{symbol}`"))
    }
}

/// プログラムの関数とその宣言。直接の呼び出しは、呼ぶ先の引数のReprと記号をここで引く。コードは呼ぶ先を記号で名指すので、
/// 宣言の番号はコードに現れない (docs/spec/native.md の「関数の形」)。
struct Callees<'a> {
    program: &'a Program,
    /// `FnIdx`の順である。
    ids: Vec<FuncId>,
}

impl Callees<'_> {
    fn id(&self, function: FnIdx) -> FuncId {
        self.ids[function.0 as usize]
    }
}

pub fn kind(repr: Repr) -> Kind {
    match repr {
        Repr::Float => Kind::F64,
        Repr::Obj | Repr::TObj | Repr::Int | Repr::Enum | Repr::Unit | Repr::Token => Kind::I64,
        Repr::Struct(_) => unreachable!("flattening leaves no struct value"),
    }
}

pub fn params(function: &LirFn) -> Vec<Kind> {
    function
        .params()
        .iter()
        .map(|&var| kind(function.repr(var)))
        .collect()
}

pub fn rets(function: &LirFn) -> Vec<Kind> {
    function.rets.iter().map(|&(repr, _)| kind(repr)).collect()
}

pub fn signature_of(function: &LirFn) -> Signature {
    tail_signature(&params(function), &rets(function))
}

pub fn program<M: Module>(module: &mut M, program: &Program, keep_clif: bool) -> Built<Lowered> {
    // エントリと`eml$raise`は、プログラムの関数より前に決まった順で宣言する。どの本体もそれらを名指せ、その番号は
    // プログラムによらない
    let mut entries = HashMap::new();
    for entry in eml_nrt::entries() {
        entries.insert(entry.symbol, declare_entry(module, entry)?);
    }
    let raise =
        module.declare_function(names::RAISE, Linkage::Local, &unwind::raise_signature())?;
    let imports = Imports { entries, raise };
    let mut ids = Vec::with_capacity(program.functions.len());
    for function in &program.functions {
        ids.push(module.declare_function(
            &names::function(&function.name),
            Linkage::Local,
            &signature_of(function),
        )?);
    }
    let unwinds = eml_lir::unwind::points(program, |_| true);
    let mut ctx = module.make_context();
    let mut fctx = FunctionBuilderContext::new();
    let mut functions = Vec::with_capacity(ids.len());
    let callees = Callees { program, ids };
    for ((function, &id), unwinds) in program.functions.iter().zip(&callees.ids).zip(unwinds) {
        let unwinds = match unwinds.expect("every function's unwind points are asked for") {
            Ok(unwinds) => unwinds,
            Err(error) => panic!("`{}` has no unwind points: {error}", function.name),
        };
        ctx.func.signature = signature_of(function);
        ctx.func.name = UserFuncName::testcase(names::function(&function.name));
        let b = FunctionBuilder::new(&mut ctx.func, &mut fctx);
        let sites = function::lower(b, module, &imports, &callees, id, function, &unwinds);
        functions.push(define(module, &mut ctx, id, keep_clif, sites)?);
    }
    ctx.func.signature = unwind::raise_signature();
    ctx.func.name = UserFuncName::testcase(names::RAISE);
    let b = FunctionBuilder::new(&mut ctx.func, &mut fctx);
    unwind::define_raise(b, module, &imports);
    let raise = define(module, &mut ctx, raise, keep_clif, Vec::new())?;
    Ok(Lowered { functions, raise })
}

fn define<M: Module>(
    module: &mut M,
    ctx: &mut Context,
    id: FuncId,
    keep_clif: bool,
    sites: Vec<SiteRow>,
) -> Built<Defined> {
    let clif = keep_clif.then(|| clif(module, &ctx.func));
    module.define_function(id, ctx)?;
    let size = ctx
        .compiled_code()
        .expect("a defined function has its code")
        .code_buffer()
        .len();
    module.clear_context(ctx);
    Ok(Defined {
        id,
        size: u32::try_from(size).expect("a function's code fits in 4 GiB"),
        clif,
        sites,
    })
}

/// `func`のCLIFの文。呼ぶ関数を記号の名前で書く。moduleの中の番号は宣言の順で決まり、関数の自分自身への参照の番号は
/// プログラムのほかの関数の数で変わるが、記号の名前はLIRの名前だけで決まるためである (docs/spec/native.md の「関数の形」)。
fn clif<M: Module>(module: &M, func: &Function) -> String {
    let mut shown = func.clone();
    for data in shown.dfg.ext_funcs.values_mut() {
        if let ExternalName::User(name) = data.name {
            let id = FuncId::from_u32(func.params.user_named_funcs()[name].index);
            let symbol = module.declarations().get_function_decl(id).linkage_name(id);
            data.name = ExternalName::testcase(symbol.as_bytes());
        }
    }
    shown.display().to_string()
}
