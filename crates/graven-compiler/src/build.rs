//! The `build.rs` API ([`Build`]) and the pipeline shared with `gravenc` ([`analyze`],
//! [`finding_line`]).
//!
//! ```no_run
//! // in the build.rs `fn main`:
//! graven_compiler::build::Build::new()
//!     .schema("schemas/bank.graven") // lock: schemas/bank.graven.lock
//!     .compile()                   // writes $OUT_DIR/bank.rs
//!     .unwrap_or_else(|e| panic!("{e}"));
//! ```
//!
//! The generated code goes in a module of its own (see "Safety" in [`crate::codegen`]):
//!
//! ```ignore
//! pub mod bank {
//!     include!(concat!(env!("OUT_DIR"), "/bank.rs"));
//! }
//! ```
//!
//! [`Build::compile`] **never writes the lock**: the lock is committed and only changes with
//! `gravenc update`, which is what applies the compatibility check. The build fails if the lock
//! does not exist, if it is out of date (regenerating it would change something), if the
//! compatibility check has an error or an unconfirmed warning, or if the schema fails parse/check.

use std::fmt;
use std::path::{Path, PathBuf};

use crate::ast::Span;
use crate::codegen::{GenOptions, generate_with_options};
use crate::compat::{Finding, compat};
use crate::ir::Ir;
use crate::lock::{Lock, LockError, lock_from_ir, read_lock, reserved_types};
use crate::check::check_with_imports;
use crate::{check, parse};
use std::collections::HashMap;

/// Path of a schema's lock: `<schema>.lock` (`bank.graven` ⇒ `bank.graven.lock`).
pub fn lock_path(schema: &Path) -> PathBuf {
    let mut s = schema.as_os_str().to_os_string();
    s.push(".lock");
    PathBuf::from(s)
}

/// `file:line:column`.
fn loc(file: &Path, span: Span) -> String {
    format!("{}:{}:{}", file.display(), span.line, span.col)
}

/// A checked schema: the IR and the compatibility findings against the baseline lock (empty without one).
pub struct Analysis {
    pub ir: Ir,
    /// Position of the `namespace` (for findings without a position of their own, like `namespace-changed`).
    pub namespace_span: Span,
    pub findings: Vec<Finding>,
}

impl Analysis {
    /// Some finding blocks compilation (an error or an unconfirmed warning).
    pub fn blocking(&self) -> bool {
        crate::compat::blocking(&self.findings)
    }

    /// One line per finding ([`finding_line`]), in the order of [`compat`].
    pub fn finding_lines(&self, file: &Path) -> Vec<String> {
        self.findings.iter().map(|f| finding_line(file, self.namespace_span, f)).collect()
    }
}

/// Parse + check of `src` (the content of `file`) against `baseline` (types of reserved fields and
/// compatibility). Parse/check errors come back as `file:line:column: message` lines.
pub fn analyze(file: &Path, src: &str, baseline: Option<&Lock>) -> Result<Analysis, Vec<String>> {
    let ast = parse(src).map_err(|e| vec![format!("{}: {}", loc(file, e.span), e.msg)])?;
    let ir = match baseline {
        Some(l) => check(&ast, &reserved_types(l)),
        None => check(&ast, &|_: &str, _: u16| None),
    }
    .map_err(|ds| ds.iter().map(|d| format!("{}: {}", loc(file, d.span), d.msg)).collect::<Vec<_>>())?;
    let findings = baseline.map(|l| compat(l, &ir)).unwrap_or_default();
    Ok(Analysis { ir, namespace_span: ast.namespace.1, findings })
}

/// The lock to compare a file with, for [`analyze_set`]: `Ok(None)` for a new schema.
pub type Baseline<'a> = &'a dyn Fn(&Path) -> Result<Option<Lock>, String>;

/// Every file's own lock (`<schema>.lock`), for [`analyze_set`].
pub fn own_locks(file: &Path) -> Result<Option<Lock>, String> {
    read_lock(&lock_path(file)).map_err(|e| e.to_string())
}

/// Parses and checks every file of `files` together, resolving `import`s among them: an
/// imported namespace is checked before the files that import it. Each file is compared with
/// its `baseline`. The result is in the order of `files`; an error is a list of
/// `file:line:column: message` lines. Two files declaring the same namespace, and import
/// cycles, are errors.
pub fn analyze_set(files: &[PathBuf], baseline: Baseline<'_>) -> Vec<Result<Analysis, Vec<String>>> {
    let n = files.len();
    let mut out: Vec<Option<Result<Analysis, Vec<String>>>> = (0..n).map(|_| None).collect();
    let mut asts: Vec<Option<crate::ast::Schema>> = Vec::with_capacity(n);
    for (i, f) in files.iter().enumerate() {
        match std::fs::read_to_string(f) {
            Err(e) => {
                out[i] = Some(Err(vec![format!("{}: could not read: {e}", f.display())]));
                asts.push(None);
            }
            Ok(src) => match parse(&src) {
                Ok(a) => asts.push(Some(a)),
                Err(e) => {
                    out[i] = Some(Err(vec![format!("{}: {}", loc(f, e.span), e.msg)]));
                    asts.push(None);
                }
            },
        }
    }
    // One namespace, one file.
    let mut by_ns: HashMap<&str, usize> = HashMap::new();
    for (i, a) in asts.iter().enumerate() {
        let Some(a) = a else { continue };
        if let Some(&j) = by_ns.get(a.namespace.0.as_str()) {
            for (k, other) in [(i, j), (j, i)] {
                let line = format!(
                    "{}: namespace `{}` is also declared in {}; each namespace is declared in exactly one file",
                    loc(&files[k], asts[k].as_ref().unwrap().namespace.1),
                    a.namespace.0,
                    files[other].display()
                );
                match &mut out[k] {
                    Some(Err(lines)) => lines.push(line),
                    _ => out[k] = Some(Err(vec![line])),
                }
            }
        } else {
            by_ns.insert(&a.namespace.0, i);
        }
    }
    // Imported namespaces first; cycles are errors.
    let mut order = Vec::new();
    let mut state = vec![0u8; n]; // 0 new, 1 in progress, 2 done
    fn visit(
        i: usize,
        asts: &[Option<crate::ast::Schema>],
        by_ns: &HashMap<&str, usize>,
        state: &mut [u8],
        order: &mut Vec<usize>,
        path: &mut Vec<usize>,
        cycles: &mut Vec<Vec<usize>>,
    ) {
        if state[i] == 2 {
            return;
        }
        if state[i] == 1 {
            let at = path.iter().position(|&p| p == i).unwrap_or(0);
            cycles.push(path[at..].to_vec());
            return;
        }
        state[i] = 1;
        path.push(i);
        if let Some(a) = &asts[i] {
            for im in &a.imports {
                // Importing its own namespace is reported by the checker, not as a cycle.
                if let Some(&j) = by_ns.get(im.namespace.as_str()).filter(|&&j| j != i) {
                    visit(j, asts, by_ns, state, order, path, cycles);
                }
            }
        }
        path.pop();
        state[i] = 2;
        order.push(i);
    }
    let mut cycles = Vec::new();
    for i in 0..n {
        visit(i, &asts, &by_ns, &mut state, &mut order, &mut Vec::new(), &mut cycles);
    }
    for cycle in &cycles {
        let names: Vec<String> = cycle.iter().chain(cycle.first()).map(|&k| asts[k].as_ref().unwrap().namespace.0.clone()).collect();
        for &k in cycle {
            let line = format!("{}: import cycle: {}", files[k].display(), names.join(" → "));
            match &mut out[k] {
                Some(Err(lines)) => lines.push(line),
                _ => out[k] = Some(Err(vec![line])),
            }
        }
    }
    let mut irs: HashMap<String, Ir> = HashMap::new();
    let mut failed: HashMap<String, PathBuf> = HashMap::new();
    for i in order {
        let file = &files[i];
        let Some(ast) = &asts[i] else { continue };
        if out[i].is_some() {
            failed.insert(ast.namespace.0.clone(), file.clone());
            continue;
        }
        let broken: Vec<String> = ast
            .imports
            .iter()
            .filter_map(|im| failed.get(&im.namespace).map(|f| (im, f)))
            .map(|(im, f)| format!("{}: imported namespace `{}` ({}) has errors", loc(file, im.span), im.namespace, f.display()))
            .collect();
        if !broken.is_empty() {
            failed.insert(ast.namespace.0.clone(), file.clone());
            out[i] = Some(Err(broken));
            continue;
        }
        let base = match baseline(file) {
            Ok(b) => b,
            Err(e) => {
                failed.insert(ast.namespace.0.clone(), file.clone());
                out[i] = Some(Err(vec![e]));
                continue;
            }
        };
        let imports: HashMap<String, Ir> =
            ast.imports.iter().filter_map(|im| irs.get(&im.namespace).map(|ir| (im.namespace.clone(), ir.clone()))).collect();
        let checked = match &base {
            Some(l) => check_with_imports(ast, &reserved_types(l), &imports),
            None => check_with_imports(ast, &|_: &str, _: u16| None, &imports),
        };
        match checked {
            Err(ds) => {
                failed.insert(ast.namespace.0.clone(), file.clone());
                out[i] = Some(Err(ds.iter().map(|d| format!("{}: {}", loc(file, d.span), d.msg)).collect()));
            }
            Ok(ir) => {
                let findings = base.as_ref().map(|l| compat(l, &ir)).unwrap_or_default();
                irs.insert(ast.namespace.0.clone(), ir.clone());
                out[i] = Some(Ok(Analysis { ir, namespace_span: ast.namespace.1, findings }));
            }
        }
    }
    out.into_iter().map(|r| r.unwrap_or_else(|| Err(Vec::new()))).collect()
}

/// `files` plus the files that declare the namespaces they import (transitively), found among
/// the `.graven` files under `dirs` (recursively). A namespace declared by none is left for the
/// checker to report; one declared by several files is an error.
pub fn with_imports(files: &[PathBuf], dirs: &[PathBuf]) -> Result<Vec<PathBuf>, Vec<String>> {
    fn scan(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        let mut entries: Vec<_> = rd.filter_map(|e| e.ok()).map(|e| e.path()).collect();
        entries.sort();
        for p in entries {
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if p.is_dir() {
                if !name.starts_with('.') && name != "target" {
                    scan(&p, out);
                }
            } else if name.ends_with(".graven") {
                out.push(p);
            }
        }
    }
    let mut candidates = Vec::new();
    for d in dirs {
        scan(d, &mut candidates);
    }
    let parsed: Vec<(PathBuf, crate::ast::Schema)> = candidates
        .into_iter()
        .filter_map(|p| std::fs::read_to_string(&p).ok().and_then(|s| parse(&s).ok()).map(|a| (p, a)))
        .collect();
    let same = |a: &Path, b: &Path| a.canonicalize().ok() == b.canonicalize().ok();
    let mut set: Vec<PathBuf> = files.to_vec();
    let mut errors = Vec::new();
    let mut i = 0;
    while i < set.len() {
        let imports = std::fs::read_to_string(&set[i]).ok().and_then(|s| parse(&s).ok()).map(|a| a.imports).unwrap_or_default();
        for im in imports {
            let declared_in_set = set.iter().any(|f| {
                std::fs::read_to_string(f).ok().and_then(|s| parse(&s).ok()).is_some_and(|a| a.namespace.0 == im.namespace)
            });
            if declared_in_set {
                continue;
            }
            let found: Vec<&PathBuf> = parsed.iter().filter(|(_, a)| a.namespace.0 == im.namespace).map(|(p, _)| p).collect();
            match found.as_slice() {
                [] => {}
                [one] => {
                    if !set.iter().any(|f| same(f, one)) {
                        set.push((*one).clone());
                    }
                }
                many => errors.push(format!(
                    "{}: namespace `{}` is declared in several files ({}); each namespace is declared in exactly one file",
                    set[i].display(),
                    im.namespace,
                    many.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(", ")
                )),
            }
        }
        i += 1;
    }
    if errors.is_empty() { Ok(set) } else { Err(errors) }
}

/// `file:line:column: message` for an error and an unconfirmed warning (both block);
/// `file:line:column: warning: message` for a confirmed warning. The message names the rule.
/// A finding without a position (line 0) uses `namespace_span`.
pub fn finding_line(file: &Path, namespace_span: Span, f: &Finding) -> String {
    let span = if f.span().line == 0 { namespace_span } else { f.span() };
    if f.is_blocking() { format!("{}: {}", loc(file, span), f.msg()) } else { format!("{}: warning: {}", loc(file, span), f.msg()) }
}

/// One table's schema id, for [`schema_id_collisions`].
#[derive(Debug, Clone)]
pub struct SchemaIdEntry {
    pub id: u64,
    /// `namespace.Table`.
    pub qualified: String,
    pub file: PathBuf,
}

/// The schema id of every table and service of a schema, as the lock records it.
pub fn schema_ids(file: &Path, ir: &Ir, lock: &Lock) -> Vec<SchemaIdEntry> {
    let lock = lock_from_ir(ir, Some(lock));
    let mut entries: Vec<SchemaIdEntry> = lock
        .tables
        .iter()
        // Imported tables are counted once, by the schema that declares them.
        .filter(|t| !t.name.contains('.'))
        .map(|t| SchemaIdEntry {
            id: u64::from_str_radix(t.schema_id.trim_start_matches("0x"), 16).unwrap_or(0),
            qualified: format!("{}.{}", ir.namespace, t.name),
            file: file.to_path_buf(),
        })
        .collect();
    for s in &lock.services {
        entries.push(SchemaIdEntry {
            id: u64::from_str_radix(s.service_id.trim_start_matches("0x"), 16).unwrap_or(0),
            qualified: format!("{}.{}", ir.namespace, s.name),
            file: file.to_path_buf(),
        });
    }
    entries
}

/// Two differently named tables with the same schema id: their envelopes could not be told
/// apart. One line per collision. (The same qualified name twice cannot happen within one
/// compilation: a namespace is declared by one file.)
pub fn schema_id_collisions(entries: &[SchemaIdEntry]) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen: std::collections::HashMap<u64, &SchemaIdEntry> = std::collections::HashMap::new();
    for e in entries {
        match seen.get(&e.id) {
            Some(first) if first.qualified != e.qualified => out.push(format!(
                "{}: schema id {:#018x} of `{}` is also the id of `{}` ({}); envelopes of the two types could not be told apart. \
                 Rename one of the tables or change its namespace.",
                e.file.display(),
                e.id,
                e.qualified,
                first.qualified,
                first.file.display()
            )),
            Some(_) => {}
            None => {
                seen.insert(e.id, e);
            }
        }
    }
    out
}

/// Error of [`Build::compile`]. `Display` gives the full message (one line per problem).
#[derive(Debug)]
pub enum CompileError {
    /// Nenhum [`Build::schema`].
    NoSchema,
    /// No [`Build::out_dir`] and no `OUT_DIR` in the environment.
    NoOutDir,
    Io { path: PathBuf, source: std::io::Error },
    Lock(LockError),
    /// The lock does not exist.
    MissingLock { schema: PathBuf, lock: PathBuf },
    /// Regenerating the lock would change its content.
    StaleLock { schema: PathBuf, lock: PathBuf },
    /// Parse, check or compatibility: `file:line:column: message` lines.
    Diagnostics(Vec<String>),
}

impl fmt::Display for CompileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CompileError::NoSchema => write!(f, "graven Build: nenhum schema (chame `.schema(\"x.graven\")`)"),
            CompileError::NoOutDir => write!(f, "graven Build: no output directory (`.out_dir(..)` or the OUT_DIR variable)"),
            CompileError::Io { path, source } => write!(f, "{}: {source}", path.display()),
            CompileError::Lock(e) => write!(f, "{e}"),
            CompileError::MissingLock { schema, lock } => {
                write!(
                    f,
                    "{}: lock missing; new schema: gravenc update --init {s}; moved/renamed schema: gravenc update {s} --from <old lock>",
                    lock.display(),
                    s = schema.display()
                )
            }
            CompileError::StaleLock { schema, lock } => write!(
                f,
                "{}: lock out of date with respect to the schema (the build never writes the lock); run: gravenc update {}",
                lock.display(),
                schema.display()
            ),
            CompileError::Diagnostics(lines) => write!(f, "{}", lines.join("\n")),
        }
    }
}

impl std::error::Error for CompileError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            CompileError::Io { source, .. } => Some(source),
            CompileError::Lock(e) => Some(e),
            _ => None,
        }
    }
}

/// Compiles `.graven` schemas in a `build.rs`: generates `<out>/<file without .graven>.rs` from the
/// schema and the committed lock (`<schema>.lock`). See the module docs.
#[derive(Debug, Clone)]
pub struct Build {
    schemas: Vec<PathBuf>,
    out_dir: Option<PathBuf>,
    /// Emits `cargo:rerun-if-changed` / `cargo:warning` (off for `gravenc gen`).
    cargo: bool,
    opts: GenOptions,
}

impl Default for Build {
    fn default() -> Self {
        Build::new()
    }
}

impl Build {
    pub fn new() -> Self {
        Build { schemas: Vec::new(), out_dir: None, cargo: true, opts: GenOptions::default() }
    }

    /// Emit RPC client stubs (`<Service>Client`, `Response`, `StreamResponse`) for any `service`
    /// declarations in the compiled schemas.
    pub fn client(mut self) -> Self {
        self.opts.client = true;
        self
    }

    /// Emit RPC server edge helpers (`<Service>Call`, `<SERVICE>_SERVICE_ID`, `<service>_timeouts`)
    /// for any `service` declarations in the compiled schemas.
    pub fn server(mut self) -> Self {
        self.opts.server = true;
        self
    }

    /// Adds a schema; the lock is `"<path>.lock"`. May be called several times.
    pub fn schema(mut self, path: impl AsRef<Path>) -> Self {
        self.schemas.push(path.as_ref().to_path_buf());
        self
    }

    /// Output directory; default: the `OUT_DIR` environment variable.
    pub fn out_dir(mut self, dir: impl AsRef<Path>) -> Self {
        self.out_dir = Some(dir.as_ref().to_path_buf());
        self
    }

    /// Without the `cargo:` lines (for use outside `build.rs`, like `gravenc gen`).
    #[doc(hidden)]
    pub fn quiet(mut self) -> Self {
        self.cargo = false;
        self
    }

    /// Generates `<out>/<file without .graven>.rs` for each schema. Emits `cargo:rerun-if-changed`
    /// for the schema and the lock (before anything else, so cargo reruns after a fix)
    /// and `cargo:warning` for confirmed warnings. **Never writes the lock.** Stops at the
    /// first schema with an error.
    pub fn compile(self) -> Result<(), CompileError> {
        if self.schemas.is_empty() {
            return Err(CompileError::NoSchema);
        }
        let out = match &self.out_dir {
            Some(d) => d.clone(),
            None => std::env::var_os("OUT_DIR").map(PathBuf::from).ok_or(CompileError::NoOutDir)?,
        };
        // The output is `<out>/<stem>.rs`: two schemas with the same file name (e.g.
        // `bank.graven` and `evo/bank.graven`) would silently overwrite each other.
        let mut seen: std::collections::HashMap<std::ffi::OsString, &Path> = std::collections::HashMap::new();
        for schema in &self.schemas {
            let stem = schema.file_stem().unwrap_or_default().to_os_string();
            if let Some(prev) = seen.insert(stem, schema) {
                return Err(CompileError::Diagnostics(vec![format!(
                    "{}: generates the same file as {} (`<name>.rs` in OUT_DIR); rename one of the schemas",
                    schema.display(),
                    prev.display()
                )]));
            }
        }
        if self.cargo {
            for schema in &self.schemas {
                println!("cargo:rerun-if-changed={}", schema.display());
                println!("cargo:rerun-if-changed={}", lock_path(schema).display());
            }
        }
        let mut locks: HashMap<PathBuf, Lock> = HashMap::new();
        for schema in &self.schemas {
            let lock_file = lock_path(schema);
            let lock = read_lock(&lock_file)
                .map_err(CompileError::Lock)?
                .ok_or_else(|| CompileError::MissingLock { schema: schema.to_path_buf(), lock: lock_file.clone() })?;
            locks.insert(schema.clone(), lock);
        }
        // Imports are resolved among the schemas of this build.
        let results = analyze_set(&self.schemas, &|f: &Path| Ok(locks.get(f).cloned()));
        let mut ids = Vec::new();
        for (schema, result) in self.schemas.iter().zip(results) {
            let a = result.map_err(CompileError::Diagnostics)?;
            let lock = &locks[schema];
            if a.blocking() {
                return Err(CompileError::Diagnostics(a.finding_lines(schema)));
            }
            if self.cargo {
                for l in a.finding_lines(schema) {
                    println!("cargo:warning={l}");
                }
            }
            if lock_from_ir(&a.ir, Some(lock)) != *lock {
                return Err(CompileError::StaleLock { schema: schema.to_path_buf(), lock: lock_path(schema) });
            }
            let name = schema.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let stem = schema.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let target = out.join(format!("{stem}.rs"));
            std::fs::write(&target, generate_with_options(&a.ir, lock, &name, self.opts))
                .map_err(|source| CompileError::Io { path: target, source })?;
            ids.extend(schema_ids(schema, &a.ir, lock));
        }
        let collisions = schema_id_collisions(&ids);
        if !collisions.is_empty() {
            return Err(CompileError::Diagnostics(collisions));
        }
        Ok(())
    }
}
