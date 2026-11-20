//! Voice-friendly sibling selection in review feedback (the reference's `parseSelection` / `stripSelection`):
//! only a *leading* clause picks numbers ("okay, three and six, more playful" / "#2, #5 - bolder"), so "make it
//! 2 columns" picks nothing. Hand-written instead of the reference's regexes (no regex dependency here).

const NUMBER_WORDS: [(&str, u32); 20] = [
    ("one", 1), ("first", 1), ("two", 2), ("second", 2), ("three", 3), ("third", 3), ("four", 4), ("fourth", 4),
    ("five", 5), ("fifth", 5), ("six", 6), ("sixth", 6), ("seven", 7), ("seventh", 7), ("eight", 8), ("eighth", 8),
    ("nine", 9), ("ninth", 9), ("ten", 10), ("tenth", 10),
];
const FILLERS: [&str; 7] = ["ok", "okay", "so", "alright", "right", "um", "uh"];
const SEPARATORS: [&str; 5] = ["and", "plus", "or", "&", "+"];

/// One lexical token with its byte range.
#[derive(Debug)]
struct Tok<'a> {
    word: &'a str,
    end: usize,
}

/// Words (letters/digits/'#'/'&'/'+') and punctuation as separate tokens.
fn tokens(text: &str) -> Vec<Tok<'_>> {
    let mut out = Vec::new();
    let mut it = text.char_indices().peekable();
    while let Some((i, c)) = it.next() {
        if c.is_whitespace() {
            continue;
        }
        if c.is_alphanumeric() || c == '\'' {
            let mut end = i + c.len_utf8();
            while let Some(&(j, d)) = it.peek() {
                if d.is_alphanumeric() || d == '\'' {
                    end = j + d.len_utf8();
                    it.next();
                } else {
                    break;
                }
            }
            out.push(Tok { word: &text[i..end], end });
        } else {
            out.push(Tok { word: &text[i..i + c.len_utf8()], end: i + c.len_utf8() });
        }
    }
    out
}

fn number(word: &str) -> Option<u32> {
    let w = word.to_ascii_lowercase();
    if !w.is_empty() && w.len() <= 2 && w.bytes().all(|b| b.is_ascii_digit()) {
        return w.parse().ok();
    }
    NUMBER_WORDS.iter().find(|(n, _)| *n == w).map(|(_, v)| *v)
}

fn is_prefix(word: &str) -> bool {
    matches!(word.to_ascii_lowercase().as_str(), "number" | "numbers" | "no" | "nos" | "no." | "nos." | "#")
}

/// The leading selection clause: its numbers (in order, any value) and the byte offset where it ends
/// (trailing `, . : ; -` included). None when the text doesn't start with one.
fn lead(text: &str) -> Option<(Vec<u32>, usize)> {
    let toks = tokens(text);
    let mut i = 0;
    // Fillers, each followed by commas/spaces: "okay, so".
    while i < toks.len() && FILLERS.contains(&toks[i].word.to_ascii_lowercase().as_str()) {
        i += 1;
        while i < toks.len() && toks[i].word == "," {
            i += 1;
        }
    }
    let mut nums = Vec::new();
    let mut end = 0;
    loop {
        let mut j = i;
        while j < toks.len() && is_prefix(toks[j].word) {
            j += 1;
        }
        let Some(n) = toks.get(j).and_then(|t| number(t.word)) else { break };
        // "#" only prefixes a number; a word number must not run into letters ("tenth" is fine, "ones" isn't).
        nums.push(n);
        end = toks[j].end;
        i = j + 1;
        // A separator (", & + and plus or") may follow; then another number must come.
        let mut k = i;
        while k < toks.len() && (toks[k].word == "," || SEPARATORS.contains(&toks[k].word.to_ascii_lowercase().as_str())) {
            k += 1;
        }
        let next_is_number = {
            let mut m = k;
            while m < toks.len() && is_prefix(toks[m].word) {
                m += 1;
            }
            toks.get(m).and_then(|t| number(t.word)).is_some()
        };
        if !next_is_number {
            break;
        }
        i = k;
    }
    if nums.is_empty() {
        return None;
    }
    // Trailing punctuation of the clause.
    let rest = &text[end..];
    let skip = rest.len() - rest.trim_start_matches(|c: char| c.is_whitespace() || ",.:;-".contains(c)).len();
    Some((nums, end + skip))
}

/// Sibling numbers picked by the leading clause, 1-based, ≤ `max`, without repeats, in order.
pub fn parse_selection(text: &str, max: usize) -> Vec<usize> {
    let mut out = Vec::new();
    for n in lead(text).map(|l| l.0).unwrap_or_default() {
        let n = n as usize;
        if (1..=max).contains(&n) && !out.contains(&n) {
            out.push(n);
        }
    }
    out
}

/// The feedback without the leading selection clause and a "do another round (on these) (with)" phrase.
pub fn strip_selection(text: &str) -> String {
    let rest = match lead(text) {
        Some((_, end)) => &text[end..],
        None => text,
    };
    let rest = strip_round(rest.trim_start());
    rest.trim().trim_start_matches(|c: char| ",.:;-".contains(c)).trim_start().to_string()
}

/// Drops a leading "(let's|please|and) (do|try|run|give me) (another|one more|a new|new|a fresh) round (on|of|for
/// these|those|them) (with|where|that)".
fn strip_round(text: &str) -> &str {
    let lower = text.to_ascii_lowercase();
    let mut pos = 0;
    let eat = |pos: &mut usize, words: &[&str]| -> bool {
        let rest = &lower[*pos..];
        for w in words {
            if let Some(after) = rest.strip_prefix(w)
                && (after.is_empty() || after.starts_with(|c: char| c.is_whitespace() || ",.:;-".contains(c)))
            {
                *pos += w.len();
                *pos += lower[*pos..].len() - lower[*pos..].trim_start().len();
                return true;
            }
        }
        false
    };
    while eat(&mut pos, &["let's", "lets", "please", "and"]) {}
    eat(&mut pos, &["do", "try", "run", "give me"]);
    if !eat(&mut pos, &["another", "one more", "a new", "new", "a fresh"]) || !eat(&mut pos, &["round"]) {
        return text; // no "… round": keep everything
    }
    if eat(&mut pos, &["on", "of", "for"]) {
        eat(&mut pos, &["these", "those", "them"]);
    }
    eat(&mut pos, &["with", "where", "that"]);
    let rest = &text[pos..];
    rest.trim_start_matches(|c: char| c.is_whitespace() || ",.:;-".contains(c))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_only_the_leading_clause() {
        assert_eq!(parse_selection("3 and 6, more playful", 8), [3, 6]);
        assert_eq!(parse_selection("okay three and six, do another round", 8), [3, 6]);
        assert_eq!(parse_selection("#2, #5 - bolder type", 8), [2, 5]);
        assert_eq!(parse_selection("numbers 1 & 4: darker", 8), [1, 4]);
        assert_eq!(parse_selection("the second one", 8), Vec::<usize>::new());
        assert_eq!(parse_selection("make it 2 columns", 8), Vec::<usize>::new());
        assert_eq!(parse_selection("9 and 2", 5), [2]);
        assert_eq!(parse_selection("first or third", 5), [1, 3]);
    }

    #[test]
    fn strips_the_clause_and_round_filler() {
        assert_eq!(strip_selection("3 and 6, more playful"), "more playful");
        assert_eq!(strip_selection("okay three and six, do another round"), "");
        assert_eq!(strip_selection("2, let's do another round on these with bigger headings"), "bigger headings");
        assert_eq!(strip_selection("make it 2 columns"), "make it 2 columns");
        assert_eq!(strip_selection("another round, warmer colours"), "warmer colours");
    }
}
