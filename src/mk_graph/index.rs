//! Index structures for resolving MIR references during graph generation.

use std::collections::{HashMap, HashSet};

extern crate stable_mir;
use stable_mir::mir::alloc::GlobalAlloc;
use stable_mir::mir::{
    Body, BorrowKind, Place, Rvalue, Statement, StatementKind, Terminator, TerminatorKind,
};
use stable_mir::ty::{IndexedVal, Ty};
use stable_mir::CrateDef;

use crate::printer::{AllocInfo, SpanInfo, TypeMetadata};

use super::util::bytes_to_u64_le;
use super::{MAX_NUMERIC_BYTES, MAX_STRING_PREVIEW_LEN};

// =============================================================================
// Alloc Index
// =============================================================================

/// Index for looking up allocation information by AllocId
#[derive(Default)]
pub struct AllocIndex {
    pub by_id: HashMap<u64, AllocEntry>,
}

/// Processed allocation entry with human-readable description
pub struct AllocEntry {
    pub alloc_id: u64,
    pub ty: Ty,
    pub kind: AllocKind,
    pub description: String,
    /// IDs of allocs referenced via provenance (for nested traversal)
    pub referenced_allocs: Vec<u64>,
}

/// Simplified allocation kind for display
pub enum AllocKind {
    Memory { bytes_len: usize, is_str: bool },
    Static { name: String },
    VTable { ty_desc: String },
    Function { name: String },
}

/// Format an allocation ID as a label
pub fn alloc_label(id: u64) -> String {
    format!("alloc{}", id)
}

impl AllocIndex {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_alloc_infos(allocs: &[AllocInfo], type_index: &TypeIndex) -> Self {
        let mut index = Self::new();
        for info in allocs {
            let entry = AllocEntry::from_alloc_info(info, type_index);
            index.by_id.insert(entry.alloc_id, entry);
        }
        index
    }

    pub fn get(&self, id: u64) -> Option<&AllocEntry> {
        self.by_id.get(&id)
    }

    pub fn iter(&self) -> impl Iterator<Item = &AllocEntry> {
        self.by_id.values()
    }

    /// Describe an alloc by its ID for use in labels
    pub fn describe(&self, id: u64) -> String {
        match self.get(id) {
            Some(entry) => entry.short_description(),
            None => alloc_label(id),
        }
    }

    /// Describe an alloc with its nested references (depth-limited to avoid explosion)
    pub fn describe_with_refs(&self, id: u64, max_depth: usize) -> String {
        self.describe_recursive(id, max_depth, &mut HashSet::new())
    }

    fn describe_recursive(&self, id: u64, depth: usize, visited: &mut HashSet<u64>) -> String {
        if depth == 0 || visited.contains(&id) {
            return self.describe(id);
        }
        visited.insert(id);

        match self.get(id) {
            Some(entry) => {
                if entry.referenced_allocs.is_empty() {
                    entry.short_description()
                } else {
                    let refs: Vec<String> = entry
                        .referenced_allocs
                        .iter()
                        .map(|&ref_id| self.describe_recursive(ref_id, depth - 1, visited))
                        .collect();
                    format!("{} -> [{}]", entry.short_description(), refs.join(", "))
                }
            }
            None => alloc_label(id),
        }
    }
}

impl AllocEntry {
    pub fn from_alloc_info(info: &AllocInfo, type_index: &TypeIndex) -> Self {
        let alloc_id = info.alloc_id().to_index() as u64;
        let ty = info.ty();
        let ty_name = type_index.get_name(ty);

        let (kind, description, referenced_allocs) = match info.global_alloc() {
            GlobalAlloc::Memory(alloc) => {
                let bytes = &alloc.bytes;
                let is_str = ty_name.contains("str");

                // Extract referenced alloc IDs from provenance
                let refs: Vec<u64> = alloc
                    .provenance
                    .ptrs
                    .iter()
                    .map(|(_offset, prov)| prov.0.to_index() as u64)
                    .collect();

                // Convert Option<u8> bytes to actual bytes for display
                let concrete_bytes: Vec<u8> = bytes.iter().filter_map(|&b| b).collect();

                let desc = if is_str && concrete_bytes.iter().all(|b| b.is_ascii()) {
                    let s: String = concrete_bytes
                        .iter()
                        .take(MAX_STRING_PREVIEW_LEN)
                        .map(|&b| b as char)
                        .collect::<String>()
                        .escape_default()
                        .to_string();
                    if concrete_bytes.len() > MAX_STRING_PREVIEW_LEN {
                        format!("\"{}...\" ({} bytes)", s, concrete_bytes.len())
                    } else {
                        format!("\"{}\"", s)
                    }
                } else if concrete_bytes.len() <= MAX_NUMERIC_BYTES && !concrete_bytes.is_empty() {
                    format!("{} = {}", ty_name, bytes_to_u64_le(&concrete_bytes))
                } else {
                    format!("{} ({} bytes)", ty_name, bytes.len())
                };

                (
                    AllocKind::Memory {
                        bytes_len: bytes.len(),
                        is_str,
                    },
                    desc,
                    refs,
                )
            }
            GlobalAlloc::Static(def) => {
                let name = def.name();
                (
                    AllocKind::Static { name: name.clone() },
                    format!("static {}", name),
                    vec![],
                )
            }
            GlobalAlloc::VTable(vty, trait_ref) => {
                let desc = if let Some(tr) = trait_ref {
                    // Binder<ExistentialTraitRef>.value.def_id identifies the trait
                    let trait_name = tr.value.def_id.name();
                    format!("{} as {}", vty, trait_name)
                } else {
                    format!("{}", vty)
                };
                (
                    AllocKind::VTable {
                        ty_desc: desc.clone(),
                    },
                    format!("vtable<{}>", desc),
                    vec![],
                )
            }
            GlobalAlloc::Function(instance) => {
                let name = instance.name();
                (
                    AllocKind::Function { name: name.clone() },
                    format!("fn {}", name),
                    vec![],
                )
            }
        };

        Self {
            alloc_id,
            ty,
            kind,
            description,
            referenced_allocs,
        }
    }

    pub fn short_description(&self) -> String {
        format!("{}: {}", alloc_label(self.alloc_id), self.description)
    }
}

// =============================================================================
// Type Index
// =============================================================================

/// Index for looking up type information
#[derive(Default)]
pub struct TypeIndex {
    by_id: HashMap<u64, String>,
}

impl TypeIndex {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_types(types: &[(Ty, TypeMetadata)]) -> Self {
        let mut index = Self::new();
        for (ty, metadata) in types {
            let name = Self::type_name_from_metadata(metadata, *ty);
            index.by_id.insert(ty.to_index() as u64, name);
        }
        index
    }

    fn type_name_from_metadata(metadata: &TypeMetadata, ty: Ty) -> String {
        match metadata {
            TypeMetadata::PrimitiveType(rigid) => format!("{:?}", rigid),
            TypeMetadata::EnumType { name, .. } => name.clone(),
            TypeMetadata::StructType { name, .. } => name.clone(),
            TypeMetadata::UnionType { name, .. } => name.clone(),
            TypeMetadata::ArrayType { .. } => format!("{}", ty),
            TypeMetadata::PtrType { .. } => format!("{}", ty),
            TypeMetadata::RefType { .. } => format!("{}", ty),
            TypeMetadata::TupleType { .. } => format!("{}", ty),
            TypeMetadata::FunType(name) => name.clone(),
            TypeMetadata::VoidType => "()".to_string(),
        }
    }

    pub fn get_name(&self, ty: Ty) -> String {
        self.by_id
            .get(&(ty.to_index() as u64))
            .cloned()
            .unwrap_or_else(|| format!("{}", ty))
    }
}

// =============================================================================
// Span Index
// =============================================================================

/// Index for looking up span/source location information.
/// Wraps a HashMap for convenient lookup by span ID.
#[derive(Default)]
pub struct SpanIndex {
    by_id: HashMap<usize, SpanInfo>,
}

impl SpanIndex {
    pub fn new() -> Self {
        Self::default()
    }

    /// Build index from spans collected by SmirJson
    pub fn from_spans(spans: &[(usize, SpanInfo)]) -> Self {
        let by_id = spans
            .iter()
            .map(|(id, info)| (*id, info.clone()))
            .collect();
        Self { by_id }
    }

    pub fn get(&self, span_id: usize) -> Option<&SpanInfo> {
        self.by_id.get(&span_id)
    }
}

// =============================================================================
// Function Key
// =============================================================================

/// Key for looking up function names, using full LinkMapKey info.
/// This avoids collisions when multiple generic instantiations share the same Ty.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct FunctionKey {
    pub ty: Ty,
    pub instance_desc: Option<String>,
}

// =============================================================================
// Borrow Index
// =============================================================================

/// A borrow operation detected in MIR
#[derive(Clone, Debug)]
pub struct BorrowInfo {
    /// Index of this borrow (for cross-referencing)
    pub index: usize,
    /// The local receiving the reference (_2 in `_2 = &_1`)
    pub borrower_local: usize,
    /// The local being borrowed (_1 in `_2 = &_1`)
    pub borrowed_local: usize,
    /// Kind of borrow
    pub kind: BorrowKindInfo,
    /// Location where borrow is created
    pub start_location: LocationKey,
    /// Span ID for source mapping
    pub span_id: usize,
}

/// Simplified borrow kind for downstream consumers
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BorrowKindInfo {
    /// &T
    Shared,
    /// &mut T
    Mutable,
    /// Used in match guards
    Shallow,
}

/// A CFG location (block + statement index)
#[derive(Clone, Debug, Hash, Eq, PartialEq)]
pub struct LocationKey {
    pub block: usize,
    pub statement: usize,
}

/// Index for tracking borrows and their lifetimes
#[derive(Default)]
pub struct BorrowIndex {
    /// All borrows in the function
    pub borrows: Vec<BorrowInfo>,
    /// For each CFG location, which borrow indices are active
    pub active_at_location: HashMap<LocationKey, Vec<usize>>,
    /// For each source line, which borrow indices are active
    pub active_at_line: HashMap<usize, Vec<usize>>,
}

impl BorrowIndex {
    pub fn new() -> Self {
        Self::default()
    }

    /// Build borrow index from a MIR body
    ///
    /// Scans the body for Rvalue::Ref operations and tracks:
    /// 1. Where each borrow is created
    /// 2. A conservative estimate of where each borrow is active
    ///    (from creation until the borrower local goes dead or is reassigned)
    pub fn from_body(body: &Body, span_index: &SpanIndex) -> Self {
        let mut index = Self::new();
        let mut borrow_count = 0;

        // Pass 1: Find all borrows
        for (block_idx, block) in body.blocks.iter().enumerate() {
            for (stmt_idx, stmt) in block.statements.iter().enumerate() {
                if let StatementKind::Assign(place, rvalue) = &stmt.kind {
                    if let Some(borrow_info) = extract_borrow(
                        place,
                        rvalue,
                        block_idx,
                        stmt_idx,
                        stmt.span.to_index(),
                        borrow_count,
                    ) {
                        index.borrows.push(borrow_info);
                        borrow_count += 1;
                    }
                }
            }
        }

        // Pass 2: Compute active ranges (conservative: start to StorageDead/reassign)
        index.compute_active_ranges(body);

        // Pass 3: Map to source lines
        index.map_to_source_lines(span_index);

        index
    }

    /// Get borrows active at a specific CFG location
    pub fn active_at(&self, block: usize, statement: usize) -> &[usize] {
        let key = LocationKey { block, statement };
        self.active_at_location
            .get(&key)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Get borrows active at a source line
    pub fn active_at_source_line(&self, line: usize) -> &[usize] {
        self.active_at_line
            .get(&line)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Get a borrow by index
    pub fn get(&self, index: usize) -> Option<&BorrowInfo> {
        self.borrows.get(index)
    }

    /// Compute where each borrow is active (conservative analysis)
    ///
    /// A borrow is active from its creation until:
    /// - The borrower local has StorageDead
    /// - The borrower local is reassigned
    /// - The function returns
    fn compute_active_ranges(&mut self, body: &Body) {
        for borrow in &self.borrows {
            let borrower = borrow.borrower_local;
            let start = &borrow.start_location;

            // Track this borrow as active from start through the CFG
            // until we hit a kill point
            let mut visited = HashSet::new();
            let mut worklist = vec![(start.block, start.statement)];

            while let Some((block_idx, mut stmt_idx)) = worklist.pop() {
                if !visited.insert((block_idx, stmt_idx)) {
                    continue;
                }

                let block = &body.blocks[block_idx];

                // Process statements from stmt_idx onwards
                while stmt_idx <= block.statements.len() {
                    let loc = LocationKey {
                        block: block_idx,
                        statement: stmt_idx,
                    };

                    // Mark as active
                    self.active_at_location
                        .entry(loc)
                        .or_default()
                        .push(borrow.index);

                    // Check for kill conditions
                    if stmt_idx < block.statements.len() {
                        let stmt = &block.statements[stmt_idx];
                        if kills_borrow(stmt, borrower) {
                            break; // Borrow ends here
                        }
                    }

                    stmt_idx += 1;
                }

                // If we reached the terminator without killing, propagate to successors
                if stmt_idx > block.statements.len() {
                    // Check if terminator kills the borrow
                    if !terminator_kills_borrow(&block.terminator, borrower) {
                        // Add successor blocks to worklist
                        for target in get_terminator_targets(&block.terminator) {
                            worklist.push((target, 0));
                        }
                    }
                }
            }
        }

        // Deduplicate active borrow lists
        for borrows in self.active_at_location.values_mut() {
            borrows.sort();
            borrows.dedup();
        }
    }

    /// Map active borrows to source lines
    fn map_to_source_lines(&mut self, span_index: &SpanIndex) {
        // For each borrow, find its source line range and mark as active
        for borrow in &self.borrows {
            if let Some(span_info) = span_index.get(borrow.span_id) {
                // Find all locations where this borrow is active
                for (_loc, borrow_indices) in &self.active_at_location {
                    if borrow_indices.contains(&borrow.index) {
                        // Add to lines covered by the borrow's span
                        for line in span_info.line_start..=span_info.line_end {
                            self.active_at_line.entry(line).or_default().push(borrow.index);
                        }
                    }
                }
            }
        }

        // Deduplicate
        for borrows in self.active_at_line.values_mut() {
            borrows.sort();
            borrows.dedup();
        }
    }
}

/// Extract borrow info from an assignment if it's a borrow
fn extract_borrow(
    place: &Place,
    rvalue: &Rvalue,
    block_idx: usize,
    stmt_idx: usize,
    span_id: usize,
    index: usize,
) -> Option<BorrowInfo> {
    // Only direct local assignments (no projections on LHS)
    if !place.projection.is_empty() {
        return None;
    }

    let borrower_local = place.local;

    match rvalue {
        Rvalue::Ref(_region, borrow_kind, borrowed_place) => {
            // Only track borrows of direct locals for now
            if !borrowed_place.projection.is_empty() {
                return None;
            }

            let kind = match borrow_kind {
                BorrowKind::Shared => BorrowKindInfo::Shared,
                BorrowKind::Mut { .. } => BorrowKindInfo::Mutable,
                BorrowKind::Fake(_) => BorrowKindInfo::Shallow,
            };

            Some(BorrowInfo {
                index,
                borrower_local,
                borrowed_local: borrowed_place.local,
                kind,
                start_location: LocationKey {
                    block: block_idx,
                    statement: stmt_idx,
                },
                span_id,
            })
        }
        _ => None,
    }
}

/// Check if a statement kills a borrow (ends its lifetime)
fn kills_borrow(stmt: &Statement, borrower: usize) -> bool {
    match &stmt.kind {
        // StorageDead kills the borrow
        StatementKind::StorageDead(local) if *local == borrower => true,

        // Reassignment kills the borrow
        StatementKind::Assign(place, _)
            if place.projection.is_empty() && place.local == borrower =>
        {
            true
        }

        _ => false,
    }
}

/// Check if a terminator kills a borrow
fn terminator_kills_borrow(term: &Terminator, borrower: usize) -> bool {
    match &term.kind {
        // Drop of the borrower kills the borrow
        TerminatorKind::Drop { place, .. }
            if place.projection.is_empty() && place.local == borrower =>
        {
            true
        }

        // Return ends all borrows
        TerminatorKind::Return => true,

        _ => false,
    }
}

/// Get target block indices from a terminator
fn get_terminator_targets(term: &Terminator) -> Vec<usize> {
    use stable_mir::mir::UnwindAction;

    match &term.kind {
        TerminatorKind::Goto { target } => vec![*target],
        TerminatorKind::SwitchInt { targets, .. } => {
            let mut result: Vec<usize> = targets.branches().map(|(_, t)| t).collect();
            result.push(targets.otherwise());
            result
        }
        TerminatorKind::Return
        | TerminatorKind::Resume
        | TerminatorKind::Abort
        | TerminatorKind::Unreachable => vec![],
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

// =============================================================================
// Lifetime Index - Variable Lexical Lifetimes
// =============================================================================

/// Source range with line and optional column precision
#[derive(Clone, Debug, Default)]
pub struct SourceRange {
    pub start_line: usize,
    pub start_col: usize,
    pub end_line: usize,
    pub end_col: usize,
}

impl SourceRange {
    /// Format the range - show columns only for single-line ranges
    pub fn format(&self) -> String {
        if self.start_line == self.end_line {
            // Single line: show column range
            format!("{}:{}-{}", self.start_line, self.start_col, self.end_col)
        } else {
            // Multi-line: just show line range
            format!("{}-{}", self.start_line, self.end_line)
        }
    }

    /// Format with "line(s)" prefix
    pub fn format_verbose(&self) -> String {
        if self.start_line == self.end_line {
            format!("line {}:{}-{}", self.start_line, self.start_col, self.end_col)
        } else {
            format!("lines {}-{}", self.start_line, self.end_line)
        }
    }
}

/// Lifetime information for a local variable
#[derive(Clone, Debug)]
pub struct LocalLifetime {
    /// Local index
    pub local: usize,
    /// Where StorageLive is called (if any)
    pub storage_live: Option<LocationKey>,
    /// Where StorageDead is called (if any)
    pub storage_dead: Option<LocationKey>,
    /// Source range if mappable
    pub source_range: Option<SourceRange>,
}

impl LocalLifetime {
    /// Check if source range info is available
    pub fn has_source_info(&self) -> bool {
        self.source_range.is_some()
    }

    /// Format as a lifetime annotation (e.g., "'_1: lines 5-12" or "'_1: 5:3-15")
    pub fn format_range(&self) -> String {
        if let Some(range) = &self.source_range {
            format!("'_{}: {}", self.local, range.format_verbose())
        } else if let (Some(live), Some(dead)) = (&self.storage_live, &self.storage_dead) {
            format!(
                "'_{}: bb{}[{}] → bb{}[{}]",
                self.local, live.block, live.statement, dead.block, dead.statement
            )
        } else {
            format!("'_{}: <unknown>", self.local)
        }
    }

    /// Get just the range portion without the lifetime name
    pub fn range_str(&self) -> String {
        if let Some(range) = &self.source_range {
            range.format()
        } else {
            "<unknown>".to_string()
        }
    }
}

/// Index for tracking variable lexical lifetimes
#[derive(Default)]
pub struct LifetimeIndex {
    /// Lifetime info for each local, indexed by local number
    pub locals: HashMap<usize, LocalLifetime>,
}

impl LifetimeIndex {
    pub fn new() -> Self {
        Self::default()
    }

    /// Build lifetime index from a MIR body
    ///
    /// Scans for StorageLive/StorageDead pairs to determine lexical scope
    pub fn from_body(body: &Body, span_index: &SpanIndex) -> Self {
        let mut index = Self::new();

        // Initialize entries for all locals
        for (i, _decl) in body.local_decls().enumerate() {
            index.locals.insert(
                i,
                LocalLifetime {
                    local: i,
                    storage_live: None,
                    storage_dead: None,
                    source_range: None,
                },
            );
        }

        // Scan for StorageLive/StorageDead
        for (block_idx, block) in body.blocks.iter().enumerate() {
            for (stmt_idx, stmt) in block.statements.iter().enumerate() {
                match &stmt.kind {
                    StatementKind::StorageLive(local) => {
                        if let Some(lifetime) = index.locals.get_mut(local) {
                            // Take first StorageLive (there may be multiple in loops)
                            if lifetime.storage_live.is_none() {
                                lifetime.storage_live = Some(LocationKey {
                                    block: block_idx,
                                    statement: stmt_idx,
                                });
                            }
                        }
                    }
                    StatementKind::StorageDead(local) => {
                        if let Some(lifetime) = index.locals.get_mut(local) {
                            // Take last StorageDead
                            lifetime.storage_dead = Some(LocationKey {
                                block: block_idx,
                                statement: stmt_idx,
                            });
                        }
                    }
                    _ => {}
                }
            }
        }

        // Map to source lines
        index.map_to_source_lines(body, span_index);

        index
    }

    /// Map lifetimes to source ranges with line and column info
    fn map_to_source_lines(&mut self, body: &Body, span_index: &SpanIndex) {
        for lifetime in self.locals.values_mut() {
            let start_info = lifetime.storage_live.as_ref().and_then(|loc| {
                body.blocks
                    .get(loc.block)
                    .and_then(|b| b.statements.get(loc.statement))
                    .and_then(|s| span_index.get(s.span.to_index()))
            });

            let end_info = lifetime.storage_dead.as_ref().and_then(|loc| {
                body.blocks
                    .get(loc.block)
                    .and_then(|b| b.statements.get(loc.statement))
                    .and_then(|s| span_index.get(s.span.to_index()))
            });

            if let (Some(start), Some(end)) = (start_info, end_info) {
                lifetime.source_range = Some(SourceRange {
                    start_line: start.line_start,
                    start_col: start.col_start,
                    end_line: end.line_end,
                    end_col: end.col_end,
                });
            }
        }
    }

    /// Get lifetime info for a local
    pub fn get(&self, local: usize) -> Option<&LocalLifetime> {
        self.locals.get(&local)
    }

    /// Iterate over all lifetimes
    pub fn iter(&self) -> impl Iterator<Item = &LocalLifetime> {
        self.locals.values()
    }

    /// Get lifetimes with known source ranges, sorted by local index
    pub fn with_source_ranges(&self) -> Vec<&LocalLifetime> {
        let mut result: Vec<_> = self
            .locals
            .values()
            .filter(|l| l.source_range.is_some())
            .collect();
        result.sort_by_key(|l| l.local);
        result
    }
}

// =============================================================================
// Extended Borrow Info with End Location
// =============================================================================

impl BorrowInfo {
    /// Format as a lifetime range (e.g., "'b0: lines 5-12")
    pub fn format_lifetime_range(&self, span_index: &SpanIndex, end_line: Option<usize>) -> String {
        let start_line = span_index
            .get(self.span_id)
            .map(|info| info.line_start)
            .unwrap_or(0);

        if let Some(end) = end_line {
            if start_line == end {
                format!("'b{}: line {}", self.index, start_line)
            } else {
                format!("'b{}: lines {}-{}", self.index, start_line, end)
            }
        } else {
            format!("'b{}: from line {}", self.index, start_line)
        }
    }
}

impl BorrowIndex {
    /// Find the end location for a borrow (first kill point encountered)
    pub fn find_borrow_end(&self, borrow_idx: usize, body: &Body) -> Option<LocationKey> {
        let borrow = self.borrows.get(borrow_idx)?;
        let borrower = borrow.borrower_local;

        // Traverse from start until we find the kill point
        let mut visited = std::collections::HashSet::new();
        let mut worklist = vec![(borrow.start_location.block, borrow.start_location.statement)];

        while let Some((block_idx, mut stmt_idx)) = worklist.pop() {
            if !visited.insert((block_idx, stmt_idx)) {
                continue;
            }

            let block = &body.blocks[block_idx];

            while stmt_idx < block.statements.len() {
                let stmt = &block.statements[stmt_idx];
                if kills_borrow(stmt, borrower) {
                    return Some(LocationKey {
                        block: block_idx,
                        statement: stmt_idx,
                    });
                }
                stmt_idx += 1;
            }

            // Check terminator
            if terminator_kills_borrow(&block.terminator, borrower) {
                return Some(LocationKey {
                    block: block_idx,
                    statement: block.statements.len(),
                });
            }

            // Propagate to successors
            for target in get_terminator_targets(&block.terminator) {
                worklist.push((target, 0));
            }
        }

        None
    }

    /// Get the source line range for a borrow
    pub fn borrow_source_range(
        &self,
        borrow_idx: usize,
        body: &Body,
        span_index: &SpanIndex,
    ) -> Option<(usize, usize)> {
        let borrow = self.borrows.get(borrow_idx)?;

        let start_line = span_index.get(borrow.span_id).map(|info| info.line_start)?;

        let end_line = self.find_borrow_end(borrow_idx, body).and_then(|loc| {
            body.blocks.get(loc.block).and_then(|block| {
                if loc.statement < block.statements.len() {
                    span_index
                        .get(block.statements[loc.statement].span.to_index())
                        .map(|info| info.line_end)
                } else {
                    // Terminator
                    span_index
                        .get(block.terminator.span.to_index())
                        .map(|info| info.line_end)
                }
            })
        });

        Some((start_line, end_line.unwrap_or(start_line)))
    }
}
