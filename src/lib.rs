use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
};

use chrono::{DateTime, Datelike, Local};
use typst::{
    Library, LibraryExt, World,
    diag::FileError,
    foundations::{Bytes, Datetime},
    layout::PagedDocument,
    syntax::{FileId, Source, VirtualPath},
    text::{Font, FontBook},
    utils::LazyHash,
};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct TypJs {
    main: FileId,
    lib: LazyHash<Library>,
    book: LazyHash<FontBook>,
    fonts: Vec<Font>,
    files: Mutex<HashMap<FileId, FileEntry>>,
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
            now: OnceLock::new(),
        }
    }

    pub fn rm(&mut self, name: &str) {
        let mut files = self.files.lock().unwrap();
        let id = FileId::from_name(name);

        files.remove(&id);
    }

    pub fn ls(&self) -> Vec<String> {
        self.files
            .lock()
            .unwrap()
            .keys()
            .into_iter()
            .flat_map(|id| id.vpath().as_rootless_path().to_str())
            .map(|str| str.to_string())
            .collect()
    }

    pub fn touch_text(&mut self, name: &str, text: &str) {
        let id = FileId::from_name(name);

        let Ok(mut fs) = self.files.lock() else {
            return;
        };

        fs.insert(id, FileEntry::Text(Source::new(id, text.to_string())));
    }

    pub fn touch_bin(&mut self, name: &str, data: Vec<u8>) {
        let path = format!("/{name}.typ");
        let id = FileId::from_path(&path);

        let Ok(mut fs) = self.files.lock() else {
            return;
        };

        fs.insert(id, FileEntry::Bin(Bytes::new(data)));
    }

    pub fn render_to_svg(&self) -> String {
        match typst::compile::<PagedDocument>(&self).output {
            Err(why) => why
                .iter()
                .map(|e| format!("{} [hint: {:?}];", e.message, e.hints))
                .collect(),
            Ok(doc) => doc.pages.iter().map(|page| typst_svg::svg(page)).collect(),
        }
    }

    // from obsidian-typst
    fn get_default_fonts() -> (FontBook, Vec<Font>) {
        let mut book = FontBook::new();
        let mut fonts = Vec::new();

        for bytes in typst_assets::fonts() {
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

#[wasm_bindgen]
pub fn add_one(x: u32) -> u32 {
    x + 1
}
