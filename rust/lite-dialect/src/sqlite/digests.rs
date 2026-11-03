// Copyright 2026 Legend Contributors
// SPDX-License-Identifier: Apache-2.0

//! Java's `SqliteDigests` (`core/src/main/java/com/legend/sql/dialect/SqliteDigests.java`): MD5, SHA-1 and SHA-256
//! written in SQLite's own SQL. The text's UTF-8 bytes are padded as each digest pads them; one walk builds each
//! 64-byte chunk's words (for the SHAs, the message schedule after the chunk's 16); a second walk runs the rounds, one
//! row per round, the chunk's state added in after its last. 32-bit arithmetic is the integer's low 32 bits; SQLite
//! has no XOR, so `x ^ y` is `(x | y) & ~(x & y)`. The database computes every value; this module writes the walks.

use super::{P, pc};
use crate::writer::SqlWriter;

const M32: &str = "4294967295";
const HEX: &str = "'0123456789ABCDEF'";

/// MD5's per-round constants and shifts.
const MD5_K: [u64; 64] = [
    0xd76aa478, 0xe8c7b756, 0x242070db, 0xc1bdceee, 0xf57c0faf, 0x4787c62a, 0xa8304613, 0xfd469501, 0x698098d8,
    0x8b44f7af, 0xffff5bb1, 0x895cd7be, 0x6b901122, 0xfd987193, 0xa679438e, 0x49b40821, 0xf61e2562, 0xc040b340,
    0x265e5a51, 0xe9b6c7aa, 0xd62f105d, 0x02441453, 0xd8a1e681, 0xe7d3fbc8, 0x21e1cde6, 0xc33707d6, 0xf4d50d87,
    0x455a14ed, 0xa9e3e905, 0xfcefa3f8, 0x676f02d9, 0x8d2a4c8a, 0xfffa3942, 0x8771f681, 0x6d9d6122, 0xfde5380c,
    0xa4beea44, 0x4bdecfa9, 0xf6bb4b60, 0xbebfbc70, 0x289b7ec6, 0xeaa127fa, 0xd4ef3085, 0x04881d05, 0xd9d4d039,
    0xe6db99e5, 0x1fa27cf8, 0xc4ac5665, 0xf4292244, 0x432aff97, 0xab9423a7, 0xfc93a039, 0x655b59c3, 0x8f0ccc92,
    0xffeff47d, 0x85845dd1, 0x6fa87e4f, 0xfe2ce6e0, 0xa3014314, 0x4e0811a1, 0xf7537e82, 0xbd3af235, 0x2ad7d2bb,
    0xeb86d391,
];
const MD5_S: [u64; 64] = [
    7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20,
    4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15,
    21,
];

/// SHA-256's round constants.
const SHA256_K: [u64; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5, 0xd807aa98,
    0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786,
    0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8,
    0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13,
    0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819,
    0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a,
    0x5b9cca4f, 0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
    0xc67178f2,
];

/// The MD5 of `text`'s UTF-8 bytes, as 32 lowercase hex digits; NULL for NULL.
pub fn md5(text: P<'_>) -> P<'_> {
    let f = format!(
        "CASE WHEN r.i < 16 THEN (r.b & r.c) | (~r.b & r.d) WHEN r.i < 32 THEN (r.d & r.b) | (~r.d & r.c) WHEN r.i < 48 \
         THEN {} ELSE {} END",
        xor(&xor("r.b", "r.c"), "r.d"),
        xor("r.c", "r.b | ~r.d")
    );
    let g = "CASE WHEN r.i < 16 THEN r.i WHEN r.i < 32 THEN (5 * r.i + 1) % 16 WHEN r.i < 48 THEN (3 * r.i + 5) % 16 \
             ELSE (7 * r.i) % 16 END";
    let t = format!("((({f}) + r.a + {} + {}) & {M32})", at(&list(&MD5_K), "r.i"), at("__c.w", g));
    let next = [
        "r.d".to_owned(),
        format!("((r.b + {}) & {M32})", rotl(&t, &at(&list(&MD5_S), "r.i"))),
        "r.b".to_owned(),
        "r.c".to_owned(),
    ];
    let mut out = String::new();
    for v in ["a", "b", "c", "d"] {
        if !out.is_empty() {
            out.push_str(" || ");
        }
        out.push_str(&format!(
            "printf('%02x%02x%02x%02x', r.{v}0 & 255, (r.{v}0 >> 8) & 255, (r.{v}0 >> 16) & 255, (r.{v}0 >> 24) & 255)"
        ));
    }
    rounds(text, false, 16, None, 64, &["a", "b", "c", "d"], &[0x67452301, 0xefcdab89, 0x98badcfe, 0x10325476], &next, &out)
}

/// The SHA-1 of `text`'s UTF-8 bytes, as 40 lowercase hex digits; NULL for NULL.
pub fn sha1(text: P<'_>) -> P<'_> {
    let parity = xor(&xor("r.b", "r.c"), "r.d");
    let f = format!(
        "CASE WHEN r.i < 20 THEN (r.b & r.c) | (~r.b & r.d) WHEN r.i < 40 THEN {parity} WHEN r.i < 60 THEN (r.b & r.c) | \
         (r.b & r.d) | (r.c & r.d) ELSE {parity} END"
    );
    let k = "CASE WHEN r.i < 20 THEN 1518500249 WHEN r.i < 40 THEN 1859775393 WHEN r.i < 60 THEN 2400959708 ELSE \
             3395469782 END";
    let t = format!("(({} + ({f}) + r.e + {k} + {}) & {M32})", rotl("r.a", "5"), at("__c.w", "r.i"));
    let next = [t, "r.a".to_owned(), rotl("r.b", "30"), "r.c".to_owned(), "r.d".to_owned()];
    let schedule = |w: &str, j: &str| {
        rotl(
            &xor(
                &xor(&xor(&at(w, &format!("{j} - 3")), &at(w, &format!("{j} - 8"))), &at(w, &format!("{j} - 14"))),
                &at(w, &format!("{j} - 16")),
            ),
            "1",
        )
    };
    let vars = ["a", "b", "c", "d", "e"];
    rounds(
        text,
        true,
        80,
        Some(&schedule),
        80,
        &vars,
        &[0x67452301, 0xefcdab89, 0x98badcfe, 0x10325476, 0xc3d2e1f0],
        &next,
        &hex_words(&vars),
    )
}

/// The SHA-256 of `text`'s UTF-8 bytes, as 64 lowercase hex digits; NULL for NULL.
pub fn sha256(text: P<'_>) -> P<'_> {
    let s1 = xor(&xor(&rotr("r.e", 6), &rotr("r.e", 11)), &rotr("r.e", 25));
    let ch = xor("r.e & r.f", "~r.e & r.g");
    let t1 = format!("(r.h + {s1} + {ch} + {} + {})", at(&list(&SHA256_K), "r.i"), at("__c.w", "r.i"));
    let s0 = xor(&xor(&rotr("r.a", 2), &rotr("r.a", 13)), &rotr("r.a", 22));
    let maj = xor(&xor("r.a & r.b", "r.a & r.c"), "r.b & r.c");
    let next = [
        format!("(({t1} + {s0} + {maj}) & {M32})"),
        "r.a".to_owned(),
        "r.b".to_owned(),
        "r.c".to_owned(),
        format!("((r.d + {t1}) & {M32})"),
        "r.e".to_owned(),
        "r.f".to_owned(),
        "r.g".to_owned(),
    ];
    let vars = ["a", "b", "c", "d", "e", "f", "g", "h"];
    let schedule = |w: &str, j: &str| {
        let w2 = at(w, &format!("{j} - 2"));
        let w15 = at(w, &format!("{j} - 15"));
        let sigma0 = xor(&xor(&rotr(&w15, 7), &rotr(&w15, 18)), &format!("({w15} >> 3)"));
        let sigma1 = xor(&xor(&rotr(&w2, 17), &rotr(&w2, 19)), &format!("({w2} >> 10)"));
        format!("(({sigma1} + {} + {sigma0} + {}) & {M32})", at(w, &format!("{j} - 7")), at(w, &format!("{j} - 16")))
    };
    rounds(
        text,
        true,
        64,
        Some(&schedule),
        64,
        &vars,
        &[0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19],
        &next,
        &hex_words(&vars),
    )
}

/// A schedule word after a chunk's 16: of the words so far (`w`, a JSON array) at position `j`.
type Schedule<'s> = &'s dyn Fn(&str, &str) -> String;

/// The walks: `__in` the text, `__m` its padded bytes' hex, `__w`/`__c` each chunk's `words` words, `__r` the rounds
/// (`rounds` per chunk), the digest `out` read off the row after the last chunk.
#[allow(clippy::too_many_arguments)]
fn rounds<'a>(
    text: P<'a>,
    big_endian: bool,
    words: usize,
    schedule: Option<Schedule<'_>>,
    rounds: usize,
    vars: &[&str],
    iv: &[u64],
    next: &[String],
    out: &str,
) -> P<'a> {
    let word = word("__m.h", "__w.k * 128 + 8 * __w.j", big_endian);
    let more = match schedule {
        None => word,
        Some(s) => format!("CASE WHEN __w.j < 16 THEN {word} ELSE {} END", s("__w.w", "__w.j")),
    };
    let last = format!("r.i = {rounds}");
    let mut cols = String::new();
    let mut start = String::new();
    let mut step = format!("CASE WHEN {last} THEN r.k + 1 ELSE r.k END, CASE WHEN {last} THEN 0 ELSE r.i + 1 END");
    for (i, v) in vars.iter().enumerate() {
        cols.push_str(&format!(", {v}"));
        step.push_str(&format!(", CASE WHEN {last} THEN (r.{v}0 + r.{v}) & {M32} ELSE {} END", next[i]));
    }
    for v in vars {
        cols.push_str(&format!(", {v}0"));
        step.push_str(&format!(", CASE WHEN {last} THEN (r.{v}0 + r.{v}) & {M32} ELSE r.{v}0 END"));
    }
    for _ in 0..2 {
        for v in iv {
            start.push_str(&format!(", {v}"));
        }
    }
    let n = "(length(hex(CAST(__in.v AS BLOB))) / 2)";
    let bits = format!("({n} * 8)");
    let length = if big_endian {
        format!("printf('%016X', {bits})")
    } else {
        let mut le = format!("printf('{}'", "%02X".repeat(8));
        for j in 0..8 {
            le.push_str(&format!(", ({bits} >> {}) & 255", 8 * j));
        }
        le.push(')');
        le
    };
    let padded = format!("hex(CAST(__in.v AS BLOB)) || '80' || hex(zeroblob((((55 - {n}) % 64) + 64) % 64)) || {length}");
    let tail = format!(
        "), __m(h) AS (SELECT {padded} FROM __in WHERE __in.v IS NOT NULL), __w(k, j, w) AS (SELECT 0, 0, '[]' UNION \
         ALL SELECT CASE WHEN __w.j = {words} THEN __w.k + 1 ELSE __w.k END, CASE WHEN __w.j = {words} THEN 0 ELSE \
         __w.j + 1 END, CASE WHEN __w.j = {words} THEN '[]' ELSE json_insert(__w.w, '$[#]', {more}) END FROM __w, __m \
         WHERE __w.j < {words} OR (__w.k + 1) * 128 < length(__m.h)), __c(k, w) AS (SELECT k, w FROM __w WHERE j = \
         {words}), __r(k, i{cols}) AS (SELECT 0, 0{start} UNION ALL SELECT {step} FROM __r AS r JOIN __c ON __c.k = \
         r.k) SELECT {out} FROM __r AS r WHERE r.k = (SELECT count(*) FROM __c) AND EXISTS (SELECT 1 FROM __m))"
    );
    pc(move |w: &mut SqlWriter| {
        w.append("(WITH RECURSIVE __in(v) AS (SELECT ");
        text(w)?;
        w.append(&tail);
        Ok(())
    })
}

/// The state words as hex, eight digits each, big-endian.
fn hex_words(vars: &[&str]) -> String {
    vars.iter().map(|v| format!("printf('%08x', r.{v}0)")).collect::<Vec<_>>().join(" || ")
}

/// `x ^ y` of two 32-bit words.
fn xor(x: &str, y: &str) -> String {
    format!("((({x}) | ({y})) & ~(({x}) & ({y})))")
}

fn rotl(x: &str, s: &str) -> String {
    format!("(((({x}) << ({s})) | (({x}) >> (32 - ({s})))) & {M32})")
}

fn rotr(x: &str, s: u32) -> String {
    format!("(((({x}) >> {s}) | (({x}) << {})) & {M32})", 32 - s)
}

/// Element `i` of the JSON array `array`.
fn at(array: &str, i: &str) -> String {
    format!("json_extract({array}, '$[' || ({i}) || ']')")
}

fn list(values: &[u64]) -> String {
    let joined: Vec<String> = values.iter().map(u64::to_string).collect();
    format!("'[{}]'", joined.join(","))
}

/// The 32-bit word whose first byte's hex is at offset `at` (0-based) of the hex text `h`.
fn word(h: &str, at: &str, big_endian: bool) -> String {
    let mut out = String::from("(");
    for j in 0..4 {
        let q = format!("({at}) + {}", 2 * j + 1);
        let b = format!(
            "((instr({HEX}, substr({h}, {q}, 1)) - 1) * 16 + instr({HEX}, substr({h}, {q} + 1, 1)) - 1)"
        );
        if j > 0 {
            out.push_str(" | ");
        }
        out.push_str(&format!("({b} << {})", if big_endian { 8 * (3 - j) } else { 8 * j }));
    }
    out.push(')');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(p: P<'_>) -> String {
        let mut w = SqlWriter::new();
        p(&mut w).unwrap();
        w.into_text().unwrap()
    }

    fn x<'a>() -> P<'a> {
        pc(|w: &mut SqlWriter| {
            w.append("x");
            Ok(())
        })
    }

    #[test]
    fn the_walks_name_their_constants() {
        let md5 = text(md5(x()));
        assert!(md5.starts_with("(WITH RECURSIVE __in(v) AS (SELECT x), __m(h) AS (SELECT hex(CAST(__in.v AS BLOB)) || '80'"));
        // MD5 pads its bit length little-endian
        assert!(md5.contains("printf('%02X%02X%02X%02X%02X%02X%02X%02X', (((length(hex(CAST(__in.v AS BLOB))) / 2) * 8) >> 0) & 255"));
        assert!(md5.contains("'[3614090360,3905402710,"), "{md5}");
        assert!(md5.contains("__r(k, i, a, b, c, d, a0, b0, c0, d0) AS (SELECT 0, 0, 1732584193, 4023233417, 2562383102, 271733878, 1732584193"));
        assert!(md5.ends_with("printf('%02x%02x%02x%02x', r.d0 & 255, (r.d0 >> 8) & 255, (r.d0 >> 16) & 255, (r.d0 >> 24) & 255) FROM __r AS r WHERE r.k = (SELECT count(*) FROM __c) AND EXISTS (SELECT 1 FROM __m))"));
        let sha1 = text(sha1(x()));
        assert!(sha1.contains("printf('%016X', ((length(hex(CAST(__in.v AS BLOB))) / 2) * 8))"));
        assert!(sha1.contains("CASE WHEN __w.j < 16 THEN"));
        assert!(sha1.ends_with("printf('%08x', r.e0) FROM __r AS r WHERE r.k = (SELECT count(*) FROM __c) AND EXISTS (SELECT 1 FROM __m))"));
        let sha256 = text(sha256(x()));
        assert!(sha256.contains("'[1116352408,1899447441,"));
        assert!(sha256.contains("(((((r.e) >> 6) | ((r.e) << 26)) & 4294967295)"));
    }
}
