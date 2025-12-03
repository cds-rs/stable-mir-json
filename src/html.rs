//! Generate annotated HTML from MIR data
//!
//! Creates a single-page HTML document showing source code, MIR, and annotations
//! in a three-column layout for each basic block.

extern crate serde_json;

use std::collections::HashMap;
use std::fs::File;
use std::io::{self, BufWriter, Write};

extern crate rustc_middle;
use rustc_middle::ty::TyCtxt;

extern crate rustc_session;
use rustc_session::config::{OutFileName, OutputType};

extern crate stable_mir;
use stable_mir::mir::{
    BasicBlock, BinOp, CastKind, NullOp, Operand, Place, Rvalue, Statement, StatementKind,
    Terminator, TerminatorKind, UnOp,
};
use stable_mir::ty::IndexedVal;
use stable_mir::CrateDef;

use crate::explore::{build_explorer_function, ExplorerFunction};
use crate::printer::{collect_smir, SmirJson};
use crate::MonoItemKind;

/// Span information: (filename, start_line, start_col, end_line, end_col)
type SpanInfo = (String, usize, usize, usize, usize);

/// A row in the three-column display
struct AnnotatedRow {
    source: String,
    mir: String,
    annotation: String,
    is_terminator: bool,
    is_recursive: bool,
}

/// Entry point to generate HTML file
pub fn emit_html(tcx: TyCtxt<'_>) {
    let smir = collect_smir(tcx);
    let html = generate_html(&smir);

    match tcx.output_filenames(()).path(OutputType::Mir) {
        OutFileName::Stdout => {
            write!(io::stdout(), "{}", html).expect("Failed to write HTML");
        }
        OutFileName::Real(path) => {
            let out_path = path.with_extension("smir.html");
            let mut b = BufWriter::new(
                File::create(&out_path)
                    .unwrap_or_else(|e| panic!("Failed to create {}: {}", out_path.display(), e)),
            );
            write!(b, "{}", html).expect("Failed to write HTML");
        }
    }
}

/// Generate the complete HTML document
fn generate_html(smir: &SmirJson) -> String {
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

        // Function header
        content.push_str(&format!(
            r#"<section class="function">
    <h2>{}</h2>
    <p class="fn-meta">{} basic blocks — <code>{}</code></p>
"#,
            escape_html(&short_name),
            body.blocks.len(),
            escape_html(name)
        ));

        // Explorer panel (collapsible)
        let explorer_fn = build_explorer_function(name, body);
        content.push_str(&render_explorer_panel(&short_name, &explorer_fn));

        // Each basic block
        for (idx, block) in body.blocks.iter().enumerate() {
            let rows = render_block_rows(block, &span_index, &short_name);
            content.push_str(&render_block_html(idx, &rows));
        }

        content.push_str("</section>\n");
    }

    // Wrap in HTML template
    format!(
        r#"<!DOCTYPE html>
<html>
<head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>{} - MIR Walkthrough</title>
    <style>
        :root {{
            --bg: #1a1a2e;
            --bg-section: #16213e;
            --bg-block: #0f0f1a;
            --bg-terminator: #1a1a3e;
            --text: #eee;
            --text-dim: #888;
            --accent: #8be9fd;
            --green: #50fa7b;
            --purple: #bd93f9;
            --pink: #ff79c6;
            --border: #333;
        }}
        * {{ box-sizing: border-box; }}
        body {{
            font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
            background: var(--bg);
            color: var(--text);
            line-height: 1.6;
            margin: 0;
            padding: 2rem;
        }}
        h1 {{
            color: var(--accent);
            border-bottom: 2px solid var(--border);
            padding-bottom: 0.5rem;
        }}
        .function {{
            background: var(--bg-section);
            border-radius: 8px;
            padding: 1.5rem;
            margin-bottom: 2rem;
        }}
        .function h2 {{
            color: var(--accent);
            margin: 0 0 0.5rem 0;
        }}
        .fn-meta {{
            color: var(--text-dim);
            margin: 0 0 1.5rem 0;
            font-size: 0.9rem;
        }}
        .fn-meta code {{
            background: var(--bg-block);
            padding: 0.2rem 0.4rem;
            border-radius: 3px;
            font-size: 0.85rem;
        }}
        .block {{
            background: var(--bg-block);
            border-radius: 6px;
            margin-bottom: 1rem;
            overflow-x: auto;
            -webkit-overflow-scrolling: touch;
        }}
        .block-header {{
            background: var(--border);
            padding: 0.5rem 1rem;
            font-weight: 600;
            color: var(--pink);
            font-family: monospace;
        }}
        .annotated-table {{
            width: 100%;
            min-width: 700px;
            border-collapse: collapse;
            font-size: 0.85rem;
            font-family: 'SF Mono', 'Fira Code', monospace;
        }}
        .annotated-table th {{
            text-align: left;
            padding: 0.6rem 1rem;
            border-bottom: 1px solid var(--border);
            color: var(--text-dim);
            font-weight: normal;
            font-size: 0.75rem;
            text-transform: uppercase;
            letter-spacing: 0.05em;
        }}
        .annotated-table td {{
            padding: 0.5rem 1rem;
            vertical-align: top;
            border-bottom: 1px solid rgba(255,255,255,0.05);
        }}
        .annotated-table tr:last-child td {{
            border-bottom: none;
        }}
        .col-source {{ width: 30%; color: var(--text); }}
        .col-mir {{ width: 35%; color: var(--green); }}
        .col-annot {{ width: 35%; color: var(--purple); }}
        .terminator {{
            background: var(--bg-terminator);
        }}
        .terminator .col-mir {{
            color: var(--pink);
        }}
        .recursive {{
            background: rgba(255, 121, 198, 0.15);
        }}
        .recursive .col-annot {{
            color: var(--pink);
            font-weight: 600;
        }}
        .empty {{ color: var(--text-dim); font-style: italic; }}
        .graph-section {{
            background: var(--bg-section);
            border-radius: 8px;
            margin-bottom: 2rem;
        }}
        .graph-section summary {{
            padding: 1rem 1.5rem;
            cursor: pointer;
            color: var(--accent);
            font-weight: 600;
            font-size: 1.1rem;
        }}
        .graph-section summary:hover {{
            background: rgba(255,255,255,0.05);
        }}
        .graph-container {{
            padding: 1rem;
            overflow-x: auto;
            background: var(--bg-block);
            border-radius: 0 0 8px 8px;
        }}
        .graph-container svg {{
            width: 100%;
            height: auto;
            min-height: 400px;
        }}
        .graph-controls {{
            padding: 0.5rem 1rem;
            display: flex;
            gap: 0.5rem;
            border-bottom: 1px solid var(--border);
        }}
        .graph-controls button {{
            background: var(--bg);
            border: 1px solid var(--border);
            color: var(--text);
            padding: 0.3rem 0.8rem;
            border-radius: 4px;
            cursor: pointer;
            font-size: 0.85rem;
        }}
        .graph-controls button:hover {{
            background: var(--border);
        }}
        .fullscreen-overlay {{
            display: none;
            position: fixed;
            top: 0;
            left: 0;
            width: 100vw;
            height: 100vh;
            z-index: 9999;
            background: var(--bg);
            flex-direction: column;
        }}
        .fullscreen-overlay.active {{
            display: flex;
        }}
        .fullscreen-overlay .fs-controls {{
            flex-shrink: 0;
            padding: 0.5rem 1rem;
            display: flex;
            gap: 0.5rem;
            background: var(--bg-section);
            border-bottom: 1px solid var(--border);
        }}
        .fullscreen-overlay .fs-controls button {{
            background: var(--bg);
            border: 1px solid var(--border);
            color: var(--text);
            padding: 0.3rem 0.8rem;
            border-radius: 4px;
            cursor: pointer;
            font-size: 0.85rem;
        }}
        .fullscreen-overlay .fs-controls button:hover {{
            background: var(--border);
        }}
        .fullscreen-overlay .fs-graph {{
            flex: 1;
            overflow: hidden;
        }}
        .fullscreen-overlay .fs-graph svg {{
            width: 100%;
            height: 100%;
        }}
        .source-section {{
            background: var(--bg-section);
            border-radius: 8px;
            margin-bottom: 2rem;
        }}
        .source-section summary {{
            padding: 1rem 1.5rem;
            cursor: pointer;
            color: var(--accent);
            font-weight: 600;
            font-size: 1.1rem;
        }}
        .source-section summary:hover {{
            background: rgba(255,255,255,0.05);
        }}
        .source-code {{
            margin: 0;
            padding: 1rem 1.5rem;
            background: var(--bg-block);
            border-radius: 0 0 8px 8px;
            overflow-x: auto;
            font-family: 'SF Mono', 'Fira Code', monospace;
            font-size: 0.85rem;
            line-height: 1.5;
            color: var(--text);
        }}
        /* Explorer panel styles */
        .explorer-section {{
            background: var(--bg-section);
            border-radius: 8px;
            margin-bottom: 1.5rem;
        }}
        .explorer-section summary {{
            padding: 1rem 1.5rem;
            cursor: pointer;
            color: var(--accent);
            font-weight: 600;
            font-size: 1.1rem;
        }}
        .explorer-section summary:hover {{
            background: rgba(255,255,255,0.05);
        }}
        .explorer-container {{
            display: flex;
            height: 500px;
            border-top: 1px solid var(--border);
        }}
        .explorer-graph {{
            flex: 1;
            min-width: 0;
            background: var(--bg);
        }}
        .explorer-context {{
            width: 300px;
            background: var(--bg-block);
            border-left: 1px solid var(--border);
            padding: 1rem;
            overflow-y: auto;
            font-size: 0.85rem;
        }}
        .explorer-context h3 {{
            color: var(--accent);
            font-size: 1.1rem;
            margin: 0 0 0.5rem 0;
            font-family: monospace;
        }}
        .explorer-context .badge {{
            display: inline-block;
            background: var(--bg);
            color: var(--text-dim);
            padding: 0.15rem 0.4rem;
            border-radius: 3px;
            font-size: 0.7rem;
            text-transform: uppercase;
            margin-bottom: 0.75rem;
        }}
        .explorer-context .badge.entry {{ background: var(--green); color: var(--bg); }}
        .explorer-context .badge.exit {{ background: #bd93f9; color: var(--bg); }}
        .explorer-context .badge.branchpoint {{ background: #ffb86c; color: var(--bg); }}
        .explorer-context .badge.mergepoint {{ background: var(--accent); color: var(--bg); }}
        .explorer-context .summary {{ color: var(--text-dim); margin-bottom: 1rem; }}
        .explorer-context h4 {{
            color: var(--text-dim);
            font-size: 0.7rem;
            text-transform: uppercase;
            letter-spacing: 0.05em;
            margin: 1rem 0 0.5rem 0;
        }}
        .explorer-context ul {{
            list-style: none;
            padding: 0;
            margin: 0;
        }}
        .explorer-context li {{
            padding: 0.3rem 0;
            border-bottom: 1px solid rgba(255,255,255,0.05);
            font-family: monospace;
        }}
        .explorer-context .mir {{ color: var(--green); }}
        .explorer-context .annotation {{ color: #bd93f9; font-size: 0.75rem; display: block; }}
        .explorer-context .term {{ color: var(--pink); }}
        .explorer-context .locals-list {{ max-height: 120px; overflow-y: auto; font-size: 0.8rem; color: var(--text-dim); }}
        .explorer-edges {{
            display: flex;
            flex-direction: column;
            gap: 0.4rem;
            margin-top: 0.5rem;
        }}
        .edge-btn {{
            background: var(--bg);
            border: 1px solid var(--border);
            color: var(--text);
            padding: 0.5rem;
            border-radius: 4px;
            cursor: pointer;
            text-align: left;
            font-size: 0.8rem;
        }}
        .edge-btn:hover {{ border-color: var(--accent); }}
        .edge-btn .target {{ color: var(--green); font-family: monospace; }}
        .edge-btn .hint {{ color: var(--text-dim); font-size: 0.7rem; display: block; }}
        .edge-btn.cleanup {{ border-color: #ff5555; border-style: dashed; }}
        .explorer-path {{
            padding: 0.5rem 1rem;
            background: var(--bg);
            font-family: monospace;
            font-size: 0.85rem;
            border-bottom: 1px solid var(--border);
            display: flex;
            align-items: center;
            gap: 0.5rem;
        }}
        .explorer-path .label {{ color: var(--text-dim); }}
        .explorer-path .crumb {{ color: var(--text-dim); }}
        .explorer-path .crumb.current {{ color: var(--pink); font-weight: bold; }}
        .explorer-path button {{
            margin-left: auto;
            background: var(--bg-section);
            border: 1px solid var(--border);
            color: var(--text);
            padding: 0.2rem 0.6rem;
            border-radius: 3px;
            cursor: pointer;
            font-size: 0.8rem;
        }}
    </style>
    <link rel="stylesheet" href="https://cdn.jsdelivr.net/gh/highlightjs/cdn-release@11.9.0/build/styles/github-dark.min.css">
    <script src="https://cdn.jsdelivr.net/gh/highlightjs/cdn-release@11.9.0/build/highlight.min.js"></script>
    <script src="https://cdn.jsdelivr.net/gh/highlightjs/cdn-release@11.9.0/build/languages/rust.min.js"></script>
    <script src="https://cdn.jsdelivr.net/npm/svg-pan-zoom@3.6.1/dist/svg-pan-zoom.min.js"></script>
    <script src="https://unpkg.com/cytoscape@3.28.1/dist/cytoscape.min.js"></script>
</head>
<body>
    <h1>{}</h1>
    {}
    <script>
    {explorer_js}
    </script>
</body>
</html>"#,
        escape_html(&smir.name),
        escape_html(&smir.name),
        content,
        explorer_js = EXPLORER_JS
    )
}

/// Render a basic block as HTML
fn render_block_html(idx: usize, rows: &[AnnotatedRow]) -> String {
    let mut html = format!(
        r#"    <div class="block">
        <div class="block-header">bb{}</div>
        <table class="annotated-table">
            <thead>
                <tr><th class="col-source">Source</th><th class="col-mir">MIR</th><th class="col-annot">Annotation</th></tr>
            </thead>
            <tbody>
"#,
        idx
    );

    for row in rows {
        let mut classes = Vec::new();
        if row.is_terminator {
            classes.push("terminator");
        }
        if row.is_recursive {
            classes.push("recursive");
        }
        let class_attr = if classes.is_empty() {
            String::new()
        } else {
            format!(" class=\"{}\"", classes.join(" "))
        };
        let source = if row.source.is_empty() {
            "<span class=\"empty\">—</span>".to_string()
        } else {
            escape_html(&row.source)
        };
        html.push_str(&format!(
            r#"                <tr{}>
                    <td class="col-source">{}</td>
                    <td class="col-mir">{}</td>
                    <td class="col-annot">{}</td>
                </tr>
"#,
            class_attr,
            source,
            escape_html(&row.mir),
            escape_html(&row.annotation)
        ));
    }

    html.push_str("            </tbody>\n        </table>\n    </div>\n");
    html
}

/// Render the explorer panel for a function
fn render_explorer_panel(fn_id: &str, explorer_fn: &ExplorerFunction) -> String {
    let json_data = serde_json::to_string(explorer_fn).unwrap_or_else(|_| "{}".to_string());
    let safe_id = fn_id.replace(|c: char| !c.is_alphanumeric(), "_");

    format!(
        r##"    <details class="explorer-section">
        <summary>Interactive Graph Explorer</summary>
        <div class="explorer-path" id="path-{id}">
            <span class="label">PATH:</span>
            <span class="breadcrumb"></span>
            <button onclick="explorers['{id}'].reset()">Reset</button>
            <button onclick="explorers['{id}'].goBack()">← Back</button>
        </div>
        <div class="explorer-container">
            <div class="explorer-graph" id="cy-{id}"></div>
            <div class="explorer-context" id="ctx-{id}">
                <h3 id="block-{id}">bb0</h3>
                <span class="badge" id="role-{id}">entry</span>
                <div class="summary" id="summary-{id}"></div>
                <h4>Locals</h4>
                <ul id="locals-{id}" class="locals-list"></ul>
                <h4>Statements</h4>
                <ul id="stmts-{id}"></ul>
                <h4>Terminator</h4>
                <div id="term-{id}"></div>
                <h4>Next</h4>
                <div class="explorer-edges" id="edges-{id}"></div>
            </div>
        </div>
        <script>
            (function() {{
                const data = {json};
                if (typeof explorers === 'undefined') window.explorers = {{}};

                // Wait for details to open before initializing
                const details = document.currentScript.closest('details');
                let initialized = false;

                details.addEventListener('toggle', function() {{
                    if (details.open && !initialized) {{
                        initialized = true;
                        setTimeout(() => initExplorer('{id}', data), 50);
                    }}
                }});
            }})();
        </script>
    </details>
"##,
        id = safe_id,
        json = json_data
    )
}

/// Render a basic block as annotated rows
fn render_block_rows(
    block: &BasicBlock,
    span_index: &HashMap<usize, &SpanInfo>,
    current_fn: &str,
) -> Vec<AnnotatedRow> {
    let mut rows = Vec::new();

    // Process each statement
    for stmt in &block.statements {
        let (mir, annotation) = render_statement_annotated(stmt);
        let source = extract_statement_source(stmt, span_index);
        rows.push(AnnotatedRow {
            source,
            mir,
            annotation,
            is_terminator: false,
            is_recursive: false,
        });
    }

    // Process terminator
    let (mir, annotation, is_recursive) =
        render_terminator_annotated(&block.terminator, current_fn);
    let source = extract_terminator_source(&block.terminator, span_index);
    rows.push(AnnotatedRow {
        source,
        mir: format!("→ {}", mir),
        annotation,
        is_terminator: true,
        is_recursive,
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

/// Render a terminator with annotation, returns (mir, annotation, is_recursive)
fn render_terminator_annotated(term: &Terminator, current_fn: &str) -> (String, String, bool) {
    match &term.kind {
        TerminatorKind::Goto { target } => (
            format!("goto bb{target}"),
            format!("Jump to bb{target}"),
            false,
        ),
        TerminatorKind::Return {} => (
            "return".to_string(),
            "Return from function".to_string(),
            false,
        ),
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

            // Check if this is a recursive call
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

/// Render a place (lvalue)
fn render_place(place: &Place) -> String {
    let mut s = format!("_{}", place.local);
    for proj in &place.projection {
        match proj {
            stable_mir::mir::ProjectionElem::Deref => s = format!("(*{})", s),
            stable_mir::mir::ProjectionElem::Field(idx, _) => s = format!("{}.{}", s, idx),
            stable_mir::mir::ProjectionElem::Index(local) => s = format!("{}[_{}]", s, local),
            stable_mir::mir::ProjectionElem::Downcast(idx) => s = format!("({} as #{:?})", s, idx),
            _ => s = format!("{}.[proj]", s),
        }
    }
    s
}

/// Render an operand
fn render_operand(op: &Operand) -> String {
    match op {
        Operand::Copy(place) => render_place(place),
        Operand::Move(place) => format!("move {}", render_place(place)),
        Operand::Constant(c) => render_mir_const(&c.const_),
    }
}

/// Render a MIR constant
fn render_mir_const(c: &stable_mir::ty::MirConst) -> String {
    use stable_mir::ty::ConstantKind;
    match c.kind() {
        ConstantKind::Allocated(alloc) => {
            if let Some(val) = bytes_to_int(&alloc.bytes) {
                val.to_string()
            } else {
                format!("[{} bytes]", alloc.bytes.len())
            }
        }
        ConstantKind::ZeroSized => "()".to_string(),
        _ => "const".to_string(),
    }
}

/// Render a type constant
fn render_ty_const(c: &stable_mir::ty::TyConst) -> String {
    format!("{:?}", c)
}

/// Render an rvalue
fn render_rvalue(rv: &Rvalue) -> String {
    match rv {
        Rvalue::Use(op) => render_operand(op),
        Rvalue::Repeat(op, count) => {
            format!("[{}; {}]", render_operand(op), render_ty_const(count))
        }
        Rvalue::Ref(_, bk, place) => {
            let prefix = match bk {
                stable_mir::mir::BorrowKind::Shared => "&",
                stable_mir::mir::BorrowKind::Mut { .. } => "&mut ",
                _ => "&?",
            };
            format!("{}{}", prefix, render_place(place))
        }
        Rvalue::AddressOf(_, place) => format!("&raw {}", render_place(place)),
        Rvalue::Len(place) => format!("len({})", render_place(place)),
        Rvalue::Cast(kind, op, ty) => {
            let kind_str = match kind {
                CastKind::IntToInt => "as",
                CastKind::PointerCoercion(_) => "as",
                _ => "cast",
            };
            format!("{} {} {:?}", render_operand(op), kind_str, ty.kind())
        }
        Rvalue::BinaryOp(binop, lhs, rhs) => {
            format!(
                "{} {} {}",
                render_operand(lhs),
                render_binop(binop),
                render_operand(rhs)
            )
        }
        Rvalue::CheckedBinaryOp(binop, lhs, rhs) => {
            format!(
                "checked({} {} {})",
                render_operand(lhs),
                render_binop(binop),
                render_operand(rhs)
            )
        }
        Rvalue::UnaryOp(unop, op) => {
            format!("{}{}", render_unop(unop), render_operand(op))
        }
        Rvalue::NullaryOp(nullop, ty) => {
            let op = match nullop {
                NullOp::SizeOf => "sizeof",
                NullOp::AlignOf => "alignof",
                NullOp::OffsetOf(_) => "offsetof",
                NullOp::UbChecks => "ub_checks",
            };
            format!("{}({:?})", op, ty.kind())
        }
        Rvalue::Discriminant(place) => format!("discr({})", render_place(place)),
        Rvalue::Aggregate(kind, ops) => {
            let ops_str: Vec<String> = ops.iter().map(render_operand).collect();
            format!("{:?}({})", kind, ops_str.join(", "))
        }
        Rvalue::ShallowInitBox(op, _) => format!("box {}", render_operand(op)),
        Rvalue::CopyForDeref(place) => format!("copy_deref({})", render_place(place)),
        Rvalue::ThreadLocalRef(_) => "thread_local".to_string(),
    }
}

/// Render binary operator
fn render_binop(op: &BinOp) -> &'static str {
    match op {
        BinOp::Add | BinOp::AddUnchecked => "+",
        BinOp::Sub | BinOp::SubUnchecked => "-",
        BinOp::Mul | BinOp::MulUnchecked => "*",
        BinOp::Div => "/",
        BinOp::Rem => "%",
        BinOp::BitXor => "^",
        BinOp::BitAnd => "&",
        BinOp::BitOr => "|",
        BinOp::Shl | BinOp::ShlUnchecked => "<<",
        BinOp::Shr | BinOp::ShrUnchecked => ">>",
        BinOp::Eq => "==",
        BinOp::Lt => "<",
        BinOp::Le => "<=",
        BinOp::Ne => "!=",
        BinOp::Ge => ">=",
        BinOp::Gt => ">",
        BinOp::Cmp => "<=>",
        BinOp::Offset => "offset",
    }
}

/// Render unary operator
fn render_unop(op: &UnOp) -> &'static str {
    match op {
        UnOp::Not => "!",
        UnOp::Neg => "-",
        UnOp::PtrMetadata => "metadata",
    }
}

/// Generate annotation for an rvalue
fn annotate_rvalue(rv: &Rvalue) -> String {
    match rv {
        Rvalue::Use(Operand::Constant(_)) => "Load constant".to_string(),
        Rvalue::Use(Operand::Copy(_)) => "Copy value".to_string(),
        Rvalue::Use(Operand::Move(_)) => "Move value".to_string(),
        Rvalue::Ref(_, stable_mir::mir::BorrowKind::Shared, _) => "Shared borrow".to_string(),
        Rvalue::Ref(_, stable_mir::mir::BorrowKind::Mut { .. }, _) => "Mutable borrow".to_string(),
        Rvalue::BinaryOp(op, _, _) => format!("{} operation", op_name(op)),
        Rvalue::CheckedBinaryOp(op, _, _) => format!("Checked {} (may panic)", op_name(op)),
        Rvalue::UnaryOp(UnOp::Not, _) => "Bitwise/logical NOT".to_string(),
        Rvalue::UnaryOp(UnOp::Neg, _) => "Negation".to_string(),
        Rvalue::Cast(CastKind::IntToInt, _, _) => "Integer conversion".to_string(),
        Rvalue::Cast(CastKind::PointerCoercion(_), _, _) => "Pointer coercion".to_string(),
        Rvalue::Len(_) => "Get length".to_string(),
        Rvalue::Discriminant(_) => "Get enum discriminant".to_string(),
        Rvalue::Aggregate(_, _) => "Construct aggregate".to_string(),
        Rvalue::AddressOf(_, _) => "Get raw pointer".to_string(),
        _ => String::new(),
    }
}

/// Human-readable operator name
fn op_name(op: &BinOp) -> &'static str {
    match op {
        BinOp::Add | BinOp::AddUnchecked => "Add",
        BinOp::Sub | BinOp::SubUnchecked => "Subtract",
        BinOp::Mul | BinOp::MulUnchecked => "Multiply",
        BinOp::Div => "Divide",
        BinOp::Rem => "Remainder",
        BinOp::BitXor => "XOR",
        BinOp::BitAnd => "AND",
        BinOp::BitOr => "OR",
        BinOp::Shl | BinOp::ShlUnchecked => "Shift left",
        BinOp::Shr | BinOp::ShrUnchecked => "Shift right",
        BinOp::Eq => "Equal",
        BinOp::Lt => "Less than",
        BinOp::Le => "Less or equal",
        BinOp::Ne => "Not equal",
        BinOp::Ge => "Greater or equal",
        BinOp::Gt => "Greater than",
        BinOp::Cmp => "Compare",
        BinOp::Offset => "Offset",
    }
}

/// Extract function name from call operand
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

/// Extract source line for a statement using its span
fn extract_statement_source(stmt: &Statement, span_index: &HashMap<usize, &SpanInfo>) -> String {
    // First try the statement's own span
    let span_id = stmt.span.to_index();
    if let Some(info) = span_index.get(&span_id) {
        if let Some(line) = extract_source_line(info) {
            return line;
        }
    }

    // Fall back to operand spans for constants
    let span_id = match &stmt.kind {
        StatementKind::Assign(_, rvalue) => get_rvalue_span(rvalue),
        _ => None,
    };

    span_id
        .and_then(|id| span_index.get(&id))
        .and_then(|info| extract_source_line(info))
        .unwrap_or_default()
}

/// Extract source line for a terminator using its span
fn extract_terminator_source(term: &Terminator, span_index: &HashMap<usize, &SpanInfo>) -> String {
    // First try the terminator's own span
    let span_id = term.span.to_index();
    if let Some(info) = span_index.get(&span_id) {
        if let Some(line) = extract_source_line(info) {
            return line;
        }
    }

    // Fall back to operand spans
    let span_id = match &term.kind {
        TerminatorKind::Call { func, .. } => get_operand_span(func),
        TerminatorKind::Assert { cond, .. } => get_operand_span(cond),
        _ => None,
    };

    span_id
        .and_then(|id| span_index.get(&id))
        .and_then(|info| extract_source_line(info))
        .unwrap_or_default()
}

/// Extract a single source line from span info
fn extract_source_line(info: &SpanInfo) -> Option<String> {
    let (file, start_line, _, _, _) = info;

    if file.contains(".rustup") || file.contains("no-location") {
        return None;
    }

    let content = std::fs::read_to_string(file).ok()?;
    let lines: Vec<&str> = content.lines().collect();
    let line_idx = start_line.saturating_sub(1);

    lines.get(line_idx).map(|s| s.trim().to_string())
}

/// Get span from rvalue
fn get_rvalue_span(rvalue: &Rvalue) -> Option<usize> {
    match rvalue {
        Rvalue::Use(op) => get_operand_span(op),
        Rvalue::BinaryOp(_, op1, _) | Rvalue::CheckedBinaryOp(_, op1, _) => get_operand_span(op1),
        _ => None,
    }
}

/// Get span from operand
fn get_operand_span(op: &Operand) -> Option<usize> {
    match op {
        Operand::Constant(c) => Some(c.span.to_index()),
        _ => None,
    }
}

/// Try to interpret bytes as an integer
fn bytes_to_int(bytes: &[Option<u8>]) -> Option<i128> {
    if bytes.iter().any(|b| b.is_none()) {
        return None;
    }

    let bytes: Vec<u8> = bytes.iter().map(|b| b.unwrap()).collect();

    match bytes.len() {
        1 => Some(bytes[0] as i128),
        2 => Some(i16::from_le_bytes(bytes.try_into().ok()?) as i128),
        4 => Some(i32::from_le_bytes(bytes.try_into().ok()?) as i128),
        8 => Some(i64::from_le_bytes(bytes.try_into().ok()?) as i128),
        _ => None,
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

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

const EXPLORER_JS: &str = r##"
function initExplorer(id, data) {
    const cy = cytoscape({
        container: document.getElementById('cy-' + id),
        elements: buildElements(data),
        style: [
            { selector: 'node', style: {
                'label': 'data(label)',
                'text-valign': 'center',
                'text-halign': 'center',
                'background-color': '#3a3a5e',
                'color': '#eee',
                'font-size': '11px',
                'font-family': 'monospace',
                'width': 50,
                'height': 30,
                'shape': 'roundrectangle',
                'border-width': 2,
                'border-color': '#555'
            }},
            { selector: 'node.entry', style: { 'border-color': '#50fa7b', 'border-width': 3 }},
            { selector: 'node.exit', style: { 'border-color': '#bd93f9', 'border-width': 3 }},
            { selector: 'node.branchpoint', style: { 'border-color': '#ffb86c', 'border-width': 3 }},
            { selector: 'node.visited', style: { 'background-color': '#2a4a6e' }},
            { selector: 'node.current', style: {
                'background-color': '#50fa7b',
                'color': '#1a1a2e',
                'border-color': '#50fa7b',
                'font-weight': 'bold'
            }},
            { selector: 'node.dim', style: { 'opacity': 0.35 }},
            { selector: 'edge', style: {
                'width': 2,
                'line-color': '#555',
                'target-arrow-color': '#555',
                'target-arrow-shape': 'triangle',
                'curve-style': 'bezier',
                'label': 'data(label)',
                'font-size': '9px',
                'color': '#888'
            }},
            { selector: 'edge.cleanup', style: {
                'line-style': 'dashed',
                'line-color': '#ff5555',
                'target-arrow-color': '#ff5555'
            }},
            { selector: 'edge.taken', style: {
                'line-color': '#50fa7b',
                'target-arrow-color': '#50fa7b',
                'width': 3
            }}
        ],
        layout: { name: 'breadthfirst', directed: true, padding: 30, spacingFactor: 1.2 }
    });

    const state = { path: [], current: null, data: data, cy: cy, id: id };
    explorers[id] = {
        goTo: (blockId) => goToBlock(state, blockId),
        goBack: () => goBack(state),
        reset: () => resetExplorer(state)
    };

    cy.on('tap', 'node', (e) => goToBlock(state, parseInt(e.target.id().replace('bb', ''))));
    cy.on('tap', 'edge', (e) => goToBlock(state, parseInt(e.target.target().id().replace('bb', ''))));

    goToBlock(state, data.entry_block);
}

function buildElements(data) {
    const nodes = data.blocks.map(b => ({
        data: { id: 'bb' + b.id, label: 'bb' + b.id },
        classes: b.role
    }));
    const edges = [];
    for (const block of data.blocks) {
        for (const edge of block.terminator.edges) {
            edges.push({
                data: { id: 'bb' + block.id + '-bb' + edge.target, source: 'bb' + block.id, target: 'bb' + edge.target, label: edge.label },
                classes: edge.kind === 'cleanup' ? 'cleanup' : ''
            });
        }
    }
    return { nodes, edges };
}

function goToBlock(state, blockId) {
    const { data, cy, id } = state;
    if (blockId < 0 || blockId >= data.blocks.length) return;
    const block = data.blocks[blockId];

    if (state.current !== null && state.current !== blockId) {
        if (state.path[state.path.length - 1] !== state.current) {
            state.path.push(state.current);
        }
    }
    state.current = blockId;

    cy.nodes().removeClass('current visited dim');
    cy.edges().removeClass('taken');
    const visited = new Set([...state.path, blockId]);
    state.path.forEach(v => cy.$('#bb' + v).addClass('visited'));
    cy.$('#bb' + blockId).addClass('current');
    cy.nodes().forEach(n => {
        if (!visited.has(parseInt(n.id().replace('bb', '')))) n.addClass('dim');
    });
    for (let i = 0; i < state.path.length; i++) {
        const from = state.path[i], to = i + 1 < state.path.length ? state.path[i + 1] : blockId;
        cy.$('#bb' + from + '-bb' + to).addClass('taken');
    }
    cy.animate({ center: { eles: cy.$('#bb' + blockId) }, duration: 150 });

    updateContext(state, block);
    updatePath(state);
}

function goBack(state) {
    if (state.path.length > 0) {
        const prev = state.path.pop();
        state.current = null;
        goToBlock(state, prev);
    }
}

function resetExplorer(state) {
    state.path = [];
    state.current = null;
    state.cy.nodes().removeClass('current visited dim');
    state.cy.edges().removeClass('taken');
    goToBlock(state, state.data.entry_block);
}

function updateContext(state, block) {
    const { id, data } = state;
    document.getElementById('block-' + id).textContent = 'bb' + block.id;
    const badge = document.getElementById('role-' + id);
    badge.textContent = block.role;
    badge.className = 'badge ' + block.role;
    document.getElementById('summary-' + id).textContent = block.summary;

    // Populate locals
    const localsList = document.getElementById('locals-' + id);
    localsList.innerHTML = data.locals.map(l => '<li>' + esc(l) + '</li>').join('');

    const stmts = document.getElementById('stmts-' + id);
    stmts.innerHTML = block.statements.length === 0 ? '<li style="color:#888">(none)</li>' :
        block.statements.map(s => '<li><span class="mir">' + esc(s.mir) + '</span><span class="annotation">' + esc(s.annotation) + '</span></li>').join('');

    document.getElementById('term-' + id).innerHTML =
        '<span class="term">' + esc(block.terminator.mir) + '</span><span class="annotation">' + esc(block.terminator.annotation) + '</span>';

    const edges = document.getElementById('edges-' + id);
    edges.innerHTML = block.terminator.edges.map(e =>
        '<button class="edge-btn' + (e.kind === 'cleanup' ? ' cleanup' : '') + '" onclick="explorers[\'' + id + '\'].goTo(' + e.target + ')">' +
        '<span class="target">→ bb' + e.target + '</span> ' + (e.label ? '<span class="label">' + esc(e.label) + '</span>' : '') +
        '<span class="hint">' + esc(e.annotation) + '</span></button>'
    ).join('');
}

function updatePath(state) {
    const crumb = document.querySelector('#path-' + state.id + ' .breadcrumb');
    const full = [...state.path, state.current];
    crumb.innerHTML = full.map((b, i) =>
        '<span class="crumb' + (i === full.length - 1 ? ' current' : '') + '">bb' + b + '</span>'
    ).join(' → ');
}

function esc(s) {
    if (!s) return '';
    return s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
}
"##;
