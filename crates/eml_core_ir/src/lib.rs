//! Core IR (docs/spec/core-ir.md)。型付き HIR から変換する、前向きの辺だけを持つ基本ブロックの列で表す IR で、
//! エフェクトを明示する。RC、box、末尾呼び出しは持たない。これらは EP の後に変換した LIR (`eml_lir`) の上で入れる
//! (docs/spec/core-ir.md の「実行系の方針」)。

use eml_extern::Extern;
use eml_lir::{
    Atom, BlockId, Case, Ctor, EffectInfo, FnIdx, Layout, LayoutId, Loc, Mark, MaskItem, Prim,
    Repr, Role, Site, VarId, VarInfo,
};

mod builder;
mod evidence;
mod fnv;
pub(crate) mod liveness;
mod lowering;
mod optimize;
mod pipeline;
mod pretty;
mod tail;
mod text;
mod translate;
mod verify;

pub use evidence::evidence;
pub use lowering::to_lir;
pub use optimize::optimize;
pub use pipeline::{Pass, core_until, lower, lower_until};
pub use pretty::{pretty, pretty_with_positions};
pub use text::parse;
pub use translate::type_repr;
pub use verify::{verify_evidence, verify_translated};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Program {
    pub functions: Vec<CoreFn>,
    /// 実行の入口。`main` を `()` で呼ぶ、引数のない関数 (docs/spec/core-ir.md)。
    pub entry: FnIdx,
    /// 文字列リテラルの定数表。`ConstString` と `CasePattern::String` が添字で引く。VM のコード生成は項目ごとに
    /// 凍結した領域の物体を1つ作り、`ConstString` はその物体の参照を1つ作る (docs/spec/vm.md の「値と定数」)。
    pub strings: Vec<String>,
    /// データの配置の表。`con`、`switch`、`unpack` (LIR では `release` も) が `LayoutId` で引く。translate は最初に
    /// 使った順に入れる (docs/spec/core-ir.md の「translate の配置」)。
    pub layouts: Vec<Layout>,
    /// エフェクトの表。添字は `Call::Handle`、`Call::Perform`、`Call::Clause`、基本操作の即値のエフェクトの番号で、
    /// HIR の `EffectId` の添字と同じである。
    pub effects: Vec<EffectInfo>,
    /// 表示用のパスの表。`Loc.file` が添字で引く。
    pub files: Vec<String>,
}

impl Program {
    pub fn function(&self, idx: FnIdx) -> &CoreFn {
        &self.functions[idx.0 as usize]
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreFn {
    pub name: String,
    /// 内部の関数。値として生まれる関数である。translate のラムダ、handle の本体、節と `return` の節、包む関数
    /// (`op$`、`con$`、`$externN`、`cont$`、`cont$state`) と、optimize と EP が値として作る関数 (値として生まれた関数の
    /// 複製、`k` として渡す継続の関数、`f$kN`) が当たる。ABI は決めない。ABI は型と役割 (`role`) で決まる
    /// (docs/superpowers/specs/2026-10-21-s14e-abi-struct-design.md の「関数の ABI」)。optimize の複製の名前などが
    /// この印を読む。テキストの形では `internal fn` と書く (docs/spec/core-ir.md)。
    pub internal: bool,
    /// EP のしくみの中での役割 (`eml_lir::Role`)。テキストの形では `fn` の直前に `role R` と書く。
    pub role: Role,
    /// 本体が yield しうる (docs/spec/core-ir.md の「`yields` の印」)。
    /// テキストの形では `internal` の後に `yields fn` と書く。
    pub yields: bool,
    /// in-place の入口の関数が受け持つ (エフェクト, 操作)。捕獲の後に evidence と操作の引数を受け、操作の結果を
    /// 返す。`ev_set_clause` に渡す値としてだけ参照される。テキストの形では `yields` の後に `clause E.op fn` と書く
    /// (docs/spec/lir.md の「入口の関数の印」)。
    pub clause: Option<(u32, u32)>,
    pub vars: Vec<VarInfo>,
    /// 直接の呼び出しの結果の Repr。
    pub ret: Repr,
    pub ret_mark: Mark,
    /// `blocks[0]` が入口で、その引数が関数の引数である。辺はすべて番号の大きいブロックへ向かう。
    pub blocks: Vec<Block>,
}

impl CoreFn {
    pub fn params(&self) -> &[VarId] {
        &self.blocks[BlockId::ENTRY.0 as usize].params
    }

    pub fn block(&self, id: BlockId) -> &Block {
        &self.blocks[id.0 as usize]
    }

    pub fn repr(&self, var: VarId) -> Repr {
        self.vars[var.0 as usize].repr
    }

    pub fn mark(&self, var: VarId) -> Mark {
        self.vars[var.0 as usize].mark
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    pub params: Vec<VarId>,
    pub stmts: Vec<Stmt>,
    pub term: Term,
}

impl Block {
    /// ブロックの中の変数の出現 (引数、束縛、使用、case のフィールド、着地の印) をすべて訪れる。継続の関数の変数の表を
    /// 詰めるのに使う (docs/spec/core-ir.md の「継続の関数」)。
    pub(crate) fn for_each_var_mut(&mut self, mut f: impl FnMut(&mut VarId)) {
        self.params.iter_mut().for_each(&mut f);
        for stmt in &mut self.stmts {
            match stmt {
                Stmt::Let { var, rhs } => {
                    f(var);
                    rhs.for_each_atom_mut(|atom| {
                        if let Atom::Var(var) = atom {
                            f(var);
                        }
                    });
                    // 着地の印の変数は呼び出しが使う値ではなく `for_each_atom_mut` に出ないので、ここで写す
                    if let Rhs::Call {
                        call:
                            Call::Apply {
                                prompt: Some(marker),
                                ..
                            },
                        mask: _,
                    } = rhs
                    {
                        f(marker);
                    }
                }
                Stmt::Unpack {
                    value,
                    ctor: _,
                    fields,
                } => {
                    f(value);
                    fields.iter_mut().for_each(&mut f);
                }
            }
        }
        self.term.for_each_atom_mut(|atom| {
            if let Atom::Var(var) = atom {
                f(var);
            }
        });
        if let Term::Switch { cases, .. } = &mut self.term {
            for case in cases {
                case.fields.iter_mut().for_each(&mut f);
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stmt {
    Let {
        var: VarId,
        rhs: Rhs,
    },
    /// コンストラクタが1つの型の値を分解する。行き先が「ブロックの残り」である1つの case の `switch` と同じ意味で、
    /// フィールドは `Case` のフィールドと同じ規則で束縛する。`ctor` の配置は、コンストラクタが1つで Repr が `obj`
    /// である (docs/spec/lir.md の「構造の規則」の R8)。
    Unpack {
        value: VarId,
        ctor: Ctor,
        fields: Vec<VarId>,
    },
}

impl Stmt {
    /// 文が定義する変数。
    pub fn defs(&self) -> &[VarId] {
        match self {
            Stmt::Let { var, rhs: _ } => std::slice::from_ref(var),
            Stmt::Unpack {
                value: _,
                ctor: _,
                fields,
            } => fields,
        }
    }

    /// 文が値として使う変数と定数。
    pub fn for_each_atom(&self, mut f: impl FnMut(Atom)) {
        match self {
            Stmt::Let { var: _, rhs } => rhs.for_each_atom(f),
            Stmt::Unpack {
                value,
                ctor: _,
                fields: _,
            } => f(Atom::Var(*value)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Term {
    Return(Atom),
    /// 並列な代入。実引数をすべて読んでから、行き先の引数に書く。
    Jump {
        target: BlockId,
        args: Vec<Atom>,
    },
    /// 合う case がなければ `default` に進む。行き先は引数を持たない。`layout` は、タグの case を持つ `switch` だけが
    /// 持つ (docs/spec/core-ir.md)。
    Switch {
        scrutinee: Atom,
        layout: Option<LayoutId>,
        cases: Vec<Case>,
        default: Option<BlockId>,
    },
}

impl Term {
    /// 行き先のブロック。`Switch` は case の順に、続けて `default` を返す。
    pub fn successors(&self) -> impl Iterator<Item = BlockId> + '_ {
        let (jump, cases, default) = match self {
            Term::Jump { target, args: _ } => (Some(*target), &[][..], None),
            Term::Switch {
                scrutinee: _,
                layout: _,
                cases,
                default,
            } => (None, &cases[..], *default),
            Term::Return(_) => (None, &[][..], None),
        };
        jump.into_iter()
            .chain(cases.iter().map(|case| case.target))
            .chain(default)
    }

    pub fn for_each_successor_mut(&mut self, mut f: impl FnMut(&mut BlockId)) {
        match self {
            Term::Jump { target, args: _ } => f(target),
            Term::Switch {
                scrutinee: _,
                layout: _,
                cases,
                default,
            } => {
                cases.iter_mut().for_each(|case| f(&mut case.target));
                if let Some(default) = default {
                    f(default);
                }
            }
            Term::Return(_) => {}
        }
    }

    pub fn for_each_atom(&self, mut f: impl FnMut(Atom)) {
        match self {
            Term::Return(atom) => f(*atom),
            Term::Jump { target: _, args } => args.iter().for_each(|&atom| f(atom)),
            Term::Switch {
                scrutinee,
                layout: _,
                cases: _,
                default: _,
            } => f(*scrutinee),
        }
    }

    pub fn for_each_atom_mut(&mut self, mut f: impl FnMut(&mut Atom)) {
        match self {
            Term::Return(atom) => f(atom),
            Term::Jump { target: _, args } => args.iter_mut().for_each(f),
            Term::Switch {
                scrutinee,
                layout: _,
                cases: _,
                default: _,
            } => f(scrutinee),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rhs {
    /// `mask` は呼び出しの間に飛ばす項目で、エフェクトの番号の真に昇順に並ぶ (docs/spec/core-ir.md)。
    Call { call: Call, mask: Vec<MaskItem> },
    /// 関数と先頭の引数の並びからクロージャを作る。ラムダの捕獲と部分適用は、どちらもこの形になる。
    MakeClosure(FnIdx, Vec<Atom>),
    /// extern の関数をその場で実行する。eml のコードを呼び返さず、継続のフレームも積まない。`at` を持つ呼び出しが
    /// 起こした実行時エラーには、その位置を付ける (docs/spec/core-ir.md)。
    Extern {
        ext: Extern,
        args: Vec<Atom>,
        at: Option<Loc>,
    },
    /// 文字列リテラルの参照を1つ作る。リテラルは不死の物体だが、LIR の `dup` と `decref` は釣り合いを保つ
    /// (docs/spec/lir.md の「値の表現」)。
    ConstString(u32),
    /// 引数を持つコンストラクタの値を作る。`args` の所有権は値に移る。引数のないコンストラクタの値は `Atom::Tag` で
    /// ある (docs/spec/core-ir.md)。
    Con { ctor: Ctor, args: Vec<Atom> },
    /// 値の所有権を受け取って捨てる。値は `()` である。
    Drop(Atom),
    /// evidence passing の基本操作。LIR と同じ操作で、結果は所有した値である。引数を消費するか借りるかは基本操作の表
    /// (`PrimRow::borrowed`) が決め、所有を扱う Perceus から後でだけ意味を持つ (docs/spec/lir.md の「基本操作」)。
    Prim(Prim, Vec<Atom>),
}

impl Rhs {
    /// 右辺が使う値。関数、extern、`perform` の引数は、どれも所有権を受け取る。基本操作の引数を消費するか借りるかは
    /// 基本操作の表が決め、Perceus から後でだけ意味を持つ (docs/spec/lir.md の「基本操作」)。
    pub fn for_each_atom(&self, mut f: impl FnMut(Atom)) {
        match self {
            Rhs::Call { call, mask: _ } => call.for_each_atom(f),
            Rhs::MakeClosure(_, args)
            | Rhs::Extern {
                ext: _,
                args,
                at: _,
            }
            | Rhs::Con { ctor: _, args }
            | Rhs::Prim(_, args) => args.iter().for_each(|&atom| f(atom)),
            Rhs::Drop(atom) => f(*atom),
            Rhs::ConstString(_) => {}
        }
    }

    pub fn for_each_atom_mut(&mut self, mut f: impl FnMut(&mut Atom)) {
        match self {
            Rhs::Call { call, mask: _ } => call.for_each_atom_mut(f),
            Rhs::MakeClosure(_, args)
            | Rhs::Extern {
                ext: _,
                args,
                at: _,
            }
            | Rhs::Con { ctor: _, args }
            | Rhs::Prim(_, args) => args.iter_mut().for_each(f),
            Rhs::Drop(atom) => f(atom),
            Rhs::ConstString(_) => {}
        }
    }
}

/// 呼び出し。クロージャと引数の所有権は呼び出しに移る。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Call {
    /// 呼ぶ相手が分かっていて、引数の個数が揃っている呼び出し。
    Direct(FnIdx, Vec<Atom>),
    /// 関数値の呼び出し。実行時に引数の個数を比べる (eval/apply)。`yields` は、この `apply` が受け持つ矢印の
    /// どれかが yield しうることである。translate は yield しうる矢印の後で `apply` を分けるので、それは最後の矢印
    /// である (docs/spec/core-ir.md の「`apply` の印と分け方」)。
    Apply {
        callee: Atom,
        args: Vec<Atom>,
        yields: bool,
        /// 着地の印。この呼び出しの中で始まった abort は、`prompt$` が使う evidence `v` を持つ呼び出しの
        /// フレームまで VM のスタックを巻き戻す。`v` は呼び出しが消費も読みもしないので `for_each_atom` は
        /// 見せず、呼び出しの後も生きている `obj` の変数である (docs/spec/lir.md の「着地の印」)。
        prompt: Option<VarId>,
        /// 引数を渡す規約 (`eml_lir::Site`)。
        site: Site,
    },
    /// handler を設け、本体の関数に `()` を適用する。translate だけが出し、EP が基本操作と `prompt$` の呼び出しに
    /// 置き換える (docs/spec/core-ir.md の「handle と `prompt$`」)。本体と節と `return` の節は、捕まえた変数を先頭の
    /// 引数に持つ関数の値である (docs/spec/core-ir.md)。節はエフェクトの操作の順に並ぶ。節は最後の引数で状態を、
    /// `ret` は本体の値と状態を受ける。状態のない handler では `init` が `()` である (docs/spec/core-ir.md)。
    Handle {
        effect: u32,
        init: Atom,
        body: Atom,
        clauses: Vec<Atom>,
        ret: Atom,
    },
    /// ユーザーのエフェクトの操作。translate だけが出し、EP が evidence を探す基本操作と節の呼び出しに置き換える
    /// (docs/spec/core-ir.md の「`perform` と節の3つの形」)。`resumable` は操作が `never` でないことで、変換の段の
    /// verifier がエフェクトの表と比べる。
    Perform {
        effect: u32,
        op: u32,
        resumable: bool,
        args: Vec<Atom>,
    },
    /// 継続を再開する。`state` は再開した handler の次の状態で、値は handle 式の値である。translate だけが出し、EP が
    /// `apply` に置き換える (docs/spec/core-ir.md の「再開と状態」)。
    ///
    /// `site` は再開の値と状態の Repr で、型どおりの `apply` の場所 (`eml_lir::Site::Typed`) と同じく translate が型から
    /// 決める。handler への特殊化が `resume` を `apply` に変えるときに場所として写す
    /// (docs/superpowers/specs/2026-10-21-s14e-abi-struct-design.md の「`apply` の ABI」)。
    Resume {
        k: Atom,
        arg: Atom,
        state: Atom,
        site: Vec<Repr>,
    },
    /// 操作の in-place の入口のクロージャ `callee` を呼ぶ。`args` の先頭は evidence (`obj`)、残りは操作の引数で、
    /// 引数と結果の Repr は操作の表の `params` と `ret` から取る。`mask` は持たない
    /// (docs/spec/lir.md の「節の呼び出し」)。
    Clause {
        effect: u32,
        op: u32,
        callee: Atom,
        args: Vec<Atom>,
    },
}

impl Call {
    /// 着地の印の変数。
    pub fn prompt(&self) -> Option<VarId> {
        match self {
            Call::Apply { prompt, .. } => *prompt,
            _ => None,
        }
    }

    /// 呼び出しが使う値。関数値の呼び出しでは、呼ばれる値が先に来る。
    pub fn for_each_atom(&self, mut f: impl FnMut(Atom)) {
        match self {
            Call::Direct(_, args)
            | Call::Perform {
                effect: _,
                op: _,
                resumable: _,
                args,
            } => args.iter().for_each(|&atom| f(atom)),
            Call::Apply {
                callee,
                args,
                yields: _,
                prompt: _,
                site: _,
            }
            | Call::Clause {
                effect: _,
                op: _,
                callee,
                args,
            } => {
                f(*callee);
                args.iter().for_each(|&atom| f(atom));
            }
            Call::Handle {
                effect: _,
                init,
                body,
                clauses,
                ret,
            } => {
                f(*init);
                f(*body);
                clauses.iter().for_each(|&atom| f(atom));
                f(*ret);
            }
            Call::Resume {
                k,
                arg,
                state,
                site: _,
            } => {
                f(*k);
                f(*arg);
                f(*state);
            }
        }
    }

    pub fn for_each_atom_mut(&mut self, mut f: impl FnMut(&mut Atom)) {
        match self {
            Call::Direct(_, args)
            | Call::Perform {
                effect: _,
                op: _,
                resumable: _,
                args,
            } => args.iter_mut().for_each(&mut f),
            Call::Apply {
                callee,
                args,
                yields: _,
                prompt: _,
                site: _,
            }
            | Call::Clause {
                effect: _,
                op: _,
                callee,
                args,
            } => {
                f(callee);
                args.iter_mut().for_each(&mut f);
            }
            Call::Handle {
                effect: _,
                init,
                body,
                clauses,
                ret,
            } => {
                f(init);
                f(body);
                clauses.iter_mut().for_each(&mut f);
                f(ret);
            }
            Call::Resume {
                k,
                arg,
                state,
                site: _,
            } => {
                f(k);
                f(arg);
                f(state);
            }
        }
    }
}
