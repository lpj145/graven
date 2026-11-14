//! Dynamic validation of decoded [`Value`]s against schema rules and a registry of
//! named check functions.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::fmt;
use std::sync::Arc;

use graven::validate::{Invalid, Rule};

use crate::interp::Value;
use crate::model::{Presence, TableDef, Ty};

#[derive(Debug, Clone, PartialEq)]
pub enum RangeBound {
    Int(i128),
    UInt(u128),
    Float(f64),
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct FieldRules {
    pub range: Option<(Option<RangeBound>, Option<RangeBound>)>,
    pub len: Option<(Option<u64>, Option<u64>)>,
    pub finite: bool,
    pub known: Option<&'static [u32]>,
    pub checks: Vec<&'static str>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct TableRules {
    pub checks: Vec<&'static str>,
    pub fields: HashMap<u16, FieldRules>,
}

/// Validation rules for a schema, keyed by `TableDef::name`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ValidationSchema {
    pub tables: HashMap<&'static str, TableRules>,
}

pub type CheckFn = Arc<dyn Fn(&Value) -> bool + Send + Sync>;

#[derive(Default, Clone)]
pub struct CheckRegistry {
    fns: HashMap<String, CheckFn>,
}

impl CheckRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, name: impl Into<String>, f: impl Fn(&Value) -> bool + Send + Sync + 'static) {
        self.fns.insert(name.into(), Arc::new(f));
    }

    pub fn get(&self, name: &str) -> Option<&CheckFn> {
        self.fns.get(name)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissingCheck {
    pub name: String,
}

impl fmt::Display for MissingCheck {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "missing check function `{}` in registry", self.name)
    }
}

impl std::error::Error for MissingCheck {}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BuiltinReport {
    pub invalid: Vec<Invalid>,
    pub unchecked: BTreeSet<&'static str>,
}

/// Validator for a root table. Constructing it verifies that every `check(name)` reachable
/// from `root` is present in `registry`.
pub struct Validator {
    root: &'static TableDef,
    schema: ValidationSchema,
    registry: CheckRegistry,
}

impl Validator {
    pub fn new(
        root: &'static TableDef,
        schema: ValidationSchema,
        registry: CheckRegistry,
    ) -> Result<Self, MissingCheck> {
        let mut seen = HashSet::new();
        check_registry_reachable(root, &schema, &registry, &mut seen)?;
        Ok(Self { root, schema, registry })
    }

    pub fn validate(&self, value: &Value) -> Result<(), Invalid> {
        let mut report = BuiltinReport::default();
        self.walk(self.root, value, Some(&self.registry), true, &mut report);
        if let Some(first) = report.invalid.into_iter().next() {
            Err(first)
        } else {
            Ok(())
        }
    }

    pub fn validate_all(&self, value: &Value) -> Vec<Invalid> {
        let mut report = BuiltinReport::default();
        self.walk(self.root, value, Some(&self.registry), false, &mut report);
        report.invalid
    }

    /// Runs built-in rules without a registry and collects any `check(name)` encountered as
    /// `"not checked"` in `unchecked`.
    pub fn validate_builtin(root: &'static TableDef, schema: &ValidationSchema, value: &Value) -> BuiltinReport {
        let v = Self { root, schema: schema.clone(), registry: CheckRegistry::new() };
        let mut report = BuiltinReport::default();
        v.walk(root, value, None, false, &mut report);
        report
    }

    fn walk(
        &self,
        td: &'static TableDef,
        value: &Value,
        registry: Option<&CheckRegistry>,
        stop_first: bool,
        report: &mut BuiltinReport,
    ) {
        let Value::Table(map) = value else { return };
        let tr = self.schema.tables.get(td.name);
        let tname = td.name.rsplit('.').next().unwrap_or(td.name);
        let tname_static: &'static str = leak_or_slice(td.name, tname);

        for f in &td.fields {
            if matches!(f.presence, Presence::Reserved) {
                continue;
            }
            let Some(fval) = map.get(f.name) else {
                continue;
            };
            if let Some(fr) = tr.and_then(|t| t.fields.get(&f.id)) {
                check_field_rules(tname_static, f.name, fr, fval, registry, stop_first, report);
                if stop_first && !report.invalid.is_empty() {
                    return;
                }
            }
            match &f.ty {
                Ty::Table(sub) => {
                    self.walk(sub.get(), fval, registry, stop_first, report);
                    if stop_first && !report.invalid.is_empty() {
                        return;
                    }
                }
                Ty::List(elem) => {
                    if let Ty::Table(sub) = &**elem
                        && let Value::List(items) = fval
                    {
                        for (i, item) in items.iter().enumerate() {
                            let start = report.invalid.len();
                            self.walk(sub.get(), item, registry, stop_first, report);
                            for inv in &mut report.invalid[start..] {
                                if inv.index.is_none() {
                                    inv.index = u32::try_from(i).ok();
                                }
                            }
                            if stop_first && !report.invalid.is_empty() {
                                return;
                            }
                        }
                    }
                }
                Ty::Union(u) => {
                    if let Value::Union { tag, value: uval } = fval
                        && let Some((_, _, sub)) = u.variants.iter().find(|(t, _, _)| t == tag)
                    {
                        self.walk(sub.get(), uval, registry, stop_first, report);
                        if stop_first && !report.invalid.is_empty() {
                            return;
                        }
                    }
                }
                _ => {}
            }
        }

        if let Some(tr) = tr {
            for &cname in &tr.checks {
                if let Some(reg) = registry {
                    if let Some(func) = reg.get(cname)
                        && !func(value)
                    {
                        report.invalid.push(Invalid {
                            table: tname_static,
                            field: None,
                            index: None,
                            rule: Rule::Check(cname),
                        });
                        if stop_first {
                            return;
                        }
                    }
                } else {
                    report.unchecked.insert(cname);
                }
            }
        }
    }
}

fn leak_or_slice(full: &'static str, suffix: &str) -> &'static str {
    let offset = full.len() - suffix.len();
    &full[offset..]
}

fn check_registry_reachable(
    td: &'static TableDef,
    schema: &ValidationSchema,
    registry: &CheckRegistry,
    seen: &mut HashSet<*const TableDef>,
) -> Result<(), MissingCheck> {
    if !seen.insert(td as *const TableDef) {
        return Ok(());
    }
    if let Some(tr) = schema.tables.get(td.name) {
        for &c in &tr.checks {
            if registry.get(c).is_none() {
                return Err(MissingCheck { name: c.to_string() });
            }
        }
        for fr in tr.fields.values() {
            for &c in &fr.checks {
                if registry.get(c).is_none() {
                    return Err(MissingCheck { name: c.to_string() });
                }
            }
        }
    }
    for f in &td.fields {
        match &f.ty {
            Ty::Table(sub) => check_registry_reachable(sub.get(), schema, registry, seen)?,
            Ty::List(elem) => {
                if let Ty::Table(sub) = &**elem {
                    check_registry_reachable(sub.get(), schema, registry, seen)?;
                }
            }
            Ty::Union(u) => {
                for (_, _, sub) in &u.variants {
                    check_registry_reachable(sub.get(), schema, registry, seen)?;
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn check_field_rules(
    table: &'static str,
    field: &'static str,
    fr: &FieldRules,
    val: &Value,
    registry: Option<&CheckRegistry>,
    stop_first: bool,
    report: &mut BuiltinReport,
) {
    if let Some((min, max)) = &fr.range
        && !in_range(val, min.as_ref(), max.as_ref())
    {
        report.invalid.push(Invalid { table, field: Some(field), index: None, rule: Rule::Range });
        if stop_first {
            return;
        }
    }

    if let Some((min, max)) = fr.len {
        let len = match val {
            Value::Str(s) => Some(s.len() as u64),
            Value::Bytes(b) => Some(b.len() as u64),
            Value::List(l) => Some(l.len() as u64),
            _ => None,
        };
        if let Some(n) = len
            && (min.is_some_and(|m| n < m) || max.is_some_and(|m| n > m))
        {
            report.invalid.push(Invalid { table, field: Some(field), index: None, rule: Rule::Len });
            if stop_first {
                return;
            }
        }
    }

    if fr.finite {
        let ok = match val {
            Value::F32(x) => x.is_finite(),
            Value::F64(x) => x.is_finite(),
            _ => true,
        };
        if !ok {
            report.invalid.push(Invalid { table, field: Some(field), index: None, rule: Rule::Finite });
            if stop_first {
                return;
            }
        }
    }

    if let Some(known_vals) = fr.known {
        match val {
            Value::UInt(raw) => {
                if u32::try_from(*raw).ok().is_none_or(|r| !known_vals.contains(&r)) {
                    report.invalid.push(Invalid { table, field: Some(field), index: None, rule: Rule::Known });
                    if stop_first {
                        return;
                    }
                }
            }
            Value::List(items) => {
                for (i, item) in items.iter().enumerate() {
                    if let Value::UInt(raw) = item
                        && u32::try_from(*raw).ok().is_none_or(|r| !known_vals.contains(&r))
                    {
                        report.invalid.push(Invalid {
                            table,
                            field: Some(field),
                            index: u32::try_from(i).ok(),
                            rule: Rule::Known,
                        });
                        if stop_first {
                            return;
                        }
                    }
                }
            }
            _ => {}
        }
    }

    for &cname in &fr.checks {
        if let Some(reg) = registry {
            if let Some(func) = reg.get(cname)
                && !func(val)
            {
                report.invalid.push(Invalid {
                    table,
                    field: Some(field),
                    index: None,
                    rule: Rule::Check(cname),
                });
                if stop_first {
                    return;
                }
            }
        } else {
            report.unchecked.insert(cname);
        }
    }
}

fn in_range(val: &Value, min: Option<&RangeBound>, max: Option<&RangeBound>) -> bool {
    match val {
        Value::Int(i) => {
            let min_ok = min.is_none_or(|b| match b {
                RangeBound::Int(m) => i >= m,
                RangeBound::UInt(m) => *i >= 0 && (*i as u128) >= *m,
                RangeBound::Float(_) => true,
            });
            let max_ok = max.is_none_or(|b| match b {
                RangeBound::Int(m) => i <= m,
                RangeBound::UInt(m) => *i < 0 || (*i as u128) <= *m,
                RangeBound::Float(_) => true,
            });
            min_ok && max_ok
        }
        Value::UInt(u) => {
            let min_ok = min.is_none_or(|b| match b {
                RangeBound::UInt(m) => u >= m,
                RangeBound::Int(m) => *m < 0 || *u >= (*m as u128),
                RangeBound::Float(_) => true,
            });
            let max_ok = max.is_none_or(|b| match b {
                RangeBound::UInt(m) => u <= m,
                RangeBound::Int(m) => *m >= 0 && *u <= (*m as u128),
                RangeBound::Float(_) => true,
            });
            min_ok && max_ok
        }
        Value::F32(x) => {
            let xf = *x as f64;
            let min_ok = min.is_none_or(|b| match b {
                RangeBound::Float(m) => xf >= *m,
                _ => true,
            });
            let max_ok = max.is_none_or(|b| match b {
                RangeBound::Float(m) => xf <= *m,
                _ => true,
            });
            min_ok && max_ok
        }
        Value::F64(x) => {
            let min_ok = min.is_none_or(|b| match b {
                RangeBound::Float(m) => *x >= *m,
                _ => true,
            });
            let max_ok = max.is_none_or(|b| match b {
                RangeBound::Float(m) => *x <= *m,
                _ => true,
            });
            min_ok && max_ok
        }
        _ => true,
    }
}
