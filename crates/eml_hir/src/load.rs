//! 読み込みの段 (docs/implementation/architecture.md の「`eml_hir` の内部」)。埋め込んだ標準ライブラリと入口を登録し、
//! import を宣言の順に幅優先でたどって、モジュールの列を作る。ファイルの読み方は `ModuleSource` で受け取り、この段は IO を
//! 持たない。同じ読み方を渡せば同じ結果を返すので、診断の並びも安定する。

use std::collections::HashMap;

use eml_diagnostics::{Diagnostic, FileId, Label, SourceFiles};
use eml_syntax::Parse;

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
    /// `tree` のポインタを解決する構文木。
    pub parse: Parse,
    pub tree: ItemTree,
    /// `tree.imports` と同じ順。
    pub targets: Vec<ImportTarget>,
}

#[derive(Debug)]
pub struct Loaded {
    pub files: SourceFiles,
    pub prelude: FileId,
    pub entry: FileId,
    /// 番号が `ModuleId` で、`FileId` の番号とも一致する。0 が Prelude、1 が入口で、その後に import と修飾子をたどって
    /// 見つけた順に、標準ライブラリのモジュールとユーザーのモジュールが並ぶ。セッション (`load_session`) では、1 から
    /// セッションのモジュールが入力の順に並び、最後の今の入力が入口である。その後に、見つけた順のモジュールが続く
    /// (docs/spec/modules.md の「標準ライブラリ」)。
    pub modules: Vec<LoadedModule>,
}

/// REPL のセッションの1つの入力。`form` は読み方と表示のパスを決める (docs/spec/modules.md の「REPL のセッション」)。
#[derive(Debug, Clone, Copy)]
pub struct SessionInput<'a> {
    pub number: u32,
    pub text: &'a str,
    pub form: InputForm<'a>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputForm<'a> {
    /// 端末で打った入力。`eml_syntax::parse_input` で読む。
    Typed,
    /// `:load` のファイル。`eml_syntax::parse` で読み、表示のパスは `path` である。
    File { path: &'a str },
    /// `:type` の問い合わせ。`eml_syntax::parse_query` で読む。
    Query,
}

/// セッションの import が解決した先と本文。後の入力でも同じモジュールを読むために持つ。ディスクのファイルが変わっても
/// セッションの中のモジュールは変わらない。読み直すと、同じ名前の関数が違うコードになるためである
/// (docs/spec/modules.md の「REPL のセッション」)。
#[derive(Debug, Clone, Default)]
pub struct ImportCache {
    entries: HashMap<ModulePath, Cached>,
}

#[derive(Debug, Clone)]
enum Cached {
    /// ユーザーの根のファイルの本文。
    User(String),
    /// ユーザーの根にファイルがなく (`NotFound`)、同じ名前の標準ライブラリのモジュールに落ちた。
    Std,
}

impl ImportCache {
    /// `other` の項目を足す。REPL は、入力を受け付けたときにだけ、その入力で読んだものを足す。
    pub fn extend(&mut self, other: ImportCache) {
        self.entries.extend(other.entries);
    }

    pub fn contains(&self, path: &ModulePath) -> bool {
        self.entries.contains_key(path)
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
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
/// 並びは `Prelude.em` を含める。extern の索引が Prelude の行を引くので、足りないと `def_map` が panic する。標準
/// ライブラリのモジュールは、使うものだけを読み込む (docs/spec/modules.md の「標準ライブラリ」)。
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
    let cache = ImportCache::default();
    let mut loader = Loader::new(std, root, entry_file, source, &cache);
    let prelude = loader.add_prelude();
    let entry = loader.add(MAIN, entry_path.to_string(), entry_text, ModuleOrigin::User);
    let (loaded, diagnostics, _) = loader.finish(prelude, entry);
    (loaded, diagnostics)
}

/// REPL のセッションを読む。受け付けた入力と今の入力 (最後) をセッションのモジュールにし、今の入力を入口にする
/// (docs/spec/modules.md の「REPL のセッション」)。`root` は import の表示の
/// パスの前に付けるディレクトリ (`/` で終わるか空) である。`entry_file` は `:load` のファイル名 (最後の `/` の後ろ) で、
/// なければ空である。ユーザーの import は、`cache` にあればその先と本文を使い、なければ `source` から読む。返す
/// `ImportCache` は、この呼び出しで新しく読んだものだけを持つ。
pub fn load_session(
    std: &[(&str, &str)],
    inputs: &[SessionInput<'_>],
    root: &str,
    entry_file: &str,
    source: &dyn ModuleSource,
    cache: &ImportCache,
) -> (Loaded, Vec<Diagnostic>, ImportCache) {
    // 入口のファイルは `:load` のファイルで、なければ空である。依存先と後の入力がそれを import すると E1030 になる
    let mut loader = Loader::new(std, root, entry_file, source, cache);
    let prelude = loader.add_prelude();
    debug_assert!(
        inputs
            .iter()
            .enumerate()
            .all(|(index, input)| index == 0 || !matches!(input.form, InputForm::File { .. })),
        "only the first input of a session is a file"
    );
    let mut entry = None;
    for input in inputs {
        let name = format!("<input {}>", input.number);
        let path = match input.form {
            InputForm::File { path } => path.to_string(),
            InputForm::Typed | InputForm::Query => name.clone(),
        };
        let origin = ModuleOrigin::Session(input.number);
        entry = Some(loader.add_session(&name, path, input.text, origin, input.form));
    }
    let entry = entry.expect("a session has the current input");
    loader.finish(prelude, entry)
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
    /// 埋め込んだ標準ライブラリ。モジュールは要るときに読む。
    std: &'a [(&'a str, &'a str)],
    /// 入口の表示のパスのディレクトリ (`/` で終わるか空)。根は入口のファイルのディレクトリである
    /// (docs/spec/modules.md の「モジュール」)。
    root: &'a str,
    entry_file: &'a str,
    source: &'a dyn ModuleSource,
    files: SourceFiles,
    modules: Vec<LoadedModule>,
    /// 読んだモジュール。標準ライブラリとユーザーの根に同じパスのモジュールがありうるので、出どころと組にする。
    /// ユーザーの根にファイルがなく標準ライブラリにフォールバックしたときも、`(User, path)` の鍵に標準ライブラリの
    /// モジュールを入れる。同じパスでユーザーの根を何度も探さないためである (docs/spec/modules.md の「標準ライブラリ」)。
    by_path: HashMap<(ModuleOrigin, ModulePath), ModuleId>,
    diagnostics: Vec<Diagnostic>,
    /// セッションの前の入力までに読んだ import。ファイルの読み込みでは空である。
    cache: &'a ImportCache,
    /// この読み込みで新しく読んだ import。
    read: ImportCache,
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

impl<'a> Loader<'a> {
    fn new(
        std: &'a [(&'a str, &'a str)],
        root: &'a str,
        entry_file: &'a str,
        source: &'a dyn ModuleSource,
        cache: &'a ImportCache,
    ) -> Loader<'a> {
        Loader {
            std,
            root,
            entry_file,
            source,
            files: SourceFiles::new(),
            modules: Vec::new(),
            by_path: HashMap::new(),
            diagnostics: Vec::new(),
            cache,
            read: ImportCache::default(),
        }
    }

    fn add_prelude(&mut self) -> FileId {
        let text = self
            .std
            .iter()
            .find(|&&(file, _)| file == PRELUDE_FILE)
            .map(|&(_, text)| text)
            .expect("the standard library has Prelude.em");
        self.add_std(PRELUDE_FILE, PRELUDE, PRELUDE_PATH.to_string(), text)
    }

    /// `std/` からのパスが `path` の標準ライブラリのモジュール。まだ読んでいなければ、ここで読んで末尾に足す。
    fn std_module(&mut self, path: &ModulePath) -> Option<ModuleId> {
        if let Some(&id) = self.by_path.get(&(ModuleOrigin::Std, path.clone())) {
            return Some(id);
        }
        let &(file, text) = self
            .std
            .iter()
            .find(|&&(file, _)| std_path(file) == *path)?;
        let id = module_id(self.modules.len());
        let name = format!("{STD_ROOT}.{}", path.dotted());
        self.add_std(file, &name, std_shown(file), text);
        Some(id)
    }

    /// ユーザーのモジュールの構文に修飾子として現れる標準ライブラリのモジュールを読む。修飾子は名前解決より前に、
    /// 位置によらず構文木の名前の経路のすべてから拾う。`Std.Fs` も数えるのは、E1031 の help が標準ライブラリの `Fs` を
    /// 引くためである (docs/spec/modules.md の「標準ライブラリ」)。
    fn load_qualified_std(&mut self, index: usize) {
        let qualifiers: Vec<ModulePath> = self.modules[index]
            .parse
            .paths()
            .filter_map(|path| {
                let mut segments: Vec<String> = path
                    .segments()
                    .map(|segment| segment.text().to_string())
                    .collect();
                segments.pop();
                (!segments.is_empty()).then_some(ModulePath(segments))
            })
            .collect();
        for qualifier in qualifiers {
            let path = std_relative(&qualifier).unwrap_or(qualifier);
            self.std_module(&path);
        }
    }

    /// import をたどり終えて、読み込みの結果にする。
    fn finish(mut self, prelude: FileId, entry: FileId) -> (Loaded, Vec<Diagnostic>, ImportCache) {
        // 見つけたモジュールを `modules` の末尾に足すので、前から順に処理すれば幅優先になる
        let mut next = 0;
        while next < self.modules.len() {
            self.resolve_imports(next);
            if !self.modules[next].origin.is_std() {
                self.load_qualified_std(next);
            }
            next += 1;
        }
        let Loader {
            files,
            modules,
            diagnostics,
            read,
            ..
        } = self;
        let loaded = Loaded {
            files,
            prelude,
            entry,
            modules,
        };
        (loaded, diagnostics, read)
    }

    /// ユーザーと標準ライブラリのモジュール。ファイルの文法で読む。
    fn add(&mut self, name: &str, path: String, text: &str, origin: ModuleOrigin) -> FileId {
        debug_assert!(
            origin.session().is_none(),
            "a session module goes through `add_session`"
        );
        self.add_parsed(name, path, text, origin, eml_syntax::parse)
    }

    /// セッションのモジュール。読み方は `form` だけで決める。
    fn add_session(
        &mut self,
        name: &str,
        path: String,
        text: &str,
        origin: ModuleOrigin,
        form: InputForm,
    ) -> FileId {
        let parse = match form {
            InputForm::Typed => eml_syntax::parse_input,
            InputForm::File { .. } => eml_syntax::parse,
            InputForm::Query => eml_syntax::parse_query,
        };
        self.add_parsed(name, path, text, origin, parse)
    }

    fn add_parsed(
        &mut self,
        name: &str,
        path: String,
        text: &str,
        origin: ModuleOrigin,
        parse: fn(FileId, &str) -> (Parse, Vec<Diagnostic>),
    ) -> FileId {
        let file = self.files.add(path, text);
        let (parse, errors) = parse(file, self.files.text(file));
        self.diagnostics.extend(errors);
        let (tree, stage) = item_tree(file, &parse);
        self.diagnostics.extend(stage);
        self.modules.push(LoadedModule {
            name: name.to_string(),
            origin,
            parse,
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
        if let Some(reserved) = self.reserved(origin, import) {
            let shown = format!("{}{}", self.root, import.path.file_path());
            self.diagnostics
                .push(reserved_import(file, import, reserved, &shown));
            return ImportTarget::Broken;
        }
        match (origin, std_relative(&import.path)) {
            (_, Some(path)) => self.std_target(file, import, &path),
            // 標準ライブラリはユーザーの根に左右されないよう、`std/` だけを探す。`import Fs` と `import Std.Fs` の
            // どちらでも書ける (docs/spec/modules.md の「標準ライブラリ」)
            (ModuleOrigin::Std, None) => self.std_target(file, import, &import.path),
            (ModuleOrigin::User | ModuleOrigin::Session(_), None) => self.user_target(file, import),
        }
    }

    /// ユーザーのモジュールの、`Std.` で始まらない import。ユーザーの根を探し、ファイルがない (`NotFound`) ときだけ
    /// 同じ名前の標準ライブラリのモジュールを探す。読めないファイルは書いたつもりのモジュールがある印なので、標準
    /// ライブラリへ進まない。
    fn user_target(&mut self, file: FileId, import: &ImportItem) -> ImportTarget {
        let shown = format!("{}{}", self.root, import.path.file_path());
        let key = (ModuleOrigin::User, import.path.clone());
        if let Some(&id) = self.by_path.get(&key) {
            return ImportTarget::Module(id);
        }
        match self.read_user(&import.path) {
            Ok(text) => {
                let id = module_id(self.modules.len());
                self.by_path.insert(key, id);
                self.remember(&import.path, Cached::User(text.clone()));
                self.add(&import.path.dotted(), shown, &text, ModuleOrigin::User);
                // ユーザーのモジュールが勝っても同じパスの標準ライブラリのモジュールを読むのは、そこにしかない名前を
                // 引いたときに `import Std.X as F` の help を付けるためである
                self.std_module(&import.path);
                ImportTarget::Module(id)
            }
            Err(ReadError::NotFound) => {
                match self.std_module(&import.path) {
                    Some(id) => {
                        // 同じパスを import するほかのモジュールが、ユーザーの根を読み直さないようにする
                        self.by_path.insert(key, id);
                        self.remember(&import.path, Cached::Std);
                        ImportTarget::Module(id)
                    }
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

    /// セッションの前の入力までに読んだものがあればそれを使い、なければ `source` から読む。標準ライブラリに落ちた
    /// ものは `NotFound` として返し、同じ標準ライブラリのモジュールに落とし直す。
    fn read_user(&self, path: &ModulePath) -> Result<String, ReadError> {
        match self.cache.entries.get(path) {
            Some(Cached::User(text)) => Ok(text.clone()),
            Some(Cached::Std) => Err(ReadError::NotFound),
            None => self.source.read(path),
        }
    }

    /// 前の入力までのキャッシュにない import だけを、この読み込みで新しく読んだものとして覚える。
    fn remember(&mut self, path: &ModulePath, cached: Cached) {
        if !self.cache.contains(path) {
            self.read.entries.insert(path.clone(), cached);
        }
    }

    /// `path` は `std/` からのパスである。
    fn std_target(&mut self, file: FileId, import: &ImportItem, path: &ModulePath) -> ImportTarget {
        match self.std_module(path) {
            Some(id) => ImportTarget::Module(id),
            None => {
                let shown = std_shown(&path.file_path());
                self.diagnostics
                    .push(unreadable(file, import, &ReadError::NotFound, &shown));
                ImportTarget::Broken
            }
        }
    }

    /// 標準ライブラリのモジュールも Prelude を暗黙に取り込むので、予約した名前の規則は出どころによらない。入口の
    /// ファイルはユーザーの根にあり、標準ライブラリの import は根を探さないので、入口の検査はユーザーの import だけに
    /// かける。
    fn reserved(&self, origin: ModuleOrigin, import: &ImportItem) -> Option<Reserved> {
        let name = import.path.dotted();
        if name == PRELUDE || name == MAIN || is_std_prelude(&name) {
            Some(Reserved::Name)
        } else if import.qualifier == PRELUDE {
            Some(Reserved::PreludeQualifier)
        } else if name == STD_ROOT {
            Some(Reserved::StdRoot)
        } else if !origin.is_std() && import.path.file_path() == self.entry_file {
            Some(Reserved::Entry)
        } else {
            None
        }
    }
}

/// `Std.` で始まるパスの、`std/` からのパス。
fn std_relative(path: &ModulePath) -> Option<ModulePath> {
    match path.0.as_slice() {
        [root, rest @ ..] if root == STD_ROOT && !rest.is_empty() => {
            Some(ModulePath(rest.to_vec()))
        }
        _ => None,
    }
}

/// `Std.Prelude` は Prelude のファイルを指すが、Prelude は暗黙に取り込むので `Prelude` と同じく予約する。別名のない
/// `import Std.Prelude` は修飾子が `Prelude` になるが、別名で直せる誤りではないので、修飾子の検査より先に引いて
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
