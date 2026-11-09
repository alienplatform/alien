import * as alien from "@alienplatform/core"

const data = new alien.Storage("data").build()

export default new alien.Stack("acme-every-platform")
  .platforms(["local", "kubernetes", "machines", "azure", "gcp", "aws"])
  .add(data, "frozen")
  .build()
