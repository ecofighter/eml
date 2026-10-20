//! シグネチャ。関数と `apply` の場所の、引数と結果の Repr の並びである。`CodeBase` の中で中身から一意な番号にするので、
//! 追記の間も番号は変わらない (docs/superpowers/specs/2026-10-21-s14e-abi-struct-design.md の「VM」)。

use std::collections::HashMap;

use eml_lir::Repr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct SigId(pub(crate) u32);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct Signature {
    pub(crate) params: Vec<Repr>,
    pub(crate) ret: Vec<Repr>,
}

/// 変換の種類。`apply` は Repr の等しさでなく種類で比べる。`obj` と `tobj` が違うだけで遅い道に入らないためである
/// (docs/superpowers/specs/2026-10-21-s14e-abi-struct-design.md の「VM」)。構造体の種類は構造体の Repr と一緒に足す。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Class {
    /// 語のまま渡せるもの (`obj`、`tobj`、`unit`)。
    Word,
    Int,
    Float,
    Enum,
}

impl Class {
    /// 逆アセンブラと一致の確かめが変換を中身で書くための名前。
    pub(crate) fn name(self) -> &'static str {
        match self {
            Class::Word => "word",
            Class::Int => "int",
            Class::Float => "float",
            Class::Enum => "enum",
        }
    }

    /// 箱を要するスカラーの種類。語のまま渡せる種類には `None` を返す。
    pub(crate) fn scalar(self) -> Option<Scalar> {
        match self {
            Class::Word => None,
            Class::Int => Some(Scalar::Int),
            Class::Float => Some(Scalar::Float),
            Class::Enum => Some(Scalar::Enum),
        }
    }
}

/// 箱を要するスカラーの種類。変換の命令が持つ。構造体の種類は持たないので、命令の 8 バイトに収まる。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Scalar {
    Int,
    Float,
    Enum,
}

impl Scalar {
    pub(crate) const ALL: [Scalar; 3] = [Scalar::Int, Scalar::Float, Scalar::Enum];

    pub(crate) fn class(self) -> Class {
        match self {
            Scalar::Int => Class::Int,
            Scalar::Float => Class::Float,
            Scalar::Enum => Class::Enum,
        }
    }
}

pub(crate) fn class(repr: Repr) -> Class {
    match repr {
        Repr::Obj | Repr::TObj | Repr::Unit | Repr::Token => Class::Word,
        Repr::Int => Class::Int,
        Repr::Float => Class::Float,
        Repr::Enum => Class::Enum,
        // 開くパスがコード生成より前に構造体を葉に開くので、VM には構造体の Repr が来ない
        // (docs/superpowers/specs/2026-10-21-s14e-abi-struct-design.md の「LIR」)
        Repr::Struct(_) => unreachable!("a struct reaches the VM before the flatten pass"),
    }
}

/// 種類を 4 ビットずつ詰めた語に入る、引数と結果の数の上限。
const PACKED: usize = 15;
/// 詰められない関数の語。どの位置からずらしても下の 4 ビットが `0xF` になり、どの場所の語とも等しくならない。
const SLOW_FN: u64 = u64::MAX;
/// 詰められない場所の語。下の 4 ビットが `0xE` なので、`SLOW_FN` をずらした語とも等しくならない。
pub(crate) const SLOW_SITE: u64 = u64::MAX - 1;

/// 引数の種類を i 番目の 4 ビットに、結果の種類をその次の 4 ビットに詰めた語。`apply` の速い道は関数の語を捕まえた値の
/// 数だけずらし、場所の語と1回で比べる (docs/superpowers/specs/2026-10-21-s14e-abi-struct-design.md の
/// 「作る人に任せること」)。詰められなければ `slow` を返す。
fn pack(sig: &Signature, slow: u64) -> u64 {
    if sig.params.len() > PACKED || sig.ret.len() != 1 {
        return slow;
    }
    let code = |repr: Repr| match class(repr) {
        Class::Word => 0,
        Class::Int => 1,
        Class::Float => 2,
        Class::Enum => 3,
    };
    sig.params
        .iter()
        .chain(&sig.ret)
        .enumerate()
        .fold(0, |key, (i, &repr)| key | code(repr) << (4 * i))
}

/// 中身で引くシグネチャの表。番号ごとに、場所として比べる語も持つ。
#[derive(Default)]
pub(crate) struct Signatures {
    list: Vec<Signature>,
    site_keys: Vec<u64>,
    index: HashMap<Signature, SigId>,
}

impl Signatures {
    pub(crate) fn intern(&mut self, sig: Signature) -> SigId {
        if let Some(&id) = self.index.get(&sig) {
            return id;
        }
        let id = SigId(u32::try_from(self.list.len()).expect("the signatures fit in u32 numbers"));
        self.site_keys.push(pack(&sig, SLOW_SITE));
        self.index.insert(sig.clone(), id);
        self.list.push(sig);
        id
    }

    pub(crate) fn get(&self, id: SigId) -> &Signature {
        &self.list[id.0 as usize]
    }

    /// `apply` の場所として比べる語。
    #[inline]
    pub(crate) fn site_key(&self, id: SigId) -> u64 {
        self.site_keys[id.0 as usize]
    }

    /// 関数の `FnEntry` に置く語。
    pub(crate) fn fn_key(&self, id: SigId) -> u64 {
        pack(self.get(id), SLOW_FN)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sig(params: &[Repr], ret: Repr) -> Signature {
        Signature {
            params: params.to_vec(),
            ret: vec![ret],
        }
    }

    /// 関数の語を捕まえた値の数だけずらすと、残りの引数と結果の語になる。
    #[test]
    fn a_function_key_shifted_past_its_captures_is_the_key_of_its_rest() {
        let f = pack(&sig(&[Repr::Float, Repr::Int], Repr::Float), SLOW_FN);
        let site = pack(&sig(&[Repr::Int], Repr::Float), SLOW_SITE);
        assert_eq!(f >> 4, site);
        let boxed = pack(&sig(&[Repr::TObj], Repr::Float), SLOW_SITE);
        assert_ne!(f >> 4, boxed);
    }

    /// `obj`、`tobj`、`unit` は同じ種類なので、語も等しい。
    #[test]
    fn word_reprs_pack_alike() {
        let a = pack(&sig(&[Repr::Obj, Repr::Unit], Repr::TObj), SLOW_SITE);
        let b = pack(&sig(&[Repr::TObj, Repr::TObj], Repr::Unit), SLOW_SITE);
        assert_eq!(a, b);
    }

    /// 詰められない関数と場所の語はどうずらしても等しくならない。
    #[test]
    fn slow_keys_never_match() {
        let long = vec![Repr::TObj; 20];
        let f = pack(&sig(&long, Repr::TObj), SLOW_FN);
        let site = pack(&sig(&long, Repr::TObj), SLOW_SITE);
        let short = pack(&sig(&[Repr::TObj], Repr::TObj), SLOW_SITE);
        for k in 0..20u32 {
            let shifted = f.wrapping_shr(4 * k);
            assert_ne!(shifted, site);
            assert_ne!(shifted, short);
        }
        assert_ne!(pack(&sig(&[], Repr::TObj), SLOW_FN), site);
    }
}
