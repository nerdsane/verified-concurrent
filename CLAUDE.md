# Verified Lock-Free Development Guide

## Project Vision

Build the "LLVM equivalent" for verified code generation - where TLA+ specs define correctness, code is disposable, and any implementation that passes the evaluator cascade is "correct by construction."

## Core Principles

1. **Specs are sacred, code is disposable** - TLA+ specs define truth
2. **Three pillars**: Correctness + Quality + Performance
3. **DST is core** - Deterministic Simulation Testing catches most issues
4. **TigerStyle is mandatory** - Not optional guidelines, required rules

## The Verification Pyramid

```
         TLA+ Specs (specs/*.tla)
                    │ defines
         Shared Invariants (vf-core/src/invariants/)
                    │ verified by
    ┌───────────────┼───────────────┐
    ▼               ▼               ▼
Stateright      DST Tests        Loom
(exhaustive)    (simulation)    (interleavings)
```

## Evaluator Cascade

All code must pass this cascade in order:

| Level | Tool | Time | Catches |
|-------|------|------|---------|
| 0 | rustc | instant | Type errors, lifetime issues |
| 1 | miri | seconds | Undefined behavior, aliasing |
| 2 | loom | seconds | Race conditions, memory ordering |
| 3 | DST | seconds | Faults, crashes, delays |
| 4 | stateright | seconds | Invariant violations |
| 5 | kani | minutes | Bounded proofs |

## Three Pillars

### Pillar 1: Correctness
- Invariants from TLA+ specs
- Verified by evaluator cascade
- Counterexamples on failure

### Pillar 2: Quality (TigerStyle)

**Safety Rules (MUST pass)**:
- Defense-in-depth: Programs verify themselves
- Explicit limits: Bound all resources with `_MAX` suffix
- Static allocation: All memory at startup
- 2+ assertions per function
- Zero dependencies (minimal Cargo.toml)
- u64 not usize for data fields

**Performance Rules (SHOULD pass)**:
- Primary Colors: network, storage, memory, compute
- Control/Data plane separation
- Zero copy operations
- Cache-aligned structs

**Naming Rules (MUST pass)**:
- Big-endian: `segment_size_bytes_max` not `max_segment_size`
- Qualifiers last: `connection_delay_min_ms`
- Snake_case, no abbreviations

### Pillar 3: Performance
- Progress guarantees: wait-free > lock-free > obstruction-free
- Memory overhead analysis
- Contention behavior

## DST Pattern

```rust
#[test]
fn test_under_faults() {
    let seed = std::env::var("DST_SEED")
        .map(|s| s.parse().unwrap())
        .unwrap_or_else(|| rand::random());
    println!("DST_SEED={}", seed);

    let env = DstEnv::new(seed);
    // ... test with env.clock(), env.rng(), env.fault()
}
```

**Reproduce failures**: `DST_SEED=12345 cargo test`

## Crate Map

| Crate | Purpose |
|-------|---------|
| vf-core | PropertyResult, invariants, counterexamples |
| vf-dst | SimClock, DeterministicRng, FaultInjector |
| vf-evaluators | Cascade orchestration (rustc → kani) |
| vf-quality | TigerStyle checker, clippy integration |
| vf-perf | Progress guarantees, benchmark harness |
| vf-stateright | State machine models mirroring TLA+ |
| vf-examples | Reference implementations |

## TLA+ Specs

All specs live in `specs/` directory:

| Spec | Purpose |
|------|---------|
| treiber_stack.tla | Lock-free stack (core teaching spec) |
| treiber_stack_aba.tla | Deliberately vulnerable version |
| ms_queue.tla | Michael-Scott queue |
| ring_buffer.tla | Lock-free ring buffer |
| epoch_gc.tla | Memory reclamation |

## Commands

```bash
# Run all tests
cargo test --workspace

# Run with specific DST seed (for reproduction)
DST_SEED=12345 cargo test -p vf-examples

# Run loom tests (slow, thorough)
RUSTFLAGS="--cfg loom" cargo test --release -p vf-examples

# Run stateright model checking
cargo test -p vf-stateright -- --ignored

# Check TigerStyle compliance
cargo run -p vf-quality -- check crates/vf-examples/src/

# Full cascade on a file
cargo run -p vf-evaluators -- cascade crates/vf-examples/src/treiber_stack.rs
```

## Adding New Lock-Free Structures

1. Write TLA+ spec in `specs/lockfree/`
2. Add invariants to `vf-core/src/invariants/`
3. Create Stateright model in `vf-stateright/src/`
4. Write reference implementation in `vf-examples/src/`
5. Add DST tests in `vf-examples/tests/`
6. Verify: `cargo run -p vf-evaluators -- cascade <file>`

## Known Pitfalls

| Mistake | Correct Behavior |
|---------|------------------|
| Using `tokio::time::sleep()` in DST | Use `env.clock().sleep()` |
| Using `usize` for counts | Use `u64` for cross-platform |
| Missing retry loop in CAS | All CAS must retry on failure |
| Wrong memory ordering | Use Acquire/Release/AcqRel appropriately |
| Unbounded data structures | Always have `_MAX` constants |

## References

- [TigerStyle](https://tigerstyle.dev) - Full philosophy
- [Loom documentation](https://docs.rs/loom)
- [Stateright book](https://www.stateright.rs/)
- [spacejam/tla-rust](https://github.com/spacejam/tla-rust) - Inspiration
