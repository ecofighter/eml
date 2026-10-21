//! LIR (docs/spec/lir.md)。EP の後の Core IR から写す、エフェクトの命令を持たない層で、box、RC、末尾呼び出しを
//! 明示する。VM とネイティブの共通の入力になる。

use std::collections::HashMap;
use std::fmt;

use eml_extern::{Extern, Linearity};
pub use eml_extern::{FloatBits, OpRet, Repr, StructId};

mod boxing;
pub mod check;
mod contract;
mod flatten;
pub mod liveness;
mod perceus;
mod pipeline;
mod pretty;
mod prim;
pub mod reuse;
pub mod syntax;
mod text;
pub mod unwind;
mod verify;

pub use boxing::boxing;
pub use contract::contract;
pub use flatten::flatten;
pub use perceus::perceus;
pub use pipeline::{Pass, passes_until};
pub use pretty::{pretty, pretty_with_positions};
pub use prim::{Immediates, Prim, PrimRow};
pub use reuse::reuse;
pub use text::parse;
pub use verify::{verify, verify_contracted, verify_flattened, verify_lowered, verify_scopes};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Program {
    pub functions: Vec<LirFn>,
    /// 実行の入口。`main` を `()` で呼ぶ、引数のない関数 (docs/spec/core-ir.md)。
    pub entry: FnIdx,
    /// 文字列リテラルの定数表。`ConstString` と `CasePattern::String` が添字で引く。VM のコード生成は項目ごとに
    /// 凍結した領域の物体を1つ作り、`ConstString` はその物体の参照を1つ作る (docs/spec/vm.md の「値と定数」)。
    pub strings: Vec<String>,
    /// データの配置の表。`con`、`switch`、`unpack`、`release` が `LayoutId` で引く。translate は最初に使った順に入れる
    /// (docs/spec/core-ir.md の「translate の配置」)。
    pub layouts: Vec<Layout>,
    /// 構造体の形の表。`Repr::Struct` の番号が引く。
    pub structs: Structs,
    /// エフェクトの表。添字は `Call::Clause` と基本操作の即値のエフェクトの番号で、HIR の `EffectId` の添字と同じで
    /// ある。
    pub effects: Vec<EffectInfo>,
    /// 表示用のパスの表。`Loc.file` が添字で引く。
    pub files: Vec<String>,
}

impl Program {
    pub fn function(&self, idx: FnIdx) -> &LirFn {
        &self.functions[idx.0 as usize]
    }

    /// 表にない番号なら `None` である。誤りを含む IR を表示でき、verifier がその番号を誤りとして報告できるようにする。
    pub fn layout(&self, id: LayoutId) -> Option<&Layout> {
        self.layouts.get(id.0 as usize)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LirFn {
    pub name: String,
    /// 内部の関数。値として生まれる関数である (`eml_core_ir::CoreFn::internal`)。ABI は決めず、box の挿入は読まない。
    /// ABI は引数と `ret` の型どおりの Repr と一様な位置 (`split`) で決まる。テキストの形では `internal fn` と書く。
    pub internal: bool,
    /// 一様な位置の始まり。`Some(n)` なら、`n` 番目以降の引数と結果が一様である。捕獲はその前に来る。`None` は型どおりの
    /// 関数で、一様な位置を持たない。`split` は関数を作るときに付け、名前で決まる (docs/spec/lir.md の「関数の ABI」)。
    /// テキストの形では `fn` の直前に `uniform from N` と書く。
    pub split: Option<u32>,
    /// 本体が yield しうる (docs/spec/core-ir.md の「`yields` の印」)。
    /// テキストの形では `internal` の後に `yields fn` と書く。
    pub yields: bool,
    /// in-place の入口の関数が受け持つ (エフェクト, 操作)。捕獲の後に evidence と操作の引数を受け、操作の結果を
    /// 返す。`ev_set_clause` に渡す値としてだけ参照される。テキストの形では `yields` の後に `clause E.op fn` と書く
    /// (docs/spec/lir.md の「入口の関数の印」)。
    pub clause: Option<(u32, u32)>,
    pub vars: Vec<VarInfo>,
    /// 直接の呼び出しの結果の Repr と印の並び。長さは1以上である。開くパス (flatten) の前はいつも1つで、開いた後は
    /// 構造体を返す関数がその葉を並べる (docs/spec/lir.md の「開く」)。
    /// テキストの形では、2つ以上の値を `-> (float, float)` と書く。
    pub rets: Vec<(Repr, Mark)>,
    /// 開いた後の関数の、源の単位の ABI (`SourceSig`)。開く前と、引数にも結果にも構造体のない関数では `None` で、ABI は
    /// 引数と `rets` のとおりである。
    pub source: Option<SourceSig>,
    /// `blocks[0]` が入口で、その引数が関数の引数である。辺はすべて番号の大きいブロックへ向かう。
    pub blocks: Vec<Block>,
}

/// 開くパスが構造体を葉に開いた関数の、開く前の引数の Repr と結果の Repr。構造体の Repr はそのまま残る。VM は
/// 関数の値の ABI (源の引数の単位の数と、構造体と箱の間の変換に要る形) をこれから作る。引数と `rets` は、これを葉に
/// 開いた並びと同じである (verifier が確かめる)。テキストの形では、関数の頭の結果の後に `source(Vec3{..}) -> Vec3{..}`
/// と書く (docs/spec/vm.md の「シグネチャ」)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceSig {
    pub params: Vec<Repr>,
    pub ret: Repr,
}

/// `apply` が引数を渡す規約。`Typed` は引数ごとの Repr で、その場所の矢印の型から決まる。変数の引数の Repr は変数の
/// Repr と同じで (verifier が確かめる)、定数の引数の Repr は translate が型から決める。`#0` が `enum` か `tobj` かを
/// 決めるためである。`Uniform` は EP が作る一様な場所である。結果の Repr は束縛の Repr で、末尾の位置では含む関数の
/// `ret` である (docs/spec/lir.md の「`apply` の場所」)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Site {
    Typed(Vec<Repr>),
    Uniform,
}

impl LirFn {
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

    /// 構造体の `unbox` が読む箱を、`VarId` の番号で引く表。Perceus はその箱を手放す所で `drop` か印の付いた
    /// `release` を選び、reuse と verifier は `release` の印を合わせる
    /// (docs/spec/lir.md の「ダミーの値」)。変数は1回だけ
    /// 定義されるので (R5)、関数に1つの表にする。表にない番号の変数は、verifier が別に誤りとして報告する。
    pub fn struct_boxes(&self) -> Vec<bool> {
        let mut boxes = vec![false; self.vars.len()];
        for stmt in self.blocks.iter().flat_map(|block| &block.stmts) {
            if let Stmt::UnboxStruct { value, .. } = stmt
                && let Some(slot) = boxes.get_mut(value.0 as usize)
            {
                *slot = true;
            }
        }
        boxes
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    pub params: Vec<VarId>,
    pub stmts: Vec<Stmt>,
    pub term: Term,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stmt {
    Let {
        var: VarId,
        rhs: Rhs,
    },
    /// 複数の値を返す呼び出しの結果を、値の順に束縛する。開くパスが構造体の結果を葉に開いて作り、開いた後の段だけに
    /// 現れる。テキストの形は `let (a.1: float, b.2: float) = call f(x.0)` である。値が1つの呼び出しは `Let` で書く。
    LetMany {
        vars: Vec<VarId>,
        call: Call,
    },
    /// コンストラクタが1つの型の値を分解する。行き先が「ブロックの残り」である1つの case の `switch` と同じ意味で、
    /// フィールドは `Case` のフィールドと同じ規則で束縛する。`ctor` の配置は、コンストラクタが1つで Repr が `obj`
    /// である (docs/spec/lir.md の「構造の規則」の R8)。
    Unpack {
        value: VarId,
        ctor: Ctor,
        fields: Vec<VarId>,
    },
    /// 構造体の箱 `value` のフィールドを読み、`fields` に束縛する。`None` は読まないフィールドである。フィールドは
    /// `unpack` のフィールドと同じく値から借りる。開くパスが構造体の `unbox` から作り、開いた後の段だけに現れる。
    /// テキストの形は `let (x.4: float, _) = unbox w.3 Vec3` である
    /// (docs/spec/lir.md の「開く」)。
    UnboxStruct {
        value: VarId,
        ctor: Ctor,
        fields: Vec<Option<VarId>>,
    },
    Dup(VarId),
    Decref(VarId),
    /// `release x L #t(p1, .., pn)`。Perceus だけが入れる RC の命令である。分解した値 `x` の参照を1つ手放し、名前を
    /// 書いた位置の変数が、そのフィールドの参照を1つずつ受け取る。`None` は残さない位置である
    /// (docs/spec/lir.md)。
    /// `token` を持つ形 (`let r: token = release ..`) は reuse のパスが作る。x が一意なら箱を token `r` にし、そうでなければ
    /// 空の token を定義する。残す位置がなくてもよいのはこの形だけである (docs/spec/object-model.md の「reuse」)。
    Release {
        value: VarId,
        ctor: Ctor,
        fields: Vec<Option<VarId>>,
        token: Option<VarId>,
        /// 構造体の `unbox` で葉を借りた箱を手放す印。VM はダミーを受けても何もしない別の命令にする。ふつうの
        /// `release` の速い道に確かめの分岐を足さないためである。テキストの形では `release unboxed` と書く
        /// (docs/spec/lir.md の「ダミーの値」)。
        unboxed: bool,
    },
    /// `drop_token r`。使わなかった token を手放す。
    DropToken(VarId),
}

impl Stmt {
    /// 文が定義する変数。
    pub fn defs(&self) -> impl Iterator<Item = VarId> + '_ {
        let (vars, fields): (&[VarId], &[Option<VarId>]) = match self {
            Stmt::Let { var, rhs: _ } => (std::slice::from_ref(var), &[]),
            Stmt::LetMany { vars, call: _ } => (vars, &[]),
            Stmt::Unpack {
                value: _,
                ctor: _,
                fields,
            } => (fields, &[]),
            Stmt::UnboxStruct {
                value: _,
                ctor: _,
                fields,
            } => (&[], fields),
            Stmt::Release {
                token: Some(token), ..
            } => (std::slice::from_ref(token), &[]),
            Stmt::Dup(_) | Stmt::Decref(_) | Stmt::Release { .. } | Stmt::DropToken(_) => {
                (&[], &[])
            }
        };
        vars.iter().copied().chain(fields.iter().flatten().copied())
    }

    /// 文が値として使う変数と定数。`Dup`、`Decref`、`Release` は Perceus の命令なので、値の使用に数えない。token は
    /// 値でないので、`DropToken` も数えない。
    pub fn for_each_atom(&self, mut f: impl FnMut(Atom)) {
        match self {
            Stmt::Let { var: _, rhs } => rhs.for_each_atom(f),
            Stmt::LetMany { vars: _, call } => call.for_each_atom(f),
            Stmt::Unpack { value, .. } | Stmt::UnboxStruct { value, .. } => f(Atom::Var(*value)),
            Stmt::Dup(_) | Stmt::Decref(_) | Stmt::Release { .. } | Stmt::DropToken(_) => {}
        }
    }

    /// 値の使いのうち、参照を1つ受け取る「消費」。`unpack` と `unbox` の値は読むだけなので数えない
    /// (docs/spec/lir.md)。
    pub fn for_each_consumed(&self, f: impl FnMut(Atom)) {
        match self {
            Stmt::Let { var: _, rhs } => rhs.for_each_consumed(f),
            Stmt::LetMany { vars: _, call } => call.for_each_atom(f),
            Stmt::Unpack { .. }
            | Stmt::UnboxStruct { .. }
            | Stmt::Dup(_)
            | Stmt::Decref(_)
            | Stmt::Release { .. }
            | Stmt::DropToken(_) => {}
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Term {
    Return(Atom),
    /// 複数の値を返す。開くパスが構造体を返す `return` から作り、開いた後の段だけに現れる。テキストの形は
    /// `return (a.1, b.2)` である。
    ReturnMany(Vec<Atom>),
    /// 縮約が作る、末尾の呼び出しの要求。
    TailCall(Call),
    /// 並列な代入。実引数をすべて読んでから、行き先の引数に書く。
    Jump {
        target: BlockId,
        args: Vec<Atom>,
    },
    /// 合う case がなければ `default` に進む。行き先は引数を持たない。`layout` は、タグの case を持つ `switch` だけが
    /// 持つ (docs/spec/lir.md)。
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
            Term::Return(_) | Term::ReturnMany(_) | Term::TailCall(_) => (None, &[][..], None),
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
            Term::Return(_) | Term::ReturnMany(_) | Term::TailCall(_) => {}
        }
    }

    pub fn for_each_atom(&self, mut f: impl FnMut(Atom)) {
        match self {
            Term::Return(atom) => f(*atom),
            Term::TailCall(call) => call.for_each_atom(f),
            Term::ReturnMany(args) | Term::Jump { target: _, args } => {
                args.iter().for_each(|&atom| f(atom))
            }
            Term::Switch {
                scrutinee,
                layout: _,
                cases: _,
                default: _,
            } => f(*scrutinee),
        }
    }

    /// 値の使いのうち、参照を1つ受け取る「消費」。`switch` の scrutinee は読むだけなので数えない
    /// (docs/spec/lir.md)。
    pub fn for_each_consumed(&self, f: impl FnMut(Atom)) {
        if !matches!(self, Term::Switch { .. }) {
            self.for_each_atom(f);
        }
    }

    pub fn for_each_atom_mut(&mut self, mut f: impl FnMut(&mut Atom)) {
        match self {
            Term::Return(atom) => f(atom),
            Term::TailCall(call) => call.for_each_atom_mut(f),
            Term::ReturnMany(args) | Term::Jump { target: _, args } => args.iter_mut().for_each(f),
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
    Call(Call),
    /// 関数と先頭の引数の並びからクロージャを作る。ラムダの捕獲と部分適用は、どちらもこの形になる。
    MakeClosure(FnIdx, Vec<Atom>),
    /// extern の関数をその場で実行する。eml のコードを呼び返さず、継続のフレームも積まない。`at` を持つ呼び出しが
    /// 起こした実行時エラーには、その位置を付ける (docs/spec/lir.md)。
    Extern {
        ext: Extern,
        args: Vec<Atom>,
        at: Option<Loc>,
    },
    /// 文字列リテラルの参照を1つ作る。リテラルは不死の物体だが、`dup` と `decref` の釣り合いは保つ
    /// (docs/spec/lir.md の「値の表現」)。
    ConstString(u32),
    /// 引数を持つコンストラクタの値を作る。`args` の所有権は値に移る。引数のないコンストラクタの値は `Atom::Tag` で
    /// ある (docs/spec/lir.md)。`reuse` を持つ `con` は、token が空でなければその箱に作る。token は値でないので、
    /// `for_each_atom` は見せない。
    Con {
        ctor: Ctor,
        args: Vec<Atom>,
        reuse: Option<Reuse>,
    },
    /// 値の所有権を受け取って捨てる。値は `()` である。
    Drop(Atom),
    /// 箱を要する値 (`Repr::needs_box`) か、`Int` か `Float` の定数を箱に入れる。オペランドを消費し、所有した箱を作る。
    /// 箱の種類はオペランドの Repr で決まる (docs/spec/lir.md の「値の表現」)。スカラーの箱は `tobj` で、構造体の箱は
    /// 配置の `con` と同じ形のヒープの物体なので `obj` である
    /// (docs/spec/lir.md の「値の表現」)。
    Box(Atom),
    /// 箱から、箱を要する値を取り出す。`switch` の scrutinee と同じく値を読むだけで、所有権を受け取らない。取り出す値の
    /// 種類は束縛する変数の Repr で決まる。スカラーは `tobj` から、構造体は `obj` か `tobj` から取り出す。
    Unbox(Atom),
    /// evidence passing の基本操作。借用の印 (`PrimRow::borrowed`) が立つ引数は読むだけで、ほかの引数は extern と同じく
    /// 消費する。結果は所有した値である (docs/spec/lir.md の「基本操作」)。
    Prim(Prim, Vec<Atom>),
}

impl Rhs {
    /// 右辺が使う値。消費する使いと読む使いの区別は `for_each_consumed` が決める。
    pub fn for_each_atom(&self, mut f: impl FnMut(Atom)) {
        match self {
            Rhs::Call(call) => call.for_each_atom(f),
            Rhs::MakeClosure(_, args)
            | Rhs::Extern {
                ext: _,
                args,
                at: _,
            }
            | Rhs::Con {
                ctor: _,
                args,
                reuse: _,
            }
            | Rhs::Prim(_, args) => args.iter().for_each(|&atom| f(atom)),
            Rhs::Drop(atom) | Rhs::Box(atom) | Rhs::Unbox(atom) => f(*atom),
            Rhs::ConstString(_) => {}
        }
    }

    /// 値の使いのうち、参照を1つ受け取る「消費」。`unbox` の値と、基本操作と extern の借用の引数は読むだけなので
    /// 数えない (docs/spec/lir.md の「ブロックの列」「基本操作」「extern の借用」)。
    pub fn for_each_consumed(&self, mut f: impl FnMut(Atom)) {
        let mut unborrowed = |args: &[Atom], borrowed: &[bool]| {
            for (&atom, &borrowed) in args.iter().zip(borrowed) {
                if !borrowed {
                    f(atom);
                }
            }
        };
        match self {
            Rhs::Unbox(_) => {}
            Rhs::Prim(prim, args) => unborrowed(args, prim.row().borrowed),
            Rhs::Extern { ext, args, at: _ } => unborrowed(args, ext.row().borrowed),
            _ => self.for_each_atom(f),
        }
    }

    pub fn for_each_atom_mut(&mut self, mut f: impl FnMut(&mut Atom)) {
        match self {
            Rhs::Call(call) => call.for_each_atom_mut(f),
            Rhs::MakeClosure(_, args)
            | Rhs::Extern {
                ext: _,
                args,
                at: _,
            }
            | Rhs::Con {
                ctor: _,
                args,
                reuse: _,
            }
            | Rhs::Prim(_, args) => args.iter_mut().for_each(f),
            Rhs::Drop(atom) | Rhs::Box(atom) | Rhs::Unbox(atom) => f(atom),
            Rhs::ConstString(_) => {}
        }
    }
}

/// `con` が受け取る token と、その箱の使い方。どちらにするかも、どの引数を書かないかも LIR に書き、VM とネイティブの
/// コード生成が同じ判断に従うようにする (docs/spec/object-model.md の「reuse」)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reuse {
    pub token: VarId,
    /// ヘッダを書かずに使う (`reuse_in_place`)。偽ならヘッダを書き直す (`con_at`)。
    pub in_place: bool,
    /// 引数ごとの、書かない印。長さは `con` の引数の数である。箱は中身を消さないので、元の値から同じ物理的な位置に
    /// 取り出した変数は、書かなくてもその位置に残っている。
    pub skip: Vec<bool>,
}

/// 呼び出し。クロージャと引数の所有権は呼び出しに移る。LIR の呼び出しは `mask` を持たない
/// (docs/spec/core-ir.md の「`mask` の包み」)。
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
        /// 引数を渡す規約 (`Site`)。型どおりの場所は源の引数ごとに Repr を1つ持つ。開いた後は、構造体の Repr の
        /// 引数が葉に開いた複数のアトムになる。
        site: Site,
        /// 開いた後の構造体の結果の、源の Repr。開く前と、結果が構造体でない場所では `None` で、結果の Repr は束縛の
        /// Repr (末尾の位置では関数の結果) である。テキストの形は `source(..)` の後の `-> Vec3{..}` である。
        source_ret: Option<Repr>,
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
            Call::Direct(_, args) => args.iter().for_each(|&atom| f(atom)),
            Call::Apply {
                callee,
                args,
                yields: _,
                prompt: _,
                site: _,
                source_ret: _,
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
        }
    }

    pub fn for_each_atom_mut(&mut self, mut f: impl FnMut(&mut Atom)) {
        match self {
            Call::Direct(_, args) => args.iter_mut().for_each(f),
            Call::Apply {
                callee,
                args,
                yields: _,
                prompt: _,
                site: _,
                source_ret: _,
            }
            | Call::Clause {
                effect: _,
                op: _,
                callee,
                args,
            } => {
                f(callee);
                args.iter_mut().for_each(f);
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LayoutId(pub u32);

/// `mask` と `evv_mask` の項目。`effect` の evidence のうち、先に同じ `effect` が `keep` 個ある所から `skip` 個を
/// 飛ばす (docs/spec/core-ir.md の「`mask` の包み」)。`skip` は 1 以上で、並びは `effect` の真に昇順である。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MaskItem {
    pub effect: u32,
    pub keep: u32,
    pub skip: u32,
}

/// `con`、`unpack`、`release` が指すコンストラクタ。タグは配置の中の添字で、宣言の順である。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Ctor {
    pub layout: LayoutId,
    pub tag: u32,
}

/// data の型1つか、タプルの要素の数1つの配置 (docs/spec/lir.md の「データの配置」)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    /// テキストの形で引く名前。関数と同じく、入口以外のモジュールの型は修飾する (`Prelude.Bool`)。タプルは `(,)`。
    pub name: String,
    /// タグの順のコンストラクタ。
    pub constructors: Vec<LayoutCtor>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayoutCtor {
    pub name: String,
    /// 宣言したフィールドの型の Repr。型変数は `tobj` で、具体化した型は見ない。
    pub fields: Vec<Repr>,
}

impl Layout {
    /// 値の Repr。型の Repr と同じ規則で、コンストラクタのフィールドの数だけから決まる。
    pub fn repr(&self) -> Repr {
        data_repr(self.constructors.iter().map(|ctor| ctor.fields.len()))
    }
}

/// コンストラクタのフィールドの物理的な形 (docs/spec/object-model.md の「ペイロード」)。走査する語 (`obj` と `tobj`)
/// を宣言の順に並べ、その後に残りを宣言の順に並べる。VM と reuse のパスが同じ規則を使うように、規則はここに
/// 1つだけ置く。走査する語が 255 個以上のときにペイロードの先頭に置く数の語は `eml_rt` の形なので、ここでは数えない。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shape {
    /// 宣言の位置ごとの、ペイロードの中の位置。
    pub positions: Vec<u32>,
    pub scanned: u32,
    pub raw: u32,
}

impl Shape {
    /// reuse の組を作るときに比べる、ペイロードの物理的な語の数。走査する語が 255 個以上の形は、`eml_rt` だけが知る
    /// 数の語を持つので、組に入れない (docs/spec/object-model.md の「ヘッダ」)。
    pub fn reuse_words(&self) -> Option<u32> {
        (self.scanned < 255).then_some(self.scanned + self.raw)
    }
}

impl LayoutCtor {
    pub fn shape(&self) -> Shape {
        let scanned = self.fields.iter().filter(|repr| repr.is_rc()).count() as u32;
        let (mut next_scanned, mut next_raw) = (0, scanned);
        let positions = self
            .fields
            .iter()
            .map(|repr| {
                let next = if repr.is_rc() {
                    &mut next_scanned
                } else {
                    &mut next_raw
                };
                let position = *next;
                *next += 1;
                position
            })
            .collect();
        Shape {
            positions,
            scanned,
            raw: self.fields.len() as u32 - scanned,
        }
    }
}

/// 構造体の形。配置と、フィールドごとの Repr と Kind の印を持つ。フィールドの Repr は具体化した型から決まり、入れ子の
/// 構造体のフィールドは `Repr::Struct` になる。配置の宣言した Repr とは別に持つ
/// (docs/spec/lir.md の「構造体の形」)。
/// テキストの形は `Vec3{float, float, float}`、`(,){int, obj lin}` である。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StructShape {
    pub layout: LayoutId,
    pub fields: Vec<(Repr, Mark)>,
}

/// 構造体の形の表。同じ中身の形には同じ番号を付ける。形は具体化した型から決まり、型は instance の名前に入って
/// いるので、名前が同じ関数はいつも同じ ABI になる。番号は1つの Program の中だけで意味を持つ
/// (docs/spec/lir.md の「構造体の形」)。
#[derive(Clone, Default)]
pub struct Structs {
    shapes: Vec<StructShape>,
    ids: HashMap<StructShape, StructId>,
}

impl Structs {
    pub fn intern(&mut self, shape: StructShape) -> StructId {
        if let Some(&id) = self.ids.get(&shape) {
            return id;
        }
        let id = StructId(self.shapes.len() as u32);
        self.shapes.push(shape.clone());
        self.ids.insert(shape, id);
        id
    }

    pub fn shape(&self, id: StructId) -> &StructShape {
        &self.shapes[id.0 as usize]
    }

    /// 番号の順の形。
    pub fn shapes(&self) -> &[StructShape] {
        &self.shapes
    }

    /// 形の葉を深さ優先にフィールドの順で並べる。`unit` の葉も位置を保つために含める。入れ子の構造体の葉の印は、
    /// 葉の印とフィールドの印の小さいほうにする。構造体の変数を開くときの葉の印の規則を、入れ子のフィールドにも
    /// 当てたものである (docs/spec/lir.md の「構造体の形」)。
    /// 再帰は形の深さだけである。
    pub fn leaves(&self, id: StructId) -> Vec<(Repr, Mark)> {
        let mut leaves = Vec::new();
        self.push_leaves(id, Mark::Lin, &mut leaves);
        leaves
    }

    fn push_leaves(&self, id: StructId, bound: Mark, leaves: &mut Vec<(Repr, Mark)>) {
        for &(repr, mark) in &self.shape(id).fields {
            let mark = mark.meet(bound);
            match repr {
                Repr::Struct(inner) => self.push_leaves(inner, mark, leaves),
                _ => leaves.push((repr, mark)),
            }
        }
    }

    /// 開いた後の葉の語の数。`unit` の葉は語を持たないので数えない。
    pub fn words(&self, id: StructId) -> usize {
        self.leaves(id)
            .iter()
            .filter(|(repr, _)| *repr != Repr::Unit)
            .count()
    }

    /// 印が `unr` でない、RC の対象の葉を持つか。「RC の対象」で後始末を決めている所は、この述語が真の構造体も含める
    /// (docs/spec/lir.md の「構造体の形」)。
    pub fn has_non_unr_rc_leaf(&self, id: StructId) -> bool {
        self.leaves(id)
            .iter()
            .any(|&(repr, mark)| repr.is_rc() && mark != Mark::Unr)
    }

    /// 「RC の対象」で後始末を決める所が後始末する値の Repr。RC の対象の Repr と、`unr` でない RC の葉を持つ構造体
    /// である (docs/spec/lir.md の「構造体の形」)。
    pub fn rc_target(&self, repr: Repr) -> bool {
        match repr {
            Repr::Struct(id) => self.has_non_unr_rc_leaf(id),
            _ => repr.is_rc(),
        }
    }

    /// 印を除いて同じ Repr。構造体どうしは骨組み (配置と、葉の Repr) が同じときに等しい。正確な位置で Repr を比べる
    /// 所が使う。状態を渡す形の複製が値の印から作る形と、型から作る形とで、印だけが違うことがあるためである
    /// (docs/spec/lir.md の「構造体の形」)。
    pub fn same(&self, a: Repr, b: Repr) -> bool {
        match (a, b) {
            (Repr::Struct(a), Repr::Struct(b)) => {
                let (a, b) = (self.shape(a), self.shape(b));
                a.layout == b.layout
                    && a.fields.len() == b.fields.len()
                    && a.fields
                        .iter()
                        .zip(&b.fields)
                        .all(|(&(a, _), &(b, _))| self.same(a, b))
            }
            _ => a == b,
        }
    }

    /// 互換の位置で命令なしで値を渡せる2つの Repr。構造体どうしは骨組みが同じときだけ互換で、印は葉ごとに印の
    /// 規則で比べる。ほかは `Repr::compatible` と同じである
    /// (docs/spec/lir.md の「構造体の形」)。
    pub fn compatible(&self, a: Repr, b: Repr) -> bool {
        match (a, b) {
            (Repr::Struct(_), Repr::Struct(_)) => self.same(a, b),
            _ => a.compatible(b),
        }
    }
}

/// 番号は中身から決まるので、等しさは番号の順の形で比べる。
impl PartialEq for Structs {
    fn eq(&self, other: &Structs) -> bool {
        self.shapes == other.shapes
    }
}

impl Eq for Structs {}

/// 番号から形を引く表は形の並びから決まるので、形の並びだけを書く。`HashMap` を書かないので、表示の順も決まる。
impl fmt::Debug for Structs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(&self.shapes).finish()
    }
}

/// data の値の Repr。フィールドのないコンストラクタだけなら `enum`、どれもフィールドを持てば `obj`、混ざれば `tobj`
/// である。コンストラクタのない型は `enum` になる。
pub fn data_repr(arities: impl IntoIterator<Item = usize>) -> Repr {
    let (mut count, mut with_fields) = (0, 0);
    for arity in arities {
        count += 1;
        with_fields += usize::from(arity > 0);
    }
    if with_fields == 0 {
        Repr::Enum
    } else if with_fields == count {
        Repr::Obj
    } else {
        Repr::TObj
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectInfo {
    pub name: String,
    /// 宣言した順の操作。`Call::Handle` の節と `Call::Perform` の操作の番号は、この順の添字である。
    pub operations: Vec<OperationInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationInfo {
    pub name: String,
    /// 操作の引数の Repr。型変数は `tobj` にする (docs/spec/lir.md の
    /// 「エフェクトの位置」)。verifier は長さで節の関数の引数の数を確かめる (docs/spec/lir.md)。
    pub params: Vec<Repr>,
    pub ret: OpRet,
    /// 引数と戻り値の印。宣言の型を、宣言に対して相対的な型も読める読み方で読んだ値である。`Rigid` に値が依存する
    /// 位置は `poly` になる。`never` の操作の戻り値は戻らないので `poly` に決める
    /// (docs/spec/core-ir.md の「表の印の列」)。
    pub param_marks: Vec<Mark>,
    pub ret_mark: Mark,
    /// `never` の操作は再開しないので、継続を作らずに捨てる (docs/spec/effects.md)。
    pub resumable: bool,
}

/// エフェクトの表の操作。番号が表にあることを verifier が確かめた後のパスが使う。関数の並びを書き換えながら引けるように、
/// `Program` でなく表だけを借りる。
pub fn operation(effects: &[EffectInfo], effect: u32, op: u32) -> &OperationInfo {
    &effects[effect as usize].operations[op as usize]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FnIdx(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct VarId(pub u32);

/// 値が `Lin` を含みうるかを表す印 (docs/spec/lir.md の「Kind の印」)。
/// 順序を持つ型にしないのは、結びの順 (`Lin` > `Poly` > `Unr`) と宣言の順を取り違えないため。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Mark {
    /// 共有してよい。
    Unr,
    /// 共有してはならず、ちょうど1回消費する。
    Lin,
    /// 型変数の位置で、`Lin` かもしれない。
    Poly,
}

impl Mark {
    pub fn name(self) -> &'static str {
        match self {
            Mark::Unr => "unr",
            Mark::Lin => "lin",
            Mark::Poly => "poly",
        }
    }

    pub fn join(self, other: Mark) -> Mark {
        match (self, other) {
            (Mark::Lin, _) | (_, Mark::Lin) => Mark::Lin,
            (Mark::Poly, _) | (_, Mark::Poly) => Mark::Poly,
            _ => Mark::Unr,
        }
    }

    /// 送り手の値を受け手に渡せるか。`lin` ビットを落とす向き (`Lin` を `Unr` の受け手へ) だけが渡せない。
    /// `Unr` を `Lin` の受け手へ渡すのは、ビットが立たないだけで安全な側である。`Poly` はどちらの向きも通す。
    pub fn passes_to(sender: Mark, receiver: Mark) -> bool {
        !matches!((sender, receiver), (Mark::Lin, Mark::Unr))
    }

    /// 2つの印の小さいほう。順序は `Unr` < `Poly` < `Lin`。
    pub fn meet(self, other: Mark) -> Mark {
        if self.at_least(other) { other } else { self }
    }

    /// `self` が `other` 以上か (`self >= other`)。順序は `Unr` < `Poly` < `Lin`。
    pub fn at_least(self, other: Mark) -> bool {
        self.rank() >= other.rank()
    }

    fn rank(self) -> u8 {
        match self {
            Mark::Unr => 0,
            Mark::Poly => 1,
            Mark::Lin => 2,
        }
    }
}

/// extern の行の Kind の列。extern は多相にならないので、`Poly` には写らない。
impl From<Linearity> for Mark {
    fn from(kind: Linearity) -> Mark {
        match kind {
            Linearity::Unr => Mark::Unr,
            Linearity::Lin => Mark::Lin,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VarInfo {
    pub name: String,
    pub repr: Repr,
    pub mark: Mark,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BlockId(pub u32);

impl BlockId {
    pub const ENTRY: BlockId = BlockId(0);
}

/// `switch` の case。引数を持つコンストラクタの case は、すべてのフィールドを順に束縛する。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Case {
    pub pattern: CasePattern,
    pub fields: Vec<VarId>,
    pub target: BlockId,
}

/// case が比べる値。`String` は文字列定数の表 (`Program::strings`) の番号である。リテラルを1つの `switch` に並べ、
/// 比べる命令の連なりで深くしないため (docs/spec/core-ir.md)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CasePattern {
    Tag(u32),
    Int(i64),
    String(u32),
}

/// ソースの位置。`file` は `Program.files` の添字で、`line` と `column` は1から数える (`column` は文字の位置)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Loc {
    pub file: u32,
    pub line: u32,
    pub column: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Atom {
    Var(VarId),
    Int(i64),
    Float(FloatBits),
    Unit,
    Tag(u32),
    /// 捕まえた変数のない関数の値。クロージャを確保しない。RC の対象ではない (docs/spec/core-ir.md)。
    Fn(FnIdx),
}

impl Atom {
    /// 変数は Repr が同じとき、定数はその Repr の値になれるときに、`vars` の関数の中で `expected` の位置に収まる (R7)。
    pub fn fits(self, vars: &[VarInfo], expected: Repr) -> bool {
        match self {
            Atom::Var(var) => vars[var.0 as usize].repr == expected,
            Atom::Int(_) => expected == Repr::Int,
            Atom::Float(_) => expected == Repr::Float,
            Atom::Unit => expected == Repr::Unit,
            // 引数のないコンストラクタは、`enum` のタグにも、`tobj` の即値にもなる
            Atom::Tag(_) => matches!(expected, Repr::Enum | Repr::TObj),
            Atom::Fn(_) => expected == Repr::TObj,
        }
    }

    /// 互換の位置 (`jump`、`return`、呼び出しのオペランド、`closure` の引数、`tobj` のフィールド) に、命令なしで
    /// 収まる値。変数は Repr が互換なときに収まる。定数は `fits` に加えて、`()` が `tobj` にも収まる。`Int` と `Float`
    /// の定数は `tobj` に収まらないので、box の挿入が `box` する (docs/spec/lir.md の「値の表現」)。box の挿入と verifier が
    /// 同じ規則で位置を比べるように、ここで1回だけ定義する。構造体の変数は `Structs::compatible` で比べる。
    pub fn fits_compatible(self, vars: &[VarInfo], structs: &Structs, expected: Repr) -> bool {
        match self {
            Atom::Var(var) => structs.compatible(vars[var.0 as usize].repr, expected),
            Atom::Unit => expected.compatible(Repr::Unit),
            Atom::Int(_) | Atom::Float(_) | Atom::Tag(_) | Atom::Fn(_) => self.fits(vars, expected),
        }
    }
}

/// `Bool` のタグ (docs/spec/core-ir.md)。
pub const FALSE: u32 = 0;
pub const TRUE: u32 = 1;

/// `Ordering` のタグ。`compare` の extern が配置の表を見ずに作る
/// (docs/spec/declarations.md の「Prelude のクラス」)。
pub const LT: u32 = 0;
pub const EQ: u32 = 1;
pub const GT: u32 = 2;

/// タプルの値のタグ。タプルは、コンストラクタが1つの `data` と同じオブジェクトで表す (docs/spec/core-ir.md)。
pub const TUPLE: u32 = 0;

/// `IOErrorKind` のタグ。`eml_rt::externs` と同じ値で、Prelude の宣言の順である (`eml_core_ir` の `tests/externs.rs` が
/// 確かめる)。
pub const IO_NOT_FOUND: u32 = 0;
pub const IO_PERMISSION_DENIED: u32 = 1;
pub const IO_NOT_AVAILABLE: u32 = 2;
pub const IO_NOT_UTF8: u32 = 3;
pub const IO_OTHER: u32 = 4;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifyError {
    pub function: String,
    pub message: String,
}

impl fmt::Display for VerifyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} in `{}`", self.message, self.function)
    }
}

impl std::error::Error for VerifyError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    /// 1 から数える行の番号。
    pub line: usize,
    pub message: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

impl std::error::Error for ParseError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn shape(layout: u32, fields: &[(Repr, Mark)]) -> StructShape {
        StructShape {
            layout: LayoutId(layout),
            fields: fields.to_vec(),
        }
    }

    #[test]
    fn structs_intern_by_content() {
        let mut structs = Structs::default();
        let vec3 = || shape(0, &[(Repr::Float, Mark::Unr); 3]);
        let a = structs.intern(vec3());
        let b = structs.intern(vec3());
        assert_eq!(a, b);
        assert_eq!(*structs.shape(a), vec3());
        // 印だけが違う形も、中身が違うので別の番号になる
        let lin = structs.intern(shape(1, &[(Repr::Int, Mark::Unr), (Repr::Obj, Mark::Lin)]));
        let poly = structs.intern(shape(1, &[(Repr::Int, Mark::Unr), (Repr::Obj, Mark::Poly)]));
        assert_ne!(lin, poly);
        assert_ne!(a, lin);
        assert_eq!(structs.intern(vec3()), a);
    }

    #[test]
    fn compatibility_ignores_marks() {
        let mut structs = Structs::default();
        let lin = Repr::Struct(
            structs.intern(shape(0, &[(Repr::Int, Mark::Unr), (Repr::Obj, Mark::Lin)])),
        );
        let poly = Repr::Struct(
            structs.intern(shape(0, &[(Repr::Int, Mark::Unr), (Repr::Obj, Mark::Poly)])),
        );
        assert!(structs.compatible(lin, poly) && structs.compatible(poly, lin));
        // `Repr::compatible` は形を見ないので、番号の違う構造体を互換と言わない
        assert!(!lin.compatible(poly) && lin.compatible(lin));
        let int = Repr::Struct(
            structs.intern(shape(0, &[(Repr::Int, Mark::Unr), (Repr::Obj, Mark::Unr)])),
        );
        let float = Repr::Struct(structs.intern(shape(
            0,
            &[(Repr::Float, Mark::Unr), (Repr::Obj, Mark::Unr)],
        )));
        assert!(!structs.compatible(int, float));
        // 配置の違う形と、入れ子の形の骨組みも比べる
        let other = Repr::Struct(
            structs.intern(shape(1, &[(Repr::Int, Mark::Unr), (Repr::Obj, Mark::Unr)])),
        );
        assert!(!structs.compatible(int, other));
        let outer = |structs: &mut Structs, inner: Repr| {
            Repr::Struct(structs.intern(shape(2, &[(inner, Mark::Poly), (Repr::Unit, Mark::Unr)])))
        };
        let (a, b, c) = (
            outer(&mut structs, lin),
            outer(&mut structs, poly),
            outer(&mut structs, float),
        );
        assert!(structs.compatible(a, b) && !structs.compatible(a, c));
        // 構造体は `obj` とも `tobj` とも命令なしで行き来しない。構造体でない Repr は `Repr::compatible` のままである
        for word in [Repr::Obj, Repr::TObj, Repr::Unit] {
            assert!(!structs.compatible(int, word) && !structs.compatible(word, int));
        }
        assert!(
            structs.compatible(Repr::Obj, Repr::TObj) && !structs.compatible(Repr::Int, Repr::TObj)
        );
    }

    #[test]
    fn nested_leaves_are_depth_first() {
        let mut structs = Structs::default();
        let inner = structs.intern(shape(
            0,
            &[
                (Repr::Float, Mark::Unr),
                (Repr::Unit, Mark::Unr),
                (Repr::Obj, Mark::Lin),
            ],
        ));
        let outer = structs.intern(shape(
            1,
            &[
                (Repr::Int, Mark::Unr),
                (Repr::Struct(inner), Mark::Poly),
                (Repr::Enum, Mark::Unr),
            ],
        ));
        // 入れ子の葉の印は、葉の印とフィールドの印の小さいほうである
        assert_eq!(
            structs.leaves(outer),
            [
                (Repr::Int, Mark::Unr),
                (Repr::Float, Mark::Unr),
                (Repr::Unit, Mark::Unr),
                (Repr::Obj, Mark::Poly),
                (Repr::Enum, Mark::Unr),
            ]
        );
        assert_eq!(structs.words(inner), 2);
        assert_eq!(structs.words(outer), 4);
        assert!(structs.has_non_unr_rc_leaf(inner) && structs.has_non_unr_rc_leaf(outer));
        let unr = structs.intern(shape(2, &[(Repr::Obj, Mark::Unr), (Repr::Int, Mark::Unr)]));
        assert!(!structs.has_non_unr_rc_leaf(unr));
        // RC の対象でない葉の印は見ない
        let lin_scalar =
            structs.intern(shape(2, &[(Repr::Int, Mark::Lin), (Repr::Unit, Mark::Unr)]));
        assert!(!structs.has_non_unr_rc_leaf(lin_scalar));
    }
}
