//! `CodeBase` を関数ごとのテキストにする。コード生成のテストのスナップショットと、誤りの調べに使う。追記の一致の
//! 確かめも、関数の命令を同じテキストにして比べる (docs/spec/vm.md の「追記」)。

use std::fmt::Write as _;

use eml_extern::Extern;
use eml_lir::Loc;
use eml_rt::Mode;

use crate::code::{CodeBase, FnEntry};
use crate::op::{KEEP, NONE, Op, Pc, RAW};

pub fn disassemble<M: Mode>(code: &CodeBase<M>) -> String {
    let view = View::of(code);
    let mut out = String::new();
    for f in &code.functions {
        writeln!(
            out,
            "fn {} (arity {}, frame {})",
            f.name, f.arity, f.frame_size
        )
        .unwrap();
        for pc in f.start..f.end {
            writeln!(out, "  {pc:>4}  {}", view.line(pc, 0)).unwrap();
        }
    }
    out
}

/// 命令を読むための `CodeBase` の列。追記の途中で `CodeBase` から借りた列も読む。
pub(crate) struct View<'a> {
    pub(crate) code: &'a [Op],
    pub(crate) operands: &'a [u32],
    pub(crate) effect_lists: &'a [u16],
    pub(crate) locations: &'a [(Pc, Loc)],
    pub(crate) functions: &'a [FnEntry],
    pub(crate) files: &'a [String],
}

fn slots(list: &[u32]) -> String {
    list.iter()
        .map(|slot| format!("s{slot}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// [位置 | RAW, スロット] の並び。
fn fields(items: &[u32]) -> String {
    items
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| {
            let raw = if pair[0] & RAW != 0 { "raw " } else { "" };
            format!("{raw}@{} -> s{}", pair[0] & !RAW, pair[1])
        })
        .collect::<Vec<_>>()
        .join(", ")
}

impl<'a> View<'a> {
    fn of<M: Mode>(code: &'a CodeBase<M>) -> View<'a> {
        View {
            code: &code.code,
            operands: &code.operands,
            effect_lists: &code.effect_lists,
            locations: &code.locations,
            functions: &code.functions,
            files: &code.files,
        }
    }

    /// 関数の命令のテキスト。命令の位置は関数の先頭からの位置で書くので、同じ関数を別の位置に置いた写しも同じテキスト
    /// になる。
    pub(crate) fn function(&self, f: &FnEntry) -> Vec<String> {
        (f.start..f.end).map(|pc| self.line(pc, f.start)).collect()
    }

    fn location(&self, pc: Pc) -> Option<Loc> {
        let index = self
            .locations
            .binary_search_by_key(&pc, |&(at, _)| at)
            .ok()?;
        Some(self.locations[index].1)
    }

    /// タグの switch の飛び先の表を、case のあるタグだけ並べる。
    fn tag_cases(&self, at: u32, origin: Pc) -> String {
        let o = &self.operands[at as usize..];
        let (m, default) = (o[0] as usize, o[1]);
        let mut parts = Vec::new();
        for tag in 0..m {
            let record = o[2 + tag];
            if record == NONE {
                continue;
            }
            let r = &o[record as usize..];
            let bound = fields(&r[3..3 + 2 * r[2] as usize]);
            parts.push(format!("#{tag} -> {} [{bound}]", r[0] - origin));
        }
        if default != NONE {
            parts.push(format!("_ -> {}", default - origin));
        }
        format!("{{ {} }}", parts.join(", "))
    }

    fn key_cases(&self, at: u32, origin: Pc) -> String {
        let o = &self.operands[at as usize..];
        let (n, default) = (o[0] as usize, o[1]);
        let mut parts: Vec<String> = (0..n)
            .map(|case| {
                let i = 2 + 3 * case;
                let key = i64::from(o[i]) | (i64::from(o[i + 1]) << 32);
                format!("{key} -> {}", o[i + 2] - origin)
            })
            .collect();
        if default != NONE {
            parts.push(format!("_ -> {}", default - origin));
        }
        format!("{{ {} }}", parts.join(", "))
    }

    /// 命令のテキスト。命令の位置 (飛び先と、自己末尾呼び出しの入口) は `origin` からの位置で書く。
    fn line(&self, pc: Pc, origin: Pc) -> String {
        let ops = |at: u32| &self.operands[at as usize..];
        let args = |at: u32, skip: usize| {
            let o = ops(at);
            let n = o[skip - 1] as usize;
            slots(&o[skip..skip + n])
        };
        let name = |f: u32| self.functions[f as usize].name.as_str();
        let bin = |name: &str, dst: u16, a: u16, b: u16| format!("{name} s{dst}, s{a}, s{b}");
        let mut text = match self.code[pc as usize] {
            Op::Move { dst, src } => format!("move s{dst}, s{src}"),
            Op::LoadRaw { dst, value } => format!("load_raw s{dst}, {value}"),
            Op::LoadImm { dst, value } => format!("load_imm s{dst}, {value}"),
            Op::LoadConst { dst, index } => format!("load_const s{dst}, c{index}"),
            Op::Goto { target } => format!("goto {}", target - origin),
            Op::Ret { src } => format!("ret s{src}"),
            Op::Call { dst, at } => {
                format!("call s{dst} = {}({})", name(ops(at)[0]), args(at, 2))
            }
            Op::TailCall { at } => format!("tail_call {}({})", name(ops(at)[0]), args(at, 2)),
            Op::Finish => "finish".to_string(),
            Op::ApplyRest => "apply_rest".to_string(),
            Op::Closure { dst, at } => {
                let o = ops(at);
                let lin = if o[1] != 0 { " lin" } else { "" };
                format!("closure s{dst} = {}({}){lin}", name(o[0]), args(at, 3))
            }
            Op::Apply { dst, at } => format!("apply s{dst} = s{}({})", ops(at)[0], args(at, 2)),
            Op::TailApply { at } => format!("tail_apply s{}({})", ops(at)[0], args(at, 2)),
            Op::Clause { dst, at } => {
                format!("clause s{dst} = s{}({})", ops(at)[0], args(at, 2))
            }
            Op::TailClause { at } => format!("tail_clause s{}({})", ops(at)[0], args(at, 2)),
            Op::TailSelf { at } => {
                format!("tail_self {}({})", ops(at)[0] - origin, args(at, 2))
            }
            Op::IntAdd { dst, a, b } => bin("int_add", dst, a, b),
            Op::IntSub { dst, a, b } => bin("int_sub", dst, a, b),
            Op::IntMul { dst, a, b } => bin("int_mul", dst, a, b),
            Op::IntDiv { dst, a, b } => bin("int_div", dst, a, b),
            Op::IntRem { dst, a, b } => bin("int_rem", dst, a, b),
            Op::IntNeg { dst, a } => format!("int_neg s{dst}, s{a}"),
            Op::IntEq { dst, a, b } => bin("int_eq", dst, a, b),
            Op::IntNe { dst, a, b } => bin("int_ne", dst, a, b),
            Op::IntLt { dst, a, b } => bin("int_lt", dst, a, b),
            Op::IntLe { dst, a, b } => bin("int_le", dst, a, b),
            Op::IntGt { dst, a, b } => bin("int_gt", dst, a, b),
            Op::IntGe { dst, a, b } => bin("int_ge", dst, a, b),
            Op::IntCompare { dst, a, b } => bin("int_compare", dst, a, b),
            Op::BoolEq { dst, a, b } => bin("bool_eq", dst, a, b),
            Op::BoolNe { dst, a, b } => bin("bool_ne", dst, a, b),
            Op::SwitchEnum { src, at } => {
                format!("switch_enum s{src} {}", self.tag_cases(at, origin))
            }
            Op::SwitchObj { src, at } => {
                format!("switch_obj s{src} {}", self.tag_cases(at, origin))
            }
            Op::SwitchTObj { src, at } => {
                format!("switch_tobj s{src} {}", self.tag_cases(at, origin))
            }
            Op::SwitchInt { src, at } => {
                format!("switch_int s{src} {}", self.key_cases(at, origin))
            }
            Op::SwitchString { src, at } => {
                format!("switch_string s{src} {}", self.key_cases(at, origin))
            }
            Op::Unpack { src, at } => {
                let o = ops(at);
                format!(
                    "unpack s{src} #{} [{}]",
                    o[0],
                    fields(&o[3..3 + 2 * o[2] as usize])
                )
            }
            Op::Dup { src } => format!("dup s{src}"),
            Op::Decref { src } => format!("decref s{src}"),
            Op::Release { src, at } => {
                let o = ops(at);
                let entries: Vec<String> = o[3..3 + o[2] as usize]
                    .iter()
                    .map(|&e| {
                        let keep = if e & KEEP != 0 { "keep " } else { "" };
                        format!("{keep}@{}", e & !KEEP)
                    })
                    .collect();
                format!("release s{src} #{} [{}]", o[0], entries.join(", "))
            }
            Op::Con { dst, at } => {
                let o = ops(at);
                let lin = if o[3] != 0 { " lin" } else { "" };
                format!(
                    "con s{dst} = #{} ({} scanned, {} raw{lin}) [{}]",
                    o[0],
                    o[1],
                    o[2],
                    fields(&o[5..5 + 2 * o[4] as usize])
                )
            }
            Op::BoxInt { dst, src } => format!("box_int s{dst}, s{src}"),
            Op::BoxEnum { dst, src } => format!("box_enum s{dst}, s{src}"),
            Op::BoxConst { dst, index } => format!("box_const s{dst}, c{index}"),
            Op::Unbox { dst, src } => format!("unbox s{dst}, s{src}"),
            Op::EvvGet { dst } => format!("evv_get s{dst}"),
            Op::EvvSet { dst, l } => format!("evv_set s{dst}, s{l}"),
            Op::EvvPush { dst, l, e } => bin("evv_push", dst, l, e),
            Op::EvvMask { dst, at } => {
                let o = ops(at);
                let effects: Vec<String> = self.effect_lists[o[0] as usize..(o[0] + o[1]) as usize]
                    .iter()
                    .map(u16::to_string)
                    .collect();
                format!("evv_mask s{dst}, [{}] s{}", effects.join(", "), o[2])
            }
            Op::EvNew { dst, at } => {
                let o = ops(at);
                format!(
                    "ev_new s{dst}, E{} ({} ops), s{}, s{}",
                    o[0], o[1], o[2], o[3]
                )
            }
            Op::EvSetClause { dst, at } => {
                let o = ops(at);
                format!("ev_set_clause s{dst}, s{}, op{}, s{}", o[0], o[1], o[2])
            }
            Op::EvRenew { dst, ev, state } => bin("ev_renew", dst, ev, state),
            Op::EvLookup { dst, effect } => format!("ev_lookup s{dst}, E{effect}"),
            Op::EvClause { dst, ev, op } => format!("ev_clause s{dst}, s{ev}, op{op}"),
            Op::EvEvv { dst, ev } => format!("ev_evv s{dst}, s{ev}"),
            Op::EvTakeState { dst, ev } => format!("ev_take_state s{dst}, s{ev}"),
            Op::EvSetState { dst, ev, s } => bin("ev_set_state", dst, ev, s),
            Op::Yielding { dst } => format!("yielding s{dst}"),
            Op::YieldStatus { dst, ev } => format!("yield_status s{dst}, s{ev}"),
            Op::YieldTo { dst, at } => {
                let o = ops(at);
                format!("yield_to s{dst}, s{}, s{}, s{}", o[0], o[1], o[2])
            }
            Op::YieldAbort { dst, at } => {
                let o = ops(at);
                format!("yield_abort s{dst}, s{}, s{}, s{}", o[0], o[1], o[2])
            }
            Op::YieldContTake { dst } => format!("yield_cont_take s{dst}"),
            Op::YieldContSet { dst, k } => format!("yield_cont_set s{dst}, s{k}"),
            Op::YieldFinish { dst } => format!("yield_finish s{dst}"),
            Op::Dummy { dst } => format!("dummy s{dst}"),
            Op::Extern { dst, at } => format!(
                "extern s{dst} = {}({})",
                Extern::ALL[ops(at)[0] as usize].row().name,
                args(at, 2)
            ),
        };
        if let Some(loc) = self.location(pc) {
            write!(
                text,
                " @{}:{}:{}",
                self.files[loc.file as usize], loc.line, loc.column
            )
            .unwrap();
        }
        text
    }
}
