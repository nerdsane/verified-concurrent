//! CLI for generating verified implementations from TLA+ specs.
//!
//! # Usage
//!
//! ```bash
//! # Generate from spec file
//! cargo run -p vf-generator --bin vf-generate -- --spec specs/lockfree/treiber_stack.tla
//!
//! # Quick mode (faster verification)
//! cargo run -p vf-generator --bin vf-generate -- --spec specs/lockfree/treiber_stack.tla --quick
//!
//! # Save output to file
//! cargo run -p vf-generator --bin vf-generate -- --spec specs/lockfree/treiber_stack.tla --output generated.rs
//! ```

use std::path::PathBuf;
use std::process::ExitCode;

use vf_evaluators::EvaluatorLevel;
use vf_generator::{CodeGenerator, GeneratorConfig};
use vf_perf::ProgressGuarantee;

#[tokio::main]
async fn main() -> ExitCode {
    let args = Args::parse();

    if args.help {
        print_help();
        return ExitCode::SUCCESS;
    }

    let spec_path = match args.spec {
        Some(path) => path,
        None => {
            eprintln!("Error: --spec is required");
            eprintln!("Run with --help for usage information");
            return ExitCode::FAILURE;
        }
    };

    // Build config
    let mut config = if args.quick {
        GeneratorConfig::quick()
    } else if args.thorough {
        GeneratorConfig::thorough()
    } else {
        GeneratorConfig::default()
    };

    if let Some(max) = args.max_attempts {
        config.max_correctness_attempts = max;
    }

    if let Some(ref level) = args.max_level {
        config.cascade_config.max_level = parse_level(level);
    }

    if let Some(ref target) = args.target_progress {
        config.target_progress_guarantee = parse_progress(target);
    }

    config.verbose = !args.quiet;

    // Create generator
    let generator = match CodeGenerator::from_env(config.clone()) {
        Ok(g) => g,
        Err(e) => {
            eprintln!("Error creating generator: {}", e);
            eprintln!();
            eprintln!("Make sure ANTHROPIC_API_KEY is set:");
            eprintln!("  export ANTHROPIC_API_KEY=sk-ant-...");
            return ExitCode::FAILURE;
        }
    };

    println!("Verified Code Generator (Bitter Lesson Aligned)");
    println!("================================================");
    println!();
    println!("Spec: {}", spec_path.display());
    println!("Max cascade level: {:?}", config.cascade_config.max_level);
    println!("Target progress: {:?}", config.target_progress_guarantee);
    println!();

    // Generate
    match generator.generate_from_file(&spec_path).await {
        Ok(result) => {
            println!();
            println!("{}", result.format_summary());

            if result.success {
                if let Some(ref code) = result.code {
                    // Output to file or stdout
                    if let Some(output_path) = args.output {
                        match std::fs::write(&output_path, code) {
                            Ok(()) => {
                                println!("Code written to: {}", output_path.display());
                            }
                            Err(e) => {
                                eprintln!("Failed to write output: {}", e);
                                return ExitCode::FAILURE;
                            }
                        }
                    } else {
                        println!();
                        println!("Generated Code:");
                        println!("===============");
                        println!();
                        println!("{}", code);
                    }
                }
                ExitCode::SUCCESS
            } else {
                let total_attempts = result.correctness_attempts + result.perf_attempts;
                eprintln!("Generation failed after {} attempts", total_attempts);
                ExitCode::FAILURE
            }
        }
        Err(e) => {
            eprintln!("Error: {}", e);
            ExitCode::FAILURE
        }
    }
}

/// Simple argument parsing (no external deps).
struct Args {
    spec: Option<PathBuf>,
    output: Option<PathBuf>,
    max_attempts: Option<u32>,
    max_level: Option<String>,
    target_progress: Option<String>,
    quick: bool,
    thorough: bool,
    quiet: bool,
    help: bool,
}

impl Args {
    fn parse() -> Self {
        let mut args = Args {
            spec: None,
            output: None,
            max_attempts: None,
            max_level: None,
            target_progress: None,
            quick: false,
            thorough: false,
            quiet: false,
            help: false,
        };

        let mut iter = std::env::args().skip(1);
        while let Some(arg) = iter.next() {
            match arg.as_str() {
                "--spec" | "-s" => {
                    args.spec = iter.next().map(PathBuf::from);
                }
                "--output" | "-o" => {
                    args.output = iter.next().map(PathBuf::from);
                }
                "--max-attempts" | "-n" => {
                    args.max_attempts = iter.next().and_then(|s| s.parse().ok());
                }
                "--max-level" | "-l" => {
                    args.max_level = iter.next();
                }
                "--target-progress" | "-p" => {
                    args.target_progress = iter.next();
                }
                "--quick" => {
                    args.quick = true;
                }
                "--thorough" => {
                    args.thorough = true;
                }
                "--quiet" | "-q" => {
                    args.quiet = true;
                }
                "--help" | "-h" => {
                    args.help = true;
                }
                other => {
                    // Treat as spec path if no flag
                    if !other.starts_with('-') && args.spec.is_none() {
                        args.spec = Some(PathBuf::from(other));
                    }
                }
            }
        }

        args
    }
}

fn parse_level(s: &str) -> EvaluatorLevel {
    match s.to_lowercase().as_str() {
        "rustc" | "0" => EvaluatorLevel::Rustc,
        "miri" | "1" => EvaluatorLevel::Miri,
        "loom" | "2" => EvaluatorLevel::Loom,
        "dst" | "3" => EvaluatorLevel::Dst,
        "stateright" | "4" => EvaluatorLevel::Stateright,
        "kani" | "5" => EvaluatorLevel::Kani,
        "verus" | "6" => EvaluatorLevel::Verus,
        _ => EvaluatorLevel::Dst, // Default
    }
}

fn parse_progress(s: &str) -> ProgressGuarantee {
    match s.to_lowercase().as_str() {
        "blocking" | "0" => ProgressGuarantee::Blocking,
        "obstruction-free" | "obstruction" | "1" => ProgressGuarantee::ObstructionFree,
        "lock-free" | "lockfree" | "2" => ProgressGuarantee::LockFree,
        "wait-free" | "waitfree" | "3" => ProgressGuarantee::WaitFree,
        _ => ProgressGuarantee::LockFree, // Default
    }
}

fn print_help() {
    println!(
        r#"vf-generate - Generate verified code from TLA+ specs (Bitter Lesson Aligned)

USAGE:
    vf-generate --spec <SPEC_FILE> [OPTIONS]

PHILOSOPHY:
    - Prompts derived from specs (no implementation hints)
    - LLM figures out implementation strategy
    - Cascade feedback is diagnostic (what failed, not how to fix)
    - Performance is first-class (iterate toward best performing correct solution)

OPTIONS:
    -s, --spec <FILE>           TLA+ specification file (required)
    -o, --output <FILE>         Output file for generated code (default: stdout)
    -n, --max-attempts <N>      Maximum correctness attempts (default: 5)
    -l, --max-level <LEVEL>     Maximum evaluator level
    -p, --target-progress <P>   Target progress guarantee (default: lock-free)
    --quick                     Quick mode (fewer attempts, fast cascade)
    --thorough                  Thorough mode (more attempts, full cascade)
    -q, --quiet                 Suppress progress output
    -h, --help                  Show this help message

EVALUATOR LEVELS:
    rustc (0)     - Type checking, lifetime analysis
    miri (1)      - Undefined behavior detection
    loom (2)      - Thread interleaving exploration
    dst (3)       - Deterministic simulation testing
    stateright (4) - Model checking against TLA+ spec
    kani (5)      - Bounded model checking / proofs
    verus (6)     - SMT theorem proving

PROGRESS GUARANTEES (best to worst):
    wait-free (3)       - Every thread completes in bounded steps
    lock-free (2)       - At least one thread makes progress
    obstruction-free (1) - Progress if run in isolation
    blocking (0)        - May block indefinitely

EXAMPLES:
    # Generate from TLA+ spec
    vf-generate --spec specs/lockfree/treiber_stack.tla

    # Quick generation targeting lock-free
    vf-generate --spec specs/lockfree/treiber_stack.tla --quick -p lock-free

    # Thorough verification targeting wait-free
    vf-generate --spec specs/lockfree/treiber_stack.tla --thorough -p wait-free

ENVIRONMENT:
    ANTHROPIC_API_KEY    Required. Your Anthropic API key.
"#
    );
}
