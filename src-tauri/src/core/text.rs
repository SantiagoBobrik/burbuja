//! Transcript text processing: Whisper non-speech markers and user replacements.

/// `[...]` markers Whisper emits for non-speech (matched by prefix, case-insensitive).
const BRACKET_MARKERS: [&str; 6] = [
    "blank_audio",
    "music",
    "sound",
    "noise",
    "silence",
    "inaudible",
];
/// `(...)` markers, matched exactly (case-insensitive).
const PAREN_MARKERS: [&str; 4] = ["silence", "music", "inaudible", "blank audio"];

/// Strips Whisper's non-speech markers and collapses the whitespace (including the line breaks
/// it adds between segments).
pub fn clean_transcript(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find(['[', '(']) {
        out.push_str(&rest[..i]);
        let (open, close) = if rest[i..].starts_with('[') {
            ('[', ']')
        } else {
            ('(', ')')
        };
        let after = &rest[i + 1..];
        match after.find(close) {
            Some(j) if is_marker(open, &after[..j]) => {
                out.push(' ');
                rest = &after[j + 1..];
            }
            _ => {
                out.push(open);
                rest = after;
            }
        }
    }
    out.push_str(rest);
    collapse_whitespace(&out)
}

fn is_marker(open: char, inner: &str) -> bool {
    let inner = inner.to_lowercase();
    if open == '[' {
        BRACKET_MARKERS.iter().any(|m| inner.starts_with(m))
    } else {
        PAREN_MARKERS.contains(&inner.as_str())
    }
}

pub fn collapse_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A user replacement from the Dictionary page, e.g. `post gres` → `PostgreSQL`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct Replacement {
    pub from: String,
    pub to: String,
}

/// Applies replacements in order, as whole-word, case-insensitive matches.
pub fn apply_replacements(text: &str, replacements: &[Replacement]) -> String {
    replacements
        .iter()
        .filter(|r| !r.from.trim().is_empty())
        .fold(text.to_string(), |acc, r| {
            replace_whole_word(&acc, r.from.trim(), &r.to)
        })
}

fn replace_whole_word(text: &str, from: &str, to: &str) -> String {
    let hay: Vec<char> = text.chars().collect();
    let needle: Vec<char> = from.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < hay.len() {
        let end = i + needle.len();
        let boundary_before = i == 0 || !is_word_char(hay[i - 1]);
        let boundary_after = end >= hay.len() || !is_word_char(hay[end]);
        if end <= hay.len()
            && boundary_before
            && boundary_after
            && hay[i..end]
                .iter()
                .zip(&needle)
                .all(|(a, b)| chars_eq_ignore_case(*a, *b))
        {
            out.push_str(to);
            i = end;
        } else {
            out.push(hay[i]);
            i += 1;
        }
    }
    out
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn chars_eq_ignore_case(a: char, b: char) -> bool {
    a == b || a.to_lowercase().eq(b.to_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rep(from: &str, to: &str) -> Replacement {
        Replacement {
            from: from.into(),
            to: to.into(),
        }
    }

    #[test]
    fn strips_bracket_markers() {
        assert_eq!(clean_transcript("[BLANK_AUDIO]"), "");
        assert_eq!(
            clean_transcript("Hello [Music playing] world"),
            "Hello world"
        );
        assert_eq!(clean_transcript("[SOUND] ok [noise]"), "ok");
    }

    #[test]
    fn strips_paren_markers_exactly() {
        assert_eq!(clean_transcript("(music) Hi (silence)"), "Hi");
        assert_eq!(clean_transcript("(blank audio)"), "");
    }

    #[test]
    fn keeps_real_brackets_and_parens() {
        assert_eq!(clean_transcript("call f(x) and a[0]"), "call f(x) and a[0]");
        assert_eq!(
            clean_transcript("(music theory) is fun"),
            "(music theory) is fun"
        );
        assert_eq!(clean_transcript("unclosed [BLANK"), "unclosed [BLANK");
    }

    #[test]
    fn collapses_whitespace_and_segment_breaks() {
        assert_eq!(clean_transcript("  one\n two\t\tthree  "), "one two three");
    }

    #[test]
    fn replacement_is_whole_word_and_case_insensitive() {
        let r = [rep("post gres", "PostgreSQL")];
        assert_eq!(
            apply_replacements("I use Post Gres daily", &r),
            "I use PostgreSQL daily"
        );
        assert_eq!(apply_replacements("post gresql", &r), "post gresql");
    }

    #[test]
    fn replacement_handles_accents_and_order() {
        let r = [rep("cordoba", "Córdoba"), rep("Córdoba", "Córdoba (AR)")];
        assert_eq!(apply_replacements("de CORDOBA.", &r), "de Córdoba (AR).");
    }

    #[test]
    fn empty_replacement_is_ignored() {
        assert_eq!(apply_replacements("hello", &[rep("  ", "x")]), "hello");
    }
}
