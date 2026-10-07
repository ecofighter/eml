//! 読み込みの段 (docs/implementation/architecture.md の「`eml_hir` の内部」)。入口から import を宣言の順に幅優先で
//! たどり、モジュールの列を作る。ファイルの読み方は `ModuleSource` で受け取り、この段は IO を持たない。同じ読み方を
//! 渡せば同じ結果を返すので、診断の並びも安定する。

use std::collections::HashMap;

use eml_diagnostics::{Diagnostic, FileId, Label, SourceFiles};

use crate::codes;
use crate::def_map::module_id;
use crate::item_tree::{ImportItem, ItemTree, ModulePath, item_tree};
use crate::program::ModuleId;
use crate::{PRELUDE_PATH, PRELUDE_SOURCE};

const PRELUDE: &str = "Prelude";
const MAIN: &str = "Main";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadError {
    NotFound,
    /// 読めない理由 (UTF-8 でない、IO の誤り)。E1026 のメッセージに書く。
    Unreadable(String),
}

/// 根からの相対パスでモジュールの本文を読む。IO は実装だけが持つ。
pub trait ModuleSource {
    fn read(&self, path: &ModulePath) -> Result<String, ReadError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportTarget {
    Module(ModuleId),
    /// 見つからないか読めない (E1026)、または予約したモジュール (E1030)。
    Broken,
}

#[derive(Debug)]
pub struct LoadedModule {
    /// `Prelude`、`Main`、`Report.Csv`。
    pub name: String,
    pub tree: ItemTree,
    /// `tree.imports` と同じ順。
    pub targets: Vec<ImportTarget>,
}

#[derive(Debug)]
pub struct Loaded {
    pub files: SourceFiles,
    pub prelude: FileId,
    pub entry: FileId,
    /// 番号が `ModuleId` で、`FileId` の番号とも一致する。0 が Prelude、1 が入口で、依存先は見つけた順に続く。
    pub modules: Vec<LoadedModule>,
}

/// Prelude と入口を登録し、import を宣言の順に幅優先でたどる。診断は構文解析、`ItemTree`、E1026、E1030 のもの。
pub fn load(
    entry_path: &str,
    entry_text: &str,
    source: &dyn ModuleSource,
) -> (Loaded, Vec<Diagnostic>) {
    load_with_prelude(PRELUDE_SOURCE, entry_path, entry_text, source)
}

/// Prelude の本文を差し替えた `load`。Prelude に定義を足して確かめるテストのため。
pub fn load_with_prelude(
    prelude_text: &str,
    entry_path: &str,
    entry_text: &str,
    source: &dyn ModuleSource,
) -> (Loaded, Vec<Diagnostic>) {
    let (root, entry_file) = match entry_path.rfind('/') {
        Some(slash) => entry_path.split_at(slash + 1),
        None => ("", entry_path),
    };
    let mut loader = Loader {
        root,
        entry_file,
        source,
        files: SourceFiles::new(),
        modules: Vec::new(),
        by_path: HashMap::new(),
        diagnostics: Vec::new(),
    };
    let prelude = loader.add(PRELUDE, PRELUDE_PATH.to_string(), prelude_text);
    let entry = loader.add(MAIN, entry_path.to_string(), entry_text);
    // 見つけたモジュールを `modules` の末尾に足すので、前から順に処理すれば幅優先になる
    let mut next = 0;
    while next < loader.modules.len() {
        loader.resolve_imports(next);
        next += 1;
    }
    let Loader {
        files,
        modules,
        diagnostics,
        ..
    } = loader;
    (
        Loaded {
            files,
            prelude,
            entry,
            modules,
        },
        diagnostics,
    )
}

struct Loader<'a> {
    /// 入口の表示のパスのディレクトリ (`/` で終わるか空)。根は入口のファイルのディレクトリである
    /// (docs/spec/modules.md の「モジュール」)。
    root: &'a str,
    entry_file: &'a str,
    source: &'a dyn ModuleSource,
    files: SourceFiles,
    modules: Vec<LoadedModule>,
    by_path: HashMap<ModulePath, ModuleId>,
    diagnostics: Vec<Diagnostic>,
}

/// import できないモジュール。
#[derive(Clone, Copy)]
enum Reserved {
    /// `Prelude` と `Main` は予約したモジュール名で、根に同じ名前のファイルがあっても読まない。
    Name,
    /// 暗黙の修飾子 `Prelude` が、ほかのモジュールと合流しないようにする。
    PreludeQualifier,
    /// 入口は、どの名前でも import できない。
    Entry,
}

impl Loader<'_> {
    fn add(&mut self, name: &str, path: String, text: &str) -> FileId {
        let file = self.files.add(path, text);
        let (parse, errors) = eml_syntax::parse(file, self.files.text(file));
        self.diagnostics.extend(errors);
        let (tree, stage) = item_tree(file, &parse.tree());
        self.diagnostics.extend(stage);
        self.modules.push(LoadedModule {
            name: name.to_string(),
            tree,
            targets: Vec::new(),
        });
        file
    }

    fn resolve_imports(&mut self, index: usize) {
        // 読んだモジュールを `modules` に足すので、import の並びを借りたままにできない
        let imports = std::mem::take(&mut self.modules[index].tree.imports);
        let file = self.modules[index].tree.file;
        let targets: Vec<ImportTarget> = imports
            .iter()
            .map(|import| self.target(file, import))
            .collect();
        let module = &mut self.modules[index];
        module.tree.imports = imports;
        module.targets = targets;
    }

    fn target(&mut self, file: FileId, import: &ImportItem) -> ImportTarget {
        let shown = format!("{}{}", self.root, import.path.file_path());
        if let Some(reserved) = self.reserved(import) {
            self.diagnostics
                .push(reserved_import(file, import, reserved, &shown));
            return ImportTarget::Broken;
        }
        if let Some(&id) = self.by_path.get(&import.path) {
            return ImportTarget::Module(id);
        }
        match self.source.read(&import.path) {
            Ok(text) => {
                let id = module_id(self.modules.len());
                self.by_path.insert(import.path.clone(), id);
                self.add(&import.path.dotted(), shown, &text);
                ImportTarget::Module(id)
            }
            Err(error) => {
                self.diagnostics
                    .push(unreadable(file, import, &error, &shown));
                ImportTarget::Broken
            }
        }
    }

    fn reserved(&self, import: &ImportItem) -> Option<Reserved> {
        let name = import.path.dotted();
        if name == PRELUDE || name == MAIN {
            Some(Reserved::Name)
        } else if import.qualifier.0 == PRELUDE {
            Some(Reserved::PreludeQualifier)
        } else if import.path.file_path() == self.entry_file {
            Some(Reserved::Entry)
        } else {
            None
        }
    }
}

fn reserved_import(
    file: FileId,
    import: &ImportItem,
    reserved: Reserved,
    shown: &str,
) -> Diagnostic {
    let label = |message: String| Label::new(file, import.range, message);
    match reserved {
        Reserved::Name => {
            let name = import.path.dotted();
            let why = if name == PRELUDE {
                "the Prelude is imported implicitly".to_string()
            } else {
                format!("`{MAIN}` is the name of the entry module")
            };
            Diagnostic::error(
                codes::RESERVED_MODULE,
                format!("the module `{name}` is reserved"),
                label(why),
            )
        }
        Reserved::PreludeQualifier => Diagnostic::error(
            codes::RESERVED_MODULE,
            format!("the qualifier `{PRELUDE}` is reserved"),
            label(format!(
                "`{PRELUDE}` always qualifies the names of the Prelude"
            )),
        )
        .with_help("choose another qualifier with `as`"),
        Reserved::Entry => Diagnostic::error(
            codes::RESERVED_MODULE,
            "the entry module cannot be imported",
            label(format!("`{shown}` is the entry file")),
        ),
    }
}

fn unreadable(file: FileId, import: &ImportItem, error: &ReadError, shown: &str) -> Diagnostic {
    let module = import.path.dotted();
    let (message, label) = match error {
        ReadError::NotFound => (
            format!("cannot find module `{module}`"),
            format!("there is no file `{shown}`"),
        ),
        ReadError::Unreadable(reason) => (
            format!("cannot read module `{module}`: {reason}"),
            format!("`{shown}` cannot be read"),
        ),
    };
    Diagnostic::error(
        codes::MODULE_NOT_FOUND,
        message,
        Label::new(file, import.path_range, label),
    )
}
