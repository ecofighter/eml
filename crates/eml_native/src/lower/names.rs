//! moduleの記号の名前。名前はLIRの名前と種類だけで決まる (docs/spec/native.md の「関数の形」)。
use eml_nrt::Kind;

pub fn function(name: &str) -> String {
    format!("f:{name}")
}

/// 種類の並び`params -> rets`のthunkの記号。`eml$thunk$if$i`のようになる。
pub fn thunk(params: &[Kind], rets: &[Kind]) -> String {
    let letters = |kinds: &[Kind]| -> String {
        kinds
            .iter()
            .map(|kind| match kind {
                Kind::I64 => 'i',
                Kind::F64 => 'f',
            })
            .collect()
    };
    format!("eml$thunk${}${}", letters(params), letters(rets))
}
