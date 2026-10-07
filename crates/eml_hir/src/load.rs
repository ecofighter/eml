//! 読み込みの段 (docs/implementation/architecture.md の「`eml_hir` の内部」)。埋め込んだ標準ライブラリと入口を登録し、
//! import を宣言の順に幅優先でたどって、モジュールの列を作る。ファイルの読み方は `ModuleSource` で受け取り、この段は IO を
//! 持たない。同じ読み方を渡せば同じ結果を返すので、診断の並びも安定する。

use std::collections::HashMap;

use eml_diagnostics::{Diagnostic, FileId, Label, SourceFiles};

use crate::codes;
use crate::def_map::module_id;
use crate::item_tree::{ImportItem, ItemTree, ModulePath, item_tree};
use crate::program::{ModuleId, ModuleOrigin};
use crate::{PRELUDE_PATH, STD};

const PRELUDE: &str = "Prelude";
const MAIN: &str = "Main";
/// 標準ライブラリのモジュールの正式な名前の根 (`Std.Fs`)。Prelude だけはこの下に置かない。
pub(crate) const STD_ROOT: &str = "Std";
const PRELUDE_FILE: &str = "Prelude.em";

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
    /// `Prelude`、`Main`、`Std.Fs`、`Report.Csv`。標準ライブラリのモジュールは正式な名前である。
    pub name: String,
    pub origin: ModuleOrigin,
    pub tree: ItemTree,
    /// `tree.imports` と同じ順。
    pub targets: Vec<ImportTarget>,
}

#[derive(Debug)]
pub struct Loaded {
    pub files: SourceFiles,
    pub prelude: FileId,
    pub entry: FileId,
    /// 番号が `ModuleId` で、`FileId` の番号とも一致する。0 が Prelude、1 が入口、2 から Prelude を除く標準ライブラリの
    /// モジュールが埋め込んだ並びの順に続き、その後にユーザーの import を見つけた順に並ぶ。
    pub modules: Vec<LoadedModule>,
}

/// 埋め込んだ標準ライブラリ (`STD`) で `load_with_std` を呼ぶ。診断は構文解析、`ItemTree`、E1026、E1030 のもの。
pub fn load(
    entry_path: &str,
    entry_text: &str,
    source: &dyn ModuleSource,
) -> (Loaded, Vec<Diagnostic>) {
    load_with_std(STD, entry_path, entry_text, source)
}

/// 標準ライブラリを `(ファイル名, 本文)` の並びで受け取る `load`。テストが標準ライブラリを差し替えられるようにするため。
/// 並びは `Prelude.em` と、本物の `Fs.em` (または同じ extern の宣言を持つもの) を含める。extern の索引が両方を引くので、
/// 足りないと `def_map` が panic する。標準ライブラリのモジュールは、使うかどうかによらずすべて読み込む。
pub fn load_with_std(
    std: &[(&str, &str)],
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
    let prelude_text = std
        .iter()
        .find(|&&(file, _)| file == PRELUDE_FILE)
        .map(|&(_, text)| text)
        .expect("the standard library has Prelude.em");
    let prelude = loader.add_std(
        PRELUDE_FILE,
        PRELUDE,
        PRELUDE_PATH.to_string(),
        prelude_text,
    );
    let entry = loader.add(MAIN, entry_path.to_string(), entry_text, ModuleOrigin::User);
    for &(file, text) in std.iter().filter(|&&(file, _)| file != PRELUDE_FILE) {
        let name = format!("{STD_ROOT}.{}", std_path(file).dotted());
        loader.add_std(file, &name, std_shown(file), text);
    }
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

/// 標準ライブラリのファイル名 (`Fs.em`) を、`std/` からのモジュールのパスにする。
fn std_path(file: &str) -> ModulePath {
    let stem = file
        .strip_suffix(".em")
        .expect("a standard library file ends with .em");
    ModulePath(stem.split('/').map(str::to_string).collect())
}

/// 診断に出す標準ライブラリのパス。手元の相対パスと見誤らないよう `<std>/` で始める。
fn std_shown(file: &str) -> String {
    format!("<std>/{file}")
}

struct Loader<'a> {
    /// 入口の表示のパスのディレクトリ (`/` で終わるか空)。根は入口のファイルのディレクトリである
    /// (docs/spec/modules.md の「モジュール」)。
    root: &'a str,
    entry_file: &'a str,
    source: &'a dyn ModuleSource,
    files: SourceFiles,
    modules: Vec<LoadedModule>,
    /// 読んだモジュール。標準ライブラリとユーザーの根に同じパスのモジュールがありうるので、出どころと組にする。
    by_path: HashMap<(ModuleOrigin, ModulePath), ModuleId>,
    diagnostics: Vec<Diagnostic>,
}

/// import できないモジュール。
#[derive(Clone, Copy)]
enum Reserved {
    /// `Prelude` と `Main` は予約したモジュール名で、根に同じ名前のファイルがあっても読まない。
    Name,
    /// 暗黙の修飾子 `Prelude` が、ほかのモジュールと合流しないようにする。
    PreludeQualifier,
    /// `Std` は標準ライブラリの根で、モジュールではない。
    StdRoot,
    /// 入口は、どの名前でも import できない。
    Entry,
}

impl Loader<'_> {
    fn add(&mut self, name: &str, path: String, text: &str, origin: ModuleOrigin) -> FileId {
        let file = self.files.add(path, text);
        let (parse, errors) = eml_syntax::parse(file, self.files.text(file));
        self.diagnostics.extend(errors);
        let (tree, stage) = item_tree(file, &parse.tree());
        self.diagnostics.extend(stage);
        self.modules.push(LoadedModule {
            name: name.to_string(),
            origin,
            tree,
            targets: Vec::new(),
        });
        file
    }

    fn add_std(&mut self, file: &str, name: &str, shown: String, text: &str) -> FileId {
        let id = module_id(self.modules.len());
        self.by_path.insert((ModuleOrigin::Std, std_path(file)), id);
        self.add(name, shown, text, ModuleOrigin::Std)
    }

    fn resolve_imports(&mut self, index: usize) {
        // 読んだモジュールを `modules` に足すので、import の並びを借りたままにできない
        let imports = std::mem::take(&mut self.modules[index].tree.imports);
        let file = self.modules[index].tree.file;
        let origin = self.modules[index].origin;
        let targets: Vec<ImportTarget> = imports
            .iter()
            .map(|import| self.target(file, origin, import))
            .collect();
        let module = &mut self.modules[index];
        module.tree.imports = imports;
        module.targets = targets;
    }

    fn target(&mut self, file: FileId, origin: ModuleOrigin, import: &ImportItem) -> ImportTarget {
        // パスの後ろの構文の誤りはパーサが報告済みである。読めた部分のパスで読むと、書いたつもりと違うモジュールを
        // 黙って取り込むか、E1026 を連鎖させる
        if import.malformed {
            return ImportTarget::Broken;
        }
        match origin {
            // 標準ライブラリはユーザーの根に左右されないよう、`std/` だけを探す。予約の検査はユーザーの誤りのためにある
            ModuleOrigin::Std => self.std_target(file, import, &import.path),
            ModuleOrigin::User => self.user_target(file, import),
        }
    }

    /// ユーザーのモジュールの import。`Std.` で始まるパスは標準ライブラリだけを探す。ほかはユーザーの根を探し、ファイルが
    /// ない (`NotFound`) ときだけ同じ名前の標準ライブラリのモジュールを探す。読めないファイルは書いたつもりのモジュールが
    /// ある印なので、標準ライブラリへ進まない。
    fn user_target(&mut self, file: FileId, import: &ImportItem) -> ImportTarget {
        let shown = format!("{}{}", self.root, import.path.file_path());
        if let Some(reserved) = self.reserved(import) {
            self.diagnostics
                .push(reserved_import(file, import, reserved, &shown));
            return ImportTarget::Broken;
        }
        if let [root, rest @ ..] = import.path.0.as_slice()
            && root == STD_ROOT
        {
            return self.std_target(file, import, &ModulePath(rest.to_vec()));
        }
        let key = (ModuleOrigin::User, import.path.clone());
        if let Some(&id) = self.by_path.get(&key) {
            return ImportTarget::Module(id);
        }
        match self.source.read(&import.path) {
            Ok(text) => {
                let id = module_id(self.modules.len());
                self.by_path.insert(key, id);
                self.add(&import.path.dotted(), shown, &text, ModuleOrigin::User);
                ImportTarget::Module(id)
            }
            Err(ReadError::NotFound) => {
                match self.by_path.get(&(ModuleOrigin::Std, import.path.clone())) {
                    Some(&id) => ImportTarget::Module(id),
                    None => {
                        self.diagnostics.push(unreadable(
                            file,
                            import,
                            &ReadError::NotFound,
                            &shown,
                        ));
                        ImportTarget::Broken
                    }
                }
            }
            Err(error) => {
                self.diagnostics
                    .push(unreadable(file, import, &error, &shown));
                ImportTarget::Broken
            }
        }
    }

    /// `path` は `std/` からのパスである。標準ライブラリのモジュールはすべて読み込み済みなので、表から引くだけでよい。
    fn std_target(&mut self, file: FileId, import: &ImportItem, path: &ModulePath) -> ImportTarget {
        match self.by_path.get(&(ModuleOrigin::Std, path.clone())) {
            Some(&id) => ImportTarget::Module(id),
            None => {
                let shown = std_shown(&path.file_path());
                self.diagnostics
                    .push(unreadable(file, import, &ReadError::NotFound, &shown));
                ImportTarget::Broken
            }
        }
    }

    fn reserved(&self, import: &ImportItem) -> Option<Reserved> {
        let name = import.path.dotted();
        if name == PRELUDE || name == MAIN || is_std_prelude(&name) {
            Some(Reserved::Name)
        } else if import.qualifier.0 == PRELUDE {
            Some(Reserved::PreludeQualifier)
        } else if name == STD_ROOT {
            Some(Reserved::StdRoot)
        } else if import.path.file_path() == self.entry_file {
            Some(Reserved::Entry)
        } else {
            None
        }
    }
}

/// `Std.Prelude` は Prelude のファイルを指すが、Prelude は暗黙に取り込むので `Prelude` と同じく予約する。別名のない
/// 別名のない `import Std.Prelude` は修飾子が `Prelude` になるが、別名で直せる誤りではないので、修飾子の検査より先に引いて
/// 「別名を選べ」という help を付けない (docs/spec/modules.md の「標準ライブラリ」)。
fn is_std_prelude(name: &str) -> bool {
    name.strip_prefix(STD_ROOT)
        .and_then(|rest| rest.strip_prefix('.'))
        == Some(PRELUDE)
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
            let why = if name == MAIN {
                format!("`{MAIN}` is the name of the entry module")
            } else {
                "the Prelude is imported implicitly".to_string()
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
        Reserved::StdRoot => Diagnostic::error(
            codes::RESERVED_MODULE,
            format!("the module `{STD_ROOT}` is reserved"),
            label(format!("`{STD_ROOT}` is the root of the standard library")),
        ),
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
