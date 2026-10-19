use std::path::{Path, PathBuf};

use serde::Serialize;
use typst::{
    Features, Library, LibraryExt, World,
    diag::{FileError, Severity, SourceDiagnostic},
    ecow::EcoVec,
    foundations::{Bytes, Datetime},
    model::Document,
    syntax::{DiagSpan, DiagSpanKind, FileId, Lines, Source},
    text::{Font, FontBook},
    utils::LazyHash,
};
use typst_kit::{datetime::Time, files::FileStore, fonts::FontStore};
use typst_layout::PagedDocument;
use typst_pdf::PdfOptions;
use typst_svg_mod::SvgFormat;
use wasm_bindgen::prelude::*;

mod fs {
    use std::{
        collections::HashMap,
        io::{Cursor, Read},
        path::{Path, PathBuf},
        sync::{Arc, Mutex},
    };

    use typst::{
        diag::FileError,
        foundations::Bytes,
        syntax::{FileId, RootedPath, VirtualPath, VirtualRoot},
    };
    use typst_kit::{
        downloader::Downloader,
        files::{FileLoader, FsRoot},
        packages::UniversePackages,
    };

    pub struct PackageLoader {
        saved: Arc<Mutex<HashMap<String, std::io::Result<(Option<usize>, bytes::Bytes)>>>>,
        missing: Arc<Mutex<Vec<String>>>,
    }
    impl Downloader for PackageLoader {
        fn stream(
            &self,
            _: &dyn std::any::Any,
            url: &str,
        ) -> std::io::Result<(Option<usize>, Box<dyn std::io::prelude::Read>)> {
            use std::io::Error;
            use std::io::ErrorKind;

            let err = Error::new(ErrorKind::NotFound, "Not Found");

            match self.saved.lock().unwrap().get(url) {
                Some(res) => match res {
                    Ok(data) => Ok((data.0, Box::new(Cursor::new(data.1.clone())))),
                    Err(_) => Err(err),
                },
                None => {
                    self.missing.lock().unwrap().push(url.to_string());
                    Err(err)
                }
            }
        }
    }

    pub struct FS {
        pub main: FileId,
        project_path: PathBuf,
        files: Mutex<HashMap<FileId, Bytes>>,
        fetch_missing: Arc<Mutex<Vec<String>>>,
        universe: UniversePackages,
        cache: Arc<Mutex<HashMap<String, std::io::Result<(Option<usize>, bytes::Bytes)>>>>,
    }

    fn str_to_rooted(root: &Path, path: &Path) -> RootedPath {
        let vpath = VirtualPath::virtualize(root, path).expect(&format!("{:?}", &path));

        RootedPath::new(VirtualRoot::Project, vpath)
    }

    impl FS {
        pub fn new() -> Self {
            let project = FsRoot::new(PathBuf::from(""));
            let project_path = project.path().to_path_buf();
            let main = str_to_rooted(&project_path, &PathBuf::from("main.typ")).intern();

            let cache = Arc::new(Mutex::new(HashMap::new()));
            let missing = Arc::new(Mutex::new(Vec::new()));

            Self {
                main,
                project_path,
                files: Mutex::new(HashMap::new()),
                universe: UniversePackages::new(PackageLoader {
                    saved: Arc::clone(&cache),
                    missing: Arc::clone(&missing),
                }),
                cache,
                fetch_missing: missing,
            }
        }

        pub fn set_main(&mut self, path: &Path) {
            self.main = str_to_rooted(&self.project_path, path).intern();
        }

        fn find(&self, path: &Path) -> FileId {
            let rooted = str_to_rooted(&self.project_path, path);

            FileId::new(rooted)
        }

        pub fn write(&mut self, path: &Path, data: Bytes) {
            let Ok(mut fs) = self.files.lock() else {
                return;
            };

            fs.insert(self.find(path), data);
        }

        pub fn clear(&mut self) {
            let files = self.files.get_mut().unwrap();
            files.drain();
        }

        pub async fn download_packages(&self) {
            let mut run = true;
            while run {
                let url = self.fetch_missing.lock().unwrap().pop();
                match url {
                    None => run = false,
                    Some(url) => {
                        self.fetch(&url).await;
                    }
                }
            }
        }

        async fn fetch(&self, url: &str) {
            use std::io::Error;
            use std::io::ErrorKind;

            let err = Error::new(ErrorKind::NotFound, "Not Found");

            let result = match reqwest::Client::new().get(url).send().await {
                Err(_) => Err(err),
                Ok(res) => {
                    let size: Option<usize> =
                        res.content_length().and_then(|n| usize::try_from(n).ok());

                    match res.bytes().await {
                        Err(_) => Err(err),
                        Ok(bytes) => Ok((size, bytes)),
                    }
                }
            };

            self.cache.lock().unwrap().insert(url.into(), result);
        }
    }

    impl FileLoader for FS {
        fn load(&self, id: FileId) -> typst::diag::FileResult<typst::foundations::Bytes> {
            let store = self.files.lock().map_err(|_| FileError::AccessDenied)?;

            match store.get(&id) {
                Some(bytes) => Ok(bytes.clone()),
                None => match id.root() {
                    VirtualRoot::Project => {
                        Err(FileError::NotFound(id.vpath().get_with_slash().into()))
                    }
                    VirtualRoot::Package(spec) => {
                        let pkg = self.universe.package(spec);
                        let err = Err(FileError::NotFound(id.vpath().get_with_slash().into()));

                        match pkg {
                            Ok(mut archive) => {
                                let wanted_path = id.vpath().get_without_slash();

                                for entry in archive.entries().unwrap() {
                                    let mut entry = entry.unwrap();

                                    let path = entry.path().unwrap();
                                    let path_ref = path.as_ref();
                                    if path_ref == Path::new(wanted_path) {
                                        let mut bytes = Vec::new();
                                        entry.read_to_end(&mut bytes).unwrap();
                                        return Ok(typst::foundations::Bytes::new(bytes));
                                    }
                                }

                                return err;
                            }
                            Err(_) => err,
                        }
                    }
                },
            }
        }
    }
}

#[derive(Serialize)]
struct SvgResult {
    svg: String,
    err: Vec<Error>,
}

#[derive(Serialize)]
struct PdfResult {
    #[serde(with = "serde_bytes")]
    pdf: Vec<u8>,
    err: Vec<Error>,
}

#[derive(Serialize)]
struct Error {
    severe: bool,
    line: usize,
    message: String,
    hints: Vec<String>,

    start: Position,
    end: Position,
}
#[derive(Serialize)]
struct Position {
    line: usize,
    character: usize,
}

type CodespanResult<T> = Result<T, CodespanError>;
type CodespanError = codespan_reporting::files::Error;

#[wasm_bindgen]
pub struct TypJs {
    lib: LazyHash<Library>,
    fonts: FontStore,
    files: FileStore<fs::FS>,
    now: Time,
}

#[wasm_bindgen]
impl TypJs {
    pub fn new() -> Self {
        console_log::init_with_level(log::Level::Debug);

        let mut fonts = FontStore::new();
        fonts.extend(typst_kit::fonts::embedded());

        let mut files = FileStore::new(fs::FS::new());
        files
            .loader_mut()
            .write(Path::new("/main.typ"), Bytes::new(""));

        Self {
            lib: LazyHash::new(
                Library::builder([typst_svg_mod::FORMAT, typst_pdf::FORMAT])
                    .with_features(Features::all())
                    .build(),
            ), // stdlib
            fonts,
            files,
            now: Time::system(),
        }
    }

    pub fn font(&mut self, data: Vec<u8>) {
        let fonts = Font::iter(Bytes::new(data)).map(|font| {
            let info = font.info().clone();
            (font, info)
        });
        self.fonts.extend(fonts);
    }

    /// Deletes everything in the filesystem (fonts & package cache are left intact)
    pub fn reset(&mut self) {
        self.files.loader_mut().clear();
        self.files.reset();
    }

    /// Adds a file
    pub fn add(&mut self, path: &str, data: Vec<u8>) {
        self.files
            .loader_mut()
            .write(Path::new(path), Bytes::new(data));
        self.files.reset();
    }

    /// Fetches universe packages for a given main file
    pub async fn deps(&mut self, main: &str) {
        self.files.loader_mut().set_main(&PathBuf::from(main));
        let _ = typst::compile::<PagedDocument>(self);
        self.files.loader().download_packages().await;
    }

    /// Renders as SVG
    pub async fn svg(&mut self, main: &str) -> JsValue {
        self.files.loader_mut().set_main(&PathBuf::from(main));
        let compiled = typst::compile::<PagedDocument>(self);

        let result = match compiled.output {
            Err(errors) => SvgResult {
                svg: String::new(),
                err: self.format_diagnostics(errors),
            },
            Ok(doc) => {
                let svg = doc
                    .pages()
                    .iter()
                    .map(|page| {
                        typst_svg_mod::svg(
                            page,
                            &typst_svg_mod::SvgOptions {
                                render_bleed: false,
                                format: typst_svg_mod::SvgFormatOptions {
                                    pretty: Some(false),
                                },
                            }
                            .resolve(doc.options().get::<SvgFormat>()),
                        )
                    })
                    .collect();

                SvgResult {
                    svg,
                    err: self.format_diagnostics(compiled.warnings),
                }
            }
        };

        serde_wasm_bindgen::to_value(&result).unwrap()
    }

    /// Renders as PDF
    pub async fn pdf(&mut self, main: &str) -> JsValue {
        self.files.loader_mut().set_main(&PathBuf::from(main));
        let compiled = typst::compile::<PagedDocument>(self);

        let result = match compiled.output {
            Err(errors) => PdfResult {
                pdf: Vec::new(),
                err: self.format_diagnostics(errors),
            },
            Ok(doc) => PdfResult {
                pdf: typst_pdf::pdf(&doc, &PdfOptions::default())
                    .output
                    .unwrap_or_default(),
                err: self.format_diagnostics(compiled.warnings),
            },
        };

        serde_wasm_bindgen::to_value(&result).unwrap()
    }

    fn format_diagnostics(&mut self, diagnostics: EcoVec<SourceDiagnostic>) -> Vec<Error> {
        diagnostics
            .iter()
            .filter_map(|diagnostic| {
                let message = diagnostic.message.clone().to_string();
                let hints: Vec<String> = diagnostic
                    .hints
                    .iter()
                    .filter(|s| s.span.is_detached())
                    .map(|s| format!("hint: {}", s.v))
                    .collect();
                let Some(range) = self.span_to_range(diagnostic.span) else {
                    return None;
                };
                let Some(id) = diagnostic.span.id() else {
                    return None;
                };
                let Ok(start_line) = self.line_index(id, range.start) else {
                    return None;
                };
                let Ok(start_character) = self.column_number(id, range.start) else {
                    return None;
                };
                let Ok(end_line) = self.line_index(id, range.end) else {
                    return None;
                };
                let Ok(end_character) = self.column_number(id, range.end) else {
                    return None;
                };
                let err = Error {
                    severe: diagnostic.severity == Severity::Error,
                    line: start_line,
                    message,
                    hints,
                    start: Position {
                        line: start_line,
                        character: start_character,
                    },
                    end: Position {
                        line: end_line,
                        character: end_character,
                    },
                };
                Some(err)
            })
            .collect()
    }
    // pretty much from typst-kit/src/diagnostics.rs
    fn span_to_range(&mut self, span: impl Into<DiagSpan>) -> Option<std::ops::Range<usize>> {
        match span.into().get() {
            DiagSpanKind::Detached => None,
            DiagSpanKind::Number { id, num, sub_range } => {
                let source = self.source(id).ok()?;
                source.range(num, sub_range)
            }
            DiagSpanKind::Range { id: _, range } => Some(range),
        }
    }
    fn lines(&self, id: FileId) -> CodespanResult<Lines<String>> {
        match self.source(id).ok() {
            Some(source) => Ok(source.lines().clone()),
            None => self
                .file(id)
                .and_then(|file| file.lines().map_err(Into::into))
                .map_err(|err| match err {
                    FileError::NotFound(_) => CodespanError::FileMissing,
                    other => CodespanError::Io(std::io::Error::other(other)),
                }),
        }
    }
    fn line_index(&self, id: FileId, given: usize) -> CodespanResult<usize> {
        let lines = self.lines(id)?;
        lines
            .byte_to_line(given)
            .ok_or_else(|| CodespanError::IndexTooLarge {
                given,
                max: lines.len_bytes(),
            })
    }
    fn column_number(&self, id: FileId, given: usize) -> CodespanResult<usize> {
        let lines = self.lines(id)?;
        lines.byte_to_column(given).ok_or_else(|| {
            let max = lines.len_bytes();
            if given <= max {
                CodespanError::InvalidCharBoundary { given }
            } else {
                CodespanError::IndexTooLarge { given, max }
            }
        })
    }
}

impl World for TypJs {
    fn library(&self) -> &LazyHash<Library> {
        &self.lib
    }

    fn book(&self) -> &LazyHash<FontBook> {
        &self.fonts.book()
    }

    fn main(&self) -> FileId {
        self.files.loader().main
    }

    fn source(&self, id: FileId) -> typst::diag::FileResult<Source> {
        self.files.source(id)
    }

    fn file(&self, id: FileId) -> typst::diag::FileResult<Bytes> {
        self.files.file(id)
    }

    fn font(&self, index: usize) -> Option<Font> {
        self.fonts.font(index)
    }

    fn today(&self, offset: Option<typst::foundations::Duration>) -> Option<Datetime> {
        self.now.today(offset)
    }
}
