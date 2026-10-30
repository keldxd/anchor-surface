//! Text and JSON output.

use crate::diff::{Report, Severity};
use crate::idl::Program;
use crate::json::quote;

pub const LIMITS: &str = "Only changes visible in the IDL are checked. Checks written in program code \
(require!, custom constraint expressions, manual key comparisons) are not, so no findings does not mean the upgrade is safe.";

pub struct Side<'a> {
    pub path: &'a str,
    pub program: &'a Program,
}

pub fn text(report: &Report, old: &Side, new: &Side) -> String {
    let mut s = String::new();
    s.push_str(&format!("anchor-surface {}\n", env!("CARGO_PKG_VERSION")));
    s.push_str(&format!("program: {}\n", report.program));
    for (label, side) in [("old", old), ("new", new)] {
        s.push_str(&format!(
            "{label}:     {} ({}, {} instructions)\n",
            side.path,
            side.program.format.label(),
            side.program.instructions.len()
        ));
    }
    s.push('\n');

    if report.findings.is_empty() {
        s.push_str("No changes to signers, accounts, arguments or IDL-level checks.\n");
    }
    for f in &report.findings {
        let place = match &f.account {
            Some(a) => format!("{} / {}", f.instruction, a),
            None => f.instruction.clone(),
        };
        s.push_str(&format!(
            "{:<7} {}  [{}]\n        {}\n",
            f.severity.label(),
            place,
            f.rule,
            f.message
        ));
    }

    for n in &report.notes {
        s.push_str(&format!("\nnote: {n}\n"));
    }
    s.push_str(&format!(
        "\n{} danger, {} review, {} info\n{}\n",
        report.count(Severity::Danger),
        report.count(Severity::Review),
        report.count(Severity::Info),
        LIMITS
    ));
    s
}

pub fn json(report: &Report, old: &Side, new: &Side) -> String {
    let side = |sd: &Side| {
        format!(
            "{{\"path\":{},\"format\":{},\"instructions\":{}}}",
            quote(sd.path),
            quote(match sd.program.format {
                crate::idl::Format::Modern => "modern",
                crate::idl::Format::Legacy => "legacy",
            }),
            sd.program.instructions.len()
        )
    };
    let findings: Vec<String> =
        report
            .findings
            .iter()
            .map(|f| {
                format!(
                "{{\"severity\":{},\"rule\":{},\"instruction\":{},\"account\":{},\"message\":{}}}",
                quote(&f.severity.label().to_ascii_lowercase()),
                quote(f.rule),
                quote(&f.instruction),
                f.account.as_deref().map(quote).unwrap_or_else(|| "null".into()),
                quote(&f.message)
            )
            })
            .collect();
    let notes: Vec<String> = report.notes.iter().map(|n| quote(n)).collect();
    format!(
        "{{\"tool\":\"anchor-surface\",\"version\":{},\"program\":{},\"old\":{},\"new\":{},\
\"constraints_compared\":{},\"summary\":{{\"danger\":{},\"review\":{},\"info\":{}}},\
\"findings\":[{}],\"notes\":[{}],\"limits\":{}}}\n",
        quote(env!("CARGO_PKG_VERSION")),
        quote(&report.program),
        side(old),
        side(new),
        report.constraints_compared,
        report.count(Severity::Danger),
        report.count(Severity::Review),
        report.count(Severity::Info),
        findings.join(","),
        notes.join(","),
        quote(LIMITS)
    )
}
