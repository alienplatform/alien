# Cluster

Kubernetes cluster setup settings.

## Example Usage

```typescript
import { Cluster } from "@alienplatform/platform-api/models/operations";

let value: Cluster = {
  ownership: "managed",
};
```

## Fields

| Field                                                          | Type                                                           | Required                                                       | Description                                                    |
| -------------------------------------------------------------- | -------------------------------------------------------------- | -------------------------------------------------------------- | -------------------------------------------------------------- |
| `cloud`                                                        | *operations.CloudUnion*                                        | :heavy_minus_sign:                                             | N/A                                                            |
| `namespace`                                                    | *string*                                                       | :heavy_minus_sign:                                             | Namespace where the Alien chart and application resources run. |
| `ownership`                                                    | [operations.Ownership](../../models/operations/ownership.md)   | :heavy_check_mark:                                             | Ownership model for the Kubernetes cluster.                    |