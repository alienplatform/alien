# DataVault

## Example Usage

```typescript
import { DataVault } from "@alienplatform/platform-api/models/operations";

let value: DataVault = {
  data: {
    accountId: "<id>",
    parameterMetadataSampled: true,
    prefix: "<value>",
    region: "<value>",
    status: {
      collectionIssues: [],
      health: "unknown",
      lifecycle: "unknown",
      partial: true,
      stale: false,
    },
    backend: "awsParameterStore",
  },
  resourceType: "vault",
};
```

## Fields

| Field                   | Type                    | Required                | Description             |
| ----------------------- | ----------------------- | ----------------------- | ----------------------- |
| `data`                  | *operations.DataUnion8* | :heavy_check_mark:      | N/A                     |
| `resourceType`          | *"vault"*               | :heavy_check_mark:      | N/A                     |