# TargetDeploymentValueSlot

A value the manager holds, named by slot, with the digest the sender expects.

The slot is `env:NAME` for an environment variable, `input:ID` for a stack
input, or `cenv.<container>.<NAME>` for a dynamic container's variable. An
input's stored value is the JSON text of the input value.

## Example Usage

```typescript
import { TargetDeploymentValueSlot } from "@alienplatform/platform-api/models";

let value: TargetDeploymentValueSlot = {
  digest: "<value>",
  kind: "plain",
  slot: "<value>",
};
```

## Fields

| Field                                                                              | Type                                                                               | Required                                                                           | Description                                                                        |
| ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- |
| `digest`                                                                           | *string*                                                                           | :heavy_check_mark:                                                                 | Keyed digest of the value, as lowercase hex.                                       |
| `envName`                                                                          | *string*                                                                           | :heavy_minus_sign:                                                                 | Variable name, repeated for display. The slot decides the name.                    |
| `kind`                                                                             | [models.TargetDeploymentValueSlotKind](../models/targetdeploymentvalueslotkind.md) | :heavy_check_mark:                                                                 | Type of environment variable                                                       |
| `slot`                                                                             | *string*                                                                           | :heavy_check_mark:                                                                 | `env:NAME`, `input:ID` or `cenv.<container>.<NAME>`.                               |
| `targets`                                                                          | *string*[]                                                                         | :heavy_minus_sign:                                                                 | Target resource patterns for an environment variable. `None` means all resources.  |