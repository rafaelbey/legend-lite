/**
 * The round trip in the tab (docs/PROTOCOL_PROGRAM_2026_10_05.md §4, leg 5): the WebAssembly module asked what the tab
 * asks legend-engine's pure/v1 for, over every input the JVM's round-trip proof reads -- legend-engine's test
 * collection, lite's projects, the upstream showcase projects -- and its answers held to the JVM's, byte for byte.
 *
 * The JVM half (//parser-equivalence:tab_round_trip, a build action) asked the same exports of the same source on the
 * JVM: per input, `grammarToJson/model` without source information, then `jsonToGrammar/model` of the JSON it
 * answered in each style; it wrote each input's text and the SHA-256 of each answer. Here the module is asked the
 * same, the second and third requests with the JSON its own first answer carried (the JVM's, when the first matched),
 * and every digest compared, refusals included: a refusal is an answer the tab is expected to give.
 *
 *   bazel test //wasm:round_trip_test
 */
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { runfileDirUrl, runfileFromEnv } from '../tools/js/runfiles.mts';

const GRAMMAR_TO_JSON = '/api/pure/v1/grammar/grammarToJson/model';
const JSON_TO_GRAMMAR = '/api/pure/v1/grammar/jsonToGrammar/model';
const STYLES = ['STANDARD', 'PRETTY'];

const sha256 = (s) => createHash('sha256').update(s, 'utf8').digest('hex');

// 2026-11-09 (docs/rust-port/PHASE2.md section 6, ruling 4; PARITY row 9): the inputs where the JVM's parser refuses with
// a placed parse error and this frozen module, TeaVM's last build of the Java parser (wasm/BUILD.bazel), still answers
// the raw JDK exception. Held both ways: the module answers exactly this, and the JVM's answer differs. They go when the
// Rust wasm32 build replaces the module.
const FROZEN_MODULE_DIFFERS = new Map([
  ['legend-pure-core/legend-pure-m3-core/src/test/java/org/finos/legend/pure/m3/tests/elements/profile/TestProfile.java#52',
    'OK\n500\napplication/json\n{"code":-1,"message":"IllegalArgumentException: Less than 4 hex digits in unicode escape","status":"error"}'],
]);

/** A folded answer's body when it is a 200 (`OK\n200\n<type>\n<body>`), else undefined (TabRoundTripRequests.okBody). */
function okBody(folded) {
  const prefix = 'OK\n200\n';
  if (!folded.startsWith(prefix)) return undefined;
  const type = folded.indexOf('\n', prefix.length);
  return type < 0 ? undefined : folded.slice(type + 1);
}

test('the tab round-trips every input as the JVM does', async () => {
  const lines = readFileSync(runfileFromEnv('TAB_ROUND_TRIP'), 'utf8').split('\n').filter((l) => l !== '');
  const planner = runfileDirUrl('WASM_PLANNER');
  const { load } = await import(new URL('wasm-gc-module-runtime.js', planner).href);
  // TeaVM's loader wants a filesystem path under Node (a file: URL fails with an ENOENT)
  const teavm = await load(fileURLToPath(new URL('classes.wasm', planner)), {
    stackDeobfuscator: { enabled: false },
    installImports(o) {
      o.teavmConsole = o.teavmConsole || {};
      o.teavmConsole.putcharStdout = () => {};
      o.teavmConsole.putcharStderr = () => {};
    },
  });
  const ask = (path, query, body) => {
    try {
      return teavm.exports.pureV1OrError(path, query, body);
    } catch (e) {
      // pureV1OrError folds Java failures into its answer: reaching here means the module itself broke
      return `THREW-THROUGH-JS\n${e && e.message}`;
    }
  };

  const counts = new Map();
  const diffs = [];
  const differing = [];
  const listed = new Set();
  for (const line of lines) {
    const { from, id, text, answers } = JSON.parse(line);
    const c = counts.get(from) ?? { inputs: 0, converted: 0, matched: 0 };
    counts.set(from, c);
    c.inputs++;
    const toJson = ask(GRAMMAR_TO_JSON, 'returnSourceInformation=false', text);
    const asked = [sha256(toJson)];
    const frozen = FROZEN_MODULE_DIFFERS.get(id);
    if (frozen !== undefined) {
      assert.equal(toJson, frozen, `${id}: the frozen module's answer moved`);
      assert.notEqual(asked[0], answers[0], `${id}: listed, yet the JVM answers as the frozen module does`);
      listed.add(id);
      continue;
    }
    const json = okBody(toJson);
    if (json !== undefined) {
      c.converted++;
      for (const style of STYLES) asked.push(sha256(ask(JSON_TO_GRAMMAR, `renderStyle=${style}`, json)));
    }
    if (asked.length === answers.length && asked.every((a, i) => a === answers[i])) {
      c.matched++;
    } else {
      const at = asked.findIndex((a, i) => a !== answers[i]);
      diffs.push(`${from} ${id}: answer ${at < 0 ? asked.length : at} differs`
        + (at === 0 ? `\n  module: ${JSON.stringify(toJson.slice(0, 300))}` : ''));
      // the module's whole answers, to hold against the JVM's (TabExports.pureV1OrError on the same text)
      differing.push(JSON.stringify({ from, id, at, toJson, prints: json === undefined ? []
        : STYLES.map((style) => ask(JSON_TO_GRAMMAR, `renderStyle=${style}`, json)) }));
    }
  }
  const outputs = process.env['TEST_UNDECLARED_OUTPUTS_DIR'];
  if (outputs && differing.length > 0) writeFileSync(join(outputs, 'round-trip-diffs.jsonl'), differing.join('\n') + '\n');
  for (const [from, c] of counts) {
    console.log(`[tab-round-trip] ${from}: inputs=${c.inputs} converted=${c.converted} matched=${c.matched}`);
  }
  // inputs that convert nothing compare refusals only, and two builds that both refuse everything agree perfectly
  for (const [from, c] of counts) {
    assert.ok(c.converted > c.inputs / 2, `${from}: only ${c.converted} of ${c.inputs} converted`);
  }
  assert.deepEqual(diffs.slice(0, 40), [], `${diffs.length} inputs the module answered differently from the JVM`);
  assert.deepEqual([...listed].sort(), [...FROZEN_MODULE_DIFFERS.keys()].sort(), 'every listed input was asked');
});
