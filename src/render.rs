//! Shared rendering utilities for MIR visualization
//!
//! This module provides common functions for rendering MIR constructs
//! to string representations, used by both the HTML and explorer outputs.

extern crate stable_mir;
use stable_mir::mir::{BinOp, CastKind, NullOp, Operand, Place, Rvalue, UnOp};
use stable_mir::CrateDef;

/// Render a place (lvalue) to string
pub fn render_place(place: &Place) -> String {
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

/// Render an operand to string
pub fn render_operand(op: &Operand) -> String {
    match op {
        Operand::Copy(place) => render_place(place),
        Operand::Move(place) => format!("move {}", render_place(place)),
        Operand::Constant(c) => render_mir_const(&c.const_),
    }
}

/// Render a MIR constant to string
pub fn render_mir_const(c: &stable_mir::ty::MirConst) -> String {
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

/// Render a type constant to string
pub fn render_ty_const(c: &stable_mir::ty::TyConst) -> String {
    format!("{:?}", c)
}

/// Render an rvalue to string
pub fn render_rvalue(rv: &Rvalue) -> String {
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

/// Render binary operator symbol
pub fn render_binop(op: &BinOp) -> &'static str {
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

/// Render unary operator symbol
pub fn render_unop(op: &UnOp) -> &'static str {
    match op {
        UnOp::Not => "!",
        UnOp::Neg => "-",
        UnOp::PtrMetadata => "metadata",
    }
}

/// Generate human-readable annotation for an rvalue
pub fn annotate_rvalue(rv: &Rvalue) -> String {
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

/// Human-readable binary operator name
pub fn op_name(op: &BinOp) -> &'static str {
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
pub fn extract_call_name(func: &Operand) -> String {
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

/// Extract short function name from full path
pub fn short_fn_name(name: &str) -> String {
    let short = name.rsplit("::").next().unwrap_or(name);
    short
        .find("::h")
        .map(|i| &short[..i])
        .unwrap_or(short)
        .to_string()
}

/// Try to interpret bytes as an integer
pub fn bytes_to_int(bytes: &[Option<u8>]) -> Option<i128> {
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

/// Escape HTML special characters
pub fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
