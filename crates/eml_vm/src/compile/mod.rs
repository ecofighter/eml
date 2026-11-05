//! LIR から `CodeBase` を作り、追記する (docs/spec/vm.md)。

mod constants;
mod emit;
mod forks;
mod moves;
mod slots;

use std::sync::Arc;

use eml_cabi::Linked;
use eml_extern::{CFunction, CHandleId, CParam, CRet, Extern, RuntimeEntry};
use eml_lir::unwind::Unwinds;
use eml_lir::{FnIdx, Program, Rhs, Stmt};
use eml_rt::{Control, Mode};

use crate::code::{
    AsyncEntries, CEntry, CHandleEntry, CodeBase, CompileError, FnEntry, Forks, Index, Tables,
};
use crate::disasm;
use constants::Constants;

/// プログラムの番号から `CodeBase` の番号への写し (docs/spec/vm.md の「追記」)。添字はプログラムの番号である。
pub(crate) struct Links {
    pub(crate) functions: Vec<u32>,
    pub(crate) strings: Vec<u32>,
    pub(crate) effects: Vec<u16>,
    pub(crate) files: Vec<u32>,
    /// C の関数の番号。添字はプログラムの `c_functions` の番号である。
    pub(crate) c_functions: Vec<u32>,
}

/// プログラムの C の表を `CodeBase` の番号に振った結果と、`CodeBase` にまだない項。
struct CLinks {
    functions: Vec<u32>,
    new_functions: Vec<CEntry>,
    new_handles: Vec<CHandleEntry>,
}

impl<M: Mode> CodeBase<M> {
    /// 組み込みの命令だけを持つ空の表。
    pub fn new(control: Arc<Control>) -> CodeBase<M> {
        CodeBase {
            functions: Vec::new(),
            tables: Tables::new(),
            constants: Vec::new(),
            region: None,
            files: Vec::new(),
            index: Index::default(),
            control,
            forks: Forks {
                tables: Vec::new(),
                panic: None,
                tail: eml_rt::Word::UNIT,
            },
            entries: None,
            c_functions: Vec::new(),
            c_handles: Vec::new(),
        }
    }

    /// プログラムを追記し、エントリの関数の番号を返す。すでにある名前の関数は前の番号を使い、ない名前の関数だけを変換して
    /// 末尾に足す。呼び出しは関数の番号で指し、VM が `FnEntry` から先頭を引くので、関数の先頭の位置が決まる前に
    /// 呼び出しを書ける。名前が同じ関数はいつも同じコードになるので (docs/spec/vm.md の「追記」)、debug ビルドでは
    /// すでにある名前の関数も変換し直して比べる。誤りを返した後の表は途中まで書いたものなので、使い続けない。
    ///
    /// `linked` は、このプログラムの C の表をつないだ結果である。ポインタが署名に合うことは `Linked::new` の約束で、
    /// ここでは長さだけを確かめる。長さが表と合わないのは呼び手の誤りなので、panic する
    /// (docs/spec/vm.md の「C の関数の命令」)。
    pub fn load(&mut self, program: &Program, linked: &Linked) -> Result<u32, CompileError> {
        assert!(
            linked.fits(&program.c_functions, &program.c_handles),
            "the linked calls and dtors line up with the C tables of the program"
        );
        let entry = program.function(program.entry);
        if entry.rets.len() > 1 {
            return Err(CompileError::MultiValueEntry {
                function: entry.name.clone(),
            });
        }
        let c_links = self.c_links(program, linked)?;
        let known_c = self.c_functions.len();
        let known_handles = self.c_handles.len();
        self.c_functions.extend(c_links.new_functions);
        self.c_handles.extend(c_links.new_handles);
        let links = Links {
            c_functions: c_links.functions,
            ..self.links(program)
        };
        let CodeBase {
            functions,
            tables: out,
            constants,
            region,
            files,
            index,
            control,
            forks,
            entries,
            c_functions,
            c_handles,
        } = self;
        let mut pool = Constants::new(constants, index, region, control);
        let strings = &program.strings;
        let strings: Vec<u32> = strings.iter().map(|text| pool.string(text)).collect();
        let links = Links { strings, ..links };
        let known = functions.len();
        // verifier の拒む関数は空のエントリの行だけを持ち、地点の行を持たない。その関数の地点で止まったフレームを
        // 通る中断は内部の誤りになる。VM 自身の見張りを確かめるテストが、verifier を通さない IR を読み込むためである。
        // 表を作るのは変換する関数だけで、debug ビルドではすでにある関数も変換し直して比べる
        let unwinds: Vec<Unwinds> = eml_lir::unwind::points(program, |FnIdx(i)| {
            cfg!(debug_assertions) || links.functions[i as usize] as usize >= known
        })
        .into_iter()
        .map(|unwinds| {
            unwinds.and_then(Result::ok).unwrap_or(Unwinds {
                entry: Vec::new(),
                points: Vec::new(),
            })
        })
        .collect();
        let mut result = Ok(());
        for (i, function) in program.functions.iter().enumerate() {
            if links.functions[i] as usize >= known {
                match emit::function(
                    program,
                    FnIdx(i as u32),
                    function,
                    &unwinds[i],
                    &links,
                    &mut pool,
                    out,
                ) {
                    Ok(entry) => functions.push(entry),
                    Err(error) => {
                        result = Err(error);
                        break;
                    }
                }
            }
        }
        if result.is_ok() && cfg!(debug_assertions) {
            result = check_known(
                program,
                &unwinds,
                &links,
                functions,
                known,
                files,
                c_functions,
                &mut pool,
                out,
            );
        }
        let frozen = pool.freeze();
        result?;
        if let Some(chunk) = frozen? {
            *region = Some(chunk);
        }
        for (i, function) in program.functions.iter().enumerate() {
            index
                .functions
                .entry(function.name.clone())
                .or_insert(links.functions[i]);
        }
        forks::extend(program, &links, index, forks, region, control)?;
        if entries.is_none() {
            *entries = async_entries(program, index)?;
        }
        // C の名前は、`load` 全体が通ってから引けるようにする
        for (i, entry) in c_functions.iter().enumerate().skip(known_c) {
            index
                .c_functions
                .insert(entry.function.name.clone(), i as u32);
        }
        for (i, entry) in c_handles.iter().enumerate().skip(known_handles) {
            index.c_handles.insert(entry.handle.name.clone(), i as u32);
        }
        Ok(links.functions[program.entry.0 as usize])
    }

    /// プログラムの C の関数とハンドルの型を、名前で `CodeBase` の番号に振る。ない名前には末尾の番号を振り、`linked` の
    /// ポインタを持つ新しい項を作る。すでにある名前の項は前のものを使い、新しいポインタは捨てる。すでにある名前で項が
    /// 違えば `Mismatch` を返す。`CodeBase` には何も書かない (docs/spec/vm.md の「C の関数の命令」)。
    fn c_links(&self, program: &Program, linked: &Linked) -> Result<CLinks, CompileError> {
        let mismatch = |name: &str| CompileError::Mismatch {
            function: name.to_string(),
        };
        let mut handles = Vec::with_capacity(program.c_handles.len());
        let mut new_handles = Vec::new();
        for (handle, &dtor) in program.c_handles.iter().zip(linked.dtors()) {
            match self.index.c_handles.get(&handle.name) {
                Some(&k) if self.c_handles[k as usize].handle == *handle => handles.push(k),
                Some(_) => return Err(mismatch(&handle.name)),
                None => {
                    handles.push((self.c_handles.len() + new_handles.len()) as u32);
                    new_handles.push(CHandleEntry {
                        handle: handle.clone(),
                        dtor,
                    });
                }
            }
        }
        let mut functions = Vec::with_capacity(program.c_functions.len());
        let mut new_functions = Vec::new();
        for (function, &call) in program.c_functions.iter().zip(linked.calls()) {
            let function = renumbered(function, &handles);
            match self.index.c_functions.get(&function.name) {
                Some(&k) if self.c_functions[k as usize].function == function => functions.push(k),
                Some(_) => return Err(mismatch(&function.name)),
                None => {
                    functions.push((self.c_functions.len() + new_functions.len()) as u32);
                    new_functions.push(CEntry { function, call });
                }
            }
        }
        Ok(CLinks {
            functions,
            new_functions,
            new_handles,
        })
    }

    /// 関数、エフェクト、位置のファイルの番号を引く。ない名前には末尾の番号を振る。文字列は定数を作るときに引く。
    fn links(&mut self, program: &Program) -> Links {
        let mut next = self.functions.len() as u32;
        let functions = program
            .functions
            .iter()
            .map(|function| {
                self.index
                    .functions
                    .get(&function.name)
                    .copied()
                    .unwrap_or_else(|| {
                        next += 1;
                        next - 1
                    })
            })
            .collect();
        let effects = program
            .effects
            .iter()
            .map(|effect| {
                let next = self.index.effects.len() as u32;
                *self
                    .index
                    .effects
                    .entry(effect.name.clone())
                    .or_insert_with(|| emit::effect_number(next))
            })
            .collect();
        let files = program
            .files
            .iter()
            .map(|path| {
                *self.index.files.entry(path.clone()).or_insert_with(|| {
                    self.files.push(path.clone());
                    self.files.len() as u32 - 1
                })
            })
            .collect();
        Links {
            functions,
            strings: Vec::new(),
            effects,
            files,
            c_functions: Vec::new(),
        }
    }
}

/// ハンドルの型の番号を `CodeBase` の番号 `handles` に付け直した C の関数。行は欄から作るので、欄を書き換えずに
/// `CFunction::new` で作り直す。
fn renumbered(function: &CFunction, handles: &[u32]) -> CFunction {
    let handle = |id: CHandleId| CHandleId(handles[id.0 as usize]);
    let params = function
        .params
        .iter()
        .map(|&param| match param {
            CParam::Handle(id) => CParam::Handle(handle(id)),
            other => other,
        })
        .collect();
    let ret = match function.ret {
        CRet::Handle(id) => CRet::Handle(handle(id)),
        other => other,
    };
    CFunction::new(
        function.name.clone(),
        function.symbol.clone(),
        function.links.clone(),
        function.pure,
        function.blocking,
        params,
        ret,
    )
}

/// ランタイムが名前で呼ぶ `Std.Async` のエントリの番号。名前は前のロードの関数からも引く。`Std.Async` は import した
/// ときだけ読み込むので、引けないロードもある。そのロードが `Async` の命令を持てば、コード生成の誤りにする
/// (docs/spec/vm.md の「`Async` のタスク」)。
fn async_entries(program: &Program, index: &Index) -> Result<Option<AsyncEntries>, CompileError> {
    let find = |entry: RuntimeEntry| index.functions.get(entry.name()).copied();
    if let (Some(start), Some(resume), Some(resume_io)) = (
        find(RuntimeEntry::StartTask),
        find(RuntimeEntry::ResumeTask),
        find(RuntimeEntry::ResumeIo),
    ) {
        return Ok(Some(AsyncEntries {
            start,
            resume,
            resume_io,
        }));
    }
    let runs_async = program
        .functions
        .iter()
        .flat_map(|function| &function.blocks)
        .flat_map(|block| &block.stmts)
        .any(|stmt| {
            matches!(
                stmt,
                Stmt::Let {
                    rhs: Rhs::Extern { ext, .. },
                    ..
                } if ext.std().is_some_and(Extern::is_async)
            )
        });
    match RuntimeEntry::ALL
        .iter()
        .find(|&&entry| find(entry).is_none())
    {
        Some(missing) if runs_async => Err(CompileError::MissingEntry {
            function: missing.name().to_string(),
        }),
        _ => Ok(None),
    }
}

/// すでにある名前の関数を変換し直し、前の命令列と比べる (docs/spec/vm.md の「追記」)。命令の位置は関数の先頭からの
/// 位置にそろえ、オペランド、エフェクトの並び、位置は指す中身で比べる。比べた写しは消す。
#[expect(
    clippy::too_many_arguments,
    reason = "the tables a reloaded function is emitted into"
)]
fn check_known<M: Mode>(
    program: &Program,
    unwinds: &[Unwinds],
    links: &Links,
    functions: &[FnEntry],
    known: usize,
    files: &[String],
    c_functions: &[CEntry],
    pool: &mut Constants<'_, M>,
    out: &mut Tables,
) -> Result<(), CompileError> {
    for (i, function) in program.functions.iter().enumerate() {
        let index = links.functions[i] as usize;
        if index >= known {
            continue;
        }
        let lengths = out.lengths();
        let again = emit::function(
            program,
            FnIdx(i as u32),
            function,
            &unwinds[i],
            links,
            pool,
            out,
        )?;
        let old = &functions[index];
        let view = |entry: &FnEntry| {
            disasm::View {
                tables: out,
                functions,
                files,
                c_functions,
            }
            .function(entry)
        };
        let same = (old.arity, old.frame_size, old.ret_rc)
            == (again.arity, again.frame_size, again.ret_rc)
            && old.params == again.params
            && view(old) == view(&again);
        out.truncate(lengths);
        if !same {
            return Err(CompileError::Mismatch {
                function: function.name.clone(),
            });
        }
    }
    Ok(())
}
