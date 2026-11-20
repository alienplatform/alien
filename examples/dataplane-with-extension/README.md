# A project with an extension beside it

A project whose `alien.ts` uses an [extension](https://alien.dev/docs/guide/extensions) that lives in the same folder. The extension is the [budget alert](../extensions/budget-alert): it adds `budget/alert`, an AWS budget that emails when the account's monthly cost passes 80% of a limit, a resource type Alien doesn't have. The stack adds one beside a Worker.

```text
├── alien.ts            # imports the extension and adds a BudgetAlert
├── src/index.ts        # the Worker
└── budget-extension/   # the extension: its own Cargo workspace
    ├── Cargo.toml
    ├── types/
    ├── hooks/
    └── tests/import/budget/alert/aws/
```

`alien.ts` imports the module the extension's build writes, and lists the extension in `.extensions(...)`:

```ts
import budget, { BudgetAlert } from "./budget-extension/generated"

const costs = new BudgetAlert("costs").monthlyLimitUsd(500).email("cloud-costs@example.com").build()

export default new alien.Stack("dataplane")
  .extensions([budget])
  .add(costs, "frozen")
  .add(api, "live")
```

## Build the extension

`generated/` doesn't exist until the extension builds. You need the Rust target the hooks build for:

```bash
rustup target add wasm32-wasip1
```

```bash
cd budget-extension
alien extensions test
alien extensions build
cd ..
```

`alien extensions build` writes the bundle to `budget-extension/target/alien-extension/` and `budget-extension/generated/index.ts`, with the `BudgetAlert` builder. `alien render`, `alien dev` and `alien release` read the bundle from beside the module `alien.ts` imports.

## See it in the template

```bash
alien render --format terraform --target aws --stack alien.ts | grep -A 15 'resource "aws_budgets_budget"'
```

The module has an `aws_budgets_budget` with the limit and the email. The budget alert has no Google Cloud or Azure emitter, so it renders only for AWS.

`alien render` makes no CloudFormation template for this stack, because the Worker is built from source and a CloudFormation template needs an image. The stack's CloudFormation package comes from `alien release`, which builds the image.
