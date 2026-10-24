//! LIR から `CodeBase` を作り、追記する (docs/spec/vm.md)。

mod constants;
mod emit;
mod moves;
mod slots;

use std::sync::Arc;

use eml_lir::unwind::UnwindPoint;
use eml_lir::{FnIdx, Program};
use eml_rt::{Control, Mode};

use crate::code::{CodeBase, CompileError, FnEntry, Index, Tables};
use crate::disasm;
use constants::Constants;

/// プログラムの番号から `CodeBase` の番号への写し (docs/spec/vm.md の「追記」)。添字はプログラムの番号である。
pub(crate) struct Links {
    pub(crate) functions: Vec<u32>,
    pub(crate) strings: Vec<u32>,
    pub(crate) effects: Vec<u16>,
    pub(crate) files: Vec<u32>,
}

/// プログラム全体を新しい `CodeBase` に変換する。逆アセンブラのテストが使う。
pub fn compile<M: Mode>(
    program: &Program,
    control: Arc<Control>,
) -> Result<CodeBase<M>, CompileError> {
    let mut code = CodeBase::new(control);
    code.load(program)?;
    Ok(code)
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
        }
    }

    /// プログラムを追記し、入口の関数の番号を返す。すでにある名前の関数は前の番号を使い、ない名前の関数だけを変換して
    /// 末尾に足す。呼び出しは関数の番号で指し、VM が `FnEntry` から先頭を引くので、関数の先頭の位置が決まる前に
    /// 呼び出しを書ける。名前が同じ関数はいつも同じコードになるので (docs/spec/vm.md の「追記」)、debug ビルドでは
    /// すでにある名前の関数も変換し直して比べる。誤りを返した後の表は途中まで書いたものなので、使い続けない。
    pub fn load(&mut self, program: &Program) -> Result<u32, CompileError> {
        let entry = program.function(program.entry);
        if entry.rets.len() > 1 {
            return Err(CompileError::MultiValueEntry {
                function: entry.name.clone(),
            });
        }
        let links = self.links(program);
        let CodeBase {
            functions,
            tables: out,
            constants,
            region,
            files,
            index,
            control,
        } = self;
        let mut pool = Constants::new(constants, index, region, control);
        let strings = &program.strings;
        let strings: Vec<u32> = strings.iter().map(|text| pool.string(text)).collect();
        let links = Links { strings, ..links };
        let known = functions.len();
        // verifier の拒む関数は表を持たず、その関数のフレームを通る中断は内部の誤りになる。VM 自身の見張りを
        // 確かめるテストが、verifier を通さない IR を読み込むためである
        let points: Vec<Vec<UnwindPoint>> = eml_lir::unwind::points_all(program)
            .into_iter()
            .map(Result::unwrap_or_default)
            .collect();
        let mut result = Ok(());
        for (i, function) in program.functions.iter().enumerate() {
            if links.functions[i] as usize >= known {
                match emit::function(
                    program,
                    FnIdx(i as u32),
                    function,
                    &points[i],
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
                program, &points, &links, functions, known, files, &mut pool, out,
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
        Ok(links.functions[program.entry.0 as usize])
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
        }
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
    points: &[Vec<UnwindPoint>],
    links: &Links,
    functions: &[FnEntry],
    known: usize,
    files: &[String],
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
            &points[i],
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
