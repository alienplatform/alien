# DNS zone

An [extension](https://alien.dev/docs/guide/extensions), named `acme`, that adds a `DnsZone` custom resource (`acme/dns-zone`): a resource type Alien doesn't have, with CloudFormation, Terraform and Helm emitters, importers on AWS, Google Cloud and Azure, permission sets, a link, and a controller for AWS, Google Cloud, Azure and local.

```text
├── Cargo.toml          # which crate is the hooks, the controller, and the types
├── package.json        # the npm package other developers install
├── types/              # the zone's config, outputs, and saved fields
├── hooks/src/
│   ├── lib.rs          # registers everything the extension adds
│   └── dns_zone.rs
├── controller/src/     # main.rs, aws.rs, gcp.rs, azure.rs, local.rs
├── permissions/dns-zone/
└── tests/import/acme/dns-zone/   # one importer fixture per platform
```

## Test and build

You need the Rust target the hooks build for:

```bash
rustup target add wasm32-wasip1
```

```bash
alien extensions test
alien extensions build
```

`alien extensions test` runs the crates' tests natively and the hooks crate's tests again inside the sandbox Alien runs hooks in, then runs each importer on its fixtures natively and in the hooks module Alien loads, and loads what it imports into the controller, failing where any of them differ. `alien extensions build` builds the hooks module, the controller for each target this machine can build, and `generated/index.ts` with the `DnsZone` builder.

## See it in the template

A stack beside the extension that adds a zone:

```ts
import * as alien from "@alienplatform/core"
import acme, { DnsZone } from "./acme/generated"

const zone = new DnsZone("zone").domain("internal.acme.dev").build()

export default new alien.Stack("my-app")
  .extensions([acme])
  .add(zone, "frozen")
  .build()
```

Render each setup template from the stack's directory:

```bash
alien render --format cloudformation --stack alien.ts
alien render --format terraform --target aws --stack alien.ts
alien render --format terraform --target gcp --stack alien.ts
alien render --format terraform --target azure --stack alien.ts
```

| Template | The zone |
|----------|----------|
| CloudFormation | an `AWS::Route53::HostedZone`, and the `acme/dns-zone/provision` grants on the extension's role |
| Terraform, AWS | `aws_route53_zone` |
| Terraform, Google Cloud | `google_dns_managed_zone` |
| Terraform, Azure | `azurerm_dns_zone` |

A CloudFormation template takes a Worker as an image. A Worker built from source, such as `.code({ type: "source", ... })`, has no image until `alien release` builds it, so `alien render --format cloudformation` refuses it. Render a stack whose Worker uses `.code({ type: "image", image: "..." })`, or read the template in the setup package the release builds.

## Publish it

To let other developers use the extension, publish this folder as an npm package. The package carries `generated/`, the module their stack imports, and `target/alien-extension/`, the bundle `alien release` uploads from beside it. `.gitignore` leaves out `/target`, so `package.json` lists the bundle in `files`, or `npm publish` drops it:

```bash
alien extensions build
npm publish
```

The [DNS zone example](https://alien.dev/docs/examples/dns-zone) walks through every part.
