# R0 — The Canonical Form Spec (the database judge's byte channel)

Charter: docs/STAMP_DISCIPLINE_PROGRAM.md § CANONICAL-RENDER VERDICTS
LEG. Every row below is traceable to docs/CANONICAL_RENDER_HOMEWORK.md
(H1–H6), which traces to real legend-pure/legend-engine sources, PCT
pins, or corpus witnesses. This doc is the SPEC the render owner
implements and the cutover slices cite; changes here are spec changes
and need a witness.

## 0. The claim, and the judge that uses it

For every value kind IN THE CLAIMED DOMAIN (§4), pure equality holds
iff the canonical renders are byte-equal:

    equal(e, a)  ⟺  render(e) == render(a)

**Rewritten in the 2026-11 docs cleanup for two-mode judging**
(`docs/JUDGING_TWO_MODES_2026_09_17.md`; the plan path program's R2,
`docs/PLAN_AND_RESULT_MODEL_2026_10_18.md` §11.5; the result boundary
program's R10). These renders are the DATABASE judge's spellings: in
database mode every assertion is a SQL predicate over the canonical
spelling of each side, composed into one verdict statement, so the
database computes both renders and the comparison. The HOST judge is the
verdict of record: a small comparator that receives both sides already
carrying their declared kinds and answers the engine's own rules. The two
must agree: under the agreement gate (`ExecuteOptions.JudgeMode.BOTH`) a
disagreement fails the assertion naming both, never a rescue, and an
unjudged or declined database verdict is reported while the host's stands;
in database mode alone, a shape the canon cannot spell fails as unjudged
and no host verdict fills in. This replaces the design of 2026-08, in which
the byte compare was the verdict of record and the host lattice a parallel
referee; §4b's opening paragraph speaks in those terms and is superseded on
that point, while its three tiers stand.

Two amendments of 2026-08-22, folded here from the cut Amendments log:
(1) the canonical Float render of EVERY zero is `0.0`: Pure grants
`0.0 == -0.0` (witness `parseFloat('-000.000')` on the PCT relational lane),
so negative zero is in the claimed domain, not in the residue §4 lists it
in; where print and equality conflict, equality binds the canonical form.
(2) A render alone is not injective across kinds (String `'8'` against
Integer `8`). Kinds are told apart by mutually disjoint spellings (a Float
its Float form, a Decimal its scale-preserving form with its kind mark, an
Integer bare, temporals prefixed, strings quoted: JUDGING_TWO_MODES §3),
which supersedes that amendment's "(kindClass, text) pairs" and its "the
numeric tower is ONE kind class" (an Integer and a Decimal are different
kinds: §3, amended 2026-09-18).

## 1. Two channels, never mixed

| Channel | Canonical form | Owner |
|---|---|---|
| SCALAR (assert operands, scalar/list values) | pure `toString()` per H1 | Render (grown by R1) |
| GRID (TDS verdicts) | engine `toCSV()` per H2 | Render.java (EXISTS — engine cell rules verbatim, corpus-proven) |

A scalar assert never sees the grid form and vice versa (H2 key
finding). The channels differ ONLY on temporals:
scalar = `yyyy-MM-ddTHH:mm[:ss[.S+]]+0000` (GMT-normalized, subsecond
precision preserved as written); grid = `yyyy-MM-dd HH:mm:ss` (space,
seconds, zoneless) / `yyyy-MM-dd` date-only.

## 2. Per-kind canonical render (scalar channel)

| Kind | Canonical form | Ground |
|---|---|---|
| Integer | `1`, `-1` — no decoration | H1 testIntegerToString |
| Boolean | `true` / `false` | H1 |
| String | verbatim bytes, unquoted | H1 (toRepresentation quoting is NOT the canonical channel) |
| Float | fixed-point ALWAYS, never exponent; trailing zeros collapse to one (`17.000`→`17.0`); integral floats keep `.0`; leading zero enforced (`0.01`) | H1 testFloatToString×4; DuckDB bare CAST DIVERGES on small magnitudes (`1.3421e-08`) → R1 builds a no-exponent format expression |
| Decimal | **SCALE-PRESERVING print (amended 2026-09-18, leg 3.0)** — `3.0D` and `3.00D` spell differently and are DIFFERENT values | The compiled engine's assert seam is `getValue().equals` — scale-SENSITIVE (VERDICT_RULE_AUDIT X2; the equality-worlds fixture, World 1); the host judge of record (`Equality`, step 2a) applies exactly that rule, and a scale-normalized canon would make database mode disagree with it. The earlier row ("FORCED by equality: pure Decimal equality is numeric/scale-blind") described the INTERPRETED runtime's compareTo, not the compiled reference this platform follows (JUDGING_TWO_MODES §1). Witness count in the corpus: ZERO pairs differ by scale only (both lanes, `decimal-scale-only=0`, leg 3.0 census) — the amendment costs no row and removes a designed disagreement. |
| Date (partial) | `2014`, `2014-01`, `2014-01-01` — precision preserved, zero-padded | H1 |
| DateTime | `yyyy-MM-ddTHH:mm[:ss[.S+]]+0000`, GMT-normalized, subsecond precision preserved as written (`.000` ≠ `.0` ≠ none) | H1; +0000 STRFTIME sites already implement this (audit: "+0000 archaeology closed clean") |
| List | `[a, b, c]` — elements by this table, `, ` separator, nested allowed | H1 testListToString |
| Enum value | bare member name | H1 |
| Empty/null cell (grid) | `''`; `TDSNull` only under renderTdsNull=true | H2 toCSVString:232 — the sentinel's origin |

Grid channel: the table above via H1 for non-temporal cells + CSV
escaping (quote iff `,` `"` `\n` `\r`; `"` doubles) + header/`\n`
framing per H2.

## 3. Equality lattice the byte channel must reproduce

From H4 (PureAsserts), the cross-kind rows the canonical form makes
byte-decidable:

- integral × integral: by value → identical canonical integers. OK.
- integral × Decimal: **NOT equal under the compiled reference (amended
  2026-09-18 with the §2 Decimal row)** — Rule 3 of the numeric charter:
  same kind by declaration, same value; an Integer and a Decimal are
  different kinds, and `Equality` (the host judge of record) answers
  false. The canon spells the Decimal with its scale and its kind mark,
  so the pair byte-differs — the same answer. (The former R1 rule that
  rendered an integral Decimal without `.0` served the interpreted
  runtime's numeric compare; superseded.)
- integral × Float: NOT equal in pure → `8` vs `8.0` byte-differ. OK.
- Float × Float: exact equal → byte-equal. The 2-ULP tolerance is a
  DECLARED NUMERIC POLICY outside the byte channel (§4), R3 census
  decides retire-vs-keep.
- Temporal precision: distinct subsecond precisions are DISTINCT pure
  values → precision-preserving render byte-differs. OK.
- TDSNull sentinel: expected-direction only (H4) — grid channel
  renders `''`; the sentinel policy rides the harness expected-side
  decode, not the render.

## 4. The claimed domain and the named residue

IN: Integer, Boolean, String, finite Float, Decimal, Date/DateTime
(all precisions), enum values, lists thereof, TDS grids thereof.

OUT (loud residue — walls or the declared policy, never silent):
- NaN / ±Infinity / -0.0: ZERO witnesses in the pure essential tree
  and the engine relational tree (H6, measured). Non-finite floats
  WALL at the render with a named reason.
- Huge-magnitude prints (1e300): no witness (H6) — same wall.
- assertEqWithinTolerance (6 corpus sites): declared numeric policy,
  outside the byte channel by definition.
- Class instances: identity ids are not canonical (H1) — instance
  equality stays a host/referee concern, out of the byte channel.

## 4b. THE PERMANENT BOUNDARY (V11 close, user-converged 2026-08-22)

The DB channel was never going to claim everything, and this section
makes the boundary a finished design statement instead of unmet
ambition. The DB is the verdict of record FOR THE CLAIMED DOMAIN; the
host lattice is the verdict of record FOR THE RESIDUE BELOW (openly,
not as a fallback); the host shadows every DB verdict as the permanent
referee. Declines are a boundary census, NOT debt — each lands in one
of three tiers:

- **Tier A — no NATIVE type (nearly empty once carriers are allowed):**
  decimals beyond 38 significant digits / integers beyond HUGEINT as
  native kinds (text-encodable, so even these are Tier-B-with-work).
  NUL-bearing strings turned out to be NEITHER tier (user catches
  2026-08-22, twice): DuckDB VARCHAR holds NUL fine — the failure was
  our StringLit renderer putting the raw byte into statement text
  (lexer death). FIXED at the spelling (chr(0) splice). A "Tier A"
  claim should be re-verified against the DB's actual value domain
  before it is believed.
- **Tier B — erased by SQL's TYPE system but encodable as data
  (CLAIMABLE; the pattern is proven):** per-element kind variance
  (mixed collections, if-branch CASE promotion, the print-form
  identity carrier) → F10 variant carrier; temporal written-precision
  → ALREADY CLAIMED via the precision-faithful VARCHAR convention.
  When the type channel erases, move the information into the value
  channel — never guess.
- **Tier C — not data at all (permanently host/model territory):**
  model-defined equality (equality.Key stereotypes — X5 claims the
  byte compare via key-derived canon, the SEMANTICS stay model-owned);
  object identity (eq() on non-primitives — rows have no identity);
  orchestration handles (Connection/Runtime); the metamodel itself
  (Package/Multiplicity/ValueSpecification, lambdas as values).

Every decline reason maps to a tier; a decline that fits no tier is a
bug in the wrap, not a new category.

> Cut in the 2026-11 docs cleanup: §5 the cutover order (per-family hard cutovers by assert-site counts, 2026-08) and
> the Amendments log (2026-08-22; its two rules are folded into §0). Full text:
> `git show de2745351:docs/CANONICAL_FORM_SPEC.md`.

