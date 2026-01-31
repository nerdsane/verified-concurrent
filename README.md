# Verified Lock-Free

Build the "LLVM equivalent" for verified code generation - where TLA+ specs define correctness, code is disposable, and any implementation that passes the evaluator cascade is "correct by construction."

## Vision

**Specs are sacred, code is disposable.** TLA+ specifications define truth. Any implementation that passes the evaluator cascade is correct by construction.

## Three Pillars

### 1. Correctness (Evaluator Cascade)

Code must pass this cascade in order:

| Level | Tool | Time | Catches |
|-------|------|------|---------|
| 0 | rustc | instant | Type errors, lifetime issues |
| 1 | miri | seconds | Undefined behavior, aliasing |
| 2 | loom | seconds | Race conditions, memory ordering |
| 3 | DST | seconds | Faults, crashes, delays |
| 4 | stateright | seconds | Invariant violations |
| 5 | kani | minutes | Bounded proofs |

### 2. Quality (TigerStyle)

Full implementation of [TigerStyle](https://tigerstyle.dev) philosophy:

**Safety Rules (MUST pass)**:
- Defense-in-depth verification
- Explicit limits with `_MAX` suffix
- 2+ assertions per function
- u64 not usize for data fields

**Naming Rules (MUST pass)**:
- Big-endian: `segment_size_bytes_max` not `max_segment_size`
- Qualifiers at end: `connection_delay_min_ms`

### 3. Performance

- Progress guarantees: wait-free > lock-free > obstruction-free
- Memory overhead analysis
- Contention behavior

## Quick Start

```bash
# Run all tests
cargo test --workspace

# Run with specific DST seed (for reproduction)
DST_SEED=12345 cargo test -p vf-examples

# Run stateright model checking
cargo test -p vf-stateright -- --ignored
```

## Crate Map

| Crate | Purpose |
|-------|---------|
| `vf-core` | PropertyResult, invariants, counterexamples |
| `vf-dst` | SimClock, DeterministicRng, FaultInjector |
| `vf-evaluators` | Cascade orchestration (rustc → kani) |
| `vf-quality` | TigerStyle checker |
| `vf-perf` | Progress guarantees, benchmarks |
| `vf-stateright` | State machine models mirroring TLA+ |
| `vf-examples` | Reference implementations |

## TLA+ Specs

All specs live in `specs/` directory:

| Spec | Purpose |
|------|---------|
| `treiber_stack.tla` | Lock-free stack (core teaching spec) |

## DST (Deterministic Simulation Testing)

All behavior is reproducible via a seed:

```rust
use vf_dst::{DstEnv, get_or_generate_seed};

let seed = get_or_generate_seed();
let mut env = DstEnv::new(seed);

// Deterministic time
env.clock().advance_ms(100);

// Deterministic randomness
let value: u64 = env.rng().gen();

// Deterministic fault injection
if env.fault().should_fail() {
    // Simulate failure
}
```

Reproduce failures: `DST_SEED=12345 cargo test`

## License

MIT OR Apache-2.0
