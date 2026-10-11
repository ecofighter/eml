//! Core IR と LIR のテキスト形式が共有する部品 (docs/implementation/testing.md の「Core IR のテキストの形」)。
//! 2つの IR に共通する命令は同じ表示になるので、表示と読み込みの部品をここに1つずつ置く。

use crate::{EffectInfo, Immediates, Prim};

/// エフェクトの名前。表にない番号は `#N` で書く。誤りを含む IR も表示でき、`parse` が読み戻せて、verifier がその番号を
/// 報告できるようにするためである。
pub fn effect_text(effects: &[EffectInfo], effect: u32) -> String {
    effects
        .get(effect as usize)
        .map_or_else(|| format!("#{effect}"), |info| info.name.clone())
}

/// エフェクトの多重集合 (`mask` と `evv_mask` の即値) を `, ` で区切って並べる。
pub fn effects_text(effects: &[EffectInfo], effects_: &[u32]) -> String {
    let names: Vec<String> = effects_
        .iter()
        .map(|&effect| effect_text(effects, effect))
        .collect();
    names.join(", ")
}

/// エフェクトで修飾しない操作の名前。表にない操作は `#N` で書く。
pub fn operation_name(effects: &[EffectInfo], effect: u32, op: u32) -> String {
    effects
        .get(effect as usize)
        .and_then(|info| info.operations.get(op as usize))
        .map_or_else(|| format!("#{op}"), |operation| operation.name.clone())
}

/// `E.op` の形。
pub fn operation_text(effects: &[EffectInfo], effect: u32, op: u32) -> String {
    format!(
        "{}.{}",
        effect_text(effects, effect),
        operation_name(effects, effect, op)
    )
}

/// 基本操作の名前と、`[…]` に書いた即値 (`ev_clause[State.get]`)。
pub fn prim_text(effects: &[EffectInfo], prim: &Prim) -> String {
    let name = prim.name();
    match prim.immediates() {
        Immediates::None => name.to_string(),
        Immediates::Effects(list) => format!("{name}[{}]", effects_text(effects, list)),
        Immediates::Effect(effect) => format!("{name}[{}]", effect_text(effects, effect)),
        Immediates::Operation { effect, op } => {
            format!("{name}[{}]", operation_text(effects, effect, op))
        }
    }
}

/// 関数と extern の行の名前。単相化の instance の名前 (`id@[Int, String]`) と instance の extern の行の名前
/// (`Prelude.Eq Int.==`) は、テキストの形の字句の区切りを含むので、文字列と同じ逃がし方で引用符で囲む
/// (docs/implementation/testing.md の「Core IR のテキストの形」)。row のビットを書いた名前 (`twice<e>`) も同じく
/// 囲む (docs/spec/core-ir.md の「row のビットと instance の鍵」)。
pub fn function_name(name: &str) -> std::borrow::Cow<'_, str> {
    let separates = |c: char| c.is_whitespace() || "(){}[],\"<>".contains(c);
    if name.is_empty() || name.contains(separates) {
        format!("{name:?}").into()
    } else {
        name.into()
    }
}
