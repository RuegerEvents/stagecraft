//! Absolute DMX addresses and footprints of a mode, with `GeometryReference`
//! instancing expanded.
//!
//! A channel whose geometry lies in the mode's own tree keeps its offsets. A channel
//! whose geometry lies in a template (a top-level geometry other than the mode's
//! root) is repeated once per `GeometryReference` to that template in the mode's tree,
//! at `Break.DMXOffset + Offset − 1` in the break the reference gives. `DMXBreak =
//! "Overwrite"` takes the reference's last `<Break>`.

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::Warning;
use crate::types::*;

/// One DMX channel at its absolute place (relative to the fixture's patch address).
#[derive(Debug, Clone, PartialEq)]
pub struct PatchedChannel {
    /// Index into [`DmxMode::channels`].
    pub channel: usize,
    /// DMX break (1-based).
    pub dmx_break: u32,
    /// Slots within the break, most significant first; 1-based, 1 = patch address.
    pub offsets: Vec<u32>,
    /// The geometry instance: the reference name for templated channels, else the
    /// channel's geometry.
    pub geometry: String,
    /// The geometry named in the `<DMXChannel>`.
    pub source_geometry: String,
    pub attribute: String,
}

impl PatchedChannel {
    /// `<instance geometry>_<attribute>`.
    pub fn name(&self) -> String {
        format!("{}_{}", self.geometry, self.attribute)
    }
}

/// A mode with its channels placed.
#[derive(Debug, Clone, Default)]
pub struct ResolvedMode {
    pub name: String,
    /// Channels with addresses, in mode order (instances of a templated channel in
    /// reference order).
    pub channels: Vec<PatchedChannel>,
    /// Virtual channels (no offsets), instanced like the others.
    pub virtual_channels: Vec<PatchedChannel>,
    /// Number of slots per break: the highest slot used. A fixture patched at
    /// address `a` uses `a .. a + footprint − 1` in that break.
    pub footprint: BTreeMap<u32, u32>,
    pub warnings: Vec<Warning>,
}

impl ResolvedMode {
    /// Footprint of break 1 (0 if the mode uses no slots there).
    pub fn footprint_break1(&self) -> u32 {
        self.footprint.get(&1).copied().unwrap_or(0)
    }

    /// Sum over all breaks.
    pub fn total_footprint(&self) -> u32 {
        self.footprint.values().sum()
    }
}

struct RefInstance<'a> {
    name: &'a str,
    template: &'a str,
    breaks: &'a [Break],
}

/// Places every channel of `mode`.
pub fn resolve_mode(ft: &FixtureType, mode: &DmxMode) -> ResolvedMode {
    let mut out = ResolvedMode { name: mode.name.clone(), ..ResolvedMode::default() };
    let ctx = format!("DMXMode {}", mode.name);
    let root = ft.top_geometry(&mode.geometry).or_else(|| {
        let first = ft.geometries.first();
        if let Some(f) = first {
            out.warnings
                .push(Warning::new(&ctx, format!("root geometry \"{}\" missing; using \"{}\"", mode.geometry, f.name)));
        }
        first
    });

    // Names in the mode's own tree, and the references in it (nested through templates).
    let mut direct: HashSet<&str> = HashSet::new();
    let mut refs: Vec<RefInstance> = Vec::new();
    if let Some(root) = root {
        collect(ft, root, &mut direct, &mut refs, 0, &mut HashSet::new());
    }
    // Template subtree names.
    let mut template_names: HashMap<&str, HashSet<&str>> = HashMap::new();
    for t in &ft.geometries {
        let mut names = HashSet::new();
        t.walk(&mut |g| {
            names.insert(g.name.as_str());
        });
        template_names.insert(t.name.as_str(), names);
    }

    let mut unknown: BTreeMap<&str, usize> = BTreeMap::new();
    let mut missing_break = 0usize;
    for (i, ch) in mode.channels.iter().enumerate() {
        let attribute = ch.attribute().to_string();
        let instancing: Vec<&RefInstance> = refs
            .iter()
            .filter(|r| template_names.get(r.template).is_some_and(|n| n.contains(ch.geometry.as_str())))
            .collect();
        let in_direct = direct.contains(ch.geometry.as_str());
        let mut place = |geometry: String, dmx_break: u32, base: u32| {
            let pc = PatchedChannel {
                channel: i,
                dmx_break,
                offsets: ch.offsets.iter().map(|o| base + o - 1).collect(),
                geometry,
                source_geometry: ch.geometry.clone(),
                attribute: attribute.clone(),
            };
            if pc.offsets.is_empty() {
                out.virtual_channels.push(pc);
            } else {
                out.channels.push(pc);
            }
        };
        if !instancing.is_empty() && !in_direct {
            for r in instancing {
                let brk = match ch.dmx_break {
                    DmxBreak::Overwrite => r.breaks.last().copied(),
                    DmxBreak::Number(b) => r.breaks.iter().find(|x| x.dmx_break == b).copied(),
                };
                let brk = brk.unwrap_or_else(|| {
                    if !ch.is_virtual() {
                        missing_break += 1;
                    }
                    Break {
                        dmx_break: match ch.dmx_break {
                            DmxBreak::Number(b) => b,
                            DmxBreak::Overwrite => 1,
                        },
                        dmx_offset: 1,
                    }
                });
                place(r.name.to_string(), brk.dmx_break.max(1), brk.dmx_offset.max(1));
            }
        } else {
            if !in_direct {
                *unknown.entry(ch.geometry.as_str()).or_default() += 1;
            }
            let b = match ch.dmx_break {
                DmxBreak::Number(b) => b,
                DmxBreak::Overwrite => {
                    out.warnings
                        .push(Warning::new(&ctx, format!("{}: Overwrite break outside a reference", ch.name())));
                    1
                }
            };
            place(ch.geometry.clone(), b.max(1), 1);
        }
    }
    for (g, n) in unknown {
        out.warnings.push(Warning::new(
            &ctx,
            format!("{n} channel(s) on geometry \"{g}\", which is not in the mode's tree; placed as written"),
        ));
    }
    if missing_break > 0 {
        out.warnings.push(Warning::new(
            &ctx,
            format!("{missing_break} templated channel instance(s) without a matching <Break>; offset 1 assumed"),
        ));
    }

    let mut used: HashMap<(u32, u32), usize> = HashMap::new();
    for c in &out.channels {
        let top = out.footprint.entry(c.dmx_break).or_default();
        for &o in &c.offsets {
            *top = (*top).max(o);
            *used.entry((c.dmx_break, o)).or_default() += 1;
        }
    }
    let clashes = used.values().filter(|n| **n > 1).count();
    if clashes > 0 {
        out.warnings.push(Warning::new(&ctx, format!("{clashes} slot(s) used by more than one channel")));
    }
    out
}

fn collect<'a>(
    ft: &'a FixtureType,
    g: &'a Geometry,
    direct: &mut HashSet<&'a str>,
    refs: &mut Vec<RefInstance<'a>>,
    depth: usize,
    stack: &mut HashSet<&'a str>,
) {
    if depth > 64 {
        return;
    }
    direct.insert(g.name.as_str());
    if let GeometryKind::Reference { geometry, breaks } = &g.kind {
        refs.push(RefInstance { name: &g.name, template: geometry, breaks });
        // References inside a template are instanced through it; their addresses are
        // not offset again (GDTF has no nested offsets), but their names count.
        if let Some(t) = ft.top_geometry(geometry)
            && stack.insert(geometry.as_str())
        {
            for c in &t.children {
                collect_nested_refs(ft, c, refs, depth + 1, stack);
            }
            stack.remove(geometry.as_str());
        }
    }
    for c in &g.children {
        collect(ft, c, direct, refs, depth + 1, stack);
    }
}

fn collect_nested_refs<'a>(
    ft: &'a FixtureType,
    g: &'a Geometry,
    refs: &mut Vec<RefInstance<'a>>,
    depth: usize,
    stack: &mut HashSet<&'a str>,
) {
    if depth > 64 {
        return;
    }
    if let GeometryKind::Reference { geometry, breaks } = &g.kind {
        refs.push(RefInstance { name: &g.name, template: geometry, breaks });
        if let Some(t) = ft.top_geometry(geometry)
            && stack.insert(geometry.as_str())
        {
            for c in &t.children {
                collect_nested_refs(ft, c, refs, depth + 1, stack);
            }
            stack.remove(geometry.as_str());
        }
    }
    for c in &g.children {
        collect_nested_refs(ft, c, refs, depth + 1, stack);
    }
}
