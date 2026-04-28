//! Raw `extern "C"` bindings to `cpp/bridge.h`.

use std::ffi::{c_char, c_double, c_int, c_uint};

#[repr(C)]
pub struct XbBuilder {
    _private: [u8; 0],
}

#[repr(C)]
pub struct XbParsedDoc {
    _private: [u8; 0],
}

unsafe extern "C" {
    pub fn xb_builder_new(
        tmp_path: *const c_char,
        final_path: *const c_char,
        language_iso6393: *const c_char,
        stopwords_text: *const c_char,
        stemmer_override: *const c_char,
        keep_termlists: c_int,
        mode: c_int,
    ) -> *mut XbBuilder;

    pub fn xb_builder_free(b: *mut XbBuilder);

    pub fn xb_builder_is_empty(b: *const XbBuilder) -> c_int;

    pub fn xb_add_title(
        b: *mut XbBuilder,
        path: *const c_char,
        title: *const c_char,
        target_path: *const c_char,
    ) -> c_int;

    pub fn xb_add_fulltext(
        b: *mut XbBuilder,
        path: *const c_char,
        title: *const c_char,
        content: *const c_char,
        content_len: usize,
        keywords: *const c_char,
        word_count: c_uint,
        has_geo: c_int,
        latitude: c_double,
        longitude: c_double,
    ) -> c_int;

    pub fn xb_finalize(b: *mut XbBuilder) -> c_int;

    pub fn xb_parse_html(html: *const c_char, len: usize) -> *mut XbParsedDoc;
    pub fn xb_pd_content(p: *const XbParsedDoc, out_len: *mut usize) -> *const c_char;
    pub fn xb_pd_keywords(p: *const XbParsedDoc) -> *const c_char;
    pub fn xb_pd_word_count(p: *const XbParsedDoc) -> c_uint;
    pub fn xb_pd_indexing_allowed(p: *const XbParsedDoc) -> c_int;
    pub fn xb_pd_has_geo(p: *const XbParsedDoc) -> c_int;
    pub fn xb_pd_latitude(p: *const XbParsedDoc) -> c_double;
    pub fn xb_pd_longitude(p: *const XbParsedDoc) -> c_double;
    pub fn xb_pd_free(p: *mut XbParsedDoc);

    pub fn xb_last_error() -> *const c_char;
}
