//! Stopword lists vendored from libzim's `static/stopwords/<lang>`.
//!
//! Lookup keys are the *short* ISO-639-1 codes that ICU produces from
//! ISO-639-3 input (e.g. "eng" -> "en"). Returns an empty string when
//! we don't ship a list for that language — matches libzim's "no
//! stopwords for this language" path.

use std::collections::HashMap;
use std::sync::OnceLock;

macro_rules! sw {
    ($($code:literal),* $(,)?) => {{
        let mut m = HashMap::new();
        $(
            m.insert(
                $code,
                include_str!(concat!("../data/stopwords/", $code)),
            );
        )*
        m
    }};
}

fn table() -> &'static HashMap<&'static str, &'static str> {
    static TABLE: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    TABLE.get_or_init(|| {
        // Keep this list in sync with `data/stopwords/`.
        sw!(
            "af", "ar", "bg", "bn", "br", "ca", "cs", "da", "de", "el",
            "en", "eo", "es", "et", "eu", "fa", "fi", "fr", "ga", "gl",
            "gu", "ha", "he", "hi", "hr", "hu", "hy", "id", "it", "ja",
            "ko", "ku", "la", "lt", "lv", "mr", "ms", "nl", "no", "pl",
            "pt", "ro", "ru", "sk", "sl", "so", "st", "sv", "sw", "th",
            "tl", "tr", "uk", "ur", "vi", "yo", "zh", "zu",
        )
    })
}

/// Looks up by ISO-639-1 short code (the form ICU's `Locale` returns).
pub fn for_short_code(code: &str) -> &'static str {
    table().get(code).copied().unwrap_or("")
}

/// Looks up by ISO-639-3 long code (caller-facing language identifier
/// in the JSONL input). Maps a small known set explicitly; for the
/// rest we just take the first two characters as a best-effort match,
/// which is enough for ISO-639-3 codes whose 1-letter equivalent is
/// the prefix.
pub fn for_iso6393(code: &str) -> &'static str {
    if code.is_empty() {
        return "";
    }
    if let Some(short) = iso6393_to_short(code) {
        return for_short_code(short);
    }
    // Best effort: try the first two characters lowercased.
    let lower: String = code.chars().take(2).flat_map(char::to_lowercase).collect();
    for_short_code(&lower)
}

fn iso6393_to_short(code: &str) -> Option<&'static str> {
    // Subset that matters in practice. Add as we encounter divergent
    // ZIMs; the fallback in `for_iso6393` handles most of the rest.
    Some(match code {
        "eng" => "en",
        "fra" | "fre" => "fr",
        "deu" | "ger" => "de",
        "spa" => "es",
        "ita" => "it",
        "por" => "pt",
        "rus" => "ru",
        "nld" | "dut" => "nl",
        "swe" => "sv",
        "nor" | "nob" | "nno" => "no",
        "dan" => "da",
        "fin" => "fi",
        "tur" => "tr",
        "pol" => "pl",
        "ces" | "cze" => "cs",
        "ron" | "rum" => "ro",
        "hun" => "hu",
        "ell" | "gre" => "el",
        "heb" => "he",
        "ara" => "ar",
        "fas" | "per" => "fa",
        "hin" => "hi",
        "ben" => "bn",
        "tha" => "th",
        "vie" => "vi",
        "ind" => "id",
        "msa" | "may" => "ms",
        "zho" | "chi" => "zh",
        "jpn" => "ja",
        "kor" => "ko",
        "ukr" => "uk",
        "cat" => "ca",
        "eus" | "baq" => "eu",
        "glg" => "gl",
        "lat" => "la",
        "lav" => "lv",
        "lit" => "lt",
        "slk" | "slo" => "sk",
        "slv" => "sl",
        "yor" => "yo",
        "zul" => "zu",
        "sot" => "st",
        "hrv" => "hr",
        "epo" => "eo",
        "afr" => "af",
        "tgl" => "tl",
        "swa" => "sw",
        "som" => "so",
        "hau" => "ha",
        "guj" => "gu",
        "mar" => "mr",
        "urd" => "ur",
        "kur" => "ku",
        "hye" | "arm" => "hy",
        "est" => "et",
        "bre" => "br",
        "bul" => "bg",
        _ => return None,
    })
}
