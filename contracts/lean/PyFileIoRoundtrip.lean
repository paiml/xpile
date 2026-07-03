/-
  PyFileIoRoundtrip.lean — Lean 4 refinement proof for `C-PY-FILE-IO-ROUNDTRIP`.

  Proof-lane counterpart to `contracts/py-file-io-roundtrip-v1.yaml` (R6 /
  PMAT-1124). Contracts xpile's whole-file I/O lowering (PMAT-1074/1075/1076/
  1078): `open(p).read()` → `std::fs::read_to_string(p)`; `open(p, "w").write(s)`
  → `std::fs::write(p, s)` (truncate); `open(p, "a").write(s)` → OpenOptions
  append; the idiomatic `with open(p) as f: …` desugar; and `for line in
  open(p)`. The load-bearing correctness property is the ROUND-TRIP: reading a
  file after writing it returns exactly what was written, write-mode `"w"`
  TRUNCATES (independent of prior content), and append-mode `"a"` ACCUMULATES.
  These are the properties verified empirically this session (write-then-read
  MATCH; overwrite → truncate; append → accumulate).

  provability/mathlib note: the file model is a pure `String` state; read/write/
  append are total functions over it, so every theorem is `rfl` (structure
  projection + `String.append` left-assoc) or structural — discharged over CORE
  Lean 4 with NO `import Mathlib`, no `sorry`, no `axiom`. A stateful file
  effect, once abstracted to its content, needs nothing from real-analysis /
  linear-algebra. (The effect ORDERING across a program is an emit-lane concern;
  this contract pins the per-operation content semantics.)

  Modelling note: the structure-extensionality Diamond registers the contract at
  depth-1; the four semantic theorems pin read-after-write, truncation, and
  append-accumulation.
-/

namespace XpileContracts.CPyFileIoRoundtrip

/--
  Abstract model of a file as xpile's whole-file I/O sees it: its byte content
  (a `String`). A file carries no other state relevant to whole-file read/write
  (mode, handle, cursor are emit concerns), so the content fully determines what
  a subsequent `read()` returns.
-/
structure FileState where
  content : String
  deriving DecidableEq

/-- `open(p, "w").write(s)` — truncating write: the new content is `s`,
    independent of the old content (the proof-lane mirror of `std::fs::write`). -/
def writeFile (_old : FileState) (s : String) : FileState := ⟨s⟩

/-- `open(p, "a").write(s)` — append: the new content is old ++ s (the mirror of
    `OpenOptions::new().append(true)…write_all`). -/
def appendFile (f : FileState) (s : String) : FileState := ⟨f.content ++ s⟩

/-- `open(p).read()` — read the whole content (the mirror of
    `std::fs::read_to_string`). -/
def readFile (f : FileState) : String := f.content

/--
  **Diamond refinement theorem** for
  `file_state_structure_extensionality_diamond` (the tier-defining equation):
  two file states with equal content are equal. Registers
  `C-PY-FILE-IO-ROUNDTRIP` at depth-1.
-/
theorem file_state_structure_extensionality_diamond (a b : FileState) :
    a.content = b.content → a = b := by
  intro h
  cases a
  cases b
  simp_all

/--
  **Round-trip** (the reason this contract exists): reading after a write returns
  exactly what was written — `read(write(f, s)) = s`. The write-then-read
  faithfulness verified empirically (write "payload"; read → "payload").
-/
theorem read_after_write (f : FileState) (s : String) :
    readFile (writeFile f s) = s := by
  rfl

/--
  **Truncation**: `"w"` mode overwrites — the result is independent of the prior
  content (`std::fs::write` truncates). A second write of the same content from
  ANY prior state gives the same file.
-/
theorem write_truncates (f g : FileState) (s : String) :
    writeFile f s = writeFile g s := by
  rfl

/--
  **Read-after-append**: `"a"` mode keeps the old content and appends —
  `read(append(f, s)) = f.content ++ s`. Distinguishes append from the
  truncating write.
-/
theorem read_after_append (f : FileState) (s : String) :
    readFile (appendFile f s) = f.content ++ s := by
  rfl

/--
  **Append accumulates**: successive appends concatenate in order — the
  build-a-log idiom (`open(p,"w").write(""); for …: open(p,"a").write(x)`)
  accumulates rather than truncating.
-/
theorem append_accumulates (f : FileState) (a b : String) :
    readFile (appendFile (appendFile f a) b) = f.content ++ a ++ b := by
  rfl

end XpileContracts.CPyFileIoRoundtrip
