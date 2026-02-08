/// Raft Leader Election - Lock-free implementation using atomics
/// Fixed: single server election case where there are no other servers to vote

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

pub type ServerId = u64;
pub type Term = u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServerState {
    Follower,
    Candidate,
    Leader,
}

const STATE_FOLLOWER: u64 = 0;
const STATE_CANDIDATE: u64 = 1;
const STATE_LEADER: u64 = 2;

fn state_to_u64(s: ServerState) -> u64 {
    match s {
        ServerState::Follower => STATE_FOLLOWER,
        ServerState::Candidate => STATE_CANDIDATE,
        ServerState::Leader => STATE_LEADER,
    }
}

fn u64_to_state(v: u64) -> ServerState {
    match v {
        STATE_FOLLOWER => ServerState::Follower,
        STATE_CANDIDATE => ServerState::Candidate,
        STATE_LEADER => ServerState::Leader,
        _ => ServerState::Follower,
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum RaftError {
    NotCandidate,
    AlreadyVoted,
    StaleTerm,
    ClusterTooSmall,
    ServerNotFound,
}

#[derive(Debug, Clone)]
pub struct VoteRequest {
    pub term: Term,
    pub candidate_id: ServerId,
}

#[derive(Debug, Clone)]
pub struct VoteResponse {
    pub term: Term,
    pub vote_granted: bool,
    pub voter_id: ServerId,
}

#[derive(Debug, Clone)]
pub struct Heartbeat {
    pub term: Term,
    pub leader_id: ServerId,
}

const TERM_BITS: u64 = 40;
const VOTED_BITS: u64 = 22;
const STATE_BITS: u64 = 2;

const STATE_MASK: u64 = (1 << STATE_BITS) - 1; // 0x3
const VOTED_MASK: u64 = (1 << VOTED_BITS) - 1; // 0x3FFFFF
const TERM_MASK: u64 = (1 << TERM_BITS) - 1;

// Layout: [term(40) | voted_for(22) | state(2)]
fn pack(term: u64, voted_for: Option<ServerId>, state: ServerState) -> u64 {
    let t = term & TERM_MASK;
    let v = match voted_for {
        None => 0u64,
        Some(id) => (id + 1) & VOTED_MASK,
    };
    let s = state_to_u64(state) & STATE_MASK;
    (t << (VOTED_BITS + STATE_BITS)) | (v << STATE_BITS) | s
}

fn unpack_term(packed: u64) -> u64 {
    (packed >> (VOTED_BITS + STATE_BITS)) & TERM_MASK
}

fn unpack_voted_for(packed: u64) -> Option<ServerId> {
    let v = (packed >> STATE_BITS) & VOTED_MASK;
    if v == 0 {
        None
    } else {
        Some(v - 1)
    }
}

fn unpack_state(packed: u64) -> ServerState {
    u64_to_state(packed & STATE_MASK)
}

struct ServerAtomic {
    /// Packed: term(40) | voted_for(22) | state(2)
    packed: AtomicU64,
    /// Bitmask of votes received (supports up to 64 servers by index)
    votes_received: AtomicU64,
}

pub struct RaftElection {
    servers: HashMap<ServerId, ServerAtomic>,
    /// Map from ServerId to bit index (0..cluster_size)
    id_to_bit: HashMap<ServerId, u32>,
    cluster_size: usize,
    next_id: AtomicU64,
}

impl RaftElection {
    pub fn new(server_ids: &[ServerId]) -> Self {
        debug_assert!(!server_ids.is_empty(), "Cluster must have at least one server");
        debug_assert!(server_ids.len() <= 64, "Max 64 servers supported");

        let mut servers = HashMap::new();
        let mut id_to_bit = HashMap::new();

        for (idx, &id) in server_ids.iter().enumerate() {
            id_to_bit.insert(id, idx as u32);
            servers.insert(
                id,
                ServerAtomic {
                    packed: AtomicU64::new(pack(0, None, ServerState::Follower)),
                    votes_received: AtomicU64::new(0),
                },
            );
        }

        RaftElection {
            cluster_size: servers.len(),
            servers,
            id_to_bit,
            next_id: AtomicU64::new(server_ids.iter().max().copied().unwrap_or(0) + 1),
        }
    }

    pub fn cluster_size(&self) -> usize {
        self.cluster_size
    }

    pub fn quorum_size(&self) -> usize {
        self.cluster_size / 2 + 1
    }

    pub fn get_state(&self, server_id: ServerId) -> Option<ServerState> {
        self.servers
            .get(&server_id)
            .map(|s| unpack_state(s.packed.load(Ordering::SeqCst)))
    }

    pub fn get_term(&self, server_id: ServerId) -> Option<Term> {
        self.servers
            .get(&server_id)
            .map(|s| unpack_term(s.packed.load(Ordering::SeqCst)))
    }

    pub fn get_leader(&self) -> Option<ServerId> {
        for (&id, server) in &self.servers {
            let packed = server.packed.load(Ordering::SeqCst);
            if unpack_state(packed) == ServerState::Leader {
                return Some(id);
            }
        }
        None
    }

    pub fn timeout(&self, server_id: ServerId) -> Result<VoteRequest, RaftError> {
        let server = self
            .servers
            .get(&server_id)
            .ok_or(RaftError::ServerNotFound)?;

        let self_bit = self.id_to_bit[&server_id];

        loop {
            let old_packed = server.packed.load(Ordering::SeqCst);
            let old_term = unpack_term(old_packed);

            let new_term = old_term + 1;
            let new_packed = pack(new_term, Some(server_id), ServerState::Candidate);

            if server
                .packed
                .compare_exchange(old_packed, new_packed, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
            {
                // Set votes_received to just self
                server.votes_received.store(1u64 << self_bit, Ordering::SeqCst);

                // If single-server cluster, immediately become leader
                if self.cluster_size == 1 {
                    let leader_packed = pack(new_term, Some(server_id), ServerState::Leader);
                    let _ = server.packed.compare_exchange(
                        new_packed,
                        leader_packed,
                        Ordering::SeqCst,
                        Ordering::SeqCst,
                    );
                }

                return Ok(VoteRequest {
                    term: new_term,
                    candidate_id: server_id,
                });
            }
            // CAS failed, retry
        }
    }

    pub fn handle_vote_request(
        &self,
        server_id: ServerId,
        request: &VoteRequest,
    ) -> Result<VoteResponse, RaftError> {
        let server = self
            .servers
            .get(&server_id)
            .ok_or(RaftError::ServerNotFound)?;

        loop {
            let old_packed = server.packed.load(Ordering::SeqCst);
            let mut current_term = unpack_term(old_packed);
            let mut voted_for = unpack_voted_for(old_packed);
            let mut state = unpack_state(old_packed);

            // If the request has a higher term, step down
            if request.term > current_term {
                current_term = request.term;
                state = ServerState::Follower;
                voted_for = None;
            }

            // Grant vote if: term matches, and we haven't voted or voted for this candidate
            let vote_granted = request.term >= current_term
                && (voted_for.is_none() || voted_for == Some(request.candidate_id));

            if vote_granted {
                voted_for = Some(request.candidate_id);
            }

            let new_packed = pack(current_term, voted_for, state);

            if old_packed == new_packed {
                // No change needed
                return Ok(VoteResponse {
                    term: current_term,
                    vote_granted,
                    voter_id: server_id,
                });
            }

            if server
                .packed
                .compare_exchange(old_packed, new_packed, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
            {
                // If we stepped down, clear votes
                if state == ServerState::Follower && unpack_state(old_packed) != ServerState::Follower {
                    server.votes_received.store(0, Ordering::SeqCst);
                }

                return Ok(VoteResponse {
                    term: current_term,
                    vote_granted,
                    voter_id: server_id,
                });
            }
            // CAS failed, retry
        }
    }

    pub fn handle_vote_response(
        &self,
        candidate_id: ServerId,
        response: &VoteResponse,
    ) -> Result<bool, RaftError> {
        let server = self
            .servers
            .get(&candidate_id)
            .ok_or(RaftError::ServerNotFound)?;

        loop {
            let old_packed = server.packed.load(Ordering::SeqCst);
            let current_term = unpack_term(old_packed);
            let state = unpack_state(old_packed);

            // If response has higher term, step down
            if response.term > current_term {
                let new_packed = pack(response.term, None, ServerState::Follower);
                if server
                    .packed
                    .compare_exchange(old_packed, new_packed, Ordering::SeqCst, Ordering::SeqCst)
                    .is_ok()
                {
                    server.votes_received.store(0, Ordering::SeqCst);
                    return Ok(false);
                }
                continue; // retry
            }

            // Only process if still a candidate in the same term
            if state != ServerState::Candidate || response.term != current_term {
                return Ok(state == ServerState::Leader && response.term == current_term);
            }

            if response.vote_granted {
                if let Some(&voter_bit) = self.id_to_bit.get(&response.voter_id) {
                    let bit = 1u64 << voter_bit;
                    // Atomically add the vote
                    let old_votes = server.votes_received.fetch_or(bit, Ordering::SeqCst);
                    let new_votes = old_votes | bit;
                    let vote_count = new_votes.count_ones() as usize;

                    if vote_count >= self.quorum_size() {
                        // Try to become leader
                        let current = server.packed.load(Ordering::SeqCst);
                        if unpack_state(current) == ServerState::Candidate
                            && unpack_term(current) == current_term
                        {
                            let leader_packed = pack(current_term, Some(candidate_id), ServerState::Leader);
                            let _ = server.packed.compare_exchange(
                                current,
                                leader_packed,
                                Ordering::SeqCst,
                                Ordering::SeqCst,
                            );
                        }
                        // Check if we actually became leader
                        let final_packed = server.packed.load(Ordering::SeqCst);
                        return Ok(
                            unpack_state(final_packed) == ServerState::Leader
                                && unpack_term(final_packed) == current_term,
                        );
                    }
                }
            }

            return Ok(false);
        }
    }

    pub fn handle_heartbeat(
        &self,
        server_id: ServerId,
        heartbeat: &Heartbeat,
    ) -> Result<(), RaftError> {
        let server = self
            .servers
            .get(&server_id)
            .ok_or(RaftError::ServerNotFound)?;

        loop {
            let old_packed = server.packed.load(Ordering::SeqCst);
            let current_term = unpack_term(old_packed);

            if heartbeat.term >= current_term {
                let voted_for = if heartbeat.term > current_term {
                    None
                } else {
                    unpack_voted_for(old_packed)
                };
                let new_packed = pack(heartbeat.term, voted_for, ServerState::Follower);

                if old_packed == new_packed {
                    server.votes_received.store(0, Ordering::SeqCst);
                    return Ok(());
                }

                if server
                    .packed
                    .compare_exchange(old_packed, new_packed, Ordering::SeqCst, Ordering::SeqCst)
                    .is_ok()
                {
                    server.votes_received.store(0, Ordering::SeqCst);
                    return Ok(());
                }
                continue;
            }

            return Ok(());
        }
    }

    pub fn create_heartbeat(&self, leader_id: ServerId) -> Result<Heartbeat, RaftError> {
        let server = self
            .servers
            .get(&leader_id)
            .ok_or(RaftError::ServerNotFound)?;

        let packed = server.packed.load(Ordering::SeqCst);

        if unpack_state(packed) != ServerState::Leader {
            return Err(RaftError::NotCandidate);
        }

        Ok(Heartbeat {
            term: unpack_term(packed),
            leader_id,
        })
    }

    pub fn run_election(&self, candidate_id: ServerId) -> Result<bool, RaftError> {
        let vote_request = self.timeout(candidate_id)?;

        // Check if already won (single server case)
        if self.get_state(candidate_id) == Some(ServerState::Leader) {
            return Ok(true);
        }

        let other_servers: Vec<ServerId> = self
            .servers
            .keys()
            .filter(|&&id| id != candidate_id)
            .copied()
            .collect();

        for &other_id in &other_servers {
            let response = self.handle_vote_request(other_id, &vote_request)?;
            let won = self.handle_vote_response(candidate_id, &response)?;
            if won {
                return Ok(true);
            }
        }

        Ok(self.get_state(candidate_id) == Some(ServerState::Leader))
    }
}