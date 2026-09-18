//! Safe wrapper over `xb_parse_html`.

use std::ffi::CString;

use anyhow::{anyhow, Result};

use crate::ffi;
use crate::AccentRule;

pub struct ParsedDoc {
    raw: *mut ffi::XbParsedDoc,
}

impl ParsedDoc {
    /// Parse the HTML buffer, applying the libzim accent rule to
    /// extracted content/keywords. For the latin rule, see
    /// [`Self::parse_with`]. Native failures retain their diagnostic in
    /// the returned error; malformed optional geo metadata is ignored.
    pub fn parse(html: &[u8]) -> Result<Self> {
        Self::parse_with(html, AccentRule::Libzim)
    }

    pub fn parse_with(html: &[u8], rule: AccentRule) -> Result<Self> {
        let rule_c = CString::new(rule.as_str())?;
        let raw =
            unsafe { ffi::xb_parse_html(html.as_ptr() as *const _, html.len(), rule_c.as_ptr()) };
        if raw.is_null() {
            Err(anyhow!("parsing HTML: {}", crate::last_error()))
        } else {
            Ok(ParsedDoc { raw })
        }
    }

    pub fn content(&self) -> &str {
        let mut len = 0usize;
        unsafe {
            let p = ffi::xb_pd_content(self.raw, &mut len);
            if p.is_null() || len == 0 {
                return "";
            }
            let bytes = std::slice::from_raw_parts(p.cast::<u8>(), len);
            std::str::from_utf8(bytes).unwrap_or("")
        }
    }

    pub fn keywords(&self) -> &str {
        let mut len = 0usize;
        unsafe {
            let p = ffi::xb_pd_keywords(self.raw, &mut len);
            if p.is_null() || len == 0 {
                return "";
            }
            let bytes = std::slice::from_raw_parts(p.cast::<u8>(), len);
            std::str::from_utf8(bytes).unwrap_or("")
        }
    }

    pub fn word_count(&self) -> u32 {
        unsafe { ffi::xb_pd_word_count(self.raw) }
    }
    pub fn indexing_allowed(&self) -> bool {
        unsafe { ffi::xb_pd_indexing_allowed(self.raw) != 0 }
    }
    pub fn geo(&self) -> Option<(f64, f64)> {
        unsafe {
            if ffi::xb_pd_has_geo(self.raw) == 0 {
                None
            } else {
                Some((
                    ffi::xb_pd_latitude(self.raw),
                    ffi::xb_pd_longitude(self.raw),
                ))
            }
        }
    }
}

impl Drop for ParsedDoc {
    fn drop(&mut self) {
        if !self.raw.is_null() {
            unsafe { ffi::xb_pd_free(self.raw) };
            self.raw = std::ptr::null_mut();
        }
    }
}
