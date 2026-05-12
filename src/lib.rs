use std::path::Path;

use typst::{
    Library, LibraryExt, World,
    diag::SourceDiagnostic,
    ecow::EcoVec,
    foundations::{Bytes, Datetime},
    syntax::{FileId, Source},
    text::{Font, FontBook},
    utils::LazyHash,
};
use typst_kit::{datetime::Time, files::FileStore, fonts::FontStore};
use typst_layout::PagedDocument;
use typst_pdf::PdfOptions;
use wasm_bindgen::prelude::*;

mod fs {
    use std::{
        collections::HashMap,
        path::{Path, PathBuf},
        sync::Mutex,
    };

    use typst::{
        diag::FileError,
        foundations::Bytes,
        syntax::{FileId, RootedPath, VirtualPath, VirtualRoot},
    };
    use typst_kit::files::{FileLoader, FsRoot};

    pub struct FS {
        pub main: FileId,
        project_path: PathBuf,
        files: Mutex<HashMap<FileId, Bytes>>,
    }

    fn str_to_rooted(root: &Path, path: &Path) -> RootedPath {
        let vpath = VirtualPath::virtualize(root, path).expect(&format!("{:?}", &path));

        RootedPath::new(VirtualRoot::Project, vpath)
    }

    impl FS {
        pub fn new() -> Self {
            let project = FsRoot::new(PathBuf::from(""));
            let project_path = project.path().to_path_buf();
            let path = PathBuf::from("main.typ");
            let main = str_to_rooted(&project_path, &path).intern();

            Self {
                main,
                project_path,
                files: Mutex::new(HashMap::new()),
            }
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
    }

    impl FileLoader for FS {
        fn load(&self, id: FileId) -> typst::diag::FileResult<typst::foundations::Bytes> {
            let store = self.files.lock().map_err(|_| FileError::AccessDenied)?;

            match store.get(&id) {
                Some(bytes) => Ok(bytes.clone()),
                None => Err(FileError::NotFound(id.vpath().get_with_slash().into())),
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
        let mut fonts = FontStore::new();
        fonts.extend(typst_kit::fonts::embedded());

        let mut files = FileStore::new(fs::FS::new());
        files
            .loader_mut()
            .write(Path::new("/main.typ"), Bytes::new("Hello"));

        Self {
            lib: LazyHash::new(Library::default()), // stdlib
            fonts,
            files,
            errors: EcoVec::new(),
            now: Time::system(),
        }
    }

    /// Deletes a given file
    pub fn delete(&mut self, path: &str) {
        self.files.loader_mut().delete(Path::new(path));
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

    /// Sets the text content of a given `.typ` file.
    ///
    /// The root file is called `main.typ`
    pub fn write(&mut self, path: &str, text: String) {
        self.files
            .loader_mut()
            .write(Path::new(path), Bytes::new(text));
    }

    /// Adds a binary file (image, font, etc.)
    pub fn attach(&mut self, path: &str, data: Vec<u8>) {
        self.files
            .loader_mut()
            .write(Path::new(path), Bytes::new(data));
    }

    /// Outputs an SVG string with the rendered document
    ///
    /// If there are compile errors, sets the `errors` field and returns empty string
    pub fn svg(&mut self) -> String {
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
                    .map(|page| typst_svg::svg(page))
                    .collect()
            }
        }
    }

    /// Outputs a PDF with the rendered document as a UInt8Array
    ///
    /// If there are compile errors, sets the `errors` field and returns empty array
    pub fn pdf(&mut self) -> Vec<u8> {
        let compiled = typst::compile::<PagedDocument>(self);

        match compiled.output {
            Err(errors) => {
                self.errors = errors;
                Vec::new()
            }
            Ok(doc) => {
                self.errors = compiled.warnings;
                typst_pdf::pdf(&doc, &PdfOptions::default()).unwrap_or_default()
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
