# Rust port — phase 3 design: node identity (D1) and the shared model context (D5)

**Status: a draft for the owner to rule on (2026-11-09). No port code exists.** [`ROADMAP.md`](ROADMAP.md) phase 3
requires these two designs settled first; [`TRIAGE.md`](TRIAGE.md) names them D1 and D5. §5 is the list of rulings.

**Where the numbers come from.** Measured at `91db76618`; `core/src/main/java` is unchanged at `2a20d1cf9`, the tip when
this was written. `==`, `equals`, map-key sites and field writes were found by a throwaway program over javac's Tree API
that attributed core, `base`, `json` and `arrow_ipc` with no compile errors, so "a reference comparison" means both
operands' static types are references and neither is an enum or `null`. The sites in §1.1 and §2.1 were then read by
hand. The classification is mine; §4 says where it could be wrong.

## 0. The decisions, on one page

**D1.** TRIAGE's reason for a NodeId does not hold. It says nodes are re-wrapped, so `Rc::ptr_eq` is not enough. But
Java's `==` does not survive a re-wrap either: `withInfo` and `withChildren` make a new object. What survives is a
*field*: `origin` points at the source node and is copied through every typed-node copy. The real requirements are
Java's three operations on a reference: alias it, compare it (`==`), compare what it holds (`.equals`).

**Recommendation: an enum of `Arc<Variant>` for the typed tree, the source tree and the model's elements, with identity
as the `Arc`'s pointer, behind one small module; the SQL tree stays as phase 1 built it.** This differs from TRIAGE's
suggestion. §1.4 gives the reasons and what would change them.

**D5.** The problem is much smaller than TRIAGE suggests. Everything written after a model is built is a memo of a pure
function of the model, in 17 fields behind one interface. No context is used by two threads at once anywhere in the
tree today. Nothing about overload rollback touches the model. **Recommendation: a frozen `Arc<Model>`: Knowledge built eagerly and
immutable, Work memos in `OnceLock` cells for the keys known at freeze and a mutex-guarded map, computed outside the
lock, for the rest. Everything per-query stays owned by the query, with no synchronization.**

## 1. D1: node identity

### 1.1 Measured

Scope: `compiler`, `normalizer`, `resolver`, `lowering`, `sql`, `planner`, `builtin`, `plan`, `executionplan`,
`lineage`, `platform`, plus `model` and `validation`, which are on the same path. `sql`, `planner`, `builtin`, `plan`,
`executionplan`, `lineage` and `platform` construct no identity collection.

| | TRIAGE at `cf73e6ef7` | Measured |
|---|---|---|
| Identity collections (`IdentityHashMap`, `newSetFromMap(new IdentityHashMap…)`) | "about 300" (compiler 19, resolver 27, sql 23, lowering 6, normalizer 5) | **33 constructed**: compiler 13, resolver 10, normalizer 4, lowering 3, model 3, sql 0 |
| In the translators, for comparison | — | 104 lines in `protocol/sql/iq` and `protocol/cypher` (a later wave); 5 in `protocol/sql/SqlToPure` (phase 3 or that wave) |
| `==`/`!=` between non-enum references | not counted | **430** |
| `System.identityHashCode` | CommonSubexpressions (C5) | 6 lines: 4 are `hashCode`s beside an `==` equals, 2 build a name. C5's site went to Rust in phase 1, which breaks the tie by pre-order (`common_subexpressions.rs`) |
| `equals` comparing a field by `==` | `TypedUserCall` | 4 types (below) |

`sql` went to zero because the dialect passes moved in phase 1. I could not reconstruct how TRIAGE counted 300; today
104 of the 163 lines in core's main sources that mention these are the later-wave translators.

**By what each site needs.** "(a′)" splits the brief's class (a): a *side table* is a map from a node to a fact about
it, where a miss changes behaviour rather than costing time.

| Class | Collections | `==` |
|---|---|---|
| (a) memo: a miss only costs time | 6 | 0 |
| (a′) side table: the lookup must hit | 7 | 0 |
| (b) semantic: equal nodes must stay distinct | 13 | 25 |
| (c) sharing: one node reached twice is one node | 6 | 3 |
| (d) "did this change?" after a rewrite | 0 | 357 |
| (e) other | 1 | 45 (35 local, 10 `d == rust` in the Java dialect shims) |

**Class (d), 357 sites**, is one idiom: rewrite the children, hand back the original if none changed. By static type:
`List` 94, `TypedSpec` 77, `SqlExpr` 54, `ValueSpecification` 30, then small counts. By place: `NameResolver` 131,
`SqlRewriter` 74, the rest scattered. Representative: `TypedSpec.java:124` (`mapChildren`), `SqlRewriter.java:53`,
`NameResolver.java:646`. Phase 1 already ported this idiom for the SQL tree as `Rw::Same`/`Rw::New`
(`rust/lite-dialect/src/rewriter.rs`).

**Class (d) is not independent of the rest.** It would be correct under structural equality *if nothing else looked at
identity*, but an unchanged subtree keeping its identity is what lets a table filled before a pass still hit after it.
Dropping (d) as "only an optimization" would silently break (a′) and (b).

**Class (a), memos (6).** `SpecCompiler.java:46` (body per `TypedFunction`), `Anchors.java:66,132`,
`ClassSources.java:319` (per routes list), `ContextReading.java:455,1060` (visited sets).

**Class (a′), side tables (7).**

| Site | Key → fact | A miss |
|---|---|---|
| `Lowerer.java:2041`, `WhereMerge.java:35` | a WHERE `SqlExpr` → its zones | **Silent**: an unregistered WHERE is all user-zone, so predicates reorder. A fallback of the kind invariant 4 forbids |
| `DateSplit.java:68,70`, read at `SyntheticHeads.java:1774-1775` | a milestoned access → rename or strip | **Silent**. Its comment (`:1765`): rebuilt nodes "would orphan the second map's identities", so it runs as one pass |
| `StackBuilder.java:99` | a stack source → its leaves | A copy is kept in step by hand (`alias`, `:173-178`), the one place Java propagates identity across a copy |
| `StoreResolver.java:2216` | an aggregate node → its read | read through `Substitution.aggReads()` |
| `InnerDemand.java:751` | an `in`/`contains` node → its read | read at `Substitution.java:546,627,1094` |
| `Lowerer.java:1166` | a `TypedAggCol` → calendar context | per groupBy |

**Class (b), semantic (13 collections, 25 `==`).** Two shapes.

*Source-node origins and occurrences:*

| Site | What is compared |
|---|---|
| `TypedUserCall.java:74,79` | `origin`: the `AppliedFunction` the typer read the call from (`Typer.java:262`) |
| `Evaluation.java:108,113,119,125` | `origin`: the `NewInstance` (`NewChecker.java:74,233,249,260,263`), or `new Object()` for a synthesized one (`:42`) |
| `UserCallInliner.java:136,142` | `CallInstantiation`'s origin and `frame` (a `new Object()`, `:127,1582`) |
| `SourceSubst.java:70` | the source nodes whose evaluations are untracked; matched against origins (`Evaluation.java:67-78`) |
| `ContextReading.java:453` | *variable occurrences* bound by an enclosing lambda: two equal `$x` nodes, one bound, one a let's |
| `SyntheticHeads.java:1960` | column lambdas born from values-position maps; its comment documents that a rebuild silently breaks it |

*Model records that may be value-equal yet are two declarations:*

| Site | Why |
|---|---|
| `ModelBuilder.java:380-402` (16 `==`) | "this very object already sits in its slot": a second, equal declaration is a duplicate to report, not a repeat to skip |
| `PureModelContext.java:104` | the boot layer's elements, by instance |
| `DatabaseDefinition.java:70,86`, `TableIndex.java:58` | a default-schema table is one no named schema owns; two schemas may declare equal tables |
| `LiftedViews.java:50,53,55,57`, `ViewSignatures.java:51` | views, likewise |
| `MappingDefinition.java:174` | "by identity: two bindings may be equal" |
| `MappingClosures.java:297`, `MappingPrePass.java:259`, `AssociationSynthesis.java:202`, `ScanRelations.java:1611` | a diamond's one mapping vs a genuine duplicate, a self-edge, a mapping in its own closure, a union arm |

**Class (c), sharing (6 collections, 3 `==`).**

| Site | The shared node |
|---|---|
| `UserCallInliner.java:114`, `:1333` (`reducedTerms`) | a term a β site bound, placed verbatim at each read: "no node is rewritten twice". Structural keys would wrongly skip an equal term written elsewhere |
| `UserCallInliner.java:268` (`countReads`) | "a term is found by reference": a `random()` bound once and read twice is refused; one written twice is the program's own |
| `Typer.java:108,118`, `Overloads.java:361` (`pretyped`) | an argument typed by its call, met again in the inlined body: typed once. Added for "a chain of k calls planned in 2^k" |
| `NameResolver.java:1388` | a schema view is the same object in `schemas[].views` and the flat `views` |
| `InnerDemand.java:486` | de-duplicates lambdas by instance |
| `ModelNormalizer.java:220`, `ModelBuilder.java:786` | an adopted property is the object in its owner's list; a self-association's end injected twice |

**Class (e).** `ObjectSites.java:16` keys ids by node, but on the product path `Lowerer.objectId` returns first
(`Lowerer.java:298`), so that map looks dead; the verdict lane's per-node ids are `judge/InstanceIds.java:22`, reached
through `BodyDriver.java:2248`, which stays Java (§3). The 35 local `==` sites ask "which of these two did I pick?" or
"which child am I?": `Anchors.java:106,156-165`, `Pipelines.java:182,199`, `RawGridSchema.java:274,276,356`,
`EnumSourceValues.java:41,44,69`, `GraphAggDecorrelate.java:99`, `VerdictSql.java:688,813,1342`, and three
find-and-replace walks (`KeyedPivot.java:143`, `Lowerer.java:3948`, `Fold.java:202`).

`NavReducer.java:63,75` names a variable `_e<identityHashCode>`. As far as I can read it never reaches output:
`GraphEmission.java:2791-2797` substitutes it away, and the error at `:2764` is unreachable for that shape. The port
should use a counter; no register row is needed if that reading holds.

**No identity collection is iterated where order can be seen** (by search and reading). The iterations are
`Typer.java:119,123`, which save and restore, and `PureModelContext.java:123`, which copies one set into another.
Identity reaches SQL only through `ObjectSites`' numbering (`o1`, `o2`, …, first seen in lowering order), so ids are
deterministic only if Rust's lowering visits constructions in Java's order.

**Hand-written equality** (a derived `PartialEq` would silently change these):

| Type | Differs from a record's default |
|---|---|
| `TypedUserCall` | `origin` by identity; includes `pos` |
| `TypedNativeCall` (`:39`) | **excludes `pos`** |
| `Evaluation`, `Evaluation.Site`, `CallInstantiation` | `origin`/`frame` by identity |
| `FunctionMatch` (`:74`) | `types` and `multiplicities` only |
| `StaticFold.TypeToken` (`:89`) | `path` only |

**Every way a node is copied, and what Java does.** Sites counted by text match.

| Operation | Count | New object? | What carries identity |
|---|---|---|---|
| `withChildren` | 83 definitions, about 76 calls | always | copies `origin`, `evaluation`, `pos` |
| `mapChildren` (`TypedSpec`, `SqlExpr`, `RelationalOperation`) | 80 mentions | only if a child changed | the original when unchanged |
| `withInfo` | 83 definitions, 4 real callers (`UserCallInliner.java:752,766,767`, `StackBuilder.java:170`) | always | copies the fields |
| `withOrigin`, `asUntracked`, `withEvaluation`, `Evaluation.at/complete/untracked` | 2, 8, 10 | always | keep `origin` |
| `rebuildChildren` (`SyntheticHeads.java:1799`), `rebuilt`, `rechild0` | 11, 19, 3 | **always, even if unchanged** | nothing: why the date pass must be one pass |
| `TypedSubst.apply` (G½) | — | rebuilds the spine | **the one term object at every read** |
| `SourceSubst.substitute`, `inlineLets` | — | rebuilds the spine | **the one value object at every read**; so copies of a let share an origin |
| `AlphaRename.apply`, `ColumnHygiene.apply`, `StaticFold` | — | rebuilds calls | new source nodes, so new origins, per inline site |
| `NameResolver.resolve*`, `SqlRewriter` | 131, 74 compares | only if changed | the original when unchanged |
| `ClassSource.withScope` then `StackBuilder.alias` | 1 | always | the table entry, copied by hand |

Constructors: 1,453 `new Typed*` and 443 of source-tree kinds, about 1,900; with the SQL tree's 2,035 `new` and 1,045
factory calls, about 5,000.

**Does identity need to survive a copy?**

| Class | Today |
|---|---|
| (a) | No. A rebuilt node misses and recomputes |
| (a′) | Until the lookup. Java relies on no pass rebuilding in between, and says so where it is fragile |
| (b) origins | Yes, always, as a field. `origin` is **a pointer to the source node, never to a previous typed node**. It is never dereferenced: it is `Object`-typed and only compared or hashed |
| (b) occurrences | No: a rebuild loses it, a documented hazard |
| (c) | No copy happens. The same reference is placed twice |
| (d) | Compares a node with its own rewrite |

### 1.2 The options

**N. A NodeId stamped at construction, trees as `Box`.** A `u64` on every node; identity maps become
`HashMap<NodeId, _>`. To match Java, `Clone` keeps the id (a clone is Java's alias) and every constructor and
`with_children`/`with_info` mints a new one. From one global `AtomicU64`: a per-compilation counter would have to be
threaded through about 1,900 construction sites and model-tier bodies outlive a compilation, and a thread-local collides
when a handle moves between workers. Kept out of output by the type: a private field, no `Display`, `Ord` or conversion
to an integer, `Debug` printing a constant, and `PartialEq`/`Hash` that always say "equal" so derived structural equality
ignores it. `origin` needs a second type with real equality.

**P. Pointer identity on shared nodes.** Every node is reached through an `Arc`; `Arc::ptr_eq` is `==`. `Rc` is out: it
is not `Send`, and `on_big_stack` (`rust/lite-ffi/src/lib.rs:130`) runs each call on the calling thread's own worker, so
a handle's next call can be on another thread. The shape matters, and is the one `lite-front` already uses, with `Arc`
in place of `Box`:

```rust
enum TypedSpec { Filter(Arc<TypedFilter>), Lambda(Arc<TypedLambda>), /* … one per record */ }
struct TypedFilter { source: TypedSpec, predicate: Arc<TypedLambda>, info: ExprType, /* … */ }
```

Cloning a `TypedSpec` copies one pointer and bumps a count. `children()` hands back
`TypedSpec::Lambda(predicate.clone())`, the same allocation as the field, so Java's "one object, several static types"
holds (`RawGridSchema.java:276` compares a `TypedSpec` with a `TypedLambda`).

**A. An arena with indexes.** Nodes in a `Vec`, children and identity are `u32` indexes, one arena per query and one for
the model. A bump arena handing out `&'a` references is the variant rustc uses.

**S. An explicit identity field only where (b) needs it, structural equality elsewhere.** `Box` trees, `origin` as a
field, and everything else keyed or compared by value.

### 1.3 What each costs here

| | N: NodeId + `Box` | P: `Arc` pointers | A: arena | S: `origin` only |
|---|---|---|---|---|
| (a) memos | hit | hit; the map must hold an `Arc` so an address is never reused | hit | **deep hash per lookup**: `Anchors` becomes quadratic |
| (a′) side tables | hit, as in Java | hit, as in Java | hit | equal nodes collide; redesign each |
| (b) origins | second id type | a token holding an `Arc` | index | as designed |
| (b) occurrences and elements | hit | hit | hit | **breaks**: `lambdaBound`, `indexed`, `valuesLambdas` need redesign |
| (c) sharing | right identity, but **every alias deep-copies** | native | native | **breaks**: `reducedTerms`, `countReads` need redesign |
| (d) | `a.id() == b.id()` | `same(&a, &b)`, line for line | index `==` | structural `==`, O(size) |
| Structural `.equals` (about 20 calls on trees, 6 `Set<TypedSpec>`) | derive, with a no-op `NodeId` impl | **plain derive**: `Arc<T>` compares contents | hand-written deep walk | derive |
| Discipline at about 1,900 construction sites | must mint; `..*old` silently copies an id | none: `Arc::new` is the mint | must allocate | none |
| Keeping identity out of output | by the newtype's API | nothing to keep out: an address cannot be spelled without `{:p}` | by API | n/a |
| Parser records (`lite-front/src/records.rs`, generated, `Box`) | add a field and keep it out of the lossless codec and protocol JSON, or stamp at D into a second tree family | `Box`→`Arc` for the `ValueSpecification` family: the writers read through `&`, so bytes cannot change | a second tree family | none |
| Lowering closures | fine (`&self`, `Cell` counters) | fine: a node is not borrowed from anything | **`&mut Arena` in every closure**; `RefCell` or lifetimes throughout | fine |
| Typer rollback (D2) | ids leave gaps, harmless; `typed.clone()` per candidate is deep | failed nodes just drop; `typed.clone()` is shallow | `truncate(mark)` is free, but anything memoized during a failed candidate sits above the mark | as N |
| Performance | no refcount, but deep clones wherever Java aliases | a malloc per node (as `Box`), an atomic per clone | fastest | deep clones and hashes |
| FFI handle held across calls | `Send`; simple | `Send + Sync`; a query handle simply holds an `Arc<Model>` | index arena `Send`; a `&'a` arena is self-referential across calls | `Send` |
| Rust ergonomics for a newcomer | moderate | closest to Java: `clone` is free, no lifetimes | hardest | moderate |

*Deep clones are a constant factor, not a blow-up*: a subtree appearing k times in the SQL already makes the SQL k times
as big. The cost is every pass that keeps the old tree while building the new one, and every place Java stores a node
twice (a `ClassSource.pipeline`, a memoized callee body, a let's term). With `Box`, each is a decision: borrow with a
lifetime, or pay for a clone. With `Arc` it is `.clone()`. I could not measure the difference (§4).

### 1.4 Recommendation

**Option P, with these parts.**

1. **Tree shape.** `TypedSpec` and `ValueSpecification` are enums of `Arc<Record>`. Model elements and identity-bearing
   records (`TypedAggCol`, `ClassSource`, `ViewDefinition`, `TableDefinition`, `ClassBinding`, …) are held as `Arc`.
   **A list field is `Arc<[T]>`, not `Vec<T>`.** A `Vec` has no identity, and Java's lists do: 94 of the class (d) sites
   compare a `List` (`NameResolver.java:646`, `args == g.arguments()`, after a helper that hands back the original list
   when nothing changed), one memo is keyed by a list (`ClassSources.java:319`), and a `withInfo` copy aliases its
   record's lists. With `Arc<[T]>` the list helpers return the original `Arc`, `same` applies to lists as to nodes, and
   a copy aliases. The cost is one more allocation copy when a built `Vec` becomes an `Arc<[T]>`.
2. **One module, `identity`**, holding `same(a, b)`, `IdMap<K, V>` and `IdSet<K>` (lookup and insert; **no iteration
   API**, so the no-hash-order invariant is enforced by the type), and `Origin`: a token with pointer equality, made from
   a source node or by `Origin::fresh()` (Java's `new Object()`). `Arc::as_ptr` appears nowhere else.
3. **Port the sites line for line**: `==` to `same`, `IdentityHashMap` to `IdMap`, `withChildren` to a new `Arc`. Write
   the five hand-written `PartialEq`s by hand.
4. **The SQL tree stays `Box`-owned** (`lite-sql`) with phase 1's `Rw`, so no dialect changes. Its 11 local `==` sites
   and `whereZones` are ported by hand: a bool for "which did I pick", an index for "which projection", `std::ptr::eq` on
   borrowed references for the three find-and-replace walks.
5. **`whereZones` is the one decision that does not fall out.** It keys a side table on a `Box`-owned `SqlExpr`. I
   recommend keying it by content (the fingerprint-plus-equality scheme `common_subexpressions.rs` uses), because two equal
   WHEREs differ at most in later predicate order, which the golden SQL shows. Java's miss fallback stays, as it is today's
   behaviour; §5 asks.

Three Rust facts decide the details. `derive(PartialEq, Hash)` over `Arc<T>` compares contents (Java's record `equals`),
and `Arc::ptr_eq` is `==`. A pointer key needs the map to keep the node alive, or a freed node's reused address gives a
false hit; Java's `IdentityHashMap` holds a strong reference, so `IdMap` holds an `Arc` clone. And `Arc` chains drop
recursively: a deep tree overflows a 1 MB stack, which aborts the JVM, so dropping a handle must run on the big-stack
worker (`on_big_stack`, 256 MB reserved). Reference cycles cannot arise, since nodes are immutable and built bottom-up.

**Why.**

- Phase 3 "has no Java checkpoint inside it" (ROADMAP). Over 130K lines, the most valuable property is that each Java line
  has one obvious Rust line. P is the only option where `new`, alias, `==` and `.equals` each map one to one with **no
  discipline at the construction sites and no analysis of which sites feed which**. In particular the (d)→(a′) dependency
  is kept without anyone having to understand it.
- N is the serious alternative and it would work. But it needs a rule followed at about 1,900 sites, an equality that
  deliberately ignores a field, an id kept out of a byte-exact codec, and, with `Box`, a decision at every Java alias. Put
  on `Arc` to fix the aliasing, the id is redundant.
- The mechanism is reversible: callers see `same`, `IdMap` and `Origin`. If stored ids are wanted later,
  `docs/architecture/README.md` §7's target ("every binder an id, every node an id"), they go in behind that module and
  the constructors.

**What would change it.**

- A measurement (below) showing allocation and refcounts cost materially more than Java on the stress corpus: then a
  per-query arena for the typed tree, accepting its costs.
- A need for ids that outlive a process or cross the wire (a persisted typed tree): then NodeId.
- If the `driver`'s shim functions must name individual typed nodes across the FFI (§3), an id may be the simpler handle.
- If the owner reads "every node an id" as binding on this iteration rather than the rebuild it came from.

### 1.5 Verification

| Gate | Catches |
|---|---|
| `ObjectIdentityTest`, `ObjectIdentitySqliteTest` | wrong `origin`, wrong path, a lost let identity |
| `//pct:pct_duckdb`, `pct_h2`, `pct_channel_b` | value semantics; channel B's verdict sides must agree on sites across two inliner runs |
| `//spec:corpus_duckdb`, `corpus_h2`, `//core:stress_suites`, the SQL ladder | a wrong (a′), (b) or (c) where it changes SQL or rows |
| The ROADMAP's "corpora through a Rust CLI", with Java's `MirJson` as the answer | any tree difference, byte for byte, before the switch |
| `ScaleCostsDuckDbTest`, `CompileLatency` | a memo that stopped hitting, only as time |

None of these sees a memo miss except as time, or a side-table miss that happens not to change a corpus query. **New
tests:**

1. **An identity hit-count oracle, made while Java still exists.** A probe patch in the style of
   `rust/tools/mirjson-roundtrip-probe.patch` counts hits and misses per query on each table of §1.1 (`Anchors`,
   `reducedTerms`, `pretyped`, `whereZones`, `aggReads`, the date tables) and commits them under `rust/oracles/`. Rust must
   match. It cannot be written once Java is gone.
2. **Two-run determinism**: the corpora twice, in two processes, plan JSON byte-identical. Addresses and `HashMap` seeds
   differ between runs, so any leak shows.
3. `identity` unit tests (equal but not same; same after clone; a `Filter` rebuilt with equal children is not `same`), and
   a guard that `Arc::as_ptr` and `{:p}` appear only in that module.
4. A deep-chain test: release a handle to a 5,000-deep tree from a 1 MB thread (as `PostgresToPureTest.java:283` does).
5. **The first measurement.** Port `TypedSpec` and the inliner first and compare allocation count and time against Java on
   the stress corpus, before the resolver and lowering are built on the tree.

## 2. D5: the shared model context

### 2.1 Measured

**Everything written after `PureModelContext.from` returns: 17 fields** (field writes outside constructors and static
initializers, found by the census and by reading; `ModelBuilder.add`'s last call is inside `from`, `:184`).

| Field | Write site | Container | Key set | Kind |
|---|---|---|---|---|
| `classCache` | `PureModelContext.java:292` | `HashMap` | closed at freeze | Work |
| `enumCache` | `:381` | `HashMap` | closed | Work |
| `functionCache` | `:411` | `HashMap` | closed | Work |
| `packages` | `:459` | `HashSet`, filled once | closed | Knowledge |
| `declarations`, `implementations` | `:622`, `:632` | lazy field | one each | Knowledge |
| `derivedCache` | `:278` | `ConcurrentHashMap`, get-then-put | 8 facts (below) | mixed |
| `Reach.of` | `:264` | `ConcurrentHashMap` | every class asked | Work |
| `ClassLayouts.LayoutMemo` (3 tables) | `ClassLayouts.java:84-88,100,142` | `ConcurrentHashMap` | any `Type`: open | Work |
| `ModelBuilder.knowledge`, `directSubclasses`, `associationEndsByOwner`, `legacySetsById` | `ModelBuilder.java:668,695,826,1304` | lazy field | one each | Knowledge |
| `KnowledgeLayer.reach` | `KnowledgeLayer.java:135` | `ConcurrentHashMap` | every class asked | Knowledge |
| `KnowledgeLayer.derived` | `:68` | `HashMap` | 1 fact (`MappingClosures`, phase E) | Knowledge |

The 8 derived facts: `Reach`, `LayoutMemo`, `OpSeeds` (`MetamodelSeeds.java:92`), `Catalog` (`SqlIslands.java:97`), the
Cypher island's `Labels`, `Ends`, `GraphEdges` (`CypherIslands.java:257,356,358`), and **`SystemDatabase`
(`driver/SystemDatabase.java:111`), which holds JDBC sessions and has `synchronized routeFor`**. Every one but the last is
a pure function of the model. Rust cannot own that one.

**Per-query, not on the context.** `SpecCompiler.memo` is per `SpecCompiler` and `TypedQuery.java:32` makes one per query,
so **no `TypedSpec` lives on the model** despite the field's comment. Likewise `StoreResolver`, `ClassSources.memo`,
`Anchors`, `Lowerer`, `ObjectSites` (`TypedQuery.java:152,171`). The model tier holds only F-level things: `TypedClass`,
`TypedEnum`, `TypedFunction` (whose bodies are untyped `ValueSpecification`), and the records.

**Process-wide state.** 667 `static final` collections in scope; 10 are written outside a field initializer. Eight are
filled by a helper from a `static {}` block (`Pure.ALL*`, `Scalars.RULES`, `Windows.*`, `Aggregates.REDUCERS`,
`FeatureRules.UNDER`) and two **grow at runtime**: `NameResolver.BUILT` (`:624`) and `BareNames.REFERENTS` (`:169`), both
`ConcurrentHashMap`. `Compiler.BOOT` is a `ContentStore` with one key (the prelude and system metamodel, normalized once per
process); `ContentStore.java:63` computes inside its lock. The only other concurrency on this path is `HeldStore`'s
per-entry `synchronized` (`:110,128`) and a `ThreadLocal` instrument (`StampCensus.java:48`).

**Who shares a context.**

| Caller | Lifetime | Threads |
|---|---|---|
| HTTP server: `PureV1Api.Models.COMPILED` (`:127-131`) | one request | `LegendHttpServer.java:331` sets a null executor: the JDK's default, one dispatcher thread |
| Native library and browser: `wasm/…/planner/Boundary.java:178`, `HeldModels` | until the last holder releases | many, serialized by `HeldStore.with` per model |
| `Lean.run` (`Lean.java:84`, the end-to-end tests) | one call | `IqCorpusTest.java:87`'s pool runs each call on a pool thread, but each call compiles its own model |
| Static contexts in tests (`QueryParametersTest.java:28`, `LadderRender.java:128`) | the test class | one; I found no JUnit parallel configuration |
| `TypedQuery` | one query; several questions | the caller's |

**So no context is used by two threads at once anywhere in the tree.** `PureModelContext`'s own comment says "Not
thread-safe (plain maps)", and `HeldStore` exists for that reason. TRIAGE's "shared across threads" is true only of the
process-wide tables and of `HeldModels`, which serializes.

### 2.2 What constrains the design

- **Invariant 5, lazy user elements.** Typed elements reference each other by FQN. `findClass`/`findEnum`/`findFunction`
  are the only load points, and they materialize on demand. `NoEagerTypeReferencesTest` walks Java fields and
  `NoEagerUserClassLoadsTest` proxies the Java interface; neither can see a Rust context, so both retire with ledger rows
  and are re-expressed (§2.6).
- **Invariant 6, F does not trigger G.** The element compilers hold a `ModelBuilder`, not a context
  (`FunctionCompiler.java:25`, `TypeClassifier.java:25`, `ClassCompiler.java:27`).
- **Re-entrancy is the real constraint.** `derived` and the layout memo are get-then-put *because* a derivation reads
  another (`PureModelContext.java:271`, `ClassLayouts.java:80`). In Rust, re-entering `OnceLock::get_or_init` on a cell the
  same thread is initializing is an error (today, a deadlock); across *distinct* cells it is fine. The three element memos
  cannot reach themselves, since their compilers have no context to call back through. The layout and derived memos can
  reach each other, so they must compute outside any lock.
- **Overload rollback does not touch the model.** At HEAD the retry (`Overloads.java:693-705`) gives each candidate
  `typed.clone()` and a fresh `Bindings`. `Env` is immutable. Every frame is pushed and popped in `finally`
  (`InferenceKernel.java:887`, `TypeAnnotations.java:38`, `Overloads.java:312,382`, `StaticFold.java:310,369`). TRIAGE's
  "snapshot" citation `Typer:118` is `withPretyped`'s save-and-restore, not a typer snapshot. A memo filled during a failed
  candidate is still true. **D5 needs no rollback story.** Two counters are *not* undone (`AlphaRename.nrFresh`,
  `ColumnHygiene.fresh`) and a failed candidate advances them. Their names can reach output, so the Rust retry must advance
  them identically.
- **`letBindings` (D4) is per-resolver.** `StoreResolver.java:91` is an instance field of an object made per resolve,
  aliased to `TemporalFrame` (`:139`) and copied into nested resolvers (`:105`, `SubQueryLift.java:241`). It never touches
  the context.
- **The FFI.** Java will hold opaque handles across calls, and each call runs on the calling thread's worker. A handle's
  contents must therefore be `Send`, and `Sync` if two JVM threads may use one handle at once. `Rc`, `Cell` and `RefCell`
  are not `Sync`; `Arc`, `OnceLock` and `Mutex` are.

### 2.3 The options

**A. An immutable `Arc<Model>` with `OnceLock` cells.** A freeze step builds all Knowledge into plain fields. A `OnceLock`
is a cell written at most once; after that a read is one atomic load and hands back a plain `&T`. **B. Concurrent maps
with single-flight** (one computation per key, others wait), as `HandleStore.getOrOpenSlowly` does. **C. Per-request
copies**: a frozen shared core, each query with its own plain memos. **D. A single owner**: a thread per model taking
jobs, or the simplest form, one mutex per model held for a whole planning call. D is `HeldStore`'s semantics made
universal.

### 2.4 What each costs here

| | A: frozen + cells | B: concurrent + single-flight | C: per-request memos | D: single owner |
|---|---|---|---|---|
| Memos handled | all; closed keys in cells, open keys in a map | all | all, but per query | all, plain maps |
| Re-entrancy | safe by construction for element cells; open memos compute outside the lock | **a recursive derivation on a waited-for key deadlocks**, the reason Java avoided `computeIfAbsent` | none | none |
| Cross-query reuse | yes | yes | **none**: every query re-materializes what it touches. The layout memo was added for a measured cost (corpus 69 s → 83 s, `ClassLayouts.java:80`); how much a per-query memo recovers is unmeasured | yes |
| Borrow checker | `&Model` everywhere, no `&mut`; closures are unaffected | same | `&mut` memos threaded through 130K lines, or `RefCell` in a query object | `RefCell` inside, guarded by the outer lock |
| Lifetimes | `find_class` can return `&TypedClass`, tied to the model | values must be `Arc`'d out | as A | values `Arc`'d out |
| Concurrency | queries on one model run in parallel | yes | yes | **one at a time**; a runaway plan blocks the model, and Java's `Future.cancel` cannot interrupt native code |
| Dependencies | std only | a crates.io crate or hand-rolled; the workspace has **no** external crates today | std | std |
| FFI handle | `Box<Arc<Model>>`; any worker | same | same | a routed job per call (thread form) or lock (mutex form); a thread each reserves a 256 MB stack |
| Boot layer | `static OnceLock<Arc<Boot>>` | same | same | **still needs `Sync`** |

### 2.5 Recommendation

**Option A, with Knowledge frozen eagerly.**

```rust
pub struct Model {                                            // Send + Sync; shared as Arc<Model>; no &mut after freeze
    index: ModelIndex,                                        // ModelBuilder, with its lazy indexes and `packages` built at freeze
    boot: Arc<Boot>,
    classes: HashMap<Fqn, OnceLock<Result<TypedClass, Refusal>>>,      // keys fixed at freeze; likewise enums, functions
    reach: Memo<Fqn, Arc<Walked>>,                             // Memo = Mutex<HashMap>; compute unlocked, first writer wins
    layouts: LayoutMemos,                                      // three Memo<Type, _>
    declarations: OnceLock<DeclarationTable>,                  // one named OnceLock per derived fact, in place of Class<?> keys
    /* … */
}
```

1. **Freeze.** `ModelBuilder`'s four lazy indexes and `packages` are Knowledge, and the tenet says Knowledge is eager and
   total; they are single passes over the index. They become plain fields, which removes five of the writes in §2.1.
2. **Element memos are cells**, one per FQN known at freeze (the model's and the platform's). A failure is cached too:
   compilation is deterministic, so the same message comes back. Java recomputes it.
3. **Open-key memos are `Memo<K, V>`**: `Mutex<HashMap>`, lock to look, unlock to compute, lock to insert if absent. That
   is Java's get-then-put, so a duplicate computation of a pure function is possible and harmless, as `derived` accepts.
4. **Derived facts become named fields**, one `OnceLock` each; the open `Class<?>` key is a Java idiom. `SystemDatabase` stays
   in Java: the shim's model object keeps a Java-side map for the driver's facts.
5. **Per-query state is owned by the query** with no synchronization: `Typer`, `SpecCompiler`, `StoreResolver`, `Lowerer`
   borrow `&Model` for one call; counters are `Cell`s (TRIAGE's fix for the lowering closures).
6. **Two handle kinds.** A model handle holds `Arc<Model>`. A query handle holds its own `Arc<Model>` plus its typed body and
   memo, so `resultType` and `plan` are separate calls on one typed query (C2b). A model cannot be freed from under a query.
7. **Process-wide tables** become `LazyLock`, except `BUILT` and `REFERENTS` (`RwLock<HashMap>`); `Compiler.BOOT` is a
   `OnceLock`. Java's class-init order does not translate: a `LazyLock` reading itself through a cycle is an error, where
   Java would see a half-initialized class, so the port must check the static graph is acyclic.
8. **`HeldStore`'s per-model lock can go** on the Rust side; hold and release counting stays.

**Why.** The mutable surface is 17 fields behind one interface, so the choice is cheap to make well, and A is the only
option that costs nothing outside that interface: `&Model` everywhere, no `&mut` to thread, no lifetime on the query
handle, no dependency. It removes the one-at-a-time rule and gives lazy loading a direct test (is this cell set?).

**What would change it.** A dependency-backed loader (`ModelContext`'s stated future) makes the key sets open, and the
cells become `Memo`s, a local change. If materializing every element at freeze proves cheap on the largest model, the cells
can go altogether, but that also changes a stated tenet (`PureModelContext`'s "Work, lazy"), so it would be the owner's
call. If `Memo` contention shows in a profile, shard it or use `RwLock`.

### 2.6 Verification

| Check | How |
|---|---|
| Data races | **The compiler.** `const _: () = assert_send_sync::<Model>();`, and `#![forbid(unsafe_code)]` in the crate. No gate is needed for what safe Rust cannot express |
| Order independence and deadlocks | New: N threads, one model, the corpus queries, every answer byte-equal to the single-threaded run. Java has no such test (`HeldModelsTest` has no threads), since nothing could run that way |
| Lazy loading (`NoEagerUserClassLoadsTest`) | New Rust test, same model: after typing the three queries, the cold FQNs' cells are unset. Stricter than the proxy and needs no mock |
| No resolved references (`NoEagerTypeReferencesTest`) | By type: `find_class` returns `&TypedClass` tied to the model, so a field holding one needs a lifetime parameter and stands out. A long-lived `Arc<TypedClass>` cannot be obtained |
| F does not trigger G | **The crate graph**: the element crate does not depend on the typer. A compile error, stronger than today's convention. It needs `lite-core` split along `tools/deps/core-layers.txt` (§5) |
| End to end | The end-to-end suites through `Lean.run` and `Execution.run`, PCT, the corpora, `HeldModelsTest` through the shim |
| Handles | A `lite_live_models` count beside `lite_unfreed`, zero at the end of a test run |
| Re-entrancy | Unit tests over a cyclic hierarchy and over layouts that read layouts: they terminate |

## 3. How D1 and D5 fit together, and the order

- **D5 forces `Arc` over `Rc` in D1.** The model is `Sync`, so what it holds (function bodies, elements) must be.
- **`Origin` tokens cross the tiers**: a typed node (query) can hold an origin in a model function's body. An `Arc`-held
  token needs no lifetime, so nothing ties the tiers' lifetimes together.
- **Identity maps stay out of the model.** Every `IdMap` is per-query, apart from `CheckedLayer`, built once at boot.
- **D2.** A `Result`-returning typer with the retry loop as written needs no checkpoints: per-candidate state is already
  fresh. A frame that was popped in `finally` becomes a closure-scoped helper (`with_rigid(names, |t| …)`), because `?` returns
  early. `typed.clone()` is shallow under `Arc`.
- **D4.** "One owning context struct" works because nodes are not borrowed from it: with `Arc`, a method can take `&mut self`
  while holding nodes. An arena would have tied the two together.
- **The constraint on the FFI's surface.** The Java `driver` and `judge` that remain name `TypedSpec`, `SpecCompiler`,
  `UserCallInliner`, `StoreResolver` and `Lowerer` hundreds of times (for example `BodyDriver.java:579,2248`). The roadmap's
  "about 8 shim functions for `driver`" must carry those reads, or that logic moves. It needs its own short design.

**The order to build:**

0. While Java exists: the identity hit-count oracle, and the `MirJson` dump for the corpora.
1. `identity`, `Type`/`ExprType`. Flip the `ValueSpecification` family to `Arc` in the generator, once the parser work now in
   this worktree has landed. `parser_parity` and `round_trip_proof` hold it.
2. The model records, `ModelIndex` and freeze, name resolution, the boot statics and `builtin`.
3. The normalizer (E), then F: `Model` and its memos, with the laziness and concurrency tests.
4. `TypedSpec`, the typer (G, D2), and the inliner (G½). **Stop and measure here.**
5. The resolver (H, D4), lowering (I) on `lite-sql`, plan and planner; the corpora through the Rust CLI.
6. The shim, the switch, deleting the Java.

## 4. What I could not establish

- **Performance.** I built and ran nothing of the product, so no option has a measured cost. Every performance statement above
  is reasoning; §1.5 item 5 is where it gets measured.
- **The classification** of 430 comparisons. The (b), (c) and (e) lists were read site by site; the 357 (d) sites were read as
  idioms, not reasoned about singly.
- **Field-write census limits.** It sees writes through a field name, not through an alias passed to another method.
  `StoreResolver.letBindings` shared with `TemporalFrame` is one such alias, found by reading.
- **How much the Lowerer aliases SQL subtrees.** With `Box` each alias is a deep clone. If that shows in the first lowering slice,
  `lite-sql`'s `Box` fields can become `Arc` (a contained change, since the code reads through deref).
- **Whether `AlphaRename`'s `_nr<N>` names reach SQL** on a retried overload (I assumed they can); **what the "about 8 shim
  functions" are** (the roadmap does not list them); **the static init graph** for §2.5 item 7 (not traced); **why a
  single-threaded context uses `ConcurrentHashMap`**, beyond the recursion comments.

## 5. Questions for the owner

Each with my recommendation first.

1. **D1 mechanism. Enum of `Arc<Record>`, list fields `Arc<[T]>`, with pointer identity behind an `identity` module, not TRIAGE's NodeId.** Alternative:
   NodeId on `Box` trees, with a rule at about 1,900 construction sites. Do you accept the divergence from TRIAGE's suggestion?
2. **"Every node an id" (`docs/architecture/README.md` §7) does not bind iteration 1.** It belongs to the parked rebuild.
3. **The `ValueSpecification` family goes from `Box` to `Arc` in the `lite-front` generator, after the parser work lands.**
   Alternative: a second tree family built at name resolution, leaving `lite-front` alone.
4. **The SQL tree stays `Box` with `Rw`; measure in the first lowering slice.** Alternative: convert `lite-sql` to `Arc` now.
5. **`whereZones` keyed by content, its miss fallback kept for now** (one variable at a time; invariant 4 says it should
   become a refusal later). Alternative: carry zones beside the WHERE in the lowering.
6. **D5. Frozen `Arc<Model>`, `OnceLock` cells for closed keys, `Memo` for open keys, per-query state owned by the query.**
7. **Knowledge eager at freeze (the four lazy indexes and `packages`); element memos stay lazy (invariant 5). Yes to both.**
   Alternative for the second: materialize everything at freeze and have no cells, if measured cheap.
8. **Drop the per-model one-at-a-time rule in Rust. Yes.** Planning on one model becomes concurrent, which is new behaviour,
   covered by the new stress test.
9. **`SystemDatabase` stays in Java, in a map on the shim's model object. Yes.** Every other derived fact moves.
10. **Two handle kinds, each released on the big-stack worker, with a live-handle count as a gate. Yes.**
11. **Make the identity hit-count oracle before Java is deleted. Yes.** It cannot be made afterward.
12. **Split `lite-core` along `tools/deps/core-layers.txt`, so F-cannot-trigger-G is a compile error. Yes.** The roadmap names one.
13. **A short separate design for the `driver`/`judge` shim, before G½ and H are ported. Yes.** It could affect question 1.
