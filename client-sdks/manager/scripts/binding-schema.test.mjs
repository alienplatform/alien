import assert from "node:assert/strict"
import fs from "node:fs"
import test from "node:test"
import Ajv2020 from "ajv/dist/2020.js"

const spec = JSON.parse(fs.readFileSync(new URL("../openapi.json", import.meta.url), "utf8"))
const reference = { secretRef: { name: "storage-auth", key: "value" } }
const expression = { "Fn::GetAtt": ["Storage", "BucketName"] }

for (const [name, literals] of [
  ["BindingValue_String", ["archive-bucket"]],
  ["BindingValue_Option_String", ["archive-bucket", null]],
  ["BindingValue_Vec_String", [["archive-bucket", "second-bucket"]]],
  ["BindingValue_u16", [443]],
  ["BindingValue_u8", [8]],
]) {
  test(`published ${name} contract accepts literals, SecretRefs and expressions`, () => {
    const ajv = new Ajv2020({ strict: false, validateFormats: false })
    const validate = ajv.compile({
      components: spec.components,
      $ref: `#/components/schemas/${name}`,
    })
    for (const value of [...literals, reference, expression]) {
      assert.equal(validate(value), true, JSON.stringify(validate.errors))
    }
  })
}
