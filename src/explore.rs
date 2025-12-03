//! Interactive MIR Graph Explorer
//!
//! Generates a self-contained HTML file with an interactive graph visualization
//! that allows step-by-step exploration of MIR control flow with path tracking
//! and contextual annotations.

use std::fs::File;
use std::io::{self, BufWriter, Write};

extern crate serde;
extern crate serde_json;
use serde::Serialize;

extern crate rustc_middle;
use rustc_middle::ty::TyCtxt;

extern crate rustc_session;
use rustc_session::config::{OutFileName, OutputType};

extern crate stable_mir;
use stable_mir::mir::{
    BasicBlock, BinOp, Body, Operand, Place, Rvalue, Statement, StatementKind, Terminator,
    TerminatorKind, UnwindAction,
};
use stable_mir::ty::IndexedVal;
use stable_mir::CrateDef;

use crate::printer::{collect_smir, SmirJson};
use crate::MonoItemKind;

// =============================================================================
// Explorer Data Model
// =============================================================================

/// Complete data for the explorer, serialized to JSON for the browser
#[derive(Serialize)]
pub struct ExplorerData {
    name: String,
    functions: Vec<ExplorerFunction>,
}

#[derive(Serialize)]
pub struct ExplorerFunction {
    pub name: String,
    pub short_name: String,
    pub blocks: Vec<ExplorerBlock>,
    pub locals: Vec<String>, // Type descriptions for each local
    pub entry_block: usize,
}

#[derive(Serialize)]
pub struct ExplorerBlock {
    id: usize,
    statements: Vec<ExplorerStmt>,
    terminator: ExplorerTerminator,
    predecessors: Vec<usize>,
    role: BlockRole,
    summary: String,
}

#[derive(Serialize)]
struct ExplorerStmt {
    mir: String,
    annotation: String,
}

#[derive(Serialize)]
struct ExplorerTerminator {
    kind: String,
    mir: String,
    annotation: String,
    edges: Vec<ExplorerEdge>,
}

#[derive(Serialize)]
struct ExplorerEdge {
    target: usize,
    label: String,
    kind: EdgeKind,
    annotation: String,
}

#[derive(Serialize, Clone, Copy)]
#[serde(rename_all = "lowercase")]
enum EdgeKind {
    Normal,
    Cleanup,
    Otherwise,
    Branch,
}

#[derive(Serialize, Clone, Copy)]
#[serde(rename_all = "lowercase")]
enum BlockRole {
    Entry,
    Exit,
    BranchPoint,
    MergePoint,
    Linear,
    Cleanup,
}

// =============================================================================
// Entry Point
// =============================================================================

/// Entry point to generate the explorer HTML file
pub fn emit_explore(tcx: TyCtxt<'_>) {
    let smir = collect_smir(tcx);
    let html = generate_explore_html(&smir);

    match tcx.output_filenames(()).path(OutputType::Mir) {
        OutFileName::Stdout => {
            write!(io::stdout(), "{}", html).expect("Failed to write HTML");
        }
        OutFileName::Real(path) => {
            let out_path = path.with_extension("explore.html");
            let mut b = BufWriter::new(
                File::create(&out_path)
                    .unwrap_or_else(|e| panic!("Failed to create {}: {}", out_path.display(), e)),
            );
            write!(b, "{}", html).expect("Failed to write explore.html");
            eprintln!("Wrote {}", out_path.display());
        }
    }
}

// =============================================================================
// Data Building
// =============================================================================

fn build_explorer_data(smir: &SmirJson) -> ExplorerData {
    let mut functions = Vec::new();

    for item in &smir.items {
        let MonoItemKind::MonoItemFn { name, body, .. } = &item.mono_item_kind else {
            continue;
        };

        let Some(body) = body else { continue };

        // Skip standard library functions
        if name.contains("std::") || name.contains("core::") {
            continue;
        }

        functions.push(build_explorer_function(name, body));
    }

    ExplorerData {
        name: smir.name.clone(),
        functions,
    }
}

pub fn build_explorer_function(name: &str, body: &Body) -> ExplorerFunction {
    let short_name = short_fn_name(name);

    // Build blocks with edges
    let mut blocks: Vec<ExplorerBlock> = body
        .blocks
        .iter()
        .enumerate()
        .map(|(id, block)| build_explorer_block(id, block, &short_name))
        .collect();

    // Compute predecessors
    let mut predecessors: Vec<Vec<usize>> = vec![Vec::new(); blocks.len()];
    for (id, block) in blocks.iter().enumerate() {
        for edge in &block.terminator.edges {
            if edge.target < predecessors.len() {
                predecessors[edge.target].push(id);
            }
        }
    }

    // Assign predecessors and compute roles
    for (id, block) in blocks.iter_mut().enumerate() {
        block.predecessors = predecessors[id].clone();
        block.role = compute_block_role(
            id,
            &block.predecessors,
            &block.terminator,
            body.blocks.len(),
        );
        block.summary = block_summary(block);
    }

    // Collect local type info
    let locals: Vec<String> = body
        .local_decls()
        .map(|(idx, decl)| format!("_{}: {}", idx, decl.ty))
        .collect();

    ExplorerFunction {
        name: name.to_string(),
        short_name,
        blocks,
        locals,
        entry_block: 0,
    }
}

fn build_explorer_block(id: usize, block: &BasicBlock, current_fn: &str) -> ExplorerBlock {
    let statements: Vec<ExplorerStmt> = block.statements.iter().map(build_explorer_stmt).collect();

    let terminator = build_explorer_terminator(&block.terminator, current_fn);

    ExplorerBlock {
        id,
        statements,
        terminator,
        predecessors: Vec::new(), // Filled in later
        role: BlockRole::Linear,  // Filled in later
        summary: String::new(),   // Filled in later
    }
}

fn build_explorer_stmt(stmt: &Statement) -> ExplorerStmt {
    let (mir, annotation) = match &stmt.kind {
        StatementKind::Assign(place, rvalue) => {
            let mir = format!("{} = {}", render_place(place), render_rvalue(rvalue));
            let annotation = annotate_rvalue(rvalue);
            (mir, annotation)
        }
        StatementKind::StorageLive(local) => (
            format!("StorageLive(_{local})"),
            format!("Allocate stack space for _{local}"),
        ),
        StatementKind::StorageDead(local) => (
            format!("StorageDead(_{local})"),
            format!("Deallocate stack space for _{local}"),
        ),
        StatementKind::Nop => ("nop".to_string(), "No operation".to_string()),
        _ => (format!("{:?}", stmt.kind), String::new()),
    };

    ExplorerStmt { mir, annotation }
}

fn build_explorer_terminator(term: &Terminator, current_fn: &str) -> ExplorerTerminator {
    let (kind, mir, annotation, edges) = match &term.kind {
        TerminatorKind::Goto { target } => (
            "goto".to_string(),
            format!("goto bb{}", target),
            "Continue to next block".to_string(),
            vec![ExplorerEdge {
                target: *target,
                label: String::new(),
                kind: EdgeKind::Normal,
                annotation: "Continue".to_string(),
            }],
        ),

        TerminatorKind::Return {} => (
            "return".to_string(),
            "return".to_string(),
            "Return from function".to_string(),
            vec![],
        ),

        TerminatorKind::Unreachable {} => (
            "unreachable".to_string(),
            "unreachable".to_string(),
            "Unreachable code (compiler optimization)".to_string(),
            vec![],
        ),

        TerminatorKind::SwitchInt { discr, targets } => {
            let discr_str = render_operand(discr);
            let mut edges = Vec::new();

            for (val, target) in targets.branches() {
                edges.push(ExplorerEdge {
                    target,
                    label: val.to_string(),
                    kind: EdgeKind::Branch,
                    annotation: format!("If {} == {}", discr_str, val),
                });
            }

            edges.push(ExplorerEdge {
                target: targets.otherwise(),
                label: "else".to_string(),
                kind: EdgeKind::Otherwise,
                annotation: "Otherwise (no match)".to_string(),
            });

            (
                "switch".to_string(),
                format!("switchInt({})", discr_str),
                format!("Branch based on value of {}", discr_str),
                edges,
            )
        }

        TerminatorKind::Call {
            func,
            args,
            destination,
            target,
            unwind,
        } => {
            let func_name = extract_call_name(func);
            let args_str: Vec<String> = args.iter().map(|a| render_operand(&a.clone())).collect();
            let dest = render_place(destination);

            let is_recursive = func_name == current_fn;
            let annotation = if is_recursive {
                format!("RECURSIVE call to {}", func_name)
            } else {
                format!("Call {}", func_name)
            };

            let mut edges = Vec::new();
            if let Some(t) = target {
                edges.push(ExplorerEdge {
                    target: *t,
                    label: "return".to_string(),
                    kind: EdgeKind::Normal,
                    annotation: format!("After {} returns", func_name),
                });
            }
            if let UnwindAction::Cleanup(t) = unwind {
                edges.push(ExplorerEdge {
                    target: *t,
                    label: "unwind".to_string(),
                    kind: EdgeKind::Cleanup,
                    annotation: "If call panics (cleanup)".to_string(),
                });
            }

            (
                "call".to_string(),
                format!("{} = {}({})", dest, func_name, args_str.join(", ")),
                annotation,
                edges,
            )
        }

        TerminatorKind::Assert {
            cond,
            expected,
            target,
            unwind,
            ..
        } => {
            let cond_str = render_operand(cond);
            let annotation = if *expected {
                format!("Panic if {} is false", cond_str)
            } else {
                format!("Panic if {} is true", cond_str)
            };

            let mut edges = vec![ExplorerEdge {
                target: *target,
                label: "ok".to_string(),
                kind: EdgeKind::Normal,
                annotation: "Assertion passed".to_string(),
            }];

            if let UnwindAction::Cleanup(t) = unwind {
                edges.push(ExplorerEdge {
                    target: *t,
                    label: "panic".to_string(),
                    kind: EdgeKind::Cleanup,
                    annotation: "Assertion failed (panic)".to_string(),
                });
            }

            (
                "assert".to_string(),
                format!("assert({} == {})", cond_str, expected),
                annotation,
                edges,
            )
        }

        TerminatorKind::Drop {
            place,
            target,
            unwind,
        } => {
            let place_str = render_place(place);
            let mut edges = vec![ExplorerEdge {
                target: *target,
                label: String::new(),
                kind: EdgeKind::Normal,
                annotation: "After drop completes".to_string(),
            }];

            if let UnwindAction::Cleanup(t) = unwind {
                edges.push(ExplorerEdge {
                    target: *t,
                    label: "unwind".to_string(),
                    kind: EdgeKind::Cleanup,
                    annotation: "If drop panics".to_string(),
                });
            }

            (
                "drop".to_string(),
                format!("drop({})", place_str),
                format!("Drop {}", place_str),
                edges,
            )
        }

        TerminatorKind::Resume {} => (
            "resume".to_string(),
            "resume".to_string(),
            "Resume unwinding (propagate panic)".to_string(),
            vec![],
        ),

        TerminatorKind::Abort {} => (
            "abort".to_string(),
            "abort".to_string(),
            "Abort the program".to_string(),
            vec![],
        ),

        _ => (
            "other".to_string(),
            format!("{:?}", term.kind),
            String::new(),
            vec![],
        ),
    };

    ExplorerTerminator {
        kind,
        mir,
        annotation,
        edges,
    }
}

fn compute_block_role(
    id: usize,
    predecessors: &[usize],
    terminator: &ExplorerTerminator,
    _total_blocks: usize,
) -> BlockRole {
    let in_count = predecessors.len();
    let out_count = terminator.edges.len();

    if id == 0 {
        BlockRole::Entry
    } else if terminator.kind == "return" || terminator.kind == "unreachable" {
        BlockRole::Exit
    } else if terminator
        .edges
        .iter()
        .any(|e| matches!(e.kind, EdgeKind::Cleanup))
        && terminator.kind != "call"
        && terminator.kind != "assert"
    {
        BlockRole::Cleanup
    } else if out_count > 1 {
        BlockRole::BranchPoint
    } else if in_count > 1 {
        BlockRole::MergePoint
    } else {
        BlockRole::Linear
    }
}

fn block_summary(block: &ExplorerBlock) -> String {
    match block.role {
        BlockRole::Entry => "Entry point".to_string(),
        BlockRole::Exit => {
            if block.terminator.kind == "return" {
                "Function returns".to_string()
            } else {
                "Unreachable code".to_string()
            }
        }
        BlockRole::BranchPoint => format!("Branches: {}", block.terminator.annotation),
        BlockRole::MergePoint => {
            format!("Merge point ({} incoming paths)", block.predecessors.len())
        }
        BlockRole::Cleanup => "Cleanup/unwind handler".to_string(),
        BlockRole::Linear => {
            if !block.statements.is_empty() {
                block.statements[0].annotation.clone()
            } else {
                block.terminator.annotation.clone()
            }
        }
    }
}

// =============================================================================
// Rendering Helpers
// =============================================================================

fn render_place(place: &Place) -> String {
    let mut s = format!("_{}", place.local);
    for proj in &place.projection {
        match proj {
            stable_mir::mir::ProjectionElem::Deref => s = format!("(*{})", s),
            stable_mir::mir::ProjectionElem::Field(idx, _) => s = format!("{}.{}", s, idx),
            stable_mir::mir::ProjectionElem::Index(local) => s = format!("{}[_{}]", s, local),
            stable_mir::mir::ProjectionElem::Downcast(idx) => {
                s = format!("({} as #{})", s, idx.to_index())
            }
            _ => s = format!("{}.[proj]", s),
        }
    }
    s
}

fn render_operand(op: &Operand) -> String {
    match op {
        Operand::Copy(place) => render_place(place),
        Operand::Move(place) => format!("move {}", render_place(place)),
        Operand::Constant(c) => render_const(&c.const_),
    }
}

fn render_const(c: &stable_mir::ty::MirConst) -> String {
    use stable_mir::ty::ConstantKind;
    match c.kind() {
        ConstantKind::Allocated(alloc) => {
            let bytes: Vec<u8> = alloc.bytes.iter().filter_map(|&b| b).collect();
            if bytes.len() <= 8 && !bytes.is_empty() {
                let val = bytes
                    .iter()
                    .enumerate()
                    .fold(0u64, |acc, (i, &b)| acc | ((b as u64) << (i * 8)));
                format!("{}", val)
            } else {
                format!("[{} bytes]", alloc.bytes.len())
            }
        }
        ConstantKind::ZeroSized => "()".to_string(),
        _ => "const".to_string(),
    }
}

fn render_rvalue(rv: &Rvalue) -> String {
    match rv {
        Rvalue::Use(op) => render_operand(op),
        Rvalue::BinaryOp(op, lhs, rhs) | Rvalue::CheckedBinaryOp(op, lhs, rhs) => {
            format!(
                "{} {} {}",
                render_operand(lhs),
                render_binop(op),
                render_operand(rhs)
            )
        }
        Rvalue::UnaryOp(op, operand) => {
            let op_str = match op {
                stable_mir::mir::UnOp::Not => "!",
                stable_mir::mir::UnOp::Neg => "-",
                stable_mir::mir::UnOp::PtrMetadata => "metadata",
            };
            format!("{}{}", op_str, render_operand(operand))
        }
        Rvalue::Ref(_, bk, place) => {
            let prefix = match bk {
                stable_mir::mir::BorrowKind::Shared => "&",
                stable_mir::mir::BorrowKind::Mut { .. } => "&mut ",
                _ => "&?",
            };
            format!("{}{}", prefix, render_place(place))
        }
        Rvalue::Cast(_, op, ty) => format!("{} as {:?}", render_operand(op), ty.kind()),
        Rvalue::Len(place) => format!("len({})", render_place(place)),
        Rvalue::Discriminant(place) => format!("discr({})", render_place(place)),
        Rvalue::Aggregate(kind, ops) => {
            let ops_str: Vec<String> = ops.iter().map(render_operand).collect();
            format!("{:?}({})", kind, ops_str.join(", "))
        }
        _ => format!("{:?}", rv),
    }
}

fn render_binop(op: &BinOp) -> &'static str {
    match op {
        BinOp::Add | BinOp::AddUnchecked => "+",
        BinOp::Sub | BinOp::SubUnchecked => "-",
        BinOp::Mul | BinOp::MulUnchecked => "*",
        BinOp::Div => "/",
        BinOp::Rem => "%",
        BinOp::Eq => "==",
        BinOp::Ne => "!=",
        BinOp::Lt => "<",
        BinOp::Le => "<=",
        BinOp::Gt => ">",
        BinOp::Ge => ">=",
        BinOp::BitAnd => "&",
        BinOp::BitOr => "|",
        BinOp::BitXor => "^",
        BinOp::Shl | BinOp::ShlUnchecked => "<<",
        BinOp::Shr | BinOp::ShrUnchecked => ">>",
        _ => "??",
    }
}

fn annotate_rvalue(rv: &Rvalue) -> String {
    match rv {
        Rvalue::Use(Operand::Constant(_)) => "Load constant".to_string(),
        Rvalue::Use(Operand::Copy(_)) => "Copy value".to_string(),
        Rvalue::Use(Operand::Move(_)) => "Move value".to_string(),
        Rvalue::BinaryOp(op, _, _) => format!("{} operation", op_name(op)),
        Rvalue::CheckedBinaryOp(op, _, _) => format!("Checked {} (may panic)", op_name(op)),
        Rvalue::Ref(_, stable_mir::mir::BorrowKind::Shared, _) => "Shared borrow".to_string(),
        Rvalue::Ref(_, stable_mir::mir::BorrowKind::Mut { .. }, _) => "Mutable borrow".to_string(),
        Rvalue::Len(_) => "Get length".to_string(),
        Rvalue::Discriminant(_) => "Get enum discriminant".to_string(),
        Rvalue::Cast(_, _, _) => "Type cast".to_string(),
        Rvalue::Aggregate(_, _) => "Construct value".to_string(),
        _ => String::new(),
    }
}

fn op_name(op: &BinOp) -> &'static str {
    match op {
        BinOp::Add | BinOp::AddUnchecked => "Add",
        BinOp::Sub | BinOp::SubUnchecked => "Subtract",
        BinOp::Mul | BinOp::MulUnchecked => "Multiply",
        BinOp::Div => "Divide",
        BinOp::Eq => "Equality",
        BinOp::Ne => "Inequality",
        BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => "Comparison",
        _ => "Binary",
    }
}

fn extract_call_name(func: &Operand) -> String {
    match func {
        Operand::Constant(c) => {
            let ty = c.const_.ty();
            match ty.kind() {
                stable_mir::ty::TyKind::RigidTy(stable_mir::ty::RigidTy::FnDef(def, _)) => {
                    short_fn_name(&def.name())
                }
                _ => "fn".to_string(),
            }
        }
        _ => "fn".to_string(),
    }
}

fn short_fn_name(name: &str) -> String {
    let short = name.rsplit("::").next().unwrap_or(name);
    short
        .find("::h")
        .map(|i| &short[..i])
        .unwrap_or(short)
        .to_string()
}

// =============================================================================
// HTML Generation
// =============================================================================

fn generate_explore_html(smir: &SmirJson) -> String {
    let data = build_explorer_data(smir);
    let json_data = serde_json::to_string(&data).expect("Failed to serialize explorer data");

    format!(
        r##"<!DOCTYPE html>
<html>
<head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>{name} - MIR Explorer</title>
    <script src="https://unpkg.com/cytoscape@3.28.1/dist/cytoscape.min.js"></script>
    <style>
{css}
    </style>
</head>
<body>
    <header id="header">
        <h1>{name}</h1>
        <div id="fn-selector"></div>
    </header>

    <div id="path-bar">
        <span class="path-label">PATH:</span>
        <span id="path-breadcrumb"></span>
        <button id="reset-btn" onclick="explorer.reset()">Reset</button>
    </div>

    <main id="main">
        <section id="graph-panel">
            <div id="cy"></div>
        </section>

        <aside id="context-panel">
            <div id="block-header">
                <h2 id="current-block">bb0</h2>
                <span id="block-role" class="badge">ENTRY</span>
            </div>

            <div id="block-summary"></div>

            <section id="locals-section">
                <h3 onclick="toggleSection('locals-section')">Locals ▾</h3>
                <ul id="locals-list"></ul>
            </section>

            <section id="statements-section">
                <h3>Statements</h3>
                <ul id="stmt-list"></ul>
            </section>

            <section id="terminator-section">
                <h3>Terminator</h3>
                <div id="term-display"></div>
            </section>

            <section id="edges-section">
                <h3>Next</h3>
                <div id="edge-buttons"></div>
            </section>

            <section id="alt-paths-section">
                <div id="alt-paths"></div>
            </section>
        </aside>
    </main>

    <footer id="controls">
        <button onclick="explorer.goBack()" id="back-btn">← Back</button>
        <span id="step-counter">Step 1</span>
        <span class="hint">Click nodes/edges or use arrow keys</span>
    </footer>

    <script>
const EXPLORER_DATA = {json};
{js}
    </script>
</body>
</html>"##,
        name = escape_html(&smir.name),
        css = CSS_TEMPLATE,
        json = json_data,
        js = JS_TEMPLATE,
    )
}

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

// =============================================================================
// Embedded CSS
// =============================================================================

const CSS_TEMPLATE: &str = r##"
:root {
    --bg: #1a1a2e;
    --bg-panel: #16213e;
    --bg-block: #0f0f1a;
    --text: #eee;
    --text-dim: #888;
    --accent: #8be9fd;
    --green: #50fa7b;
    --purple: #bd93f9;
    --pink: #ff79c6;
    --orange: #ffb86c;
    --border: #333;
}

* { box-sizing: border-box; margin: 0; padding: 0; }

body {
    font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
    background: var(--bg);
    color: var(--text);
    height: 100vh;
    display: flex;
    flex-direction: column;
    overflow: hidden;
}

#header {
    padding: 0.75rem 1.5rem;
    background: var(--bg-panel);
    border-bottom: 1px solid var(--border);
    display: flex;
    align-items: center;
    gap: 1rem;
}

#header h1 {
    color: var(--accent);
    font-size: 1.25rem;
    margin: 0;
}

#fn-selector select {
    background: var(--bg);
    color: var(--text);
    border: 1px solid var(--border);
    padding: 0.3rem 0.6rem;
    border-radius: 4px;
}

#path-bar {
    padding: 0.5rem 1.5rem;
    background: var(--bg-block);
    border-bottom: 1px solid var(--border);
    display: flex;
    align-items: center;
    gap: 0.5rem;
    font-family: monospace;
    font-size: 0.9rem;
}

.path-label { color: var(--text-dim); }

#path-breadcrumb {
    flex: 1;
    color: var(--green);
}

#path-breadcrumb .current {
    color: var(--pink);
    font-weight: bold;
}

#path-breadcrumb .visited {
    color: var(--text-dim);
}

#reset-btn, #back-btn {
    background: var(--bg-panel);
    color: var(--text);
    border: 1px solid var(--border);
    padding: 0.3rem 0.8rem;
    border-radius: 4px;
    cursor: pointer;
}

#reset-btn:hover, #back-btn:hover {
    background: var(--border);
}

#main {
    flex: 1;
    display: flex;
    overflow: hidden;
}

#graph-panel {
    flex: 1;
    min-width: 0;
}

#cy {
    width: 100%;
    height: 100%;
    background: var(--bg);
}

#context-panel {
    width: 350px;
    background: var(--bg-panel);
    border-left: 1px solid var(--border);
    overflow-y: auto;
    padding: 1rem;
}

#block-header {
    display: flex;
    align-items: center;
    gap: 0.75rem;
    margin-bottom: 0.75rem;
}

#current-block {
    color: var(--pink);
    font-size: 1.5rem;
    font-family: monospace;
}

.badge {
    background: var(--bg);
    color: var(--accent);
    padding: 0.2rem 0.5rem;
    border-radius: 4px;
    font-size: 0.7rem;
    font-weight: 600;
    text-transform: uppercase;
}

.badge.entry { background: var(--green); color: var(--bg); }
.badge.exit { background: var(--purple); color: var(--bg); }
.badge.branchpoint { background: var(--orange); color: var(--bg); }
.badge.mergepoint { background: var(--accent); color: var(--bg); }
.badge.cleanup { background: #ff5555; color: white; }

#block-summary {
    color: var(--text-dim);
    font-size: 0.9rem;
    margin-bottom: 1rem;
    padding-bottom: 1rem;
    border-bottom: 1px solid var(--border);
}

#context-panel section {
    margin-bottom: 1rem;
}

#context-panel h3 {
    color: var(--text-dim);
    font-size: 0.75rem;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    margin-bottom: 0.5rem;
}

#context-panel section.collapsed ul,
#context-panel section.collapsed div:not(h3) {
    display: none;
}

#context-panel section.collapsed h3 {
    cursor: pointer;
}

#stmt-list, #locals-list {
    list-style: none;
    font-family: 'SF Mono', 'Fira Code', monospace;
    font-size: 0.8rem;
}

#stmt-list li, #locals-list li {
    padding: 0.4rem 0;
    border-bottom: 1px solid rgba(255,255,255,0.05);
}

#stmt-list .mir {
    color: var(--green);
}

#stmt-list .annotation {
    color: var(--purple);
    font-size: 0.75rem;
    display: block;
    margin-top: 0.2rem;
}

#term-display {
    font-family: monospace;
    font-size: 0.85rem;
}

#term-display .mir {
    color: var(--pink);
}

#term-display .annotation {
    color: var(--purple);
    font-size: 0.8rem;
    margin-top: 0.3rem;
}

#edge-buttons {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
}

.edge-btn {
    background: var(--bg);
    border: 1px solid var(--border);
    color: var(--text);
    padding: 0.6rem 0.8rem;
    border-radius: 6px;
    cursor: pointer;
    text-align: left;
    transition: all 0.15s;
}

.edge-btn:hover {
    background: var(--border);
    border-color: var(--accent);
}

.edge-btn .target {
    color: var(--green);
    font-family: monospace;
    font-weight: 600;
}

.edge-btn .label {
    color: var(--orange);
    margin-left: 0.5rem;
    font-size: 0.85rem;
}

.edge-btn .hint {
    display: block;
    color: var(--text-dim);
    font-size: 0.75rem;
    margin-top: 0.2rem;
}

.edge-btn.cleanup {
    border-color: #ff5555;
    border-style: dashed;
}

.edge-btn.cleanup .target {
    color: #ff5555;
}

#alt-paths {
    color: var(--text-dim);
    font-size: 0.85rem;
    padding: 0.5rem;
    background: rgba(255,255,255,0.03);
    border-radius: 4px;
}

#alt-paths:empty {
    display: none;
}

#controls {
    padding: 0.5rem 1.5rem;
    background: var(--bg-panel);
    border-top: 1px solid var(--border);
    display: flex;
    align-items: center;
    gap: 1rem;
}

#step-counter {
    color: var(--text-dim);
    font-size: 0.85rem;
}

.hint {
    color: var(--text-dim);
    font-size: 0.8rem;
    margin-left: auto;
}
"##;

// =============================================================================
// Embedded JavaScript
// =============================================================================

const JS_TEMPLATE: &str = r##"
class MirExplorer {
    constructor(data) {
        this.data = data;
        this.currentFnIndex = 0;
        this.path = [];
        this.currentBlock = null;

        this.initFunctionSelector();
        this.initGraph();
        this.goToBlock(this.currentFn.entry_block);
        this.initKeyboard();
    }

    get currentFn() {
        return this.data.functions[this.currentFnIndex];
    }

    initFunctionSelector() {
        const selector = document.getElementById('fn-selector');
        if (this.data.functions.length <= 1) return;

        const select = document.createElement('select');
        this.data.functions.forEach((fn, i) => {
            const opt = document.createElement('option');
            opt.value = i;
            opt.textContent = fn.short_name;
            select.appendChild(opt);
        });
        select.onchange = (e) => {
            this.currentFnIndex = parseInt(e.target.value);
            this.reset();
            this.initGraph();
            this.goToBlock(this.currentFn.entry_block);
        };
        selector.appendChild(select);
    }

    initGraph() {
        const elements = this.buildElements();

        if (this.cy) {
            this.cy.destroy();
        }

        this.cy = cytoscape({
            container: document.getElementById('cy'),
            elements: elements,
            style: [
                {
                    selector: 'node',
                    style: {
                        'label': 'data(label)',
                        'text-valign': 'center',
                        'text-halign': 'center',
                        'background-color': '#3a3a5e',
                        'color': '#eee',
                        'font-size': '12px',
                        'font-family': 'monospace',
                        'width': 60,
                        'height': 35,
                        'shape': 'roundrectangle',
                        'border-width': 2,
                        'border-color': '#555'
                    }
                },
                {
                    selector: 'node.entry',
                    style: { 'border-color': '#50fa7b', 'border-width': 3 }
                },
                {
                    selector: 'node.exit',
                    style: { 'border-color': '#bd93f9', 'border-width': 3 }
                },
                {
                    selector: 'node.branchpoint',
                    style: { 'border-color': '#ffb86c', 'border-width': 3 }
                },
                {
                    selector: 'node.visited',
                    style: { 'background-color': '#2a4a6e' }
                },
                {
                    selector: 'node.current',
                    style: {
                        'background-color': '#50fa7b',
                        'color': '#1a1a2e',
                        'border-color': '#50fa7b',
                        'font-weight': 'bold'
                    }
                },
                {
                    selector: 'node.dim',
                    style: { 'opacity': 0.35 }
                },
                {
                    selector: 'edge',
                    style: {
                        'width': 2,
                        'line-color': '#555',
                        'target-arrow-color': '#555',
                        'target-arrow-shape': 'triangle',
                        'curve-style': 'bezier',
                        'label': 'data(label)',
                        'font-size': '10px',
                        'color': '#888',
                        'text-rotation': 'autorotate',
                        'text-margin-y': -10
                    }
                },
                {
                    selector: 'edge.cleanup',
                    style: {
                        'line-style': 'dashed',
                        'line-color': '#ff5555',
                        'target-arrow-color': '#ff5555'
                    }
                },
                {
                    selector: 'edge.taken',
                    style: {
                        'line-color': '#50fa7b',
                        'target-arrow-color': '#50fa7b',
                        'width': 3
                    }
                }
            ],
            layout: {
                name: 'breadthfirst',
                directed: true,
                padding: 50,
                spacingFactor: 1.5
            }
        });

        // Click handlers
        this.cy.on('tap', 'node', (evt) => {
            const id = parseInt(evt.target.id().replace('bb', ''));
            this.goToBlock(id);
        });

        this.cy.on('tap', 'edge', (evt) => {
            const targetId = parseInt(evt.target.target().id().replace('bb', ''));
            this.goToBlock(targetId);
        });
    }

    buildElements() {
        const fn = this.currentFn;
        const nodes = fn.blocks.map(b => ({
            data: { id: `bb${b.id}`, label: `bb${b.id}` },
            classes: b.role
        }));

        const edges = [];
        for (const block of fn.blocks) {
            for (const edge of block.terminator.edges) {
                edges.push({
                    data: {
                        id: `bb${block.id}-bb${edge.target}`,
                        source: `bb${block.id}`,
                        target: `bb${edge.target}`,
                        label: edge.label
                    },
                    classes: edge.kind === 'cleanup' ? 'cleanup' : ''
                });
            }
        }

        return { nodes, edges };
    }

    goToBlock(blockId) {
        const fn = this.currentFn;
        if (blockId < 0 || blockId >= fn.blocks.length) return;

        const block = fn.blocks[blockId];

        // Update path
        if (this.currentBlock !== null && this.currentBlock !== blockId) {
            // Only add to path if not already there (avoid duplicates when going back)
            if (this.path[this.path.length - 1] !== this.currentBlock) {
                this.path.push(this.currentBlock);
            }
        }
        this.currentBlock = blockId;

        // Update graph styling
        this.cy.nodes().removeClass('current visited dim');
        this.cy.edges().removeClass('taken');

        const visitedSet = new Set([...this.path, blockId]);

        // Mark visited
        for (const v of this.path) {
            this.cy.$(`#bb${v}`).addClass('visited');
        }

        // Mark current
        this.cy.$(`#bb${blockId}`).addClass('current');

        // Dim unvisited
        this.cy.nodes().forEach(n => {
            const id = parseInt(n.id().replace('bb', ''));
            if (!visitedSet.has(id)) {
                n.addClass('dim');
            }
        });

        // Mark taken edges
        for (let i = 0; i < this.path.length; i++) {
            const from = this.path[i];
            const to = i + 1 < this.path.length ? this.path[i + 1] : blockId;
            this.cy.$(`#bb${from}-bb${to}`).addClass('taken');
        }

        // Center on current node
        this.cy.animate({
            center: { eles: this.cy.$(`#bb${blockId}`) },
            duration: 200
        });

        // Update UI
        this.updateContextPanel(block);
        this.updatePathBreadcrumb();
        this.updateStepCounter();
    }

    goBack() {
        if (this.path.length > 0) {
            const prev = this.path.pop();
            this.currentBlock = null; // Reset to avoid double-push
            this.goToBlock(prev);
        }
    }

    reset() {
        this.path = [];
        this.currentBlock = null;
        if (this.cy) {
            this.cy.nodes().removeClass('current visited dim');
            this.cy.edges().removeClass('taken');
        }
    }

    updateContextPanel(block) {
        // Header
        document.getElementById('current-block').textContent = `bb${block.id}`;
        const badge = document.getElementById('block-role');
        badge.textContent = block.role.toUpperCase();
        badge.className = 'badge ' + block.role;

        // Summary
        document.getElementById('block-summary').textContent = block.summary;

        // Locals (collapsed by default)
        const localsList = document.getElementById('locals-list');
        localsList.innerHTML = '';
        for (const local of this.currentFn.locals) {
            const li = document.createElement('li');
            li.textContent = local;
            localsList.appendChild(li);
        }

        // Statements
        const stmtList = document.getElementById('stmt-list');
        stmtList.innerHTML = '';
        if (block.statements.length === 0) {
            const li = document.createElement('li');
            li.innerHTML = '<span class="mir" style="color: var(--text-dim);">(no statements)</span>';
            stmtList.appendChild(li);
        } else {
            for (const stmt of block.statements) {
                const li = document.createElement('li');
                li.innerHTML = `
                    <span class="mir">${escapeHtml(stmt.mir)}</span>
                    <span class="annotation">${escapeHtml(stmt.annotation)}</span>
                `;
                stmtList.appendChild(li);
            }
        }

        // Terminator
        const termDisplay = document.getElementById('term-display');
        termDisplay.innerHTML = `
            <div class="mir">${escapeHtml(block.terminator.mir)}</div>
            <div class="annotation">${escapeHtml(block.terminator.annotation)}</div>
        `;

        // Edge buttons
        const edgeContainer = document.getElementById('edge-buttons');
        edgeContainer.innerHTML = '';
        for (const edge of block.terminator.edges) {
            const btn = document.createElement('button');
            btn.className = 'edge-btn' + (edge.kind === 'cleanup' ? ' cleanup' : '');
            btn.innerHTML = `
                <span class="target">→ bb${edge.target}</span>
                <span class="label">${escapeHtml(edge.label)}</span>
                <span class="hint">${escapeHtml(edge.annotation)}</span>
            `;
            btn.onclick = () => this.goToBlock(edge.target);
            edgeContainer.appendChild(btn);
        }

        // Alternative paths
        const altPaths = document.getElementById('alt-paths');
        const otherPreds = block.predecessors.filter(p =>
            !this.path.includes(p) && p !== this.path[this.path.length - 1]
        );
        if (otherPreds.length > 0 && this.path.length > 0) {
            altPaths.innerHTML = `
                <strong>Also reachable from:</strong>
                ${otherPreds.map(p => `bb${p}`).join(', ')}
            `;
        } else {
            altPaths.innerHTML = '';
        }
    }

    updatePathBreadcrumb() {
        const crumb = document.getElementById('path-breadcrumb');
        const fullPath = [...this.path, this.currentBlock];
        crumb.innerHTML = fullPath.map((b, i) => {
            const isLast = i === fullPath.length - 1;
            return `<span class="${isLast ? 'current' : 'visited'}">bb${b}</span>`;
        }).join(' → ');
    }

    updateStepCounter() {
        document.getElementById('step-counter').textContent =
            `Step ${this.path.length + 1}`;
    }

    initKeyboard() {
        document.addEventListener('keydown', (e) => {
            if (e.key === 'ArrowLeft' || e.key === 'Backspace') {
                e.preventDefault();
                this.goBack();
            }
            // Number keys 1-9 for quick edge selection
            if (e.key >= '1' && e.key <= '9') {
                const block = this.currentFn.blocks[this.currentBlock];
                const idx = parseInt(e.key) - 1;
                if (block.terminator.edges[idx]) {
                    this.goToBlock(block.terminator.edges[idx].target);
                }
            }
        });
    }
}

function escapeHtml(s) {
    if (!s) return '';
    return s.replace(/&/g, '&amp;')
            .replace(/</g, '&lt;')
            .replace(/>/g, '&gt;')
            .replace(/"/g, '&quot;');
}

function toggleSection(sectionId) {
    const section = document.getElementById(sectionId);
    section.classList.toggle('collapsed');
    const h3 = section.querySelector('h3');
    if (section.classList.contains('collapsed')) {
        h3.textContent = h3.textContent.replace('▾', '▸');
    } else {
        h3.textContent = h3.textContent.replace('▸', '▾');
    }
}

// Initialize
const explorer = new MirExplorer(EXPLORER_DATA);
"##;
