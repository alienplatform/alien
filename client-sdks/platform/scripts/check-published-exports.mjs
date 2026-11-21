// Compares the public exports of two builds of the Platform TypeScript SDK.
//
//   node check-published-exports.mjs <published-esm-dir> <new-esm-dir> [accepted-changes-file]
//
// Every module the published build ships is importable by deep path, so every
// module must still exist and still export every name it exported, with the same
// meaning (a value stays a value, a type stays a type). Every module of the new
// build must also load, which catches import cycles that leave a schema
// uninitialized.
//
// Every published function (`funcs/`) and SDK method (`sdk/`) must also stay
// callable as before: no parameter removed or newly required, no request field
// newly required or removed, and no response field removed. This compares
// property names and optionality, four levels into request and response models;
// it does not compare primitive types or union members, and an added optional
// parameter or field passes. A break the API itself made is listed, one report
// line each, in the accepted-changes file; an entry stops mattering once an SDK
// without the old signature is committed, and can then be removed. Exits 1 on any
// loss, unaccepted signature break or load failure.
import { createRequire } from "node:module";
import { existsSync, readFileSync, readdirSync } from "node:fs";
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
  const program = ts.createProgram(files, {
    noEmit: true,
    skipLibCheck: true,
    types: [],
    module: ts.ModuleKind.Node16,
    moduleResolution: ts.ModuleResolutionKind.Node16,
  });
  const checker = program.getTypeChecker();
  const modules = new Map();
  const callables = new Map();
  for (const file of files) {
    const source = program.getSourceFile(file);
    const symbol = source && checker.getSymbolAtLocation(source);
    const names = new Map();
    for (const exported of symbol ? checker.getExportsOfModule(symbol) : []) {
      const target = exported.flags & ts.SymbolFlags.Alias ? checker.getAliasedSymbol(exported) : exported;
      const module = path.relative(root, file).replaceAll(path.sep, "/");
      if (/^(funcs|sdk)\//.test(module) && target.flags & ts.SymbolFlags.Function) {
        callables.set(`${module}: ${exported.getName()}`, checker.getTypeOfSymbol(target).getCallSignatures().at(-1));
      } else if (module.startsWith("sdk/") && target.flags & ts.SymbolFlags.Class) {
        for (const method of checker.getDeclaredTypeOfSymbol(target).getProperties()) {
          const signature = checker.getTypeOfSymbol(method).getCallSignatures().at(-1);
          if (signature) callables.set(`${module}: ${exported.getName()}.${method.getName()}`, signature);
        }
      }
      names.set(exported.getName(), {
        value: Boolean(target.flags & ts.SymbolFlags.Value),
        type: Boolean(target.flags & (ts.SymbolFlags.Type | ts.SymbolFlags.Namespace)),
      });
    }
    modules.set(path.relative(root, file).replaceAll(path.sep, "/"), names);
  }
  return { root, modules, callables, checker };
}

const depthLimit = 4;
const optional = symbol => Boolean(symbol.flags & ts.SymbolFlags.Optional);
function properties(side, type) {
  type = side.checker.getNonNullableType(type);
  if (type.isUnion() || !(type.flags & ts.TypeFlags.Object) || type.getCallSignatures().length) return null;
  if (side.checker.isArrayType(type)) return null;
  // Only the SDK's own models: not DOM, Node or library types.
  const declaration = (type.aliasSymbol ?? type.getSymbol())?.declarations?.[0]?.getSourceFile().fileName ?? "";
  if (!declaration.startsWith(side.root)) return null;
  return new Map(type.getProperties().map(property => [property.getName(), property]));
}
function elementType(side, type) {
  type = side.checker.getNonNullableType(type);
  return side.checker.isArrayType(type) ? side.checker.getTypeArguments(type)[0] : null;
}
// Callers read response fields: each one must still be there.
function compareResponse(a, b, where, breaks, depth = 0) {
  if (depth > depthLimit) return;
  const [ea, eb] = [elementType(before, a), elementType(after, b)];
  if (ea && eb) return compareResponse(ea, eb, `${where}[]`, breaks, depth + 1);
  const [pa, pb] = [properties(before, a), properties(after, b)];
  if (!pa || !pb) return;
  for (const [name, property] of pa) {
    const current = pb.get(name);
    if (!current) breaks.push(`${where}.${name}: response field removed`);
    else compareResponse(before.checker.getTypeOfSymbol(property), after.checker.getTypeOfSymbol(current), `${where}.${name}`, breaks, depth + 1);
  }
}
// Callers build requests: nothing they omitted may become required, and nothing
// they may pass may disappear.
function compareRequest(a, b, where, breaks, depth = 0) {
  if (depth > depthLimit) return;
  const [ea, eb] = [elementType(before, a), elementType(after, b)];
  if (ea && eb) return compareRequest(ea, eb, `${where}[]`, breaks, depth + 1);
  const [pa, pb] = [properties(before, a), properties(after, b)];
  if (!pa || !pb) return;
  for (const [name, current] of pb) {
    const property = pa.get(name);
    if (!optional(current) && (!property || optional(property))) breaks.push(`${where}.${name}: request field newly required`);
    else if (property) compareRequest(before.checker.getTypeOfSymbol(property), after.checker.getTypeOfSymbol(current), `${where}.${name}`, breaks, depth + 1);
  }
  for (const name of pa.keys()) if (!pb.has(name)) breaks.push(`${where}.${name}: request field removed`);
}
// A function resolves to `Result<T, E>` (`{ ok: true; value: T } | ...`), a method to T.
function responseType(side, signature) {
  const awaited = side.checker.getAwaitedType(signature.getReturnType()) ?? signature.getReturnType();
  for (const member of awaited.isUnion() ? awaited.types : []) {
    const value = member.getProperty("value");
    if (value && member.getProperty("ok")) return side.checker.getTypeOfSymbol(value);
  }
  return awaited;
}
function compareSignatures(name, a, b, breaks) {
  const isOptional = (side, parameter) => side.checker.isOptionalParameter(parameter.valueDeclaration);
  const [pa, pb] = [a.getParameters(), b.getParameters()];
  pb.forEach((parameter, index) => {
    const old = pa[index];
    const label = `${name}(${parameter.getName()})`;
    if (!isOptional(after, parameter) && (!old || isOptional(before, old))) breaks.push(`${label}: parameter newly required`);
    if (old) compareRequest(before.checker.getTypeOfSymbol(old), after.checker.getTypeOfSymbol(parameter), label, breaks);
  });
  for (const old of pa.slice(pb.length)) breaks.push(`${name}(${old.getName()}): parameter removed`);
  compareResponse(responseType(before, a), responseType(after, b), `${name} response`, breaks);
}

const [published, current, acceptedFile] = process.argv.slice(2);
const accepted = new Set(
  acceptedFile && existsSync(acceptedFile)
    ? readFileSync(acceptedFile, "utf8").split("\n").map(line => line.trim()).filter(line => line && !line.startsWith("#"))
    : [],
);
if (!published || !current) {
  console.error("usage: check-published-exports.mjs <published-esm-dir> <new-esm-dir>");
  process.exit(2);
}

const before = publicExports(path.resolve(published));
const after = publicExports(path.resolve(current));
const signatureBreaks = [];
for (const [name, signature] of before.callables) {
  const current = after.callables.get(name);
  if (current) compareSignatures(name, signature, current, signatureBreaks);
}
const acceptedBreaks = signatureBreaks.filter(line => accepted.has(line));
signatureBreaks.splice(0, signatureBreaks.length, ...signatureBreaks.filter(line => !accepted.has(line)));
const missingModules = [];
const missingNames = [];
let publishedNames = 0;
for (const [module, names] of before.modules) {
  publishedNames += names.size;
  const now = after.modules.get(module);
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
for (const [module, names] of after.modules) {
  const old = before.modules.get(module);
  if (!old) addedModules.push(module);
  for (const name of names.keys()) if (!old?.has(name)) addedNames += 1;
}

console.log(`published: ${before.modules.size} modules, ${publishedNames} exported names`);
console.log(`new build: ${after.modules.size} modules (${addedModules.length} new), ${addedNames} new exported names`);
console.log(`missing: ${missingModules.length} modules, ${missingNames.length} exported names`);
const limit = Number(process.env.LIMIT ?? 200);
for (const line of [...missingModules.map(module => `module ${module}`), ...missingNames].slice(0, limit)) {
  console.log(`  ${line}`);
}
if (missingModules.length + missingNames.length > limit) console.log("  ...");
console.log(
  `signatures: ${before.callables.size} published functions and methods, ${signatureBreaks.length} breaks (${acceptedBreaks.length} accepted)`,
);
for (const line of signatureBreaks.slice(0, limit)) console.log(`  ${line}`);
const loadFailures = [];
for (const module of after.modules.keys()) {
  const file = path.resolve(current, module.replace(/\.d\.ts$/, ".js"));
  try {
    await import(pathToFileURL(file).href);
  } catch (error) {
    loadFailures.push(`${module.replace(/\.d\.ts$/, ".js")}: ${String(error?.message ?? error).split("\n")[0]}`);
  }
}
console.log(`loaded: ${after.modules.size - loadFailures.length} of ${after.modules.size} modules`);
for (const line of loadFailures.slice(0, limit)) console.log(`  ${line}`);
process.exit(missingModules.length || missingNames.length || signatureBreaks.length || loadFailures.length ? 1 : 0);
