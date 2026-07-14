//! Procedural emblem rendering: an [`EmblemView`] becomes one inline SVG —
//! base plate polygon + motif polygons + optional initial. Pure geometry
//! from the typed definition; no image assets.

use crate::data::team::{EmblemBase, EmblemMotif};
use crate::frontend::widgets::EmblemView;

fn base_shape(base: EmblemBase, fill: &str, stroke: &str) -> String {
    match base {
        EmblemBase::Shield => format!(
            "<polygon points='50,4 92,18 92,55 50,96 8,55 8,18' fill='{fill}' \
             stroke='{stroke}' stroke-width='5'/>"
        ),
        EmblemBase::Disc => format!(
            "<circle cx='50' cy='50' r='45' fill='{fill}' stroke='{stroke}' stroke-width='5'/>"
        ),
        EmblemBase::Hex => format!(
            "<polygon points='50,4 90,27 90,73 50,96 10,73 10,27' fill='{fill}' \
             stroke='{stroke}' stroke-width='5'/>"
        ),
        EmblemBase::Pennant => format!(
            "<polygon points='12,12 90,50 12,88' fill='{fill}' stroke='{stroke}' \
             stroke-width='5'/>"
        ),
    }
}

fn motif_shape(motif: EmblemMotif, fill: &str, accent: &str) -> String {
    match motif {
        EmblemMotif::Bolt => {
            format!("<polygon points='56,14 32,54 46,54 40,86 70,44 53,44' fill='{fill}'/>")
        }
        EmblemMotif::Wing => format!(
            "<polygon points='16,60 48,24 52,36 42,48 84,32 76,48 38,62' fill='{fill}'/>\
             <polygon points='26,70 62,58 56,70' fill='{accent}'/>"
        ),
        EmblemMotif::Claw => format!(
            "<polygon points='26,26 36,24 44,74 36,76' fill='{fill}'/>\
             <polygon points='45,22 55,20 60,76 51,78' fill='{fill}'/>\
             <polygon points='63,26 73,28 70,74 62,72' fill='{fill}'/>"
        ),
        EmblemMotif::Star => format!(
            "<polygon points='50,14 60,39 87,39 65,55 73,82 50,66 27,82 35,55 13,39 40,39' \
             fill='{fill}'/>"
        ),
        EmblemMotif::Fang => format!(
            "<polygon points='24,28 50,18 76,28 68,54 50,80 32,54' fill='{fill}'/>\
             <polygon points='38,54 45,54 41,68' fill='{accent}'/>\
             <polygon points='55,54 62,54 59,68' fill='{accent}'/>"
        ),
        EmblemMotif::Chevrons => format!(
            "<polygon points='26,30 50,48 74,30 74,44 50,62 26,44' fill='{fill}'/>\
             <polygon points='26,52 50,70 74,52 74,66 50,84 26,66' fill='{accent}'/>"
        ),
    }
}

/// The complete inline-SVG markup for one emblem.
pub fn emblem_svg(view: &EmblemView) -> String {
    let base = base_shape(view.base, &view.primary, &view.accent);
    let motif = motif_shape(view.motif, &view.secondary, &view.accent);
    let initial = view
        .initial
        .map(|c| {
            format!(
                "<text x='50' y='62' text-anchor='middle' font-size='34' \
                 font-family='Impact,Arial Narrow,sans-serif' font-style='italic' \
                 font-weight='900' fill='{}' stroke='#06090e' stroke-width='1.5' \
                 opacity='0.92'>{c}</text>",
                view.accent
            )
        })
        .unwrap_or_default();
    format!("<svg viewBox='0 0 100 100' width='100%' height='100%'>{base}{motif}{initial}</svg>")
}
