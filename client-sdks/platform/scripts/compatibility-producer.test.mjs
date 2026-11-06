import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { appendFile, cp, mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

test("a newly generated public name requires review instead of being shadowed by a compatibility alias", async () => {
  const fixture = await mkdtemp(join(tmpdir(), "sdk-contract-"));
  try {
    const scripts = join(fixture, "scripts");
    const models = join(fixture, "typescript/src/models");
    await cp(fileURLToPath(new URL(".", import.meta.url)), scripts, { recursive: true });
    await cp(fileURLToPath(new URL("../typescript/src/models", import.meta.url)), models, { recursive: true });
    const producer = join(scripts, "generate-compatibility-exports.mjs");
    execFileSync(process.execPath, [producer], { stdio: "pipe", timeout: 30_000 });
    const indexBefore = await readFile(join(models, "index.ts"), "utf8");
    execFileSync(process.execPath, [producer], { stdio: "pipe", timeout: 30_000 });
    assert.equal(await readFile(join(models, "index.ts"), "utf8"), indexBefore);

    await appendFile(join(models, "kubernetesexposurecustom.ts"), "\nexport type CreateManagerResponseProviderAwsAlb2 = { changed: true };\n");
    assert.throws(() => execFileSync(process.execPath, [producer], { stdio: "pipe", timeout: 30_000 }), error => {
      assert.equal(error.status, 1);
      assert.match(error.stderr.toString(), /Published alias CreateManagerResponseProviderAwsAlb2 now has a generated root declaration/);
      return true;
    });
  } finally {
    await rm(fixture, { recursive: true, force: true });
  }
});
