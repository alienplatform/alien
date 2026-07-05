# OperatorCapabilityReport

Report-only Operator capability status.

## Example Usage

```typescript
import { OperatorCapabilityReport } from "@alienplatform/manager-api/models";

let value: OperatorCapabilityReport = {
  key: "<key>",
  state: "granted",
};
```

## Fields

| Field                                                                  | Type                                                                   | Required                                                               | Description                                                            |
| ---------------------------------------------------------------------- | ---------------------------------------------------------------------- | ---------------------------------------------------------------------- | ---------------------------------------------------------------------- |
| `detail`                                                               | *string*                                                               | :heavy_minus_sign:                                                     | Optional human-readable detail from the Operator.                      |
| `key`                                                                  | *string*                                                               | :heavy_check_mark:                                                     | Stable capability key, such as `k8s-workloads` or `logs`.              |
| `state`                                                                | [models.OperatorCapabilityState](../models/operatorcapabilitystate.md) | :heavy_check_mark:                                                     | State of an Operator capability as observed inside the environment.    |