//! `gravenc diff`: a readable report of what changed between the lock and the schema,
//! with the compatibility verdict of each change (spec §9).

use crate::compat::Finding;
use crate::lock::{Lock, LockField, LockMethod};

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

    for os in &old.services {
        let scope = format!("service {}", os.name);
        let Some(ns) = new.services.iter().find(|s| s.name == os.name) else {
            let v = verdict(&os.name, &[], "");
            changes.push(Change { scope, what: "- service removed".into(), verdict: v });
            continue;
        };
        if os.timeout_ms != ns.timeout_ms {
            changes.push(Change {
                scope: scope.clone(),
                what: format!("~ timeout {} → {}", fmt_timeout(os.timeout_ms), fmt_timeout(ns.timeout_ms)),
                verdict: "allowed: timeout-changed".into(),
            });
        }
        for om in &os.methods {
            match ns.methods.iter().find(|m| m.id == om.id) {
                None => {
                    let v = match om.name.as_deref() {
                        Some(oname) => verdict(&os.name, &[oname], ""),
                        None => verdict(&os.name, &[], ""),
                    };
                    changes.push(Change { scope: scope.clone(), what: format!("- {}", method_text(om)), verdict: v });
                }
                Some(nm) if nm == om => {}
                Some(nm) if !om.reserved && nm.reserved => {
                    let oname = om.name.as_deref().unwrap_or("?");
                    let v = verdict(&os.name, &[oname], "");
                    changes.push(Change {
                        scope: scope.clone(),
                        what: format!("~ {oname} @{}: → reserved", om.id),
                        verdict: v,
                    });
                }
                Some(nm) if om.reserved && !nm.reserved => {
                    let nname = nm.name.as_deref().unwrap_or("?");
                    let v = verdict(&os.name, &[nname], "");
                    changes.push(Change { scope: scope.clone(), what: format!("~ @{}: reserved → {nname}", om.id), verdict: v });
                }
                Some(nm) => {
                    let mut parts = Vec::new();
                    if om.input != nm.input || om.output != nm.output || om.error != nm.error || om.shape != nm.shape {
                        parts.push("signature/shape changed".to_string());
                    }
                    if om.name != nm.name {
                        parts.push(format!(
                            "renamed {} → {}",
                            om.name.as_deref().unwrap_or("?"),
                            nm.name.as_deref().unwrap_or("?")
                        ));
                    }
                    if om.timeout_ms != nm.timeout_ms {
                        parts.push(format!("timeout {} → {}", fmt_timeout(om.timeout_ms), fmt_timeout(nm.timeout_ms)));
                    }
                    let allowed = if parts.iter().all(|p| p.starts_with("timeout")) {
                        "timeout-changed"
                    } else if parts.iter().any(|p| p.starts_with("renamed"))
                        && parts.iter().all(|p| p.starts_with("renamed") || p.starts_with("timeout"))
                    {
                        "method-renamed"
                    } else {
                        ""
                    };
                    let nname = nm.name.as_deref().unwrap_or("?");
                    let oname = om.name.as_deref().unwrap_or("?");
                    let v = verdict(&os.name, &[oname, nname], allowed);
                    changes.push(Change {
                        scope: scope.clone(),
                        what: format!("~ {nname} @{}: {}", nm.id, parts.join(", ")),
                        verdict: v,
                    });
                }
            }
        }
        for nm in ns.methods.iter().filter(|m| !os.methods.iter().any(|o| o.id == m.id)) {
            changes.push(Change {
                scope: scope.clone(),
                what: format!("+ {}", method_text(nm)),
                verdict: "allowed: new-method".into(),
            });
        }
    }
    for ns in new.services.iter().filter(|s| !old.services.iter().any(|o| o.name == s.name)) {
        changes.push(Change {
            scope: format!("service {}", ns.name),
            what: format!("+ service added ({} methods)", ns.methods.len()),
            verdict: "allowed: new-service".into(),
        });
    }

    let other = findings
        .iter()
        .zip(&used)
        .filter(|(f, u)| !**u && f.rule() != "namespace-changed")
        .map(|(f, _)| format!("{}: {}", verdict_of(f), f.msg()))
        .collect();
    Report { changes, other }
}

fn fmt_timeout(ms: Option<u32>) -> String {
    match ms {
        Some(v) => format!("{v}ms"),
        None => "none".into(),
    }
}

fn method_text(m: &LockMethod) -> String {
    if m.reserved {
        return format!("@{}: reserved", m.id);
    }
    let name = m.name.as_deref().unwrap_or("?");
    let input = m.input.as_deref().unwrap_or("?");
    let ret = match (m.shape.as_deref(), m.output.as_deref(), m.error.as_deref()) {
        (Some("one-way"), _, _) => String::new(),
        (Some("stream"), Some(out), Some(err)) => format!(" -> Result<stream {out}, {err}>"),
        (Some("stream"), Some(out), None) => format!(" -> stream {out}"),
        (_, Some(out), Some(err)) => format!(" -> Result<{out}, {err}>"),
        (_, Some(out), None) => format!(" -> {out}"),
        _ => String::new(),
    };
    format!("{name} @{}: ({input}){ret}", m.id)
}
