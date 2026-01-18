//! Mermaid diagram format output for MIR graphs.

extern crate stable_mir;
use stable_mir::mir::TerminatorKind;
use std::collections::HashSet;

use crate::printer::SmirJson;
use crate::MonoItemKind;

use crate::mk_graph::context::GraphContext;
use crate::mk_graph::util::{escape_mermaid, is_unqualified, name_lines, short_name, terminator_targets, hash_body};

impl SmirJson<'_> {
    /// Convert the MIR to Mermaid diagram format
    pub fn to_mermaid_file(self) -> String {
        let ctx = GraphContext::from_smir(&self);
        let mut output = String::new();

        output.push_str("flowchart TD\n\n");
        render_mermaid_allocs_legend(&ctx, &mut output);

        for item in self.items {
            match item.mono_item_kind {
                MonoItemKind::MonoItemFn { name, body, .. } => {
                    render_mermaid_function(&name, body.as_ref(), &ctx, &mut output);
                }
                MonoItemKind::MonoItemGlobalAsm { asm } => {
                    render_mermaid_asm(&asm, &mut output);
                }
                MonoItemKind::MonoItemStatic { name, .. } => {
                    render_mermaid_static(&name, &mut output);
                }
            }
        }

        output
    }
}

// =============================================================================
// Mermaid Rendering Helpers
// =============================================================================

fn render_mermaid_allocs_legend(ctx: &GraphContext, out: &mut String) {
    let legend_lines = ctx.allocs_legend_lines();
    if legend_lines.is_empty() {
        return;
    }

    out.push_str("    ALLOCS[\"");

    let legend_text = legend_lines
        .iter()
        .map(|s| escape_mermaid(s))
        .collect::<Vec<_>>()
        .join("<br/>");

    out.push_str(&legend_text);
    out.push_str("\"]\n");

    out.push_str("    style ALLOCS fill:#ffffcc,stroke:#999999\n\n");
}

fn render_mermaid_function(
    name: &str,
    body: Option<&stable_mir::mir::Body>,
    ctx: &GraphContext,
    out: &mut String,
) {
    let fn_id = match body {
        Some(body) => {
            let h = hash_body(body);
            format!("fn_{}_{}", short_name(name), h)
        }
        None => {
            // Stable fallback for body-less functions
            format!("fn_{}_no_body", short_name(name))
        }
    };

    let display_name = escape_mermaid(&name_lines(name));

    // Function subgraph container
    out.push_str(&format!("    subgraph {}[\"{}\"]\n", fn_id, display_name));
    out.push_str("        direction TD\n");

    if let Some(body) = body {
        render_mermaid_blocks(&fn_id, body, ctx, out);
        render_mermaid_block_edges(&fn_id, body, out);
    } else {
        out.push_str("        empty[\"<empty body>\"]\n");
    }

    out.push_str("    end\n");
    out.push_str(&format!("    style {} fill:#e0e0ff,stroke:#333\n\n", fn_id));

    // Call edges (must be outside the subgraph)
    if let Some(body) = body {
        render_mermaid_call_edges(&fn_id, body, ctx, out);
    }
}

fn render_mermaid_blocks(fn_id: &str, body: &stable_mir::mir::Body, ctx: &GraphContext, out: &mut String) {
    for (idx, block) in body.blocks.iter().enumerate() {
        let stmts: Vec<String> = block
            .statements
            .iter()
            .map(|s| escape_mermaid(&ctx.render_stmt(s)))
            .collect();

        let term_str = escape_mermaid(&ctx.render_terminator(&block.terminator));

        let mut label = format!("bb{}:", idx);
        for stmt in &stmts {
            label.push_str(&format!("<br/>{}", stmt));
        }
        label.push_str(&format!("<br/>---<br/>{}", term_str));

        out.push_str(&format!("        {}_bb{}[\"{}\"]\n", fn_id, idx, label));
    }
}

fn render_mermaid_block_edges(fn_id: &str, body: &stable_mir::mir::Body, out: &mut String) {
    for (idx, block) in body.blocks.iter().enumerate() {
        for target in terminator_targets(&block.terminator) {
            out.push_str(&format!("        {}_bb{} --> {}_bb{}\n", fn_id, idx, fn_id, target));
        }
    }
}

fn render_mermaid_call_edges(
    fn_id: &str,
    body: &stable_mir::mir::Body,
    ctx: &GraphContext,
    out: &mut String,
) {
    let mut emitted_callees = HashSet::new();

    for (idx, block) in body.blocks.iter().enumerate() {
        let TerminatorKind::Call { func, .. } = &block.terminator.kind else {
            continue;
        };

        let Some(callee_name) = ctx.resolve_call_target(func) else {
            continue;
        };

        if !is_unqualified(&callee_name) {
            continue;
        }

        let callee_id = short_name(&callee_name);

        if emitted_callees.insert(callee_id.clone()) {
            out.push_str(&format!(
                "    {}[\"{}\"]\n",
                callee_id,
                escape_mermaid(&callee_name)
            ));
            out.push_str(&format!(
                "    style {} fill:#ffe0e0,stroke:#333\n",
                callee_id
            ));
        }

        out.push_str(&format!(
            "    {}_bb{} -.->|call| {}\n",
            fn_id, idx, callee_id
        ));
    }

    if !emitted_callees.is_empty() {
        out.push('\n');
    }
}

fn render_mermaid_asm(asm: &str, out: &mut String) {
    let asm_id = short_name(asm);
    let asm_text = escape_mermaid(&asm.lines().collect::<String>());
    out.push_str(&format!("    {}[\"{}\"]\n", asm_id, asm_text));
    out.push_str(&format!("    style {} fill:#ffe0ff,stroke:#333\n\n", asm_id));
}

fn render_mermaid_static(name: &str, out: &mut String) {
    let static_id = short_name(name);
    out.push_str(&format!("    {}[\"{}\"]\n", static_id, escape_mermaid(name)));
    out.push_str(&format!("    style {} fill:#e0ffe0,stroke:#333\n\n", static_id));
}
