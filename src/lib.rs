//! Safe Rust wrappers over the C++ Xapian builder bridge.
//!
//! Everything that touches GPL-derived libzim/xapian code lives in
//! `cpp/bridge.cc`; this file is the thin RAII layer over it.

use std::ffi::{CStr, CString};
use std::path::Path;

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
    finalized: bool,
}

impl Builder {
    /// `tmp_path` is the workspace where the WritableDatabase is written
    /// before compaction; `final_path` is where the single-file glass
    /// blob ends up. They must be different paths (libzim convention is
    /// `<final>.tmp`).
    pub fn new(
        tmp_path: &Path,
        final_path: &Path,
        language_iso6393: &str,
        stopwords_text: &str,
        mode: Mode,
    ) -> Result<Self> {
        let tmp = path_to_cstring(tmp_path)?;
        let fin = path_to_cstring(final_path)?;
        let lang = CString::new(language_iso6393)?;
        let sw = CString::new(stopwords_text)?;

        let raw = unsafe {
            ffi::xb_builder_new(
                tmp.as_ptr(),
                fin.as_ptr(),
                lang.as_ptr(),
                sw.as_ptr(),
                mode.as_int(),
            )
        };
        if raw.is_null() {
            return Err(anyhow!("xb_builder_new failed: {}", last_error()));
        }
        Ok(Builder {
            raw,
            finalized: false,
        })
    }

    pub fn add_title(&mut self, path: &str, title: &str, target_path: &str) -> Result<()> {
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

    pub fn add_fulltext(
        &mut self,
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
