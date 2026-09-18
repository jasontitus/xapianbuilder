use xapianbuilder::parse::ParsedDoc;

#[test]
fn optional_geo_rejects_invalid_values_without_losing_content() {
    for value in [
        "",
        ";",
        " ; ",
        "north;east",
        "1x;2",
        "1;2x",
        "1;2;3",
        "nan;0",
        "0;inf",
        "1e999;0",
        "90.0000001;0",
        "0;-180.0000001",
        "91;0",
        "0;181",
        "1\0;2",
    ] {
        let html =
            format!("<meta name='geo.position' content='{value}'><body>Still searchable</body>");
        let parsed = ParsedDoc::parse(html.as_bytes()).expect("optional geo must not fail parsing");
        assert_eq!(parsed.geo(), None, "accepted invalid geo: {value:?}");
        assert_eq!(parsed.content(), "still searchable");
        assert!(parsed.indexing_allowed());
    }
}

#[test]
fn geo_accepts_boundaries_and_complete_finite_coordinates() {
    for (value, expected) in [
        ("90;180", (90.0, 180.0)),
        ("-90;-180", (-90.0, -180.0)),
        (" 12.5 ; -4.25 ", (12.5, -4.25)),
        ("1.25e1;-4.25e0", (12.5, -4.25)),
    ] {
        let html = format!("<meta name='geo.position' content='{value}'><body>Place</body>");
        let parsed = ParsedDoc::parse(html.as_bytes()).unwrap();
        assert_eq!(parsed.geo(), Some(expected), "geo: {value:?}");
    }
}

#[test]
fn invalid_geo_tag_does_not_corrupt_a_previous_valid_tag() {
    let parsed = ParsedDoc::parse(
        b"<meta name='geo.position' content='12.5;-4.25'>\
          <meta name='geo.position' content='80;invalid'><body>Place</body>",
    )
    .unwrap();
    assert_eq!(parsed.geo(), Some((12.5, -4.25)));
}

#[test]
fn keywords_preserve_text_after_embedded_nul() {
    let parsed = ParsedDoc::parse(
        "<meta name='keywords' content='CAFÉ\0RÉSUMÉ &amp; TEA'>\
         <meta name='keywords' content='AFTER'><body>Body</body>"
            .as_bytes(),
    )
    .unwrap();
    assert_eq!(parsed.keywords(), "cafe\0resume & tea after");
}

#[test]
fn intentional_parser_stops_preserve_indexing_policy() {
    let parsed = ParsedDoc::parse(b"<body>Visible</body>Not in the body").unwrap();
    assert_eq!(parsed.content(), "visible");
    assert!(parsed.indexing_allowed());

    let excluded =
        ParsedDoc::parse(b"<meta name='robots' content='noindex'><body>Excluded</body>").unwrap();
    assert!(!excluded.indexing_allowed());
}

#[test]
fn truncated_html_preserves_preceding_text() {
    let trailing_angle = ParsedDoc::parse(b"<body>Before &amp; after <").unwrap();
    assert_eq!(trailing_angle.content(), "before & after <");
    for suffix in ["<p key=", "<p key=   ", "<!--", "</", "<script>unfinished"] {
        let html = format!("<body>Before{suffix}");
        let parsed = ParsedDoc::parse(html.as_bytes()).unwrap();
        assert_eq!(parsed.content(), "before", "truncated suffix: {suffix:?}");
    }
}

#[test]
fn numeric_entities_reject_overflow_and_invalid_unicode_scalars() {
    let parsed = ParsedDoc::parse(
        b"<body>&#65; &#x1F600; &#4294967361; &#x100000041; \
          &#x110000; &#xD800; &#999999999999999999999999999999999999999; \
          &#x; &unknown;</body>",
    )
    .unwrap();
    assert_eq!(
        parsed.content(),
        "a \u{1f600} \u{fffd} \u{fffd} \u{fffd} \u{fffd} \u{fffd} &#x; &unknown;"
    );

    let keywords = ParsedDoc::parse(
        b"<meta name='keywords' content='before &#xDFFF; after'><body>Body</body>",
    )
    .unwrap();
    assert_eq!(keywords.keywords(), "before \u{fffd} after");
}

#[test]
fn native_coordinate_errors_retain_diagnostics_and_builder_remains_usable() {
    use xapianbuilder::{AccentRule, Builder, BuilderOptions, FulltextDocument, Mode};

    let dir = tempfile::tempdir().unwrap();
    let builder = Builder::new(
        &dir.path().join("index.xapian"),
        BuilderOptions {
            language: "eng",
            stopwords: "",
            stemmer: "none",
            accent_rule: AccentRule::Libzim,
            keep_termlists: false,
            mode: Mode::Fulltext,
        },
    )
    .unwrap();
    let input = |geo| FulltextDocument {
        path: "place",
        title: "Place",
        content: b"searchable place",
        keywords: b"",
        word_count: 2,
        geo: Some(geo),
        language: "",
    };
    for coordinates in [
        (91.0, 0.0),
        (0.0, 181.0),
        (f64::NAN, 0.0),
        (0.0, f64::INFINITY),
    ] {
        let error = builder
            .prepare_fulltext(input(coordinates))
            .err()
            .expect("invalid direct coordinates must fail");
        assert!(
            error.to_string().contains("InvalidArgumentError"),
            "native error type was lost: {error}"
        );
        assert!(builder.is_empty());
    }
    let doc = builder.prepare_fulltext(input((90.0, -180.0))).unwrap();
    builder.add_doc(&doc).unwrap();
    assert!(!builder.is_empty());
}

#[test]
fn rawtext_only_ends_at_its_matching_tag() {
    for html in [
        "<body>before<script>const x = '</body><!--<style>';</ScRiPt>after</body>",
        "<body>before<style>x::after{content:'</body><script>'}</STYLE>after</body>",
        "<body>before<script>ignored </scripture> ignored</script>after</body>",
        "<body>before<style>ignored </stylesheet> ignored</style>after</body>",
    ] {
        assert_eq!(
            ParsedDoc::parse(html.as_bytes()).unwrap().content(),
            "beforeafter"
        );
    }
}

#[test]
fn normalization_keeps_context_and_sequences_across_batches() {
    use xapianbuilder::AccentRule;
    for rule in [AccentRule::Libzim, AccentRule::Latin] {
        for (tail, expected) in [
            ("ΣA", "σa"),
            ("Σ ", "ς"),
            ("\u{1100}\u{1161}", "가"),
            ("\u{1100}\u{034f}\u{1161}", "가"),
            ("😀B", "😀b"),
        ] {
            let prefix = "a".repeat(4095);
            let html = format!("<body>{prefix}{tail}</body>");
            let parsed = ParsedDoc::parse_with(html.as_bytes(), rule).unwrap();
            assert_eq!(parsed.content(), format!("{prefix}{expected}"));
        }
    }
}

#[test]
fn dense_entities_preserve_single_pass_decoding_semantics() {
    let text = "&amp;&amp;lt;&unknown;&bogus&amp;&#65;&#x1f600;".repeat(20_000);
    let expected = "&&lt;&unknown;&bogus&a😀".repeat(20_000);
    let html = format!("<body>{text}</body>");
    assert_eq!(
        ParsedDoc::parse(html.as_bytes()).unwrap().content(),
        expected
    );
}
