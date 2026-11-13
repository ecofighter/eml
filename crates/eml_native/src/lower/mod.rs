//! LIRをCLIFに下ろす。どの関数を定義するよりも前にプログラムのすべての関数と`apply`の補助の関数を宣言するので、本体は
//! それらのどれも名指せる。クロージャのエントリは、データの記号と同じく本体が初めて使うときに宣言し、本体の後で定義する。
//! PAPのアダプタはコードが名指さず、`Image.paps`だけが指すので、最後に宣言して定義する (docs/spec/native.md の「関数の形」)。
mod apply;
mod call;
pub mod consts;
mod data;
mod function;
pub mod names;
mod rc;
pub mod rows;
mod safepoint;
mod unwind;

use std::collections::HashMap;

use cranelift_codegen::Context;
use cranelift_codegen::ir::{
    ExternalName, Function, GlobalValueData, Signature, UserExternalNameRef, UserFuncName,
};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_module::{DataId, FuncId, Linkage, Module};
use eml_lir::{FnIdx, LirFn, Program, Repr};
use eml_nrt::Kind;
use eml_nrt::image::SiteRow;

use self::apply::{ClosureEntries, ClosureEntry};
use self::consts::{Data, Datum};
use crate::abi::{Built, Import, declare_import, tail_signature};

pub struct Defined {
    pub id: FuncId,
    pub size: u32,
    /// コンパイルの前のCLIF。求められたときだけ残す。
    pub clif: Option<String>,
    /// 関数の位置の行 (`FnRow.sites`)。失敗を上げる文ごとに1つで、文の順である。
    pub sites: Vec<SiteRow>,
}

/// moduleが定義したもの。プログラムの関数は`Program.functions`の順で、読み込み時の値は最初に使った順である。
pub struct Lowered {
    pub functions: Vec<Defined>,
    pub raise: Defined,
    /// クロージャのエントリ`c<k>:<f>`と、その値の待つ引数の数。最初に使った順である。
    pub closures: Vec<(ClosureEntry, u16, Defined)>,
    /// `eml$apply$<m>`。添字が`m - 1`である。
    pub applies: Vec<Defined>,
    /// PAPのアダプタ`eml$pap$<r>`と、その`r`。
    pub paps: Vec<(u32, Defined)>,
    pub data: Vec<(Datum, DataId)>,
}

/// 下ろしたコードが名指す、プログラムの関数でない関数。コードは名前と形だけで決まる。
struct Imports {
    entries: HashMap<&'static str, FuncId>,
    raise: FuncId,
    /// `eml$apply$<m>`。添字が`m - 1`である。
    applies: Vec<FuncId>,
}

impl Imports {
    fn entry(&self, symbol: &str) -> FuncId {
        *self
            .entries
            .get(symbol)
            .unwrap_or_else(|| panic!("no runtime entry `{symbol}`"))
    }

    fn apply(&self, args: usize) -> FuncId {
        self.applies[args - 1]
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
        entries.insert(entry.symbol, declare_import(module, Import::Entry(entry))?);
    }
    let raise =
        module.declare_function(names::RAISE, Linkage::Local, &unwind::raise_signature())?;
    let mut applies = Vec::new();
    for args in 1..=apply::largest_apply(program) {
        applies.push(module.declare_function(
            &names::apply(args),
            Linkage::Local,
            &apply::uniform(args),
        )?);
    }
    let imports = Imports {
        entries,
        raise,
        applies,
    };
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
    let mut data = Data::default();
    let mut closure_entries = ClosureEntries::default();
    let callees = Callees { program, ids };
    for ((function, &id), unwinds) in program.functions.iter().zip(&callees.ids).zip(unwinds) {
        let unwinds = match unwinds.expect("every function's unwind points are asked for") {
            Ok(unwinds) => unwinds,
            Err(error) => panic!("`{}` has no unwind points: {error}", function.name),
        };
        ctx.func.signature = signature_of(function);
        ctx.func.name = UserFuncName::testcase(names::function(&function.name));
        let b = FunctionBuilder::new(&mut ctx.func, &mut fctx);
        let sites = function::lower(
            b,
            module,
            &mut data,
            &mut closure_entries,
            &imports,
            &callees,
            id,
            function,
            &unwinds,
        );
        functions.push(define(module, &mut ctx, id, keep_clif, sites)?);
    }
    ctx.func.signature = unwind::raise_signature();
    ctx.func.name = UserFuncName::testcase(names::RAISE);
    let b = FunctionBuilder::new(&mut ctx.func, &mut fctx);
    unwind::define_raise(b, module, &imports);
    let raise = define(module, &mut ctx, raise, keep_clif, Vec::new())?;
    let mut closures = Vec::new();
    // どの関数の値もクロージャのエントリを持つので、エントリの待つ引数の数の最大が、関数の値の待ちうる数の最大である
    let mut largest = 0;
    for (closure, id) in closure_entries.declared() {
        let args = apply::arity(
            program.function(closure.function),
            closure.captures as usize,
        );
        largest = largest.max(args);
        ctx.func.signature = apply::uniform(usize::from(args));
        ctx.func.name = UserFuncName::testcase(names::closure_entry(
            closure.captures,
            &program.function(closure.function).name,
        ));
        let b = FunctionBuilder::new(&mut ctx.func, &mut fctx);
        apply::define_closure_entry(b, module, &imports, &callees, closure);
        closures.push((
            closure,
            args,
            define(module, &mut ctx, id, false, Vec::new())?,
        ));
    }
    let mut applies = Vec::with_capacity(imports.applies.len());
    for (index, &id) in imports.applies.iter().enumerate() {
        let args = index + 1;
        ctx.func.signature = apply::uniform(args);
        ctx.func.name = UserFuncName::testcase(names::apply(args));
        let b = FunctionBuilder::new(&mut ctx.func, &mut fctx);
        apply::define_apply(b, module, &imports, args);
        applies.push(define(module, &mut ctx, id, false, Vec::new())?);
    }
    let mut paps = Vec::new();
    for rest in apply::pap_rests(largest) {
        let name = names::pap(rest);
        let signature = apply::uniform(rest as usize);
        let id = module.declare_function(&name, Linkage::Local, &signature)?;
        ctx.func.signature = signature;
        ctx.func.name = UserFuncName::testcase(name);
        let b = FunctionBuilder::new(&mut ctx.func, &mut fctx);
        apply::define_pap_adapter(b, module, &imports, rest);
        paps.push((rest, define(module, &mut ctx, id, false, Vec::new())?));
    }
    let data = data.define(module)?;
    Ok(Lowered {
        functions,
        raise,
        closures,
        applies,
        paps,
        data,
    })
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

/// `func`のCLIFの文。呼ぶ関数とデータの記号を、記号の名前で書く。moduleの中の番号は宣言の順で決まり、プログラムの
/// ほかの関数と値の数で変わるが、記号の名前はLIRの名前と値の中身だけで決まるためである (docs/spec/native.md の
/// 「関数の形」)。
fn clif<M: Module>(module: &M, func: &Function) -> String {
    let mut shown = func.clone();
    for data in shown.dfg.ext_funcs.values_mut() {
        if let ExternalName::User(name) = data.name {
            data.name = symbol_name(module, func, name);
        }
    }
    for value in shown.global_values.values_mut() {
        if let GlobalValueData::Symbol { name, .. } = value
            && let ExternalName::User(user) = *name
        {
            *name = symbol_name(module, func, user);
        }
    }
    shown.display().to_string()
}

/// `func`が名指すmoduleの記号`name`の、名前で書いた形。`cranelift_module`は関数を名前空間0に、データを1に置く。
fn symbol_name<M: Module>(module: &M, func: &Function, name: UserExternalNameRef) -> ExternalName {
    let user = &func.params.user_named_funcs()[name];
    let declarations = module.declarations();
    let symbol = match user.namespace {
        0 => {
            let id = FuncId::from_u32(user.index);
            declarations
                .get_function_decl(id)
                .linkage_name(id)
                .into_owned()
        }
        1 => {
            let id = DataId::from_u32(user.index);
            declarations.get_data_decl(id).linkage_name(id).into_owned()
        }
        namespace => unreachable!("cranelift_module has no namespace {namespace}"),
    };
    ExternalName::testcase(symbol.as_bytes())
}
