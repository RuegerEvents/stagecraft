//! DMX patch: footprints from the fixture's GDTF mode, the address ranges a fixture
//! takes (one address per DMX break), conflicts, and free ranges for auto-patching.
//!
//! Addresses are absolute and 1-based (universe 2 address 1 = 513).

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use crate::fixture_type::{FixtureType, ModeInfo};
use crate::model::{Document, Id, Object, ObjectKind, format_address};

pub const UNIVERSE: u32 = 512;

/// The fixture type of a fixture, if it has one in the library.
pub fn fixture_type<'d>(doc: &'d Document, o: &Object) -> Option<&'d FixtureType> {
    let ObjectKind::Fixture { gdtf: Some(k), .. } = &o.kind else { return None };
    doc.fixture_types.get(k)
}

/// The fixture's mode in its type (named, else the first).
pub fn mode<'d>(doc: &'d Document, o: &Object) -> Option<&'d ModeInfo> {
    let ObjectKind::Fixture { mode, .. } = &o.kind else { return None };
    fixture_type(doc, o)?.mode_or_first(mode.as_deref())
}

/// Slots per break. Fixtures without a type (or an unknown mode) take one slot.
pub fn footprint(doc: &Document, o: &Object) -> Vec<u32> {
    match mode(doc, o) {
        Some(m) if !m.footprint.is_empty() => m.footprint.iter().map(|b| b.slots.max(1)).collect(),
        _ => vec![1],
    }
}

/// The patch addresses of a fixture: `address` for the first break, then
/// `extra_addresses`. `None` where a break is not patched.
pub fn addresses(o: &Object) -> Vec<Option<u32>> {
    let ObjectKind::Fixture { address, extra_addresses, .. } = &o.kind else { return Vec::new() };
    let mut v = vec![*address];
    v.extend(extra_addresses.iter().map(|a| (*a != 0).then_some(*a)));
    v
}

/// One patched break: absolute first and last slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Range {
    pub fixture: Id,
    /// Index of the break in the mode's footprint (0 = first).
    pub part: usize,
    pub first: u32,
    pub last: u32,
}

/// Patched ranges of one fixture.
pub fn ranges(doc: &Document, o: &Object) -> Vec<Range> {
    let fp = footprint(doc, o);
    addresses(o)
        .into_iter()
        .zip(fp)
        .enumerate()
        .filter_map(|(part, (a, n))| a.map(|a| Range { fixture: o.id, part, first: a, last: a + n - 1 }))
        .collect()
}

/// `1.001`, or `1.001 / 1.101` for several breaks; empty when unpatched.
pub fn address_text(o: &Object) -> String {
    let v = addresses(o);
    if v.iter().all(Option::is_none) {
        return String::new();
    }
    v.iter().map(|a| a.map(format_address).unwrap_or_else(|| "–".into())).collect::<Vec<_>>().join(" / ")
}

/// Why a fixture's patch is wrong.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Problem {
    /// Overlaps another fixture's range.
    Overlap { with: Id },
    /// A break runs past the end of its universe.
    CrossesUniverse { part: usize },
}

/// Every patch problem, per fixture. Overlaps are reported on both fixtures.
pub fn conflicts(doc: &Document) -> BTreeMap<Id, Vec<Problem>> {
    let mut all: Vec<Range> = doc.objects.values().flat_map(|o| ranges(doc, o)).collect();
    all.sort_by_key(|r| (r.first, r.last));
    let mut out: BTreeMap<Id, Vec<Problem>> = BTreeMap::new();
    let mut add = |id: Id, p: Problem| {
        let v = out.entry(id).or_default();
        if !v.contains(&p) {
            v.push(p);
        }
    };
    for (i, r) in all.iter().enumerate() {
        if (r.first - 1) / UNIVERSE != (r.last - 1) / UNIVERSE {
            add(r.fixture, Problem::CrossesUniverse { part: r.part });
        }
        for s in &all[i + 1..] {
            if s.first > r.last {
                break;
            }
            // Two breaks of one fixture on the same slots count as well.
            add(r.fixture, Problem::Overlap { with: s.fixture });
            if s.fixture != r.fixture {
                add(s.fixture, Problem::Overlap { with: r.fixture });
            }
        }
    }
    out
}

/// Ids of fixtures with a patch problem.
pub fn conflicting(doc: &Document) -> BTreeSet<Id> {
    conflicts(doc).into_keys().collect()
}

/// Slots taken by fixtures not in `except`, as sorted, merged ranges.
pub fn used(doc: &Document, except: &BTreeSet<Id>) -> Vec<(u32, u32)> {
    let mut v: Vec<(u32, u32)> = doc
        .objects
        .values()
        .filter(|o| !except.contains(&o.id))
        .flat_map(|o| ranges(doc, o))
        .map(|r| (r.first, r.last))
        .collect();
    v.sort();
    let mut merged: Vec<(u32, u32)> = Vec::new();
    for (a, b) in v {
        match merged.last_mut() {
            Some(last) if a <= last.1 + 1 => last.1 = last.1.max(b),
            _ => merged.push((a, b)),
        }
    }
    merged
}

/// The first address ≥ `from` where `n` slots fit without touching `used` or
/// crossing a universe boundary. `used` is sorted and merged.
pub fn next_free(used: &[(u32, u32)], from: u32, n: u32) -> Option<u32> {
    let n = n.clamp(1, UNIVERSE);
    let mut a = from.max(1);
    for _ in 0..100_000 {
        // Keep the block inside one universe.
        let start_u = (a - 1) / UNIVERSE;
        if (a + n - 2) / UNIVERSE != start_u {
            a = (start_u + 1) * UNIVERSE + 1;
        }
        let end = a + n - 1;
        match used.iter().find(|(f, l)| *f <= end && *l >= a) {
            Some((_, l)) => a = l + 1,
            None => return Some(a),
        }
        if a > 32_768 * UNIVERSE {
            return None;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn free_ranges_skip_used_and_universe_ends() {
        let used = vec![(1, 10), (20, 30)];
        assert_eq!(next_free(&used, 1, 5), Some(11));
        assert_eq!(next_free(&used, 1, 10), Some(31));
        assert_eq!(next_free(&[], 510, 4), Some(513));
        assert_eq!(next_free(&[], 509, 4), Some(509));
    }
}
