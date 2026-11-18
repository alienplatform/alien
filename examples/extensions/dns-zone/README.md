# DNS zone

An [extension](https://alien.dev/docs/guide/extensions), named `acme`, that adds a `DnsZone` custom resource (`acme/dns-zone`): a resource type Alien doesn't have, with CloudFormation, Terraform and Helm emitters, importers on AWS, Google Cloud and Azure, permission sets, a link, and a controller for AWS, Google Cloud, Azure and local.

```text
├── Cargo.toml          # which crate is the hooks, the controller, and the types
├── package.json        # the npm package other developers install
├── types/              # the zone's config, outputs, and saved fields
├── hooks/src/
│   ├── lib.rs          # registers everything the extension adds
│   └── dns_zone.rs
├── controller/src/
│   ├── lib.rs          # registers the controllers for AWS, Google Cloud and Azure
│   ├── aws.rs, gcp.rs, azure.rs
│   └── main.rs, local.rs   # the controller `alien dev` runs
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

`alien extensions test` runs the crates' tests natively and the hooks crate's tests again inside the sandbox Alien runs hooks in, then runs each importer on its fixtures natively and in the hooks module Alien loads, and loads what it imports into the controller module, failing where any of them differ. `alien extensions build` builds the hooks module, the controller module, the local controller for each target this machine can build, and `generated/index.ts` with the `DnsZone` builder.

## The controllers

The controller crate's library builds for `wasm32-wasip1` into the controller module, `controller.wasm` in the bundle. Alien loads it wherever it steps a deployment and runs the zone's controller for AWS, Google Cloud or Azure in a sandbox with no network, no files and no credentials. A controller's cloud calls go through Alien, which sends each one to the cloud's own API as the deployment:

- `aws.rs` uses the official AWS SDK for Route53, with the config `ctx.aws_sdk_config()` returns.
- `gcp.rs` and `azure.rs` write their requests with `CloudRequest::gcp` and `CloudRequest::azure` and send them with `ctx.cloud().send()`.

Alien can run a step a second time, from the same state, when it got no reply to the first run. Each controller's calls are safe to repeat: a create that finds a zone already there takes it only when the zone carries the label (a tag on Azure) that create wrote, a delete removes only such a zone or the one the setup template made, and a delete that finds it gone is done.

Route53 and Cloud DNS refuse to delete a zone that holds records besides its apex SOA and NS, so `aws.rs` and `gcp.rs` delete those records first. Azure deletes a zone's records with the zone.

The library also builds natively, so `cargo test` runs `aws.rs`'s tests against a Route53 they stand in for, and `gcp.rs`'s tests with `controller_module::testing::answer`, which steps a controller and answers its calls in Alien's place.

The crate's binary, `main.rs`, runs the controller for the local platform, which `alien dev` starts as a process on your machine. It records the zone and makes no cloud call.

## The permission sets

`permissions/dns-zone/` holds the extension's grants on AWS, Google Cloud and Azure. `provision` gives the controller the calls it makes and no others: create the zone, list and delete its records (on Azure they go with the zone), and delete it. `records-write` is what a workload linked to the zone gets, to write its records.

On AWS, both grant their record and delete calls on `arn:aws:route53:::hostedzone/*`, which covers every hosted zone in the account, not only the zone this extension creates. Route53 names a zone by an id it assigns at create, which no binding variable holds yet, and a hosted zone supports no tag condition, so a grant can't name the one zone. `CreateHostedZone` and `ListHostedZonesByName` take no zone ARN at all, so they are granted on `*`.

On Google Cloud both grant on the project, which covers every managed zone in it: Alien binds a Google Cloud grant on the project or on one topic, subscription, repository or reasoning engine, never on one managed zone. On Azure both grant on the deployment's resource group, which covers every DNS zone in it: Azure names a zone by its domain, which no binding variable holds.

## The link

A workload that links a zone gets `records-write` on it and the zone's id in `ACME_ZONE_ID`:

```ts
const api = new alien.Worker("api")
  .code({ type: "image", image: "..." })
  .link(zone)
  .permissions("execution")
  .build()
```

Every zone's link sets the same variable, so a release refuses a workload that links two zones: link each zone from its own workload. A release also refuses a link whose variable the workload sets in its own environment.

On AWS, a link to a Live zone grants `records-write` on that one zone, which only setup can grant. Grant it to the profile for the whole stack instead, which covers the same hosted zones:

```ts
.permissions({ profiles: { execution: { "*": ["acme/dns-zone/records-write"] } } })
```

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
