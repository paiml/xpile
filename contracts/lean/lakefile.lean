import Lake
open Lake DSL

/-!
  `xpile` proof-lane build — PMAT-903 (Sprint Day 4).

  Turns the proof lane from a `grep sorry` claim into a machine-checked one:
  `lake build` elaborates the PILOT set below against a pinned Lean toolchain
  (`lean-toolchain` → v4.15.0), wired as a SEPARATE ADVISORY CI job (never the
  blocking publish gate — mirrors the kani job posture, so Lean weight cannot
  stall the fast gate or the crates.io window).

  Core-only, no Mathlib: PMAT-903 verified that NOT ONE `contracts/lean/*.lean`
  module carries an `import Mathlib` (or any `import` at all) — the "Mathlib"
  mentions are docstring references to lemma names, not real dependencies. So
  the pilot needs no Mathlib cache and the job stays fast and hermetic.

  Truth-up (PMAT-903): the long-quoted "6 `sorry` across 5 files" debt was a
  naive-`grep` artifact — those hits are docstring prose ("…genuinely provable,
  sorry-free…") plus a `ProofStubReason` inductive constructor literally named
  `sorry` in `Notation.lean`. There is ZERO use of the `sorry` tactic and ZERO
  `axiom` declarations in the whole tree (all "axiom" hits are prose). With
  `warningAsError := true` below, a real `sorry` could not survive a green build
  — so this job is exactly the check that makes "provable" un-falsifiable by
  `grep sorry`.

  PILOT = the 23 modules that elaborate clean under bare core with
  warnings-as-errors (9 from PMAT-903 + 2 PMAT-904 + FfiShellSubprocess/907 +
  CFloatArith/912 + XpileFrontendTrait/913 + XlateRustFnToLeanThm/914 +
  XlateLeanToRust/915 + Notation/916 + Bashrs/928 + XlatePyBoolToRustBool/935 +
  XlatePyListToVec/936 + FfiCpythonExt/937 + CompileRustToPtxMma/938 +
  PyIntArith/948 — the CAPSTONE: the LAST non-elaborating module).
  The known-incomplete remainder is now ZERO — the ENTIRE
  `contracts/lean/` substrate is machine-checked by this advisory job.
  (Historical remainder enumeration retained in PROVABILITY-INVENTORY.md.)
  honestly in `PROVABILITY-INVENTORY.md` and deliberately EXCLUDED here so this
  advisory job is GREEN without overstating what is proven. Discharging the
  remaining errors and re-adding files is ongoing Day 5+ work.
-/

package «xpileContracts» where
  -- Treat `sorry` as an error inside the pilot: the whole point is that the
  -- pilot set is genuinely hole-free. Anything that regresses to a `sorry`
  -- breaks `lake build`, not just emits a warning.
  leanOptions := #[⟨`warningAsError, true⟩]

@[default_target]
lean_lib «XpileContractsPilot» where
  srcDir := "."
  -- Each module is fully standalone (zero inter-module imports), so listing a
  -- module as a root builds exactly it and nothing else. Keep this list in
  -- lockstep with PROVABILITY-INVENTORY.md's "pilot (machine-checked)" table.
  roots := #[
    `CIntArith,                  -- C-C-INT-ARITH
    `PyFloatArith,               -- C-PY-FLOAT-ARITH            (PMAT-903 nested-comment fix)
    `XlatePyDictToHashmap,       -- C-XLATE-PY-DICT-TO-HASHMAP
    `XlatePyStrToRustString,     -- C-XLATE-PY-STR-TO-RUST-STRING
    `XpileContractBackendTrait,  -- C-XPILE-CONTRACT-BACKEND
    `XlatePyClassToStruct,       -- C-XLATE-PY-CLASS-TO-STRUCT   (was mis-flagged "sorry")
    `XlatePyOptionalToOption,    -- C-XLATE-PY-OPTIONAL-TO-OPTION(was mis-flagged "sorry")
    `XlatePySetToHashset,        -- C-XLATE-PY-SET-TO-HASHSET    (was mis-flagged "sorry")
    `XlatePyTupleToRustTuple,    -- C-XLATE-PY-TUPLE-TO-RUST-TUPLE (was mis-flagged "sorry")
    -- PMAT-904 (Sprint Day 5): discharged the two cheapest elaboration-error
    -- files. XpileBackendTrait: Mathlib-only `tauto` → core `decide` over the
    -- decidable enum cases. XpileContractFrontendTrait: derived `Inhabited
    -- EquationsBlock` for `[0]!`, and re-proved `frame_safety_transitive_platinum`
    -- as a defeq `calc` (`rw` can't see through the `before`/`after` defs).
    `XpileBackendTrait,          -- C-XPILE-BACKEND-TRAIT        (PMAT-904: tauto→decide)
    `XpileContractFrontendTrait, -- C-XPILE-CONTRACT-FRONTEND    (PMAT-904: Inhabited + calc)
    -- PMAT-907 (Sprint Day 8): the new Shell-subprocess FFI contract joins the
    -- pilot at depth-1 — a core-only, import-free STRUCTURE EXTENSIONALITY proof
    -- (same shape as PyFloatArith), machine-checked here so the contract's
    -- depth-1 Diamond is not a string claim.
    `FfiShellSubprocess,         -- C-FFI-SHELL-SUBPROCESS       (PMAT-907)
    -- PMAT-912 (backlog slice): the new C-float arithmetic contract joins the
    -- pilot at depth-1 — core-only STRUCTURE EXTENSIONALITY with TWO bit-width
    -- models (binary32 c_float + binary64 c_double) + an ABI-distinctness lemma,
    -- same shape as PyFloatArith. Discharges the citation PMAT-910/911 deferred.
    `CFloatArith,                -- C-C-FLOAT-ARITH              (PMAT-912)
    -- PMAT-913 (backlog slice): discharged the cheapest non-elaborating
    -- module — NOT a termination error but the PMAT-904 class: (a) clause (c)
    -- of `parse_and_lower_function_diamond` lacked parens, so its `∀ f'`
    -- right-extended and swallowed clause (d) (the 4-way `refine ⟨…⟩` then hit
    -- a `∀`, not an inductive); (b) `source_lang_enum_completeness_diamond`
    -- used Mathlib-only `tauto` over the decidable `SourceLang` disjunction →
    -- core `decide`. Layer-3 frontend trait now machine-checked, not excluded.
    `XpileFrontendTrait,          -- C-XPILE-FRONTEND-TRAIT       (PMAT-913)
    -- PMAT-914 (backlog slice): discharged the cheapest termination-led head.
    -- The fault was NOT a genuine missing termination argument but a NAME
    -- SHADOWING bug: `def NonEmptyPreconditionList.val (n) := n.val` resolved
    -- `n.val` by dot-notation to *itself* (a non-terminating recursive call,
    -- `n` unchanged), and that broken `.val` then poisoned every downstream
    -- `n.val`, cascading into the `n.property` and `Subtype.ext` type
    -- mismatches. Fix = use the positional `.1` Subtype projection in the body
    -- (`n.1`), breaking the self-reference; all three errors clear at once.
    `XlateRustFnToLeanThm,         -- C-XLATE-RUST-FN-TO-LEAN-THM (PMAT-914)
    -- PMAT-915 (backlog slice): discharged the next cheapest termination-led
    -- head — again NOT a genuine termination argument but the PMAT-914 NAME
    -- SHADOWING class: `def WarningLineCount.val (w) := w.val` resolved `w.val`
    -- by dot-notation to *itself* (a non-terminating self-call, `w` unchanged),
    -- poisoning the derived `DecidableEq`, the `.property` refinement theorems,
    -- and the `Subtype.ext` extensionality proof. Two-part fix: (a) positional
    -- `.1` Subtype projection in the body breaks the self-reference (clears the
    -- termination + `.property` + `Subtype.ext` errors at once); (b) an explicit
    -- `DecidableEq WarningLineCount` instance (`unfold` + `infer_instance`) so
    -- the two structures `deriving DecidableEq` over that opaque-`def` subtype
    -- field synthesize. Layer-2 Lean→Rust translation now machine-checked.
    `XlateLeanToRust,              -- C-XLATE-LEAN-TO-RUST       (PMAT-915)
    -- PMAT-916 (backlog slice): discharged a 7-error head whose two faults are
    -- BOTH already-established classes (no new termination territory):
    -- (a) the PMAT-914/915 NAME SHADOWING — `def NonEmptyDefinition.val (n) :=
    --     n.val` resolved `n.val` by dot-notation to *itself* (a non-terminating
    --     self-call, `n` unchanged → `fail to show termination` at :816),
    --     poisoning the `.property` (:858) and `Subtype.ext` (:1477) proofs;
    --     fix = positional `.1` Subtype projection in the body.
    -- (b) the PMAT-904/913 Mathlib-only `tauto` — `cases k <;> tauto` over the
    --     decidable `LatexDisplayKind` enum (:1387) → core `cases k <;> decide`.
    -- Layer-5 LaTeX-math notation contract now machine-checked, not excluded.
    `Notation,                     -- C-NOTATION-LATEX-MATH-TO-EQUATION (PMAT-916)
    -- PMAT-928 (backlog slice): discharged the `Bashrs` head — a MIXED
    -- head, as the inventory flagged. Two faults, one already-known and one
    -- genuinely new territory:
    -- (a) the PMAT-914/915/916 NAME SHADOWING — `def SuccessfulOutcome.val
    --     (s) := s.val` resolved `s.val` by dot-notation to *itself* (a
    --     non-terminating self-call, `s` unchanged → `:213` `fail to show
    --     termination`), poisoning the `:233` Gold equality, the `:243`/`:244`
    --     `.property` witnesses, and the `:827` `Subtype.ext`; fix = positional
    --     `.1` Subtype projection in the body.
    -- (b) NEW — a GENUINE Mathlib gap (not a name-shadow): the
    --     `outcome_exit_code_int_sign_diamond` Diamond used Mathlib's `|·|`
    --     absolute-value notation (`:683` `unexpected token '|'`) + `abs_nonneg`
    --     + `simp`, none of which resolve with no `import Mathlib`. Restated
    --     over CORE `Int.natAbs : Int → Nat`: non-negativity is now type-level
    --     (`Nat.zero_le`, no lemma), zero-abs-of-zero is `rw`+`rfl`, and the
    --     trichotomy uses core `Int.lt_trichotomy` (bare `lt_trichotomy` is the
    --     PMAT-904/913 Mathlib alias). Same Int-sign Diamond claim, core-only.
    `Bashrs,                        -- C-BASHRS-POSIX-IDEMPOTENCE (PMAT-928)
    -- PMAT-935 (R6 backlog slice): the new pure-`bool` translation contract joins
    -- the pilot at depth-1 — a core-only, import-free STRUCTURE EXTENSIONALITY
    -- proof (same shape as PyFloatArith / the str/list/set/tuple/Optional
    -- structural Diamonds): a Python bool is determined by its single truth-flag,
    -- so the lowering's polarity is pinned. Closes the last uncited core scalar.
    `XlatePyBoolToRustBool,         -- C-XLATE-PY-BOOL-TO-RUST-BOOL (PMAT-935)
    -- PMAT-936 (backlog slice): discharged the `XlatePyListToVec` head — a
    -- MIXED head, exactly as the inventory flagged (8 errors). Four distinct
    -- classes, all sound, NO new termination territory:
    -- (a) the PMAT-914/915/916/928 NAME SHADOWING — `def
    --     NonEmptyHomogeneousList.val (n) := n.val` resolved `n.val` by
    --     dot-notation to *itself* (a non-terminating self-call, `n` unchanged
    --     → `:593` `fail to show termination`), poisoning the `:632`
    --     `n.property` witness and the `:1257` `Subtype.ext` extensionality;
    --     fix = positional `.1` Subtype projection in the body — all three
    --     cascade errors clear at once.
    -- (b) `:796` — `list_free_monoid_diamond`'s 4th bullet used
    --     `simp [List.length_append]` with no preceding `unfold`, so `simp`
    --     could not fold through the `def` (`simp made no progress`). Reused
    --     the already-discharged Platinum companion
    --     `lower_length_homomorphism_platinum` (which unfolds then simps).
    -- (c) `:980` — core `List.length_reverse` in v4.15.0 takes the list as an
    --     EXPLICIT argument; the bare term left the metavar open (type
    --     mismatch). `List.length_reverse l.elems`. CORE lemma, not Mathlib —
    --     the docstring's "Mathlib's" framing was inaccurate (corrected).
    -- (d) `:1359` — `Array.toList_length` is not a real constant; the core
    --     lemma is `Array.length_toList : a.toList.length = a.size`. `exact
    --     Array.length_toList` (the goal reduces definitionally). Layer-2
    --     Python-list → Rust-Vec contract now machine-checked, not excluded.
    `XlatePyListToVec,              -- C-XLATE-PY-LIST-TO-VEC      (PMAT-936)
    -- PMAT-937 (backlog slice): discharged the `FfiCpythonExt` head — the
    -- Layer-4 hybrid CPython-extension contract, the cheapest of the three
    -- remaining non-elaborating modules by `error:` count (20). FOUR distinct
    -- classes across the errors, all sound, NO new termination territory:
    -- (a) the PMAT-914/915/916/928/936 NAME SHADOWING — `def
    --     BoundedRefcountDelta.val (b) := b.val` resolved `b.val` by
    --     dot-notation to *itself* (a non-terminating self-call, `b` unchanged
    --     → `:979` `fail to show termination`), poisoning the `:987`/`:993`
    --     `deriving DecidableEq` synthesis, the `:1037` `.property` witness,
    --     the `:1750` `Subtype.ext`, and the `:1786`-`:1788` canonical-zero
    --     `rfl`/`decide`s; fix = positional `.1` Subtype projection in the body
    --     PLUS an explicit `DecidableEq BoundedRefcountDelta` instance (the
    --     `deriving` handler can't peer through the opaque `def` subtype — same
    --     fix PMAT-915 needed for `WarningLineCount`).
    -- (b) `:1235`/`:1236` — `refcount_inverse_diamond` used the Mathlib-only
    --     `use` tactic for the existential witness → unknown tactic with no
    --     `import Mathlib`; replaced with core `refine ⟨witness, ?_⟩`.
    -- (c) `:1466` — `refcount_delta_sign_decomp_diamond` used Mathlib's `|·|`
    --     absolute-value notation (`unexpected token '|'`) + `lt_trichotomy` +
    --     `abs_of_pos`/`abs_of_neg` + `Int.sign_mul_abs`; restated over CORE
    --     `Int.natAbs` (PMAT-928 lesson): magnitude `(·.natAbs : Int)`,
    --     trichotomy `Int.lt_trichotomy`, magnitude facts
    --     `Int.natAbs_of_nonneg`/`Int.natAbs_neg`, reconstruction
    --     `Int.sign_mul_natAbs`. Same sign-decomposition claim, core-only.
    -- (d) `:1808`/`:1817`/`:1821`/`:1849` — `lift_ffi_call_bronze_to_silver`
    --     was annotated `: FfiCallSilver` (a 2-field record) but constructed
    --     the SIX structured fields (`symbol`/`from_lang`/…), which only exist
    --     on `FfiCallStructuredSilver`; the annotation made every field
    --     reference an unknown field, cascading into the `:1826`/`:1862`-`:1864`
    --     round-trip `rfl`s. Fix = retarget the lift/projection to
    --     `FfiCallStructuredSilver` (the docstring already described it).
    -- Layer-4 hybrid CPython-extension contract now machine-checked, not
    -- excluded. KNOWN-INCOMPLETE 3 → 2 (CompileRustToPtxMma 38, PyIntArith 45).
    `FfiCpythonExt,                 -- C-FFI-CPYTHON-EXT           (PMAT-937)
    -- PMAT-938 (backlog slice): discharged `CompileRustToPtxMma` — the deepest
    -- module in the tree (20 stacked Diamond categories, depth-3..20). The
    -- inventory's "38 errors" was almost entirely ONE cascading root fault plus
    -- a handful of bare-core lemma-name gaps; FIVE classes, all sound, NO new
    -- termination territory:
    -- (a) the PMAT-914/915/916/928/936/937 NAME SHADOWING — `def BoundedSmem.val
    --     (b) := b.val` resolved `b.val` by dot-notation to *itself* (a
    --     non-terminating self-call, `b` unchanged → `fail to show termination`),
    --     poisoning EVERY downstream `.val`/`.property`/`Subtype.ext`/derived
    --     `DecidableEq` across all 20 Diamonds — the bulk of the 38; fix =
    --     positional `.1` Subtype projection in the body PLUS an explicit
    --     `DecidableEq BoundedSmem` instance (`unfold` + `infer_instance`), the
    --     same opaque-`def`-subtype fix PMAT-915/937 needed.
    -- (b) two `omega` heads failed on NAMESPACED `Nat.min`/`Nat.max` (which omega
    --     v4.15.0 treats as opaque atoms, unlike the `Min.min`/`Max.max`
    --     instances): the +/max·min distributivity and the max/min monotonicity
    --     Diamonds → rebuilt from core `Nat.add_{max,min}_add_{left,right}` and
    --     `Nat.{max_le,le_min,le_max_*,min_le_*,le_trans}`.
    -- (c) `Nat.max_min_self`/`Nat.min_max_self` (absorption) are Mathlib-only →
    --     proved by `Nat.le_antisymm` over the core lattice primitives.
    -- (d) Mathlib-name gaps — `Nat.eq_or_ne` → `omega` (pure linear-arith
    --     disjunction); bare `pow_zero`/`pow_succ`/`pow_add`/`one_pow` (Monoid
    --     lemmas) → `Nat.`-namespaced; `Nat.one_le_pow` → derived from
    --     `Nat.pow_le_pow_left` + `Nat.one_pow`.
    -- (e) a LATENT STATEMENT bug surfaced once it elaborated: the `mod is *
    --     homomorphism` clause wrote `a%2 * b%2 % 2`, which parses left-assoc as
    --     `((a%2)*b)%2`, NOT the both-factors-reduced ring-hom form
    --     `((a%2)*(b%2))%2` the comment intends and `Nat.mul_mod` proves; added
    --     the parens so the proved claim is the genuine homomorphism law.
    -- Layer-5 Rust→PTX-MMA GPU-compile contract now machine-checked, not
    -- excluded. KNOWN-INCOMPLETE 2 → 1 (only PyIntArith 45 remains).
    `CompileRustToPtxMma,           -- C-COMPILE-RUST-TO-PTX-MMA   (PMAT-938)
    -- PMAT-948 (CAPSTONE — the LAST non-elaborating module): discharged
    -- `PyIntArith` (45 errors). The `:892` `fail to show termination`
    -- first-error was — for the NINTH+ time — the NAME-SHADOW class, NOT a
    -- genuine missing measure: `def PyIntFast.val (p) := p.val` self-resolved
    -- by dot-notation to itself, cascading into the `:923`/`:945` `rfl`s and
    -- the `:931` `.property` mismatch → positional `.1` clears all four. The
    -- remaining ~41 errors were ALL cheap classes already exhausted across
    -- PMAT-904..938: Mathlib-only lemma-name gaps restated over core
    -- (`pow_zero`/`pow_one`/`pow_add` → `Int.pow_*` + a hand-rolled
    -- `int_pow_add`; `Nat.land_comm` → core `Nat.and_comm` under `Int.ofNat`;
    -- `Int.lt_asymm`/`Int.one_ne_zero`/min-max-distrib/`dvd_trans`/`Nat.cast_*`/
    -- `Int.toNat_*` → `omega`/`decide`/`Int.*`); Mathlib-only TACTICS
    -- (`ring`/`nlinarith` → explicit core distributivity + `Int.mul_nonneg`/
    -- `Int.mul_pos`); the `|·|` abs NOTATION (undefined in core) restated over
    -- `Int.natAbs` (PMAT-928/937 lesson); and a latent PARENTHESIZATION bug
    -- (`a%2 * b%2 % 2` parses `((a%2)*b)%2`, fixed to the ring-hom
    -- `((a%2)*(b%2))%2`, the PMAT-938 class). The ONE genuinely new piece —
    -- the Bézout identity (`Int.gcdA`/`gcdB`/`gcd_eq_gcd_ab` are Mathlib-only,
    -- verified absent from the entire toolchain `src`) — was NOT weakened or
    -- faked: conjunct (d) is restated as the EXISTENTIAL Bézout
    -- `∃ x y, gcd a b = a*x + b*y` (the genuine mathematical content; the
    -- Mathlib `gcdA`/`gcdB` are just one witness choice) and PROVED core-only
    -- via `Nat.gcd.induction` (the real extended-Euclid structural recursion),
    -- plus universality (c) via `Int.natAbs`/`Nat.dvd_gcd`. No `sorry`, no
    -- `axiom`, no `import Mathlib`. KNOWN-INCOMPLETE 1 → 0 — the ENTIRE
    -- substrate is now machine-checked.
    `PyIntArith,                    -- C-PY-INT-ARITH              (PMAT-948 CAPSTONE)
    -- PMAT-950 (backlog slice): the NEW WGSL compile contract joins the pilot
    -- at depth-1 — a core-only, import-free STRUCTURE EXTENSIONALITY proof (same
    -- shape as PyFloatArith / the str/list/set/tuple structural Diamonds): an
    -- emitted WGSL compute kernel is determined by its structural signature
    -- (entry, workgroup_size, ordered bindings). This is the proof-lane half of
    -- the §29 cross-vendor wgpu DiffExec witness (the runtime half RUNS the
    -- emitted WGSL on a real Vulkan/Metal/DX12 adapter). NEW contract, so the
    -- pilot grows 23 → 24; the entire substrate stays machine-checked.
    `CompileRustToWgsl,             -- C-COMPILE-RUST-TO-WGSL      (PMAT-950)
    -- PMAT-951 (native WASM emit): the NEW WASM compile contract joins the pilot
    -- at depth-1 — a core-only, import-free STRUCTURE EXTENSIONALITY proof (same
    -- shape as CompileRustToWgsl / the str/list/set structural Diamonds): an
    -- emitted WAT function is determined by its structural signature (name,
    -- ordered param value-types, result value-type). This is the proof-lane half
    -- of native WASM emission (the EMIT direction of bidirectional WASM); the
    -- runtime-stratum two-emitter wasm-runtime DiffExec witness is deferred to
    -- PMAT-952. NEW contract, so the pilot grows 24 → 25; the substrate stays
    -- machine-checked.
    `XlateRustToWasm,               -- C-COMPILE-RUST-TO-WASM      (PMAT-951)
    -- PMAT-953 (forjar.yaml backend-only): the NEW forjar compile contract joins
    -- the pilot at depth-1 — a core-only, import-free STRUCTURE EXTENSIONALITY
    -- proof (same shape as CompileRustToWasm / the str/list/set structural
    -- Diamonds): an emitted forjar resource is determined by its structural
    -- signature (id, kind ∈ {file,task,cron}, machine). This is the proof-lane
    -- half of the BACKEND-ONLY forjar integration (xpile-forjar-codegen lowers a
    -- SHELL-origin command sequence to forjar `type: file`/`type: task`
    -- resources, NOT merge/federate); apply-convergence is forjar's own tier
    -- (idempotent-apply), handed off at the YAML boundary. NEW contract, so the
    -- pilot grows 25 → 26; the substrate stays machine-checked.
    `XlateShellToForjar             -- C-COMPILE-SHELL-TO-FORJAR   (PMAT-953)
  ]
