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
    BasicBlock, Body, Statement, StatementKind, Terminator, TerminatorKind, UnwindAction,
};

use crate::assets::{explorer, RENDER_LOCAL_JS};
use crate::printer::{collect_smir, SmirJson};
use crate::render::{
    annotate_rvalue, escape_html, extract_call_name, render_operand, render_place, render_rvalue,
    short_fn_name,
};
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
    pub locals: Vec<ExplorerLocal>,
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

#[derive(Serialize, Clone)]
pub struct ExplorerAssignment {
    pub block_id: usize,
    pub value: String,
}

#[derive(Serialize)]
pub struct ExplorerLocal {
    pub name: String,
    pub ty: String,
    pub source_name: Option<String>,
    pub assignments: Vec<ExplorerAssignment>,
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

    // Collect local type info and track assignments
    let num_locals = body.local_decls().count();
    let mut assignments: Vec<Vec<ExplorerAssignment>> = vec![Vec::new(); num_locals];

    // Scan all blocks for assignments to locals
    for (block_id, block) in body.blocks.iter().enumerate() {
        for stmt in &block.statements {
            if let StatementKind::Assign(place, rvalue) = &stmt.kind {
                // Only track direct local assignments (not projections)
                if place.projection.is_empty() {
                    let local_idx = place.local;
                    if local_idx < num_locals {
                        assignments[local_idx].push(ExplorerAssignment {
                            block_id,
                            value: render_rvalue(rvalue),
                        });
                    }
                }
            }
        }
    }

    // Build map from local index to source variable name from debug info
    let mut source_names: std::collections::HashMap<usize, String> =
        std::collections::HashMap::new();
    for debug_info in &body.var_debug_info {
        if let stable_mir::mir::VarDebugInfoContents::Place(place) = &debug_info.value {
            // Only map direct locals (no projections)
            if place.projection.is_empty() {
                source_names.insert(place.local, debug_info.name.clone());
            }
        }
    }

    // Build locals with assignments and source names
    let locals: Vec<ExplorerLocal> = body
        .local_decls()
        .enumerate()
        .map(|(idx, (local_idx, decl))| {
            let local_assignments = if idx < assignments.len() {
                std::mem::take(&mut assignments[idx])
            } else {
                Vec::new()
            };
            let source_name = source_names.get(&local_idx).cloned();
            ExplorerLocal {
                name: format!("_{}", local_idx),
                ty: format!("{}", decl.ty),
                source_name,
                assignments: local_assignments,
            }
        })
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
{render_local_js}
{js}
    </script>
</body>
</html>"##,
        name = escape_html(&smir.name),
        css = explorer::CSS,
        json = json_data,
        render_local_js = RENDER_LOCAL_JS,
        js = explorer::JS,
    )
}
