//! Safe Rust wrappers over the C++ Xapian builder bridge.
//!
//! Everything that touches GPL-derived libzim/xapian code lives in
//! `cpp/bridge.cc`; this file is the thin RAII layer over it.

use std::ffi::{CStr, CString};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};

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
/// `Libzim` matches kiwix exactly (`"Lower; NFD; [:M:] remove; NFC"`)
/// and fragments Indic/Thai/Arabic vowel marks as a side-effect.
/// `Latin` only strips combining marks in the Latin/IPA blocks
/// (`U+0300-036F`, `U+1AB0-1AFF`, `U+1DC0-1DFF`, `U+20D0-20FF`),
/// preserving Indic/Thai/Arabic correctness at the cost of byte
/// divergence from libzim on those scripts.
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

pub struct Builder {
    raw: *mut ffi::XbBuilder,
    tmp_path: PathBuf,
    finalized: bool,
}

// SAFETY: every method that touches `raw` either takes `&mut self`,
// only reads builder state that is immutable after construction
// (xb_prepare_*), or goes through a function the C++ side guards with
// its own mutex (xb_add_doc). The C ABI is therefore safe to use from
// multiple threads holding shared references to the same builder;
// that's how the parallel feeder in main.rs uses it.
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
    /// `tmp_path` is the workspace where the WritableDatabase is written
    /// before compaction; `final_path` is where the single-file glass
    /// blob ends up. They must be different paths (libzim convention is
    /// `<final>.tmp`). The temp path is removed on Drop regardless of
    /// whether `finalize()` succeeded, so callers don't have to clean
    /// up on error.
    ///
    /// `stemmer_override` is forwarded straight to `Xapian::Stem` if
    /// non-empty: pass `"porter"` for old-style stemming (matches
    /// pre-2024 kiwix ZIMs), `""` to derive the stemmer from
    /// `language_iso6393` via ICU, `"none"` to disable stemming.
    pub fn new(
        tmp_path: &Path,
        final_path: &Path,
        language_iso6393: &str,
        stopwords_text: &str,
        stemmer_override: &str,
        accent_rule: AccentRule,
        keep_termlists: bool,
        mode: Mode,
    ) -> Result<Self> {
        let tmp = path_to_cstring(tmp_path)?;
        let fin = path_to_cstring(final_path)?;
        let lang = CString::new(language_iso6393)?;
        let sw = CString::new(stopwords_text)?;
        let stemmer = CString::new(stemmer_override)?;
        let rule = CString::new(accent_rule.as_str())?;

        let raw = unsafe {
            ffi::xb_builder_new(
                tmp.as_ptr(),
                fin.as_ptr(),
                lang.as_ptr(),
                sw.as_ptr(),
                stemmer.as_ptr(),
                rule.as_ptr(),
                if keep_termlists { 1 } else { 0 },
                mode.as_int(),
            )
        };
        if raw.is_null() {
            return Err(anyhow!("xb_builder_new failed: {}", last_error()));
        }
        Ok(Builder {
            raw,
            tmp_path: tmp_path.to_path_buf(),
            finalized: false,
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
    pub fn prepare_fulltext(
        &self,
        path: &str,
        title: &str,
        content: &[u8],
        keywords: &[u8],
        word_count: u32,
        geo: Option<(f64, f64)>,
        lang_override: &str,
    ) -> Result<PreparedDoc> {
        let path_c = CString::new(path)?;
        let title_c = CString::new(title)?;
        let lang_c = CString::new(lang_override)?;
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
    pub fn add_fulltext(
        &self,
        path: &str,
        title: &str,
        content: &str,
        keywords: &str,
        word_count: u32,
        geo: Option<(f64, f64)>,
        lang_override: &str,
    ) -> Result<()> {
        self.add_doc(&self.prepare_fulltext(
            path,
            title,
            content.as_bytes(),
            keywords.as_bytes(),
            word_count,
            geo,
            lang_override,
        )?)
    }

    pub fn finalize(mut self) -> Result<()> {
        let rc = unsafe { ffi::xb_finalize(self.raw) };
        self.finalized = true;
        if rc != 0 {
            return Err(anyhow!("xb_finalize: {}", last_error()));
        }
        Ok(())
    }
}

impl Drop for Builder {
    fn drop(&mut self) {
        if !self.raw.is_null() {
            unsafe { ffi::xb_builder_free(self.raw) };
            self.raw = std::ptr::null_mut();
        }
        // Always clean up the temp directory; libzim does the same in
        // its indexer destructor. Errors here are best-effort — the
        // caller may already be unwinding from a different failure.
        if self.tmp_path.exists() {
            let _ = std::fs::remove_dir_all(&self.tmp_path);
        }
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
    let s = p.to_str().with_context(|| {
        format!("path is not valid UTF-8: {}", p.display())
    })?;
    Ok(CString::new(s)?)
}
