use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
};

use chrono::{DateTime, Datelike, Local};
use typst::{
    Library, LibraryExt, World,
    diag::{FileError, SourceDiagnostic},
    ecow::EcoVec,
    foundations::{Bytes, Datetime},
    layout::PagedDocument,
    syntax::{FileId, Source, VirtualPath},
    text::{Font, FontBook},
    utils::LazyHash,
};
use typst_pdf::PdfOptions;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct TypJs {
    main: FileId,
    lib: LazyHash<Library>,
    book: LazyHash<FontBook>,
    fonts: Vec<Font>,
    files: Mutex<HashMap<FileId, FileEntry>>,
    errors: EcoVec<SourceDiagnostic>,
    now: OnceLock<DateTime<Local>>,
}

pub enum FileEntry {
    Bin(Bytes),
    Text(Source),
}

trait FromPath {
    fn from_path(path: &str) -> Self;
    fn from_name(name: &str) -> Self;
}

impl FromPath for FileId {
    fn from_path(path: &str) -> Self {
        Self::new(None, VirtualPath::new(path))
    }

    fn from_name(name: &str) -> Self {
        Self::new(None, VirtualPath::new(format!("/{name}")))
    }
}

#[wasm_bindgen]
impl TypJs {
    pub fn new() -> Self {
        let (book, fonts) = Self::get_default_fonts();
        let main = FileId::from_path("/main.typ");

        let files = Mutex::new(HashMap::from([(
            main,
            FileEntry::Text(Source::new(main, "typ-js is ready".to_string())),
        )]));

        Self {
            main,
            lib: LazyHash::new(Library::default()), // stdlib
            book: LazyHash::new(book),
            fonts,
            files,
            errors: EcoVec::new(),
            now: OnceLock::new(),
        }
    }

    /// Deletes a given file
    pub fn delete(&mut self, name: &str) {
        let mut files = self.files.lock().unwrap();
        let id = FileId::from_name(name);

        files.remove(&id);
    }

    /// Returns the paths to all files available to the compiler,
    /// including `main.typ`.
    ///
    /// Paths do *NOT* start with `/`.
    pub fn list(&self) -> Vec<String> {
        self.files
            .lock()
            .unwrap()
            .keys()
            .into_iter()
            .flat_map(|id| id.vpath().as_rootless_path().to_str())
            .map(|str| str.to_string())
            .collect()
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
                    err.hints.join(", ")
                )
            })
            .collect()
    }

    /// Sets the text content of a given `.typ` file.
    ///
    /// The root file is called `main.typ`
    pub fn write(&mut self, filename: &str, text: &str) {
        let id = FileId::from_name(filename);

        let Ok(mut fs) = self.files.lock() else {
            return;
        };

        fs.insert(id, FileEntry::Text(Source::new(id, text.to_string())));
    }

    /// Adds a binary file (image, font, etc.)
    pub fn attach(&mut self, filename: &str, data: Vec<u8>) {
        let path = format!("/{filename}");
        let id = FileId::from_path(&path);

        let Ok(mut fs) = self.files.lock() else {
            return;
        };

        fs.insert(id, FileEntry::Bin(Bytes::new(data)));
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
                doc.pages.iter().map(|page| typst_svg::svg(page)).collect()
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

    // from obsidian-typst
    fn get_default_fonts() -> (FontBook, Vec<Font>) {
        let mut book = FontBook::new();
        let mut fonts = Vec::new();
        let list = typst_assets::fonts().chain(
            [
                include_bytes!("../fonts/JetBrainsMono-BoldItalic.ttf").as_slice(),
                include_bytes!("../fonts/JetBrainsMono-Bold.ttf"),
                include_bytes!("../fonts/JetBrainsMono-ExtraBoldItalic.ttf"),
                include_bytes!("../fonts/JetBrainsMono-ExtraBold.ttf"),
                include_bytes!("../fonts/JetBrainsMono-ExtraLightItalic.ttf"),
                include_bytes!("../fonts/JetBrainsMono-ExtraLight.ttf"),
                include_bytes!("../fonts/JetBrainsMono-Italic.ttf"),
                include_bytes!("../fonts/JetBrainsMono-LightItalic.ttf"),
                include_bytes!("../fonts/JetBrainsMono-Light.ttf"),
                include_bytes!("../fonts/JetBrainsMono-MediumItalic.ttf"),
                include_bytes!("../fonts/JetBrainsMono-Medium.ttf"),
                include_bytes!("../fonts/JetBrainsMono-Regular.ttf"),
                include_bytes!("../fonts/JetBrainsMono-SemiBoldItalic.ttf"),
                include_bytes!("../fonts/JetBrainsMono-SemiBold.ttf"),
                include_bytes!("../fonts/JetBrainsMono-ThinItalic.ttf"),
                include_bytes!("../fonts/JetBrainsMono-Thin.ttf"),
            ]
            .into_iter(),
        );

        for bytes in list {
            let buffer = Bytes::new(bytes);
            for font in Font::iter(buffer) {
                book.push(font.info().clone());
                fonts.push(font);
            }
        }

        return (book, fonts);
    }
}

impl World for TypJs {
    fn library(&self) -> &LazyHash<Library> {
        &self.lib
    }

    fn book(&self) -> &LazyHash<FontBook> {
        &self.book
    }

    fn main(&self) -> FileId {
        // FileId::new(None, VirtualPath::new(""))
        self.main
    }

    fn source(&self, id: FileId) -> typst::diag::FileResult<Source> {
        let fs = self.files.lock().map_err(|_| FileError::AccessDenied)?;

        match fs.get(&id) {
            Some(FileEntry::Text(source)) => Ok(source.clone()),
            Some(FileEntry::Bin(_)) => Err(FileError::NotSource),
            None => Err(FileError::NotFound(
                id.vpath().as_rootless_path().to_path_buf(),
            )),
        }
    }

    fn file(&self, id: FileId) -> typst::diag::FileResult<Bytes> {
        let fs = self.files.lock().map_err(|_| FileError::AccessDenied)?;

        match fs.get(&id) {
            Some(FileEntry::Text(source)) => Ok(Bytes::from_string(source.text().to_string())),
            Some(FileEntry::Bin(bytes)) => Ok(bytes.clone()),
            None => Err(FileError::NotFound(
                id.vpath().as_rootless_path().to_path_buf(),
            )),
        }
    }

    fn font(&self, index: usize) -> Option<Font> {
        Some(self.fonts[index].clone())
    }

    fn today(&self, offset: Option<i64>) -> Option<Datetime> {
        let now = self.now.get_or_init(chrono::Local::now);

        let naive = match offset {
            None => now.naive_local(),
            Some(o) => now.naive_utc() + chrono::Duration::hours(o),
        };

        Datetime::from_ymd(
            naive.year(),
            naive.month().try_into().ok()?,
            naive.day().try_into().ok()?,
        )
    }
}
