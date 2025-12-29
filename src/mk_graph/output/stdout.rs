//! Plain text stdout output for MIR annotation.
//!
//! Generates human-readable MIR output for the terminal.

use std::io::{self, Write};

extern crate rustc_middle;
use rustc_middle::ty::TyCtxt;

use crate::printer::{collect_smir, SmirJson};
use crate::render::short_fn_name;
use crate::MonoItemKind;

use super::traversal::{BlockRole, FunctionContext, SpanIndex};

/// Entry point to emit annotated MIR to stdout
pub fn emit_stdout(tcx: TyCtxt<'_>) {
    let smir = collect_smir(tcx);
    let output = generate_text(&smir);
    write!(io::stdout(), "{}", output).expect("Failed to write to stdout");
}

/// Generate the complete plain text output
fn generate_text(smir: &SmirJson) -> String {
    let mut content = String::new();

    // Build span index for source lookups
    let span_index = SpanIndex::from_spans(&smir.spans);

    // Header
    content.push_str(&format!("=== {} ===\n\n", smir.name));

    // Generate content for each function
    for item in &smir.items {
        let MonoItemKind::MonoItemFn { name, body, .. } = &item.mono_item_kind else {
            continue;
        };

        let Some(body) = body else { continue };

        // Skip standard library functions
        if name.contains("std::") || name.contains("core::") {
            continue;
        }

        let short_name = short_fn_name(name);
        let ctx = FunctionContext::new(&short_name, name, body, &span_index);
        content.push_str(&generate_function_text(&ctx));
    }

    content
}

/// Generate plain text for a single function
fn generate_function_text(ctx: &FunctionContext) -> String {
    let mut out = String::new();

    // Function header
    out.push_str(&format!("┌─ {} ", ctx.short_name));
    out.push_str(&"─".repeat(60_usize.saturating_sub(ctx.short_name.len() + 3)));
    out.push_str("┐\n");

    // Source context (if available)
    if let Some(source) = &ctx.source {
        out.push_str("│ Source:\n");
        for line in source.lines() {
            out.push_str(&format!("│   {}\n", line));
        }
        out.push_str("│\n");
    }

    // Overview
    out.push_str(&format!("│ Blocks: {}  ", ctx.body.blocks.len()));
    if let Some((_, decl)) = ctx.body.local_decls().next() {
        out.push_str(&format!("Returns: {}\n", decl.ty));
    } else {
        out.push('\n');
    }

    // Properties
    let props = ctx.property_strings();
    if !props.is_empty() {
        out.push_str(&format!("│ Properties: {}\n", props.join(", ")));
    }

    // Locals with lifetime info
    out.push_str("│\n│ Locals:\n");
    for (i, (index, decl)) in ctx.body.local_decls().enumerate() {
        let note = if i == 0 { " (return)" } else { "" };
        // Add lifetime range if available
        let lifetime_str = ctx
            .lifetime_of(i)
            .and_then(|l| l.source_range.as_ref())
            .map(|r| format!(" [{}]", r.format()))
            .unwrap_or_default();
        out.push_str(&format!("│   {}: {}{}{}\n", index, decl.ty, note, lifetime_str));
    }

    // Borrows with lifetime ranges
    if ctx.has_borrows() {
        out.push_str("│\n│ Borrows:\n");
        for borrow in ctx.borrows() {
            let kind = match borrow.kind {
                super::traversal::BorrowKindInfo::Shared => "&",
                super::traversal::BorrowKindInfo::Mutable => "&mut",
                super::traversal::BorrowKindInfo::Shallow => "&shallow",
            };
            // Add borrow lifetime range if available
            let range_str = ctx
                .borrow_range(borrow.index)
                .map(|(start, end)| {
                    if start == end {
                        format!(" [line {}]", start)
                    } else {
                        format!(" [lines {}-{}]", start, end)
                    }
                })
                .unwrap_or_default();
            out.push_str(&format!(
                "│   #{}: _{} = {}_{} at bb{}[{}]{}\n",
                borrow.index,
                borrow.borrower_local,
                kind,
                borrow.borrowed_local,
                borrow.start_location.block,
                borrow.start_location.statement,
                range_str
            ));
        }
    }

    // CFG
    out.push_str("│\n│ Control Flow:\n");
    for line in ctx.ascii_cfg().lines() {
        out.push_str(&format!("│   {}\n", line));
    }

    // Basic blocks
    out.push_str("│\n");
    for idx in 0..ctx.body.blocks.len() {
        let role = ctx.block_role(idx);
        let rows = ctx.render_block(idx);
        out.push_str(&render_block_text(idx, &rows, role));
    }

    out.push_str("└");
    out.push_str(&"─".repeat(60));
    out.push_str("┘\n\n");

    out
}

/// Render a basic block as plain text
fn render_block_text(
    idx: usize,
    rows: &[super::traversal::AnnotatedRow],
    role: BlockRole,
) -> String {
    let mut out = String::new();

    // Block header
    let role_str = match role {
        BlockRole::Entry => " (entry)",
        BlockRole::Return => " (return)",
        BlockRole::Panic => " (panic)",
        BlockRole::Cleanup => " (cleanup)",
        BlockRole::Branch => " (branch)",
        BlockRole::Loop => " (loop)",
        BlockRole::Normal => "",
    };
    out.push_str(&format!("├── bb{}{}\n", idx, role_str));

    // Statements and terminator
    for row in rows {
        let prefix = if row.is_terminator { "│   → " } else { "│     " };
        let suffix = if !row.annotation.is_empty() {
            format!("  // {}", row.annotation)
        } else {
            String::new()
        };
        out.push_str(&format!("{}{}{}\n", prefix, row.mir, suffix));
    }

    out
}
