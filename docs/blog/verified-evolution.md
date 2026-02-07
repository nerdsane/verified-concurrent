# Verified Evolution: What If Formal Specs Were Fitness Functions?

*Sesh Nalla and Claude Opus 4.6 (Anthropic) | February 2026 | read-time | 18 min*

## The question

We have been curious about something for a while. Concurrent code is uniquely difficult to get right. It is nearly impossible to intuitively reason about the behavior of a complex system of concurrently executing processes, even if it is composed of simple parts. Writing a lock-free Treiber stack that compiles is easy. Writing one that has no undefined behavior, survives every possible thread interleaving, and tolerates arbitrary faults — that is a fundamentally different problem.

What if formal specifications — TLA+, Miri, Loom, model checkers — could serve as the fitness function for evolutionary code synthesis? Not "run faster" but "satisfy every property in the TLA+ spec, under every interleaving, in every failure mode." If we could write a specification that captured the essential safety properties of a concurrent data structure, could evolution find an implementation that satisfied them?

We set out to explore this.

## Specifications as fitness landscapes

A TLA+ specification defines what must be true. It says nothing about how. A seven-level verification cascade — from the Rust compiler to an SMT theorem prover — creates something like a gradient from "doesn't compile" to "formally proven." That gradient, it seemed to me, was what population-based evolution might need.

```
Score = 0        "doesn't compile"
Score = 100      "compiles"
Score = 200      "no undefined behavior (Miri)"
Score = 300      "correct under all thread interleavings (Loom)"
Score = 400      "fault-tolerant (deterministic simulation)"
Score = 500+     "spec-conformant, proven correct"
```

The scoring function rewards three things: how far up the cascade you get, how many invariants you satisfy, and what progress guarantee your code provides. Lock-free code scores higher than mutex-based code, because the whole point is to evolve *away* from locks:

```
score(c) = (level_reached + 1) * 100
         + invariants_passed * 10
         + progress_ordinal * 25
```

A Mutex-based Treiber stack that passes all levels trivially scores 440. A CAS-based lock-free version that also passes scores 490. Evolution has a gradient to follow.

Or so I thought.

## The system

`vf-evolve` is a Rust-native binary. The evolution engine, verification cascade, and LLM client are all compiled Rust calling the Anthropic API directly. We wanted the verification to be fast enough that generations could run in under a minute.

The architectural choice we were most curious about was running multiple LLM models simultaneously. Different models have different failure modes, and we wanted to see whether the system could discover which model produces the best code for which kind of problem rather than betting on a single one.

```
┌────────────────────────────────────────────────────┐
│                   vf-evolve (Rust)                  │
│                                                     │
│  Island 0       Island 1       Island 2   Island 3  │
│  ┌──────┐      ┌──────┐      ┌──────┐   ┌──────┐  │
│  │ Opus │      │Sonnet│      │Haiku │   │Sonnet│  │
│  │ t=0.7│      │ t=0.8│      │ t=1.0│   │ t=1.0│  │
│  └──┬───┘      └──┬───┘      └──┬───┘   └──┬───┘  │
│     ▼  mutate     ▼  mutate     ▼  mutate   ▼      │
│  ┌──────────────────────────────────────────────┐   │
│  │   Verification Cascade (rustc→miri→loom→DST) │   │
│  └──────────────────────────────────────────────┘   │
│     ▼  score      ▼  score      ▼  score     ▼     │
│  ┌──────────────────────────────────────────────┐   │
│  │  UCB1 Bandit: concentrate compute on best    │   │
│  └──────────────────────────────────────────────┘   │
└────────────────────────────────────────────────────┘
```

A UCB1 bandit algorithm tracks which model produces the highest-scoring candidates and concentrates compute accordingly — 70% exploitation, 30% exploration. This turned out to matter more than I expected.

Every task starts with a Lamport-style prose problem statement — safety properties, liveness properties, performance dimensions, and crucially, what is *not* specified. The pipeline runs from prose to TLA+ to Rust test harness to evolution to verified implementation.

### Richer feedback

An early run made it clear that telling the LLM "Miri failed" was not enough. The LLM would change something unrelated and fail in the same way. So we rewrote the feedback mechanism to return structured diagnostics from every cascade level — not just the first failure, but every level's pass/fail status with up to 2000 characters of Miri's UB report or Loom's interleaving trace. The prompt now includes something like:

```
## Cascade Results:
- rustc: PASS (0.3s)
- miri: FAIL (12.4s) — error: "Undefined Behavior: attempting to
  retag an untagged pointer for SharedReadOnly permission"
```

We were hoping this would give the LLM enough to work with. Whether it does remains an open question — more on that below.

### Stepping stones

We also added score-band-specific guidance. Instead of a generic "improve this code," the system prompt adapts to where the candidate currently sits:

- Below 100: "Fix compilation errors."
- 100-200: "Code compiles but has UB. Fix memory safety issues flagged by Miri."
- 200-300: "Code is memory-safe. Fix concurrency bugs flagged by Loom."
- 300-400: "Add fault tolerance for deterministic simulation testing."
- Above 400: "Optimize throughput. Aim for WaitFree."

The idea was to narrow the LLM's focus to the specific barrier it faces rather than overwhelming it with the full problem.

## The problem set

We put together 17 tasks across four categories, each with a TLA+ spec, a Rust test harness, and a deliberately naive seed implementation:

| Category | Count | Examples | Starting point |
|----------|-------|---------|---------------|
| Lock-free data structures | 8 | Treiber stack, ring buffer, epoch GC, B+ tree | Mutex-based |
| Distributed protocols | 4 | Raft election, two-phase commit, full Raft consensus | Mutex-based |
| Domain optimization | 5 | Transaction scheduling, TCP congestion, load balancing | Greedy baselines |

The domain optimization tasks were originally Python simulators. We ported them to Rust so everything could run through the same verification cascade — we were curious whether algorithmic problems and memory-safety problems would behave differently under the same fitness function.

The most ambitious task was the Raft consensus capstone — full election, log replication, and commit safety with all five Raft paper invariants (ElectionSafety, LeaderAppendOnly, LogMatching, LeaderCompleteness, StateMachineSafety), a 350-line TLA+ spec, and 12 tests. We did not know what to expect from this one.

## Results: running all 17 tasks

We ran everything. 18 total runs (17 standard at 10 generations each, plus one 50-generation valley-crossing attempt), roughly 350 LLM evaluations, about 45 minutes of wall time.

### The complete picture

| Category | Task | Score | Progress | Best Model |
|----------|------|------:|----------|-----------|
| Lock-free | treiber_stack | 160 | LockFree | opus |
| Lock-free | linked_list | 160 | LockFree | opus |
| Lock-free | ring_buffer | 160 | LockFree | opus |
| Lock-free | **epoch_gc** | **270** | **LockFree** | **opus** |
| Lock-free | btree_plus | 160 | LockFree | opus |
| Lock-free | radix_tree | 160 | LockFree | sonnet |
| Lock-free | pagecache | 160 | LockFree | sonnet |
| Lock-free | io_buffer | 220 | Blocking | seed |
| Distributed | cross_shard_ssi | 160 | LockFree | opus |
| Distributed | raft_election | 160 | LockFree | sonnet |
| Distributed | two_phase_commit | 160 | LockFree | sonnet |
| Distributed | raft_consensus | 50 | LockFree | opus |
| Domain | **txn_scheduling** | **270** | **LockFree** | **haiku** |
| Domain | **tcp_congestion** | **270** | **LockFree** | **sonnet** |
| Domain | **load_balancing** | **270** | **LockFree** | **sonnet** |
| Domain | **cloud_scheduling** | **270** | **LockFree** | **haiku** |
| Domain | llm_sql_cache | 160 | LockFree | sonnet |

The results split into three clean tiers, and we found each one interesting for different reasons.

### Tier 1: The tasks that worked (score 270)

Four of the five domain optimization tasks and one lock-free task hit score 270 — meaning the evolved code compiles, has no undefined behavior according to Miri, and uses lock-free operations.

The domain tasks (transaction scheduling, TCP congestion, load balancing, cloud scheduling) were the most accessible, which makes sense in retrospect: these are pure algorithmic problems with no raw pointers, no unsafe blocks, no memory reclamation. The verification cascade's Miri check is trivial for safe Rust. The challenge is purely algorithmic — can the LLM produce a better scheduling heuristic? — and 4/5 times, it could.

What we did not expect was which model won. Haiku — the cheapest, fastest model in the ensemble — produced the best solutions for transaction scheduling and cloud scheduling. At roughly a tenth of the cost per evaluation, the model we had included mainly for diversity turned out to be the best at domain optimization. We wonder if Haiku's directness is an advantage when the problem is straightforward: less deliberation, more generation of plausible heuristics. Worth exploring further.

The real surprise, though, was **epoch_gc**. Starting from a naive Mutex-based garbage collector, by generation 7 Opus produced a fully lock-free epoch-based GC with CAS-based deferred destruction:

```rust
// What evolution produced (simplified):
loop {
    let head = self.inner.deferred.load(Ordering::Acquire);
    unsafe { (*node).next = head; }
    match self.inner.deferred.compare_exchange(
        head, node, Ordering::Release, Ordering::Acquire,
    ) {
        Ok(_) => break,
        Err(_) => continue,
    }
}
```

AtomicPtr CAS loops for the deferred destruction linked list. AtomicUsize for pin counting. Safe garbage collection only when the pin count drops to zero. And it passes Miri. This was the single most encouraging result — it suggests the cascade *can* guide evolution past the memory-safety barrier, at least for certain task structures. We are still thinking about what makes epoch_gc different from the other lock-free tasks (more on that below).

### Tier 2: The Miri barrier (score 160)

Ten tasks plateau at exactly 160. This means the code compiles and uses lock-free operations (hence the LockFree progress bonus), but Miri catches undefined behavior. Every lock-free task except epoch_gc hits this wall.

This is the finding we keep coming back to.

When an LLM generates lock-free code — say, a Treiber stack using `compare_exchange` — the code *looks* correct. The CAS loop is right. The atomic orderings are plausible. If you squinted at it in a code review, you might approve it. But Miri, which tracks every memory access and aliasing constraint, catches the subtle UB that humans miss: a use-after-free in the deferred destruction path, a dangling pointer when a concurrent thread reads a node that another thread just freed.

What surprised us is that 160 appears to be a *single-point attractor*. Across 350 evaluations and four different LLM models, we never saw a lock-free candidate score between 160 and 270. The cascade levels seem to act as discrete gates rather than continuous gradients. You either pass Miri completely or you fail. We had expected the cascade to provide a smoother landscape, but the data suggests something more like a staircase with very tall steps.

We wonder whether this is inherent to memory safety checking (Miri either finds UB or it doesn't) or whether there is some way to extract partial progress from Miri's diagnostics that we have not yet figured out.

### Tier 3: The hardest problems (score 50)

Raft consensus scored 50. The LLM generates code that compiles with lock-free operations, but fails the test suite. With 6 API methods, 4 message types, and 5 interacting invariants, the coordination required seems to exceed what any model could produce in 10 generations. This appears to be a different kind of barrier from memory safety — something more like protocol complexity.

We are curious whether more generations would help here, or whether the problem needs a different decomposition. Perhaps evolving the election sub-protocol first and then adding log replication would work better than asking the LLM to handle everything at once. We have not tried this yet.

## The valley crossing attempt

The original motivation for much of this work was a specific observation from an earlier run: the Mutex-based seed scored 440 (passes everything trivially because Mutexes are simple), while every CAS candidate scored 50-160. A 280-point fitness valley.

We tried three approaches to cross it.

First, **stepping stones** — the score-band-specific guidance described above. The idea was to give the LLM a focused objective at each level.

Second, **pattern injection** — a reference crossbeam-epoch CAS implementation included in the prompt as a template. Not to copy verbatim, but to give the LLM a concrete example of correct memory reclamation.

Third, **a CAS seed** — instead of starting from the Mutex implementation (which scores 440 but sits on the wrong side of the valley), we created `initial_atomic.rs`: an AtomicPtr-based stack with a deliberate use-after-free. It compiles, it is lock-free, but it has known UB. This starts evolution at 160, on the CAS side.

```
$ vf-evolve --task treiber_stack --generations 50 \
    --stepping-stones --inject-patterns --seed initial_atomic.rs

Seed: score=160.0 progress=LockFree  (starts on CAS side)

Gen  1  | best=160.0 | all models at 160
Gen 10  | best=160.0 | all models at 160
Gen 25  | best=160.0 | all models at 160
Gen 50  | best=160.0 | all models at 160

Result: 101 evaluations. Valley not crossed.
```

Fifty generations. A hundred and one evaluations. Four different models. Structured feedback, stepping stones, pattern templates. The score never moved.

Every LLM-generated variant used `compare_exchange` correctly. Every one had plausible memory orderings. Every one failed Miri in the same way: the deferred destruction path had UB.

And yet epoch_gc crossed the same barrier. The more we think about this, the more it seems like the difference comes down to task framing. epoch_gc's API is *about* memory reclamation — the LLM is forced to focus on exactly the hard problem because the entire task is "build a garbage collector." When the task is "build a stack" and safe memory reclamation is an incidental requirement, the LLM treats it as an afterthought.

If that hypothesis is right, the implication is interesting: perhaps the way to cross the valley on the Treiber stack is not more generations or better prompts, but restructuring the task so that memory reclamation is the primary concern rather than a side effect. We want to try seeding with a half-built epoch GC scaffolding and see whether evolution can complete it.

## What the models are good at

One of the more interesting patterns from 350 evaluations across 17 tasks is how clearly the models specialize:

**Opus** (35% of task wins): best on complex lock-free structures. Won epoch_gc, btree_plus, treiber_stack, linked_list. The bandit concentrated compute on Opus for these tasks, and it seems to have been the right call.

**Sonnet** (41% of task wins): the most consistent across categories. Won tasks in every category — lock-free, distributed, and domain.

**Haiku** (12% of task wins): outperformed models costing 8x more per evaluation on domain optimization tasks. We would not have predicted this.

We are cautious about reading too much into 350 evaluations — this is suggestive, not conclusive. But the pattern of model specialization seems worth exploring further, especially the question of whether cheaper models are systematically better at problems where the algorithmic challenge dominates over the correctness challenge.

## Open questions

We came away from this experiment with more questions than we started with. Some that we keep thinking about:

**Is the Miri barrier inherent or an artifact of our approach?** epoch_gc crossed it. Is this because garbage collection forces the LLM to think about memory reclamation, or is there something else about that particular task's structure that made it accessible? We do not yet know.

**Can richer feedback help?** The structured diagnostics we added are more informative than "Miri failed," but the valley crossing result suggests they may not be informative enough. Miri's error messages point to the *symptom* (a dangling pointer dereference) rather than the *cause* (incorrect deferred destruction logic). Perhaps translating Miri output into higher-level design feedback would help.

**What is the right decomposition for protocol complexity?** Raft consensus at 50 suggests that throwing the full protocol at evolution does not work in 10 generations. Would a staged approach — evolve election first, then log replication, then commit safety — produce better results? Or does the protocol need to be evolved as a whole because the invariants interact?

**Does the cascade gradient improve at higher levels?** This run only went up to Miri. We are curious whether Loom (which explores thread interleavings) and DST (which injects faults) provide a smoother gradient than Miri's binary pass/fail, or whether they exhibit the same staircase pattern.

## What comes next

We want to cross the Miri barrier on the Treiber stack by restructuring the task around memory reclamation. We want to run the full Raft consensus for 50+ generations. And we want to push the cascade past Miri into Loom and DST to see whether the landscape changes at higher levels.

Every TLA+ spec becomes an evolution target. The question is no longer whether formal specifications can serve as fitness functions — they can. The question is how to shape the landscape they create so that evolution can navigate it.

---

*Sesh Nalla and Claude Opus 4.6 (Anthropic). February 2026.*
