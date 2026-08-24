import { Bindings } from "@alienplatform/bindings"

function requiredEnvironmentVariable(name: string): string {
  const value = process.env[name]
  if (!value) throw new Error(`${name} is required`)
  return value
}

const bindings = await Bindings.forRemoteDeployment({
  deploymentId: requiredEnvironmentVariable("ALIEN_DEPLOYMENT_ID"),
  token: requiredEnvironmentVariable("ALIEN_API_TOKEN"),
})

const key = bindings.key("customer-key")
const context = { tenant: requiredEnvironmentVariable("CUSTOMER_ID") }
const plaintext = new TextEncoder().encode("customer data")
const ciphertext = await key.encrypt(plaintext, { context })
const decrypted = await key.decrypt(ciphertext, { context })

console.log({
  ciphertext: ciphertext.toString("base64"),
  decrypted: new TextDecoder().decode(decrypted),
})
