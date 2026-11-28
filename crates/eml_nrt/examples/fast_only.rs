//! 速いモードのエントリすべてと速いモードの実行の駆動を名指し、検査モードのものは何も名指さない。
use eml_nrt::mode::Fast;

fn main() {
    let entries = eml_nrt::entries::<Fast>()
        .map(|entry| (entry.address)() as usize)
        .fold(0usize, |a, b| a ^ b);
    let run = (eml_nrt::run::run::<Fast> as *const ()).addr();
    println!("{}", std::hint::black_box(entries ^ run));
}
