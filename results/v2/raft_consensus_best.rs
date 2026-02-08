/// Raft Consensus - Lock-free implementation using atomics and CAS operations
/// Progress guarantee: LockFree (CAS-based state transitions, wait-free reads)
use std::sync::atomic::{AtomicU64, AtomicPtr, Ordering};
use std::collections::HashMap;
use std::ptr;

pub type NodeId = u64;
pub type Term = u64;
pub type LogIndex = u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeState {
    Follower,
    Candidate,
    Leader,
}

#[derive(Debug, Clone, PartialEq)]
pub enum RaftError {
    NotLeader,
    NotCandidate,
    StaleTerm,
    NodeNotFound,
    LogInconsistency,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LogEntry {
    pub index: LogIndex,
    pub term: Term,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone)]
pub enum Message {
    RequestVote {
        term: Term,
        candidate_id: NodeId,
        last_log_index: LogIndex,
        last_log_term: Term,
    },
    RequestVoteResponse {
        term: Term,
        voter_id: NodeId,
        vote_granted: bool,
    },
    AppendEntries {
        term: Term,
        leader_id: NodeId,
        prev_log_index: LogIndex,
        prev_log_term: Term,
        entries: Vec<LogEntry>,
        leader_commit: LogIndex,
    },
    AppendEntriesResponse {
        term: Term,
        follower_id: NodeId,
        success: bool,
        match_index: LogIndex,
    },
}

#[derive(Debug, Clone, Default)]
pub struct Ready {
    pub messages: Vec<(NodeId, Message)>,
    pub committed_entries: Vec<LogEntry>,
    pub should_persist: bool,
}

/// Inner mutable state, swapped atomically via AtomicPtr (CAS-based, lock-free)
#[derive(Clone)]
struct NodeInner {
    id: NodeId,
    peers: Vec<NodeId>,
    state: NodeState,
    current_term: Term,
    voted_for: Option<NodeId>,
    log: Vec<LogEntry>,
    commit_index: LogIndex,
    last_applied: LogIndex,
    next_index: HashMap<NodeId, LogIndex>,
    match_index: HashMap<NodeId, LogIndex>,
    votes_received: Vec<NodeId>,
    election_elapsed: u64,
    heartbeat_elapsed: u64,
    election_timeout: u64,
    heartbeat_interval: u64,
    pending_messages: Vec<(NodeId, Message)>,
    pending_committed: Vec<LogEntry>,
}

pub struct RaftNode {
    // Wait-free read caches
    cached_id: AtomicU64,
    cached_term: AtomicU64,
    cached_state: AtomicU64,
    cached_commit_index: AtomicU64,
    cached_log_len: AtomicU64,
    // Lock-free mutable state via CAS on pointer
    state_ptr: AtomicPtr<NodeInner>,
}

// Manual Send+Sync since we manage the pointer carefully
unsafe impl Send for RaftNode {}
unsafe impl Sync for RaftNode {}

impl Drop for RaftNode {
    fn drop(&mut self) {
        let ptr = self.state_ptr.load(Ordering::Acquire);
        if !ptr.is_null() {
            unsafe { drop(Box::from_raw(ptr)); }
        }
    }
}

impl RaftNode {
    pub fn new(id: NodeId, peers: Vec<NodeId>) -> Self {
        let inner = Box::new(NodeInner {
            id,
            peers,
            state: NodeState::Follower,
            current_term: 0,
            voted_for: None,
            log: Vec::new(),
            commit_index: 0,
            last_applied: 0,
            next_index: HashMap::new(),
            match_index: HashMap::new(),
            votes_received: Vec::new(),
            election_elapsed: 0,
            heartbeat_elapsed: 0,
            election_timeout: 10,
            heartbeat_interval: 3,
            pending_messages: Vec::new(),
            pending_committed: Vec::new(),
        });
        RaftNode {
            cached_id: AtomicU64::new(id),
            cached_term: AtomicU64::new(0),
            cached_state: AtomicU64::new(0),
            cached_commit_index: AtomicU64::new(0),
            cached_log_len: AtomicU64::new(0),
            state_ptr: AtomicPtr::new(Box::into_raw(inner)),
        }
    }

    // Wait-free reads
    pub fn id(&self) -> NodeId {
        self.cached_id.load(Ordering::Acquire)
    }

    pub fn state(&self) -> NodeState {
        match self.cached_state.load(Ordering::Acquire) {
            1 => NodeState::Candidate,
            2 => NodeState::Leader,
            _ => NodeState::Follower,
        }
    }

    pub fn term(&self) -> Term {
        self.cached_term.load(Ordering::Acquire)
    }

    pub fn commit_index(&self) -> LogIndex {
        self.cached_commit_index.load(Ordering::Acquire)
    }

    pub fn log_len(&self) -> u64 {
        self.cached_log_len.load(Ordering::Acquire)
    }

    fn state_to_u64(s: NodeState) -> u64 {
        match s {
            NodeState::Follower => 0,
            NodeState::Candidate => 1,
            NodeState::Leader => 2,
        }
    }

    fn sync_caches(&self, inner: &NodeInner) {
        self.cached_term.store(inner.current_term, Ordering::Release);
        self.cached_state.store(Self::state_to_u64(inner.state), Ordering::Release);
        self.cached_commit_index.store(inner.commit_index, Ordering::Release);
        self.cached_log_len.store(inner.log.len() as u64, Ordering::Release);
    }

    /// CAS-based state update: read current, clone, mutate, CAS swap.
    /// Returns old Box for reclamation on success, or None on CAS failure (retry).
    fn cas_update<F>(&self, mut f: F) where F: FnMut(&mut NodeInner) {
        loop {
            let old_ptr = self.state_ptr.load(Ordering::Acquire);
            if old_ptr.is_null() {
                return;
            }
            // Clone the current state
            let old_ref = unsafe { &*old_ptr };
            let mut new_inner = old_ref.clone();
            // Apply mutation
            f(&mut new_inner);
            // Update caches
            self.sync_caches(&new_inner);
            let new_ptr = Box::into_raw(Box::new(new_inner));
            // CAS
            match self.state_ptr.compare_exchange(old_ptr, new_ptr, Ordering::AcqRel, Ordering::Acquire) {
                Ok(_old) => {
                    // Successfully swapped; reclaim old
                    unsafe { drop(Box::from_raw(old_ptr)); }
                    return;
                }
                Err(_) => {
                    // CAS failed, reclaim the new allocation and retry
                    unsafe { drop(Box::from_raw(new_ptr)); }
                    // Loop and retry - this is lock-free (progress guaranteed if no contention)
                }
            }
        }
    }

    /// Read current state snapshot
    fn read_state<R, F>(&self, f: F) -> R where F: FnOnce(&NodeInner) -> R {
        let ptr = self.state_ptr.load(Ordering::Acquire);
        assert!(!ptr.is_null());
        let inner = unsafe { &*ptr };
        f(inner)
    }

    fn quorum_size(cluster_size: usize) -> usize {
        cluster_size / 2 + 1
    }

    pub fn tick(&self) {
        self.cas_update(|inner| {
            match inner.state {
                NodeState::Leader => {
                    inner.heartbeat_elapsed += 1;
                    if inner.heartbeat_elapsed >= inner.heartbeat_interval {
                        inner.heartbeat_elapsed = 0;
                        Self::send_heartbeats(inner);
                    }
                }
                NodeState::Follower | NodeState::Candidate => {
                    inner.election_elapsed += 1;
                    if inner.election_elapsed >= inner.election_timeout {
                        Self::start_election(inner);
                    }
                }
            }
        });
    }

    pub fn propose(&self, data: Vec<u8>) -> Result<u64, RaftError> {
        // First check state wait-free
        if self.state() != NodeState::Leader {
            return Err(RaftError::NotLeader);
        }

        let mut result = Err(RaftError::NotLeader);
        self.cas_update(|inner| {
            if inner.state != NodeState::Leader {
                result = Err(RaftError::NotLeader);
                return;
            }

            let index = inner.log.len() as u64 + 1;
            let term = inner.current_term;
            let entry = LogEntry { index, term, data: data.clone() };
            inner.log.push(entry);

            let self_id = inner.id;
            inner.match_index.insert(self_id, index);

            Self::send_append_entries(inner);
            result = Ok(index);
        });
        result
    }

    pub fn step(&self, message: Message) -> Result<(), RaftError> {
        self.cas_update(|inner| {
            match message.clone() {
                Message::RequestVote {
                    term,
                    candidate_id,
                    last_log_index,
                    last_log_term,
                } => {
                    Self::handle_request_vote(inner, term, candidate_id, last_log_index, last_log_term);
                }
                Message::RequestVoteResponse {
                    term,
                    voter_id,
                    vote_granted,
                } => {
                    Self::handle_vote_response(inner, term, voter_id, vote_granted);
                }
                Message::AppendEntries {
                    term,
                    leader_id,
                    prev_log_index,
                    prev_log_term,
                    entries,
                    leader_commit,
                } => {
                    Self::handle_append_entries(inner, term, leader_id, prev_log_index, prev_log_term, entries, leader_commit);
                }
                Message::AppendEntriesResponse {
                    term,
                    follower_id,
                    success,
                    match_index,
                } => {
                    Self::handle_append_entries_response(inner, term, follower_id, success, match_index);
                }
            }
        });
        Ok(())
    }

    pub fn ready(&self) -> Ready {
        self.read_state(|inner| {
            Ready {
                messages: inner.pending_messages.clone(),
                committed_entries: inner.pending_committed.clone(),
                should_persist: !inner.pending_messages.is_empty() || !inner.pending_committed.is_empty(),
            }
        })
    }

    pub fn advance(&self, _ready: Ready) {
        self.cas_update(|inner| {
            inner.pending_messages.clear();
            inner.pending_committed.clear();
        });
    }

    fn last_log_info(inner: &NodeInner) -> (LogIndex, Term) {
        inner.log.last().map(|e| (e.index, e.term)).unwrap_or((0, 0))
    }

    fn start_election(inner: &mut NodeInner) {
        inner.current_term += 1;
        inner.state = NodeState::Candidate;
        let self_id = inner.id;
        inner.voted_for = Some(self_id);
        inner.votes_received = vec![self_id];
        inner.election_elapsed = 0;

        let cluster_size = inner.peers.len() + 1;
        if inner.votes_received.len() >= Self::quorum_size(cluster_size) {
            Self::become_leader(inner);
            return;
        }

        let (last_log_index, last_log_term) = Self::last_log_info(inner);
        let term = inner.current_term;
        let candidate_id = inner.id;

        let peers: Vec<NodeId> = inner.peers.clone();
        for peer in peers {
            inner.pending_messages.push((
                peer,
                Message::RequestVote {
                    term,
                    candidate_id,
                    last_log_index,
                    last_log_term,
                },
            ));
        }
    }

    fn send_heartbeats(inner: &mut NodeInner) {
        let peers: Vec<NodeId> = inner.peers.clone();
        let term = inner.current_term;
        let leader_id = inner.id;
        let leader_commit = inner.commit_index;

        for peer in peers {
            let next_idx = inner.next_index.get(&peer).copied().unwrap_or(1);
            let prev_log_index = next_idx.saturating_sub(1);
            let prev_log_term = if prev_log_index == 0 {
                0
            } else {
                inner.log.get((prev_log_index - 1) as usize).map(|e| e.term).unwrap_or(0)
            };

            let entries: Vec<LogEntry> = inner.log.iter().filter(|e| e.index >= next_idx).cloned().collect();

            inner.pending_messages.push((
                peer,
                Message::AppendEntries {
                    term,
                    leader_id,
                    prev_log_index,
                    prev_log_term,
                    entries,
                    leader_commit,
                },
            ));
        }
    }

    fn send_append_entries(inner: &mut NodeInner) {
        Self::send_heartbeats(inner);
    }

    fn handle_request_vote(
        inner: &mut NodeInner,
        term: Term,
        candidate_id: NodeId,
        last_log_index: LogIndex,
        last_log_term: Term,
    ) {
        if term > inner.current_term {
            inner.current_term = term;
            inner.state = NodeState::Follower;
            inner.voted_for = None;
            inner.votes_received.clear();
        }

        let (my_last_index, my_last_term) = Self::last_log_info(inner);
        let log_ok = last_log_term > my_last_term
            || (last_log_term == my_last_term && last_log_index >= my_last_index);

        let vote_granted = term >= inner.current_term
            && (inner.voted_for.is_none() || inner.voted_for == Some(candidate_id))
            && log_ok;

        if vote_granted {
            inner.voted_for = Some(candidate_id);
            inner.election_elapsed = 0;
        }

        let self_id = inner.id;
        let current_term = inner.current_term;
        inner.pending_messages.push((
            candidate_id,
            Message::RequestVoteResponse {
                term: current_term,
                voter_id: self_id,
                vote_granted,
            },
        ));
    }

    fn handle_vote_response(inner: &mut NodeInner, term: Term, voter_id: NodeId, vote_granted: bool) {
        if term > inner.current_term {
            inner.current_term = term;
            inner.state = NodeState::Follower;
            inner.voted_for = None;
            inner.votes_received.clear();
            return;
        }

        if inner.state != NodeState::Candidate || term != inner.current_term {
            return;
        }

        if vote_granted && !inner.votes_received.contains(&voter_id) {
            inner.votes_received.push(voter_id);
        }

        let cluster_size = inner.peers.len() + 1;
        if inner.votes_received.len() >= Self::quorum_size(cluster_size) {
            Self::become_leader(inner);
        }
    }

    fn become_leader(inner: &mut NodeInner) {
        inner.state = NodeState::Leader;
        inner.heartbeat_elapsed = 0;

        let last_log_idx = inner.log.len() as u64 + 1;
        let self_id = inner.id;
        let self_log_len = inner.log.len() as u64;
        let peers: Vec<NodeId> = inner.peers.clone();
        for peer in &peers {
            inner.next_index.insert(*peer, last_log_idx);
            inner.match_index.insert(*peer, 0);
        }
        inner.match_index.insert(self_id, self_log_len);

        Self::send_heartbeats(inner);
    }

    fn handle_append_entries(
        inner: &mut NodeInner,
        term: Term,
        leader_id: NodeId,
        prev_log_index: LogIndex,
        prev_log_term: Term,
        entries: Vec<LogEntry>,
        leader_commit: LogIndex,
    ) {
        if term > inner.current_term {
            inner.current_term = term;
            inner.voted_for = None;
            inner.votes_received.clear();
        }

        if term < inner.current_term {
            let self_id = inner.id;
            let current_term = inner.current_term;
            inner.pending_messages.push((
                leader_id,
                Message::AppendEntriesResponse {
                    term: current_term,
                    follower_id: self_id,
                    success: false,
                    match_index: 0,
                },
            ));
            return;
        }

        inner.state = NodeState::Follower;
        inner.election_elapsed = 0;

        if prev_log_index > 0 {
            let has_entry = inner
                .log
                .get((prev_log_index - 1) as usize)
                .map(|e| e.term == prev_log_term)
                .unwrap_or(false);
            if !has_entry {
                let self_id = inner.id;
                let current_term = inner.current_term;
                inner.pending_messages.push((
                    leader_id,
                    Message::AppendEntriesResponse {
                        term: current_term,
                        follower_id: self_id,
                        success: false,
                        match_index: 0,
                    },
                ));
                return;
            }
        }

        for entry in &entries {
            let idx = (entry.index - 1) as usize;
            if idx < inner.log.len() {
                if inner.log[idx].term != entry.term {
                    inner.log.truncate(idx);
                    inner.log.push(entry.clone());
                }
            } else {
                inner.log.push(entry.clone());
            }
        }

        if leader_commit > inner.commit_index {
            let last_new_idx = entries.last().map(|e| e.index).unwrap_or(inner.log.len() as u64);
            inner.commit_index = leader_commit.min(last_new_idx);
            Self::apply_committed(inner);
        }

        let match_idx = inner.log.len() as u64;
        let self_id = inner.id;
        let current_term = inner.current_term;
        inner.pending_messages.push((
            leader_id,
            Message::AppendEntriesResponse {
                term: current_term,
                follower_id: self_id,
                success: true,
                match_index: match_idx,
            },
        ));
    }

    fn handle_append_entries_response(
        inner: &mut NodeInner,
        term: Term,
        follower_id: NodeId,
        success: bool,
        match_index: LogIndex,
    ) {
        if term > inner.current_term {
            inner.current_term = term;
            inner.state = NodeState::Follower;
            inner.voted_for = None;
            inner.votes_received.clear();
            return;
        }

        if inner.state != NodeState::Leader || term != inner.current_term {
            return;
        }

        if success {
            let current_match = inner.match_index.get(&follower_id).copied().unwrap_or(0);
            if match_index > current_match {
                inner.next_index.insert(follower_id, match_index + 1);
                inner.match_index.insert(follower_id, match_index);
            }
            Self::try_advance_commit(inner);
        } else {
            let next = inner.next_index.get(&follower_id).copied().unwrap_or(1);
            let new_next = next.saturating_sub(1).max(1);
            inner.next_index.insert(follower_id, new_next);

            // Immediately retry
            let term = inner.current_term;
            let leader_id = inner.id;
            let leader_commit = inner.commit_index;
            let prev_log_index = new_next.saturating_sub(1);
            let prev_log_term = if prev_log_index == 0 {
                0
            } else {
                inner.log.get((prev_log_index - 1) as usize).map(|e| e.term).unwrap_or(0)
            };
            let entries: Vec<LogEntry> = inner.log.iter().filter(|e| e.index >= new_next).cloned().collect();
            inner.pending_messages.push((
                follower_id,
                Message::AppendEntries {
                    term,
                    leader_id,
                    prev_log_index,
                    prev_log_term,
                    entries,
                    leader_commit,
                },
            ));
        }
    }

    fn try_advance_commit(inner: &mut NodeInner) {
        let cluster_size = inner.peers.len() + 1;
        let quorum = Self::quorum_size(cluster_size);
        let current_term = inner.current_term;
        let log_len = inner.log.len() as u64;

        for n in (inner.commit_index + 1..=log_len).rev() {
            let replicated = inner.match_index.values().filter(|&&mi| mi >= n).count();

            if replicated >= quorum {
                if let Some(entry) = inner.log.get((n - 1) as usize) {
                    if entry.term == current_term {
                        inner.commit_index = n;
                        Self::apply_committed(inner);
                        break;
                    }
                }
            }
        }
    }

    fn apply_committed(inner: &mut NodeInner) {
        while inner.last_applied < inner.commit_index {
            inner.last_applied += 1;
            if let Some(entry) = inner.log.get((inner.last_applied - 1) as usize) {
                inner.pending_committed.push(entry.clone());
            }
        }
    }
}