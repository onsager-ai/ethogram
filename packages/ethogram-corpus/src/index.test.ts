import assert from "node:assert/strict";
import { describe, test } from "node:test";
import { readFile, readdir } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import path from "node:path";

import { parseEvent, serialiseEvent } from "@onsager-ai/ethogram";

import { parseFixture, v1Fixtures } from "./index.js";

const corpusDirectory = fileURLToPath(
  new URL("../../../conformance/v1/", import.meta.url),
);

async function diskFixtures(): Promise<
  readonly { name: string; rawJson: string }[]
> {
  const entries = await readdir(corpusDirectory, { withFileTypes: true });
  const names = entries
    .filter((entry) => entry.isFile() && entry.name.endsWith(".json"))
    .map((entry) => entry.name)
    .sort((left, right) =>
      Buffer.compare(Buffer.from(left, "utf8"), Buffer.from(right, "utf8")),
    );
  return Promise.all(
    names.map(async (name) => ({
      name,
      rawJson: await readFile(path.join(corpusDirectory, name), "utf8"),
    })),
  );
}

describe("v1 corpus", () => {
  test("exposes the exact directory view", async () => {
    assert.deepEqual(v1Fixtures, await diskFixtures());
  });

  test("round-trips every fixture to the canonical file form", async () => {
    for (const fixture of v1Fixtures) {
      const diskJson = await readFile(
        path.join(corpusDirectory, fixture.name),
        "utf8",
      );
      const canonicalDisk = serialiseEvent(parseEvent(JSON.parse(diskJson)));
      const canonicalExposed = serialiseEvent(parseFixture(fixture));

      assert.equal(canonicalExposed, canonicalDisk, fixture.name);
    }
  });
});
