// A minimal harness for the tests of the interface logic. There is no DOM
// and no Node: `tools/build_ui.py` bundles each `*.test.ts` with esbuild
// and runs it with QuickJS. A failed check throws; `run` rejects when a
// test failed, which makes QuickJS exit with an error.

type Body = () => void | Promise<void>;

const tests: { name: string; body: Body }[] = [];

export function test(name: string, body: Body): void {
  tests.push({ name, body });
}

/// `actual` and `expected` are equal once written as JSON.
export function equal(actual: unknown, expected: unknown, what: string): void {
  const got = JSON.stringify(actual);
  const wanted = JSON.stringify(expected);
  if (got !== wanted) {
    throw new Error(`${what}\n    expected ${wanted}\n    got      ${got}`);
  }
}

/// Run the tests declared so far, in order.
export async function run(): Promise<void> {
  let failed = 0;
  for (const { name, body } of tests) {
    try {
      await body();
      console.log(`    ok      ${name}`);
    } catch (e: unknown) {
      failed += 1;
      console.log(`    FAILED  ${name}\n    ${e instanceof Error ? e.message : String(e)}`);
    }
  }
  if (failed > 0) {
    throw new Error(`${failed} of ${tests.length} tests failed`);
  }
}
