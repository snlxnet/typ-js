use std::path::{Path, PathBuf};

use typst::{
    Features, Library, LibraryExt, World,
    diag::SourceDiagnostic,
    ecow::EcoVec,
    foundations::{Bytes, Datetime},
    model::Document,
    syntax::{FileId, Source},
    text::{Font, FontBook},
    utils::LazyHash,
};
use typst_kit::{datetime::Time, files::FileStore, fonts::FontStore};
use typst_layout::PagedDocument;
use typst_pdf::PdfOptions;
use typst_svg::SvgFormat;
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

        pub fn delete(&mut self, path: &Path) {
            let mut files = self.files.lock().unwrap();

            files.remove(&self.find(path));
        }

        pub fn list(&self) -> Vec<String> {
            self.files
                .lock()
                .unwrap()
                .keys()
                .into_iter()
                .map(|id| id.vpath().get_without_slash().to_string())
                .collect()
        }

        pub fn write(&mut self, path: &Path, data: Bytes) {
            let Ok(mut fs) = self.files.lock() else {
                return;
            };

            fs.insert(self.find(path), data);
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

#[wasm_bindgen]
pub struct TypJs {
    lib: LazyHash<Library>,
    fonts: FontStore,
    files: FileStore<fs::FS>,
    errors: EcoVec<SourceDiagnostic>,
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
                Library::builder([typst_svg::FORMAT, typst_pdf::FORMAT])
                    .with_features(Features::all())
                    .build(),
            ), // stdlib
            fonts,
            files,
            errors: EcoVec::new(),
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

    /// Deletes a given file
    pub fn delete(&mut self, path: &str) {
        self.files.loader_mut().delete(Path::new(path));

        self.files.reset();
    }

    /// Returns the paths to all files available to the compiler,
    /// including `main.typ`.
    ///
    /// Paths do *NOT* start with `/`.
    pub fn list(&self) -> Vec<String> {
        self.files.loader().list()
    }

    /// Returns a list of errors if the last compilation failed or warnings if it finished successfully
    pub fn errors(&self) -> Vec<String> {
        self.errors
            .iter()
            .map(|err| {
                format!(
                    "SPAN: {:?} ||| MSG: {} ||| HINT: {}",
                    err.span,
                    err.message.clone(),
                    err.hints
                        .iter()
                        .map(|spanned| spanned.v.clone())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            })
            .collect()
    }

    /// Adds a binary file (image, font, etc.)
    pub fn add(&mut self, path: &str, data: Vec<u8>) {
        self.files
            .loader_mut()
            .write(Path::new(path), Bytes::new(data));
        self.files.reset();
    }

    pub async fn deps(&mut self, main: &str) {
        self.files.loader_mut().set_main(&PathBuf::from(main));
        let _ = typst::compile::<PagedDocument>(self);
        self.files.loader().download_packages().await;
    }

    /// Outputs an SVG string with the rendered document
    ///
    /// If there are compile errors, sets the `errors` field and returns empty string
    pub async fn svg(&mut self, main: &str) -> String {
        self.files.loader_mut().set_main(&PathBuf::from(main));
        let compiled = typst::compile::<PagedDocument>(self);

        match compiled.output {
            Err(errors) => {
                self.errors = errors;
                String::new()
            }
            Ok(doc) => {
                self.errors = compiled.warnings;
                doc.pages()
                    .iter()
                    .map(|page| {
                        typst_svg::svg(
                            page,
                            &typst_svg::SvgOptions {
                                render_bleed: false,
                                format: typst_svg::SvgFormatOptions {
                                    pretty: Some(false),
                                },
                            }
                            .resolve(doc.options().get::<SvgFormat>()),
                        )
                    })
                    .collect()
            }
        }
    }

    /// Outputs a PDF with the rendered document as a UInt8Array
    ///
    /// If there are compile errors, sets the `errors` field and returns empty array
    pub fn pdf(&mut self, main: &str) -> Vec<u8> {
        self.files.loader_mut().set_main(&PathBuf::from(main));
        let compiled = typst::compile::<PagedDocument>(self);

        match compiled.output {
            Err(errors) => {
                self.errors = errors;
                Vec::new()
            }
            Ok(doc) => {
                self.errors = compiled.warnings;
                typst_pdf::pdf(&doc, &PdfOptions::default())
                    .output
                    .unwrap_or_default()
            }
        }
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
