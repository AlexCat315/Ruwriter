use ruwriter_core::stats::count;

#[test]
fn empty_document_has_no_words() {
    let stats = count("");
    assert_eq!(stats.words, 0);
    assert_eq!(stats.characters, 0);
    assert_eq!(stats.lines, 1);
    assert_eq!(stats.reading_minutes, 0);
}

#[test]
fn latin_word_runs_and_cjk_characters() {
    let stats = count("Hello world，中文写作 123");
    assert_eq!(stats.words, 2 + 4 + 1);
    assert_eq!(stats.lines, 1);
}

#[test]
fn punctuation_flushes_latin_words() {
    assert_eq!(count("one, two! three").words, 3);
    assert_eq!(count("don't stop").words, 2);
}

#[test]
fn lines_and_reading_time() {
    let stats = count("first\nsecond\nthird");
    assert_eq!(stats.lines, 3);
    assert_eq!(count(&"word ".repeat(601)).reading_minutes, 3);
}
