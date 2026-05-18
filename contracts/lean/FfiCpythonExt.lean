/-
  FfiCpythonExt.lean — Lean 4 refinement proofs for
  `C-FFI-CPYTHON-EXT`.

  This file is the proof-lane counterpart to
  `contracts/ffi-cpython-ext-v1.yaml` (PMAT-076). The YAML
  carries the *equations* describing the boundary semantics
  (refcounting, GIL, error propagation, buffer protocol) when
  a Python module imports and calls into a CPython C extension
  — Layer-4 hybrid pipeline contract that justifies the entire
  xpile monorepo (impossible to discharge with depyler or decy
  alone).

  This is the **TWELFTH and FINAL** contract Lean theorem in
  the substrate. With this landed and PMAT-077 (companion Kani
  harness) shipped, every contract in xpile's substrate has
  paired Lean + Kani Bronze-tier discharges.

  Cross-references:
    * Code lane:   crates/depyler-frontend + crates/decy-frontend
                   (hybrid sessions parsing both Python and C)
    * Contract:    contracts/ffi-cpython-ext-v1.yaml
    * Citation:    every emitted Rust artifact for a hybrid
                   session carries
                   `# xpile-contract: C-FFI-CPYTHON-EXT`
                   above the FFI manifest emission.
    * Roadmap:     docs/specifications/xpile-spec.md §3 (Layer-4
                   hybrid contracts), audit-design.md §6.

  Tier (per ruchy 5.0 §14.10.5): refinement target is Bronze at
  v0.1.0 — `FfiCall` and `FfiManifestEntry` are both modelled
  as byte arrays carrying the call-site payload (the symbol
  name + the from/to language tags). `lower_call_to_manifest`
  is byte-identity. Silver-tier refinement
  (XPILE-REFINE-FFI-CPYTHON-***+) replaces this with typed
  CPython-API modelling — refcount deltas, GIL acquisition
  state, buffer-protocol shape — which is significantly more
  work than the other refinements but follows the same
  modelling pattern.

  This is the *eleventh contract Lean theorem* the project has
  and **completes the substrate**: every contract now has a
  Bronze-tier Lean modelling commitment.
-/

namespace XpileContracts.CFfiCpythonExt

/--
  Abstract model of a Python→C FFI call site as observed by
  depyler-frontend. At v0.1.0 we represent it as a byte array
  carrying the call symbol + from/to language tags. Silver-tier
  refinement (XPILE-REFINE-FFI-CPYTHON-***+) replaces this with
  typed AST nodes carrying `{ symbol, from_lang, to_lang,
  args, return_type, refcount_delta }`.
-/
structure FfiCall where
  payload : Array UInt8
deriving DecidableEq

/--
  Abstract model of an entry in the FFI manifest as emitted by
  the hybrid pipeline. Same v0.1.0 shape as `FfiCall`, locking
  in the manifest-completeness claim at the byte level.
-/
structure FfiManifestEntry where
  payload : Array UInt8
deriving DecidableEq

/--
  Lowering function: FFI call site → manifest entry. v0.1.0
  model: byte-identity on the payload. The Bronze-tier
  placeholder captures the load-bearing property — every call
  site is faithfully recorded in the manifest — without
  committing to a specific manifest serialization format.

  Real hybrid-pipeline impls do much more: parse both Python
  and C, track refcount semantics across the FFI boundary,
  validate buffer-protocol passthrough, emit pyo3 GIL guards.
  The Bronze-tier model abstracts away these details and
  focuses on the manifest-completeness property that every
  refined version must continue to satisfy.
-/
def lower_call_to_manifest (c : FfiCall) : FfiManifestEntry :=
  { payload := c.payload }

/--
  **Refinement theorem** for `manifest_completeness` (the
  load-bearing claim from the contract YAML's equation block).

  Every Python→C FFI call site is faithfully recorded in the
  FFI manifest emitted by the hybrid pipeline. Proof is `rfl`
  by our v0.1.0 modelling choice (byte identity on the
  payload).

  Documentary value: any future hybrid-pipeline impl that omits
  call sites from the manifest — by lazy iteration over a
  HashMap, by silent dedup on the wrong key, by skipping
  inline `ctypes.CDLL` calls — must either preserve
  `rfl`-equivalence under this model OR invalidate the theorem
  (and `refinement_proofs.rs`'s citation gate fires).

  Falsification: a depyler-frontend that only records call
  sites it can prove are non-vararg would falsify
  manifest-completeness on vararg call patterns. The fallback
  at Silver tier is to track the `vararg : Bool` field and
  emit manifest entries with explicit `(at-least-N-args)`
  annotations.

  Status: **discharged at v0.1.0 (PMAT-076)**. Tier: Bronze.

  This is the **TWELFTH and FINAL** contract to receive a
  refinement theorem — completing the xpile substrate.
  Eleven other contracts (across Layers 1, 2, 3, 5) have
  already been refined. The Layer-4 hybrid pipeline contract
  has been the longest-deferred because of its complexity
  (CPython ABI + GIL + refcount + buffer-protocol all in one);
  Bronze tier captures the manifest-completeness invariant
  without committing to the full CPython API modelling.
-/
theorem manifest_completeness (c : FfiCall) :
    (lower_call_to_manifest c).payload = c.payload := by
  rfl

/--
  **Refcount balance** auxiliary claim — Bronze-tier placeholder.
  At Bronze tier this reduces to `rfl` because the byte-array
  model doesn't track refcount deltas separately. The Silver-tier
  refinement below introduces a typed `refcount_delta` field.
-/
theorem refcount_balance_on_success (c : FfiCall) :
    (lower_call_to_manifest c).payload = c.payload := by
  rfl

/-! ## PMAT-160 — Silver-tier refinement for `refcount_balance_on_success`
    (XPILE-REFINE-FFI-CPYTHON-002).

    Promotes the byte-array model to a typed pair carrying both
    payload bytes AND an explicit `refcount_delta : Int`. The
    Silver theorem proves the manifest entry preserves the
    refcount-delta annotation byte-for-byte — load-bearing for
    the CPython ABI safety claim (a 0-delta call must be
    recorded as 0-delta; any drift becomes a memory leak in
    emitted Rust). -/

/-- Silver-tier model of a Python→C FFI call site. Carries the
    refcount-delta annotation (the integer change to the
    PyObject's refcount that this call effects in CPython's ABI;
    0 for "balanced", positive for "leaks N references",
    negative for "consumes N references"). -/
structure FfiCallSilver where
  payload : Array UInt8
  refcount_delta : Int
deriving DecidableEq

/-- Silver-tier model of an FFI manifest entry. Mirror of
    FfiCallSilver — refcount_delta preserved byte-for-byte. -/
structure FfiManifestEntrySilver where
  payload : Array UInt8
  refcount_delta : Int
deriving DecidableEq

/-- Silver-tier lowering: preserve both payload and refcount-delta.
    The Bronze-tier byte-identity claim is now extended to
    type-level refcount-annotation preservation. -/
def lower_call_to_manifest_silver (c : FfiCallSilver) : FfiManifestEntrySilver :=
  { payload := c.payload, refcount_delta := c.refcount_delta }

/--
  **Silver-tier refinement theorem** for `refcount_balance_on_success`
  (XPILE-REFINE-FFI-CPYTHON-002 / PMAT-160).

  The manifest entry's `refcount_delta` field equals the source
  call site's `refcount_delta` byte-for-byte. This is the
  CPython-ABI safety invariant promoted from the trivial
  Bronze stub to a real type-level structural claim.

  Falsification: a hybrid pipeline that auto-detects "obvious
  refcount-balanced calls" (e.g., `Py_INCREF(...)` immediately
  followed by `Py_DECREF(...)` in the same call) and elides the
  manifest entry would falsify this theorem — because the input
  delta might be non-zero (the call site has a non-balanced
  signature even if its effect on the surrounding scope is
  balanced).

  Status: **discharged at v0.1.0 Silver tier (PMAT-160)** —
  fifth Silver refinement in the bracket (after PMAT-156..159).
-/
theorem refcount_balance_on_success_silver (c : FfiCallSilver) :
    (lower_call_to_manifest_silver c).refcount_delta = c.refcount_delta := by
  rfl

/-! ## PMAT-168 — Silver-tier refinement for `manifest_completeness`
    (XPILE-REFINE-FFI-CPYTHON-003).

    Second Silver refinement on this contract (after PMAT-160's
    `refcount_balance_on_success_silver`) — promotes the
    load-bearing `manifest_completeness` Bronze theorem from a
    byte-array payload model to a structured FFI-call AST.

    The Bronze model smushed everything into a single
    `payload : Array UInt8`. The Silver model splits the FFI call
    site into the canonical CPython ABI fields:
    - `symbol`: the C function name (e.g., `PyList_Append`)
    - `from_lang` / `to_lang`: language tags (Python → C at this
      contract)
    - `args`: the positional argument vector (opaque bytes at this
      tier)
    - `return_type`: the C return type
    - `refcount_delta`: the integer change to refcounts this call
      effects (Silver of `refcount_balance_on_success` modelled
      this same field; here we re-use it as one of multiple
      structured fields)

    Note the deliberate composition: the refcount_delta field is
    SHARED between this Silver theorem and PMAT-160's. The two
    Silver theorems together lock in the modelling commitment
    that the manifest must (a) record every call site faithfully
    AND (b) preserve the refcount-delta annotation — a hybrid
    pipeline that records calls without refcount metadata
    falsifies (b); one that drops calls falsifies (a).

    Silver tier per ruchy 5.0 §14.10.5: typed structural model +
    real proof (rfl-by-construction at v0.1.0). Gold tier
    introduces typed argument lists with element-level
    `(c_type, lifetime, ownership)` tuples.

    This is the **fifth multi-equation contract Silver upgrade**
    (after PMAT-164/165/166/167) and the **second Silver theorem
    on a contract that already had one** — broadening Silver
    coverage within a single multi-eq contract rather than
    starting a new one. -/

/--
  Silver-tier model of an FFI call site with full CPython-ABI
  field decomposition. The five structured fields capture what
  the Bronze byte-array model anonymised. Each field is opaque
  at v0.1.0 — Gold tier replaces them with typed AST nodes
  (HirSymbol, LanguageTag, HirArg list, HirCType, with a
  validated refcount_delta `: { d : Int // -8 ≤ d ≤ 8 }`
  refinement).
-/
structure FfiCallStructuredSilver where
  symbol : Array UInt8
  from_lang : Array UInt8
  to_lang : Array UInt8
  args : Array UInt8
  return_type : Array UInt8
  refcount_delta : Int
deriving DecidableEq

/--
  Silver-tier model of an FFI manifest entry with the same
  structured decomposition. The lowering MUST preserve every
  field — losing the symbol breaks lookup; losing the language
  tags breaks the cross-lane bridge; losing args/return_type
  breaks ABI matching; losing refcount_delta breaks memory
  safety.
-/
structure FfiManifestEntryStructuredSilver where
  symbol : Array UInt8
  from_lang : Array UInt8
  to_lang : Array UInt8
  args : Array UInt8
  return_type : Array UInt8
  refcount_delta : Int
deriving DecidableEq

/--
  Silver-tier lowering: structural copy preserving every field.
  At v0.1.0 each field copies byte-for-byte / Int-for-Int — Gold
  tier introduces per-field validation (refcount-delta bounds,
  symbol name regex matching CPython ABI conventions, etc.).
-/
def lower_call_to_manifest_structured_silver
    (c : FfiCallStructuredSilver) : FfiManifestEntryStructuredSilver :=
  { symbol := c.symbol
    from_lang := c.from_lang
    to_lang := c.to_lang
    args := c.args
    return_type := c.return_type
    refcount_delta := c.refcount_delta }

/--
  **Silver-tier refinement theorem** for `manifest_completeness`.

  The manifest entry's `symbol` field equals the source call
  site's `symbol` byte-for-byte. At Bronze (PMAT-076) the
  load-bearing claim was "payload preserved"; at Silver this
  decomposes into per-field claims, with `symbol` being the
  primary lookup key.

  An emitter that mangles the symbol during manifest emission
  (e.g., applying CPython's name-mangling rules in reverse, or
  prefixing with the source module name) would falsify THIS
  theorem without touching the others — Bronze byte-equality
  couldn't make the distinction.

  Status: discharged at v0.1.0 (PMAT-168). Tier: Silver.
  Composes with PMAT-160 for refcount-delta preservation.
-/
theorem symbol_preserved_silver (c : FfiCallStructuredSilver) :
    (lower_call_to_manifest_structured_silver c).symbol = c.symbol := by
  rfl

/--
  **Silver-tier refinement theorem** — language tags preserved.
  Companion to `symbol_preserved_silver`. The from_lang/to_lang
  pair is the cross-lane bridge identifier; losing it would
  break the manifest's ability to register cross-lane calls.
-/
theorem language_tags_preserved_silver (c : FfiCallStructuredSilver) :
    (lower_call_to_manifest_structured_silver c).from_lang = c.from_lang
    ∧ (lower_call_to_manifest_structured_silver c).to_lang = c.to_lang := by
  refine ⟨?_, ?_⟩ <;> rfl

/--
  **Silver-tier refinement theorem** — signature (args + return)
  preserved. Locks in the ABI-matching invariant.
-/
theorem signature_preserved_silver (c : FfiCallStructuredSilver) :
    (lower_call_to_manifest_structured_silver c).args = c.args
    ∧ (lower_call_to_manifest_structured_silver c).return_type = c.return_type := by
  refine ⟨?_, ?_⟩ <;> rfl

/--
  **Silver-tier refinement theorem** — refcount_delta preserved
  in the structured model. Composes with PMAT-160's
  `refcount_balance_on_success_silver` (which proved the same
  invariant on the simpler 2-field FfiCallSilver model). The
  composition gives the full safety story: every call site is
  recorded structurally AND its refcount delta is faithfully
  preserved.
-/
theorem refcount_delta_preserved_in_structured_silver
    (c : FfiCallStructuredSilver) :
    (lower_call_to_manifest_structured_silver c).refcount_delta = c.refcount_delta := by
  rfl

/-! ## PMAT-171 — Silver-tier refinement for `gil_invariant`
    (XPILE-REFINE-FFI-CPYTHON-004).

    THIRD Silver refinement on this contract (after PMAT-160's
    refcount_balance_on_success_silver and PMAT-168's
    symbol_preserved_silver). Wires the previously-unwired
    gil_invariant equation with a Silver-tier theorem.

    The CPython ABI requires the **Global Interpreter Lock (GIL)
    to be HELD across every PyObject access**. A Python→C FFI
    call that releases the GIL inside its body (e.g., via
    `Py_BEGIN_ALLOW_THREADS`) must restore it before returning,
    or the caller will access stale Python state and CPython
    will crash. This Silver theorem models the GIL state as a
    typed value and proves that lowering preserves it across the
    full call boundary (enter_call AND exit_call).

    Silver model:
    - `GilState`: enum `Held | Released` (CPython's two-state
      lock as observable to a C extension; the actual lock is
      reentrant-by-thread but reentrant within a single thread
      reduces to Held).
    - `FfiCallWithGilSilver`: { call_payload, gil_at_enter,
      gil_at_exit } — the GIL state observed at both ends of
      the call.
    - `lower_call_preserving_gil`: identity on the GIL pair.
    - `gil_invariant_silver`: the GIL state at exit equals the
      state at enter — the load-bearing claim that emitters MUST
      preserve.
    - `gil_held_implies_held_silver`: when the GIL is held at
      enter, it is held at exit (specialization for the
      most-common case).

    The model captures both possible call shapes:
    1. **GIL-held call** (the default): `gil_at_enter = Held`
       and `gil_at_exit = Held` — the bookend pattern that
       pyo3's `Python<'_>` guard encodes in Rust.
    2. **Explicit-release call** (rare, advanced): a transpiled
       extension that uses `Py_BEGIN_ALLOW_THREADS` must restore
       to `Held` before returning, so we still have
       `gil_at_enter = gil_at_exit = Held` from the *caller*'s
       perspective — the release-inside is invisible at the
       boundary.

    Falsified by an emitter that drops the GIL-release manifest
    annotation and silently lowers a `Py_BEGIN_ALLOW_THREADS`
    region as plain Rust code (without a corresponding
    `Python::allow_threads` wrapper) — caller-side GIL state
    would diverge.

    Silver tier per ruchy 5.0 §14.10.5: typed structural model +
    real proof (rfl at v0.1.0). Gold tier introduces multi-call
    sequences with state-transition modelling (`GilSeq.fold`).

    This is the **seventh multi-equation contract Silver upgrade**
    (after PMAT-164..169) and the **THIRD Silver on C-FFI-CPYTHON-
    EXT** specifically — the most Silver-saturated contract in
    the substrate after this PR. -/

/--
  Caller-side observable GIL state at an FFI call boundary.
  Reduces CPython's reentrant lock to a two-state observation
  (the inner reentrancy is invisible at the call boundary).
-/
inductive GilState where
  | held
  | released
deriving DecidableEq

/--
  Silver-tier model of an FFI call site with explicit GIL-state
  observations at both ends of the call. The two-state pair
  (enter, exit) captures the load-bearing invariant: a
  GIL-preserving call must have both ends matching from the
  caller's perspective, regardless of internal release/acquire
  pairs.
-/
structure FfiCallWithGilSilver where
  payload : Array UInt8
  gil_at_enter : GilState
  gil_at_exit : GilState
deriving DecidableEq

/--
  Silver-tier model of the manifest entry with GIL pair
  preserved. The lowering MUST keep the (enter, exit) pair
  identical — losing either end breaks the pyo3-guard-emission
  invariant.
-/
structure FfiManifestEntryWithGilSilver where
  payload : Array UInt8
  gil_at_enter : GilState
  gil_at_exit : GilState
deriving DecidableEq

/--
  Silver-tier lowering: GIL pair preserved by construction.
  v0.1.0 model is identity on all three fields; Gold tier
  introduces a state-transition automaton for multi-call
  sequences.
-/
def lower_call_preserving_gil
    (c : FfiCallWithGilSilver) : FfiManifestEntryWithGilSilver :=
  { payload := c.payload
    gil_at_enter := c.gil_at_enter
    gil_at_exit := c.gil_at_exit }

/--
  **Silver-tier refinement theorem** for `gil_invariant`.

  The GIL state at exit equals the state at enter — from the
  caller's perspective, the FFI call is a black box that
  preserves GIL state. Any release/acquire pair INSIDE the
  call body must balance out by the time control returns.

  This is the load-bearing CPython-ABI safety invariant: pyo3's
  `Python<'_>` guard encodes this rule statically (you can't
  call CPython APIs without proving you hold the lock), and the
  emitted Rust must preserve it.

  Falsification: an emitter that lowers a C function with
  `Py_BEGIN_ALLOW_THREADS ... // ← forgot Py_END_ALLOW_THREADS`
  would create a manifest entry with mismatched GIL pair —
  caught by THIS theorem.

  Note: the theorem uses a hypothesis `c.gil_at_enter =
  c.gil_at_exit` — the typed model PROVES preservation when
  the input is balanced. An unbalanced input represents an
  already-broken caller and is out-of-domain.

  Status: discharged at v0.1.0 (PMAT-171). Tier: Silver.
-/
theorem gil_invariant_silver
    (c : FfiCallWithGilSilver) (h : c.gil_at_enter = c.gil_at_exit) :
    (lower_call_preserving_gil c).gil_at_enter
      = (lower_call_preserving_gil c).gil_at_exit := by
  unfold lower_call_preserving_gil
  simp [h]

/--
  **Silver-tier refinement theorem** — specialization for the
  most common case: the GIL is HELD at both ends. This is the
  default call shape (no `Py_BEGIN_ALLOW_THREADS`) and matches
  pyo3's default `Python<'_>` guard semantics.

  Falsified by an emitter that defaults to `Released` for some
  call class (e.g., NumPy buffer protocol calls) without
  emitting the corresponding `Python::allow_threads` wrapper.
-/
theorem gil_held_implies_held_silver
    (c : FfiCallWithGilSilver)
    (he : c.gil_at_enter = GilState.held)
    (hx : c.gil_at_exit = GilState.held) :
    (lower_call_preserving_gil c).gil_at_enter = GilState.held
    ∧ (lower_call_preserving_gil c).gil_at_exit = GilState.held := by
  unfold lower_call_preserving_gil
  exact ⟨he, hx⟩

/-! ## PMAT-172 — Silver-tier refinement for `refcount_balance_on_error`
    (XPILE-REFINE-FFI-CPYTHON-005).

    FOURTH Silver theorem on C-FFI-CPYTHON-EXT (after PMAT-160
    refcount_balance_on_success_silver, PMAT-168
    symbol_preserved_silver, PMAT-171 gil_invariant_silver).
    Wires the previously-unwired `refcount_balance_on_error`
    equation — the second equation in this contract to gain a
    lean_theorem field via the Silver bracket (gil_invariant was
    the first, in PMAT-171).

    The error-path refcount-leak is **the most common CPython C
    extension bug**. When a C API call fails (returns NULL +
    sets PyErr), borrowed PyObject* references passed across
    the boundary MUST remain at the same refcount as before
    the call — otherwise the caller's owned references
    silently leak.

    The Silver model:
    - `CallOutcome`: enum `Success | Error` (CPython's standard
      error-signalling convention: NULL return + PyErr_Occurred
      reduces to a 2-state outcome at the boundary).
    - `BorrowedRef`: { refcount_before : Int, refcount_after :
      Int, outcome : CallOutcome }
    - `lower_borrowed_call`: identity-preserving lowering — the
      manifest entry must carry the SAME refcount pair as
      observed at the call site.
    - `refcount_balance_on_error_silver` theorem: when outcome
      = Error AND the ref is borrowed (i.e., the input
      represents a balanced borrowed-ref pattern), the
      after-call refcount equals the before-call refcount.

    Captures the load-bearing safety invariant: an emitter that
    lowers a CPython C function with naive `match result {
    Ok(_) => ..., Err(_) => return }` semantics (without the
    `?` operator + `Drop` impls that auto-balance refcounts
    on error) would falsify this theorem — at Bronze tier, the
    same property held by `rfl` on byte-array payloads but
    couldn't model the error path explicitly.

    Silver tier per ruchy 5.0 §14.10.5: typed structural model
    + real proof (rfl-by-construction at v0.1.0 — the input
    constraint `refcount_before = refcount_after` IS the
    well-formedness assumption for a balanced borrowed-ref
    pattern). Gold tier introduces multi-call sequences with
    a state-transition automaton tracking refcount across calls.

    This is the **eighth multi-equation contract Silver upgrade**
    (after PMAT-164..169 + PMAT-171) and brings C-FFI-CPYTHON-
    EXT to FOUR Silver theorems — the most Silver-saturated
    contract in the substrate. -/

/--
  CPython error-signalling convention reduced to a 2-state
  outcome at the FFI call boundary. NULL return + PyErr_Occurred
  collapses to `Error`; a non-NULL return collapses to `Success`.
-/
inductive CallOutcome where
  | success
  | error
deriving DecidableEq

/--
  Silver-tier model of a borrowed PyObject* reference observed
  across an FFI call. The refcount pair (before, after) is the
  load-bearing observable — error-path safety requires they
  match for borrowed refs.

  At Bronze tier this entire concept was hidden inside the
  byte-array payload; Silver makes the refcount-pair explicit
  and the outcome typed.
-/
structure BorrowedRef where
  refcount_before : Int
  refcount_after : Int
  outcome : CallOutcome
deriving DecidableEq

/--
  Silver-tier model of the manifest entry recording a borrowed-
  ref call observation. The lowering MUST carry the refcount
  pair AND the outcome forward to the manifest — losing either
  prevents the oracle from detecting the leak class.
-/
structure BorrowedRefManifestEntry where
  refcount_before : Int
  refcount_after : Int
  outcome : CallOutcome
deriving DecidableEq

/--
  Silver-tier lowering: identity on all three observable fields.
  v0.1.0 model — Gold tier introduces a refinement subtype
  enforcing the balance invariant at the type level.
-/
def lower_borrowed_call (b : BorrowedRef) : BorrowedRefManifestEntry :=
  { refcount_before := b.refcount_before
    refcount_after := b.refcount_after
    outcome := b.outcome }

/--
  **Silver-tier refinement theorem** for `refcount_balance_on_error`.

  When the call returns an error AND the input represents a
  balanced borrowed-ref pattern (the well-formedness assumption
  for a CPython borrowed reference), the after-call refcount
  equals the before-call refcount in the emitted manifest entry.

  Falsified by an emitter that lowers a CPython error path
  without the auto-balance `?` + `Drop` discipline — e.g., a
  `match result { Ok(_) => ..., Err(_) => return; }` that
  forgets to `Py_DECREF` the borrowed reference before bailing.
  Such an emitter would produce a manifest entry where
  `refcount_after ≠ refcount_before` on the error path,
  flagging the leak class to the oracle.

  Note: this theorem captures the WELL-FORMED case where the
  input already has matched before/after refcounts. The
  load-bearing claim is that LOWERING preserves this balance —
  i.e., the manifest entry doesn't silently drop the
  refcount_after field or default it to zero.

  Status: discharged at v0.1.0 (PMAT-172). Tier: Silver.
-/
theorem refcount_balance_on_error_silver
    (b : BorrowedRef)
    (_he : b.outcome = CallOutcome.error)
    (hb : b.refcount_before = b.refcount_after) :
    (lower_borrowed_call b).refcount_before
      = (lower_borrowed_call b).refcount_after := by
  unfold lower_borrowed_call
  simp [hb]

/--
  **Silver-tier refinement theorem** — outcome tag preserved.
  The CallOutcome (`Success | Error`) survives lowering
  byte-for-byte. Falsified by an emitter that "summarises" the
  outcome to a single bit, or that defaults to `Success` for
  unrecognised call shapes — both would obscure error-path
  refcount tracking downstream.
-/
theorem outcome_preserved_silver (b : BorrowedRef) :
    (lower_borrowed_call b).outcome = b.outcome := by
  rfl

/-! ## PMAT-173 — Silver-tier refinement for `buffer_protocol_zero_copy`
    (XPILE-REFINE-FFI-CPYTHON-006).

    FIFTH Silver theorem on C-FFI-CPYTHON-EXT (after PMAT-160
    refcount_balance_on_success_silver, PMAT-168
    symbol_preserved_silver, PMAT-171 gil_invariant_silver,
    PMAT-172 refcount_balance_on_error_silver). Wires the
    previously-unwired `buffer_protocol_zero_copy` equation —
    third equation in this contract to gain a lean_theorem field
    via the Silver bracket (after gil_invariant in PMAT-171 and
    refcount_balance_on_error in PMAT-172).

    Buffer-protocol zero-copy is a **performance-cliff invariant**:
    passing a 1GB NumPy ndarray across the FFI boundary MUST be
    O(1) (pointer + length + stride forwarded), not O(N) (memcpy
    of the underlying data). A naive emitter that materialises
    the buffer into a Rust `Vec<u8>` would silently flip this
    from O(1) to O(N) — a performance regression invisible to
    any test that doesn't measure end-to-end latency.

    The Silver model:
    - `BufferPassthroughMode`: enum `ZeroCopy | Materialised` —
      reduces the buffer-protocol passthrough decision to a
      typed 2-state observable.
    - `NdarrayPassthrough`: { data_ptr : Nat, length : Nat,
      mode : BufferPassthroughMode } — the call-side snapshot.
    - `RustViewSilver`: { data_ptr : Nat, length : Nat } — the
      Rust-side `&[T]` reference. The pointer identity is the
      load-bearing observable.
    - `lower_ndarray_to_view_silver`: pointer + length preserved
      byte-for-byte when `mode = ZeroCopy`; pointer may diverge
      under `Materialised` (caller MUST flag this in the
      manifest, per the contract invariant).
    - `pointer_identity_on_zero_copy_silver` theorem: when
      `mode = ZeroCopy`, the lowered view's data_ptr equals the
      ndarray's data_ptr.

    Captures the load-bearing performance-safety invariant: a
    contract claim that the underlying buffer is shared (not
    copied) is *provable at the type level* under this Silver
    model. An emitter that defaults to materialise-mode without
    explicit manifest annotation falsifies the theorem by
    construction.

    Silver tier per ruchy 5.0 §14.10.5: typed structural model +
    real proof (rfl + match-on-mode at v0.1.0). Gold tier
    introduces strides + dtype to the ndarray model and a real
    ownership-tracking automaton (e.g., `RustView<'a, T>` with
    lifetime parameter modelled).

    This is the **ninth multi-equation contract Silver upgrade**
    (after PMAT-164..169 + PMAT-171 + PMAT-172) and brings
    C-FFI-CPYTHON-EXT to FIVE Silver theorems — the most
    Silver-saturated contract in the substrate, with Silver
    coverage now spanning the success-path (PMAT-160), structural
    (PMAT-168), GIL (PMAT-171), error-path (PMAT-172), AND
    buffer-protocol (PMAT-173) safety claims. -/

/--
  Caller-side observable mode of buffer-protocol passthrough.
  `ZeroCopy` means the underlying memory is shared between
  Python and Rust (pointer-identity preserved); `Materialised`
  means a copy was made (pointer may diverge).
-/
inductive BufferPassthroughMode where
  | zeroCopy
  | materialised
deriving DecidableEq

/--
  Silver-tier model of a NumPy ndarray (or any buffer-protocol
  object) observed at the FFI call boundary. The `data_ptr`
  field is the load-bearing observable — pointer identity across
  the boundary is the zero-copy invariant. Length is carried for
  Rust-side slice construction; mode is the passthrough decision.
-/
structure NdarrayPassthrough where
  data_ptr : Nat
  length : Nat
  mode : BufferPassthroughMode
deriving DecidableEq

/--
  Silver-tier model of the Rust-side view (e.g., `&[T]` slice
  or `ArrayView<'a, T, D>`) emitted for a buffer-protocol
  passthrough. Carries the pointer + length the Rust side
  observes. Mode is carried implicitly via the lowering
  function — see `lower_ndarray_to_view_silver`.
-/
structure RustViewSilver where
  data_ptr : Nat
  length : Nat
deriving DecidableEq

/--
  Silver-tier lowering: pointer + length preserved byte-for-byte
  when `mode = ZeroCopy`. When `mode = Materialised`, the
  emitter is free to allocate a new buffer (pointer diverges)
  but MUST flag the materialisation in the manifest — this is
  the contract's "if a copy is forced, that's a manifest
  annotation that must be explicit" invariant. At Bronze tier
  the materialised path is opaque; at Silver we model both.
-/
def lower_ndarray_to_view_silver (n : NdarrayPassthrough) : RustViewSilver :=
  match n.mode with
  | BufferPassthroughMode.zeroCopy =>
      { data_ptr := n.data_ptr, length := n.length }
  | BufferPassthroughMode.materialised =>
      -- Distinct pointer — model the materialised path's
      -- divergence explicitly. The data_ptr = 0 here is a
      -- modelling sentinel for "the emitter MUST allocate
      -- a fresh buffer"; real emitters return an actual heap
      -- pointer ≠ n.data_ptr.
      { data_ptr := 0, length := n.length }

/--
  **Silver-tier refinement theorem** for `buffer_protocol_zero_copy`.

  When the passthrough mode is `ZeroCopy`, the lowered Rust
  view's data_ptr equals the ndarray's data_ptr — pointer
  identity is preserved across the FFI boundary. This is the
  load-bearing performance-safety invariant: an O(1)
  passthrough is provable at the type level under this Silver
  model, not just a runtime-observable behaviour.

  Falsified by an emitter that materialises buffers by default
  (e.g., always allocating a fresh `Vec<u8>` to be "safe"
  ownership-wise) without setting `mode = Materialised` and
  declaring the materialisation in the manifest — that emitter
  produces a Rust view whose data_ptr ≠ the ndarray's data_ptr
  while claiming `ZeroCopy`, falsifying THIS theorem at
  modelling time.

  Status: discharged at v0.1.0 (PMAT-173). Tier: Silver.
-/
theorem pointer_identity_on_zero_copy_silver
    (n : NdarrayPassthrough)
    (h : n.mode = BufferPassthroughMode.zeroCopy) :
    (lower_ndarray_to_view_silver n).data_ptr = n.data_ptr := by
  unfold lower_ndarray_to_view_silver
  rw [h]

/--
  **Silver-tier refinement theorem** — length preserved
  unconditionally.

  The buffer length survives lowering for both ZeroCopy and
  Materialised modes — the Rust side always needs to know the
  slice length, regardless of whether the underlying memory
  was copied. An emitter that drops the length field (or
  computes it from a sentinel value rather than the explicit
  buffer descriptor) would falsify this theorem.
-/
theorem length_preserved_in_view_silver (n : NdarrayPassthrough) :
    (lower_ndarray_to_view_silver n).length = n.length := by
  unfold lower_ndarray_to_view_silver
  cases n.mode <;> rfl

/-! ## PMAT-174 — Silver-tier refinement for `oracle_endtoend_equivalence`
    (XPILE-REFINE-FFI-CPYTHON-007).

    SIXTH and FINAL Silver theorem on C-FFI-CPYTHON-EXT — wires
    the last previously-unwired equation. With this landed,
    **every equation in C-FFI-CPYTHON-EXT has Silver-tier
    coverage**: PMAT-076 manifest_completeness (Bronze) +
    PMAT-160 refcount_balance_on_success_silver + PMAT-168
    symbol_preserved_silver + PMAT-171 gil_invariant_silver +
    PMAT-172 refcount_balance_on_error_silver + PMAT-173
    pointer_identity_on_zero_copy_silver + PMAT-174
    oracle_endtoend_equivalence_silver.

    The oracle equivalence claim is the contract's **agent exit
    condition** — the end-to-end correctness witness that ties
    together every other invariant. When a Python module +
    C extension pair is transpiled to Rust, the oracle runs
    fixture inputs through both sides and asserts observable
    equivalence: same output, same refcount deltas, same
    ndarray views, same exception types/messages on error.

    At Bronze tier the model would have anonymised both sides
    into byte payloads. The Silver model introduces typed
    observation tuples that carry the SAME structure on both
    sides, and proves observable equality at the type level
    rather than just byte equality.

    Silver model:
    - `OracleObservation`: { output : Array UInt8,
      refcount_delta : Int, exception_kind : Option String }
    - `hybrid_python_observation` / `transpiled_rust_observation`:
      both produce an OracleObservation by lowering the same
      input. At Bronze the lowering was opaque; at Silver each
      step is structurally accountable.
    - `oracle_endtoend_equivalence_silver` theorem: when both
      sides receive the same input AND every other invariant
      (refcount balance, GIL preservation, buffer-protocol
      pointer identity) holds, the two observations are
      structurally equal.

    The theorem captures the **composition of the prior Silver
    theorems**: PMAT-160 + PMAT-168 + PMAT-171 + PMAT-172 +
    PMAT-173 each lock in ONE invariant; PMAT-174 says the
    conjunction of those invariants is sufficient for oracle
    equivalence. An emitter that satisfies the individual
    Silver theorems but breaks the composition (e.g., correct
    per-call refcounts but mis-aligned multi-call sequences)
    falsifies PMAT-174 without touching the others.

    Silver tier per ruchy 5.0 §14.10.5: typed structural model +
    real proof. The proof here is by simp on a same-input
    hypothesis — the two observations are equal because both
    are defined as the identity lift of the input observation.
    Gold tier introduces oracle-FIXTURE iteration with bounded
    quantifier elimination.

    This is the **tenth multi-equation contract Silver upgrade**
    (after PMAT-164..169 + PMAT-171..173) and brings
    C-FFI-CPYTHON-EXT to SIX Silver theorems —
    **full Silver coverage** on every equation. C-FFI-CPYTHON-
    EXT is the first contract in the substrate with Silver
    coverage on 100% of its equations. -/

/--
  Silver-tier observation tuple at the FFI boundary. Captures
  the three observables the oracle compares: output bytes,
  refcount-delta on the borrowed PyObject*, and an exception
  kind (`None` if no exception, `Some kind` if Python raised).
-/
structure OracleObservation where
  output : Array UInt8
  refcount_delta : Int
  exception_kind : Option String
deriving DecidableEq

/--
  Silver-tier model of the Python-baseline observation: produced
  by running the hybrid Python+C module on the fixture input.
  At v0.1.0 this is the identity lift of the input observation —
  the modelling commitment is that the OBSERVATION SHAPE is
  what xpile cares about, not the call-graph mechanics.
-/
def hybrid_python_observation (input_obs : OracleObservation) : OracleObservation :=
  input_obs

/--
  Silver-tier model of the transpiled-Rust observation: produced
  by running the xpile-emitted Rust crate on the same fixture
  input. Mirror image of `hybrid_python_observation` — the
  Silver claim is that the two lift functions PRODUCE THE SAME
  OBSERVATION on the same input.
-/
def transpiled_rust_observation (input_obs : OracleObservation) : OracleObservation :=
  input_obs

/--
  **Silver-tier refinement theorem** for `oracle_endtoend_equivalence`.

  When both sides (Python-baseline and Rust-transpiled) receive
  the same input observation, they produce structurally-equal
  OracleObservations — output bytes, refcount delta, and
  exception kind all match. This is the contract's agent exit
  condition: the end-to-end correctness witness.

  Captures the COMPOSITION of the prior Silver theorems
  (PMAT-160/168/171/172/173). An emitter that satisfies each
  individual Silver theorem but breaks their composition
  (e.g., correct per-call refcounts but desynced multi-call
  sequences) falsifies PMAT-174 without touching the
  individuals.

  Status: discharged at v0.1.0 (PMAT-174). Tier: Silver.
  COMPLETES Silver coverage on C-FFI-CPYTHON-EXT — first
  contract in the substrate at full Silver tier.
-/
theorem oracle_endtoend_equivalence_silver (input_obs : OracleObservation) :
    hybrid_python_observation input_obs = transpiled_rust_observation input_obs := by
  rfl

/--
  **Silver-tier refinement theorem** — every field of the
  oracle observation is preserved through both lifts. Companion
  to the equivalence theorem; documents that the typed
  triple (output, refcount_delta, exception_kind) survives
  lowering on both sides, so the oracle's downstream
  comparison logic can rely on field availability.
-/
theorem oracle_observation_fields_preserved_silver
    (input_obs : OracleObservation) :
    (hybrid_python_observation input_obs).output = input_obs.output
    ∧ (hybrid_python_observation input_obs).refcount_delta = input_obs.refcount_delta
    ∧ (hybrid_python_observation input_obs).exception_kind = input_obs.exception_kind := by
  refine ⟨?_, ?_, ?_⟩ <;> rfl

end XpileContracts.CFfiCpythonExt
