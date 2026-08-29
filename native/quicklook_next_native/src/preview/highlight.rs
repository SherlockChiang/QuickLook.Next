//! Native syntax-highlight tokenization for text previews.
//!
//! The tokenizer classifies bounded preview text into token spans and reports offsets in UTF-16
//! code units so the managed presenter can slice its original string directly. Span text is never
//! emitted; the caller slices its own copy, so span text plus the inter-span gaps always
//! reconstructs the exact input.

use std::collections::HashSet;
use std::sync::LazyLock;

/// Hard cap on emitted spans; remaining text stays Default after the cap is reached.
pub(crate) const MAX_HIGHLIGHT_SPANS: usize = 16384;

/// Token kinds, matching the managed `TokenKind` discriminant order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HighlightKind {
    Default = 0,
    Keyword = 1,
    Str = 2,
    Comment = 3,
    Number = 4,
    Type = 5,
    Property = 6,
    Punctuation = 7,
}

/// One classified token: UTF-16 start offset, UTF-16 length, kind. Default text is represented
/// by the absence of a span, not by an emitted span.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct HighlightSpan {
    pub start: u32,
    pub len: u32,
    pub kind: HighlightKind,
}

struct Lang {
    line_comments: &'static [&'static str],
    block: Option<(&'static str, &'static str)>,
    quotes: &'static [char],
}

const fn lang(
    line_comments: &'static [&'static str],
    block: Option<(&'static str, &'static str)>,
    quotes: &'static [char],
) -> Lang {
    Lang {
        line_comments,
        block,
        quotes,
    }
}

const C_LIKE: Lang = lang(&["//"], Some(("/*", "*/")), &['"', '\'']);
const SQL_LANG: Lang = lang(&["--"], Some(("/*", "*/")), &['"', '\'']);
const BATCH_LANG: Lang = lang(&["::", "REM ", "rem "], None, &['"']);
const PROPERTY_LANG: Lang = lang(&[";", "#"], None, &['"', '\'']);
const HASH_LANG: Lang = lang(&["#"], None, &['"', '\'']);
const LUA_LANG: Lang = lang(&["--"], Some(("--[[", "]]")), &['"', '\'']);
const F_SHARP_LANG: Lang = lang(&["//"], Some(("(*", "*)")), &['"', '\'']);
const DEFAULT_LANG: Lang = lang(&["//", "#"], Some(("/*", "*/")), &['"', '\'']);

fn spec_for(language: &str) -> Lang {
    match language {
        "csharp" | "rust" | "javascript" | "typescript" | "java" | "go" | "c" | "cpp" | "php"
        | "swift" | "kotlin" | "scala" | "dart" => C_LIKE,
        "sql" => SQL_LANG,
        "batch" => BATCH_LANG,
        "ini" | "toml" | "properties" | "env" => PROPERTY_LANG,
        "python" | "shell" | "powershell" | "yaml" | "ruby" | "perl" | "makefile"
        | "dockerfile" => HASH_LANG,
        "lua" => LUA_LANG,
        "fsharp" => F_SHARP_LANG,
        _ => DEFAULT_LANG,
    }
}

fn keywords() -> HashSet<&'static str> {
    HashSet::from([
        "if",
        "else",
        "elif",
        "for",
        "foreach",
        "while",
        "do",
        "switch",
        "case",
        "default",
        "break",
        "continue",
        "return",
        "function",
        "func",
        "fn",
        "def",
        "class",
        "struct",
        "enum",
        "interface",
        "trait",
        "impl",
        "record",
        "protocol",
        "namespace",
        "using",
        "import",
        "from",
        "export",
        "module",
        "package",
        "use",
        "mod",
        "pub",
        "open",
        "public",
        "private",
        "protected",
        "internal",
        "static",
        "readonly",
        "const",
        "let",
        "var",
        "val",
        "mut",
        "final",
        "void",
        "int",
        "uint",
        "long",
        "short",
        "byte",
        "float",
        "double",
        "decimal",
        "bool",
        "boolean",
        "char",
        "string",
        "str",
        "true",
        "false",
        "null",
        "nil",
        "none",
        "None",
        "True",
        "False",
        "undefined",
        "NaN",
        "new",
        "this",
        "self",
        "super",
        "base",
        "ref",
        "out",
        "in",
        "async",
        "await",
        "yield",
        "try",
        "catch",
        "except",
        "finally",
        "throw",
        "throws",
        "raise",
        "defer",
        "as",
        "is",
        "typeof",
        "instanceof",
        "sizeof",
        "where",
        "with",
        "match",
        "when",
        "lambda",
        "guard",
        "goto",
        "then",
        "fi",
        "esac",
        "done",
        "echo",
        "exit",
        "param",
        "begin",
        "end",
        "select",
        "unset",
        "set",
        "extends",
        "implements",
        "virtual",
        "override",
        "abstract",
        "sealed",
        "partial",
        "operator",
        "delegate",
        "event",
        "loop",
        "crate",
        "dyn",
        "move",
        "unsafe",
        "extern",
        "type",
        "alias",
        "macro",
        "macro_rules",
        "require",
        "include",
        "SELECT",
        "FROM",
        "WHERE",
        "JOIN",
        "LEFT",
        "RIGHT",
        "INNER",
        "OUTER",
        "GROUP",
        "ORDER",
        "BY",
        "INSERT",
        "UPDATE",
        "DELETE",
        "CREATE",
        "ALTER",
        "DROP",
        "TABLE",
        "VIEW",
        "INDEX",
        "VALUES",
        "INTO",
        "AND",
        "OR",
        "NOT",
        "NULL",
    ])
}

fn type_words() -> HashSet<&'static str> {
    HashSet::from([
        "String",
        "Object",
        "Array",
        "Map",
        "Set",
        "List",
        "Dictionary",
        "Task",
        "ValueTask",
        "DateTime",
        "Guid",
        "Exception",
        "Int32",
        "Int64",
        "Boolean",
        "Double",
        "Float",
        "Number",
        "Promise",
        "Console",
        "Math",
        "Regex",
        "Path",
        "File",
    ])
}

static KEYWORDS: LazyLock<HashSet<&'static str>> = LazyLock::new(keywords);
static TYPE_WORDS: LazyLock<HashSet<&'static str>> = LazyLock::new(type_words);

static PROPERTY_LANGUAGES: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    HashSet::from([
        "json",
        "yaml",
        "toml",
        "ini",
        "env",
        "properties",
        "xml",
        "html",
        "xaml",
        "css",
        "scss",
        "sass",
        "less",
    ])
});

/// Classify `text` with `language` into ordered, non-overlapping non-Default spans.
pub(crate) fn highlight_spans(text: &str, language: &str) -> Vec<HighlightSpan> {
    let mut spans = Vec::new();
    match language {
        "xml" | "html" | "xaml" => highlight_markup(text, &mut spans),
        "json" => highlight_json(text, &mut spans),
        "css" | "scss" | "sass" | "less" => highlight_css(text, &mut spans),
        "csv" | "tsv" => highlight_delimited(text, language == "tsv", &mut spans),
        _ => highlight_generic(text, language, &mut spans),
    }
    spans
}

/// Scan cursor that tracks both the byte position and the UTF-16 code-unit position, so span
/// offsets match .NET string indices without rescanning the prefix.
struct Cursor<'a> {
    text: &'a str,
    byte: usize,
    utf16: usize,
}

impl<'a> Cursor<'a> {
    fn new(text: &'a str) -> Self {
        Self {
            text,
            byte: 0,
            utf16: 0,
        }
    }

    fn done(&self) -> bool {
        self.byte >= self.text.len()
    }

    /// Current char; only valid while `!done()`.
    fn c(&self) -> char {
        self.text[self.byte..]
            .chars()
            .next()
            .expect("cursor rests on a char boundary")
    }

    fn step(&mut self) {
        let len = self.c().len_utf8();
        self.utf16 += self.c().len_utf16();
        self.byte += len;
    }

    /// Jump forward to a char-boundary offset, accumulating the UTF-16 distance.
    fn jump(&mut self, to: usize) {
        debug_assert!(to >= self.byte && to <= self.text.len());
        self.utf16 += self.text[self.byte..to]
            .chars()
            .map(char::len_utf16)
            .sum::<usize>();
        self.byte = to;
    }

    fn push(&self, spans: &mut Vec<HighlightSpan>, end: usize, kind: HighlightKind) {
        self.push_at(spans, self.byte, end, kind);
    }

    /// Push a span starting at or after the cursor (used for sub-cursor positions).
    fn push_at(
        &self,
        spans: &mut Vec<HighlightSpan>,
        start: usize,
        end: usize,
        kind: HighlightKind,
    ) {
        if spans.len() >= MAX_HIGHLIGHT_SPANS || end <= start {
            return;
        }
        let start_utf16 = self.utf16
            + self.text[self.byte..start]
                .chars()
                .map(char::len_utf16)
                .sum::<usize>();
        let len = self.text[start..end]
            .chars()
            .map(char::len_utf16)
            .sum::<usize>();
        spans.push(HighlightSpan {
            start: start_utf16 as u32,
            len: len as u32,
            kind,
        });
    }
}

fn find_from(text: &str, needle: &str, from: usize) -> Option<usize> {
    text[from..].find(needle).map(|at| at + from)
}

fn is_punctuation(c: char) -> bool {
    "{}[]();,.=:+-*/%!<>|&?".contains(c)
}

fn next_non_white_byte(text: &str, from: usize) -> Option<char> {
    text[from..].chars().find(|c| !c.is_whitespace())
}

/// Walk a quoted run starting at the opening quote; returns the byte index after the run.
fn quoted_end(text: &str, start: usize, quote: char, allow_newline: bool) -> usize {
    let mut iter = text[start..].char_indices();
    iter.next(); // opening quote
    while let Some((offset, c)) = iter.next() {
        let j = start + offset;
        if c == '\\' {
            iter.next(); // skip the escaped char regardless of its byte length
            continue;
        }
        if c == quote {
            return j + c.len_utf8();
        }
        if !allow_newline && c == '\n' {
            return j;
        }
    }
    text.len()
}

fn word_end(text: &str, start: usize) -> usize {
    text[start..]
        .char_indices()
        .find(|(_, c)| !(c.is_alphanumeric() || matches!(c, '_' | '-' | '$')))
        .map_or(text.len(), |(offset, _)| start + offset)
}

fn is_word_start(c: char) -> bool {
    c.is_alphabetic() || c == '_' || c == '$'
}

fn classify_word(
    text: &str,
    start: usize,
    end: usize,
    word: &str,
    property_language: bool,
    suppress_type_words: bool,
) -> HighlightKind {
    if KEYWORDS.contains(word) || KEYWORDS.contains(word.to_ascii_uppercase().as_str()) {
        return HighlightKind::Keyword;
    }
    let starts_upper = word.chars().next().is_some_and(char::is_uppercase);
    if TYPE_WORDS.contains(word) || (starts_upper && !suppress_type_words) {
        return HighlightKind::Type;
    }
    if property_language && matches!(next_non_white_byte(text, end), Some(':') | Some('=')) {
        return HighlightKind::Property;
    }
    if start > 0 && text[..start].ends_with('.') {
        return HighlightKind::Property;
    }
    HighlightKind::Default
}

fn highlight_generic(text: &str, language: &str, spans: &mut Vec<HighlightSpan>) {
    let lang = spec_for(language);
    let allow_newline_quotes = matches!(language, "yaml" | "toml");
    let property_language = PROPERTY_LANGUAGES.contains(language);
    let suppress_type_words = matches!(language, "yaml" | "ini" | "env" | "properties");

    let mut cur = Cursor::new(text);
    while !cur.done() {
        if let Some((block_start, block_end)) = lang.block {
            if text[cur.byte..].starts_with(block_start) {
                let search_from = (cur.byte + block_start.len()).min(text.len());
                let stop = find_from(text, block_end, search_from)
                    .map_or(text.len(), |end| end + block_end.len());
                cur.push(spans, stop, HighlightKind::Comment);
                cur.jump(stop);
                continue;
            }
        }

        let matched_line_comment = lang
            .line_comments
            .iter()
            .find(|comment| text[cur.byte..].starts_with(**comment));
        if let Some(comment) = matched_line_comment {
            let stop = find_from(text, "\n", cur.byte + comment.len()).unwrap_or(text.len());
            cur.push(spans, stop, HighlightKind::Comment);
            cur.jump(stop);
            continue;
        }

        let c = cur.c();
        if lang.quotes.contains(&c) {
            let stop = quoted_end(text, cur.byte, c, allow_newline_quotes);
            cur.push(spans, stop, HighlightKind::Str);
            cur.jump(stop);
            continue;
        }

        let next_is_digit = text[cur.byte + c.len_utf8()..]
            .chars()
            .next()
            .is_some_and(char::is_numeric);
        if c.is_numeric() || (matches!(c, '-' | '+') && next_is_digit) {
            let mut j = cur.byte + c.len_utf8();
            while let Some(ch) = text[j..].chars().next() {
                if ch.is_alphanumeric() || matches!(ch, '.' | '_' | 'x' | 'X') {
                    j += ch.len_utf8();
                } else {
                    break;
                }
            }
            cur.push(spans, j, HighlightKind::Number);
            cur.jump(j);
            continue;
        }

        if is_word_start(c) {
            let j = word_end(text, cur.byte);
            let word = &text[cur.byte..j];
            let kind = classify_word(
                text,
                cur.byte,
                j,
                word,
                property_language,
                suppress_type_words,
            );
            if kind != HighlightKind::Default {
                cur.push(spans, j, kind);
            }
            cur.jump(j);
            continue;
        }

        if is_punctuation(c) {
            cur.push(spans, cur.byte + c.len_utf8(), HighlightKind::Punctuation);
        }
        cur.step();
    }
}

fn highlight_json(text: &str, spans: &mut Vec<HighlightSpan>) {
    let mut cur = Cursor::new(text);
    while !cur.done() {
        let c = cur.c();
        if c.is_whitespace() {
            cur.step();
        } else if c == '"' {
            let stop = quoted_end(text, cur.byte, '"', false);
            let kind = if matches!(next_non_white_byte(text, stop), Some(':')) {
                HighlightKind::Property
            } else {
                HighlightKind::Str
            };
            cur.push(spans, stop, kind);
            cur.jump(stop);
        } else if c.is_numeric() || c == '-' {
            let mut j = cur.byte + c.len_utf8();
            while let Some(ch) = text[j..].chars().next() {
                if ch.is_numeric() || matches!(ch, '.' | 'e' | 'E' | '+' | '-') {
                    j += ch.len_utf8();
                } else {
                    break;
                }
            }
            cur.push(spans, j, HighlightKind::Number);
            cur.jump(j);
        } else if is_word_start(c) {
            let j = word_end(text, cur.byte);
            cur.push(spans, j, HighlightKind::Keyword);
            cur.jump(j);
        } else {
            if "{}[]:,".contains(c) {
                cur.push(spans, cur.byte + c.len_utf8(), HighlightKind::Punctuation);
            }
            cur.step();
        }
    }
}

fn is_markup_name_part(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, ':' | '-' | '_' | '.')
}

fn markup_name_end(text: &str, start: usize) -> usize {
    text[start..]
        .char_indices()
        .find(|(_, c)| !is_markup_name_part(*c))
        .map_or(text.len(), |(offset, _)| start + offset)
}

fn highlight_markup(text: &str, spans: &mut Vec<HighlightSpan>) {
    let mut cur = Cursor::new(text);
    while !cur.done() {
        if text[cur.byte..].starts_with("<!--") {
            let stop = find_from(text, "-->", cur.byte + 4).map_or(text.len(), |end| end + 3);
            cur.push(spans, stop, HighlightKind::Comment);
            cur.jump(stop);
            continue;
        }

        let c = cur.c();
        if c != '<' {
            cur.step();
            continue;
        }

        cur.push(spans, cur.byte + 1, HighlightKind::Punctuation);
        cur.step();
        if !cur.done() && cur.c() == '/' {
            cur.push(spans, cur.byte + 1, HighlightKind::Punctuation);
            cur.step();
        }
        let tag = cur.byte;
        let name_end = markup_name_end(text, tag);
        if name_end > tag {
            cur.push(spans, name_end, HighlightKind::Keyword);
        }
        cur.jump(name_end);

        while !cur.done() && cur.c() != '>' {
            let ch = cur.c();
            if ch.is_whitespace() {
                cur.step();
            } else if ch == '"' || ch == '\'' {
                let stop = quoted_end(text, cur.byte, ch, true);
                cur.push(spans, stop, HighlightKind::Str);
                cur.jump(stop);
            } else if is_markup_name_part(ch) {
                let end = markup_name_end(text, cur.byte);
                cur.push(spans, end, HighlightKind::Property);
                cur.jump(end);
            } else {
                cur.push(spans, cur.byte + ch.len_utf8(), HighlightKind::Punctuation);
                cur.step();
            }
        }
        if !cur.done() {
            cur.push(spans, cur.byte + 1, HighlightKind::Punctuation);
            cur.step();
        }
    }
}

fn is_css_name_start(c: char) -> bool {
    c.is_alphabetic() || matches!(c, '_' | '-' | '.')
}

fn is_css_name_part(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '-')
}

fn css_name_end(text: &str, start: usize, part: fn(char) -> bool) -> usize {
    text[start..]
        .char_indices()
        .find(|(_, c)| !part(*c))
        .map_or(text.len(), |(offset, _)| start + offset)
}

fn highlight_css(text: &str, spans: &mut Vec<HighlightSpan>) {
    let mut cur = Cursor::new(text);
    while !cur.done() {
        let c = cur.c();
        if text[cur.byte..].starts_with("/*") {
            let stop = find_from(text, "*/", cur.byte + 2).map_or(text.len(), |end| end + 2);
            cur.push(spans, stop, HighlightKind::Comment);
            cur.jump(stop);
        } else if c == '"' || c == '\'' {
            let stop = quoted_end(text, cur.byte, c, false);
            cur.push(spans, stop, HighlightKind::Str);
            cur.jump(stop);
        } else if c == '@' {
            let j = css_name_end(text, cur.byte + 1, is_css_name_part);
            cur.push(spans, j, HighlightKind::Keyword);
            cur.jump(j);
        } else if c == '#' {
            let mut j = cur.byte + 1;
            while let Some(ch) = text[j..].chars().next() {
                if ch.is_ascii_hexdigit() {
                    j += ch.len_utf8();
                } else {
                    break;
                }
            }
            cur.push(spans, j, HighlightKind::Number);
            cur.jump(j);
        } else if is_css_name_start(c) {
            let j = css_name_end(text, cur.byte, is_css_name_part);
            if matches!(next_non_white_byte(text, j), Some(':')) {
                cur.push(spans, j, HighlightKind::Property);
            }
            cur.jump(j);
        } else if is_punctuation(c) {
            cur.push(spans, cur.byte + c.len_utf8(), HighlightKind::Punctuation);
            cur.step();
        } else {
            cur.step();
        }
    }
}

fn highlight_delimited(text: &str, tab_separated: bool, spans: &mut Vec<HighlightSpan>) {
    let delimiter = if tab_separated { '\t' } else { ',' };
    let mut cur = Cursor::new(text);
    while !cur.done() {
        let c = cur.c();
        if c == '"' {
            let mut j = cur.byte + 1;
            while j < text.len() {
                let ch = text[j..].chars().next().expect("char boundary at j");
                if ch != '"' {
                    j += ch.len_utf8();
                    continue;
                }
                if text[j + 1..].starts_with('"') {
                    j += 2;
                } else {
                    j += 1;
                    break;
                }
            }
            cur.push(spans, j, HighlightKind::Str);
            cur.jump(j);
        } else if c == delimiter {
            cur.push(spans, cur.byte + 1, HighlightKind::Punctuation);
            cur.step();
        } else {
            let mut j = cur.byte + c.len_utf8();
            while let Some(ch) = text[j..].chars().next() {
                if ch == '"' || ch == delimiter {
                    break;
                }
                j += ch.len_utf8();
            }
            cur.jump(j);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{highlight_spans, HighlightKind, HighlightSpan, MAX_HIGHLIGHT_SPANS};

    fn utf16_len(text: &str) -> u32 {
        text.chars().map(char::len_utf16).sum::<usize>() as u32
    }

    /// All test inputs are BMP-only, so char skip/take equals UTF-16 slicing here.
    fn slice_at_utf16(text: &str, span: HighlightSpan) -> String {
        text.chars()
            .skip(span.start as usize)
            .take(span.len as usize)
            .collect()
    }

    fn rendered(text: &str, language: &str) -> Vec<(String, HighlightKind)> {
        highlight_spans(text, language)
            .into_iter()
            .map(|s| (slice_at_utf16(text, s), s.kind))
            .collect()
    }

    #[test]
    fn generic_classifies_keywords_numbers_punctuation_and_comments() {
        let text = "let value = 42; // done";
        assert_eq!(
            rendered(text, "rust"),
            vec![
                ("let".to_string(), HighlightKind::Keyword),
                ("=".to_string(), HighlightKind::Punctuation),
                ("42".to_string(), HighlightKind::Number),
                (";".to_string(), HighlightKind::Punctuation),
                ("// done".to_string(), HighlightKind::Comment),
            ]
        );
    }

    #[test]
    fn string_runs_cover_the_closing_quote() {
        let text = "let s = \"hi\";";
        let spans = highlight_spans(text, "rust");
        assert_eq!(slice_at_utf16(text, spans[2]), "\"hi\"");
        assert_eq!(spans[2].kind, HighlightKind::Str);
    }

    #[test]
    fn uppercase_words_become_types_outside_property_languages() {
        let spans = highlight_spans("Foo bar", "csharp");
        assert_eq!(slice_at_utf16("Foo bar", spans[0]), "Foo");
        assert_eq!(spans[0].kind, HighlightKind::Type);
    }

    #[test]
    fn property_languages_mark_keys_before_the_separator() {
        let spans = highlight_spans("key=value", "ini");
        assert_eq!(slice_at_utf16("key=value", spans[0]), "key");
        assert_eq!(spans[0].kind, HighlightKind::Property);
    }

    #[test]
    fn block_comments_take_their_terminator() {
        for (text, language) in [("/* c */ x", "rust"), ("(* c *) x", "fsharp")] {
            let spans = highlight_spans(text, language);
            assert_eq!(spans[0].kind, HighlightKind::Comment);
            assert!(
                slice_at_utf16(text, spans[0]).starts_with("/*")
                    || slice_at_utf16(text, spans[0]).starts_with("(*")
            );
        }
    }

    #[test]
    fn unterminated_block_comments_reach_the_end() {
        let text = "/* never closed";
        let spans = highlight_spans(text, "rust");
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].start, 0);
        assert_eq!(spans[0].len, utf16_len(text));
    }

    #[test]
    fn batch_line_comments_match_the_documented_markers() {
        let text = ":: note\nREM note2";
        let spans = highlight_spans(text, "batch");
        assert_eq!(slice_at_utf16(text, spans[0]), ":: note");
        assert_eq!(spans[0].kind, HighlightKind::Comment);
        assert_eq!(slice_at_utf16(text, spans[1]), "REM note2");
        assert_eq!(spans[1].kind, HighlightKind::Comment);
    }

    #[test]
    fn single_line_comments_stop_before_the_newline() {
        let text = "# c\nrest";
        let spans = highlight_spans(text, "python");
        assert_eq!(slice_at_utf16(text, spans[0]), "# c");
        assert_eq!(spans[0].kind, HighlightKind::Comment);
    }

    #[test]
    fn yaml_quotes_allow_newlines_while_c_like_quotes_do_not() {
        let text = "\"a\nb\"";
        let yaml_spans = highlight_spans(text, "yaml");
        assert_eq!(yaml_spans.len(), 1);
        assert_eq!(yaml_spans[0].len, utf16_len(text));

        let c_spans = highlight_spans(text, "rust");
        assert_eq!(slice_at_utf16(text, c_spans[0]), "\"a");
    }

    #[test]
    fn json_marks_properties_numbers_and_punctuation() {
        let text = "{\"a\": 1}";
        assert_eq!(
            rendered(text, "json"),
            vec![
                ("{".to_string(), HighlightKind::Punctuation),
                ("\"a\"".to_string(), HighlightKind::Property),
                (":".to_string(), HighlightKind::Punctuation),
                ("1".to_string(), HighlightKind::Number),
                ("}".to_string(), HighlightKind::Punctuation),
            ]
        );
    }

    #[test]
    fn markup_marks_tags_attributes_and_strings() {
        let text = "<div class=\"a\">x</div>";
        assert_eq!(
            rendered(text, "html"),
            vec![
                ("<".to_string(), HighlightKind::Punctuation),
                ("div".to_string(), HighlightKind::Keyword),
                ("class".to_string(), HighlightKind::Property),
                ("=".to_string(), HighlightKind::Punctuation),
                ("\"a\"".to_string(), HighlightKind::Str),
                (">".to_string(), HighlightKind::Punctuation),
                ("<".to_string(), HighlightKind::Punctuation),
                ("/".to_string(), HighlightKind::Punctuation),
                ("div".to_string(), HighlightKind::Keyword),
                (">".to_string(), HighlightKind::Punctuation),
            ]
        );
    }

    #[test]
    fn css_marks_at_rules_hex_colors_and_properties() {
        let text = "@media { color: #fff; }";
        assert_eq!(
            rendered(text, "css"),
            vec![
                ("@media".to_string(), HighlightKind::Keyword),
                ("{".to_string(), HighlightKind::Punctuation),
                ("color".to_string(), HighlightKind::Property),
                (":".to_string(), HighlightKind::Punctuation),
                ("#fff".to_string(), HighlightKind::Number),
                (";".to_string(), HighlightKind::Punctuation),
                ("}".to_string(), HighlightKind::Punctuation),
            ]
        );
    }

    #[test]
    fn delimited_input_marks_quotes_and_delimiters() {
        let text = "a,\"b,c\",\"d\"\"e\"";
        assert_eq!(
            rendered(text, "csv"),
            vec![
                (",".to_string(), HighlightKind::Punctuation),
                ("\"b,c\"".to_string(), HighlightKind::Str),
                (",".to_string(), HighlightKind::Punctuation),
                ("\"d\"\"e\"".to_string(), HighlightKind::Str),
            ]
        );
    }

    #[test]
    fn span_offsets_use_utf16_units_not_utf8_bytes() {
        let text = "let s = 1; // 中文注释";
        let spans = highlight_spans(text, "rust");
        let comment = spans
            .iter()
            .find(|s| s.kind == HighlightKind::Comment)
            .copied()
            .expect("comment span");
        assert_eq!(slice_at_utf16(text, comment), "// 中文注释");
        assert_eq!(comment.start, 11);
    }

    #[test]
    fn spans_stay_ordered_within_bounds_and_cap_limited() {
        let text = "1;".repeat(20_000);
        let spans = highlight_spans(&text, "rust");
        assert_eq!(spans.len(), MAX_HIGHLIGHT_SPANS);
        let total = utf16_len(&text);
        let mut last_end = 0u32;
        for span in &spans {
            assert!(span.start >= last_end);
            last_end = span.start + span.len;
        }
        assert!(last_end <= total);
    }

    #[test]
    fn every_kind_keeps_the_managed_discriminant_order() {
        assert_eq!(HighlightKind::Default as u32, 0);
        assert_eq!(HighlightKind::Keyword as u32, 1);
        assert_eq!(HighlightKind::Str as u32, 2);
        assert_eq!(HighlightKind::Comment as u32, 3);
        assert_eq!(HighlightKind::Number as u32, 4);
        assert_eq!(HighlightKind::Type as u32, 5);
        assert_eq!(HighlightKind::Property as u32, 6);
        assert_eq!(HighlightKind::Punctuation as u32, 7);
    }
}
