# Extension examples

An [extension](https://alien.dev/docs/guide/extensions) lets you change what Alien deploys, add a resource type Alien doesn't have, or change the calls Alien makes to a cloud, without a pull request to Alien. Alien keeps doing the work: it builds the setup templates, sets up the customer's environment, keeps the state, and runs the manager and the Operator. An extension plugs in at a few fixed points and implements only what it needs.

## Where an extension plugs in

| Stage | What Alien does | What an extension can add |
| --- | --- | --- |
| Define | You describe the app in `alien.ts` | A new resource type, used like any Alien resource |
| Build templates | Alien generates the CloudFormation, Terraform and Helm the customer gets | A **modifier** edits the template; an **emitter** adds the new resource to it |
| Set up | The template runs in the customer's cloud, and Alien reads what it created | An **importer** turns what the template created into Alien's state |
| Manage | The manager (push) or the Operator (pull) updates resources and calls the cloud | An **interceptor** changes a call to the cloud; a **controller** manages the new resource |

Modifiers, emitters, importers and interceptors are hooks: small pure functions that never call the cloud. They compile to one WebAssembly module inside the release, which Alien runs sandboxed. A controller does call the cloud, so it's a native binary: a server you host in push, and a process next to the Operator in pull and in the setup CLI.

## The examples

Read them in this order:

| Example | What it shows | What it implements |
| --- | --- | --- |
| [postgres-iam-auth](./postgres-iam-auth) | Change something Alien already creates: turn on IAM database authentication on AWS, Google Cloud and Azure | Template modifiers, and interceptors for the databases Alien's controllers create |
| [postgres-backup-retention](./postgres-backup-retention) | The same idea in its smallest form: keep Postgres backups for 30 days | Two template modifiers |
| [resource-tags](./resource-tags) | Change the calls Alien's controllers make: add tags to every create call | Interceptors, one per cloud |
| [budget-alert](./budget-alert) | Add a resource Alien doesn't have, created only at setup: an AWS budget alert | A resource type, CloudFormation and Terraform emitters, an importer; no controller |
| [dns-zone](./dns-zone) | Add a resource Alien doesn't have and manages over time: a DNS zone | A resource type, emitters, importers, permission sets, a link, and a controller for AWS, Google Cloud, Azure and local |

An extension that implements less gets less. A resource with no controller can't be updated after setup, and a resource with only a CloudFormation emitter turns the Terraform package off for the stacks that use it.

[dataplane-with-extension](../dataplane-with-extension) is a project that uses one of these: its `alien.ts` adds the budget alert from an extension in the same folder.

## Test and build

Each example is its own Cargo workspace. You need the Rust target the hooks build for:

```bash
rustup target add wasm32-wasip1
```

From the example's folder:

```bash
alien extensions test
alien extensions build
```

`alien extensions test` runs the extension's tests natively and again inside the sandbox Alien runs hooks in, and checks each importer against its fixtures. `alien extensions build` builds the hooks module, the controller for each target the machine can build, and the TypeScript a stack imports.

To start your own from one of them:

```bash
alien init extensions/postgres-backup-retention backups
```
