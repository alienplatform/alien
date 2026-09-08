import assert from "node:assert/strict"
import { readFileSync } from "node:fs"
import test from "node:test"
import { withSDKReleaseVersion } from "./sdk-release-metadata.mjs"

for (const sdk of ["platform", "manager"]) {
  const path = `client-sdks/${sdk}/typescript/src/lib/config.ts`
  const source = readFileSync(path, "utf8")
  test(`${sdk} release metadata advances without changing the API or generator`, () => {
    const generated = withSDKReleaseVersion(source, "9.8.7")
    const metadata = text =>
      JSON.parse(
        text
          .slice(
            text.indexOf("export const SDK_METADATA = ") + "export const SDK_METADATA = ".length,
          )
          .replace(/ as const;\s*$/, "")
          .replace(/(\w+):/g, '"$1":')
          .replace(/,\s*}/, "}"),
      )
    const before = metadata(source)
    const after = metadata(generated)
    assert.deepEqual(after, {
      ...before,
      sdkVersion: "9.8.7",
      userAgent: before.userAgent.replace(/^(speakeasy-sdk\/typescript )\S+/, "$19.8.7"),
    })
    assert.equal(withSDKReleaseVersion(generated, before.sdkVersion), source)
    assert.equal(withSDKReleaseVersion(generated, "9.8.7"), generated)
  })
  test(`${sdk} rejects missing or duplicate runtime version fields`, () => {
    assert.throws(() => withSDKReleaseVersion(source.replace("sdkVersion:", "version:"), "9.8.7"))
    assert.throws(() => withSDKReleaseVersion(source + source, "9.8.7"))
    assert.throws(() =>
      withSDKReleaseVersion(
        source.replace("speakeasy-sdk/typescript", "other-sdk/typescript"),
        "9.8.7",
      ),
    )
    assert.throws(() => withSDKReleaseVersion(source, "invalid"))
  })
}
