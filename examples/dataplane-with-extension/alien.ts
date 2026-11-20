import * as alien from "@alienplatform/core"
import budget, { BudgetAlert } from "./budget-extension/generated"

// A resource type Alien doesn't have, from the extension beside this file.
const costs = new BudgetAlert("costs").monthlyLimitUsd(500).email("cloud-costs@example.com").build()

const api = new alien.Worker("api")
  .code({ type: "source", src: "./", toolchain: { type: "typescript" } })
  .publicEndpoint("api")
  .permissions("execution")
  .build()

export default new alien.Stack("dataplane")
  .extensions([budget])
  .add(costs, "frozen")
  .add(api, "live")
  .permissions({
    profiles: {
      execution: {},
    },
  })
  .build()
