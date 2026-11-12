# Add an AWS budget alert

An [extension](https://alien.dev/docs/guide/extensions) that adds `budget/alert`, a resource type Alien doesn't have: an AWS budget that emails when the account's monthly cost passes 80% of a limit. It has a CloudFormation emitter, a Terraform emitter and an importer, and no controller, so the resource is frozen only.

| Setup path | Works |
|------------|-------|
| CloudFormation package on AWS | Yes, the CloudFormation emitter |
| Terraform package on AWS | Yes, the Terraform emitter |
| The setup CLI, Helm, Google Cloud, Azure | No |

## Test and build

You need the Rust target the hooks build for:

```bash
rustup target add wasm32-wasip1
```

```bash
alien extensions test
alien extensions build
```

`alien extensions test` runs the crates' tests natively and the hooks crate's tests again inside the sandbox Alien runs hooks in, then runs the importer on `tests/import/budget/alert/aws/basic.json` natively and in the hooks module Alien loads, and fails if the two differ. `alien extensions build` writes the bundle to `target/alien-extension/` and `generated/index.ts`, with the `BudgetAlert` builder your stack uses.

## In your stack

```ts
import * as alien from "@alienplatform/core"
import budget, { BudgetAlert } from "./budget/generated"

const alert = new BudgetAlert("costs")
  .monthlyLimitUsd(500)
  .email("cloud-costs@acme.dev")
  .build()

export default new alien.Stack("my-app")
  .extensions([budget])
  .add(alert, "frozen")
  .build()
```

The [budget example](https://alien.dev/docs/examples/budget-alert) walks through the emitters and the importer.
