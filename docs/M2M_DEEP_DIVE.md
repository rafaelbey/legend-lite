# Deep Dive: M2M Mappings in legend-engine/legend-pure vs legend-lite

> Research doc: how legend-pure models model-to-model (M2M) mappings (§1) and how legend-engine connects and executes
> them (§2–3), from the upstream sources. Lite's deliberate differences are rows of `docs/SEMANTICS_REGISTER.md`: S27
> (the engine executes M2M in memory; lite renders SQL) and S21 (a class mapping with no `~src`).

> Cut in the 2026-11 docs cleanup: the era note of 2026-08-06 (it described the cut sections, written against the frozen `engine/com.gs.legend`). Full text: `git show de2745351:docs/M2M_DEEP_DIVE.md`.

---

## 1. Upstream Metamodel (legend-pure)

The canonical M2M class hierarchy in `platform_dsl_mapping/grammar/mapping.pure`:

```
SetImplementation                      (id, root, class, parent)
  └─ InstanceSetImplementation         (mappingClass, aggregateSpecification) + PropertyMappingsImplementation (stores, propertyMappings)
       └─ PureInstanceSetImplementation  (srcClass: Type[0..1], filter: LambdaFunction<Any>[0..1])

PropertyMapping                        (owner, targetSetImplementationId, sourceSetImplementationId, property, localMappingProperty, store)
  └─ PurePropertyMapping               (transform: LambdaFunction<Any>[1], transformer: ValueTransformer<Any>[0..1], explodeProperty: Boolean[0..1])
```

### Key Fields

| Field | Description |
|-------|-------------|
| `srcClass` | The source class (optional — `[0..1]`). When present, the mapping pulls data from instances of this class. |
| `filter` | Optional filter lambda on source instances (predicate over `$src`). |
| `transform` | The property mapping expression. A lambda `{src: SrcClass[1] \| ...}` computing one target property from `$src`. |
| `transformer` | Optional `EnumerationMapping` for enum-valued properties. |
| `explodeProperty` | For `*` multiplicity explosion (one-to-many from single source). |
| `targetSetImplementationId` | For class-typed properties: which `SetImplementation` maps the return type. |
| `sourceSetImplementationId` | Which set impl this property mapping belongs to (for disambiguation). |

### Processor Flow

`PureInstanceSetImplementationProcessor.process()`:
1. Resolves `srcClass` via `ImportStub`
2. For `filter`: adds `$src` param to lambda, then full-matches
3. For each `PurePropertyMapping`: adds `$src` param to `transform` lambda, full-matches
4. If `transformer` present (enum mapping), processes the `EnumerationMapping`

The `$src` parameter is **injected by the processor** — the grammar doesn't include it in the lambda signature. The processor creates a `VariableExpression` named `"src"` typed to `srcClass`.

### Validator Rules

`PureInstanceSetImplementationValidator`:
- Filter must return `Boolean`
- DataType properties: expression type must be compatible with property type
- Class-typed properties: `targetSetImplementationId` must exist; target set impl's `srcClass` must match expression return type
- Multiplicity: expression multiplicity must be subsumed by property multiplicity (unless `explodeProperty`)

---

## 2. Connection & Store Model (legend-engine)

### ModelStore

```pure
Class meta::external::store::model::ModelStore extends Store
{
  toString(){'ModelStore'}:String[1];
}
```

The "store" for M2M is a **phantom store** — it doesn't represent a real database. It's a marker that routes execution to the M2M engine.

### Connection Hierarchy

```
Connection
  └─ PureModelConnection (base)
       ├─ ModelConnection        (instances: Map<Class, List<Any>>)     — in-memory objects
       ├─ JsonModelConnection    (class: Class, url: String)            — JSON data via URL
       ├─ XmlModelConnection     (class: Class, url: String)            — XML data via URL
       └─ ModelQueryConnection   (instancesProvider: Map<Class, Func>)  — lazy providers
  └─ ModelChainConnection       (mappings: Mapping[*])                  — M2M chain
```

### Connection Resolution (Root Graph Fetch)

In `planRootGraphFetchExecutionInMemory`:
1. Gets `srcClass` from `PureInstanceSetImplementation.srcClass`
2. Searches runtime's `connectionStores` for a match:
   - `JsonModelConnection.class == srcClass` → use this connection
   - `ModelConnection.instances.get(srcClass).isNotEmpty()` → use this
   - `ModelChainConnection` → separate chain handling
3. Falls back to chain connection if no direct match

### Chain Execution Pattern

```pure
// Runtime setup for a 3-step chain: JSON → Mapping1 → Mapping2
^Runtime(connectionStores = [
    ^ConnectionStore(element=^ModelStore(),
        connection=^ModelChainConnection(mappings=[SrcToBridgeMapping])),
    ^ConnectionStore(element=^ModelStore(),
        connection=^JsonModelConnection(class=SourceClass, url='data:...'))
])
```

Execution via `executeChain()`:
1. `allReprocess()` rewrites the query through each mapping in the chain
2. For each mapping, the query's class references are rewritten to the **source** class
3. Property access (`$src.firstName`) is replaced with `eval(transformLambda, $srcExpr)`
4. The final query targets the last mapping in the chain
5. That query is routed to the terminal connection (e.g., `JsonModelConnection`)

---

## 3. Execution Modes

### In-Memory Execution (interpretive)

`executeInMemory()` / `inMemoryGetAll()`:
1. Get source instances from `ModelConnection.instances.get(srcClass)`
2. Apply filter if present
3. For each instance, call `transformWithMapping(src, impl, routedSpec, staticData)`
4. `transformWithMapping` applies each `PurePropertyMapping.transform` lambda to the source instance
5. For class-typed properties, recursively resolves the target mapping and transforms

This is **fully interpretive** — Pure lambdas are evaluated against live objects.

### Graph Fetch Execution Plan

`planRootGraphFetchExecutionInMemory()` creates:
- `StoreStreamReadingExecutionNode` — reads source data (JSON deserialization)
- `InMemoryRootGraphFetchExecutionNode` — applies M2M transforms
- `InMemoryPropertyGraphFetchExecutionNode` — nested property graph fetch

The Java execution engine deserializes JSON → objects → applies transforms → serializes result.

### Upstream facts found while building

- An M2M mapping can also be embedded (`M2MEmbeddedSetImplementation`) or a union/merge of set implementations
  (`OperationSetImplementation`).
- Over a `JsonModelConnection` the engine needs no mapping for the source class: it deserializes the JSON into
  instances of that class and `$src.prop` reads them. No SQL and no table are involved.

---

> Cut in the 2026-11 docs cleanup: §4–10, the comparison with the engine-era legend-lite (`engine/com.gs.legend`), the relation-space/object-space type-algebra brainstorm, the DuckDB 1.5 VARIANT design for JSON sources and its seven-phase implementation plan, all written before 2026-08-06. Full text: `git show de2745351:docs/M2M_DEEP_DIVE.md`.
