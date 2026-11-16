//! A compiled schema loaded at run time, for the tools that read data without generated
//! code (`gravenc inspect`, `scan`, `to-json`, `from-json`).
//!
//! [`Schema::load`] parses and checks a `.graven` file against its committed lock and turns
//! every table into a [`graven_dynamic::TableDef`], so `graven_dynamic::read` / `write`
//! can decode and encode messages of any table. Schema ids come from the lock, like in
//! generated code.

use crate::build::{analyze_set, lock_path, own_locks, with_imports};
use crate::ir::{self, Ir, Prim, TableId};
use crate::lock::{Lock, read_lock};
use graven_dynamic as dynm;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// A checked schema, its lock and a run-time model of each table.
pub struct Schema {
    pub path: PathBuf,
    pub ir: Ir,
    pub lock: Lock,
    tables: Vec<&'static dynm::TableDef>,
    validation: dynm::validate::ValidationSchema,
    by_id: HashMap<u64, TableId>,
}

impl Schema {
    /// Loads `path` and its lock (`<path>.lock`). The lock must exist and match the schema,
    /// as for `gravenc check --locked`: schema ids are only meaningful once recorded.
    pub fn load(path: &Path) -> Result<Schema, String> {
        let lp = lock_path(path);
        let lock = read_lock(&lp)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("{}: lock missing; run: gravenc update {} --init", lp.display(), path.display()))?;
        // The schemas it imports are found under its directory.
        let dir = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new(".")).to_path_buf();
        let set = with_imports(&[path.to_path_buf()], &[dir]).map_err(|lines| lines.join("\n"))?;
        let a = analyze_set(&set, &own_locks).into_iter().next().expect("the schema itself").map_err(|lines| lines.join("\n"))?;
        if crate::lock::lock_from_ir(&a.ir, Some(&lock)) != lock {
            return Err(format!(
                "{}: lock out of date with respect to the schema; run: gravenc update {}",
                lp.display(),
                path.display()
            ));
        }
        let tables = build_model(&a.ir);
        let validation = build_validation(&a.ir, &tables);
        let mut by_id = HashMap::new();
        for (i, t) in a.ir.tables.iter().enumerate() {
            let lt = lock.tables.iter().find(|l| l.name == t.name).expect("lock matches the IR");
            let id = u64::from_str_radix(lt.schema_id.trim_start_matches("0x"), 16).expect("lock schema_id is valid");
            by_id.insert(id, TableId(i));
        }
        Ok(Schema { path: path.to_path_buf(), ir: a.ir, lock, tables, validation, by_id })
    }

    /// Table by its (unqualified) name.
    pub fn table(&self, name: &str) -> Option<TableId> {
        self.ir.tables.iter().position(|t| t.name == name).map(TableId)
    }

    /// Table whose schema id (from the lock) is `id`.
    pub fn table_by_schema_id(&self, id: u64) -> Option<TableId> {
        self.by_id.get(&id).copied()
    }

    /// Schema id of a table, from the lock.
    pub fn schema_id(&self, t: TableId) -> u64 {
        *self.by_id.iter().find(|(_, v)| **v == t).expect("every table has an id").0
    }

    /// Fully qualified name, `namespace.Table`.
    pub fn qualified(&self, t: TableId) -> String {
        format!("{}.{}", self.ir.namespace, self.ir.tables[t.0].name)
    }

    /// Run-time model of a table, for `graven_dynamic::read` / `write`.
    pub fn model(&self, t: TableId) -> &'static dynm::TableDef {
        self.tables[t.0]
    }

    /// Validation rules of the schema, for `graven_dynamic::validate`.
    pub fn validation(&self) -> &dynm::validate::ValidationSchema {
        &self.validation
    }
}

fn build_validation(ir: &Ir, tables: &[&'static dynm::TableDef]) -> dynm::validate::ValidationSchema {
    use dynm::validate as dv;
    let mut out = HashMap::new();
    for (t, td) in ir.tables.iter().zip(tables) {
        let mut fields = HashMap::new();
        for f in &t.fields {
            if f.rules.is_empty() {
                continue;
            }
            let range = f.rules.range.as_ref().map(|(min, max)| {
                (
                    min.as_ref().map(conv_bound),
                    max.as_ref().map(conv_bound),
                )
            });
            let known = if f.rules.known {
                let eid = match &f.ty {
                    ir::Ty::Enum(id) => Some(*id),
                    ir::Ty::List(elem) => match &**elem {
                        ir::Ty::Enum(id) => Some(*id),
                        _ => None,
                    },
                    _ => None,
                };
                eid.map(|id| {
                    let vals: Vec<u32> = ir.enums[id.0].values.iter().map(|(_, v)| *v).collect();
                    &*Box::leak(vals.into_boxed_slice())
                })
            } else {
                None
            };
            let checks = f.rules.checks.iter().map(|c| leak(c)).collect();
            let text = |b: Option<String>| b.map(|s| leak(&s));
            let range_text = f.rules.range.as_ref().map_or((None, None), |(min, max)| {
                (text(min.as_ref().map(ir::RangeBound::text)), text(max.as_ref().map(ir::RangeBound::text)))
            });
            let len_text = f.rules.len.map_or((None, None), |(min, max)| {
                (text(min.map(|n| n.to_string())), text(max.map(|n| n.to_string())))
            });
            fields.insert(
                f.id,
                dv::FieldRules {
                    range,
                    range_text,
                    len: f.rules.len,
                    len_text,
                    finite: f.rules.finite,
                    known,
                    checks,
                },
            );
        }
        let checks: Vec<&'static str> = t.checks.iter().map(|c| leak(c)).collect();
        if !checks.is_empty() || !fields.is_empty() {
            out.insert(td.name, dv::TableRules { checks, fields });
        }
    }
    dv::ValidationSchema { tables: out }
}

fn conv_bound(b: &ir::RangeBound) -> dynm::validate::RangeBound {
    match b {
        ir::RangeBound::Int(i) => dynm::validate::RangeBound::Int(*i),
        ir::RangeBound::UInt(u) => dynm::validate::RangeBound::UInt(*u),
        ir::RangeBound::Float(x) => dynm::validate::RangeBound::Float(*x),
    }
}

fn leak(s: &str) -> &'static str {
    Box::leak(s.to_string().into_boxed_str())
}

/// Builds the run-time model of every table. Tables are linked through `OnceLock` cells
/// created up front, so recursive schemas work. The model lives for the rest of the
/// process (it is leaked), which suits a command-line tool.
fn build_model(ir: &Ir) -> Vec<&'static dynm::TableDef> {
    let cells: Vec<&'static OnceLock<dynm::TableDef>> =
        ir.tables.iter().map(|_| &*Box::leak(Box::new(OnceLock::new()))).collect();
    let mut structs: HashMap<usize, &'static dynm::StructDef> = HashMap::new();
    let mut unions: HashMap<usize, &'static dynm::UnionDef> = HashMap::new();
    for (i, t) in ir.tables.iter().enumerate() {
        let fields = t
            .fields
            .iter()
            .map(|f| dynm::Field {
                id: f.id,
                name: leak(&f.name),
                ty: ty(ir, &f.ty, &cells, &mut structs, &mut unions),
                presence: presence(&f.presence, &f.ty, ir),
            })
            .collect();
        let def = dynm::TableDef { name: leak(&format!("{}.{}", ir.namespace, t.name)), fields };
        assert!(cells[i].set(def).is_ok(), "each table is built once");
    }
    cells.iter().map(|c| c.get().expect("set above")).collect()
}

fn prim(p: Prim) -> dynm::Ty {
    match p {
        Prim::U8 => dynm::Ty::U8,
        Prim::U16 => dynm::Ty::U16,
        Prim::U32 => dynm::Ty::U32,
        Prim::U64 => dynm::Ty::U64,
        Prim::U128 => dynm::Ty::U128,
        Prim::I8 => dynm::Ty::I8,
        Prim::I16 => dynm::Ty::I16,
        Prim::I32 => dynm::Ty::I32,
        Prim::I64 => dynm::Ty::I64,
        Prim::I128 => dynm::Ty::I128,
        Prim::F32 => dynm::Ty::F32,
        Prim::F64 => dynm::Ty::F64,
    }
}

fn ty(
    ir: &Ir,
    t: &ir::Ty,
    cells: &[&'static OnceLock<dynm::TableDef>],
    structs: &mut HashMap<usize, &'static dynm::StructDef>,
    unions: &mut HashMap<usize, &'static dynm::UnionDef>,
) -> dynm::Ty {
    match t {
        ir::Ty::Scalar(p) => prim(*p),
        ir::Ty::Bool => dynm::Ty::Bool,
        ir::Ty::Enum(id) => dynm::Ty::Enum { base: Box::new(prim(ir.enums[id.0].base)) },
        ir::Ty::Struct(id) => {
            if let Some(s) = structs.get(&id.0) {
                return dynm::Ty::Struct(s);
            }
            let s = &ir.structs[id.0];
            let fields = s.fields.iter().map(|(n, ft)| (leak(n), ty(ir, ft, cells, structs, unions))).collect();
            let def: &'static dynm::StructDef = Box::leak(Box::new(dynm::StructDef {
                name: leak(&format!("{}.{}", ir.namespace, s.name)),
                fields,
                size: s.size,
            }));
            structs.insert(id.0, def);
            dynm::Ty::Struct(def)
        }
        ir::Ty::Str => dynm::Ty::Str,
        ir::Ty::Bytes => dynm::Ty::Bytes,
        ir::Ty::List(e) => dynm::Ty::List(Box::new(ty(ir, e, cells, structs, unions))),
        ir::Ty::Table(id) => dynm::Ty::Table(dynm::TableLink::cell(cells[id.0])),
        ir::Ty::Union(id) => {
            if let Some(u) = unions.get(&id.0) {
                return dynm::Ty::Union(u);
            }
            let u = &ir.unions[id.0];
            let variants =
                u.variants.iter().map(|(n, tag, tid)| (*tag, leak(n), dynm::TableLink::cell(cells[tid.0]))).collect();
            let def: &'static dynm::UnionDef = Box::leak(Box::new(dynm::UnionDef {
                name: leak(&format!("{}.{}", ir.namespace, u.name)),
                variants,
            }));
            unions.insert(id.0, def);
            dynm::Ty::Union(def)
        }
    }
}

fn presence(p: &ir::Presence, t: &ir::Ty, ir: &Ir) -> dynm::Presence {
    match p {
        ir::Presence::Required => dynm::Presence::Required,
        ir::Presence::Optional => dynm::Presence::Optional,
        ir::Presence::Reserved { .. } => dynm::Presence::Reserved,
        ir::Presence::Default(v) => dynm::Presence::Default(default_value(v, t, ir)),
    }
}

fn default_value(v: &ir::Value, t: &ir::Ty, ir: &Ir) -> dynm::Value {
    match (v, t) {
        (ir::Value::Bool(b), _) => dynm::Value::Bool(*b),
        (ir::Value::Enum { raw, .. }, _) => dynm::Value::UInt(*raw as u128),
        (ir::Value::Int(i), ir::Ty::Scalar(p)) if p.is_float() => float(*i as f64, *p),
        (ir::Value::UInt(u), ir::Ty::Scalar(p)) if p.is_float() => float(*u as f64, *p),
        (ir::Value::UInt(u), _) => dynm::Value::UInt(*u),
        (ir::Value::Int(i), _) => dynm::Value::Int(*i),
        (ir::Value::Float(x), ir::Ty::Scalar(p)) => float(*x, *p),
        (ir::Value::Float(x), _) => panic!("float default {x} on non-float type {}", ir.ty_name(t)),
    }
}

fn float(x: f64, p: Prim) -> dynm::Value {
    if p == Prim::F32 { dynm::Value::F32(x as f32) } else { dynm::Value::F64(x) }
}
