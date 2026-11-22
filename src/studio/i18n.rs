//! The interface in another language, from a catalog of translations.
//!
//! The source keeps its English text; a translation is looked up as the
//! text is laid out (the hook in `vendor/epaint/src/text/translate.rs`), so
//! nothing that draws text needs to know about it. A catalog
//! (`locales/<lang>.tsv`) has one line per piece of text: the English, a
//! tab, the translation. English with `{}`-style holes (as in `format!`)
//! is a template: `{} layers` matches "12 layers", and the translation
//! says where each hole goes by its place in the English, `{0}`, `{1}`…
//! (`{0} katman`). Text a catalog doesn't have stays English.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Mutex, OnceLock};

/// The languages the interface can be shown in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Lang {
    #[default]
    English,
    Turkish,
}

impl Lang {
    pub const ALL: [Lang; 2] = [Lang::English, Lang::Turkish];

    /// How it's kept in the settings file.
    pub fn id(self) -> &'static str {
        match self {
            Lang::English => "en",
            Lang::Turkish => "tr",
        }
    }

    pub fn from_id(id: &str) -> Option<Lang> {
        Lang::ALL.into_iter().find(|l| l.id() == id)
    }

    /// Its name in itself (never translated: it's how it's found).
    pub fn label(self) -> &'static str {
        match self {
            Lang::English => "English",
            Lang::Turkish => "Türkçe",
        }
    }

    fn catalog_text(self) -> &'static str {
        match self {
            Lang::English => "",
            Lang::Turkish => include_str!("../../locales/tr.tsv"),
        }
    }
}

static CURRENT: AtomicU8 = AtomicU8::new(0);

/// Show the interface in `lang` from now on.
pub fn set(lang: Lang) {
    let n = Lang::ALL.iter().position(|l| *l == lang).unwrap_or(0) as u8;
    if CURRENT.swap(n, Ordering::Relaxed) != n
        && let Ok(mut m) = memo().lock()
    {
        m.clear();
    }
    eframe::egui::epaint::text::translate::set_translator(match lang {
        Lang::English => None,
        _ => Some(translate),
    });
}

pub fn current() -> Lang {
    Lang::ALL[CURRENT.load(Ordering::Relaxed) as usize % Lang::ALL.len()]
}

/// `text` in the interface's language (for text that isn't laid out by
/// egui: the window title, file dialogs).
pub fn t(text: &str) -> String {
    translate(text).unwrap_or_else(|| text.to_string())
}

/// One piece of English with holes.
struct Template {
    /// Literal text and holes, alternating as they come.
    english: Vec<Piece>,
    /// The translation: literal text, and holes by their place in `english`.
    out: Vec<Piece>,
    /// How much literal text it has: the most specific matches first.
    weight: usize,
}

#[derive(Debug, PartialEq)]
enum Piece {
    Text(String),
    Hole(usize),
}

struct Catalog {
    exact: HashMap<String, String>,
    templates: Vec<Template>,
}

fn catalog(lang: Lang) -> &'static Catalog {
    static TR: OnceLock<Catalog> = OnceLock::new();
    match lang {
        Lang::English => {
            static NONE: OnceLock<Catalog> = OnceLock::new();
            NONE.get_or_init(|| Catalog::parse(""))
        }
        Lang::Turkish => TR.get_or_init(|| Catalog::parse(lang.catalog_text())),
    }
}

fn memo() -> &'static Mutex<HashMap<String, Option<String>>> {
    static MEMO: OnceLock<Mutex<HashMap<String, Option<String>>>> = OnceLock::new();
    MEMO.get_or_init(Default::default)
}

/// The translator registered with epaint: memoized, since the same text is
/// laid out every frame.
fn translate(text: &str) -> Option<String> {
    let lang = current();
    if lang == Lang::English || text.trim().is_empty() {
        return None;
    }
    if let Ok(m) = memo().lock()
        && let Some(hit) = m.get(text)
    {
        return hit.clone();
    }
    let found = catalog(lang).lookup(text, 0);
    if let Ok(mut m) = memo().lock() {
        // (Text that keeps changing, a counter say, mustn't grow it forever.)
        if m.len() > 20_000 {
            m.clear();
        }
        m.insert(text.to_string(), found.clone());
    }
    found
}

/// Split a catalog field into text and holes. `{}`, `{name}`, `{:.1}`,
/// `{0}` are holes (numbered as they come unless `numbered`, when `{N}`
/// says which); `{{` and `}}` are braces.
fn pieces(s: &str, numbered: bool) -> Vec<Piece> {
    let mut out = Vec::new();
    let mut text = String::new();
    let mut holes = 0;
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' if chars.peek() == Some(&'{') => {
                chars.next();
                text.push('{');
            }
            '}' if chars.peek() == Some(&'}') => {
                chars.next();
                text.push('}');
            }
            '{' => {
                let mut inner = String::new();
                for d in chars.by_ref() {
                    if d == '}' {
                        break;
                    }
                    inner.push(d);
                }
                if !text.is_empty() {
                    out.push(Piece::Text(std::mem::take(&mut text)));
                }
                let n = if numbered {
                    inner.parse().unwrap_or(holes)
                } else {
                    holes
                };
                holes += 1;
                out.push(Piece::Hole(n));
            }
            _ => text.push(c),
        }
    }
    if !text.is_empty() {
        out.push(Piece::Text(text));
    }
    out
}

/// A field with `\t`, `\n` and `\\` written out.
fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some(o) => out.push(o),
            None => out.push('\\'),
        }
    }
    out
}

impl Catalog {
    fn parse(text: &str) -> Catalog {
        let mut exact = HashMap::new();
        let mut templates = Vec::new();
        for line in text.lines() {
            if line.starts_with('#') || line.trim().is_empty() {
                continue;
            }
            let Some((en, tr)) = line.split_once('\t') else {
                continue;
            };
            let (en, tr) = (unescape(en), unescape(tr));
            let english = pieces(&en, false);
            if !english.iter().any(|p| matches!(p, Piece::Hole(_))) {
                // (Braces written as `{{` in a template are plain here.)
                exact.insert(en, tr);
                continue;
            }
            // Two holes side by side can't be told apart, and a template
            // without words would match anything.
            let adjacent = english
                .windows(2)
                .any(|w| matches!(w, [Piece::Hole(_), Piece::Hole(_)]));
            let weight: usize = english
                .iter()
                .map(|p| match p {
                    Piece::Text(t) => t.chars().filter(|c| c.is_alphabetic()).count(),
                    Piece::Hole(_) => 0,
                })
                .sum();
            if adjacent || weight < 3 {
                continue;
            }
            templates.push(Template {
                english,
                out: pieces(&tr, true),
                weight,
            });
        }
        templates.sort_by(|a, b| b.weight.cmp(&a.weight));
        Catalog { exact, templates }
    }

    fn lookup(&self, text: &str, depth: u8) -> Option<String> {
        if let Some(t) = self.exact.get(text) {
            return Some(t.clone());
        }
        // (Leading and trailing spaces are kept as they are.)
        let trimmed = text.trim();
        if trimmed.len() != text.len()
            && let Some(t) = self.exact.get(trimmed)
        {
            let start = text.len() - text.trim_start().len();
            let end = text.trim_end().len();
            return Some(format!("{}{t}{}", &text[..start], &text[end..]));
        }
        if depth > 1 {
            return None;
        }
        self.templates
            .iter()
            .find_map(|t| t.fill(text, |h| self.lookup(h, depth + 1)))
    }
}

impl Template {
    /// The translation of `text` if it fits this template; what fills each
    /// hole is translated too when the catalog has it.
    fn fill(&self, text: &str, inner: impl Fn(&str) -> Option<String>) -> Option<String> {
        let mut holes: Vec<&str> = Vec::new();
        let mut at = 0;
        let n = self.english.len();
        for (i, p) in self.english.iter().enumerate() {
            match p {
                Piece::Text(lit) => {
                    if !text[at..].starts_with(lit.as_str()) {
                        return None;
                    }
                    at += lit.len();
                }
                Piece::Hole(_) => {
                    let end = match self.english.get(i + 1) {
                        // The last piece: the hole runs to the end.
                        None => text.len(),
                        Some(Piece::Text(lit)) if i + 2 == n => {
                            if !text.ends_with(lit.as_str()) || text.len() < at + lit.len() {
                                return None;
                            }
                            text.len() - lit.len()
                        }
                        Some(Piece::Text(lit)) => at + text[at..].find(lit.as_str())?,
                        Some(Piece::Hole(_)) => return None,
                    };
                    if end <= at {
                        return None;
                    }
                    holes.push(&text[at..end]);
                    at = end;
                }
            }
        }
        if at != text.len() {
            return None;
        }
        let mut out = String::new();
        for p in &self.out {
            match p {
                Piece::Text(t) => out.push_str(t),
                Piece::Hole(k) => {
                    let h = holes.get(*k)?;
                    match inner(h) {
                        Some(t) => out.push_str(&t),
                        None => out.push_str(h),
                    }
                }
            }
        }
        Some(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "# a comment\n\
        Open…\tAç…\n\
        untitled\tadsız\n\
        {} layers\t{0} katman\n\
        {} — ZigZag\t{0} — ZigZag\n\
        Moved {n} objects by {:.1} mm.\t{0} nesne {1} mm taşındı.\n\
        Matched {n} object(s) to {name}.\t{0} nesne {1} paletine eşlendi.\n\
        Two lines\\nhere\tİki satır\\nburada\n";

    #[test]
    fn exact_text_and_templates_are_translated() {
        let c = Catalog::parse(SAMPLE);
        assert_eq!(c.lookup("Open…", 0).as_deref(), Some("Aç…"));
        assert_eq!(c.lookup("  Open…", 0).as_deref(), Some("  Aç…"));
        assert_eq!(c.lookup("12 layers", 0).as_deref(), Some("12 katman"));
        assert_eq!(
            c.lookup("Moved 3 objects by 2.5 mm.", 0).as_deref(),
            Some("3 nesne 2.5 mm taşındı.")
        );
        assert_eq!(
            c.lookup("Matched 4 object(s) to Brother.", 0).as_deref(),
            Some("4 nesne Brother paletine eşlendi.")
        );
        assert_eq!(
            c.lookup("Two lines\nhere", 0).as_deref(),
            Some("İki satır\nburada")
        );
        // A hole's text is translated too when the catalog has it.
        assert_eq!(
            c.lookup("untitled — ZigZag", 0).as_deref(),
            Some("adsız — ZigZag")
        );
        assert_eq!(c.lookup("Something else", 0), None);
        assert_eq!(c.lookup("layers", 0), None);
    }

    #[test]
    fn templates_without_words_are_left_out() {
        let c = Catalog::parse("{} · {}\t{0} | {1}\n{}{} x\t{0}\n");
        assert!(c.templates.is_empty());
    }

    #[test]
    fn the_catalogs_read_whole() {
        for lang in Lang::ALL {
            let text = lang.catalog_text();
            let c = Catalog::parse(text);
            let lines = text
                .lines()
                .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
                .count();
            // Every line has a tab, and a template's holes are all in its
            // English.
            assert!(
                text.lines()
                    .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
                    .all(|l| l.contains('\t')),
                "{lang:?}"
            );
            for t in &c.templates {
                let holes = t
                    .english
                    .iter()
                    .filter(|p| matches!(p, Piece::Hole(_)))
                    .count();
                assert!(
                    t.out
                        .iter()
                        .all(|p| !matches!(p, Piece::Hole(k) if *k >= holes)),
                    "{lang:?}: a hole the English hasn't"
                );
            }
            assert!(c.exact.len() + c.templates.len() <= lines);
        }
    }
}
