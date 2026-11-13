//! moduleの記号の名前。名前はLIRの名前と種類だけで決まる (docs/spec/native.md の「関数の形」)。
use eml_nrt::Kind;

/// moduleに1つある、置いた失敗を上げる関数の記号 (docs/spec/native.md の「巻き戻しの地点」)。
pub const RAISE: &str = "eml$raise";

pub fn function(name: &str) -> String {
    format!("f:{name}")
}

/// 関数`name`の、捕まえた値を`captures`個持つクロージャのエントリの記号 (docs/spec/native.md の「関数の値」)。
pub fn adapter(captures: u32, name: &str) -> String {
    format!("c{captures}:{name}")
}

/// 引数を`args`個渡す`apply`の補助の関数の記号 (docs/spec/native.md の「関数の値」)。
pub fn apply(args: usize) -> String {
    format!("eml$apply${args}")
}

/// 関数`name`の静的なクロージャのデータの記号。
pub fn fn_value(name: &str) -> String {
    format!("v:{name}")
}

/// 文字列の定数`text`のデータの記号。
pub fn string(text: &str) -> String {
    format!("s:{text}")
}

/// 即値に収まらない`int`の定数`n`の箱のデータの記号。
pub fn big_int(n: i64) -> String {
    format!("i:{n}")
}

/// ビット列が`bits`の`float`の定数の箱のデータの記号。
pub fn float(bits: u64) -> String {
    format!("x:{bits:016x}")
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
