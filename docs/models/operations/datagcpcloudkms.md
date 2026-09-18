# DataGcpCloudKms

## Example Usage

```typescript
import { DataGcpCloudKms } from "@alienplatform/platform-api/models/operations";

let value: DataGcpCloudKms = {
  data: {
    cryptoKeyName: "<value>",
    purpose: "<value>",
    status: {
      health: "healthy",
      lifecycle: "deleted",
    },
  },
  provider: "gcp-cloud-kms",
};
```

## Fields

| Field                                                | Type                                                 | Required                                             | Description                                          |
| ---------------------------------------------------- | ---------------------------------------------------- | ---------------------------------------------------- | ---------------------------------------------------- |
| `data`                                               | [operations.Data7](../../models/operations/data7.md) | :heavy_check_mark:                                   | N/A                                                  |
| `provider`                                           | *"gcp-cloud-kms"*                                    | :heavy_check_mark:                                   | N/A                                                  |