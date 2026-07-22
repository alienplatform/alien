// A `with { type: "file" }` import of the embedded launcher binary resolves to its
// path string. `bun build --compile` embeds the file and rewrites the path to the
// in-binary copy; `native.ts` hands that path to the loader to extract and spawn.
declare module "*.bin" {
  const path: string
  export default path
}
