// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! The lexer (core/'s `com.legend.lexer`, ported: `Lexer`, `TokenStream`, `TokenType`), over a text's UTF-16 units
//! as Java's indexes a `String` (docs/rust-port/PHASE2.md §3): every offset is a `char` index, every span column a
//! count of them, an error column a count of code points. The scan rules are Java's line for line, its quirks
//! included -- the bounds of a range scan (`lexRange`), the one lookahead that reads past them (`}#` in a braced TDS
//! literal), the JDK's `Character` predicates where Java calls them (`lite_javacompat::character`, the JDK's table).
//! Held to Java's every token stream by `front_lexer_differential_test`.

use lite_javacompat::character::{is_digit, is_letter_or_digit, is_whitespace};
use std::cell::OnceCell;

macro_rules! token_types {
    ($($name:ident),* $(,)?) => {
        /// A token's classification, in Java's `TokenType` order (an ordinal is Java's ordinal).
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        #[repr(u16)]
        #[allow(non_camel_case_types, clippy::upper_case_acronyms)]
        pub enum TokenType { $($name),* }

        impl TokenType {
            /// Every type, by ordinal.
            pub const ALL: &'static [TokenType] = &[$(TokenType::$name),*];

            /// Java's name for the type.
            pub fn name(self) -> &'static str {
                match self { $(TokenType::$name => stringify!($name)),* }
            }
        }
    };
}

token_types! {
    STRING, BOOLEAN, TRUE, FALSE, INTEGER, FLOAT, DECIMAL, DATE, STRICTTIME, LATEST_DATE, QUOTED_STRING, DOC_STRING,
    AND, OR, NOT, COMMA, EQUAL, TEST_EQUAL, TEST_NOT_EQUAL, ARROW, BRACE_OPEN, BRACE_CLOSE, BRACKET_OPEN,
    BRACKET_CLOSE, PAREN_OPEN, PAREN_CLOSE, COLON, DOT, DOLLAR, DOT_DOT, SEMI_COLON, NEW_SYMBOL, PIPE, TILDE, SUBSET,
    QUESTION, AT, PLUS, STAR, MINUS, DIVIDE, LESS_THAN, LESS_OR_EQUAL, GREATER_THAN, GREATER_OR_EQUAL, NOT_EQUAL,
    PATH_SEPARATOR, ALL, LET, ALL_VERSIONS, ALL_VERSIONS_IN_RANGE, IMPORT, CLASS, ASSOCIATION, PROFILE, ENUM,
    FUNCTION, EXTENDS, STEREOTYPES, TAGS, NATIVE, AS, MAPPING, INCLUDE, MAPPING_TESTABLE_SUITES, MAPPING_TESTS_QUERY,
    SERVICE, SERVICE_PATTERN, SERVICE_OWNERS, SERVICE_DOCUMENTATION, SERVICE_AUTO_ACTIVATE_UPDATES, SERVICE_EXEC,
    SERVICE_SINGLE, SERVICE_MAPPING, SERVICE_RUNTIME, RUNTIME, SINGLE_CONNECTION_RUNTIME, MAPPINGS, CONNECTIONS,
    CONNECTION, DATABASE, TABLE, SCHEMA, VIEW, FILTER, MULTIGRAIN_FILTER, JOIN, FILTER_CMD, DISTINCT_CMD,
    GROUP_BY_CMD, MAIN_TABLE_CMD, PRIMARY_KEY_CMD, SRC_CMD, TARGET, PRIMARY_KEY, NOT_NULL, IS_NULL, IS_NOT_NULL,
    RELATIONAL_AND, RELATIONAL_OR, RELATIONAL, PURE_MAPPING, ASSOCIATION_MAPPING, ENUMERATION_MAPPING, OTHERWISE,
    INLINE, RELATIONAL_DATABASE_CONNECTION, STORE, TYPE, RELATIONAL_DATASOURCE_SPEC, RELATIONAL_AUTH_STRATEGY, H2,
    ISLAND_OPEN, ISLAND_START, ISLAND_HASH, ISLAND_CONTENT, INVALID, ISLAND_END, ISLAND_BRACE_OPEN,
    ISLAND_BRACE_CLOSE, TDS_LITERAL, PATH_LITERAL, VALID_STRING, EOF,
}

use TokenType as T;

/// The keywords: Java's `KEYWORDS` map, an identifier that is one of these lexes to its type.
fn keyword(text: &str) -> Option<TokenType> {
    Some(match text {
        "all" => T::ALL,
        "let" => T::LET,
        "allVersions" => T::ALL_VERSIONS,
        "allVersionsInRange" => T::ALL_VERSIONS_IN_RANGE,
        "import" => T::IMPORT,
        "Class" => T::CLASS,
        "Association" => T::ASSOCIATION,
        "Profile" => T::PROFILE,
        "Enum" => T::ENUM,
        "function" => T::FUNCTION,
        "extends" => T::EXTENDS,
        "stereotypes" => T::STEREOTYPES,
        "tags" => T::TAGS,
        "native" => T::NATIVE,
        "as" => T::AS,
        "true" => T::TRUE,
        "false" => T::FALSE,
        "Mapping" => T::MAPPING,
        "include" => T::INCLUDE,
        "testSuites" => T::MAPPING_TESTABLE_SUITES,
        "query" => T::MAPPING_TESTS_QUERY,
        "Service" => T::SERVICE,
        "pattern" => T::SERVICE_PATTERN,
        "owners" => T::SERVICE_OWNERS,
        "documentation" => T::SERVICE_DOCUMENTATION,
        "autoActivateUpdates" => T::SERVICE_AUTO_ACTIVATE_UPDATES,
        "execution" => T::SERVICE_EXEC,
        "Single" => T::SERVICE_SINGLE,
        "mapping" => T::SERVICE_MAPPING,
        "runtime" => T::SERVICE_RUNTIME,
        "Runtime" => T::RUNTIME,
        "SingleConnectionRuntime" => T::SINGLE_CONNECTION_RUNTIME,
        "mappings" => T::MAPPINGS,
        "connections" => T::CONNECTIONS,
        "connection" => T::CONNECTION,
        "Database" => T::DATABASE,
        "Table" => T::TABLE,
        "Schema" => T::SCHEMA,
        "View" => T::VIEW,
        "Filter" => T::FILTER,
        "MultiGrainFilter" => T::MULTIGRAIN_FILTER,
        "Join" => T::JOIN,
        "and" => T::RELATIONAL_AND,
        "or" => T::RELATIONAL_OR,
        "Relational" => T::RELATIONAL,
        "Pure" => T::PURE_MAPPING,
        "AssociationMapping" => T::ASSOCIATION_MAPPING,
        "EnumerationMapping" => T::ENUMERATION_MAPPING,
        "Otherwise" => T::OTHERWISE,
        "Inline" => T::INLINE,
        "RelationalDatabaseConnection" => T::RELATIONAL_DATABASE_CONNECTION,
        "store" => T::STORE,
        "type" => T::TYPE,
        "specification" => T::RELATIONAL_DATASOURCE_SPEC,
        "auth" => T::RELATIONAL_AUTH_STRATEGY,
        "H2" => T::H2,
        _ => return None,
    })
}

/// Java's `TILDE_COMMANDS`.
fn tilde_command(text: &str) -> Option<TokenType> {
    Some(match text {
        "~filter" => T::FILTER_CMD,
        "~distinct" => T::DISTINCT_CMD,
        "~groupBy" => T::GROUP_BY_CMD,
        "~mainTable" => T::MAIN_TABLE_CMD,
        "~primaryKey" => T::PRIMARY_KEY_CMD,
        "~src" => T::SRC_CMD,
        _ => return None,
    })
}

/// The section kinds whose content the lexer understands (Java's `LEXABLE_SECTIONS`); any other kind is an opaque
/// DSL its content raw-skips to the next header.
pub const LEXABLE_SECTIONS: &[&str] = &[
    "Pure",
    "Mapping",
    "Relational",
    "Connection",
    "Runtime",
    "Data",
    "Service",
    "DataSpace",
    "Persistence",
    "Snowflake",
    "ExternalFormat",
    "FileGeneration",
    "MemSql",
    "BigQuery",
    "HostedService",
    "FunctionJar",
    "Text",
    "GenerationSpecification",
    "DataQualityValidation",
    "ServiceStore",
    "MongoDB",
    "Deephaven",
    "Elasticsearch",
    "QueryPostProcessor",
];

/// Every `###Section` header the lexer saw (Java's `TokenStream.SectionHeader`): its name, where the name starts,
/// where its content begins, and the non-comment residue after the name on the header line, if any (with its
/// 1-based line and column, counted in UTF-16 units; `-1` without one).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SectionHeader {
    pub name: Vec<u16>,
    pub name_offset: usize,
    pub content_start_offset: usize,
    pub residue_token: Option<Vec<u16>>,
    pub residue_line: i32,
    pub residue_col: i32,
}

/// A section whose content the lexer raw-skipped as opaque (Java's `TokenStream.SkippedSection`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkippedSection {
    pub name: Vec<u16>,
    pub name_offset: usize,
    pub start_offset: usize,
    pub end_offset: usize,
}

/// One token: its type and its `[start, end)` offsets into the source.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Token {
    pub ty: TokenType,
    pub start: usize,
    pub end: usize,
}

/// The lexer's result (Java's `TokenStream`): the tokens over a borrowed source, the section headers and skipped
/// sections, and the positions -- the line index and each token's span, built once on first use, as Java's are.
#[derive(Debug)]
pub struct TokenStream<'a> {
    source: &'a [u16],
    tokens: Vec<Token>,
    pub section_headers: Vec<SectionHeader>,
    pub skipped_sections: Vec<SkippedSection>,
    line_starts: OnceCell<Vec<usize>>,
    positions: OnceCell<Vec<[usize; 4]>>,
}

impl<'a> TokenStream<'a> {
    /// The source the offsets index.
    pub fn source(&self) -> &'a [u16] {
        self.source
    }

    pub fn count(&self) -> usize {
        self.tokens.len()
    }

    pub fn tokens(&self) -> &[Token] {
        &self.tokens
    }

    pub fn ty(&self, i: usize) -> TokenType {
        self.tokens[i].ty
    }

    pub fn start(&self, i: usize) -> usize {
        self.tokens[i].start
    }

    pub fn end(&self, i: usize) -> usize {
        self.tokens[i].end
    }

    /// The token's verbatim text, as UTF-16 units.
    pub fn text(&self, i: usize) -> &'a [u16] {
        &self.source[self.tokens[i].start..self.tokens[i].end]
    }

    /// Offsets at which each line starts (Java's `lineStarts`): over the whole source, whatever range was lexed.
    fn line_starts(&self) -> &[usize] {
        self.line_starts.get_or_init(|| {
            let mut ls = vec![0];
            for (i, &c) in self.source.iter().enumerate() {
                if c == u16::from(b'\n') {
                    ls.push(i + 1);
                }
            }
            ls
        })
    }

    /// 1-based line of a source offset (Java's `lineOf`: a binary search over the line index).
    pub fn line_of(&self, offset: usize) -> usize {
        let ls = self.line_starts();
        let (mut lo, mut hi) = (0usize, ls.len() - 1);
        while lo < hi {
            let mid = (lo + hi + 1) >> 1;
            if ls[mid] <= offset {
                lo = mid;
            } else {
                hi = mid - 1;
            }
        }
        lo + 1
    }

    /// 1-based column of a source offset in CODE POINTS (Java's `columnOf`: `codePointCount(lineStart, offset) + 1`,
    /// a surrogate pair one column, a lone surrogate one each) -- the column an error names.
    pub fn column_of(&self, offset: usize) -> usize {
        let line_start = self.line_starts()[self.line_of(offset) - 1];
        code_point_count(self.source, line_start, offset) + 1
    }

    /// Each token's start line, start column, end line and end column (Java's `tokenPositions`): 1-based, columns
    /// in UTF-16 units, the end column inclusive (the last character's, an empty token's its start's).
    fn positions(&self) -> &[[usize; 4]] {
        self.positions.get_or_init(|| {
            let ls = self.line_starts();
            let n = self.tokens.len();
            let mut out = Vec::with_capacity(n);
            let mut line = if n > 0 { self.line_of(self.tokens[0].start) - 1 } else { 0 };
            for t in &self.tokens {
                let s = t.start;
                while line + 1 < ls.len() && ls[line + 1] <= s {
                    line += 1;
                }
                let sl = line + 1;
                let sc = s - ls[line] + 1;
                // Java: Math.max(s, ends[i] - 1), in ints (an end of 0 gives -1, below any start)
                let e = if t.end == 0 { s } else { s.max(t.end - 1) };
                let mut eline = line;
                while eline + 1 < ls.len() && ls[eline + 1] <= e {
                    eline += 1;
                }
                out.push([sl, sc, eline + 1, e - ls[eline] + 1]);
            }
            out
        })
    }

    pub fn start_line(&self, i: usize) -> usize {
        self.positions()[i][0]
    }

    pub fn start_column(&self, i: usize) -> usize {
        self.positions()[i][1]
    }

    pub fn end_line(&self, i: usize) -> usize {
        self.positions()[i][2]
    }

    pub fn end_column(&self, i: usize) -> usize {
        self.positions()[i][3]
    }

    /// The characters `[from, to)` of this stream's source lexed again in normal mode (Java's `lexRange`): an
    /// island's content, its offsets the source's own.
    pub fn lex_range(&self, from: usize, to: usize) -> Result<TokenStream<'a>, String> {
        if to > self.source.len() || from > to {
            return Err(format!("lexRange({from}, {to}) outside the source"));
        }
        Ok(tokenize_range(self.source, from, to))
    }
}

/// Java's `String.codePointCount(begin, end)`: a high surrogate followed by a low one counts once.
pub fn code_point_count(s: &[u16], begin: usize, end: usize) -> usize {
    let mut n = 0;
    let mut i = begin;
    while i < end {
        if is_high_surrogate(s[i]) && i + 1 < end && is_low_surrogate(s[i + 1]) {
            i += 2;
        } else {
            i += 1;
        }
        n += 1;
    }
    n
}

fn is_high_surrogate(c: u16) -> bool {
    (0xD800..=0xDBFF).contains(&c)
}

fn is_low_surrogate(c: u16) -> bool {
    (0xDC00..=0xDFFF).contains(&c)
}

/// Lex a whole source (Java's `Lexer.tokenize`).
pub fn tokenize(source: &[u16]) -> TokenStream<'_> {
    tokenize_range(source, 0, source.len())
}

/// Lex `source`'s characters `[from, to)` only, every offset the source's own (Java's `Lexer.tokenizeRange`).
pub fn tokenize_range(source: &[u16], from: usize, to: usize) -> TokenStream<'_> {
    let mut lexer = Lexer {
        source,
        length: to,
        pos: from,
        island_depth: 0,
        tokens: Vec::with_capacity(((to - from) / 3).max(16)),
        section_headers: Vec::new(),
        skipped_sections: Vec::new(),
    };
    lexer.run();
    TokenStream {
        source,
        tokens: lexer.tokens,
        section_headers: lexer.section_headers,
        skipped_sections: lexer.skipped_sections,
        line_starts: OnceCell::new(),
        positions: OnceCell::new(),
    }
}

struct Lexer<'a> {
    source: &'a [u16],
    /// The scan's end (Java's `length`): the source's length, or a range's end.
    length: usize,
    pos: usize,
    island_depth: i32,
    tokens: Vec<Token>,
    section_headers: Vec<SectionHeader>,
    skipped_sections: Vec<SkippedSection>,
}

fn is_ident_start(c: u16) -> bool {
    matches!(c, 0x41..=0x5A | 0x61..=0x7A | 0x5F | 0x30..=0x39)
}

fn is_ident_part(c: u16) -> bool {
    matches!(c, 0x41..=0x5A | 0x61..=0x7A | 0x30..=0x39 | 0x5F | 0x24)
}

const fn u(c: char) -> u16 {
    c as u16
}

impl<'a> Lexer<'a> {
    fn at(&self, i: usize) -> u16 {
        self.source[i]
    }

    /// Java's `source.startsWith(prefix, offset)`, over the WHOLE source (not the range's bound).
    fn starts_with(&self, offset: usize, prefix: &str) -> bool {
        let p: Vec<u16> = prefix.encode_utf16().collect();
        offset + p.len() <= self.source.len() && self.source[offset..offset + p.len()] == p[..]
    }

    fn ascii(&self, from: usize, to: usize) -> String {
        String::from_utf16_lossy(&self.source[from..to])
    }

    fn run(&mut self) {
        while self.pos < self.length {
            if self.island_depth > 0 {
                self.scan_island_token();
            } else {
                self.scan_normal_token();
            }
        }
    }

    fn emit(&mut self, ty: TokenType, start: usize, end: usize) {
        self.tokens.push(Token { ty, start, end });
    }

    fn scan_normal_token(&mut self) {
        let c = self.at(self.pos);
        if c == u(' ') || c == u('\t') || c == u('\r') || c == u('\n') {
            self.skip_whitespace();
            return;
        }
        if c == u('/') {
            if self.pos + 1 < self.length {
                let c2 = self.at(self.pos + 1);
                if c2 == u('/') {
                    self.skip_line_comment();
                    return;
                }
                if c2 == u('*') {
                    self.skip_block_comment();
                    return;
                }
            }
            self.emit(T::DIVIDE, self.pos, self.pos + 1);
            self.pos += 1;
            return;
        }
        if self.at_section_boundary(self.pos) {
            self.skip_section_header();
            return;
        }
        if c == u('\'') {
            self.scan_string_literal();
            return;
        }
        if c == u('"') {
            self.scan_quoted_string();
            return;
        }
        if c == u('%') {
            self.scan_percent();
            return;
        }
        if c == u('#') {
            self.scan_hash();
            return;
        }
        if c == u('~') {
            self.scan_tilde();
            return;
        }
        if c == u('-') {
            self.scan_minus_or_arrow();
            return;
        }
        if (u('0')..=u('9')).contains(&c) {
            self.scan_numeric_literal();
            return;
        }
        if is_ident_start(c) {
            self.scan_identifier_or_keyword();
            return;
        }
        if c == u('?') && self.pos + 1 < self.length && self.at(self.pos + 1) == u('[') {
            self.scan_file_name();
            return;
        }
        if self.scan_multi_char_punct(c) {
            return;
        }
        self.scan_single_char_punct(c);
    }

    /// A LINE-ANCHORED `###` at `p` (Java's `atSectionBoundary`): `p == 0` is the source's start, not a range's.
    fn at_section_boundary(&self, p: usize) -> bool {
        p + 2 < self.length
            && self.at(p) == u('#')
            && self.at(p + 1) == u('#')
            && self.at(p + 2) == u('#')
            && (p == 0 || self.at(p - 1) == u('\n'))
    }

    fn skip_whitespace(&mut self) {
        while self.pos < self.length {
            let c = self.at(self.pos);
            if c == u(' ') || c == u('\t') || c == u('\r') || c == u('\n') {
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    fn skip_line_comment(&mut self) {
        self.pos += 2;
        while self.pos < self.length && self.at(self.pos) != u('\n') {
            self.pos += 1;
        }
        if self.pos < self.length {
            self.pos += 1;
        }
    }

    fn skip_block_comment(&mut self) {
        let start = self.pos;
        self.pos += 2;
        while self.pos < self.length {
            if self.at_section_boundary(self.pos) {
                self.emit(T::INVALID, start, self.pos);
                return;
            }
            if self.at(self.pos) == u('*') && self.pos + 1 < self.length && self.at(self.pos + 1) == u('/') {
                self.pos += 2;
                return;
            }
            self.pos += 1;
        }
        self.emit(T::INVALID, start, self.pos);
    }

    fn skip_section_header(&mut self) {
        let name_start = self.pos + 3;
        let mut name_end = name_start;
        while name_end < self.length && is_letter_or_digit(self.at(name_end)) {
            name_end += 1;
        }
        let kind: Vec<u16> = self.source[name_start..name_end].to_vec();
        while self.pos < self.length && self.at(self.pos) != u('\n') {
            self.pos += 1;
        }
        if self.pos < self.length {
            self.pos += 1;
        }
        // HEADER-LINE RESIDUE (Java: source.indexOf('\n', nameEnd) over the whole source, clamped to the scan's end)
        let mut residue = None;
        let mut r_line = -1;
        let mut r_col = -1;
        let mut line_end = self.source[name_end.min(self.source.len())..]
            .iter()
            .position(|&c| c == u('\n'))
            .map(|p| name_end + p)
            .unwrap_or(usize::MAX);
        if line_end != usize::MAX && line_end > self.length {
            line_end = self.length;
        }
        if line_end == usize::MAX {
            line_end = self.length;
        }
        let mut r = name_end;
        while r < line_end && (self.at(r) == u(' ') || self.at(r) == u('\t') || self.at(r) == u('\r')) {
            r += 1;
        }
        let comment = r + 1 < line_end && self.at(r) == u('/') && self.at(r + 1) == u('/');
        if r < line_end && !comment {
            let mut we = r;
            while we < line_end && !is_whitespace(self.at(we)) {
                we += 1;
            }
            residue = Some(self.source[r..we].to_vec());
            r_line = 1;
            r_col = 1;
            for k in 0..r {
                if self.at(k) == u('\n') {
                    r_line += 1;
                    r_col = 1;
                } else {
                    r_col += 1;
                }
            }
        }
        self.section_headers.push(SectionHeader {
            name: kind.clone(),
            name_offset: name_start,
            content_start_offset: self.pos,
            residue_token: residue,
            residue_line: r_line,
            residue_col: r_col,
        });
        let kind_text = String::from_utf16_lossy(&kind);
        if !kind.is_empty() && !LEXABLE_SECTIONS.contains(&kind_text.as_str()) {
            let skip_start = self.pos;
            while self.pos < self.length {
                if self.at(self.pos) == u('#')
                    && self.pos + 3 <= self.length
                    && self.starts_with(self.pos, "###")
                    && (self.pos == 0 || self.at(self.pos - 1) == u('\n'))
                {
                    break;
                }
                self.pos += 1;
            }
            self.skipped_sections.push(SkippedSection {
                name: kind,
                name_offset: name_start,
                start_offset: skip_start,
                end_offset: self.pos,
            });
        }
    }

    fn skip_whitespace_only(&mut self) {
        self.skip_whitespace();
    }

    fn scan_string_literal(&mut self) {
        let start = self.pos;
        if self.pos + 2 < self.length && self.at(self.pos + 1) == u('\'') && self.at(self.pos + 2) == u('\'') {
            self.pos += 3;
            while self.pos < self.length && !self.at_section_boundary(self.pos) {
                if self.at(self.pos) == u('\'')
                    && self.pos + 2 < self.length
                    && self.at(self.pos + 1) == u('\'')
                    && self.at(self.pos + 2) == u('\'')
                {
                    self.pos += 3;
                    self.emit(T::DOC_STRING, start, self.pos);
                    return;
                }
                self.pos += 1;
            }
            self.emit(T::DOC_STRING, start, self.pos);
            return;
        }
        self.pos += 1;
        while self.pos < self.length && !self.at_section_boundary(self.pos) {
            let c = self.at(self.pos);
            if c == u('\\') {
                self.pos = (self.pos + 2).min(self.length);
            } else if c == u('\'') {
                self.pos += 1;
                self.emit(T::STRING, start, self.pos);
                return;
            } else {
                self.pos += 1;
            }
        }
        self.emit(T::STRING, start, self.pos);
    }

    fn scan_quoted_string(&mut self) {
        let start = self.pos;
        self.pos += 1;
        while self.pos < self.length && !self.at_section_boundary(self.pos) {
            let c = self.at(self.pos);
            if c == u('\\') {
                self.pos = (self.pos + 2).min(self.length);
            } else if c == u('"') {
                self.pos += 1;
                self.emit(T::QUOTED_STRING, start, self.pos);
                return;
            } else {
                self.pos += 1;
            }
        }
        self.emit(T::QUOTED_STRING, start, self.pos);
    }

    fn is_ascii_digit_at(&self, i: usize) -> bool {
        (u('0')..=u('9')).contains(&self.at(i))
    }

    fn scan_numeric_literal(&mut self) {
        let start = self.pos;
        while self.pos < self.length && self.is_ascii_digit_at(self.pos) {
            self.pos += 1;
        }
        if self.pos < self.length
            && self.at(self.pos) == u('.')
            && self.pos + 1 < self.length
            && self.at(self.pos + 1) != u('.')
        {
            self.pos += 1;
            while self.pos < self.length && self.is_ascii_digit_at(self.pos) {
                self.pos += 1;
            }
            self.scan_exponent();
            if self.pos < self.length && (self.at(self.pos) == u('d') || self.at(self.pos) == u('D')) {
                self.pos += 1;
                self.emit(T::DECIMAL, start, self.pos);
                return;
            }
            if self.pos < self.length && (self.at(self.pos) == u('f') || self.at(self.pos) == u('F')) {
                self.pos += 1;
            }
            self.emit(T::FLOAT, start, self.pos);
            return;
        }
        if self.pos < self.length
            && (self.at(self.pos) == u('e') || self.at(self.pos) == u('E'))
            && self.pos + 1 < self.length
            && (is_digit(self.at(self.pos + 1))
                || ((self.at(self.pos + 1) == u('+') || self.at(self.pos + 1) == u('-'))
                    && self.pos + 2 < self.length
                    && is_digit(self.at(self.pos + 2))))
        {
            self.scan_exponent();
            if self.pos < self.length && (self.at(self.pos) == u('d') || self.at(self.pos) == u('D')) {
                self.pos += 1;
                self.emit(T::DECIMAL, start, self.pos);
                return;
            }
            if self.pos < self.length && (self.at(self.pos) == u('f') || self.at(self.pos) == u('F')) {
                self.pos += 1;
            }
            self.emit(T::FLOAT, start, self.pos);
            return;
        }
        if self.pos < self.length && (self.at(self.pos) == u('d') || self.at(self.pos) == u('D')) {
            self.pos += 1;
            self.emit(T::DECIMAL, start, self.pos);
            return;
        }
        self.emit(T::INTEGER, start, self.pos);
    }

    fn scan_exponent(&mut self) {
        if self.pos < self.length && (self.at(self.pos) == u('e') || self.at(self.pos) == u('E')) {
            self.pos += 1;
            if self.pos < self.length && (self.at(self.pos) == u('+') || self.at(self.pos) == u('-')) {
                self.pos += 1;
            }
            while self.pos < self.length && self.is_ascii_digit_at(self.pos) {
                self.pos += 1;
            }
        }
    }

    fn scan_percent(&mut self) {
        let start = self.pos;
        self.pos += 1;
        if self.pos >= self.length {
            self.emit(T::INVALID, start, self.pos);
            return;
        }
        if self.pos + 6 <= self.length
            && self.starts_with(self.pos, "latest")
            && (self.pos + 6 >= self.length || !is_ident_part(self.at(self.pos + 6)))
        {
            self.pos += 6;
            self.emit(T::LATEST_DATE, start, self.pos);
            return;
        }
        let mut next = self.at(self.pos);
        if next == u('-') && self.pos + 1 < self.length && self.is_ascii_digit_at(self.pos + 1) {
            self.pos += 1;
            next = self.at(self.pos);
        }
        if (u('0')..=u('9')).contains(&next) {
            let mut has_dash = false;
            let mut has_colon = false;
            let mut scan = self.pos;
            while scan < self.length {
                let sc = self.at(scan);
                if (u('0')..=u('9')).contains(&sc) {
                    scan += 1;
                    continue;
                }
                if sc == u('-') {
                    if scan + 1 < self.length && self.at(scan + 1) == u('>') {
                        break;
                    }
                    has_dash = true;
                    scan += 1;
                    continue;
                }
                if sc == u(':') {
                    has_colon = true;
                    scan += 1;
                    continue;
                }
                if sc == u('T') || sc == u('.') {
                    scan += 1;
                    continue;
                }
                if sc == u('+') && scan > self.pos {
                    scan += 1;
                    continue;
                }
                if sc == u('Z') && (scan + 1 >= self.length || !is_letter_or_digit(self.at(scan + 1))) {
                    scan += 1;
                    break;
                }
                break;
            }
            self.pos = scan;
            let ty = if has_colon && !has_dash { T::STRICTTIME } else { T::DATE };
            self.emit(ty, start, self.pos);
            return;
        }
        self.pos = start + 1;
        self.emit(T::INVALID, start, self.pos);
    }

    fn scan_hash(&mut self) {
        let start = self.pos;
        if self.pos + 3 < self.length && self.starts_with(self.pos + 1, "TDS") {
            let braced = self.pos + 4 < self.length && self.at(self.pos + 4) == u('{');
            self.pos += 1;
            if braced {
                while self.pos < self.length
                    && !self.starts_with(self.pos, "}#")
                    && !self.at_section_boundary(self.pos)
                {
                    self.pos += 1;
                }
                if self.pos < self.length && self.starts_with(self.pos, "}#") {
                    self.pos += 2;
                }
            } else {
                while self.pos < self.length && self.at(self.pos) != u('#') && !self.at_section_boundary(self.pos) {
                    self.pos += 1;
                }
                if self.pos < self.length && self.at(self.pos) == u('#') {
                    self.pos += 1;
                }
            }
            self.emit(T::TDS_LITERAL, start, self.pos);
            return;
        }
        if self.pos + 1 < self.length && self.at(self.pos + 1) == u('/') {
            self.pos += 1;
            while self.pos < self.length && self.at(self.pos) != u('#') && !self.at_section_boundary(self.pos) {
                self.pos += 1;
            }
            if self.pos < self.length && self.at(self.pos) == u('#') {
                self.pos += 1;
            }
            self.emit(T::PATH_LITERAL, start, self.pos);
            return;
        }
        self.pos += 1;
        while self.pos < self.length && self.at(self.pos) != u('{') && self.at(self.pos) != u('#') {
            self.pos += 1;
        }
        if self.pos < self.length && self.at(self.pos) == u('{') {
            self.pos += 1;
            self.emit(T::ISLAND_OPEN, start, self.pos);
            self.island_depth += 1;
            return;
        }
        self.emit(T::INVALID, start, self.pos);
    }

    fn scan_tilde(&mut self) {
        let start = self.pos;
        if self.pos + 1 < self.length && is_ident_start(self.at(self.pos + 1)) {
            let mut id_end = self.pos + 1;
            while id_end < self.length && is_ident_part(self.at(id_end)) {
                id_end += 1;
            }
            if let Some(ty) = tilde_command(&self.ascii(start, id_end)) {
                self.pos = id_end;
                self.emit(ty, start, self.pos);
                return;
            }
        }
        self.emit(T::TILDE, self.pos, self.pos + 1);
        self.pos += 1;
    }

    fn scan_minus_or_arrow(&mut self) {
        if self.pos + 1 < self.length && self.at(self.pos + 1) == u('>') {
            self.emit(T::ARROW, self.pos, self.pos + 2);
            self.pos += 2;
            return;
        }
        self.emit(T::MINUS, self.pos, self.pos + 1);
        self.pos += 1;
    }

    fn scan_brace_open(&mut self) {
        if self.pos + 7 < self.length && self.starts_with(self.pos, "{target}") {
            self.emit(T::TARGET, self.pos, self.pos + 8);
            self.pos += 8;
            return;
        }
        self.emit(T::BRACE_OPEN, self.pos, self.pos + 1);
        self.pos += 1;
    }

    fn scan_file_name(&mut self) {
        let start = self.pos;
        self.pos += 2;
        while self.pos < self.length {
            let c = self.at(self.pos);
            if is_letter_or_digit(c) || c == u('_') || c == u('.') || c == u('/') {
                self.pos += 1;
            } else {
                break;
            }
        }
        self.emit(T::INVALID, start, self.pos);
    }

    fn scan_identifier_or_keyword(&mut self) {
        let start = self.pos;
        self.pos += 1;
        while self.pos < self.length && is_ident_part(self.at(self.pos)) {
            self.pos += 1;
        }
        let text = self.ascii(start, self.pos);
        if self.check_multi_word_token(&text, start) {
            return;
        }
        let ty = keyword(&text).unwrap_or(T::VALID_STRING);
        self.emit(ty, start, self.pos);
    }

    /// `word` at `pos`, not followed by an identifier character (Java: `pos + n <= length && startsWith(word, pos)
    /// && (pos + n >= length || !isIdentPart(charAt(pos + n)))`).
    fn word_at(&self, word: &str) -> bool {
        let n = word.len();
        self.pos + n <= self.length
            && self.starts_with(self.pos, word)
            && (self.pos + n >= self.length || !is_ident_part(self.at(self.pos + n)))
    }

    fn check_multi_word_token(&mut self, text: &str, start: usize) -> bool {
        match text {
            "PRIMARY" => {
                let saved = self.pos;
                self.skip_whitespace_only();
                if self.word_at("KEY") {
                    self.pos += 3;
                    self.emit(T::PRIMARY_KEY, start, self.pos);
                    return true;
                }
                self.pos = saved;
            }
            "NOT" => {
                let saved = self.pos;
                self.skip_whitespace_only();
                if self.word_at("NULL") {
                    self.pos += 4;
                    self.emit(T::NOT_NULL, start, self.pos);
                    return true;
                }
                self.pos = saved;
            }
            "is" => {
                let saved = self.pos;
                self.skip_whitespace_only();
                if self.word_at("not") {
                    self.pos += 3;
                    self.skip_whitespace_only();
                    if self.word_at("null") {
                        self.pos += 4;
                        self.emit(T::IS_NOT_NULL, start, self.pos);
                        return true;
                    }
                    self.pos = saved;
                } else if self.word_at("null") {
                    self.pos += 4;
                    self.emit(T::IS_NULL, start, self.pos);
                    return true;
                } else {
                    self.pos = saved;
                }
            }
            _ => {}
        }
        false
    }

    fn scan_multi_char_punct(&mut self, c: u16) -> bool {
        let next = |s: &Self, ch: char| s.pos + 1 < s.length && s.at(s.pos + 1) == u(ch);
        if c == u(':') {
            if next(self, ':') {
                self.emit(T::PATH_SEPARATOR, self.pos, self.pos + 2);
                self.pos += 2;
                return true;
            }
            self.emit(T::COLON, self.pos, self.pos + 1);
            self.pos += 1;
            return true;
        }
        if c == u('.') && next(self, '.') {
            self.emit(T::DOT_DOT, self.pos, self.pos + 2);
            self.pos += 2;
            return true;
        }
        if c == u('.') && self.pos + 1 < self.length && self.is_ascii_digit_at(self.pos + 1) {
            self.scan_numeric_literal();
            return true;
        }
        if c == u('&') && next(self, '&') {
            self.emit(T::AND, self.pos, self.pos + 2);
            self.pos += 2;
            return true;
        }
        if c == u('|') && next(self, '|') {
            self.emit(T::OR, self.pos, self.pos + 2);
            self.pos += 2;
            return true;
        }
        if c == u('=') && next(self, '=') {
            self.emit(T::TEST_EQUAL, self.pos, self.pos + 2);
            self.pos += 2;
            return true;
        }
        if c == u('!') && next(self, '=') {
            self.emit(T::TEST_NOT_EQUAL, self.pos, self.pos + 2);
            self.pos += 2;
            return true;
        }
        if c == u('<') {
            if self.pos + 1 < self.length {
                let c2 = self.at(self.pos + 1);
                if c2 == u('=') {
                    self.emit(T::LESS_OR_EQUAL, self.pos, self.pos + 2);
                    self.pos += 2;
                    return true;
                }
                if c2 == u('>') {
                    self.emit(T::NOT_EQUAL, self.pos, self.pos + 2);
                    self.pos += 2;
                    return true;
                }
            }
            self.emit(T::LESS_THAN, self.pos, self.pos + 1);
            self.pos += 1;
            return true;
        }
        if c == u('>') {
            if next(self, '=') {
                self.emit(T::GREATER_OR_EQUAL, self.pos, self.pos + 2);
                self.pos += 2;
                return true;
            }
            self.emit(T::GREATER_THAN, self.pos, self.pos + 1);
            self.pos += 1;
            return true;
        }
        false
    }

    fn scan_single_char_punct(&mut self, c: u16) {
        let ty = match char::from_u32(u32::from(c)) {
            Some('{') => {
                self.scan_brace_open();
                return;
            }
            Some('}') => T::BRACE_CLOSE,
            Some('(') => T::PAREN_OPEN,
            Some(')') => T::PAREN_CLOSE,
            Some('[') => T::BRACKET_OPEN,
            Some(']') => T::BRACKET_CLOSE,
            Some(',') => T::COMMA,
            Some('=') => T::EQUAL,
            Some(';') => T::SEMI_COLON,
            Some('.') => T::DOT,
            Some('$') => T::DOLLAR,
            Some('^') => T::NEW_SYMBOL,
            Some('|') => T::PIPE,
            Some('@') => T::AT,
            Some('+') => T::PLUS,
            Some('*') => T::STAR,
            Some('!') => T::NOT,
            Some('?') => T::QUESTION,
            Some('\u{2286}') => T::SUBSET,
            _ => T::INVALID,
        };
        self.emit(ty, self.pos, self.pos + 1);
        self.pos += 1;
    }

    fn scan_island_token(&mut self) {
        let c = self.at(self.pos);
        if c == u('}') && self.pos + 1 < self.length && self.at(self.pos + 1) == u('#') {
            self.emit(T::ISLAND_END, self.pos, self.pos + 2);
            self.pos += 2;
            self.island_depth -= 1;
            return;
        }
        if c == u('}') {
            self.emit(T::ISLAND_BRACE_CLOSE, self.pos, self.pos + 1);
            self.pos += 1;
            return;
        }
        if c == u('#') && self.pos + 1 < self.length && self.at(self.pos + 1) == u('{') {
            self.emit(T::ISLAND_START, self.pos, self.pos + 2);
            self.pos += 2;
            self.island_depth += 1;
            return;
        }
        if c == u('#') {
            self.emit(T::ISLAND_HASH, self.pos, self.pos + 1);
            self.pos += 1;
            return;
        }
        if c == u('{') {
            self.emit(T::ISLAND_BRACE_OPEN, self.pos, self.pos + 1);
            self.pos += 1;
            return;
        }
        let start = self.pos;
        while self.pos < self.length && !self.at_section_boundary(self.pos) {
            let ic = self.at(self.pos);
            if ic == u('{') || ic == u('}') || ic == u('#') {
                break;
            }
            self.pos += 1;
        }
        if self.pos > start {
            self.emit(T::ISLAND_CONTENT, start, self.pos);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn units(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    #[test]
    fn an_error_column_counts_code_points_and_a_span_column_utf16_units() {
        // 'a😀b' then x, then y on line 2: the emoji is one code point, two UTF-16 units
        let src = units("'a\u{1F600}b' x\ny");
        let ts = tokenize(&src);
        assert_eq!(ts.ty(0), T::STRING);
        assert_eq!(ts.ty(1), T::VALID_STRING);
        // x starts at unit 7: span column 8, error column 7 (the pair is one column there)
        assert_eq!(ts.start(1), 7);
        assert_eq!(ts.start_column(1), 8);
        assert_eq!(ts.column_of(ts.start(1)), 7);
        assert_eq!((ts.start_line(2), ts.start_column(2)), (2, 1));
    }

    #[test]
    fn a_lone_surrogate_is_one_unit_and_one_code_point() {
        let src = vec![u16::from(b'\''), 0xD800, u16::from(b'\''), u16::from(b' '), u16::from(b'z')];
        let ts = tokenize(&src);
        assert_eq!(ts.ty(0), T::STRING);
        assert_eq!(ts.end(0), 3);
        assert_eq!(code_point_count(&src, 0, 3), 3);
        assert_eq!(ts.column_of(4), 5);
        // in punctuation position, a lone surrogate lexes INVALID, one unit
        let lone = [0xDC00];
        let alone = tokenize(&lone);
        assert_eq!((alone.ty(0), alone.start(0), alone.end(0)), (T::INVALID, 0, 1));
    }

    #[test]
    fn a_range_scan_keeps_the_source_offsets() {
        let src = units("Class a::B { x: Integer[1]; }");
        let ts = tokenize(&src);
        let inner = ts.lex_range(13, 26).unwrap();
        assert_eq!(inner.ty(0), T::VALID_STRING);
        assert_eq!(inner.start(0), 13);
        assert!(ts.lex_range(5, 400).is_err());
    }
}
