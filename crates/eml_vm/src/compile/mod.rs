//! LIR から `CodeBase` を作り、追記する (docs/spec/vm.md)。

mod constants;
mod emit;
mod moves;
mod slots;

use std::mem;
use std::sync::Arc;

use eml_lir::unwind::UnwindPoint;
use eml_lir::{FnIdx, Loc, Program};
use eml_rt::{Control, MaskItem, Mode};

use crate::code::{CodeBase, CompileError, FnEntry, Index, Unwind, builtins};
use crate::disasm;
use crate::op::{Op, Pc};
use constants::Constants;

/// 命令とオペランドを書く先。関数をまたいで1つにする。`load` の間だけ `CodeBase` の列を借りて末尾に書く。
#[derive(Default)]
pub(crate) struct Out {
    pub(crate) code: Vec<Op>,
    pub(crate) operands: Vec<u32>,
    pub(crate) mask_lists: Vec<MaskItem>,
    pub(crate) locations: Vec<(Pc, Loc)>,
    pub(crate) unwinds: Vec<(Pc, Unwind)>,
}

impl Out {
    pub(crate) fn pc(&self) -> Pc {
        Pc::try_from(self.code.len()).expect("the code fits in u32 positions")
    }

    pub(crate) fn push(&mut self, op: Op) -> Pc {
        let pc = self.pc();
        self.code.push(op);
        pc
    }

    /// オペランドの並びを足し、その先頭の添字を返す。
    pub(crate) fn operands(&mut self, items: impl IntoIterator<Item = u32>) -> u32 {
        let at = u32::try_from(self.operands.len()).expect("the operands fit in u32 positions");
        self.operands.extend(items);
        at
    }

    /// 書いた長さ。一致の確かめで書いた写しを消すために覚える。
    fn lengths(&self) -> [usize; 5] {
        [
            self.code.len(),
            self.operands.len(),
            self.mask_lists.len(),
            self.locations.len(),
            self.unwinds.len(),
        ]
    }

    fn truncate(&mut self, [code, operands, mask_lists, locations, unwinds]: [usize; 5]) {
        self.code.truncate(code);
        self.operands.truncate(operands);
        self.mask_lists.truncate(mask_lists);
        self.locations.truncate(locations);
        self.unwinds.truncate(unwinds);
    }
}

/// プログラムの番号から `CodeBase` の番号への写し (docs/spec/vm.md の「追記」)。添字はプログラムの番号である。
pub(crate) struct Links {
    pub(crate) functions: Vec<u32>,
    pub(crate) strings: Vec<u32>,
    pub(crate) effects: Vec<u16>,
    pub(crate) files: Vec<u32>,
}

impl<M: Mode> CodeBase<M> {
    /// 組み込みの命令だけを持つ空の表。
    pub fn new(control: Arc<Control>) -> CodeBase<M> {
        CodeBase {
            functions: Vec::new(),
            code: builtins(),
            operands: Vec::new(),
            mask_lists: Vec::new(),
            constants: Vec::new(),
            region: None,
            locations: Vec::new(),
            unwinds: Vec::new(),
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
            code,
            operands,
            mask_lists,
            constants,
            region,
            locations,
            unwinds,
            files,
            index,
            control,
        } = self;
        let mut out = Out {
            code: mem::take(code),
            operands: mem::take(operands),
            mask_lists: mem::take(mask_lists),
            locations: mem::take(locations),
            unwinds: mem::take(unwinds),
        };
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
                    &mut out,
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
                program, &points, &links, functions, known, files, &mut pool, &mut out,
            );
        }
        let frozen = pool.freeze();
        *code = out.code;
        *operands = out.operands;
        *mask_lists = out.mask_lists;
        *locations = out.locations;
        *unwinds = out.unwinds;
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
    out: &mut Out,
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
                code: &out.code,
                operands: &out.operands,
                mask_lists: &out.mask_lists,
                locations: &out.locations,
                unwinds: &out.unwinds,
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
