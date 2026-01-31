//! Stateright model for Treiber stack.
//!
//! This model mirrors `specs/lockfree/treiber_stack.tla` and can be used
//! for exhaustive state space exploration.

use std::collections::{BTreeMap, BTreeSet};
use std::hash::Hash;

use stateright::Model;

/// Unique identifier for a node.
pub type NodeId = u64;

/// Unique identifier for a thread.
pub type ThreadId = u64;

/// A node in the stack.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Node {
    pub value: u64,
    pub next: Option<NodeId>,
}

/// Thread-local state for ongoing operations.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ThreadState {
    Idle,
    PushAllocated {
        node_id: NodeId,
        value: u64,
    },
    PushReadHead {
        node_id: NodeId,
        value: u64,
        observed_head: Option<NodeId>,
    },
    PopReadHead {
        observed_head: NodeId,
        value: u64,
        next: Option<NodeId>,
    },
}

/// State of the Treiber stack model.
///
/// Mirrors the TLA+ spec's state variables.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StackState {
    /// Pointer to top node (corresponds to TLA+ `head`)
    pub head: Option<NodeId>,
    /// All nodes in the system (corresponds to TLA+ `nodes`)
    pub nodes: BTreeMap<NodeId, Node>,
    /// Counter for allocating new node IDs
    pub node_id_next: NodeId,
    /// Set of pushed elements (corresponds to TLA+ `pushed`)
    pub pushed: BTreeSet<u64>,
    /// Set of popped elements (corresponds to TLA+ `popped`)
    pub popped: BTreeSet<u64>,
    /// Thread states
    pub threads: BTreeMap<ThreadId, ThreadState>,
}

impl StackState {
    /// Create initial state with given number of threads.
    pub fn new(threads_count: u64) -> Self {
        debug_assert!(threads_count > 0, "Must have at least one thread");
        debug_assert!(threads_count <= 8, "Model checking with many threads is slow");

        let mut threads = BTreeMap::new();
        for tid in 0..threads_count {
            threads.insert(tid, ThreadState::Idle);
        }

        Self {
            head: None,
            nodes: BTreeMap::new(),
            node_id_next: 0,
            pushed: BTreeSet::new(),
            popped: BTreeSet::new(),
            threads,
        }
    }

    /// Get current stack contents by traversing from head.
    pub fn contents(&self) -> Vec<u64> {
        let mut result = Vec::new();
        let mut current = self.head;

        while let Some(node_id) = current {
            if let Some(node) = self.nodes.get(&node_id) {
                result.push(node.value);
                current = node.next;
            } else {
                break;
            }
        }

        result
    }

    // ========== Invariants (from TLA+ spec) ==========

    /// Line 45: NoLostElements
    ///
    /// Every element that was pushed must either be in the stack or was popped.
    pub fn no_lost_elements(&self) -> bool {
        let contents: BTreeSet<u64> = self.contents().into_iter().collect();

        for element in &self.pushed {
            if !contents.contains(element) && !self.popped.contains(element) {
                return false;
            }
        }
        true
    }

    /// Line 58: NoDuplicates
    ///
    /// No element appears twice in the stack.
    pub fn no_duplicates(&self) -> bool {
        let contents = self.contents();
        let unique: BTreeSet<u64> = contents.iter().copied().collect();
        contents.len() == unique.len()
    }

    /// Combined invariant check.
    pub fn invariants_hold(&self) -> bool {
        self.no_lost_elements() && self.no_duplicates()
    }
}

/// Actions that threads can take.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum StackAction {
    /// Thread allocates a new node for push
    PushAlloc { thread: ThreadId, value: u64 },
    /// Thread reads current head
    PushReadHead { thread: ThreadId },
    /// Thread attempts CAS to complete push
    PushCas { thread: ThreadId },
    /// Thread reads head and value for pop
    PopReadHead { thread: ThreadId },
    /// Thread attempts CAS to complete pop
    PopCas { thread: ThreadId },
}

/// Model for bounded model checking.
pub struct StackModel {
    pub threads_count: u64,
    pub values: Vec<u64>,
    pub operations_per_thread_max: u64,
}

impl StackModel {
    /// Create a new model with given parameters.
    pub fn new(threads_count: u64, values: Vec<u64>) -> Self {
        debug_assert!(threads_count > 0);
        debug_assert!(!values.is_empty());

        Self {
            threads_count,
            values,
            operations_per_thread_max: 4,
        }
    }
}

impl Model for StackModel {
    type State = StackState;
    type Action = StackAction;

    fn init_states(&self) -> Vec<Self::State> {
        vec![StackState::new(self.threads_count)]
    }

    fn actions(&self, state: &Self::State, actions: &mut Vec<Self::Action>) {
        for (&tid, thread_state) in &state.threads {
            match thread_state {
                ThreadState::Idle => {
                    // Collect values that are already in-flight (being pushed by other threads)
                    let in_flight: BTreeSet<u64> = state.threads.values()
                        .filter_map(|ts| match ts {
                            ThreadState::PushAllocated { value, .. } => Some(*value),
                            ThreadState::PushReadHead { value, .. } => Some(*value),
                            _ => None,
                        })
                        .collect();

                    // Can start a push with any value not yet pushed or in-flight
                    for &value in &self.values {
                        if !state.pushed.contains(&value) && !in_flight.contains(&value) {
                            actions.push(StackAction::PushAlloc { thread: tid, value });
                        }
                    }
                    // Can start a pop if stack is not empty
                    if state.head.is_some() {
                        actions.push(StackAction::PopReadHead { thread: tid });
                    }
                }
                ThreadState::PushAllocated { .. } => {
                    actions.push(StackAction::PushReadHead { thread: tid });
                }
                ThreadState::PushReadHead { .. } => {
                    actions.push(StackAction::PushCas { thread: tid });
                }
                ThreadState::PopReadHead { .. } => {
                    actions.push(StackAction::PopCas { thread: tid });
                }
            }
        }
    }

    fn next_state(&self, state: &Self::State, action: Self::Action) -> Option<Self::State> {
        let mut next = state.clone();

        match action {
            StackAction::PushAlloc { thread, value } => {
                let node_id = next.node_id_next;
                next.node_id_next += 1;

                next.nodes.insert(
                    node_id,
                    Node {
                        value,
                        next: None,
                    },
                );

                next.threads.insert(
                    thread,
                    ThreadState::PushAllocated { node_id, value },
                );
            }

            StackAction::PushReadHead { thread } => {
                if let Some(ThreadState::PushAllocated { node_id, value }) =
                    next.threads.get(&thread).cloned()
                {
                    // Set node's next pointer to current head
                    if let Some(node) = next.nodes.get_mut(&node_id) {
                        node.next = next.head;
                    }

                    next.threads.insert(
                        thread,
                        ThreadState::PushReadHead {
                            node_id,
                            value,
                            observed_head: next.head,
                        },
                    );
                }
            }

            StackAction::PushCas { thread } => {
                if let Some(ThreadState::PushReadHead {
                    node_id,
                    value,
                    observed_head,
                }) = next.threads.get(&thread).cloned()
                {
                    if next.head == observed_head {
                        // CAS succeeds
                        next.head = Some(node_id);
                        next.pushed.insert(value);
                        next.threads.insert(thread, ThreadState::Idle);
                    } else {
                        // CAS fails - retry
                        next.threads.insert(
                            thread,
                            ThreadState::PushAllocated { node_id, value },
                        );
                    }
                }
            }

            StackAction::PopReadHead { thread } => {
                if let Some(head_id) = next.head {
                    if let Some(node) = next.nodes.get(&head_id) {
                        next.threads.insert(
                            thread,
                            ThreadState::PopReadHead {
                                observed_head: head_id,
                                value: node.value,
                                next: node.next,
                            },
                        );
                    }
                }
            }

            StackAction::PopCas { thread } => {
                if let Some(ThreadState::PopReadHead {
                    observed_head,
                    value,
                    next: next_ptr,
                }) = next.threads.get(&thread).cloned()
                {
                    if next.head == Some(observed_head) {
                        // CAS succeeds
                        next.head = next_ptr;
                        next.popped.insert(value);
                        next.threads.insert(thread, ThreadState::Idle);
                    } else {
                        // CAS fails - retry
                        next.threads.insert(thread, ThreadState::Idle);
                    }
                }
            }
        }

        Some(next)
    }

    fn properties(&self) -> Vec<stateright::Property<Self>> {
        vec![
            stateright::Property::always("NoLostElements", |_model: &Self, state: &Self::State| {
                state.no_lost_elements()
            }),
            stateright::Property::always("NoDuplicates", |_model: &Self, state: &Self::State| {
                state.no_duplicates()
            }),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stateright::Checker;

    #[test]
    fn test_initial_state() {
        let state = StackState::new(2);
        assert!(state.head.is_none());
        assert!(state.nodes.is_empty());
        assert!(state.invariants_hold());
    }

    #[test]
    fn test_model_checking_small() {
        let model = StackModel::new(2, vec![1, 2]);

        // Run bounded model checking
        model
            .checker()
            .threads(1)
            .spawn_bfs()
            .join()
            .assert_properties();
    }

    #[test]
    #[ignore] // Slower test, run with --ignored
    fn test_model_checking_medium() {
        let model = StackModel::new(3, vec![1, 2, 3]);

        model
            .checker()
            .threads(num_cpus::get())
            .spawn_bfs()
            .join()
            .assert_properties();
    }
}
