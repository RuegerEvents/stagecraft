//! Fuzzy matching for the command palette: every query word must appear as a
//! subsequence of the name (or, weaker, the summary). Word starts and runs score high.

/// Score of `query` against `text`; `None` if a character is missing.
pub fn subsequence(query: &str, text: &str) -> Option<i32> {
    let q: Vec<char> = query.to_lowercase().chars().collect();
    let s: Vec<char> = text.to_lowercase().chars().collect();
    if q.is_empty() {
        return Some(0);
    }
    let mut score = 0;
    let mut qi = 0;
    let mut last: Option<usize> = None;
    for (i, c) in s.iter().enumerate() {
        if qi < q.len() && *c == q[qi] {
            score += 10;
            let word_start = i == 0 || matches!(s[i - 1], '.' | '_' | ' ' | '-');
            if word_start {
                score += 18;
            }
            match last {
                Some(l) if l + 1 == i => score += 15,
                Some(l) => score -= ((i - l - 1) as i32).min(8),
                None => score -= (i as i32).min(8),
            }
            last = Some(i);
            qi += 1;
        }
    }
    (qi == q.len()).then_some(score)
}

/// Score of a command for a query of space-separated words.
pub fn score(query: &str, name: &str, summary: &str) -> Option<i32> {
    let mut total = 0;
    for word in query.split_whitespace() {
        let n = subsequence(word, name);
        let s = subsequence(word, summary).map(|v| v / 3);
        total += match (n, s) {
            (Some(a), Some(b)) => a.max(b),
            (Some(a), None) => a,
            (None, Some(b)) => b,
            (None, None) => return None,
        };
    }
    if name.starts_with(query.trim()) {
        total += 40;
    }
    Some(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_subsequences_and_ranks_word_starts() {
        assert!(subsequence("lc", "layer.create").is_some());
        assert!(subsequence("xz", "layer.create").is_none());
        let good = subsequence("lacr", "layer.create").unwrap();
        let worse = subsequence("lacr", "selection.clear").unwrap_or(i32::MIN);
        assert!(good > worse);
    }

    #[test]
    fn words_can_come_from_the_summary() {
        let names = [
            ("layer.create", "Create a spatial layer"),
            ("object.attach", "Attach objects to a parent (fixture to truss)"),
            ("ui.zoom_to_fit", "Fit the drawing"),
        ];
        let mut ranked: Vec<_> =
            names.iter().filter_map(|(n, s)| score("layer create", n, s).map(|v| (v, *n))).collect();
        ranked.sort_by_key(|r| std::cmp::Reverse(r.0));
        assert_eq!(ranked[0].1, "layer.create");
        assert!(score("truss", "object.attach", names[1].1).is_some());
        assert!(score("zoom fit", "ui.zoom_to_fit", "").unwrap() > score("zoom fit", "layer.create", "").unwrap_or(0));
    }
}
