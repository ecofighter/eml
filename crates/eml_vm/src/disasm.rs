//! `CodeBase` を関数ごとのテキストにする。コード生成のテストのスナップショットと、誤りの調べに使う。追記の一致の
//! 確かめも、関数の命令を同じテキストにして比べる (docs/spec/vm.md の「追記」)。

use std::fmt::Write as _;

use eml_extern::Extern;
use eml_lir::Loc;
use eml_rt::{MaskItem, Mode};

use crate::code::{CodeBase, FnEntry, Unwind};
use crate::op::{KEEP, NONE, Op, Pc, RAW, SKIP};
use crate::sig::{SigId, Signatures};

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
    pub(crate) mask_lists: &'a [MaskItem],
    pub(crate) locations: &'a [(Pc, Loc)],
    pub(crate) unwinds: &'a [(Pc, Unwind)],
    pub(crate) signatures: &'a Signatures,
    pub(crate) functions: &'a [FnEntry],
    pub(crate) files: &'a [String],
}

/// `release` の項目の並び。
fn release_entries(entries: &[u32]) -> String {
    entries
        .iter()
        .map(|&e| {
            let keep = if e & KEEP != 0 { "keep " } else { "" };
            format!("{keep}@{}", e & !KEEP)
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// mask の項目を、IR のテキストの形の数字の版で書く。`keep` が 0 なら `@n` を、`skip` が 1 なら `*m` を省く。
fn mask_item(item: &MaskItem) -> String {
    let mut text = item.effect.to_string();
    if item.keep != 0 {
        write!(text, "@{}", item.keep).unwrap();
    }
    if item.skip != 1 {
        write!(text, "*{}", item.skip).unwrap();
    }
    text
}

fn slots(list: &[u32]) -> String {
    list.iter()
        .map(|slot| format!("s{slot}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// [位置 | RAW | SKIP, スロット] の並び。
fn fields(items: &[u32]) -> String {
    items
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| {
            let skip = if pair[0] & SKIP != 0 { "skip " } else { "" };
            let raw = if pair[0] & RAW != 0 { "raw " } else { "" };
            format!("{skip}{raw}@{} -> s{}", pair[0] & !(RAW | SKIP), pair[1])
        })
        .collect::<Vec<_>>()
        .join(", ")
}

impl<'a> View<'a> {
    fn of<M: Mode>(code: &'a CodeBase<M>) -> View<'a> {
        View {
            code: &code.code,
            operands: &code.operands,
            mask_lists: &code.mask_lists,
            locations: &code.locations,
            unwinds: &code.unwinds,
            signatures: &code.signatures,
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

    /// 命令 `pc` の後の巻き戻しの表の行。表は命令の次の位置で引く。
    fn unwind(&self, pc: Pc) -> Option<&Unwind> {
        let index = self
            .unwinds
            .binary_search_by_key(&(pc + 1), |&(at, _)| at)
            .ok()?;
        Some(&self.unwinds[index].1)
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

    /// シグネチャを中身で書く。番号は読み込みの順で変わるので書かない。
    fn signature(&self, id: u32) -> String {
        let sig = self.signatures.get(SigId(id));
        let names = |reprs: &[eml_lir::Repr]| {
            reprs
                .iter()
                .map(|repr| repr.name())
                .collect::<Vec<_>>()
                .join(", ")
        };
        let ret = match sig.ret.as_slice() {
            [one] => one.name().to_string(),
            many => format!("({})", names(many)),
        };
        format!("({}) -> {ret}", names(&sig.params))
    }

    /// `apply` のオペランド [呼ばれる値, n, 場所のシグネチャ, 引数…]。
    fn apply(&self, o: &[u32]) -> String {
        let n = o[1] as usize;
        format!(
            "s{}({}) : {}",
            o[0],
            slots(&o[3..3 + n]),
            self.signature(o[2])
        )
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
            Op::Apply { dst, at } => format!("apply s{dst} = {}", self.apply(ops(at))),
            Op::TailApply { at } => format!("tail_apply {}", self.apply(ops(at))),
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
            Op::FloatAdd { dst, a, b } => bin("float_add", dst, a, b),
            Op::FloatSub { dst, a, b } => bin("float_sub", dst, a, b),
            Op::FloatMul { dst, a, b } => bin("float_mul", dst, a, b),
            Op::FloatDiv { dst, a, b } => bin("float_div", dst, a, b),
            Op::FloatRem { dst, a, b } => bin("float_rem", dst, a, b),
            Op::FloatNeg { dst, a } => format!("float_neg s{dst}, s{a}"),
            Op::FloatEq { dst, a, b } => bin("float_eq", dst, a, b),
            Op::FloatNe { dst, a, b } => bin("float_ne", dst, a, b),
            Op::FloatLt { dst, a, b } => bin("float_lt", dst, a, b),
            Op::FloatLe { dst, a, b } => bin("float_le", dst, a, b),
            Op::FloatGt { dst, a, b } => bin("float_gt", dst, a, b),
            Op::FloatGe { dst, a, b } => bin("float_ge", dst, a, b),
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
                let entries = release_entries(&o[3..3 + o[2] as usize]);
                format!("release s{src} #{} [{entries}]", o[0])
            }
            Op::ReleaseReuse { src, at } => {
                let o = ops(at);
                let entries = release_entries(&o[4..4 + o[3] as usize]);
                format!("release s{src} #{} [{entries}] token s{}", o[1], o[0])
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
            Op::ConReuse { dst, at } => {
                let o = ops(at);
                let lin = if o[4] != 0 { " lin" } else { "" };
                let in_place = if o[5] != 0 { " in place" } else { "" };
                format!(
                    "con s{dst} = #{} ({} scanned, {} raw{lin}) [{}] reuse s{}{in_place}",
                    o[1],
                    o[2],
                    o[3],
                    fields(&o[7..7 + 2 * o[6] as usize]),
                    o[0]
                )
            }
            Op::DropToken { tok } => format!("drop_token s{tok}"),
            Op::BoxInt { dst, src } => format!("box_int s{dst}, s{src}"),
            Op::BoxEnum { dst, src } => format!("box_enum s{dst}, s{src}"),
            Op::BoxFloat { dst, src } => format!("box_float s{dst}, s{src}"),
            Op::BoxConst { dst, index } => format!("box_const s{dst}, c{index}"),
            Op::Unbox { dst, src } => format!("unbox s{dst}, s{src}"),
            Op::UnboxFloat { dst, src } => format!("unbox_float s{dst}, s{src}"),
            Op::EvvGet { dst } => format!("evv_get s{dst}"),
            Op::EvvSet { dst, l } => format!("evv_set s{dst}, s{l}"),
            Op::EvvPush { dst, l, e } => bin("evv_push", dst, l, e),
            Op::EvvMask { dst, at } => {
                let o = ops(at);
                let items: Vec<String> = self.mask_lists[o[0] as usize..(o[0] + o[1]) as usize]
                    .iter()
                    .map(mask_item)
                    .collect();
                format!("evv_mask s{dst}, [{}] s{}", items.join(", "), o[2])
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
            Op::YieldAbort { ev, clause } => format!("yield_abort s{ev}, s{clause}"),
            Op::YieldContTake { dst } => format!("yield_cont_take s{dst}"),
            Op::YieldContSet { dst, k } => format!("yield_cont_set s{dst}, s{k}"),
            Op::YieldFinish { dst } => format!("yield_finish s{dst}"),
            Op::Dummy { dst } => format!("dummy s{dst}"),
            Op::RootGet { dst, root } => format!("root_get s{dst}, r{root}"),
            Op::RootSet { dst, at } => {
                let o = ops(at);
                format!("root_set s{dst}, r{}, s{}", o[0], o[1])
            }
            Op::ArrayGet { dst, a, b } => bin("array_get", dst, a, b),
            Op::ArrayLength { dst, a } => format!("array_length s{dst}, s{a}"),
            Op::ArraySet { dst, at } => format!("array_set s{dst} = {}", slots(&ops(at)[..3])),
            Op::Extern { dst, at } => format!(
                "extern s{dst} = {}({})",
                Extern::ALL[ops(at)[0] as usize].row().name,
                args(at, 2)
            ),
        };
        // 手放すものも token も着地もない行は書かない。どの命令に行があるかは命令の種類で決まるので、書かなくても失うものはない
        if let Some(entry) = self.unwind(pc)
            && (!entry.slots.is_empty() || !entry.tokens.is_empty() || entry.landing.is_some())
        {
            let slots: Vec<String> = entry
                .slots
                .iter()
                .map(|&(slot, count)| match count {
                    1 => format!("s{slot}"),
                    n => format!("s{slot}*{n}"),
                })
                .collect();
            write!(text, " unwind [{}]", slots.join(", ")).unwrap();
            if !entry.tokens.is_empty() {
                let tokens: Vec<u32> = entry.tokens.iter().map(|&slot| slot.into()).collect();
                write!(text, " tokens [{}]", self::slots(&tokens)).unwrap();
            }
            if let Some(slot) = entry.landing {
                write!(text, " land s{slot}").unwrap();
            }
        }
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
