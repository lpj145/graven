//! `gravenc diff`: a readable report of what changed between the lock and the schema,
//! with the compatibility verdict of each change (spec §9).

use crate::compat::Finding;
use crate::lock::{Lock, LockField};

/// One line of the report.
pub struct Change {
    /// `table Entry`, `enum Currency`, …
    pub scope: String,
    /// `+ fee @7: i64?`, `~ amount @1: i64 → i32`, …
    pub what: String,
    /// `allowed`, `error: type-changed`, `warning: required-reserved (confirmed)`.
    pub verdict: String,
}

/// The report: every change, and the findings that no change line explains.
pub struct Report {
    pub changes: Vec<Change>,
    pub other: Vec<String>,
}

fn finding_parts(f: &Finding) -> (&str, Option<&str>) {
    match f {
        Finding::Error { table, field, .. } | Finding::Warning { table, field, .. } => (table, field.as_deref()),
    }
}

fn verdict_of(f: &Finding) -> String {
    match f {
        Finding::Error { rule, .. } => format!("error: {rule}"),
        Finding::Warning { rule, confirmed: true, .. } => format!("warning: {rule} (confirmed)"),
        Finding::Warning { rule, confirmed: false, .. } => format!("warning: {rule} (not confirmed: blocks)"),
    }
}

fn field_text(f: &LockField) -> String {
    match f.presence.as_str() {
        "reserved" => format!("@{}: reserved {}", f.id, f.ty),
        "required" => format!("{} @{}: {}", f.name, f.id, f.ty),
        "optional" => format!("{} @{}: {}?", f.name, f.id, f.ty),
        p => format!("{} @{}: {} = {}", f.name, f.id, f.ty, p.trim_start_matches("default:")),
    }
}

fn presence_word(p: &str) -> &str {
    match p {
        "required" => "required",
        "optional" => "optional",
        "reserved" => "reserved",
        _ => "default",
    }
}

/// Compares `old` (the lock) with `new` (`lock_from_ir` of the new schema, built on `old`)
/// and attaches each compatibility finding to the change it is about.
pub fn diff(old: &Lock, new: &Lock, findings: &[Finding]) -> Report {
    let mut used = vec![false; findings.len()];
    let mut changes = Vec::new();
    let mut verdict = |scope_name: &str, names: &[&str], allowed: &str| -> String {
        for (i, f) in findings.iter().enumerate() {
            let (t, fld) = finding_parts(f);
            if t == scope_name && (names.is_empty() && fld.is_none() || fld.is_some_and(|x| names.contains(&x))) {
                used[i] = true;
                return verdict_of(f);
            }
        }
        if allowed.is_empty() { "allowed".into() } else { format!("allowed: {allowed}") }
    };

    if old.namespace != new.namespace {
        changes.push(Change {
            scope: "namespace".into(),
            what: format!("~ {} → {}", old.namespace, new.namespace),
            verdict: "error: namespace-changed".into(),
        });
    }

    for ot in &old.tables {
        let scope = format!("table {}", ot.name);
        let Some(nt) = new.tables.iter().find(|t| t.name == ot.name) else {
            let v = verdict(&ot.name, &[], "");
            changes.push(Change { scope, what: "- table removed".into(), verdict: v });
            continue;
        };
        for of in &ot.fields {
            match nt.fields.iter().find(|f| f.id == of.id) {
                None => {
                    let v = verdict(&ot.name, &[&of.name], "");
                    changes.push(Change { scope: scope.clone(), what: format!("- {}", field_text(of)), verdict: v });
                }
                Some(nf) if nf == of => {}
                Some(nf) => {
                    let mut parts = Vec::new();
                    if of.ty != nf.ty || of.size != nf.size {
                        parts.push(format!("type {} → {}", of.ty, nf.ty));
                    }
                    if of.presence != nf.presence {
                        let (a, b) = (&of.presence, &nf.presence);
                        if presence_word(a) == "default" && presence_word(b) == "default" {
                            parts.push(format!(
                                "default {} → {}",
                                a.trim_start_matches("default:"),
                                b.trim_start_matches("default:")
                            ));
                        } else {
                            parts.push(format!("{} → {}", presence_word(a), presence_word(b)));
                        }
                    }
                    if of.name != nf.name && nf.presence != "reserved" {
                        parts.push(format!("renamed {} → {}", of.name, nf.name));
                    }
                    if parts.is_empty() {
                        // Only the slot/bit moved: a consequence of an earlier field's size
                        // change (reported there), unless the checker flags it by itself.
                        let flagged = findings.iter().any(|f| {
                            f.rule() == "layout-changed" && finding_parts(f) == (ot.name.as_str(), Some(nf.name.as_str()))
                        });
                        if !flagged {
                            continue;
                        }
                        parts.push("layout changed".into());
                    }
                    let allowed = if parts.iter().any(|p| p.starts_with("renamed")) && parts.len() == 1 {
                        "field-renamed"
                    } else if nf.presence == "reserved" {
                        "reserved-optional"
                    } else if presence_word(&of.presence) == "default" && nf.presence == "optional" {
                        "default-to-optional"
                    } else {
                        ""
                    };
                    let v = verdict(&ot.name, &[&of.name, &nf.name], allowed);
                    changes.push(Change {
                        scope: scope.clone(),
                        what: format!("~ {} @{}: {}", if nf.presence == "reserved" { &of.name } else { &nf.name }, nf.id, parts.join(", ")),
                        verdict: v,
                    });
                }
            }
        }
        for nf in nt.fields.iter().filter(|f| !ot.fields.iter().any(|o| o.id == f.id)) {
            let allowed = if nf.presence == "optional" { "new-optional" } else { "new-default" };
            let v = verdict(&ot.name, &[&nf.name], allowed);
            changes.push(Change { scope: scope.clone(), what: format!("+ {}", field_text(nf)), verdict: v });
        }
    }
    for nt in new.tables.iter().filter(|t| !old.tables.iter().any(|o| o.name == t.name)) {
        changes.push(Change {
            scope: format!("table {}", nt.name),
            what: format!("+ table added ({} fields)", nt.fields.len()),
            verdict: "allowed: new-table".into(),
        });
    }

    for oe in &old.enums {
        let scope = format!("enum {}", oe.name);
        let Some(ne) = new.enums.iter().find(|e| e.name == oe.name) else { continue };
        if oe.base != ne.base {
            let v = verdict(&oe.name, &[], "");
            changes.push(Change { scope: scope.clone(), what: format!("~ base {} → {}", oe.base, ne.base), verdict: v });
        }
        for ov in &oe.values {
            match (ne.values.iter().find(|v| v.name == ov.name), ne.values.iter().find(|v| v.value == ov.value)) {
                (Some(nv), _) if nv.value == ov.value => {}
                (Some(nv), _) => {
                    let v = verdict(&oe.name, &[&ov.name], "");
                    changes.push(Change { scope: scope.clone(), what: format!("~ {} = {} → {}", ov.name, ov.value, nv.value), verdict: v });
                }
                (None, Some(nv)) => {
                    let v = verdict(&oe.name, &[&ov.name, &nv.name], "variant renamed");
                    changes.push(Change { scope: scope.clone(), what: format!("~ {} renamed {}", ov.name, nv.name), verdict: v });
                }
                (None, None) => {
                    let v = verdict(&oe.name, &[&ov.name], "");
                    changes.push(Change { scope: scope.clone(), what: format!("- {} = {}", ov.name, ov.value), verdict: v });
                }
            }
        }
        for nv in ne.values.iter().filter(|v| !oe.values.iter().any(|o| o.name == v.name || o.value == v.value)) {
            changes.push(Change {
                scope: scope.clone(),
                what: format!("+ {} = {}", nv.name, nv.value),
                verdict: "allowed: new-enum-variant".into(),
            });
        }
    }
    for os in &old.structs {
        let Some(ns) = new.structs.iter().find(|s| s.name == os.name) else { continue };
        if os != ns {
            let v = verdict(&os.name, &[], "");
            changes.push(Change { scope: format!("struct {}", os.name), what: "~ fields changed".into(), verdict: v });
        }
    }
    for ou in &old.unions {
        let scope = format!("union {}", ou.name);
        let Some(nu) = new.unions.iter().find(|u| u.name == ou.name) else { continue };
        for ov in &ou.variants {
            match nu.variants.iter().find(|v| v.tag == ov.tag) {
                Some(nv) if nv.table == ov.table => {}
                Some(nv) => {
                    let v = verdict(&ou.name, &[&ov.name, &nv.name], "");
                    changes.push(Change {
                        scope: scope.clone(),
                        what: format!("~ tag {}: {} → {}", ov.tag, ov.table, nv.table),
                        verdict: v,
                    });
                }
                None => {
                    let v = verdict(&ou.name, &[&ov.name], "");
                    changes.push(Change { scope: scope.clone(), what: format!("- {} (tag {})", ov.name, ov.tag), verdict: v });
                }
            }
        }
        for nv in nu.variants.iter().filter(|v| !ou.variants.iter().any(|o| o.tag == v.tag)) {
            changes.push(Change {
                scope: scope.clone(),
                what: format!("+ {} (tag {})", nv.name, nv.tag),
                verdict: "allowed: new-union-variant".into(),
            });
        }
    }

    let other = findings
        .iter()
        .zip(&used)
        .filter(|(f, u)| !**u && f.rule() != "namespace-changed")
        .map(|(f, _)| format!("{}: {}", verdict_of(f), f.msg()))
        .collect();
    Report { changes, other }
}
