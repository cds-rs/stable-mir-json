//! Markdown format output for MIR annotation.
//!
//! Generates a structured Markdown document following the MIR Walkthrough template:
//! - Source context
//! - Function overview with detected properties
//! - Locals table with notes
//! - Control-flow graph (ASCII)
//! - Basic blocks with inferred titles
//! - Placeholder sections for human-authored content

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{self, BufWriter, Write};

extern crate rustc_middle;
use rustc_middle::ty::TyCtxt;

extern crate rustc_session;
use rustc_session::config::{OutFileName, OutputType};

extern crate stable_mir;
use stable_mir::mir::{
    BasicBlock, Body, Rvalue, Statement, StatementKind, Terminator, TerminatorKind, UnwindAction,
};
use stable_mir::ty::IndexedVal;

use crate::printer::{collect_smir, SmirJson};
use crate::render::{
    annotate_rvalue, extract_call_name, render_operand, render_place, render_rvalue, short_fn_name,
};
use crate::MonoItemKind;

/// Span information: (filename, start_line, start_col, end_line, end_col)
type SpanInfo = (String, usize, usize, usize, usize);

/// A row in the MIR table
struct AnnotatedRow {
    mir: String,
    annotation: String,
    is_terminator: bool,
}

/// Detected properties of a function
#[derive(Default)]
struct FunctionProperties {
    has_panic_path: bool,
    has_checked_ops: bool,
    has_borrows: bool,
    has_drops: bool,
    has_recursion: bool,
    has_assertions: bool,
    has_switches: bool,
}

/// Inferred role of a basic block
#[derive(Clone, Copy, PartialEq)]
enum BlockRole {
    Entry,
    Return,
    Panic,
    Cleanup,
    Branch,
    Loop,
    Normal,
}

impl BlockRole {
    fn title(&self) -> &'static str {
        match self {
            BlockRole::Entry => "entry",
            BlockRole::Return => "return / success",
            BlockRole::Panic => "panic path",
            BlockRole::Cleanup => "cleanup / unwind",
            BlockRole::Branch => "branch point",
            BlockRole::Loop => "loop",
            BlockRole::Normal => "",
        }
    }
}

/// Entry point to generate Markdown file
pub fn emit_mdfile(tcx: TyCtxt<'_>) {
    let smir = collect_smir(tcx);
    let markdown = generate_markdown(&smir);

    match tcx.output_filenames(()).path(OutputType::Mir) {
        OutFileName::Stdout => {
            write!(io::stdout(), "{}", markdown).expect("Failed to write Markdown");
        }
        OutFileName::Real(path) => {
            let out_path = path.with_extension("smir.md");
            let mut b = BufWriter::new(
                File::create(&out_path)
                    .unwrap_or_else(|e| panic!("Failed to create {}: {}", out_path.display(), e)),
            );
            write!(b, "{}", markdown).expect("Failed to write Markdown");
        }
    }
}

/// Generate the complete Markdown document
fn generate_markdown(smir: &SmirJson) -> String {
    let mut content = String::new();

    // Build span index for source lookups
    let span_index: HashMap<usize, &SpanInfo> =
        smir.spans.iter().map(|(id, info)| (*id, info)).collect();

    // Generate content for each function
    for item in &smir.items {
        let MonoItemKind::MonoItemFn { name, body, .. } = &item.mono_item_kind else {
            continue;
        };

        let Some(body) = body else { continue };

        // Skip standard library functions for cleaner output
        if name.contains("std::") || name.contains("core::") {
            continue;
        }

        let short_name = short_fn_name(name);
        content.push_str(&generate_function_markdown(
            &short_name,
            name,
            body,
            &span_index,
        ));
    }

    content
}

/// Generate markdown for a single function
fn generate_function_markdown(
    short_name: &str,
    full_name: &str,
    body: &Body,
    span_index: &HashMap<usize, &SpanInfo>,
) -> String {
    let mut md = String::new();

    // Analyze the function
    let properties = analyze_function(body, short_name);
    let block_roles = infer_block_roles(body);

    // === Header ===
    md.push_str(&format!("# `{}` — MIR Walkthrough\n\n", short_name));

    // Purpose placeholder
    md.push_str("> **Purpose:** <!-- TODO: Describe why this walkthrough exists -->\n\n");
    md.push_str("---\n\n");

    // === Source Context ===
    md.push_str("## Source Context\n\n");
    if let Some(source) = extract_function_source(span_index, body) {
        md.push_str("```rust\n");
        md.push_str(&source);
        md.push_str("\n```\n\n");
    } else {
        md.push_str("```rust\n// Source not available\n```\n\n");
    }

    // === Function Overview ===
    md.push_str("---\n\n");
    md.push_str("## Function Overview\n\n");
    md.push_str(&format!("- **Function:** `{}`\n", full_name));
    md.push_str(&format!("- **Basic blocks:** {}\n", body.blocks.len()));

    // Return type from _0
    if let Some((_, decl)) = body.local_decls().next() {
        md.push_str(&format!("- **Return type:** `{}`\n", decl.ty));
    }

    // Notable properties
    let props = format_properties(&properties);
    if !props.is_empty() {
        md.push_str("- **Notable properties:**\n");
        for prop in props {
            md.push_str(&format!("  - {}\n", prop));
        }
    }
    md.push('\n');

    // === Locals ===
    md.push_str("---\n\n");
    md.push_str("## Locals\n\n");
    md.push_str("| Local | Type | Notes |\n");
    md.push_str("|-------|------|-------|\n");

    // Note: In MIR, local 0 is the return place, then arguments, then temporaries.
    // Without access to arg_count, we mark local 0 as return and others as variables.
    for (i, (index, decl)) in body.local_decls().enumerate() {
        let note = if i == 0 { "Return place" } else { "" };
        md.push_str(&format!(
            "| `{}` | `{}` | {} |\n",
            index,
            escape_code_cell(&format!("{}", decl.ty)),
            note
        ));
    }
    md.push_str("\n> *Note: Locals are numbered in declaration order; temporaries introduced by lowering may not map 1-to-1 with source variables.*\n\n");

    // === Control-Flow Overview ===
    md.push_str("---\n\n");
    md.push_str("## Control-Flow Overview\n\n");
    md.push_str("```\n");
    md.push_str(&generate_ascii_cfg(body, &block_roles));
    md.push_str("```\n\n");

    // === Basic Blocks ===
    md.push_str("---\n\n");
    md.push_str("## Basic Blocks\n\n");

    for (idx, block) in body.blocks.iter().enumerate() {
        let role = block_roles.get(&idx).copied().unwrap_or(BlockRole::Normal);
        let rows = render_block_rows(block, short_name);
        md.push_str(&render_block_markdown(idx, &rows, role));
    }

    // === Key Observations ===
    md.push_str("---\n\n");
    md.push_str("## Key Observations\n\n");
    md.push_str("<!-- TODO: Add bullet points summarizing what this MIR teaches -->\n\n");
    md.push_str("- \n");
    md.push_str("- \n\n");

    // === Takeaways ===
    md.push_str("---\n\n");
    md.push_str("## Takeaways\n\n");
    md.push_str("<!-- TODO: One or two sentences to help generalize this example -->\n\n");

    md.push_str("---\n\n");
    md
}

/// Analyze a function body to detect notable properties
fn analyze_function(body: &Body, current_fn: &str) -> FunctionProperties {
    let mut props = FunctionProperties::default();

    for block in &body.blocks {
        // Check statements
        for stmt in &block.statements {
            if let StatementKind::Assign(_, rvalue) = &stmt.kind {
                match rvalue {
                    Rvalue::CheckedBinaryOp(..) => props.has_checked_ops = true,
                    Rvalue::Ref(..) | Rvalue::AddressOf(..) => props.has_borrows = true,
                    _ => {}
                }
            }
        }

        // Check terminator
        match &block.terminator.kind {
            TerminatorKind::Call { func, target, .. } => {
                let func_name = extract_call_name(func);
                if func_name == current_fn {
                    props.has_recursion = true;
                }
                if func_name.contains("panic")
                    || func_name.contains("assert_failed")
                    || target.is_none()
                {
                    props.has_panic_path = true;
                }
            }
            TerminatorKind::Assert { .. } => {
                props.has_assertions = true;
                props.has_panic_path = true;
            }
            TerminatorKind::SwitchInt { .. } => props.has_switches = true,
            TerminatorKind::Drop { .. } => props.has_drops = true,
            TerminatorKind::Resume {} | TerminatorKind::Abort {} => props.has_panic_path = true,
            _ => {}
        }
    }

    props
}

/// Format detected properties as strings
fn format_properties(props: &FunctionProperties) -> Vec<&'static str> {
    let mut result = Vec::new();
    if props.has_panic_path {
        result.push("Contains panic path");
    }
    if props.has_checked_ops {
        result.push("Uses checked arithmetic");
    }
    if props.has_borrows {
        result.push("Introduces borrows");
    }
    if props.has_drops {
        result.push("Has explicit drops");
    }
    if props.has_recursion {
        result.push("Contains recursion");
    }
    if props.has_assertions {
        result.push("Contains assertions");
    }
    if props.has_switches {
        result.push("Has conditional branches");
    }
    result
}

/// Infer the role of each basic block
fn infer_block_roles(body: &Body) -> HashMap<usize, BlockRole> {
    let mut roles = HashMap::new();

    // Entry block is always bb0
    roles.insert(0, BlockRole::Entry);

    // Find cleanup targets
    let mut cleanup_blocks = HashSet::new();
    for block in &body.blocks {
        let unwind = match &block.terminator.kind {
            TerminatorKind::Drop { unwind, .. } => Some(unwind),
            TerminatorKind::Call { unwind, .. } => Some(unwind),
            TerminatorKind::Assert { unwind, .. } => Some(unwind),
            _ => None,
        };
        if let Some(UnwindAction::Cleanup(target)) = unwind {
            cleanup_blocks.insert(*target);
        }
    }

    // Detect loops (blocks that can reach themselves)
    let loop_blocks = detect_loops(body);

    for (idx, block) in body.blocks.iter().enumerate() {
        if roles.contains_key(&idx) {
            continue;
        }

        if cleanup_blocks.contains(&idx) {
            roles.insert(idx, BlockRole::Cleanup);
            continue;
        }

        if loop_blocks.contains(&idx) {
            roles.insert(idx, BlockRole::Loop);
            continue;
        }

        match &block.terminator.kind {
            TerminatorKind::Return {} => {
                roles.insert(idx, BlockRole::Return);
            }
            TerminatorKind::Resume {} | TerminatorKind::Abort {} | TerminatorKind::Unreachable {} => {
                roles.insert(idx, BlockRole::Panic);
            }
            TerminatorKind::Call { target: None, .. } => {
                roles.insert(idx, BlockRole::Panic);
            }
            TerminatorKind::Call { func, .. } => {
                let name = extract_call_name(func);
                if name.contains("panic") || name.contains("assert_failed") {
                    roles.insert(idx, BlockRole::Panic);
                }
            }
            TerminatorKind::SwitchInt { .. } => {
                roles.insert(idx, BlockRole::Branch);
            }
            _ => {}
        }
    }

    roles
}

/// Detect blocks that are part of loops
fn detect_loops(body: &Body) -> HashSet<usize> {
    let mut loop_blocks = HashSet::new();

    // Build successor map
    let successors: Vec<Vec<usize>> = body
        .blocks
        .iter()
        .map(|b| get_terminator_targets(&b.terminator))
        .collect();

    // For each block, check if it can reach itself
    for start in 0..body.blocks.len() {
        let mut visited = HashSet::new();
        let mut stack = successors[start].clone();

        while let Some(curr) = stack.pop() {
            if curr == start {
                loop_blocks.insert(start);
                break;
            }
            if visited.insert(curr) && curr < successors.len() {
                stack.extend(successors[curr].iter().copied());
            }
        }
    }

    loop_blocks
}

/// Get target block indices from a terminator
fn get_terminator_targets(term: &Terminator) -> Vec<usize> {
    match &term.kind {
        TerminatorKind::Goto { target } => vec![*target],
        TerminatorKind::SwitchInt { targets, .. } => {
            let mut result: Vec<usize> = targets.branches().map(|(_, t)| t).collect();
            result.push(targets.otherwise());
            result
        }
        TerminatorKind::Return {}
        | TerminatorKind::Resume {}
        | TerminatorKind::Abort {}
        | TerminatorKind::Unreachable {} => vec![],
        TerminatorKind::Drop { target, unwind, .. } => {
            let mut result = vec![*target];
            if let UnwindAction::Cleanup(t) = unwind {
                result.push(*t);
            }
            result
        }
        TerminatorKind::Call { target, unwind, .. } => {
            let mut result = vec![];
            if let Some(t) = target {
                result.push(*t);
            }
            if let UnwindAction::Cleanup(t) = unwind {
                result.push(*t);
            }
            result
        }
        TerminatorKind::Assert { target, unwind, .. } => {
            let mut result = vec![*target];
            if let UnwindAction::Cleanup(t) = unwind {
                result.push(*t);
            }
            result
        }
        TerminatorKind::InlineAsm {
            destination,
            unwind,
            ..
        } => {
            let mut result = vec![];
            if let Some(t) = destination {
                result.push(*t);
            }
            if let UnwindAction::Cleanup(t) = unwind {
                result.push(*t);
            }
            result
        }
    }
}

/// Generate ASCII control-flow graph
fn generate_ascii_cfg(body: &Body, roles: &HashMap<usize, BlockRole>) -> String {
    let mut lines = Vec::new();

    for (idx, block) in body.blocks.iter().enumerate() {
        let role = roles.get(&idx).copied().unwrap_or(BlockRole::Normal);
        let role_suffix = match role {
            BlockRole::Entry => " (entry)",
            BlockRole::Return => " (return)",
            BlockRole::Panic => " (panic)",
            BlockRole::Cleanup => " (cleanup)",
            BlockRole::Branch => " (branch)",
            BlockRole::Loop => " (loop)",
            BlockRole::Normal => "",
        };

        let targets = get_terminator_targets(&block.terminator);
        if targets.is_empty() {
            lines.push(format!("bb{}{}", idx, role_suffix));
        } else {
            let arrows: Vec<String> = targets.iter().map(|t| format!("bb{}", t)).collect();
            lines.push(format!("bb{}{} ──▶ {}", idx, role_suffix, arrows.join(", ")));
        }
    }

    lines.join("\n") + "\n"
}

/// Render a basic block as Markdown
fn render_block_markdown(idx: usize, rows: &[AnnotatedRow], role: BlockRole) -> String {
    let mut md = String::new();

    // Block header with role
    let title = role.title();
    if title.is_empty() {
        md.push_str(&format!("### bb{}\n\n", idx));
    } else {
        md.push_str(&format!("### bb{} — {}\n\n", idx, title));
    }

    // Description placeholder for important blocks
    match role {
        BlockRole::Entry => md.push_str("Entry point of the function.\n\n"),
        BlockRole::Return => md.push_str("Normal return path.\n\n"),
        BlockRole::Panic => md.push_str("Panic/diverging path.\n\n"),
        BlockRole::Cleanup => md.push_str("Cleanup during unwinding.\n\n"),
        _ => {}
    }

    md.push_str("| MIR | Annotation |\n");
    md.push_str("|-----|------------|\n");

    for row in rows {
        let mir = if row.is_terminator {
            format!("→ {}", escape_code_cell(&row.mir))
        } else {
            escape_code_cell(&row.mir)
        };

        md.push_str(&format!(
            "| `{}` | {} |\n",
            mir,
            escape_table_cell(&row.annotation)
        ));
    }

    md.push('\n');
    md
}

/// Render a basic block as annotated rows
fn render_block_rows(block: &BasicBlock, current_fn: &str) -> Vec<AnnotatedRow> {
    let mut rows = Vec::new();

    // Process each statement
    for stmt in &block.statements {
        let (mir, annotation) = render_statement_annotated(stmt);
        rows.push(AnnotatedRow {
            mir,
            annotation,
            is_terminator: false,
        });
    }

    // Process terminator
    let (mir, annotation, _is_recursive) =
        render_terminator_annotated(&block.terminator, current_fn);
    rows.push(AnnotatedRow {
        mir,
        annotation,
        is_terminator: true,
    });

    rows
}

/// Render a statement with annotation
fn render_statement_annotated(stmt: &Statement) -> (String, String) {
    match &stmt.kind {
        StatementKind::Assign(place, rvalue) => {
            let mir = format!("{} = {}", render_place(place), render_rvalue(rvalue));
            let annotation = annotate_rvalue(rvalue);
            (mir, annotation)
        }
        StatementKind::StorageLive(local) => (
            format!("StorageLive(_{local})"),
            format!("Allocate stack slot for _{local}"),
        ),
        StatementKind::StorageDead(local) => (
            format!("StorageDead(_{local})"),
            format!("Deallocate stack slot for _{local}"),
        ),
        StatementKind::Nop => ("nop".to_string(), "No operation".to_string()),
        StatementKind::Retag(_, place) => (
            format!("retag({})", render_place(place)),
            "Stacked borrows retag".to_string(),
        ),
        StatementKind::FakeRead(_, place) => (
            format!("FakeRead({})", render_place(place)),
            "Compiler hint for borrow checker".to_string(),
        ),
        _ => (format!("{:?}", stmt.kind), String::new()),
    }
}

/// Render a terminator with annotation
fn render_terminator_annotated(term: &Terminator, current_fn: &str) -> (String, String, bool) {
    match &term.kind {
        TerminatorKind::Goto { target } => (
            format!("goto bb{target}"),
            format!("Jump to bb{target}"),
            false,
        ),
        TerminatorKind::Return {} => ("return".to_string(), "Return from function".to_string(), false),
        TerminatorKind::Unreachable {} => (
            "unreachable".to_string(),
            "Unreachable code".to_string(),
            false,
        ),
        TerminatorKind::SwitchInt { discr, targets } => {
            let discr_str = render_operand(discr);
            let branches: Vec<String> = targets
                .branches()
                .map(|(val, bb)| format!("{val}→bb{bb}"))
                .collect();
            let otherwise = targets.otherwise();
            let mir = format!(
                "switch({}) [{}; else→bb{}]",
                discr_str,
                branches.join(", "),
                otherwise
            );
            let annotation = format!("Branch on {}", discr_str);
            (mir, annotation, false)
        }
        TerminatorKind::Call {
            func,
            args,
            destination,
            target,
            ..
        } => {
            let func_name = extract_call_name(func);
            let args_str: Vec<String> = args.iter().map(|a| render_operand(&a.clone())).collect();
            let dest = render_place(destination);
            let target_str = target.map(|t| format!(" → bb{t}")).unwrap_or_default();
            let mir = format!(
                "{} = {}({}){}",
                dest,
                func_name,
                args_str.join(", "),
                target_str
            );

            let is_recursive = func_name == current_fn;
            let annotation = if is_recursive {
                format!("⟳ RECURSIVE call to {}", func_name)
            } else {
                format!("Call {}", func_name)
            };
            (mir, annotation, is_recursive)
        }
        TerminatorKind::Assert {
            cond,
            expected,
            target,
            ..
        } => {
            let cond_str = render_operand(cond);
            let mir = format!("assert({} == {}) → bb{}", cond_str, expected, target);
            let annotation = if *expected {
                format!("Panic if {} is false", cond_str)
            } else {
                format!("Panic if {} is true", cond_str)
            };
            (mir, annotation, false)
        }
        TerminatorKind::Drop { place, target, .. } => {
            let place_str = render_place(place);
            let mir = format!("drop({}) → bb{}", place_str, target);
            let annotation = format!("Drop {}", place_str);
            (mir, annotation, false)
        }
        TerminatorKind::Resume {} => ("resume".to_string(), "Resume unwinding".to_string(), false),
        TerminatorKind::Abort {} => ("abort".to_string(), "Abort program".to_string(), false),
        _ => (format!("{:?}", term.kind), String::new(), false),
    }
}

/// Extract the source code for a function from spans
fn extract_function_source(
    span_index: &HashMap<usize, &SpanInfo>,
    body: &Body,
) -> Option<String> {
    // Try to find the span covering the function body
    // Look at the first block's first statement or terminator
    let first_span = if !body.blocks.is_empty() {
        let block = &body.blocks[0];
        if !block.statements.is_empty() {
            Some(block.statements[0].span.to_index())
        } else {
            Some(block.terminator.span.to_index())
        }
    } else {
        None
    };

    let info = first_span.and_then(|id| span_index.get(&id))?;
    let (file, _, _, _, _) = info;

    if file.contains(".rustup") || file.contains("no-location") {
        return None;
    }

    // Read the source file and extract relevant lines
    let content = std::fs::read_to_string(file).ok()?;

    // Find function boundaries by looking at all spans
    let mut min_line = usize::MAX;
    let mut max_line = 0usize;

    for block in &body.blocks {
        for stmt in &block.statements {
            if let Some(span_info) = span_index.get(&stmt.span.to_index()) {
                if span_info.0 == *file {
                    min_line = min_line.min(span_info.1);
                    max_line = max_line.max(span_info.3);
                }
            }
        }
        if let Some(span_info) = span_index.get(&block.terminator.span.to_index()) {
            if span_info.0 == *file {
                min_line = min_line.min(span_info.1);
                max_line = max_line.max(span_info.3);
            }
        }
    }

    if min_line == usize::MAX {
        return None;
    }

    // Expand to include function signature (look for fn keyword above)
    let lines: Vec<&str> = content.lines().collect();
    let mut start = min_line.saturating_sub(1);
    while start > 0 {
        let line = lines.get(start - 1).unwrap_or(&"");
        if line.trim().starts_with("fn ") || line.trim().starts_with("pub fn ") {
            start -= 1;
            break;
        }
        if line.trim().is_empty() || line.trim().starts_with("//") || line.trim().starts_with("#[") {
            start -= 1;
        } else {
            break;
        }
    }

    // Extract lines
    let end = max_line.min(lines.len());
    let source_lines: Vec<&str> = lines[start..end].to_vec();
    Some(source_lines.join("\n"))
}

/// Escape for table cells - minimal escaping for technical content
fn escape_table_cell(s: &str) -> String {
    s.replace('|', "\\|").replace('\n', " ")
}

/// Escape for code content inside backticks
fn escape_code_cell(s: &str) -> String {
    s.replace('`', "'").replace('|', "\\|").replace('\n', " ")
}
