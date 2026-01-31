//! # vf-perf
//!
//! Performance analysis for verified lock-free structures.
//!
//! ## Progress Guarantees
//!
//! Lock-free algorithms have different progress guarantees:
//!
//! | Guarantee | Description |
//! |-----------|-------------|
//! | WaitFree | Every thread completes in bounded steps |
//! | LockFree | At least one thread makes progress |
//! | ObstructionFree | Progress if run in isolation |
//! | Blocking | May block indefinitely |

/// Progress guarantee levels for concurrent algorithms.
///
/// Ordered from strongest (WaitFree) to weakest (Blocking).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ProgressGuarantee {
    /// May block indefinitely waiting for another thread
    Blocking = 0,
    /// Makes progress if run in isolation (no interference)
    ObstructionFree = 1,
    /// At least one thread makes progress in any execution
    LockFree = 2,
    /// Every thread completes in bounded steps
    WaitFree = 3,
}

impl ProgressGuarantee {
    /// Get a description of this guarantee level.
    pub fn description(&self) -> &'static str {
        match self {
            ProgressGuarantee::Blocking => "May block indefinitely",
            ProgressGuarantee::ObstructionFree => "Progress if run in isolation",
            ProgressGuarantee::LockFree => "At least one thread makes progress",
            ProgressGuarantee::WaitFree => "Every thread completes in bounded steps",
        }
    }

    /// Check if this guarantee is at least as strong as another.
    pub fn at_least(&self, other: ProgressGuarantee) -> bool {
        *self >= other
    }
}

/// Performance profile for an implementation.
#[derive(Debug, Clone)]
pub struct PerfProfile {
    /// Progress guarantee level
    pub progress: ProgressGuarantee,
    /// Memory overhead per element in bytes
    pub memory_overhead_bytes: u64,
    /// Whether the implementation uses helping (for wait-free)
    pub uses_helping: bool,
    /// Maximum retry count (for lock-free with bounded retries)
    pub retry_count_max: Option<u64>,
    /// Notes about performance characteristics
    pub notes: Vec<String>,
}

impl PerfProfile {
    /// Create a new profile.
    pub fn new(progress: ProgressGuarantee) -> Self {
        Self {
            progress,
            memory_overhead_bytes: 0,
            uses_helping: false,
            retry_count_max: None,
            notes: Vec::new(),
        }
    }

    /// Set memory overhead.
    pub fn with_memory_overhead(mut self, bytes: u64) -> Self {
        self.memory_overhead_bytes = bytes;
        self
    }

    /// Mark as using helping mechanism.
    pub fn with_helping(mut self) -> Self {
        self.uses_helping = true;
        self
    }

    /// Set maximum retry count.
    pub fn with_retry_count_max(mut self, count: u64) -> Self {
        self.retry_count_max = Some(count);
        self
    }

    /// Add a note.
    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.notes.push(note.into());
        self
    }
}

/// Analyze progress guarantee from code structure.
///
/// This is a heuristic analysis - not a proof.
pub fn analyze_progress_guarantee(code: &str) -> ProgressGuarantee {
    // Check for wait-free indicators
    if code.contains("helping") || code.contains("announce") {
        return ProgressGuarantee::WaitFree;
    }

    // Check for lock-free indicators
    let has_cas_loop =
        code.contains("compare_exchange") || code.contains("compare_and_swap");
    let has_bounded_retry = code.contains("retry_count") || code.contains("MAX_RETRIES");

    if has_cas_loop {
        if has_bounded_retry {
            // Bounded retries could be obstruction-free or lock-free
            // depending on implementation
            return ProgressGuarantee::LockFree;
        }
        return ProgressGuarantee::LockFree;
    }

    // Check for blocking indicators
    if code.contains("Mutex") || code.contains("RwLock") || code.contains(".lock()") {
        return ProgressGuarantee::Blocking;
    }

    // Default to obstruction-free (weakest non-blocking)
    ProgressGuarantee::ObstructionFree
}

/// Memory overhead analysis result.
#[derive(Debug, Clone)]
pub struct MemoryOverhead {
    /// Bytes per node/element
    pub per_element_bytes: u64,
    /// Fixed overhead bytes
    pub fixed_bytes: u64,
    /// Description of what contributes to overhead
    pub breakdown: Vec<(String, u64)>,
}

impl MemoryOverhead {
    /// Calculate total memory for N elements.
    pub fn total_bytes(&self, elements_count: u64) -> u64 {
        self.fixed_bytes + (self.per_element_bytes * elements_count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_progress_guarantee_ordering() {
        assert!(ProgressGuarantee::WaitFree > ProgressGuarantee::LockFree);
        assert!(ProgressGuarantee::LockFree > ProgressGuarantee::ObstructionFree);
        assert!(ProgressGuarantee::ObstructionFree > ProgressGuarantee::Blocking);
    }

    #[test]
    fn test_analyze_blocking() {
        let code = r#"
fn push(&self, val: T) {
    let guard = self.mutex.lock().unwrap();
    // ...
}
"#;
        assert_eq!(analyze_progress_guarantee(code), ProgressGuarantee::Blocking);
    }

    #[test]
    fn test_analyze_lock_free() {
        let code = r#"
fn push(&self, val: T) {
    loop {
        let head = self.head.load(Ordering::Acquire);
        if self.head.compare_exchange(head, new, Ordering::Release, Ordering::Relaxed).is_ok() {
            break;
        }
    }
}
"#;
        assert_eq!(analyze_progress_guarantee(code), ProgressGuarantee::LockFree);
    }

    #[test]
    fn test_perf_profile() {
        let profile = PerfProfile::new(ProgressGuarantee::LockFree)
            .with_memory_overhead(24)
            .with_note("8 bytes for pointer, 16 bytes for atomic tag");

        assert_eq!(profile.progress, ProgressGuarantee::LockFree);
        assert_eq!(profile.memory_overhead_bytes, 24);
    }

    #[test]
    fn test_memory_overhead() {
        let overhead = MemoryOverhead {
            per_element_bytes: 24,
            fixed_bytes: 8,
            breakdown: vec![
                ("pointer".to_string(), 8),
                ("value".to_string(), 8),
                ("tag".to_string(), 8),
            ],
        };

        assert_eq!(overhead.total_bytes(100), 8 + 24 * 100);
    }
}
