//! The command line: a slim bar floating at the bottom of the canvas, as in CAD programs.
//! The line above the input says what the active tool expects next; typing completes any
//! command of either registry (`ui.*` included) and tool names. Enter runs it:
//!
//! - a tool name (`wall`) picks the tool;
//! - a command alone runs it when it takes no arguments, else opens its form in the palette;
//! - a command followed by a JSON object (`ui.zoom {"scale": 0.1}`) or `key=value` pairs
//!   (`layer.create name=Stage elevation=0`) runs it with those arguments.
//!
//! The last command and its answer stay above the prompt line.

use egui::{Key, Rect, Role, Ui, pos2, vec2};
use serde_json::{Map, Value, json};

use crate::app::Studio;
use crate::i18n::{t, tf};
use crate::state::{Tool, View};
use crate::theme;

#[derive(Clone, Debug, Default)]
pub struct CmdLine {
    pub text: String,
    /// The last command run from here and its answer.
    pub echo: Option<(String, bool)>,
    /// Highlighted completion.
    pub cursor: usize,
}

/// What the command line can complete to: a tool or a command (its ID and title).
#[derive(Clone, Debug, PartialEq)]
pub enum Completion {
    Tool(Tool),
    Command(&'static str, &'static str),
}

impl Completion {
    fn text(&self) -> String {
        match self {
            Completion::Tool(tool) => t(tool.key()).to_lowercase(),
            Completion::Command(name, _) => (*name).to_owned(),
        }
    }
}

/// Completions for what is typed, best first: tool names, then commands by title, alias
/// or ID (as the palette finds them). Nothing once a known command is followed by
/// arguments.
pub fn complete(text: &str) -> Vec<Completion> {
    let first = text.split_whitespace().next().unwrap_or("");
    if first.is_empty() || (crate::palette::is_command(first) && text.trim_start().contains(char::is_whitespace)) {
        return Vec::new();
    }
    let query = text.trim();
    let mut tools: Vec<(i32, Completion)> = Tool::ALL
        .iter()
        .filter_map(|tool| {
            let name = t(tool.key()).to_lowercase();
            crate::fuzzy::subsequence(query, &name)
                .filter(|_| name.starts_with(&query.to_lowercase()))
                .map(|sc| (sc + 40, Completion::Tool(*tool)))
        })
        .collect();
    tools.sort_by_key(|x| std::cmp::Reverse(x.0));
    // Base rows only: a completion is an ID one can type.
    let mut seen = Vec::new();
    let commands = crate::palette::search_commands(query, query.contains('.'), None)
        .into_iter()
        .filter_map(|(i, _)| {
            let m = &crate::command_meta::META[i];
            if m.id.starts_with("app.") || seen.contains(&m.id) {
                return None;
            }
            seen.push(m.id);
            let base = crate::command_meta::of(m.id).unwrap_or(m);
            Some(Completion::Command(m.id, base.title()))
        })
        .take(12);
    tools.into_iter().map(|x| x.1).chain(commands).take(8).collect()
}

/// `key=value` pairs as a JSON object: values that read as JSON stay JSON, the rest are text.
fn pairs(rest: &str) -> Option<Value> {
    let mut m = Map::new();
    for part in rest.split_whitespace() {
        let (k, v) = part.split_once('=')?;
        let v = serde_json::from_str(v).unwrap_or_else(|_| Value::String(v.to_owned()));
        m.insert(k.to_owned(), v);
    }
    Some(Value::Object(m))
}

/// What a typed line asks for.
#[derive(Debug, PartialEq)]
pub enum Parsed {
    Tool(Tool),
    Run(String, Value),
    /// A command that needs arguments: open its form.
    Form(String),
    Unknown(String),
    BadArgs(String),
}

pub fn parse(line: &str) -> Parsed {
    let line = line.trim();
    let (name, rest) = line.split_once(char::is_whitespace).map_or((line, ""), |(a, b)| (a, b.trim()));
    if rest.is_empty()
        && let Some(tool) = Tool::ALL.iter().find(|tool| t(tool.key()).eq_ignore_ascii_case(name))
    {
        return Parsed::Tool(*tool);
    }
    if !crate::palette::is_command(name) {
        return Parsed::Unknown(name.to_owned());
    }
    if rest.is_empty() {
        let needs = crate::schema_form::fields(&crate::palette::schema_of(name)).iter().any(|f| f.required);
        return if needs { Parsed::Form(name.to_owned()) } else { Parsed::Run(name.to_owned(), json!({})) };
    }
    let args = if rest.starts_with('{') { serde_json::from_str(rest).ok() } else { pairs(rest) };
    match args {
        Some(a) => Parsed::Run(name.to_owned(), a),
        None => Parsed::BadArgs(rest.to_owned()),
    }
}

fn submit(s: &mut Studio, line: &str) {
    let echo = |s: &mut Studio, text: String, err: bool| s.ui.cmdline.echo = Some((text, err));
    match parse(line) {
        Parsed::Tool(tool) => {
            let _ = s.run("ui.set_tool", json!({ "tool": tool }));
            echo(s, format!("› {}", t(tool.key())), false);
        }
        Parsed::Form(name) => {
            s.ui.palette.open();
            crate::palette::choose(s, &name);
            echo(s, format!("› {name} …"), false);
        }
        Parsed::Run(name, args) => match s.run(&name, args) {
            Ok(v) => {
                let short: String = serde_json::to_string(&v).unwrap_or_default().chars().take(90).collect();
                echo(s, format!("› {name}  →  {short}"), false);
            }
            Err(e) => echo(s, format!("› {name}: {}", e.message), true),
        },
        Parsed::Unknown(name) => echo(s, tf("cmdline.unknown", &[("name", &name)]), true),
        Parsed::BadArgs(rest) => echo(s, tf("cmdline.bad_args", &[("args", &rest)]), true),
    }
}

/// What the active tool expects next, for the prompt line.
fn prompt(s: &Studio) -> String {
    if s.ui.view == View::Sheets {
        return format!("{}: {}", t("view.sheets"), t("hint.sheets"));
    }
    format!("{}: {}", t(s.ui.tool.key()), crate::panels::hint(s))
}

const ROW: f32 = 18.0;
const INPUT: f32 = 24.0;

/// The bar at the bottom of `canvas`.
pub fn show(ui: &mut Ui, s: &mut Studio, canvas: Rect) {
    let c = theme::of(ui);
    // Clear of the axis icon in the bottom left corner.
    let width = (canvas.width() - 2.0 * 96.0).clamp(120.0, 640.0);
    // The prompt wraps to two lines and is cut with "…" after that (the whole text is
    // the tooltip); the echo of the last command takes one line.
    let wrap = |text: String, colour, rows: usize| {
        let mut job = egui::text::LayoutJob::single_section(text, egui::TextFormat::simple(theme::small(), colour));
        job.wrap = egui::text::TextWrapping {
            max_width: width - 16.0,
            max_rows: rows,
            break_anywhere: false,
            overflow_character: Some('…'),
        };
        ui.fonts_mut(|f| f.layout_job(job))
    };
    let full_prompt = prompt(s);
    let prompt_g = wrap(full_prompt.clone(), c.text, 2);
    let echo_g =
        s.ui.cmdline.echo.as_ref().map(|(text, err)| wrap(text.clone(), if *err { c.warning } else { c.text2 }, 1));
    let pad = (ROW - prompt_g.rows.first().map_or(12.0, |r| r.height())) / 2.0;
    let text_h = echo_g.as_ref().map_or(0.0, |g| g.size().y + 2.0) + prompt_g.size().y;
    let height = INPUT + (text_h + 2.0 * pad).max(ROW);
    let rect = Rect::from_min_size(
        pos2(canvas.center().x - width / 2.0, canvas.bottom() - height - 12.0),
        vec2(width, height),
    );
    if rect.top() < canvas.top() + 40.0 {
        return;
    }
    let p = ui.painter().clone();
    let history = Rect::from_min_max(rect.min, pos2(rect.right(), rect.bottom() - INPUT));
    p.rect_filled(history, egui::CornerRadius { nw: 3, ne: 3, sw: 0, se: 0 }, c.cmd_history);
    let mut y = history.top() + pad;
    if let Some(g) = echo_g {
        let h = g.size().y;
        p.galley(pos2(history.left() + 8.0, y), g, c.text2);
        y += h + 2.0;
    }
    let elided = prompt_g.elided;
    let prompt_rect = Rect::from_min_size(pos2(history.left() + 8.0, y), prompt_g.size());
    p.galley(prompt_rect.min, prompt_g, c.text);
    if elided {
        let r = ui.interact(prompt_rect, egui::Id::unique("command_line_prompt"), egui::Sense::hover());
        r.on_hover_text(full_prompt);
    }
    let input = Rect::from_min_max(pos2(rect.left(), rect.bottom() - INPUT), rect.max);
    p.rect_filled(input, egui::CornerRadius { nw: 0, ne: 0, sw: 3, se: 3 }, c.cmd_bg);
    p.rect_stroke(input, 3.0, egui::Stroke::new(1.0, c.cmd_border), egui::StrokeKind::Inside);
    p.text(pos2(input.left() + 9.0, input.center().y), egui::Align2::LEFT_CENTER, "›_", theme::body(), c.text2);

    let id = egui::Id::unique("command_line");
    let completions = complete(&s.ui.cmdline.text);
    let focused = ui.memory(|m| m.has_focus(id));
    let (down, up, tab) = if focused && !completions.is_empty() {
        ui.input_mut(|i| {
            (
                i.consume_key(egui::Modifiers::NONE, Key::ArrowDown),
                i.consume_key(egui::Modifiers::NONE, Key::ArrowUp),
                i.consume_key(egui::Modifiers::NONE, Key::Tab),
            )
        })
    } else {
        (false, false, false)
    };
    let cl = &mut s.ui.cmdline;
    if down {
        cl.cursor = (cl.cursor + 1).min(completions.len().saturating_sub(1));
    }
    if up {
        cl.cursor = cl.cursor.saturating_sub(1);
    }
    if tab && let Some(cmp) = completions.get(cl.cursor) {
        cl.text = format!("{} ", cmp.text());
        cl.cursor = 0;
        let mut state = egui::TextEdit::load_state(ui.ctx(), id).unwrap_or_default();
        let end = egui::text::CCursor::new(cl.text.chars().count());
        state.cursor.set_char_range(Some(egui::text::CCursorRange::one(end)));
        state.store(ui.ctx(), id);
    }
    let field = Rect::from_min_max(
        pos2(input.left() + 28.0, input.top() + 2.0),
        pos2(input.right() - 6.0, input.bottom() - 2.0),
    );
    let mut child =
        ui.new_child(egui::UiBuilder::new().max_rect(field).layout(egui::Layout::left_to_right(egui::Align::Center)));
    let r = child.add(
        egui::TextEdit::singleline(&mut cl.text)
            .id(id)
            .frame(egui::Frame::NONE)
            .hint_text(t("cmdline.hint"))
            .font(theme::body())
            .desired_width(field.width()),
    );
    crate::widgets::name(&r, Role::TextInput, "Command line");
    if r.changed() {
        cl.cursor = 0;
    }
    let escape = ui.input(|i| i.key_pressed(Key::Escape));
    if r.lost_focus() && escape {
        cl.text.clear();
    }
    let enter = r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
    if enter {
        let first = cl.text.split_whitespace().next().unwrap_or("");
        let typed_known = crate::palette::is_command(first)
            || Tool::ALL.iter().any(|tool| t(tool.key()).eq_ignore_ascii_case(cl.text.trim()));
        let line = match completions.get(cl.cursor) {
            // Enter on half-typed words or a title takes the highlighted completion.
            Some(cmp) if !typed_known && cmp.text() != cl.text.trim() => cmp.text(),
            _ => cl.text.clone(),
        };
        cl.text.clear();
        cl.cursor = 0;
        if !line.trim().is_empty() {
            submit(s, &line);
        }
        return;
    }
    if focused && !completions.is_empty() {
        list(ui, s, &completions, rect);
    }
}

/// The completion list above the bar.
fn list(ui: &mut Ui, s: &mut Studio, completions: &[Completion], bar: Rect) {
    let c = theme::of(ui);
    let h = ROW * completions.len() as f32 + 4.0;
    let rect = Rect::from_min_size(pos2(bar.left(), bar.top() - h - 2.0), vec2(bar.width(), h));
    let layer = egui::LayerId::new(egui::Order::Foreground, egui::Id::unique("command_line_list"));
    let p = ui.ctx().layer_painter(layer);
    p.rect_filled(rect, 3.0, c.list_bg);
    p.rect_stroke(rect, 3.0, egui::Stroke::new(1.0, c.cmd_border), egui::StrokeKind::Inside);
    let mut y = rect.top() + 2.0;
    for (i, cmp) in completions.iter().enumerate() {
        let row = Rect::from_min_size(pos2(rect.left() + 2.0, y), vec2(rect.width() - 4.0, ROW));
        if i == s.ui.cmdline.cursor {
            p.rect_filled(row, 2.0, c.list_row);
        }
        let (title, note) = match cmp {
            Completion::Tool(tool) => (t(tool.key()).to_owned(), t("cmdline.tool").to_owned()),
            Completion::Command(name, title) => ((*title).to_owned(), (*name).to_owned()),
        };
        let title_g = p.layout_no_wrap(title, theme::body(), c.text);
        let w = title_g.size().x;
        p.galley(pos2(row.left() + 6.0, row.center().y - title_g.size().y / 2.0), title_g, c.text);
        p.with_clip_rect(row).text(
            pos2(row.left() + 14.0 + w, row.center().y),
            egui::Align2::LEFT_CENTER,
            note,
            egui::FontId::monospace(10.0),
            c.text3,
        );
        y += ROW;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_read_as_tools_commands_and_arguments() {
        assert_eq!(parse("wall"), Parsed::Tool(Tool::Wall));
        assert_eq!(parse("Truss"), Parsed::Tool(Tool::Truss));
        assert_eq!(parse("ui.zoom_to_fit"), Parsed::Run("ui.zoom_to_fit".into(), json!({})));
        assert_eq!(parse("ui.zoom {\"scale\": 0.1}"), Parsed::Run("ui.zoom".into(), json!({ "scale": 0.1 })));
        assert_eq!(
            parse("layer.create name=Stage elevation=1000"),
            Parsed::Run("layer.create".into(), json!({ "name": "Stage", "elevation": 1000 }))
        );
        assert_eq!(parse("layer.create"), Parsed::Form("layer.create".into()));
        assert_eq!(parse("no.such"), Parsed::Unknown("no.such".into()));
        assert!(matches!(parse("ui.zoom {oops"), Parsed::BadArgs(_)));
    }

    #[test]
    fn completion_offers_tools_and_every_registry() {
        let c = complete("wal");
        assert_eq!(c.first(), Some(&Completion::Tool(Tool::Wall)));
        assert!(complete("ui.set_th").contains(&Completion::Command("ui.set_theme", "Set Theme")));
        assert!(complete("layer.cre").iter().any(|c| matches!(c, Completion::Command("layer.create", _))));
        // Titles and aliases complete to IDs.
        assert!(complete("auto patch").iter().any(|c| matches!(c, Completion::Command("patch.auto", _))));
        assert!(complete("traverse").iter().any(|c| matches!(c, Completion::Command(n, _) if n.starts_with("truss."))));
        assert!(complete("").is_empty());
        assert!(complete("layer.create name=X").is_empty());
    }
}
