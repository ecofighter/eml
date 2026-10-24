//! パイプラインを通して比べるテストが使う、プログラムの一覧。UI テストの `run` と `run-fail` のすべてのテストと、
//! `bench/` のプログラムである (docs/implementation/testing.md の「UI テスト」)。

use std::fs;
use std::path::{Path, PathBuf};

/// 1つのプログラム。`modules` は入口のディレクトリからの相対パスと本文の組で、
/// `eml_test_support::with_modules` にそのまま渡す。
pub struct UiProgram {
    /// `run/basics/hello.em`、`run/names/import_cycle`、`bench/fib` の形の名前。
    pub name: String,
    pub entry: String,
    pub modules: Vec<(String, String)>,
}

impl UiProgram {
    pub fn module_refs(&self) -> Vec<(&str, &str)> {
        self.modules
            .iter()
            .map(|(path, text)| (path.as_str(), text.as_str()))
            .collect()
    }
}

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// 名前の順に並べる。比べた結果の報告を、ファイルシステムのたどり方によらず同じ順にするためである。
pub fn ui_programs() -> Vec<UiProgram> {
    let mut programs = Vec::new();
    for top in ["run", "run-fail"] {
        let top_dir = workspace().join("tests/ui").join(top);
        for category in sorted_entries(&top_dir) {
            let category_name = file_name(&category);
            for entry in sorted_entries(&category) {
                if entry.is_dir() {
                    programs.push(directory_program(top, &category_name, &entry));
                } else if entry.extension().is_some_and(|ext| ext == "em") {
                    programs.push(UiProgram {
                        name: format!("{top}/{category_name}/{}", file_name(&entry)),
                        entry: fs::read_to_string(&entry).unwrap(),
                        modules: Vec::new(),
                    });
                }
            }
        }
    }
    for entry in sorted_entries(&workspace().join("bench")) {
        if entry.extension().is_some_and(|ext| ext == "em") {
            programs.push(UiProgram {
                name: format!("bench/{}", entry.file_stem().unwrap().to_string_lossy()),
                entry: fs::read_to_string(&entry).unwrap(),
                modules: Vec::new(),
            });
        }
    }
    programs.sort_by(|a, b| a.name.cmp(&b.name));
    programs
}

/// ディレクトリのテスト。入口は `main.em` で、ほかの `.em` はディレクトリからの相対パスのモジュールである。
fn directory_program(top: &str, category: &str, dir: &Path) -> UiProgram {
    let mut modules = Vec::new();
    collect_modules(dir, dir, &mut modules);
    modules.sort();
    UiProgram {
        name: format!("{top}/{category}/{}", file_name(dir)),
        entry: fs::read_to_string(dir.join("main.em")).unwrap(),
        modules,
    }
}

fn collect_modules(root: &Path, dir: &Path, modules: &mut Vec<(String, String)>) {
    for entry in sorted_entries(dir) {
        if entry.is_dir() {
            collect_modules(root, &entry, modules);
        } else if entry.extension().is_some_and(|ext| ext == "em") && entry != root.join("main.em")
        {
            let relative = entry
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            modules.push((relative, fs::read_to_string(&entry).unwrap()));
        }
    }
}

fn sorted_entries(dir: &Path) -> Vec<PathBuf> {
    let mut entries: Vec<PathBuf> = fs::read_dir(dir)
        .unwrap_or_else(|error| panic!("{}: {error}", dir.display()))
        .map(|entry| entry.unwrap().path())
        .collect();
    entries.sort();
    entries
}

fn file_name(path: &Path) -> String {
    path.file_name().unwrap().to_string_lossy().into_owned()
}
