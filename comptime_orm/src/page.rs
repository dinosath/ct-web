// comptime_orm/src/page.rs – Jakarta Data-style pagination and sorting.
//
// Mirrors jakarta.data.page.Page / Pageable / Sort.
// Migration note (Agents.md §16): when #[comptime] lands, sort field
// names will be validated against entity columns at compile time.

// ──────────────────────────────────────────────────────────────────────
// Sort
// ──────────────────────────────────────────────────────────────────────

/// Sort direction for a single column.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Direction {
    Asc,
    Desc,
}

impl Direction {
    pub fn as_sql(&self) -> &'static str {
        match self {
            Direction::Asc  => "ASC",
            Direction::Desc => "DESC",
        }
    }
}

/// A single sort instruction: (column_name, direction).
#[derive(Debug, Clone)]
pub struct Sort {
    pub column:    String,
    pub direction: Direction,
}

impl Sort {
    pub fn asc(column: impl Into<String>) -> Self {
        Sort { column: column.into(), direction: Direction::Asc }
    }

    pub fn desc(column: impl Into<String>) -> Self {
        Sort { column: column.into(), direction: Direction::Desc }
    }

    /// Emit ORDER BY fragment, e.g. `"created_at DESC"`.
    pub fn as_sql(&self) -> String {
        format!("{} {}", self.column, self.direction.as_sql())
    }
}

// ──────────────────────────────────────────────────────────────────────
// Pageable
// ──────────────────────────────────────────────────────────────────────

/// Page request: zero-based page number, page size, optional sorts.
#[derive(Debug, Clone)]
pub struct Pageable {
    /// Zero-based page index.
    pub page:  usize,
    /// Number of rows per page (must be > 0).
    pub size:  usize,
    /// Ordered list of sort instructions.
    pub sorts: Vec<Sort>,
}

impl Pageable {
    /// Create a simple pageable with no sorts.
    pub fn of(page: usize, size: usize) -> Self {
        Pageable { page, size, sorts: vec![] }
    }

    /// Add a sort instruction.
    pub fn sort(mut self, s: Sort) -> Self {
        self.sorts.push(s);
        self
    }

    /// SQL OFFSET value.
    pub fn offset(&self) -> usize {
        self.page * self.size
    }

    /// Build an `ORDER BY … LIMIT … OFFSET …` suffix.
    ///
    /// `base_param_idx` is the index of the next `$N` placeholder so
    /// LIMIT and OFFSET use contiguous parameter slots.
    pub fn sql_suffix(&self, base_param_idx: usize) -> (String, usize) {
        let mut sql = String::new();

        if !self.sorts.is_empty() {
            sql.push_str(" ORDER BY ");
            let parts: Vec<String> = self.sorts.iter().map(Sort::as_sql).collect();
            sql.push_str(&parts.join(", "));
        }

        sql.push_str(&format!(
            " LIMIT ${} OFFSET ${}",
            base_param_idx,
            base_param_idx + 1
        ));

        (sql, base_param_idx + 2)
    }
}

// ──────────────────────────────────────────────────────────────────────
// Page
// ──────────────────────────────────────────────────────────────────────

/// A single page of results, mirroring `jakarta.data.page.Page<T>`.
#[derive(Debug, Clone)]
pub struct Page<T> {
    /// The records for this page.
    pub content:        Vec<T>,
    /// The request that produced this page.
    pub pageable:       Pageable,
    /// Total number of matching records across all pages.
    pub total_elements: u64,
}

impl<T> Page<T> {
    pub fn new(content: Vec<T>, pageable: Pageable, total_elements: u64) -> Self {
        Page { content, pageable, total_elements }
    }

    /// Total number of pages.
    pub fn total_pages(&self) -> usize {
        if self.pageable.size == 0 {
            return 0;
        }
        ((self.total_elements as usize) + self.pageable.size - 1) / self.pageable.size
    }

    /// Whether there is a subsequent page.
    pub fn has_next(&self) -> bool {
        self.pageable.page + 1 < self.total_pages()
    }

    /// Whether this is the first page (page == 0).
    pub fn is_first(&self) -> bool {
        self.pageable.page == 0
    }
}
