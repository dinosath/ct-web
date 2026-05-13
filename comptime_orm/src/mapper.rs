// comptime_orm/src/mapper.rs – ParamValue for use in Query<E> dynamic filters.

/// A single bound value used in a dynamic `Query<E>` WHERE clause.
#[derive(Debug, Clone)]
pub enum ParamValue {
    I64(i64),
    F64(f64),
    Bool(bool),
    Text(String),
    Null,
}
