use std::fmt;

/// Something odd about an imported file. Import never fails for these.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Warning {
    /// Where: `Layer "Rig" / Truss "T1"`, `archive`, `AUXData` …
    pub context: String,
    pub message: String,
}

impl Warning {
    pub fn new(context: impl Into<String>, message: impl Into<String>) -> Self {
        Warning { context: context.into(), message: message.into() }
    }
}

impl fmt::Display for Warning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.context.is_empty() {
            write!(f, "{}", self.message)
        } else {
            write!(f, "{}: {}", self.context, self.message)
        }
    }
}

/// Collapse a message repeated more than `max_each` times (with different
/// contexts) into one warning that names the first context and the count.
/// Order follows first occurrence.
pub fn collapse(warnings: Vec<Warning>, max_each: usize) -> Vec<Warning> {
    use std::collections::HashMap;
    let mut counts: HashMap<String, usize> = HashMap::new();
    for w in &warnings {
        *counts.entry(w.message.clone()).or_default() += 1;
    }
    let mut emitted: HashMap<String, bool> = HashMap::new();
    let mut out = Vec::new();
    for w in warnings {
        let n = counts[&w.message];
        if n <= max_each {
            out.push(w);
        } else if !emitted.contains_key(&w.message) {
            emitted.insert(w.message.clone(), true);
            out.push(Warning { message: format!("{} ({n}×, first here)", w.message), context: w.context });
        }
    }
    out
}
