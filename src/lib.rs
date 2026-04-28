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

pub struct Builder {
    raw: *mut ffi::XbBuilder,
    tmp_path: PathBuf,
    finalized: bool,
}

// SAFETY: every method that touches `raw` either takes `&mut self` or
// goes through a function the C++ side guards with its own mutex
// (xb_add_*). The C ABI is therefore safe to use from multiple threads
// holding shared references to the same builder; that's how the
// parallel feeder in main.rs uses it.
unsafe impl Send for Builder {}
unsafe impl Sync for Builder {}

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
        keep_termlists: bool,
        mode: Mode,
    ) -> Result<Self> {
        let tmp = path_to_cstring(tmp_path)?;
        let fin = path_to_cstring(final_path)?;
        let lang = CString::new(language_iso6393)?;
        let sw = CString::new(stopwords_text)?;
        let stemmer = CString::new(stemmer_override)?;

        let raw = unsafe {
            ffi::xb_builder_new(
                tmp.as_ptr(),
                fin.as_ptr(),
                lang.as_ptr(),
                sw.as_ptr(),
                stemmer.as_ptr(),
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

    /// Adds a title-DB document. Safe to call concurrently from
    /// multiple threads — the C++ side serialises actual database
    /// writes via an internal mutex.
    pub fn add_title(&self, path: &str, title: &str, target_path: &str) -> Result<()> {
        let path_c = CString::new(path)?;
        let title_c = CString::new(title)?;
        let target_c = CString::new(target_path)?;
        let rc = unsafe {
            ffi::xb_add_title(self.raw, path_c.as_ptr(), title_c.as_ptr(), target_c.as_ptr())
        };
        if rc != 0 {
            return Err(anyhow!("xb_add_title({path}): {}", last_error()));
        }
        Ok(())
    }

    /// Adds a fulltext-DB document. Concurrent-safe like `add_title`.
    pub fn add_fulltext(
        &self,
        path: &str,
        title: &str,
        content: &str,
        keywords: &str,
        word_count: u32,
        geo: Option<(f64, f64)>,
    ) -> Result<()> {
        let path_c = CString::new(path)?;
        let title_c = CString::new(title)?;
        let keywords_c = CString::new(keywords)?;
        // Content may contain interior NULs in pathological inputs; we
        // strip them so CString creation succeeds.
        let content_c = if content.contains('\0') {
            CString::new(content.replace('\0', " "))?
        } else {
            CString::new(content)?
        };
        let (has_geo, lat, lng) = match geo {
            Some((lat, lng)) => (1, lat, lng),
            None => (0, 0.0, 0.0),
        };
        let rc = unsafe {
            ffi::xb_add_fulltext(
                self.raw,
                path_c.as_ptr(),
                title_c.as_ptr(),
                content_c.as_ptr(),
                content.len(),
                keywords_c.as_ptr(),
                word_count,
                has_geo,
                lat,
                lng,
            )
        };
        if rc != 0 {
            return Err(anyhow!("xb_add_fulltext({path}): {}", last_error()));
        }
        Ok(())
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
