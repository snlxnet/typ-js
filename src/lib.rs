use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Mutex, OnceLock},
};

use chrono::{DateTime, Datelike, Local};
use typst::{
    Library, LibraryExt, World,
    diag::FileError,
    foundations::{Bytes, Datetime},
    syntax::{FileId, Source, VirtualPath},
    text::{Font, FontBook},
    utils::LazyHash,
};
use wasm_bindgen::prelude::*;

pub struct WrapperWorld {
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
}

impl FromPath for FileId {
    fn from_path(path: &str) -> Self {
        Self::new(None, VirtualPath::new(path))
    }
}

impl WrapperWorld {
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

    pub fn rm(&mut self, id: FileId) {
        let mut files = self.files.lock().unwrap();

        files.remove(&id);
    }

    pub fn ls(&self) -> Vec<FileId> {
        self.files
            .lock()
            .unwrap()
            .keys()
            .into_iter()
            .map(|id| id.clone())
            .collect()
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

    pub fn touch_text(&mut self, name: &str, text: &str) {
        let path = format!("/{name}.typ");
        let id = FileId::from_path(&path);

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
}

impl World for WrapperWorld {
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
