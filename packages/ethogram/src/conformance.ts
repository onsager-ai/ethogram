import { mkdir, readdir, readFile, writeFile } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import {
  EVENT_SCHEMA_VERSION,
  ValidationError,
  parseEvent,
  serialiseEvent,
  serialiseValidationError,
  validate,
} from "./index.js";

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
  const errorDirectory = join(
    repositoryRoot,
    "conformance",
    "handwritten-validation-inputs",
  );
  const errorCases = (await readdir(errorDirectory, { withFileTypes: true }))
    .filter((entry) => entry.isFile() && entry.name.endsWith(".json"))
    .map((entry) => entry.name)
    .sort();
  const agreementDirectory = join(
    repositoryRoot,
    "conformance",
    "handwritten-agreement-inputs",
  );
  const agreementInputs = (await readdir(agreementDirectory, { withFileTypes: true }))
    .filter((entry) => entry.isFile() && entry.name.endsWith(".json"))
    .map((entry) => entry.name)
    .sort();

  await mkdir(outputDirectory, { recursive: true });
  for (const fixture of fixtures) {
    const source = await readFile(join(corpusDirectory, fixture), "utf8");
    const event = parseEvent(JSON.parse(source) as unknown);
    validate(event.type, event.payload);
    const compact = serialiseEvent(event);
    await writeFile(join(outputDirectory, fixture), compact, "utf8");
  }

  await mkdir(join(outputDirectory, "errors"), { recursive: true });
  for (const name of errorCases) {
    const input = JSON.parse(await readFile(join(errorDirectory, name), "utf8")) as {
      type: string;
      payload: unknown;
      expectedKind: string;
    };
    let failure: ValidationError | undefined;
    try {
      validate(input.type, input.payload);
    } catch (error) {
      if (!(error instanceof ValidationError)) {
        throw error;
      }
      failure = error;
    }
    if (failure === undefined) {
      throw new Error(`validation input ${name} unexpectedly validated cleanly`);
    }
    if (failure.kind !== input.expectedKind) {
      throw new Error(
        `validation input ${name} expected ${input.expectedKind}, received ${failure.kind}`,
      );
    }
    await writeFile(
      join(outputDirectory, "errors", name),
      serialiseValidationError(failure),
      "utf8",
    );
  }

  await mkdir(join(outputDirectory, "agreement"), { recursive: true });
  for (const name of agreementInputs) {
    const source = await readFile(join(agreementDirectory, name), "utf8");
    const event = parseEvent(JSON.parse(source) as unknown);
    try {
      validate(event.type, event.payload);
    } catch (error) {
      throw new Error(`agreement input ${name} failed validation`, { cause: error });
    }
    await writeFile(
      join(outputDirectory, "agreement", name),
      serialiseEvent(event),
      "utf8",
    );
  }

  await writeFile(
    join(outputDirectory, "_harness.json"),
    JSON.stringify({
      agreementInputs: agreementInputs.length,
      errorCases: errorCases.length,
      fixtures: fixtures.length,
      schemaVersion: EVENT_SCHEMA_VERSION,
    }),
    "utf8",
  );
  console.log(
    `TypeScript conformance: prepared ${fixtures.length} fixture${fixtures.length === 1 ? "" : "s"}, ${errorCases.length} validation error cases, and ${agreementInputs.length} agreement inputs for comparison.`,
  );
}

await main();
