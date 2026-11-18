//! The first-run offer of the tutorial: a small egui modal ("Build a basic stage?")
//! with Start tutorial / Not now / Don't ask again. The answer goes into the user's
//! settings (`Prefs.onboarding`); "Not now" asks again on the next launch, the other two
//! never. Only the app asks ([`Studio::offer_tutorial`] from `run`), so tests that build a
//! [`Studio`] with fresh settings see nothing unless they ask too.

use egui::{Align2, RichText, vec2};
use serde::{Deserialize, Serialize};

use crate::app::Studio;
use crate::i18n::t;
use crate::theme;
use crate::tutorial::Action;

/// What the user answered to the tutorial offer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TutorialOffer {
    /// Never asked.
    #[default]
    Ask,
    /// Started it from the offer.
    Started,
    /// "Not now": ask again next launch.
    Later,
    /// "Don't ask again".
    Never,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct OnboardingPrefs {
    pub tutorial: TutorialOffer,
}

impl Studio {
    /// Show the tutorial offer if the user has not started it or turned it off.
    pub fn offer_tutorial(&mut self) {
        let asked = self.prefs.get().onboarding.tutorial;
        if matches!(asked, TutorialOffer::Ask | TutorialOffer::Later) && !self.ui.tutorial.active {
            self.ui.tutorial_offer = true;
        }
    }

    /// Answer the offer (the buttons do this).
    pub fn answer_tutorial_offer(&mut self, answer: TutorialOffer) {
        self.ui.tutorial_offer = false;
        if answer == TutorialOffer::Started {
            crate::tutorial::act(&mut self.ui, Action::Start);
        }
        self.prefs.update(|p| p.onboarding.tutorial = answer);
    }
}

/// The offer, when it is open. Drawn after the window, as a modal over it.
pub fn show(ctx: &egui::Context, s: &mut Studio) {
    if !s.ui.tutorial_offer || s.ui.palette.open {
        return;
    }
    let c = theme::tokens(ctx.theme() == egui::Theme::Dark);
    let id = egui::Id::unique("tutorial_offer");
    let frame = egui::Frame::NONE
        .fill(c.pal_bg)
        .stroke(egui::Stroke::new(1.0, c.pal_border))
        .corner_radius(10.0)
        .inner_margin(egui::Margin::same(20))
        .shadow(egui::Shadow { offset: [0, 14], blur: 36, spread: 0, color: c.shadow });
    let mut answer = None;
    let modal = egui::Modal::new(id)
        .area(egui::Modal::default_area(id).anchor(Align2::CENTER_CENTER, vec2(0.0, -40.0)))
        .frame(frame)
        .backdrop_color(c.shadow.gamma_multiply(0.35))
        .show(ctx, |ui| {
            ui.set_width(340.0);
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(vec2(24.0, 24.0), egui::Sense::hover());
                crate::icons::paint(ui, r, "graduation-cap", 24.0, c.accent);
                ui.add_space(6.0);
                ui.label(RichText::new(t("offer.title")).font(theme::bold(15.0)).color(c.text));
            });
            ui.add_space(8.0);
            ui.label(RichText::new(t("offer.text")).color(c.text2));
            ui.add_space(14.0);
            ui.horizontal(|ui| {
                let start = egui::Button::new(RichText::new(t("offer.start")).color(c.on_accent).strong())
                    .fill(c.accent)
                    .corner_radius(5.0)
                    .min_size(vec2(0.0, 24.0));
                let r = ui.add(start);
                crate::widgets::name(&r, egui::Role::Button, "Start tutorial");
                if r.clicked() {
                    answer = Some(TutorialOffer::Started);
                }
                if crate::widgets::button(ui, t("offer.later"), "Not now").clicked() {
                    answer = Some(TutorialOffer::Later);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let never = egui::Button::new(RichText::new(t("offer.never")).color(c.text2)).frame(false);
                    let r = ui.add(never);
                    crate::widgets::name(&r, egui::Role::Button, "Don't ask again");
                    if r.clicked() {
                        answer = Some(TutorialOffer::Never);
                    }
                });
            });
        });
    // Esc or a click beside it: not now.
    if answer.is_none() && modal.should_close() {
        answer = Some(TutorialOffer::Later);
    }
    if let Some(a) = answer {
        s.answer_tutorial_offer(a);
    }
}
