//! Stopword lists vendored from libzim's `static/stopwords/<lang>`.
//!
//! libzim's indexer looks up stopwords by exact filename match against
//! whatever language string the caller passed in. The shipped files
//! are 2-letter codes (`en`, `fr`, …), but kiwix tooling routinely
//! passes ISO-639-3 codes (`eng`, `fra`, …), and libzim swallows the
//! "file not found" exception silently and indexes with no stopper.
//! That's why production kiwix ZIMs typically contain stopword tokens
//! like "a", "and", "the" in their fulltext indexes.
//!
//! `for_language` mirrors that behaviour: exact filename lookup, empty
//! string on miss. Callers that want stopword filtering should pass the
//! 2-letter code (`en`) or use `--stopwords-file` to point at an
//! explicit list. `resolve_iso6393` is available for callers that want
//! the convenience-translation path.

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
            "af", "ar", "bg", "bn", "br", "ca", "cs", "da", "de", "el", "en", "eo", "es", "et",
            "eu", "fa", "fi", "fr", "ga", "gl", "gu", "ha", "he", "hi", "hr", "hu", "hy", "id",
            "it", "ja", "ko", "ku", "la", "lt", "lv", "mr", "ms", "nl", "no", "pl", "pt", "ro",
            "ru", "sk", "sl", "so", "st", "sv", "sw", "th", "tl", "tr", "uk", "ur", "vi", "yo",
            "zh", "zu",
        )
    })
}

/// Exact filename lookup, mirroring libzim's `getResource("stopwords/" + lang)`.
/// Returns an empty string for any code that doesn't have a vendored file —
/// e.g. `for_language("eng")` is empty, `for_language("en")` is the
/// English stopwords. This is the libzim-compat default.
pub fn for_language(code: &str) -> &'static str {
    table().get(code).copied().unwrap_or("")
}

/// Convenience translation: ISO-639-3 → ISO-639-1 → vendored list.
/// Callers can opt into this when they want the "do what I mean"
/// behaviour rather than libzim-bug-compat. Returns "" if neither the
/// input nor its translation has a vendored list.
pub fn resolve_iso6393(code: &str) -> &'static str {
    if code.is_empty() {
        return "";
    }
    if let Some(direct) = table().get(code) {
        return direct;
    }
    if let Some(short) = iso6393_to_short(code) {
        return for_language(short);
    }
    let lower: String = code.chars().take(2).flat_map(char::to_lowercase).collect();
    for_language(&lower)
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
