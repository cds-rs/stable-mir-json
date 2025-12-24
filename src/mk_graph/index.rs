//! Index structures for resolving MIR references during graph generation.

use std::collections::{HashMap, HashSet};

extern crate stable_mir;
use stable_mir::mir::alloc::GlobalAlloc;
use stable_mir::ty::{IndexedVal, Ty};
use stable_mir::CrateDef;

use crate::printer::{AllocInfo, TypeMetadata};

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

/// Raw span data tuple: (file, line_start, col_start, line_end, col_end)
pub type SpanData = (String, usize, usize, usize, usize);

/// Index for looking up span/source location information
#[derive(Default)]
pub struct SpanIndex {
    by_id: HashMap<usize, SpanInfo>,
}

/// Source location information for a span
#[derive(Clone)]
pub struct SpanInfo {
    pub file: String,
    pub line_start: usize,
    pub col_start: usize,
    pub line_end: usize,
    pub col_end: usize,
}

impl SpanIndex {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_spans(spans: &[(usize, SpanData)]) -> Self {
        let by_id = spans
            .iter()
            .map(|(id, (file, lo_line, lo_col, hi_line, hi_col))| {
                (
                    *id,
                    SpanInfo {
                        file: file.clone(),
                        line_start: *lo_line,
                        col_start: *lo_col,
                        line_end: *hi_line,
                        col_end: *hi_col,
                    },
                )
            })
            .collect();
        Self { by_id }
    }

    pub fn get(&self, span_id: usize) -> Option<&SpanInfo> {
        self.by_id.get(&span_id)
    }
}

impl SpanInfo {
    /// Format as "file:line" for compact display
    pub fn short(&self) -> String {
        // Extract just the filename from the path
        let file = self.file.rsplit('/').next().unwrap_or(&self.file);
        format!("{}:{}", file, self.line_start)
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
