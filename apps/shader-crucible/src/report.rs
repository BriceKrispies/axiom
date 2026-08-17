//! The crucible's own report: what it authored, what the barrier compiled, what
//! both backends say about it, and what it does **not** do.
//!
//! One function assembles it, so the browser console, a native `--report` run
//! and this app's `README.md` all print the same lines from the same source of
//! truth. A report a human has to keep in sync with the code is a report that is
//! wrong within a month.

use crate::backends::support_lines;
use crate::introspection::introspection_lines;
use crate::limitations::LIMITATIONS;
use crate::preparation::PreparedPrograms;
use crate::stations::{all_surfaces, STATIONS};

/// The whole report, one line per entry.
pub fn report_lines(prepared: Option<&PreparedPrograms>) -> Vec<String> {
    let surfaces = all_surfaces();
    let mut lines = vec![
        "=== the shader crucible ==========================================".to_string(),
        String::new(),
        "stations".to_string(),
    ];
    lines.extend(STATIONS.iter().map(|station| {
        format!(
            "  {:>2}. {:<26} {}",
            station.number, station.name, station.proves
        )
    }));

    lines.push(String::new());
    lines.push("the preparation barrier".to_string());
    lines.push(match prepared {
        Some(p) => format!(
            "  {} programs compiled from {} authored surfaces; \
             rigid degradations: {:?}; skinned degradations: {:?}",
            p.program_count, p.surface_count, p.degradations, p.skinned_degradations
        ),
        None => "  not yet run".to_string(),
    });

    lines.push(String::new());
    lines.push("supported_by, for both real backend profiles".to_string());
    lines.extend(support_lines(&surfaces).iter().map(|l| format!("  {l}")));

    lines.push(String::new());
    lines.extend(introspection_lines());

    lines.push(String::new());
    lines.push("what this does NOT do".to_string());
    lines.extend(LIMITATIONS.iter().flat_map(|l| {
        vec![
            format!("  {}. (station {}) {}", l.number, l.station, l.headline),
            format!("      {}", l.detail),
        ]
    }));
    lines
}

/// The report as one newline-joined block.
pub fn report(prepared: Option<&PreparedPrograms>) -> String {
    report_lines(prepared).join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_report_names_every_station_and_every_limitation() {
        let text = report(None);
        STATIONS
            .iter()
            .for_each(|s| assert!(text.contains(s.name), "missing station {}", s.name));
        LIMITATIONS
            .iter()
            .for_each(|l| assert!(text.contains(l.headline), "missing limitation {}", l.number));
        assert!(text.contains("not yet run"));
    }

    #[test]
    fn the_report_carries_the_barriers_numbers_when_it_has_them() {
        let prepared = PreparedPrograms {
            program_count: 10,
            surface_count: 10,
            degradations: Vec::new(),
            skinned_degradations: vec![axiom_host::FrameFeature::ProceduralSurface],
        };
        let text = report(Some(&prepared));
        assert!(text.contains("10 programs compiled from 10 authored surfaces"));
        assert!(text.contains("ProceduralSurface"));
    }

    #[test]
    fn the_report_is_deterministic() {
        assert_eq!(report(None), report(None));
    }
}
