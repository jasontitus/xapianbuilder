//! Safe Rust wrappers over the C++ Xapian builder bridge.
//!
//! Everything that touches GPL-derived libzim/xapian code lives in
//! `cpp/bridge.cc`; this file is the thin RAII layer over it.

use std::ffi::{CStr, CString};
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use tempfile::TempDir;

mod ffi;
pub mod parse;
pub mod stopwords;

#[derive(Copy, Clone, Debug)]
pub enum Mode {
    Title,
    Fulltext,
}

impl Mode {
    fn as_int(self) -> i32 {
        match self {
            Mode::Title => 0,
            Mode::Fulltext => 1,
        }
    }
}

/// Which ICU accent-removal pipeline to apply before tokenisation.
///
/// `Libzim` uses `"Lower; NFD; [:M:] remove; NFC"`, removing combining
/// marks across scripts. `Latin` removes marks only in selected blocks
/// (`U+0300-036F`, `U+1AB0-1AFF`, `U+1DC0-1DFF`, `U+20D0-20FF`).
/// The latter preserves more non-Latin marks but differs from libzim;
/// neither option guarantees linguistically correct search for every script.
#[derive(Copy, Clone, Debug)]
pub enum AccentRule {
    Libzim,
    Latin,
}

impl AccentRule {
    pub fn as_str(self) -> &'static str {
        match self {
            AccentRule::Libzim => "libzim",
            AccentRule::Latin => "latin",
        }
    }
}

/// Index settings shared by all documents in one database.
pub struct BuilderOptions<'a> {
    pub language: &'a str,
    pub stopwords: &'a str,
    /// Empty derives from `language`; `"none"` disables stemming.
    pub stemmer: &'a str,
    pub accent_rule: AccentRule,
    pub keep_termlists: bool,
    pub mode: Mode,
}

/// Fulltext fields. Content and keywords must already be normalized;
/// the builder normalizes the original title using its accent rule.
pub struct FulltextDocument<'a> {
    pub path: &'a str,
    pub title: &'a str,
    pub content: &'a [u8],
    pub keywords: &'a [u8],
    pub word_count: u32,
    pub geo: Option<(f64, f64)>,
    pub language: &'a str,
}

pub struct Builder {
    raw: *mut ffi::XbBuilder,
    workspace: TempDir,
    final_path: PathBuf,
}

// SAFETY: every method that touches `raw` either consumes self,
// only reads builder state that is immutable after construction
// (xb_prepare_*), reads an atomic (xb_builder_is_empty), or goes through
// a function the C++ side guards with its own mutex (xb_add_doc).
// Shared references can therefore be used by the parallel feeder in main.rs.
unsafe impl Send for Builder {}
unsafe impl Sync for Builder {}

/// A fully tokenised Xapian document, produced by
/// [`Builder::prepare_title`] / [`Builder::prepare_fulltext`] (cheap
/// to build concurrently) and consumed by [`Builder::add_doc`] (the
/// serialised database write).
pub struct PreparedDoc {
    raw: *mut ffi::XbDoc,
}

// SAFETY: the underlying Xapian::Document is only ever touched by one
// thread at a time (built in a worker, then handed to the writer);
// transferring ownership across threads is fine.
unsafe impl Send for PreparedDoc {}

impl Drop for PreparedDoc {
    fn drop(&mut self) {
        if !self.raw.is_null() {
            unsafe { ffi::xb_doc_free(self.raw) };
            self.raw = std::ptr::null_mut();
        }
    }
}

impl Builder {
    /// Creates a private workspace beside `final_path`. Existing files,
    /// directories and symlinks are never overwritten. The parent must exist.
    ///
    /// Only this builder's workspace is removed on failure or drop. On success,
    /// a complete database is published atomically without replacing a competing
    /// writer's output. Scratch paths are deliberately not caller-configurable.
    pub fn new(final_path: &Path, options: BuilderOptions<'_>) -> Result<Self> {
        let filename = final_path.file_name().context("output must name a file")?;
        let parent = final_path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let parent = parent
            .canonicalize()
            .with_context(|| format!("opening output directory {}", parent.display()))?;
        let final_path = parent.join(filename);
        match std::fs::symlink_metadata(&final_path) {
            Ok(_) => bail!("output path already exists: {}", final_path.display()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error).context("checking output path"),
        }
        let workspace = tempfile::Builder::new()
            .prefix(".xapianbuilder-")
            .tempdir_in(parent)
            .context("creating private index workspace")?;
        let tmp = path_to_cstring(&workspace.path().join("database"))?;
        let fin = path_to_cstring(&workspace.path().join("compacted"))?;
        let lang = CString::new(options.language)?;
        let sw = CString::new(options.stopwords)?;
        let stemmer = CString::new(options.stemmer)?;
        let rule = CString::new(options.accent_rule.as_str())?;
        let raw = unsafe {
            ffi::xb_builder_new(
                tmp.as_ptr(),
                fin.as_ptr(),
                lang.as_ptr(),
                sw.as_ptr(),
                stemmer.as_ptr(),
                rule.as_ptr(),
                if options.keep_termlists { 1 } else { 0 },
                options.mode.as_int(),
            )
        };
        if raw.is_null() {
            return Err(anyhow!("xb_builder_new failed: {}", last_error()));
        }
        Ok(Builder {
            raw,
            workspace,
            final_path,
        })
    }

    pub fn is_empty(&self) -> bool {
        unsafe { ffi::xb_builder_is_empty(self.raw) != 0 }
    }

    /// Builds a title-DB document without touching the database. Runs
    /// the full tokenise/stem pipeline, so it's the expensive half;
    /// safe to call concurrently from multiple threads.
    ///
    /// `lang_override` (empty = inherit builder default) selects the
    /// per-doc Snowball stemmer; useful for multilingual ZIMs.
    pub fn prepare_title(
        &self,
        path: &str,
        title: &str,
        target_path: &str,
        lang_override: &str,
    ) -> Result<PreparedDoc> {
        let path_c = CString::new(path)?;
        let title_c = CString::new(title)?;
        let target_c = CString::new(target_path)?;
        let lang_c = CString::new(lang_override)?;
        let raw = unsafe {
            ffi::xb_prepare_title(
                self.raw,
                path_c.as_ptr(),
                title_c.as_ptr(),
                target_c.as_ptr(),
                lang_c.as_ptr(),
            )
        };
        if raw.is_null() {
            return Err(anyhow!("xb_prepare_title({path}): {}", last_error()));
        }
        Ok(PreparedDoc { raw })
    }

    /// Builds a fulltext-DB document without touching the database.
    /// Concurrent-safe like `prepare_title`. `content`/`keywords` are
    /// expected pre-processed (lowercased + accent-stripped, as
    /// produced by [`parse::ParsedDoc`]); `title` is raw — the
    /// builder's accent rule is applied to it internally, matching
    /// libzim.
    pub fn prepare_fulltext(&self, input: FulltextDocument<'_>) -> Result<PreparedDoc> {
        let FulltextDocument {
            path,
            title,
            content,
            keywords,
            word_count,
            geo,
            language,
        } = input;
        let path_c = CString::new(path)?;
        let title_c = CString::new(title)?;
        let lang_c = CString::new(language)?;
        let (has_geo, lat, lng) = match geo {
            Some((lat, lng)) => (1, lat, lng),
            None => (0, 0.0, 0.0),
        };
        let raw = unsafe {
            ffi::xb_prepare_fulltext(
                self.raw,
                path_c.as_ptr(),
                title_c.as_ptr(),
                content.as_ptr() as *const _,
                content.len(),
                keywords.as_ptr() as *const _,
                keywords.len(),
                word_count,
                has_geo,
                lat,
                lng,
                lang_c.as_ptr(),
            )
        };
        if raw.is_null() {
            return Err(anyhow!("xb_prepare_fulltext({path}): {}", last_error()));
        }
        Ok(PreparedDoc { raw })
    }

    /// Appends a prepared document to the database. Doc IDs are
    /// assigned in call order; the write is serialised by a mutex on
    /// the C++ side (matching libzim's `s_dbaccessLock`).
    pub fn add_doc(&self, doc: &PreparedDoc) -> Result<()> {
        let rc = unsafe { ffi::xb_add_doc(self.raw, doc.raw) };
        if rc != 0 {
            return Err(anyhow!("xb_add_doc: {}", last_error()));
        }
        Ok(())
    }

    /// Convenience: prepare + add in one call.
    pub fn add_title(
        &self,
        path: &str,
        title: &str,
        target_path: &str,
        lang_override: &str,
    ) -> Result<()> {
        self.add_doc(&self.prepare_title(path, title, target_path, lang_override)?)
    }

    /// Convenience: prepare + add in one call.
    pub fn add_fulltext(&self, input: FulltextDocument<'_>) -> Result<()> {
        self.add_doc(&self.prepare_fulltext(input)?)
    }

    /// Publishes a complete database, failing if another writer created the
    /// output meanwhile. Requires a filesystem supporting same-volume hard links.
    pub fn finalize(self) -> Result<()> {
        let rc = unsafe { ffi::xb_finalize(self.raw) };
        if rc != 0 {
            return Err(anyhow!("xb_finalize: {}", last_error()));
        }
        // Linking a private, complete file is atomic and cannot replace an
        // existing destination, including a dangling symlink. Both paths are
        // on the same filesystem. Drop removes only our private link/workspace.
        std::fs::hard_link(self.workspace.path().join("compacted"), &self.final_path)
            .with_context(|| {
                format!(
                    "publishing index without overwrite: {}",
                    self.final_path.display()
                )
            })?;
        Ok(())
    }
}

impl Drop for Builder {
    fn drop(&mut self) {
        if !self.raw.is_null() {
            unsafe { ffi::xb_builder_free(self.raw) };
            self.raw = std::ptr::null_mut();
        }
        // TempDir removes only the workspace exclusively created by this builder,
        // after the native database has closed. Never remove a caller-owned path.
    }
}

fn last_error() -> String {
    unsafe {
        let p = ffi::xb_last_error();
        if p.is_null() {
            String::new()
        } else {
            CStr::from_ptr(p).to_string_lossy().into_owned()
        }
    }
}

fn path_to_cstring(p: &Path) -> Result<CString> {
    let s = p
        .to_str()
        .with_context(|| format!("path is not valid UTF-8: {}", p.display()))?;
    Ok(CString::new(s)?)
}
