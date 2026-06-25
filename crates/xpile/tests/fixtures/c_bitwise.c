/* PMAT-964: C bitwise + shift operators over the decy C→Rust path.
   Integer-only, ABI-honest (rides the existing i32 width — no new token),
   governed by the existing C-C-INT-ARITH integer-semantics contract.
   Bitwise `& | ^` lower to parenthesized Rust infix; shift `<< >>` lower to
   `wrapping_shl`/`wrapping_shr` (UB-free, the defined replacement for C's
   out-of-range-shift UB); unary `~` lowers to Rust `!` (one's complement). */

/* bitwise AND / OR / XOR over packed flags */
int band(int a, int b) { return a & b; }
int bor(int a, int b) { return a | b; }
int bxor(int a, int b) { return a ^ b; }

/* one's complement (unary ~) */
int bnot(int x) { return ~x; }

/* shifts: `1 << n` builds a bit mask; `x >> k` extracts a field */
int shl(int x, int n) { return x << n; }
int shr(int x, int k) { return x >> k; }

/* precedence: C binds `<<` below `+` and `&` below `==`. So
   `(a + b) << 1` and `& 255` must group as written — the masked
   low byte of (a+b), doubled. (Decimal literals only — decy has no
   hex-literal lexer yet.) */
int mix(int a, int b) { return ((a + b) << 1) & 255; }

/* combine: set bit n in x, then clear bit 0 — pure bit-twiddling */
int setbit(int x, int n) { return (x | (1 << n)) & ~1; }
