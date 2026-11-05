# `#Cypher{...}#` showcase: one Cypher query, Neo4j's answer, and legend-lite's DuckDB and Snowflake SQL

This page follows three Cypher queries of rising difficulty through legend-lite's `#Cypher{...}#` island
(`docs/CYPHER_STUDY_2026_10_15.md`, `docs/CYPHER_PLAN_2026_10_15.md`), each end to end: the graph, the model it
implies, Neo4j's answer, lite's answer, the Pure the island desugars to, and the SQL lite plans for DuckDB (run) and
for Snowflake (rendered). The medium one is §1 to §11 (verified 2026-10-15 against the translator on branch
`claude/keen-noether-v5mis0`, PR1b, the study's §12.2); the easy one is §17 and the hard one §18 (verified 2026-10-16,
after the study's §12.35). §19 (one Neo4j-judged case for each leg landed after §16) and §20 (the TCK ledger of
2026-10-17) were cut in the 2026-11 docs cleanup. Every commit that touched this page pinned the model, the Pure and the SQL and changed no translator code.

- **Accepted.** lite translates the query; it does not refuse it.
- **Correct.** lite's answer matches Neo4j Community 2026.09.0's cell for cell, in the query's
  order, on both parity lanes (DuckDB and the embedded Postgres 16). The judge is the Cypher parity harness's own
  (`CypherParity.outcome`), and its guards are clean. Neo4j's rows were snapped by the repo's oracle,
  `tools/neo4j-oracle`, under Cypher 25.
- **Pinned.** The model the graph implies, the Pure the island desugars to, and the SQL the query plans for DuckDB and
  for Snowflake are pinned byte for byte, so a change to any is seen and reviewed.
- **Snowflake.** The same query, compiled against a Snowflake connection, plans to the SQL in §8. That SQL is rendered
  only: nothing ran on Snowflake.

**Where it lives.**

- `core/src/test/resources/cypher-parity/lite/basics.yaml`: the query (`showcase_main`) and the 25 cases beside it, run
  by both Cypher parity lanes, `//core:cypher_parity_test` and `//core:cypher_parity_postgres_test`.
- `core/src/test/resources/cypher-parity/graphs/legend.cypher`: the graph (§2), which the oracle creates on Neo4j and
  dumps; the lanes seed their tables from the dump.
- `core/src/test/resources/cypher-parity/neo4j-answers.jsonl`, key `lite/basics/showcase_main`: Neo4j's answer,
  snapped by `tools/neo4j-oracle/snap.sh --write`; `ledger.tsv` holds the case's status (PASS on both lanes).
- `core/src/test/resources/cypher-parity/showcase/model.pure`, `pure.txt`, `duckdb.sql` and `snowflake.sql`: the
  pinned texts of the medium example, and `showcase/easy/` and `showcase/hard/` the same four for the easy and the hard
  ones (`lite/basics.yaml`'s `showcase_easy` and `showcase_hard`, over the same graph), all checked by
  `CypherParityTest.theShowcasePlansItsPinnedSql` in `//core:cypher_parity_test`. If one changes on purpose, rewrite
  them from the workspace root and update §4 to §8, §17 and §18 here:

  ```sh
  bazel test //core:cypher_parity_test --strategy=TestRunner=local \
    --test_env=JDK_JAVA_OPTIONS=-Dcypher.showcase.update=$PWD/core/src/test/resources/cypher-parity/showcase
  ```

## 1. The query

```
#Cypher{
MATCH (p:Person)-[:WORKS_AT]->(f:Firm)-[:HQ]->(c:City)
WHERE p.age IS NOT NULL AND c.name <> 'Nowhere'
WITH f.legalName AS firm, c.name AS city, count(*) AS headcount, avg(p.age) AS avgAge,
     min(p.age) AS youngest, max(p.age) AS oldest
WHERE headcount >= 1
RETURN firm, city, headcount, avgAge, youngest, oldest,
       CASE WHEN avgAge >= 32 THEN 'seasoned' ELSE 'young' END AS band
ORDER BY headcount DESC, firm
}#
```

Five people, three firms, two cities; who works where, and where each firm is based. For each firm with a headquarters,
the people of known age who work there: how many, their average, youngest and oldest age, and a band by the average.

## 2. The graph

The graph is written as Neo4j takes it (`graphs/legend.cypher`); the oracle creates it, and dumps it for the lanes:

```
CREATE (ny:City {name: 'New York'}),
       (ld:City {name: 'London'}),
       (acme:Firm {legalName: 'Acme', founded: 1990}),
       (globex:Firm {legalName: 'Globex'}),
       (initech:Firm {legalName: 'Initech', founded: 2001}),
       (alice:Person {name: 'Alice', age: 30, active: true}),
       (bob:Person {name: 'Bob', age: 40}),
       (charlie:Person {name: 'Charlie', age: 25, active: false}),
       (diana:Person {name: 'Diana', active: true}),
       (eve:Person {name: 'Eve', age: 35, active: true}),
       (acme)-[:HQ]->(ny),
       (globex)-[:HQ]->(ld),
       (alice)-[:WORKS_AT]->(acme),
       (bob)-[:WORKS_AT]->(acme),
       (charlie)-[:WORKS_AT]->(globex),
       (eve)-[:WORKS_AT]->(globex)
```

## 3. What it exercises

- Two hops in one pattern, `(p:Person)-[:WORKS_AT]->(f:Firm)-[:HQ]->(c:City)`, read from the root node `p` (each hop
  to-one from the node before it: `WORKS_AT` and `HQ` are foreign keys on the inferred tables), each hop a navigation
  (`$p.WORKS_AT.HQ.name`) with the condition that it finds a node.
- A `WHERE` with `IS NOT NULL` and a `<>` against a string: Cypher's three-valued logic, kept by `SqlToPure.Tx`'s
  spelling (the condition holds only where every operand is known).
- `WITH` with the implicit grouping of four aggregates (`count(*)`, `avg`, `min`, `max`) by two keys, a `WHERE` after
  it over the aggregate (a `HAVING`), and a `RETURN` that reads the projected columns, with a `CASE` over a nullable
  average and an `ORDER BY` on two keys (nulls first descending, last ascending: Neo4j's rule).

## 4. Neo4j 2026.09.0's answer

| firm | city | headcount | avgAge | youngest | oldest | band |
|---|---|---|---|---|---|---|
| 'Acme' | 'New York' | 2 | 35.0 | 30 | 40 | 'seasoned' |
| 'Globex' | 'London' | 2 | 30.0 | 25 | 35 | 'young' |

## 5. legend-lite's answer: identical

Cell for cell, in order, on both lanes (`ledger.tsv`: `lite/basics/showcase_main` PASS, PASS). The harness compares
the values in the TCK's value grammar: strings, integers, and floats within a relative tolerance of 1e-9.

## 6. The model the graph implies

The harness infers it from the dump (`CypherGraph`), the lanes compile it, and nothing hand-written is in it: a class
per label, a property per key typed by its values (every property optional, as a graph's are), each relationship type
an association (`g::R_<type>`, named apart from the classes) whose ends are named by the type (`WORKS_AT` holds the
firm, `WORKS_AT_in` the people), realized as a foreign key on the to-one side. The graph itself is a DataSpace,
`g::Graph`, listing every class and marked `<<graph::graph.Open>>` (the study's §12.45): each island opens with
`USE g::Graph`, and the graph is OPEN, as a Neo4j graph is, so a label, a relationship type or a property the graph
lacks reads as Neo4j reads it (no rows, null) rather than as a compile error.

```
###Pure
Class g::Duration
{
    months: Integer[1];
    days: Integer[1];
    seconds: Integer[1];
    nanoseconds: Integer[1];
}
Class g::City
{
    name: String[0..1];
}
Class g::Firm
{
    founded: Integer[0..1];
    legalName: String[0..1];
}
Class g::Person
{
    active: Boolean[0..1];
    age: Integer[0..1];
    name: String[0..1];
}
Association g::R_HQ
{
    HQ: g::City[0..1];
    HQ_in: g::Firm[*];
}
Association g::R_WORKS_AT
{
    WORKS_AT: g::Firm[0..1];
    WORKS_AT_in: g::Person[*];
}
###Relational
Database g::DB
(
    Table n_city (id BIGINT PRIMARY KEY, p_name VARCHAR(4000))
    Table n_firm (id BIGINT PRIMARY KEY, p_founded BIGINT, p_legalname VARCHAR(4000), r_hq BIGINT)
    Table n_person (id BIGINT PRIMARY KEY, p_active BIT, p_age BIGINT, p_name VARCHAR(4000), r_works_at BIGINT)
    Join HQ(n_firm.r_hq = n_city.id)
    Join WORKS_AT(n_person.r_works_at = n_firm.id)
)
###Mapping
Mapping g::Map
(
    g::City: Relational
    {
        ~mainTable [g::DB] n_city
        name: [g::DB] n_city.p_name
    }
    g::Firm: Relational
    {
        ~mainTable [g::DB] n_firm
        founded: [g::DB] n_firm.p_founded,
        legalName: [g::DB] n_firm.p_legalname
    }
    g::Person: Relational
    {
        ~mainTable [g::DB] n_person
        active: [g::DB] n_person.p_active,
        age: [g::DB] n_person.p_age,
        name: [g::DB] n_person.p_name
    }
    g::R_HQ: Relational { AssociationMapping ( HQ: [g::DB]@HQ, HQ_in: [g::DB]@HQ ) }
    g::R_WORKS_AT: Relational { AssociationMapping ( WORKS_AT: [g::DB]@WORKS_AT, WORKS_AT_in: [g::DB]@WORKS_AT ) }
)
```

## 7. The Pure the island desugars to

`compiler/spec/CypherIslands.translate`, printed as a person writes it (`pure.txt`; the lane runs it with the
mapping and runtime appended to the model):

```
g::Person.all()->filter(p|($p.WORKS_AT->isNotEmpty() && $p.WORKS_AT.HQ->isNotEmpty()) && ($p.age->isNotEmpty() && ($p.WORKS_AT.HQ.name->isNotEmpty() && ($p.WORKS_AT.HQ.name->toOne() != 'Nowhere'))))
    ->project(~[firm:p|$p.WORKS_AT.legalName, city:p|$p.WORKS_AT.HQ.name, _cypher_1:p|1, _cypher_2:p|$p.age, _cypher_3:p|$p.age, _cypher_4:p|$p.age])
    ->groupBy(~[firm, city], ~[headcount:x|$x._cypher_1:y|$y->count(), avgAge:x|$x._cypher_2:y|$y->average(), youngest:x|$x._cypher_3:y|$y->min(), oldest:x|$x._cypher_4:y|$y->max()])
    ->select(~[firm, city, headcount, avgAge, youngest, oldest])
    ->filter(r|$r.headcount >= 1)
    ->extend(~[band:r|if($r.avgAge->isNotEmpty() && ($r.avgAge->toOne() >= 32), |'seasoned', |'young')])
    ->select(~[firm, city, headcount, avgAge, youngest, oldest, band])
    ->sort([~headcount->descending(meta::pure::functions::relation::NullOrder.FIRST), ~firm->ascending(meta::pure::functions::relation::NullOrder.LAST)])
```

How the Cypher lands in it: the pattern's root is `Person` (`all()`); each hop that may find nothing adds an
`isNotEmpty` to the filter; `WHERE`'s `IS NOT NULL` is `isNotEmpty`, its `<>` a strict test that holds only where the
name is known; `WITH`'s aggregates project their arguments first (`_cypher_n`) and `groupBy` by the two keys, a `select`
restoring the items' order; the `WHERE` after `WITH` is a `filter` over the grouped relation; `RETURN`'s `CASE` is an
`extend` whose `if` tests the average where it is known; the `ORDER BY` is a `sort` with Neo4j's null order.

## 8. The SQL for DuckDB (rendered and run)

```sql
SELECT t8.firm, t8.city, t8.headcount, t8.avgAge, t8.youngest, t8.oldest, CASE WHEN t8.avgAge IS NOT NULL AND t8.avgAge >= 32 THEN 'seasoned' ELSE 'young' END AS band
FROM (
  SELECT t3.p_legalname AS firm, t3.HQ_p_name AS city, COUNT(1) AS headcount, AVG(1.0 * t0.p_age) AS avgAge, MIN(t0.p_age) AS youngest, MAX(t0.p_age) AS oldest
  FROM n_person AS t0
  LEFT OUTER JOIN (
    SELECT t1.id AS id, t1.p_legalname AS p_legalname, t1.r_hq AS r_hq, t2.p_name AS HQ_p_name
    FROM n_firm AS t1
    LEFT OUTER JOIN n_city AS t2 ON t1.r_hq = t2.id
  ) AS t3 ON t0.r_works_at = t3.id
  LEFT OUTER JOIN (
    SELECT DISTINCT t4.id
    FROM n_firm AS t4
  ) AS t6 ON t0.r_works_at = t6.id
  LEFT OUTER JOIN (
    SELECT DISTINCT t5.id
    FROM n_city AS t5
  ) AS t7 ON t3.r_hq = t7.id
  WHERE t6.id IS NOT NULL AND t7.id IS NOT NULL AND t0.p_age IS NOT NULL AND t3.HQ_p_name IS NOT NULL AND (t3.HQ_p_name IS DISTINCT FROM 'Nowhere')
  GROUP BY t3.p_legalname, t3.HQ_p_name
  HAVING COUNT(1) >= 1
) AS t8
ORDER BY t8.headcount DESC NULLS FIRST, t8.firm NULLS LAST
```

## 9. The SQL for Snowflake (rendered, not run)

```sql
SELECT "t8"."firm" AS "firm", "t8"."city" AS "city", "t8"."headcount" AS "headcount", "t8"."avgAge" AS "avgAge", "t8"."youngest" AS "youngest", "t8"."oldest" AS "oldest", CASE WHEN "t8"."avgAge" IS NOT NULL AND "t8"."avgAge" >= 32 THEN 'seasoned' ELSE 'young' END AS "band"
FROM (
  SELECT "t3"."p_legalname" AS "firm", "t3"."HQ_p_name" AS "city", COUNT(1) AS "headcount", CAST(AVG(CAST(1.0 AS NUMBER(2,1)) * "t0"."p_age") AS DOUBLE) AS "avgAge", MIN("t0"."p_age") AS "youngest", MAX("t0"."p_age") AS "oldest"
  FROM "n_person" AS "t0"
  LEFT OUTER JOIN (
    SELECT "t1"."id" AS "id", "t1"."p_legalname" AS "p_legalname", "t1"."r_hq" AS "r_hq", "t2"."p_name" AS "HQ_p_name"
    FROM "n_firm" AS "t1"
    LEFT OUTER JOIN "n_city" AS "t2" ON "t1"."r_hq" = "t2"."id"
  ) AS "t3" ON "t0"."r_works_at" = "t3"."id"
  LEFT OUTER JOIN (
    SELECT DISTINCT "t4"."id" AS "id"
    FROM "n_firm" AS "t4"
  ) AS "t6" ON "t0"."r_works_at" = "t6"."id"
  LEFT OUTER JOIN (
    SELECT DISTINCT "t5"."id" AS "id"
    FROM "n_city" AS "t5"
  ) AS "t7" ON "t3"."r_hq" = "t7"."id"
  WHERE "t6"."id" IS NOT NULL AND "t7"."id" IS NOT NULL AND "t0"."p_age" IS NOT NULL AND "t3"."HQ_p_name" IS NOT NULL AND ("t3"."HQ_p_name" IS DISTINCT FROM 'Nowhere')
  GROUP BY "t3"."p_legalname", "t3"."HQ_p_name"
  HAVING COUNT(1) >= 1
) AS "t8"
ORDER BY "t8"."headcount" DESC NULLS FIRST, "t8"."firm" NULLS LAST
```

## 10. What lite refused on the way

Nothing in this query. The cases beside it in `lite/basics.yaml` show the refusals by name: `RETURN p` (a node as a
value), `UNWIND` (lists), a property no class declares (C4: lite's compile error where Neo4j answers `null`,
`docs/SEMANTICS_REGISTER.md` S58).

## 11. How it was checked, and the limits

- The oracle ran the graph and the query on Neo4j Community 2026.09.0 (`snap.sh --write`), and `--check` reports no
  drift; the lanes judged lite's rows against the snapped answer on DuckDB and on Postgres 16.
- The SQL is lite's own for its DuckDB and Snowflake dialects; Snowflake's is rendered only.
- The query reads to-one hops only: a pattern that fans out (a firm's many employees read from the firm), lists,
  `OPTIONAL MATCH`, nodes as values and the graph algorithms are the later slices of the plan (`docs/CYPHER_PLAN_2026_10_15.md` §4).

## 12. What PR2 brings: lists, maps, the quantifiers, `UNWIND` (2026-10-15)

The values slice (the study's §12.5) reads lists and maps of known shape, the four quantifiers, comprehensions,
indexing and slicing, `IN` over lists, `WITH` as a first clause, `UNWIND`, lists and maps as returned values, and a
function table. Three of its 28 Neo4j-judged cases (`lite/values.yaml`; `ledger.tsv`: PASS on both lanes):

**Lists built from the rows** (`returned_lists_over_rows`), the database writing each as a Variant (`toVariant` of a
collection; a map built from the rows is refused by name until lite's Postgres lowering writes `newMap` as an
object, the study's §12.5.5):

```
MATCH (p:Person)
RETURN p.name AS name, [p.age IS NULL, p.active IS NULL] AS unknowns,
       [coalesce(p.name, '?'), coalesce(p.name, '?') + '!'] AS names
ORDER BY name
```

| name | unknowns | names |
|---|---|---|
| 'Alice' | [false, false] | ['Alice', 'Alice!'] |
| 'Bob' | [false, true] | ['Bob', 'Bob!'] |
| 'Charlie' | [false, false] | ['Charlie', 'Charlie!'] |
| 'Diana' | [true, false] | ['Diana', 'Diana!'] |
| 'Eve' | [false, false] | ['Eve', 'Eve!'] |

**The quantifiers under Cypher's three-valued rules** (`quantifiers_three_valued`), each unrolled over the spelled
list, the predicate computed by the database:

```
RETURN all(x IN [1, null] WHERE x > 0) AS a, any(x IN [null, 2] WHERE x = 2) AS b,
       none(x IN [1, 2] WHERE x = 3) AS c, single(x IN [2, null] WHERE x = 2) AS s,
       single(x IN [34, 0, null, 5, 900] WHERE x < 10) AS t,
       single(x IN [34, 10, null, 15, 900] WHERE x < 10) AS u, all(x IN [] WHERE false) AS e
```

| a | b | c | s | t | u | e |
|---|---|---|---|---|---|---|
| null | true | true | null | false | null | true |

**`UNWIND` after a projection** (`unwind_after_with_over_rows`), the rows in hand paired with the list's elements
(Pure's `lateral` of a `flatten`):

```
MATCH (p:Person) WITH p.name AS name UNWIND [1, 2] AS k RETURN name, k ORDER BY name, k
```

Ten rows, `'Alice' 1`, `'Alice' 2`, ... `'Eve' 2`, identical to Neo4j's on both lanes. What PR2 refuses by name, and
what the census says comes next (the temporal values, the join form, `collect`), is the study's §12.5.3 and §12.5.4.

## 13. What the temporal slice brings: dates and local date-times (2026-10-16)

Cypher's `date` is Pure's StrictDate and `localdatetime` Pure's DateTime (the study's §12.6); a zoned
`datetime`, a `time` and a `duration` are refused by name until a Pure type carries them. One of its 16 Neo4j-judged
cases (`lite/temporal.yaml`, `stored_dates`; `ledger.tsv`: PASS on both lanes), over a graph whose nodes hold a date
and a local date-time:

```
CREATE (:Val {name: 'a', d: date('1984-10-11'), dt: localdatetime('1984-10-11T12:31:14.645')}),
       (:Val {name: 'b', d: date('2015-06-24'), dt: localdatetime('2016-07-21T21:45:22.142')})
```

```
MATCH (v:Val)
RETURN v.name AS name, v.d AS d, v.d.year AS y, v.dt AS dt, v.dt.month AS m,
       v.d < date('2000-01-01') AS early, date.truncate('month', v.dt) AS t
ORDER BY name
```

| name | d | y | dt | m | early | t |
|---|---|---|---|---|---|---|
| 'a' | 1984-10-11 | 1984 | 1984-10-11T12:31:14.645 | 10 | true | 1984-10-01 |
| 'b' | 2015-06-24 | 2015 | 2016-07-21T21:45:22.142 | 7 | false | 2016-07-01 |

The inferred model types `d` as StrictDate (a DATE column) and `dt` as DateTime (a TIMESTAMP); the island reads
`v.d.year` as Pure's `year`, the comparison as the database's, the truncation as `firstDayOfMonth`, and the judge
reads each date cell as Neo4j's driver spells one. What the slice refuses by name is the study's §12.6.5.

## 14. What the time-of-day leg brings: `localtime` over a SQL TIME (2026-10-16)

Cypher's `localtime` is Pure's StrictTime, which lite now lowers to the database's TIME (the study's §12.7: Pure
declares the type and only prints it, legend-engine refuses it in SQL, so this is lite's own). One of the leg's
Neo4j-judged cases (`lite/temporal.yaml`, `stored_datetimes_times`; `ledger.tsv`: PASS on both lanes), over the
graph whose nodes hold a local date-time:

```
MATCH (v:Val)
RETURN v.name AS name, localtime(v.dt) AS t, localtime(v.dt).hour AS h,
       localtime(v.dt) < localtime('13:00') AS early, localtime.truncate('hour', v.dt) AS th
ORDER BY name
```

| name | t | h | early | th |
|---|---|---|---|---|
| 'a' | 12:31:14.645 | 12 | true | 12:00:00 |
| 'b' | 21:45:22.142 | 21 | false | 21:00:00 |

`localtime(v.dt)` is lite's `timeOf` (a CAST to TIME); `localtime('13:00')` is Pure's `%13:00` literal, a
`TIME '13:00:00'`; the field, the comparison and the truncation read a time through the date-time at it on
1970-01-01 (lite's `atTime`: the date is a token the translator spells, the database computes every value), and
`timeOf` takes the time back. The judge reads a TIME cell as Neo4j's driver spells a local time. A time of day is
never stored: the store grammar has no TIME column (`docs/SEMANTICS_REGISTER.md` S64).

## 15. What the zoned date-time leg brings: `datetime` as an instant, its zone beside it (2026-10-16)

Cypher's `datetime` is Pure's DateTime, the instant — upstream's model: a zone is not kept with the value — and the
island carries the zone beside the value while it can see it (the study's §12.8). One of the leg's Neo4j-judged
cases (`lite/temporal.yaml`, `stored_datetimes_zoned`; `ledger.tsv`: PASS on both lanes), over the graph whose
nodes hold a local date-time:

```
MATCH (v:Val)
RETURN v.name AS name, datetime(v.dt) AS z, datetime({datetime: v.dt, timezone: '+02:00'}) AS z2,
       datetime({datetime: v.dt, timezone: '+02:00'}).hour AS h,
       datetime({datetime: v.dt, timezone: 'Europe/Stockholm'}).offsetMinutes AS om
ORDER BY name
```

| name | z | z2 | h | om |
|---|---|---|---|---|
| 'a' | 1984-10-11T12:31:14.645Z | 1984-10-11T12:31:14.645+02:00 | 12 | 60 |
| 'b' | 2016-07-21T21:45:22.142Z | 2016-07-21T21:45:22.142+02:00 | 21 | 120 |

`z` is the stored local date-time read at the default zone, UTC; `z2` its wall time in `+02:00`, the instant the
database computes (`adjust(-2, HOURS)`), the zone a fact the translator keeps, so `h` reads the hour back on the
wall time; `om` is the database's distance between the instant and its Stockholm wall time (winter 60, summer 120:
DuckDB's ICU and Postgres's `AT TIME ZONE` through lite's `localIn`). The judge equates Neo4j's zoned text with the
instant lite returns (`docs/SEMANTICS_REGISTER.md` S65); Neo4j's `=` between zones is the decision S66.

## 16. What the duration leg brings: a duration as a class of the model (2026-10-16)

A Cypher duration is an instance of a class the model declares — `Duration` with `months`, `days`, `seconds` and
`nanoseconds` as `Integer[1]` (the study's §12.9: the user's question, answered: a user class, nothing in the
platform) — carried as the database's struct. One of the leg's Neo4j-judged cases (`lite/temporal.yaml`,
`stored_dates_with_durations`; `ledger.tsv`: PASS on both lanes), over the graph whose nodes hold a date:

```
MATCH (v:Val)
RETURN v.name AS name, v.d + duration({months: 1}) AS m,
       duration.between(v.d, date('2020-01-01')).months AS mo,
       duration.between(v.d, date('2020-01-01')) AS dur
ORDER BY name
```

| name | m | mo | dur |
|---|---|---|---|
| 'a' | 1984-11-11 | 422 | P35Y2M21D |
| 'b' | 2015-07-24 | 54 | P4Y6M8D |

`v.d + duration({months: 1})` is Pure's `adjust` by the duration's months and days (a date takes no seconds);
`duration.between` is Neo4j's split as the database's — the whole months (the calendar count, stepped back when
stepping it overshoots), the whole days from there, the rest in seconds — written into `^g::Duration(months=…,
days=…, seconds=…, nanoseconds=…)`; `.months` reads the slot. The judge sums Neo4j's `P35Y2M21D` into the four slots
(`docs/SEMANTICS_REGISTER.md` S67); `toString` of a duration is refused by name.

## 17. The easy example, end to end (2026-10-16)

One node, a filter, a projection with `coalesce`, a sort (`lite/basics.yaml`, `showcase_easy`; `ledger.tsv`: PASS on
both lanes). Over the graph of §2:

```
MATCH (p:Person)
WHERE p.age > 28
RETURN p.name AS name, p.age AS age, coalesce(p.active, false) AS active
ORDER BY age DESC
```

**Neo4j 2026.09.0's answer, and lite's, cell for cell:**

| name | age | active |
|---|---|---|
| 'Bob' | 40 | false |
| 'Eve' | 35 | true |
| 'Alice' | 30 | true |

**The model** is §6's (the graph is the same; `showcase/easy/model.pure` is pinned and identical to `showcase/model.pure`).

**The Pure** (`showcase/easy/pure.txt`):

```
g::Person.all()->filter(p|$p.age->isNotEmpty() && ($p.age->toOne() > 28))->project(~[name:p|$p.name, age:p|$p.age, active:p|if(if($p.active->isNotEmpty() && $p.active->toOne(), |true, |if($p.active->isNotEmpty() && !$p.active->toOne(), |false, |[]))->isEmpty(), |false, |if($p.active->isNotEmpty() && $p.active->toOne(), |true, |if($p.active->isNotEmpty() && !$p.active->toOne(), |false, |[])))->toOne()])->sort([~age->descending(meta::pure::functions::relation::NullOrder.FIRST)])
```

The `WHERE` is a `filter` whose condition holds only where `age` is known (`isNotEmpty`, then the comparison over
`toOne`); `coalesce` over the optional boolean is an `if` that answers `false` where the three-valued `active` is
null; `ORDER BY age DESC` is a `sort` with Neo4j's null order (nulls first descending).

**The SQL for DuckDB** (`showcase/easy/duckdb.sql`, rendered and run):

```sql
SELECT t0.p_name AS name, t0.p_age AS age, CASE WHEN CASE WHEN t0.p_active IS NOT NULL AND t0.p_active THEN TRUE WHEN t0.p_active IS NOT NULL AND NOT t0.p_active THEN FALSE ELSE NULL END IS NULL THEN FALSE WHEN t0.p_active IS NOT NULL AND t0.p_active THEN TRUE WHEN t0.p_active IS NOT NULL AND NOT t0.p_active THEN FALSE ELSE NULL END AS active
FROM n_person AS t0
WHERE t0.p_age IS NOT NULL AND t0.p_age IS NOT NULL AND t0.p_age > 28
ORDER BY t0.p_age DESC NULLS FIRST
```

**The SQL for Snowflake** (`showcase/easy/snowflake.sql`, rendered only):

```sql
SELECT "t0"."p_name" AS "name", "t0"."p_age" AS "age", CASE WHEN CASE WHEN "t0"."p_active" IS NOT NULL AND "t0"."p_active" THEN TRUE WHEN "t0"."p_active" IS NOT NULL AND NOT "t0"."p_active" THEN FALSE ELSE NULL END IS NULL THEN FALSE WHEN "t0"."p_active" IS NOT NULL AND "t0"."p_active" THEN TRUE WHEN "t0"."p_active" IS NOT NULL AND NOT "t0"."p_active" THEN FALSE ELSE NULL END AS "active"
FROM "n_person" AS "t0"
WHERE "t0"."p_age" IS NOT NULL AND "t0"."p_age" IS NOT NULL AND "t0"."p_age" > 28
ORDER BY "t0"."p_age" DESC NULLS FIRST
```

## 18. The hard example, end to end (2026-10-16)

An optional hop, a `WITH` that keeps the graph, two pattern comprehensions (one sized, one filtered and listed), a
three-valued boolean, a `CASE` over an absent node (`lite/basics.yaml`, `showcase_hard`; `ledger.tsv`: PASS on both
lanes). Over the graph of §2:

```
MATCH (f:Firm)
OPTIONAL MATCH (f)-[:HQ]->(c:City)
WITH f, c
RETURN f.legalName AS firm, c.name AS city, c IS NULL AS noHq,
       size([(f)<-[:WORKS_AT]-(p) | p]) AS headcount,
       [(f)<-[:WORKS_AT]-(p) WHERE p.age > 35 | p.name] AS seniors,
       f.founded IS NOT NULL AND f.founded < 2000 AS old,
       CASE WHEN c IS NULL THEN 'no hq' ELSE 'in ' + c.name END AS where
ORDER BY firm
```

What it draws on: `OPTIONAL MATCH` (the study's §12.18: the city absent where the firm has no `HQ`), a `WITH` of bare
variables keeping the graph (§12.5), `c IS NULL` over an absent node (§12.18), pattern comprehensions over the
inverse end `WORKS_AT_in` (§12.31: `size` as the matches counted, the filtered list as a collection the database
computes), a three-valued `AND` over an optional property (§12.3), a `CASE` whose branch reads the optional node's
property (§12.3).

**Neo4j 2026.09.0's answer, and lite's, cell for cell:**

| firm | city | noHq | headcount | seniors | old | where |
|---|---|---|---|---|---|---|
| 'Acme' | 'New York' | false | 2 | ['Bob'] | true | 'in New York' |
| 'Globex' | 'London' | false | 2 | [] | false | 'in London' |
| 'Initech' | null | true | 0 | [] | false | 'no hq' |

**The model** is §6's (`showcase/hard/model.pure`, identical to `showcase/model.pure`).

**The Pure** (`showcase/hard/pure.txt`):

```
g::Firm.all()->project(~[firm:f|$f.legalName, city:f|$f.HQ.name, noHq:f|!if($f.HQ->isEmpty(), |[]->cast(@meta::pure::metamodel::variant::Variant), |fromJson((('{"@labels":' + '["City"]') + if(if($f.HQ->isEmpty(), |[]->cast(@String), |$f.HQ->meta::legend::lite::elementId())->isEmpty(), |'', |',"@id":' + ('"' + if($f.HQ->isEmpty(), |[]->cast(@String), |$f.HQ->meta::legend::lite::elementId())->toOne()->replace('\\', '\\\\')->replace('"', '\\"') + '"'))) + if($f.HQ.name->isEmpty(), |'', |',"name":' + ('"' + $f.HQ.name->toOne()->replace('\\', '\\\\')->replace('"', '\\"') + '"')) + '}'))->isNotEmpty(), headcount:f|$f.WORKS_AT_in->size(), seniors:f|fromJson('[' + $f.WORKS_AT_in->filter(_cypher_1|$_cypher_1.age->isNotEmpty() && ($_cypher_1.age->toOne() > 35))->map(_cypher_1|',' + if($_cypher_1.name->isEmpty(), |'null', |'"' + $_cypher_1.name->toOne()->replace('\\', '\\\\')->replace('"', '\\"') + '"'))->joinStrings('')->substring(2) + ']'), old:f|if($f.founded->isNotEmpty() && ($f.founded->isNotEmpty() && ($f.founded->toOne() < 2000)), |true, |if(!$f.founded->isNotEmpty() || ($f.founded->isNotEmpty() && !($f.founded->toOne() < 2000)), |false, |[])), where:f|if(!if($f.HQ->isEmpty(), |[]->cast(@meta::pure::metamodel::variant::Variant), |fromJson((('{"@labels":' + '["City"]') + if(if($f.HQ->isEmpty(), |[]->cast(@String), |$f.HQ->meta::legend::lite::elementId())->isEmpty(), |'', |',"@id":' + ('"' + if($f.HQ->isEmpty(), |[]->cast(@String), |$f.HQ->meta::legend::lite::elementId())->toOne()->replace('\\', '\\\\')->replace('"', '\\"') + '"'))) + if($f.HQ.name->isEmpty(), |'', |',"name":' + ('"' + $f.HQ.name->toOne()->replace('\\', '\\\\')->replace('"', '\\"') + '"')) + '}'))->isNotEmpty(), |'no hq', |if($f.HQ.name->isEmpty(), |[], |'in ' + $f.HQ.name->toOne()))])->sort([~firm->ascending(meta::pure::functions::relation::NullOrder.LAST)])
```

How the Cypher lands in it: the root is `Firm` (`all()`); the optional hop is the navigation `$f.HQ` with no
condition that it finds a node, so `city` is `$f.HQ.name` (null where there is no city); `c IS NULL` is the node's
value tested for emptiness, the value being null where `$f.HQ` is empty; `size([...])` is `$f.WORKS_AT_in->size()`
(the inverse end of `WORKS_AT`, the people of the firm); the filtered comprehension is `filter` then `map` over that
navigation, each name's JSON text led by a comma, joined, the first comma cut and the whole read as JSON; `old` is a
three-valued condition (`if ... true ... false ... []`); the `CASE` is an `if` over the same emptiness test.

**The SQL for DuckDB** (`showcase/hard/duckdb.sql`, rendered and run):

```sql
SELECT t0.p_legalname AS firm, t1.p_name AS city, NOT CASE WHEN NOT EXISTS (SELECT * FROM n_city AS t6 WHERE t0.r_hq = t6.id) THEN CAST(NULL AS JSON) ELSE CAST(concat('{"@labels":', '["City"]', CASE WHEN CASE WHEN NOT EXISTS (SELECT * FROM n_city AS t7 WHERE t0.r_hq = t7.id) THEN NULL WHEN t1.id IS NOT NULL THEN coalesce(list_aggregate(list_filter(['g::City:', CAST(t1.id AS VARCHAR)], x -> x IS NOT NULL), 'string_agg', ''), '') ELSE NULL END IS NULL THEN '' ELSE concat(',"@id":', '"', replace(replace(CASE WHEN NOT EXISTS (SELECT * FROM n_city AS t8 WHERE t0.r_hq = t8.id) THEN NULL WHEN t1.id IS NOT NULL THEN coalesce(list_aggregate(list_filter(['g::City:', CAST(t1.id AS VARCHAR)], x -> x IS NOT NULL), 'string_agg', ''), '') ELSE NULL END, '\', '\\'), '"', '\"'), '"') END, CASE WHEN t1.p_name IS NULL THEN '' ELSE concat(',"name":', '"', replace(replace(t1.p_name, '\', '\\'), '"', '\"'), '"') END, '}') AS JSON) END IS NOT NULL AS noHq, CASE WHEN t3.agg_0 IS NOT NULL THEN t3.agg_0 ELSE 0 END AS headcount, CAST(concat('[', substr(t5.agg_0, 2), ']') AS JSON) AS seniors, CASE WHEN t0.p_founded IS NOT NULL AND t0.p_founded IS NOT NULL AND t0.p_founded IS NOT NULL AND t0.p_founded < 2000 THEN TRUE WHEN NOT t0.p_founded IS NOT NULL OR t0.p_founded IS NOT NULL AND NOT (t0.p_founded IS NOT NULL AND t0.p_founded < 2000) THEN FALSE ELSE NULL END AS old, CASE WHEN NOT CASE WHEN NOT EXISTS (SELECT * FROM n_city AS t9 WHERE t0.r_hq = t9.id) THEN CAST(NULL AS JSON) ELSE CAST(concat('{"@labels":', '["City"]', CASE WHEN CASE WHEN NOT EXISTS (SELECT * FROM n_city AS t10 WHERE t0.r_hq = t10.id) THEN NULL WHEN t1.id IS NOT NULL THEN coalesce(list_aggregate(list_filter(['g::City:', CAST(t1.id AS VARCHAR)], x -> x IS NOT NULL), 'string_agg', ''), '') ELSE NULL END IS NULL THEN '' ELSE concat(',"@id":', '"', replace(replace(CASE WHEN NOT EXISTS (SELECT * FROM n_city AS t11 WHERE t0.r_hq = t11.id) THEN NULL WHEN t1.id IS NOT NULL THEN coalesce(list_aggregate(list_filter(['g::City:', CAST(t1.id AS VARCHAR)], x -> x IS NOT NULL), 'string_agg', ''), '') ELSE NULL END, '\', '\\'), '"', '\"'), '"') END, CASE WHEN t1.p_name IS NULL THEN '' ELSE concat(',"name":', '"', replace(replace(t1.p_name, '\', '\\'), '"', '\"'), '"') END, '}') AS JSON) END IS NOT NULL THEN 'no hq' WHEN t1.p_name IS NULL THEN NULL ELSE concat('in ', t1.p_name) END AS "where"
FROM n_firm AS t0
LEFT OUTER JOIN n_city AS t1 ON t0.r_hq = t1.id
LEFT OUTER JOIN (
  SELECT t2.r_works_at, COUNT(*) AS agg_0
  FROM n_person AS t2
  GROUP BY t2.r_works_at
) AS t3 ON t3.r_works_at = t0.id
LEFT OUTER JOIN (
  SELECT t4.r_works_at, STRING_AGG(concat(',', CASE WHEN t4.p_name IS NULL THEN 'null' ELSE concat('"', replace(replace(t4.p_name, '\', '\\'), '"', '\"'), '"') END), '') AS agg_0
  FROM n_person AS t4
  WHERE t4.p_age IS NOT NULL AND t4.p_age IS NOT NULL AND t4.p_age > 35
  GROUP BY t4.r_works_at
) AS t5 ON t5.r_works_at = t0.id
ORDER BY t0.p_legalname NULLS LAST
```

The optional hop is a `LEFT OUTER JOIN` on the foreign key, with an `EXISTS` for "the city is there"; `headcount` is a
grouped `COUNT(*)` over the people, keyed by their firm, joined back and read as 0 where none; `seniors` is a
`STRING_AGG` of the JSON pieces over the people older than 35, keyed the same way, cut to a JSON array; the booleans are
`CASE` chains that keep Cypher's unknown.

**The SQL for Snowflake** (`showcase/hard/snowflake.sql`, rendered only):

```sql
SELECT "t0"."p_legalname" AS "firm", "t1"."p_name" AS "city", NOT CASE WHEN NOT EXISTS (SELECT * FROM "n_city" AS "t6" WHERE "t0"."r_hq" = "t6"."id") THEN TO_VARIANT(NULL) ELSE PARSE_JSON(('{"@labels":' || '["City"]' || COALESCE(CASE WHEN CASE WHEN NOT EXISTS (SELECT * FROM "n_city" AS "t7" WHERE "t0"."r_hq" = "t7"."id") THEN NULL WHEN "t1"."id" IS NOT NULL THEN coalesce((CASE WHEN ARRAY_SIZE(ARRAY_COMPACT(ARRAY_COMPACT(ARRAY_CONSTRUCT('g::City:', CAST("t1"."id" AS VARCHAR))))) > 0 THEN ARRAY_TO_STRING(ARRAY_COMPACT(ARRAY_COMPACT(ARRAY_CONSTRUCT('g::City:', CAST("t1"."id" AS VARCHAR)))), '') END), '') ELSE NULL END IS NULL THEN '' ELSE (',"@id":' || '"' || COALESCE(replace(replace(CASE WHEN NOT EXISTS (SELECT * FROM "n_city" AS "t8" WHERE "t0"."r_hq" = "t8"."id") THEN NULL WHEN "t1"."id" IS NOT NULL THEN coalesce((CASE WHEN ARRAY_SIZE(ARRAY_COMPACT(ARRAY_COMPACT(ARRAY_CONSTRUCT('g::City:', CAST("t1"."id" AS VARCHAR))))) > 0 THEN ARRAY_TO_STRING(ARRAY_COMPACT(ARRAY_COMPACT(ARRAY_CONSTRUCT('g::City:', CAST("t1"."id" AS VARCHAR)))), '') END), '') ELSE NULL END, '\\', '\\\\'), '"', '\\"'), '') || '"') END, '') || COALESCE(CASE WHEN "t1"."p_name" IS NULL THEN '' ELSE (',"name":' || '"' || COALESCE(replace(replace("t1"."p_name", '\\', '\\\\'), '"', '\\"'), '') || '"') END, '') || '}')) END IS NOT NULL AS "noHq", CASE WHEN "t3"."agg_0" IS NOT NULL THEN "t3"."agg_0" ELSE 0 END AS "headcount", PARSE_JSON(('[' || COALESCE(substr("t5"."agg_0", 2), '') || ']')) AS "seniors", CASE WHEN "t0"."p_founded" IS NOT NULL AND "t0"."p_founded" IS NOT NULL AND "t0"."p_founded" IS NOT NULL AND "t0"."p_founded" < 2000 THEN TRUE WHEN NOT "t0"."p_founded" IS NOT NULL OR "t0"."p_founded" IS NOT NULL AND NOT ("t0"."p_founded" IS NOT NULL AND "t0"."p_founded" < 2000) THEN FALSE ELSE NULL END AS "old", CASE WHEN NOT CASE WHEN NOT EXISTS (SELECT * FROM "n_city" AS "t9" WHERE "t0"."r_hq" = "t9"."id") THEN TO_VARIANT(NULL) ELSE PARSE_JSON(('{"@labels":' || '["City"]' || COALESCE(CASE WHEN CASE WHEN NOT EXISTS (SELECT * FROM "n_city" AS "t10" WHERE "t0"."r_hq" = "t10"."id") THEN NULL WHEN "t1"."id" IS NOT NULL THEN coalesce((CASE WHEN ARRAY_SIZE(ARRAY_COMPACT(ARRAY_COMPACT(ARRAY_CONSTRUCT('g::City:', CAST("t1"."id" AS VARCHAR))))) > 0 THEN ARRAY_TO_STRING(ARRAY_COMPACT(ARRAY_COMPACT(ARRAY_CONSTRUCT('g::City:', CAST("t1"."id" AS VARCHAR)))), '') END), '') ELSE NULL END IS NULL THEN '' ELSE (',"@id":' || '"' || COALESCE(replace(replace(CASE WHEN NOT EXISTS (SELECT * FROM "n_city" AS "t11" WHERE "t0"."r_hq" = "t11"."id") THEN NULL WHEN "t1"."id" IS NOT NULL THEN coalesce((CASE WHEN ARRAY_SIZE(ARRAY_COMPACT(ARRAY_COMPACT(ARRAY_CONSTRUCT('g::City:', CAST("t1"."id" AS VARCHAR))))) > 0 THEN ARRAY_TO_STRING(ARRAY_COMPACT(ARRAY_COMPACT(ARRAY_CONSTRUCT('g::City:', CAST("t1"."id" AS VARCHAR)))), '') END), '') ELSE NULL END, '\\', '\\\\'), '"', '\\"'), '') || '"') END, '') || COALESCE(CASE WHEN "t1"."p_name" IS NULL THEN '' ELSE (',"name":' || '"' || COALESCE(replace(replace("t1"."p_name", '\\', '\\\\'), '"', '\\"'), '') || '"') END, '') || '}')) END IS NOT NULL THEN 'no hq' WHEN "t1"."p_name" IS NULL THEN NULL ELSE ('in ' || COALESCE("t1"."p_name", '')) END AS "where"
FROM "n_firm" AS "t0"
LEFT OUTER JOIN "n_city" AS "t1" ON "t0"."r_hq" = "t1"."id"
LEFT OUTER JOIN (
  SELECT "t2"."r_works_at" AS "r_works_at", COUNT(*) AS "agg_0"
  FROM "n_person" AS "t2"
  GROUP BY "t2"."r_works_at"
) AS "t3" ON "t3"."r_works_at" = "t0"."id"
LEFT OUTER JOIN (
  SELECT "t4"."r_works_at" AS "r_works_at", (CASE WHEN COUNT((',' || COALESCE(CASE WHEN "t4"."p_name" IS NULL THEN 'null' ELSE ('"' || COALESCE(replace(replace("t4"."p_name", '\\', '\\\\'), '"', '\\"'), '') || '"') END, ''))) = 0 THEN NULL ELSE LISTAGG((',' || COALESCE(CASE WHEN "t4"."p_name" IS NULL THEN 'null' ELSE ('"' || COALESCE(replace(replace("t4"."p_name", '\\', '\\\\'), '"', '\\"'), '') || '"') END, '')), '') END) AS "agg_0"
  FROM "n_person" AS "t4"
  WHERE "t4"."p_age" IS NOT NULL AND "t4"."p_age" IS NOT NULL AND "t4"."p_age" > 35
  GROUP BY "t4"."r_works_at"
) AS "t5" ON "t5"."r_works_at" = "t0"."id"
ORDER BY "t0"."p_legalname" NULLS LAST
```

> Cut in the 2026-11 docs cleanup: §19, one Neo4j-judged case per leg from the study's §12.12 to §12.38 (2026-10-16), and §20, the TCK ledger on DuckDB of 2026-10-17; the cases are pinned under `core/src/test/resources/cypher-parity/lite/`, and the Neo4j facts they measured are in the study's "Upstream facts found while building". Full text: `git show de2745351:docs/CYPHER_SHOWCASE.md`.
