//! Graph generation from MIR data.
//!
//! This module provides functionality to convert stable MIR JSON representation
//! into graph formats (DOT/Graphviz, D2, Markdown, Typst) for visualization.
//!
//! # Module Structure
//!
//! - `context`: GraphContext for rendering graph labels with resolved indices
//! - `index`: Index structures for resolving MIR references (allocs, types, spans)
//! - `util`: Utility functions and traits for graph generation
//! - `output/dot`: DOT (Graphviz) format output
//! - `output/d2`: D2 diagram format output
//! - `output/markdown`: Markdown annotated output
//! - `output/typst`: Typst document output

use std::fs::File;
use std::io::{self, Write};

extern crate rustc_middle;
use rustc_middle::ty::TyCtxt;

extern crate rustc_session;
use rustc_session::config::{OutFileName, OutputType};

extern crate stable_mir;

use crate::printer::{collect_smir, SmirJson};

pub mod context;
pub mod index;
mod output;
pub mod util;

// Re-export commonly used items
pub use context::GraphContext;
pub use index::{
    AllocEntry, AllocIndex, AllocKind, FunctionKey, SpanData, SpanIndex, SpanInfo, TypeIndex,
};
pub use util::{GraphLabelString, MAX_NUMERIC_BYTES, MAX_STRING_PREVIEW_LEN};

// =============================================================================
// Entry Points
// =============================================================================

/// Helper to emit a graph file in a given format
fn emit_graph_file<F>(tcx: TyCtxt<'_>, extension: &str, generate: F)
where
    F: FnOnce(SmirJson<'_>) -> String,
{
    let content = generate(collect_smir(tcx));
    match tcx.output_filenames(()).path(OutputType::Mir) {
        OutFileName::Stdout => {
            write!(io::stdout(), "{}", content)
                .unwrap_or_else(|_| panic!("Failed to write {}", extension));
        }
        OutFileName::Real(path) => {
            let out_path = path.with_extension(extension);
            let mut b = io::BufWriter::new(
                File::create(&out_path)
                    .unwrap_or_else(|e| panic!("Failed to create {}: {}", out_path.display(), e)),
            );
            write!(b, "{}", content)
                .unwrap_or_else(|_| panic!("Failed to write {}", extension));
        }
    }
}

/// Emit MIR as a DOT (Graphviz) file
pub fn emit_dotfile(tcx: TyCtxt<'_>) {
    emit_graph_file(tcx, "smir.dot", |s| s.to_dot_file());
}

/// Emit MIR as a D2 diagram file
pub fn emit_d2file(tcx: TyCtxt<'_>) {
    emit_graph_file(tcx, "smir.d2", |s| s.to_d2_file());
}

// Re-export document format entry points
pub use output::markdown::emit_mdfile;
pub use output::typst::emit_typstfile;
