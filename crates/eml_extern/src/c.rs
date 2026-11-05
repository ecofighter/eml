//! 利用者の `extern "C"` の宣言の記述。データだけを持ち、C の関数を呼ぶコードは含まない。

use crate::{FunctionRow, Mark, OpRet, OwnedRow, Purity, Repr};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Os {
    Linux,
    Macos,
    Windows,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum CLibrary {
    Named(String),
    Framework(String),
}

impl CLibrary {
    /// `link` に書いた文字列。`--link` の KEY もこの文字列で引く。
    pub fn written(&self) -> &str {
        match self {
            CLibrary::Named(name) | CLibrary::Framework(name) => name,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct CLink {
    pub os: Os,
    pub library: CLibrary,
}

/// `Int` の引数と結果に書ける C の整数の幅。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CInt {
    I64,
    I32,
    SizeT,
}

impl CInt {
    pub fn c_name(self) -> &'static str {
        match self {
            CInt::I64 => "int64_t",
            CInt::I32 => "int32_t",
            CInt::SizeT => "size_t",
        }
    }

    /// 値が C の型に収まるか。`SizeT` は eml の `Int` が符号付きなので、負の値だけを外す。
    pub fn fits(self, value: i64) -> bool {
        match self {
            CInt::I64 => true,
            CInt::I32 => i32::try_from(value).is_ok(),
            CInt::SizeT => value >= 0,
        }
    }

    /// 渡す前に範囲を確かめる必要があり、外れると実行時に中断する幅。
    pub fn narrows(self) -> bool {
        matches!(self, CInt::I32 | CInt::SizeT)
    }

    fn slot(self) -> CType {
        match self {
            CInt::I32 => CType::I32,
            CInt::I64 | CInt::SizeT => CType::I64,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CFnId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CHandleId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CParam {
    Int(CInt),
    Float,
    UnboxedArray,
    MutUnboxedArray,
    Text,
    Handle(CHandleId),
    /// 引数のない C の関数のために eml 側に置く `Unit` の引数。C には渡さない。
    NoArg,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CRet {
    Void,
    Int(CInt),
    Float,
    Handle(CHandleId),
}

/// 橋渡しが受け渡す語の型。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CType {
    I32,
    I64,
    F64,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct CSignature {
    pub params: Vec<CType>,
    pub ret: Option<CType>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CHandleType {
    pub name: String,
    pub dtor: Option<String>,
    pub links: Vec<CLink>,
}

/// 作った後で欄を書き換えると `row` が古くなるので、作るのも作り直すのも `CFunction::new` を通す。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CFunction {
    /// 正規の名前 `Module.name`。
    pub name: String,
    pub symbol: String,
    /// 宣言したモジュールの `link` の item の写し。
    pub links: Vec<CLink>,
    pub pure: bool,
    pub blocking: bool,
    pub params: Vec<CParam>,
    pub ret: CRet,
    row: OwnedRow,
    /// `blocking` の行だけが持つ、受け付けと取り出しの見え方 (docs/spec/vm.md の「`blocking` の呼び出し」)。
    submit_row: Option<OwnedRow>,
    take_row: Option<OwnedRow>,
}

impl CFunction {
    pub fn new(
        name: String,
        symbol: String,
        links: Vec<CLink>,
        pure: bool,
        blocking: bool,
        params: Vec<CParam>,
        ret: CRet,
    ) -> CFunction {
        let faults = faults(&params, ret);
        let purity = match (pure, faults) {
            (true, false) => Purity::Pure,
            (true, true) => Purity::Panics,
            (false, _) => Purity::Effectful,
        };
        let row = OwnedRow {
            name: name.clone(),
            params: params.iter().map(|&param| param_repr(param)).collect(),
            param_marks: params.iter().map(|&param| param_mark(param)).collect(),
            borrowed: params
                .iter()
                .map(|&param| param_repr(param) == Repr::Obj)
                .collect(),
            with_args: params
                .iter()
                .enumerate()
                .filter(|&(_, &param)| param_mark(param) == Mark::Lin)
                .map(|(k, _)| k)
                .collect(),
            held: vec![false; params.len()],
            ret: OpRet::Repr(match ret {
                CRet::Void => Repr::Unit,
                CRet::Int(_) => Repr::Int,
                CRet::Float => Repr::Float,
                CRet::Handle(_) => Repr::Obj,
            }),
            ret_mark: match ret {
                CRet::Handle(_) => Mark::Lin,
                CRet::Void | CRet::Int(_) | CRet::Float => Mark::Unr,
            },
            purity,
            faults,
            writes: writes(&params),
        };
        let (submit_row, take_row) = match blocking {
            true => (
                Some(submit_row(&row, &params)),
                Some(take_row(&row, &params)),
            ),
            false => (None, None),
        };
        CFunction {
            name,
            symbol,
            links,
            pure,
            blocking,
            params,
            ret,
            row,
            submit_row,
            take_row,
        }
    }

    /// C の行の見え方 (docs/spec/lir.md の「extern の行」)。
    pub fn row(&self) -> FunctionRow<'_> {
        self.row.view()
    }

    /// `blocking` の行の受け付けの見え方。`blocking` でない行は `None` である。
    pub fn submit_row(&self) -> Option<FunctionRow<'_>> {
        self.submit_row.as_ref().map(OwnedRow::view)
    }

    /// `blocking` の行の取り出しの見え方。`blocking` でない行は `None` である。
    pub fn take_row(&self) -> Option<FunctionRow<'_>> {
        self.take_row.as_ref().map(OwnedRow::view)
    }

    /// 橋渡しの引数と結果の型。配列、文字列、ハンドルはポインタ1つの語で渡し、`NoArg` は語を持たない。
    pub fn c_signature(&self) -> CSignature {
        let params = self
            .params
            .iter()
            .filter_map(|param| match param {
                CParam::Int(int) => Some(int.slot()),
                CParam::Float => Some(CType::F64),
                CParam::UnboxedArray
                | CParam::MutUnboxedArray
                | CParam::Text
                | CParam::Handle(_) => Some(CType::I64),
                CParam::NoArg => None,
            })
            .collect();
        let ret = match self.ret {
            CRet::Void => None,
            CRet::Int(int) => Some(int.slot()),
            CRet::Float => Some(CType::F64),
            CRet::Handle(_) => Some(CType::I64),
        };
        CSignature { params, ret }
    }

    /// 呼び出しが `Panic` で中断しうるか。狭める `Int as` の引数は範囲を外れうるし、ハンドルの結果は null になりうる
    /// (docs/spec/declarations.md の「型の対応」)。
    pub fn faults(&self) -> bool {
        faults(&self.params, self.ret)
    }
}

/// 受け付けは C の引数をすべて C の行と同じに借り、記録の番号 (`Std.Async.OpId`) を返す。範囲の確かめは記録を作る
/// 前に走るので、中断しうるのは狭める引数があるときだけである (docs/spec/vm.md の「`blocking` の呼び出し」)。
fn submit_row(row: &OwnedRow, params: &[CParam]) -> OwnedRow {
    let narrows = params
        .iter()
        .any(|param| matches!(param, CParam::Int(int) if int.narrows()));
    OwnedRow {
        name: row.name.clone(),
        params: row.params.clone(),
        param_marks: row.param_marks.clone(),
        borrowed: row.borrowed.clone(),
        with_args: Vec::new(),
        held: vec![false; params.len()],
        ret: OpRet::Repr(Repr::Int),
        // 記録の番号は即値なので、型の Kind によらず `unr` である (docs/spec/core-ir.md の「translate が決める印」)
        ret_mark: Mark::Unr,
        purity: Purity::Effectful,
        faults: narrows,
        writes: writes(params),
    }
}

/// 取り出しは記録の番号と `Wake` に続けて、C の引数のうち物体のものを順に借りる。物体の最後の使い道を取り出しの後に
/// 置き、プールが読む間に落とさないためである。null のハンドル、IO の失敗、取り消しをどれも取り出しで起こすので、
/// いつも中断しうる (docs/spec/vm.md の「`blocking` の呼び出し」)。
fn take_row(row: &OwnedRow, params: &[CParam]) -> OwnedRow {
    let objects: Vec<CParam> = params
        .iter()
        .copied()
        .filter(|&param| param_repr(param) == Repr::Obj)
        .collect();
    let m = objects.len();
    OwnedRow {
        name: row.name.clone(),
        params: [Repr::Int, Repr::Enum]
            .into_iter()
            .chain(std::iter::repeat_n(Repr::Obj, m))
            .collect(),
        param_marks: [Mark::Unr, Mark::Unr]
            .into_iter()
            .chain(objects.iter().map(|&param| param_mark(param)))
            .collect(),
        borrowed: [false, false]
            .into_iter()
            .chain(std::iter::repeat_n(true, m))
            .collect(),
        with_args: objects
            .iter()
            .enumerate()
            .filter(|&(_, &param)| param_mark(param) == Mark::Lin)
            .map(|(k, _)| 2 + k)
            .collect(),
        held: vec![false; 2 + m],
        ret: row.ret,
        ret_mark: row.ret_mark,
        purity: Purity::Effectful,
        faults: true,
        writes: writes(params),
    }
}

fn faults(params: &[CParam], ret: CRet) -> bool {
    let ints = params.iter().filter_map(|param| match param {
        CParam::Int(int) => Some(*int),
        _ => None,
    });
    c_faults(ints, matches!(ret, CRet::Handle(_)))
}

/// `Int` の引数の幅 `ints` と、結果がハンドルか (`handle_result`) から、呼び出しが `Panic` で中断しうるかを決める。
/// 型検査は行を作る前にこれで期待する行を決めるので、行の `faults` と食い違わない
/// (docs/spec/declarations.md の「型の対応」)。
pub fn c_faults(ints: impl IntoIterator<Item = CInt>, handle_result: bool) -> bool {
    ints.into_iter().any(CInt::narrows) || handle_result
}

/// C が借りた配列に書き込みうるか。`MutUnboxedArray` の引数だけが書ける (docs/spec/lir.md の「extern の借用」)。
pub fn writes(params: &[CParam]) -> bool {
    params.contains(&CParam::MutUnboxedArray)
}

fn param_repr(param: CParam) -> Repr {
    match param {
        CParam::Int(_) => Repr::Int,
        CParam::Float => Repr::Float,
        CParam::UnboxedArray | CParam::MutUnboxedArray | CParam::Text | CParam::Handle(_) => {
            Repr::Obj
        }
        CParam::NoArg => Repr::Unit,
    }
}

fn param_mark(param: CParam) -> Mark {
    match param {
        CParam::MutUnboxedArray | CParam::Handle(_) => Mark::Lin,
        CParam::Int(_) | CParam::Float | CParam::UnboxedArray | CParam::Text | CParam::NoArg => {
            Mark::Unr
        }
    }
}
