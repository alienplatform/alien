// Type declaration for the statically-embedded native addon that `native.ts`
// imports via the literal specifier `./alien-bindings.node`. No `.node` file
// exists at build time in this repo — task 13's `alien build` stages it next to
// the built `native.js`. This declaration lets `native.ts` typecheck and gives
// the default import the full addon type.
import type { NativeAddon } from "./loader.js"

declare const addon: NativeAddon
export default addon
