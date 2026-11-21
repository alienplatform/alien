// Compares the public exports of two builds of the Platform TypeScript SDK.
//
//   node check-published-exports.mjs <published-esm-dir> <new-esm-dir>
//
// Every module the published build ships is importable by deep path, so every
// module must still exist and still export every name it exported, with the same
// meaning (a value stays a value, a type stays a type). Every module of the new
// build must also load, which catches import cycles that leave a schema
// uninitialized. Exits 1 on any loss or load failure.
import { createRequire } from "node:module";
import { readdirSync } from "node:fs";
import path from "node:path";
import { pathToFileURL } from "node:url";

const require = createRequire(new URL("../typescript/package.json", import.meta.url));
const ts = require("typescript");

function declarationFiles(root) {
  const files = [];
  for (const entry of readdirSync(root, { withFileTypes: true, recursive: true })) {
    if (entry.isFile() && entry.name.endsWith(".d.ts")) files.push(path.join(entry.parentPath, entry.name));
  }
  return files.sort();
}

function publicExports(root) {
  const files = declarationFiles(root);
  const program = ts.createProgram(files, { noEmit: true, skipLibCheck: true, types: [] });
  const checker = program.getTypeChecker();
  const modules = new Map();
  for (const file of files) {
    const source = program.getSourceFile(file);
    const symbol = source && checker.getSymbolAtLocation(source);
    const names = new Map();
    for (const exported of symbol ? checker.getExportsOfModule(symbol) : []) {
      const target = exported.flags & ts.SymbolFlags.Alias ? checker.getAliasedSymbol(exported) : exported;
      names.set(exported.getName(), {
        value: Boolean(target.flags & ts.SymbolFlags.Value),
        type: Boolean(target.flags & (ts.SymbolFlags.Type | ts.SymbolFlags.Namespace)),
      });
    }
    modules.set(path.relative(root, file).replaceAll(path.sep, "/"), names);
  }
  return modules;
}

const [published, current] = process.argv.slice(2);
if (!published || !current) {
  console.error("usage: check-published-exports.mjs <published-esm-dir> <new-esm-dir>");
  process.exit(2);
}

const before = publicExports(path.resolve(published));
const after = publicExports(path.resolve(current));
const missingModules = [];
const missingNames = [];
let publishedNames = 0;
for (const [module, names] of before) {
  publishedNames += names.size;
  const now = after.get(module);
  if (!now) {
    missingModules.push(module);
    continue;
  }
  for (const [name, meaning] of names) {
    const found = now.get(name);
    if (!found) missingNames.push(`${module}: ${name}`);
    else if ((meaning.value && !found.value) || (meaning.type && !found.type)) {
      const describe = ({ value, type }) => [value && "value", type && "type"].filter(Boolean).join(" and ") || "nothing";
      missingNames.push(`${module}: ${name} (was ${describe(meaning)}, now ${describe(found)})`);
    }
  }
}

let addedNames = 0;
const addedModules = [];
for (const [module, names] of after) {
  const old = before.get(module);
  if (!old) addedModules.push(module);
  for (const name of names.keys()) if (!old?.has(name)) addedNames += 1;
}

console.log(`published: ${before.size} modules, ${publishedNames} exported names`);
console.log(`new build: ${after.size} modules (${addedModules.length} new), ${addedNames} new exported names`);
console.log(`missing: ${missingModules.length} modules, ${missingNames.length} exported names`);
const limit = Number(process.env.LIMIT ?? 200);
for (const line of [...missingModules.map(module => `module ${module}`), ...missingNames].slice(0, limit)) {
  console.log(`  ${line}`);
}
if (missingModules.length + missingNames.length > limit) console.log("  ...");
const loadFailures = [];
for (const module of after.keys()) {
  const file = path.resolve(current, module.replace(/\.d\.ts$/, ".js"));
  try {
    await import(pathToFileURL(file).href);
  } catch (error) {
    loadFailures.push(`${module.replace(/\.d\.ts$/, ".js")}: ${String(error?.message ?? error).split("\n")[0]}`);
  }
}
console.log(`loaded: ${after.size - loadFailures.length} of ${after.size} modules`);
for (const line of loadFailures.slice(0, limit)) console.log(`  ${line}`);
process.exit(missingModules.length || missingNames.length || loadFailures.length ? 1 : 0);
