//! シグネチャと構造体の形。シグネチャは関数と `apply` の場所の、源の単位の引数と結果の Repr の並びである。どちらも
//! `CodeBase` の中で中身から一意な番号にするので、追記の間も番号は変わらない
//! (docs/spec/vm.md の「シグネチャ」)。

use std::collections::HashMap;

use eml_lir::{LayoutCtor, Program, Repr, Shape, StructId};

/// VM の構造体の形の中身。配置の名前、0番のコンストラクタの宣言したフィールドの Repr、形のフィールドの Repr で決まる。
/// 入れ子の構造体のフィールドは VM の番号で指す。印は持たない。形は骨組みで比べ、印だけが違う形は同じ変換を使うため
/// である (docs/spec/vm.md の「シグネチャ」)。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct StructKey {
    pub(crate) layout: String,
    pub(crate) declared: Vec<Repr>,
    pub(crate) fields: Vec<Repr>,
}

/// VM の構造体の形と、そこから決まる物理的な形と葉。
#[derive(Debug)]
pub(crate) struct StructInfo {
    pub(crate) key: StructKey,
    /// 箱の物理的な形。箱は配置の `con` と同じ形の物体である。
    pub(crate) shape: Shape,
    /// `unit` でない葉の Repr を、深さ優先にフィールドの順で並べたもの。葉1つが語1つである。
    pub(crate) leaves: Vec<Repr>,
}

/// 中身で引く構造体の形の表。
#[derive(Default)]
pub(crate) struct Structs {
    list: Vec<StructInfo>,
    index: HashMap<StructKey, StructId>,
}

impl Structs {
    pub(crate) fn get(&self, id: StructId) -> &StructInfo {
        &self.list[id.0 as usize]
    }

    /// プログラムの形の番号から VM の番号への写しと、新しく引いた形の番号を返す。プログラムの番号は1回の変換の中だけで
    /// 意味を持つので、中身で引き直す。
    pub(crate) fn link(&mut self, program: &Program) -> (Vec<StructId>, Vec<StructId>) {
        let count = program.structs.shapes().len();
        let mut links = vec![None; count];
        let mut fresh = Vec::new();
        for id in 0..count {
            self.link_one(program, StructId(id as u32), &mut links, &mut fresh);
        }
        let links = links
            .into_iter()
            .map(|id| id.expect("every shape is linked"))
            .collect();
        (links, fresh)
    }

    /// 1つの形を引き直す。入れ子の形を先に引く。再帰は形の深さだけである。
    fn link_one(
        &mut self,
        program: &Program,
        id: StructId,
        links: &mut [Option<StructId>],
        fresh: &mut Vec<StructId>,
    ) -> StructId {
        if let Some(linked) = links[id.0 as usize] {
            return linked;
        }
        let shape = program.structs.shape(id);
        let fields = shape
            .fields
            .iter()
            .map(|&(repr, _)| match repr {
                Repr::Struct(inner) => Repr::Struct(self.link_one(program, inner, links, fresh)),
                other => other,
            })
            .collect();
        let layout = &program.layouts[shape.layout.0 as usize];
        let key = StructKey {
            layout: layout.name.clone(),
            declared: layout.constructors[0].fields.clone(),
            fields,
        };
        let linked = match self.index.get(&key) {
            Some(&linked) => linked,
            None => {
                let linked =
                    StructId(u32::try_from(self.list.len()).expect("the shapes fit in u32"));
                let ctor = LayoutCtor {
                    name: String::new(),
                    fields: key.declared.clone(),
                };
                let mut leaves = Vec::new();
                for &field in &key.fields {
                    match field {
                        Repr::Unit => {}
                        Repr::Struct(inner) => leaves.extend_from_slice(&self.get(inner).leaves),
                        other => leaves.push(other),
                    }
                }
                self.index.insert(key.clone(), linked);
                self.list.push(StructInfo {
                    key,
                    shape: ctor.shape(),
                    leaves,
                });
                fresh.push(linked);
                linked
            }
        };
        links[id.0 as usize] = Some(linked);
        linked
    }

    /// 源の単位の Repr が占める語の Repr。構造体は葉に開き、ほかは1語である。
    pub(crate) fn words<'a>(&'a self, repr: &'a Repr) -> &'a [Repr] {
        match repr {
            Repr::Struct(id) => &self.get(*id).leaves,
            other => std::slice::from_ref(other),
        }
    }

    /// 逆アセンブラが Repr を中身で書くための名前。構造体は `Vec3{float, float, float}` と書く。
    pub(crate) fn repr_name(&self, repr: Repr) -> String {
        match repr {
            Repr::Struct(id) => {
                let key = &self.get(id).key;
                let fields: Vec<String> = key.fields.iter().map(|&f| self.repr_name(f)).collect();
                format!("{}{{{}}}", key.layout, fields.join(", "))
            }
            other => other.name().to_string(),
        }
    }

    /// 逆アセンブラと一致の確かめが変換を中身で書くための名前。
    pub(crate) fn class_name(&self, class: Class) -> String {
        match class {
            Class::Word => "word".to_string(),
            Class::Int => "int".to_string(),
            Class::Float => "float".to_string(),
            Class::Enum => "enum".to_string(),
            Class::Struct(id) => self.repr_name(Repr::Struct(StructId(id))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct SigId(pub(crate) u32);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct Signature {
    pub(crate) params: Vec<Repr>,
    pub(crate) ret: Vec<Repr>,
}

/// 変換の種類。`apply` は Repr の等しさでなく種類で比べる。`obj` と `tobj` が違うだけで遅い道に入らないためである
/// (docs/spec/vm.md の「シグネチャ」)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Class {
    /// 語のまま渡せるもの (`obj`、`tobj`、`unit`)。
    Word,
    Int,
    Float,
    Enum,
    /// 構造体。VM の形の番号を持つ。番号は骨組みから一意なので、番号が等しいことが骨組みが等しいことである。
    Struct(u32),
}

impl Class {
    /// 箱を要するスカラーの種類。語のまま渡せる種類と構造体には `None` を返す。
    pub(crate) fn scalar(self) -> Option<Scalar> {
        match self {
            Class::Word | Class::Struct(_) => None,
            Class::Int => Some(Scalar::Int),
            Class::Float => Some(Scalar::Float),
            Class::Enum => Some(Scalar::Enum),
        }
    }
}

/// 箱を要するスカラーの種類。変換の命令が持つ。構造体の番号は持たないので、命令の 8 バイトに収まる。
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

/// Repr の種類。構造体の Repr は VM の番号で持つ (`Structs::link`)。
pub(crate) fn class(repr: Repr) -> Class {
    match repr {
        Repr::Obj | Repr::TObj | Repr::Unit | Repr::Token => Class::Word,
        Repr::Int => Class::Int,
        Repr::Float => Class::Float,
        Repr::Enum => Class::Enum,
        Repr::Struct(id) => Class::Struct(id.0),
    }
}

/// 種類を 4 ビットずつ詰めた語に入る、引数と結果の数の上限。
const PACKED: usize = 15;
/// 詰められない関数の語。どの位置からずらしても下の 4 ビットが `0xF` になり、どの場所の語とも等しくならない。
const SLOW_FN: u64 = u64::MAX;
/// 詰められない場所の語。下の 4 ビットが `0xE` なので、`SLOW_FN` をずらした語とも等しくならない。
pub(crate) const SLOW_SITE: u64 = u64::MAX - 1;

/// 引数の種類を i 番目の 4 ビットに、結果の種類をその次の 4 ビットに詰めた語。`apply` の速い道は関数の語を捕まえた値の
/// 数だけずらし、場所の語と1回で比べる (docs/spec/vm.md の「シグネチャ」)。詰められなければ `slow` を返す。
/// 構造体を含むシグネチャも詰めない。構造体の引数は語の数が Repr の数と違い、速い道は語の数で引数の数を比べるため
/// である。
fn pack(sig: &Signature, slow: u64) -> u64 {
    if sig.params.len() > PACKED
        || sig.ret.len() != 1
        || sig
            .params
            .iter()
            .chain(&sig.ret)
            .any(|repr| matches!(repr, Repr::Struct(_)))
    {
        return slow;
    }
    let code = |repr: Repr| match class(repr) {
        Class::Word => 0,
        Class::Int => 1,
        Class::Float => 2,
        Class::Enum => 3,
        Class::Struct(_) => unreachable!("a signature with a struct is not packed"),
    };
    sig.params
        .iter()
        .chain(&sig.ret)
        .enumerate()
        .fold(0, |key, (i, &repr)| key | code(repr) << (4 * i))
}

/// シグネチャの語の並び。スロットとクロージャと `ApplyRest` のフレームは、構造体を葉に開いた語で値を持つので、
/// 源の単位の Repr から語の位置を引く。構造体のないシグネチャでは、語の並びは源の並びと同じである。
#[derive(Debug)]
pub(crate) struct Words {
    /// 引数の語の Repr。
    pub(crate) params: Vec<Repr>,
    /// 源の i 番目の引数の最初の語の位置。長さは源の引数の数 + 1 で、最後は語の数である。
    pub(crate) starts: Vec<usize>,
}

/// 中身で引くシグネチャの表。番号ごとに、場所として比べる語と語の並びも持つ。
#[derive(Default)]
pub(crate) struct Signatures {
    list: Vec<Signature>,
    words: Vec<Words>,
    site_keys: Vec<u64>,
    index: HashMap<Signature, SigId>,
}

impl Signatures {
    /// `sig` の構造体の Repr は `structs` の番号である。
    pub(crate) fn intern(&mut self, sig: Signature, structs: &Structs) -> SigId {
        if let Some(&id) = self.index.get(&sig) {
            return id;
        }
        let id = SigId(u32::try_from(self.list.len()).expect("the signatures fit in u32 numbers"));
        let mut words = Words {
            params: Vec::with_capacity(sig.params.len()),
            starts: Vec::with_capacity(sig.params.len() + 1),
        };
        for param in &sig.params {
            words.starts.push(words.params.len());
            words.params.extend_from_slice(structs.words(param));
        }
        words.starts.push(words.params.len());
        self.site_keys.push(pack(&sig, SLOW_SITE));
        self.words.push(words);
        self.index.insert(sig.clone(), id);
        self.list.push(sig);
        id
    }

    pub(crate) fn get(&self, id: SigId) -> &Signature {
        &self.list[id.0 as usize]
    }

    #[inline]
    pub(crate) fn words(&self, id: SigId) -> &Words {
        &self.words[id.0 as usize]
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
