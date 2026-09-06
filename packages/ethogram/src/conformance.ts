import { mkdir, readdir, readFile, writeFile } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { EVENT_SCHEMA_VERSION, parseEvent, serialiseEvent } from "./index.js";

function recursivelySortObjectKeys(value: unknown): unknown {
  if (Array.isArray(value)) {
    return value.map(recursivelySortObjectKeys);
  }
  if (typeof value === "object" && value !== null) {
    return Object.fromEntries(
      Object.entries(value)
        .sort(([left], [right]) =>
          Buffer.compare(Buffer.from(left, "utf8"), Buffer.from(right, "utf8")),
        )
        .map(([key, child]) => [key, recursivelySortObjectKeys(child)]),
    );
  }
  return value;
}

async function main(): Promise<void> {
  const outputDirectory = process.argv[2];
  if (outputDirectory === undefined) {
    throw new Error("usage: conformance.ts <output-directory>");
  }

  const repositoryRoot = resolve(
    dirname(fileURLToPath(import.meta.url)),
    "../../..",
  );
  const corpusDirectory = join(repositoryRoot, "conformance", "v1");
  const fixtures = (await readdir(corpusDirectory, { withFileTypes: true }))
    .filter((entry) => entry.isFile() && entry.name.endsWith(".json"))
    .map((entry) => entry.name)
    .sort();

  await mkdir(outputDirectory, { recursive: true });
  for (const fixture of fixtures) {
    const source = await readFile(join(corpusDirectory, fixture), "utf8");
    const event = parseEvent(JSON.parse(source) as unknown);
    const compact = serialiseEvent(event);
    const canonical = JSON.stringify(
      recursivelySortObjectKeys(JSON.parse(compact) as unknown),
    );
    await writeFile(join(outputDirectory, fixture), canonical, "utf8");
  }

  await writeFile(
    join(outputDirectory, "_harness.json"),
    JSON.stringify({
      fixtures: fixtures.length,
      schemaVersion: EVENT_SCHEMA_VERSION,
    }),
    "utf8",
  );
  console.log(
    `TypeScript conformance: prepared ${fixtures.length} fixture${fixtures.length === 1 ? "" : "s"} for comparison.`,
  );
}

await main();
