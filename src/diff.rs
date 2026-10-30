//! Compares two versions of a program's IDL and reports every change to who
//! can call what, and how the program's instructions must be called.

use crate::idl::{Account, Format, Instruction, Program};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// A check visible in the IDL was removed or loosened, or existing
    /// clients and accounts will break.
    Danger,
    /// New surface or a behaviour change a reviewer should look at.
    Review,
    /// A check was added or tightened.
    Info,
}

impl Severity {
    pub fn label(self) -> &'static str {
        match self {
            Severity::Danger => "DANGER",
            Severity::Review => "REVIEW",
            Severity::Info => "INFO",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "danger" => Some(Severity::Danger),
            "review" => Some(Severity::Review),
            "info" => Some(Severity::Info),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Finding {
    pub severity: Severity,
    /// Stable rule id, e.g. `signer-removed`.
    pub rule: &'static str,
    pub instruction: String,
    pub account: Option<String>,
    pub message: String,
}

#[derive(Debug)]
pub struct Report {
    pub program: String,
    pub old_format: Format,
    pub new_format: Format,
    /// False when one side is legacy and the other modern: fixed addresses,
    /// PDA seeds and relations are then not compared (see `notes`).
    pub constraints_compared: bool,
    pub findings: Vec<Finding>,
    pub notes: Vec<String>,
}

impl Report {
    pub fn count(&self, s: Severity) -> usize {
        self.findings.iter().filter(|f| f.severity == s).count()
    }
}

pub fn diff(old: &Program, new: &Program) -> Report {
    let constraints_compared = old.format == new.format;
    let mut notes = Vec::new();
    if !constraints_compared {
        notes.push(format!(
            "The two IDLs use different formats (old: {}, new: {}). Signers, writable and optional \
             flags, accounts, account order and arguments were compared. Fixed addresses, PDA seeds \
             and relations were NOT compared, because the older format does not reliably record them.",
            old.format.label(),
            new.format.label()
        ));
    }

    let mut findings = Vec::new();
    let old_by_key: BTreeMap<&str, &Instruction> = old
        .instructions
        .iter()
        .map(|i| (i.key.as_str(), i))
        .collect();
    let new_keys: BTreeSet<&str> = new.instructions.iter().map(|i| i.key.as_str()).collect();

    for ix in &new.instructions {
        match old_by_key.get(ix.key.as_str()) {
            Some(prev) => instruction_changes(prev, ix, constraints_compared, &mut findings),
            None => findings.push(instruction_added(ix)),
        }
    }
    for ix in &old.instructions {
        if !new_keys.contains(ix.key.as_str()) {
            findings.push(Finding {
                severity: Severity::Review,
                rule: "instruction-removed",
                instruction: ix.name.clone(),
                account: None,
                message: "instruction removed; clients that still call it will fail".into(),
            });
        }
    }

    // Most serious first; within a severity, keep the order of the new IDL.
    findings.sort_by_key(|f| f.severity);

    Report {
        program: new.name.clone(),
        old_format: old.format,
        new_format: new.format,
        constraints_compared,
        findings,
        notes,
    }
}

fn instruction_added(ix: &Instruction) -> Finding {
    let signers: Vec<&str> = ix
        .accounts
        .iter()
        .filter(|a| a.signer)
        .map(|a| a.name.as_str())
        .collect();
    let who = if signers.is_empty() {
        "no account must sign, so anyone can call it".to_string()
    } else {
        format!("must be signed by: {}", signers.join(", "))
    };
    let writable = ix.accounts.iter().filter(|a| a.writable).count();
    Finding {
        severity: Severity::Review,
        rule: "instruction-added",
        instruction: ix.name.clone(),
        account: None,
        message: format!("new instruction; {who}; writes to {writable} account(s)"),
    }
}

fn instruction_changes(
    old: &Instruction,
    new: &Instruction,
    constraints: bool,
    out: &mut Vec<Finding>,
) {
    let push = |out: &mut Vec<Finding>, severity, rule, account: Option<&str>, message: String| {
        out.push(Finding {
            severity,
            rule,
            instruction: new.name.clone(),
            account: account.map(str::to_string),
            message,
        });
    };

    if let (Some(a), Some(b)) = (&old.discriminator, &new.discriminator) {
        if a != b {
            push(out, Severity::Danger, "discriminator-changed", None,
                format!("instruction discriminator changed from {a:?} to {b:?}; every existing client call will fail"));
        }
    }

    if let Some(msg) = args_change(old, new) {
        push(out, Severity::Danger, "args-changed", None,
            format!("instruction arguments changed ({msg}); existing clients will encode the wrong data"));
    }

    // Accounts are matched by name within the instruction.
    let old_acc: BTreeMap<&str, &Account> =
        old.accounts.iter().map(|a| (a.key.as_str(), a)).collect();
    let new_acc: BTreeMap<&str, &Account> =
        new.accounts.iter().map(|a| (a.key.as_str(), a)).collect();

    let old_order: Vec<&str> = old
        .accounts
        .iter()
        .map(|a| a.key.as_str())
        .filter(|k| new_acc.contains_key(k))
        .collect();
    let new_order: Vec<&str> = new
        .accounts
        .iter()
        .map(|a| a.key.as_str())
        .filter(|k| old_acc.contains_key(k))
        .collect();
    if old_order != new_order {
        push(out, Severity::Danger, "account-order-changed", None,
            "accounts are passed in a different order; clients that build the account list positionally will pass the wrong accounts".into());
    }

    for a in &new.accounts {
        let Some(b) = old_acc.get(a.key.as_str()) else {
            let (sev, what) = if a.optional {
                (Severity::Info, "new optional account")
            } else if a.signer {
                (
                    Severity::Review,
                    "new required account that must sign; existing clients will fail until updated",
                )
            } else if a.writable {
                (
                    Severity::Review,
                    "new required writable account; existing clients will fail until updated",
                )
            } else {
                (
                    Severity::Review,
                    "new required account; existing clients will fail until updated",
                )
            };
            push(out, sev, "account-added", Some(&a.name), what.into());
            continue;
        };
        account_changes(b, a, constraints, &mut |sev, rule, msg| {
            push(out, sev, rule, Some(&a.name), msg)
        });
    }

    for b in &old.accounts {
        if !new_acc.contains_key(b.key.as_str()) {
            if b.signer {
                push(out, Severity::Danger, "signer-account-removed", Some(&b.name),
                    "account that had to sign was removed; this instruction no longer requires that signature".into());
            } else {
                push(out, Severity::Review, "account-removed", Some(&b.name),
                    "account removed from the instruction; existing clients will pass the wrong accounts".into());
            }
        }
    }
}

fn account_changes(
    old: &Account,
    new: &Account,
    constraints: bool,
    emit: &mut dyn FnMut(Severity, &'static str, String),
) {
    match (old.signer, new.signer) {
        (true, false) => emit(Severity::Danger, "signer-removed",
            "no longer required to sign; any account can be passed here without its signature unless the program checks it in code".into()),
        (false, true) => emit(Severity::Review, "signer-added",
            "now required to sign; existing clients that do not sign will fail".into()),
        _ => {}
    }
    match (old.writable, new.writable) {
        (false, true) => emit(
            Severity::Review,
            "writable-added",
            "now writable; the instruction can modify this account".into(),
        ),
        (true, false) => emit(
            Severity::Info,
            "writable-removed",
            "no longer writable".into(),
        ),
        _ => {}
    }
    match (old.optional, new.optional) {
        (false, true) if old.signer || new.signer => emit(
            Severity::Danger,
            "optional-added",
            "a signing account is now optional; the signature can be skipped by omitting it".into(),
        ),
        (false, true) => emit(
            Severity::Review,
            "optional-added",
            "now optional; any check tied to this account is skipped when it is omitted".into(),
        ),
        (true, false) => emit(
            Severity::Review,
            "optional-removed",
            "now required; clients that omit it will fail".into(),
        ),
        _ => {}
    }

    if !constraints {
        return;
    }

    match (&old.address, &new.address) {
        (Some(a), None) if new.signer => emit(Severity::Danger, "address-removed",
            format!("was fixed to {a}; any signer can now act here unless the program checks the key in code")),
        (Some(a), None) => emit(Severity::Danger, "address-removed",
            format!("was fixed to {a}; any account can now be passed here unless the program checks it in code")),
        (Some(a), Some(b)) if a != b => emit(Severity::Danger, "address-changed",
            format!("fixed address changed from {a} to {b}")),
        (None, Some(b)) => emit(Severity::Info, "address-added", format!("now fixed to {b}")),
        _ => {}
    }
    match (&old.pda, &new.pda) {
        (Some(a), None) => emit(Severity::Danger, "pda-removed",
            format!("PDA seed check removed (was {a}); any account can now be passed here unless the program checks it in code")),
        (Some(a), Some(b)) if a != b => emit(Severity::Danger, "pda-changed",
            format!("PDA seeds changed from {a} to {b}; accounts created with the old seeds no longer match")),
        (None, Some(b)) => emit(Severity::Info, "pda-added", format!("now checked as a PDA: {b}")),
        _ => {}
    }
    for r in &old.relations {
        if !new.relations.contains(r) {
            emit(Severity::Danger, "relation-removed",
                format!("relation `{r}` removed (has_one check); this account is no longer tied to `{r}`"));
        }
    }
    for r in &new.relations {
        if !old.relations.contains(r) {
            emit(
                Severity::Info,
                "relation-added",
                format!("now tied to `{r}` (has_one check)"),
            );
        }
    }
}

/// Describes what changed in the argument list, or None if nothing did.
fn args_change(old: &Instruction, new: &Instruction) -> Option<String> {
    let sig = |ix: &Instruction| {
        ix.args
            .iter()
            .map(|a| (a.key.clone(), a.ty.clone()))
            .collect::<Vec<_>>()
    };
    if sig(old) == sig(new) {
        return None;
    }
    let mut parts = Vec::new();
    let old_map: BTreeMap<&str, &str> = old
        .args
        .iter()
        .map(|a| (a.key.as_str(), a.ty.as_str()))
        .collect();
    let new_map: BTreeMap<&str, &str> = new
        .args
        .iter()
        .map(|a| (a.key.as_str(), a.ty.as_str()))
        .collect();
    for a in &new.args {
        match old_map.get(a.key.as_str()) {
            None => parts.push(format!("added `{}: {}`", a.name, a.ty)),
            Some(t) if *t != a.ty => parts.push(format!("`{}` type {} -> {}", a.name, t, a.ty)),
            _ => {}
        }
    }
    for a in &old.args {
        if !new_map.contains_key(a.key.as_str()) {
            parts.push(format!("removed `{}: {}`", a.name, a.ty));
        }
    }
    if parts.is_empty() {
        parts.push("argument order changed".into());
    }
    Some(parts.join("; "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::idl::load;

    fn modern(accounts: &str, args: &str) -> Program {
        load(&format!(
            r#"{{"address":"P","metadata":{{"name":"t","version":"0.1.0","spec":"0.1.0"}},
            "instructions":[{{"name":"act","discriminator":[1,1,1,1,1,1,1,1],"accounts":[{accounts}],"args":[{args}]}}]}}"#
        ))
        .unwrap()
    }

    fn rules(old: &Program, new: &Program) -> Vec<(Severity, &'static str)> {
        diff(old, new)
            .findings
            .iter()
            .map(|f| (f.severity, f.rule))
            .collect()
    }

    const ADMIN: &str = r#"{"name":"admin","signer":true}"#;
    const POOL: &str = r#"{"name":"pool","writable":true}"#;

    #[test]
    fn identical_idls_have_no_findings() {
        let a = modern(&format!("{ADMIN},{POOL}"), "");
        assert!(diff(&a, &a).findings.is_empty());
    }

    #[test]
    fn docs_only_changes_are_ignored() {
        let a = modern(r#"{"name":"admin","signer":true}"#, "");
        let b = modern(r#"{"name":"admin","signer":true,"docs":["new words"]}"#, "");
        assert!(diff(&a, &b).findings.is_empty());
    }

    #[test]
    fn signer_removed_is_danger() {
        let a = modern(&format!("{ADMIN},{POOL}"), "");
        let b = modern(&format!(r#"{{"name":"admin"}},{POOL}"#), "");
        assert_eq!(rules(&a, &b), vec![(Severity::Danger, "signer-removed")]);
    }

    #[test]
    fn signer_added_is_review() {
        let a = modern(r#"{"name":"admin"}"#, "");
        let b = modern(ADMIN, "");
        assert_eq!(rules(&a, &b), vec![(Severity::Review, "signer-added")]);
    }

    #[test]
    fn signer_account_removed_is_danger() {
        let a = modern(&format!("{ADMIN},{POOL}"), "");
        let b = modern(POOL, "");
        assert_eq!(
            rules(&a, &b),
            vec![(Severity::Danger, "signer-account-removed")]
        );
    }

    #[test]
    fn relation_removed_is_danger_and_added_is_info() {
        let a = modern(
            r#"{"name":"pool","writable":true,"relations":["admin"]}"#,
            "",
        );
        let b = modern(r#"{"name":"pool","writable":true}"#, "");
        assert_eq!(rules(&a, &b), vec![(Severity::Danger, "relation-removed")]);
        assert_eq!(rules(&b, &a), vec![(Severity::Info, "relation-added")]);
    }

    #[test]
    fn address_rules() {
        let fixed = modern(r#"{"name":"owner","address":"AAA"}"#, "");
        let other = modern(r#"{"name":"owner","address":"BBB"}"#, "");
        let free = modern(r#"{"name":"owner"}"#, "");
        assert_eq!(
            rules(&fixed, &free),
            vec![(Severity::Danger, "address-removed")]
        );
        assert_eq!(
            rules(&fixed, &other),
            vec![(Severity::Danger, "address-changed")]
        );
        assert_eq!(
            rules(&free, &fixed),
            vec![(Severity::Info, "address-added")]
        );
    }

    #[test]
    fn pda_rules() {
        let a = modern(
            r#"{"name":"vault","pda":{"seeds":[{"kind":"const","value":[118]}]}}"#,
            "",
        );
        let b = modern(
            r#"{"name":"vault","pda":{"seeds":[{"kind":"const","value":[119]}]}}"#,
            "",
        );
        let none = modern(r#"{"name":"vault"}"#, "");
        assert_eq!(rules(&a, &b), vec![(Severity::Danger, "pda-changed")]);
        assert_eq!(rules(&a, &none), vec![(Severity::Danger, "pda-removed")]);
        assert_eq!(rules(&none, &a), vec![(Severity::Info, "pda-added")]);
    }

    #[test]
    fn writable_rules() {
        let ro = modern(r#"{"name":"cfg"}"#, "");
        let rw = modern(r#"{"name":"cfg","writable":true}"#, "");
        assert_eq!(rules(&ro, &rw), vec![(Severity::Review, "writable-added")]);
        assert_eq!(rules(&rw, &ro), vec![(Severity::Info, "writable-removed")]);
    }

    #[test]
    fn optional_signer_is_danger() {
        let a = modern(ADMIN, "");
        let b = modern(r#"{"name":"admin","signer":true,"optional":true}"#, "");
        assert_eq!(rules(&a, &b), vec![(Severity::Danger, "optional-added")]);
        let c = modern(r#"{"name":"x"}"#, "");
        let d = modern(r#"{"name":"x","optional":true}"#, "");
        assert_eq!(rules(&c, &d), vec![(Severity::Review, "optional-added")]);
    }

    #[test]
    fn account_order_and_args() {
        let a = modern(
            &format!("{ADMIN},{POOL}"),
            r#"{"name":"amount","type":"u64"}"#,
        );
        let b = modern(
            &format!("{POOL},{ADMIN}"),
            r#"{"name":"amount","type":"u128"}"#,
        );
        let r = rules(&a, &b);
        assert!(r.contains(&(Severity::Danger, "account-order-changed")));
        assert!(r.contains(&(Severity::Danger, "args-changed")));
        assert_eq!(r.len(), 2);
    }

    #[test]
    fn new_permissionless_instruction_is_called_out() {
        let a = load(r#"{"metadata":{"name":"t","spec":"0.1.0"},"instructions":[]}"#).unwrap();
        let b = modern(POOL, "");
        let report = diff(&a, &b);
        assert_eq!(report.findings.len(), 1);
        assert_eq!(report.findings[0].rule, "instruction-added");
        assert!(report.findings[0].message.contains("anyone can call it"));
    }

    #[test]
    fn mixed_formats_skip_constraints_and_say_so() {
        let legacy = load(r#"{"name":"t","instructions":[{"name":"act","accounts":[{"name":"owner","isMut":false,"isSigner":true}],"args":[]}]}"#).unwrap();
        let modern_ = modern(r#"{"name":"owner","signer":true,"address":"AAA"}"#, "");
        let report = diff(&legacy, &modern_);
        assert!(!report.constraints_compared);
        assert!(report.findings.is_empty(), "{:?}", report.findings);
        assert_eq!(report.notes.len(), 1);
    }

    #[test]
    fn findings_are_sorted_most_serious_first() {
        let a = modern(r#"{"name":"admin","signer":true},{"name":"cfg"}"#, "");
        let b = modern(
            r#"{"name":"admin"},{"name":"cfg","writable":true},{"name":"extra","optional":true}"#,
            "",
        );
        let sev: Vec<Severity> = diff(&a, &b).findings.iter().map(|f| f.severity).collect();
        let mut sorted = sev.clone();
        sorted.sort();
        assert_eq!(sev, sorted);
        assert_eq!(sev.first(), Some(&Severity::Danger));
    }
}
