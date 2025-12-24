//! GraphContext provides resolved indices and rendering methods for graph generation.

use std::collections::HashMap;

extern crate stable_mir;
use stable_mir::mir::{
    BorrowKind, ConstOperand, Mutability, NonDivergingIntrinsic, Operand, Rvalue, Statement,
    StatementKind, Terminator, TerminatorKind,
};
use stable_mir::ty::{ConstDef, ConstantKind, IndexedVal, MirConst, Ty};
use stable_mir::CrateDef;

use crate::printer::SmirJson;

use super::index::{AllocIndex, FunctionKey, SpanIndex, TypeIndex};
use super::util::{bytes_to_u64_le, short_fn_name, GraphLabelString};
use super::MAX_NUMERIC_BYTES;

/// Context for rendering graph labels with access to indices
pub struct GraphContext {
    pub allocs: AllocIndex,
    pub types: TypeIndex,
    /// Primary function lookup using full key (Ty + instance descriptor).
    /// Prevents collisions for generic functions with different instantiations.
    pub functions: HashMap<FunctionKey, String>,
    /// Fallback lookup by Ty only, used when instance info isn't available
    /// at the call site (e.g., when resolving from just a type).
    pub functions_by_ty: HashMap<Ty, String>,
    pub uneval_consts: HashMap<ConstDef, String>,
    pub spans: SpanIndex,
    pub show_spans: bool,
    /// When DEBUG is set, extra info is available
    pub show_debug: bool,
    /// Debug: function source info (where functions are referenced from)
    pub fn_sources: HashMap<FunctionKey, String>,
}

impl GraphContext {
    pub fn from_smir(smir: &SmirJson) -> Self {
        let types = TypeIndex::from_types(&smir.types);
        let allocs = AllocIndex::from_alloc_infos(&smir.allocs, &types);

        // Build both function maps: full key and Ty-only fallback
        let mut functions = HashMap::new();
        let mut functions_by_ty = HashMap::new();
        for (k, v) in &smir.functions {
            let name = super::util::function_string(v.clone());
            let key = FunctionKey {
                ty: k.0,
                instance_desc: k.instance_desc(),
            };
            functions.insert(key, name.clone());
            functions_by_ty.insert(k.0, name);
        }

        let uneval_consts: HashMap<ConstDef, String> =
            smir.uneval_consts.iter().cloned().collect();
        let spans = SpanIndex::from_spans(&smir.spans);
        let show_spans = std::env::var("SHOW_SPANS").is_ok();

        // Extract debug info if available
        let show_debug = smir.debug.is_some();
        let fn_sources = smir
            .debug
            .as_ref()
            .map(|d| {
                d.fn_sources()
                    .iter()
                    .map(|(k, source)| {
                        let key = FunctionKey {
                            ty: k.0,
                            instance_desc: k.instance_desc(),
                        };
                        (key, source.clone())
                    })
                    .collect()
            })
            .unwrap_or_default();

        Self {
            allocs,
            types,
            functions,
            functions_by_ty,
            uneval_consts,
            spans,
            show_spans,
            show_debug,
            fn_sources,
        }
    }

    /// Render a constant operand with alloc information
    pub fn render_const(&self, const_: &MirConst) -> String {
        let ty = const_.ty();
        let ty_name = self.types.get_name(ty);

        match const_.kind() {
            ConstantKind::Allocated(alloc) => {
                // Check if this constant references any allocs via provenance
                if !alloc.provenance.ptrs.is_empty() {
                    // Use depth 2 to show nested references without explosion
                    let alloc_refs: Vec<String> = alloc
                        .provenance
                        .ptrs
                        .iter()
                        .map(|(_offset, prov)| {
                            self.allocs.describe_with_refs(prov.0.to_index() as u64, 2)
                        })
                        .collect();
                    format!("const [{}]", alloc_refs.join(", "))
                } else {
                    // Inline constant - try to show value
                    let bytes = &alloc.bytes;
                    // Convert Option<u8> to concrete bytes
                    let concrete_bytes: Vec<u8> = bytes.iter().filter_map(|&b| b).collect();
                    if concrete_bytes.len() <= MAX_NUMERIC_BYTES && !concrete_bytes.is_empty() {
                        format!("const {}_{}", bytes_to_u64_le(&concrete_bytes), ty_name)
                    } else {
                        format!("const {}", ty_name)
                    }
                }
            }
            ConstantKind::ZeroSized => {
                // Function pointers, unit type, etc.
                if ty.kind().is_fn() {
                    if let Some(name) = self.functions_by_ty.get(&ty) {
                        format!("const fn {}", short_fn_name(name))
                    } else {
                        format!("const {}", ty_name)
                    }
                } else {
                    format!("const {}", ty_name)
                }
            }
            ConstantKind::Ty(_) => format!("const {}", ty_name),
            ConstantKind::Unevaluated(uneval) => self
                .uneval_consts
                .get(&uneval.def)
                .map(|name| format!("const {}", name))
                .unwrap_or_else(|| format!("const unevaluated {}", ty_name)),
            ConstantKind::Param(_) => format!("const param {}", ty_name),
        }
    }

    /// Render an operand with context
    pub fn render_operand(&self, op: &Operand) -> String {
        match op {
            Operand::Constant(ConstOperand { const_, .. }) => self.render_const(const_),
            Operand::Copy(place) => format!("cp({})", place.label()),
            Operand::Move(place) => format!("mv({})", place.label()),
        }
    }

    /// Format a span suffix if SHOW_SPANS is enabled
    pub fn span_suffix(&self, span: &stable_mir::ty::Span) -> String {
        if !self.show_spans {
            return String::new();
        }
        self.spans
            .get(span.to_index())
            .map(|info| format!(" @ {}", info.short()))
            .unwrap_or_default()
    }

    /// Get debug source info for a function if DEBUG is enabled
    pub fn fn_source_suffix(&self, ty: Ty) -> String {
        if !self.show_debug {
            return String::new();
        }
        // Look up by Ty with None instance_desc as fallback
        let key = FunctionKey {
            ty,
            instance_desc: None,
        };
        self.fn_sources
            .get(&key)
            .map(|s| format!(" [{}]", s))
            .unwrap_or_default()
    }

    /// Generate the allocs legend as lines for display
    pub fn allocs_legend_lines(&self) -> Vec<String> {
        let mut lines = vec!["ALLOCS".to_string()];
        let mut entries: Vec<_> = self.allocs.iter().collect();
        entries.sort_by_key(|e| e.alloc_id);
        for entry in entries {
            lines.push(entry.short_description());
        }
        lines
    }

    /// Resolve a call target to a function name if it's a constant function pointer
    pub fn resolve_call_target(&self, func: &Operand) -> Option<String> {
        match func {
            Operand::Constant(ConstOperand { const_, .. }) => {
                let ty = const_.ty();
                if ty.kind().is_fn() {
                    self.functions_by_ty.get(&ty).cloned()
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// Render a statement with context
    pub fn render_stmt(&self, s: &Statement) -> String {
        use StatementKind::*;
        let span_suffix = self.span_suffix(&s.span);
        let base = match &s.kind {
            Assign(p, v) => format!("{} <- {}", p.label(), self.render_rvalue(v)),
            FakeRead(_cause, p) => format!("Fake-Read {}", p.label()),
            SetDiscriminant {
                place,
                variant_index,
            } => format!(
                "set discriminant {}({})",
                place.label(),
                variant_index.to_index()
            ),
            Deinit(p) => format!("Deinit {}", p.label()),
            StorageLive(l) => format!("Storage Live _{}", &l),
            StorageDead(l) => format!("Storage Dead _{}", &l),
            Retag(_retag_kind, p) => format!("Retag {}", p.label()),
            PlaceMention(p) => format!("Mention {}", p.label()),
            AscribeUserType {
                place,
                projections,
                variance: _,
            } => format!("Ascribe {}.{}", place.label(), projections.base),
            Coverage(_) => "Coverage".to_string(),
            Intrinsic(intr) => format!("Intr: {}", self.render_intrinsic(intr)),
            ConstEvalCounter {} => "ConstEvalCounter".to_string(),
            Nop {} => "Nop".to_string(),
        };
        format!("{}{}", base, span_suffix)
    }

    /// Render rvalue with context
    pub fn render_rvalue(&self, v: &Rvalue) -> String {
        use Rvalue::*;
        match v {
            AddressOf(mutability, p) => match mutability {
                Mutability::Not => format!("&raw {}", p.label()),
                Mutability::Mut => format!("&raw mut {}", p.label()),
            },
            Aggregate(kind, operands) => {
                let os: Vec<String> = operands.iter().map(|op| self.render_operand(op)).collect();
                format!("{} ({})", kind.label(), os.join(", "))
            }
            BinaryOp(binop, op1, op2) => format!(
                "{:?}({}, {})",
                binop,
                self.render_operand(op1),
                self.render_operand(op2)
            ),
            Cast(kind, op, _ty) => format!("Cast-{:?} {}", kind, self.render_operand(op)),
            CheckedBinaryOp(binop, op1, op2) => {
                format!(
                    "chkd-{:?}({}, {})",
                    binop,
                    self.render_operand(op1),
                    self.render_operand(op2)
                )
            }
            CopyForDeref(p) => format!("CopyForDeref({})", p.label()),
            Discriminant(p) => format!("Discriminant({})", p.label()),
            Len(p) => format!("Len({})", p.label()),
            Ref(_region, borrowkind, p) => {
                format!(
                    "&{} {}",
                    match borrowkind {
                        BorrowKind::Mut { kind: _ } => "mut",
                        _other => "",
                    },
                    p.label()
                )
            }
            Repeat(op, _ty_const) => format!("Repeat {}", self.render_operand(op)),
            ShallowInitBox(op, _ty) => format!("ShallowInitBox({})", self.render_operand(op)),
            ThreadLocalRef(item) => format!("ThreadLocalRef({})", item.name()),
            NullaryOp(nullop, ty) => format!("{} :: {}", nullop.label(), ty),
            UnaryOp(unop, op) => format!("{:?}({})", unop, self.render_operand(op)),
            Use(op) => format!("Use({})", self.render_operand(op)),
        }
    }

    /// Render intrinsic with context
    pub fn render_intrinsic(&self, intr: &NonDivergingIntrinsic) -> String {
        use NonDivergingIntrinsic::*;
        match intr {
            Assume(op) => format!("Assume {}", self.render_operand(op)),
            CopyNonOverlapping(c) => format!(
                "CopyNonOverlapping: {} <- {}({})",
                self.render_operand(&c.dst),
                self.render_operand(&c.src),
                self.render_operand(&c.count)
            ),
        }
    }

    /// Render terminator with context for alloc/type information
    pub fn render_terminator(&self, term: &Terminator) -> String {
        use TerminatorKind::*;
        let span_suffix = self.span_suffix(&term.span);
        let base = match &term.kind {
            Goto { .. } => "Goto".to_string(),
            SwitchInt { discr, .. } => format!("SwitchInt {}", self.render_operand(discr)),
            Resume {} => "Resume".to_string(),
            Abort {} => "Abort".to_string(),
            Return {} => "Return".to_string(),
            Unreachable {} => "Unreachable".to_string(),
            Drop { place, .. } => format!("Drop {}", place.label()),
            Call {
                func,
                args,
                destination,
                ..
            } => {
                let fn_name = self
                    .resolve_call_target(func)
                    .map(|n| short_fn_name(&n))
                    .unwrap_or_else(|| "?".to_string());
                let arg_str = args
                    .iter()
                    .map(|op| self.render_operand(op))
                    .collect::<Vec<_>>()
                    .join(", ");
                // Add debug source info if available (shows where fn was referenced from)
                let debug_suffix = match func {
                    Operand::Constant(ConstOperand { const_, .. }) => {
                        self.fn_source_suffix(const_.ty())
                    }
                    _ => String::new(),
                };
                format!(
                    "{} = {}({}){}",
                    destination.label(),
                    fn_name,
                    arg_str,
                    debug_suffix
                )
            }
            Assert { cond, expected, .. } => {
                format!("Assert {} == {}", self.render_operand(cond), expected)
            }
            InlineAsm { .. } => "InlineAsm".to_string(),
        };
        format!("{}{}", base, span_suffix)
    }
}
