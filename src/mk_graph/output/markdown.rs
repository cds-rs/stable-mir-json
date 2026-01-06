//! Markdown format output for MIR annotation.
//!
//! Generates a structured Markdown document following the MIR Walkthrough template:
//! - Source context
//! - Function overview with detected properties
//! - Locals table with notes
//! - Control-flow graph (ASCII)
//! - Basic blocks with inferred titles
//! - Placeholder sections for human-authored content

use std::fs::File;
use std::io::{self, BufWriter, Write};

extern crate rustc_middle;
use rustc_middle::ty::TyCtxt;

extern crate rustc_session;
use rustc_session::config::{OutFileName, OutputType};

use crate::printer::{collect_smir, SmirJson};
use crate::render::short_fn_name;
use crate::MonoItemKind;

use super::traversal::{BlockRole, FunctionContext, SpanIndex, TypeIndex};

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

    // Build indices for lookups
    let span_index = SpanIndex::from_spans(&smir.spans);
    let type_index = TypeIndex::from_types(&smir.types);

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
        let ctx = FunctionContext::new(&short_name, name, body, &span_index, &type_index);
        content.push_str(&generate_function_markdown(&ctx));
    }

    content
}

/// Generate markdown for a single function
fn generate_function_markdown(ctx: &FunctionContext) -> String {
    let mut md = String::new();

    // === Header ===
    md.push_str(&format!("# `{}` — MIR Walkthrough\n\n", ctx.short_name));

    // Purpose placeholder
    md.push_str("> **Purpose:** <!-- TODO: Describe why this walkthrough exists -->\n\n");
    md.push_str("---\n\n");

    // === Source Context ===
    md.push_str("## Source Context\n\n");
    if let Some(source) = &ctx.source {
        md.push_str("```rust\n");
        md.push_str(source);
        md.push_str("\n```\n\n");
    } else {
        md.push_str("```rust\n// Source not available\n```\n\n");
    }

    // === Function Overview ===
    md.push_str("---\n\n");
    md.push_str("## Function Overview\n\n");
    md.push_str(&format!("- **Function:** `{}`\n", ctx.full_name));
    md.push_str(&format!("- **Basic blocks:** {}\n", ctx.body.blocks.len()));

    // Return type from _0
    if let Some((_, decl)) = ctx.body.local_decls().next() {
        md.push_str(&format!(
            "- **Return type:** `{}`\n",
            ctx.render_type(decl.ty)
        ));
    }

    // Notable properties
    let props = ctx.property_strings();
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

    for (i, (index, decl)) in ctx.body.local_decls().enumerate() {
        let note = if i == 0 { "Return place" } else { "" };
        md.push_str(&format!(
            "| `{}` | `{}` | {} |\n",
            index,
            escape_code_cell(&ctx.render_type(decl.ty)),
            note
        ));
    }
    md.push_str("\n> *Note: Locals are numbered in declaration order; temporaries introduced by lowering may not map 1-to-1 with source variables.*\n\n");

    // === Borrows ===
    if ctx.has_borrows() {
        md.push_str("---\n\n");
        md.push_str("## Borrows\n\n");
        md.push_str("| # | Borrow | Kind | Created At | Borrowed Local |\n");
        md.push_str("|---|--------|------|------------|----------------|\n");

        for borrow in ctx.borrows() {
            let kind = match borrow.kind {
                super::traversal::BorrowKindInfo::Shared => "`&`",
                super::traversal::BorrowKindInfo::Mutable => "`&mut`",
                super::traversal::BorrowKindInfo::Shallow => "`&shallow`",
            };
            md.push_str(&format!(
                "| {} | `_{}` | {} | `bb{}[{}]` | `_{}` |\n",
                borrow.index,
                borrow.borrower_local,
                kind,
                borrow.start_location.block,
                borrow.start_location.statement,
                borrow.borrowed_local
            ));
        }
        md.push_str("\n> *Borrows are tracked conservatively: a borrow is considered active from creation until the borrower is reassigned or goes out of scope.*\n\n");
    }

    // === Control-Flow Overview ===
    md.push_str("---\n\n");
    md.push_str("## Control-Flow Overview\n\n");
    md.push_str("```\n");
    md.push_str(&ctx.ascii_cfg());
    md.push_str("```\n\n");

    // === Basic Blocks ===
    md.push_str("---\n\n");
    md.push_str("## Basic Blocks\n\n");

    for idx in 0..ctx.body.blocks.len() {
        let role = ctx.block_role(idx);
        let rows = ctx.render_block(idx);
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

/// Render a basic block as Markdown
fn render_block_markdown(
    idx: usize,
    rows: &[super::traversal::AnnotatedRow],
    role: BlockRole,
) -> String {
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

/// Escape for table cells - minimal escaping for technical content
fn escape_table_cell(s: &str) -> String {
    s.replace('|', "\\|").replace('\n', " ")
}

/// Escape for code content inside backticks
fn escape_code_cell(s: &str) -> String {
    s.replace('`', "'").replace('|', "\\|").replace('\n', " ")
}
