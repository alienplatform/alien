# DataAwsKms

## Example Usage

```typescript
import { DataAwsKms } from "@alienplatform/platform-api/models/operations";

let value: DataAwsKms = {
  data: {
    enabled: true,
    keyArn: "<value>",
    keySpec: "<value>",
    keyState: "<value>",
    keyUsage: "<value>",
    status: {
      health: "healthy",
      lifecycle: "failed",
    },
  },
  provider: "aws-kms",
};
```

## Fields

| Field                                                | Type                                                 | Required                                             | Description                                          |
| ---------------------------------------------------- | ---------------------------------------------------- | ---------------------------------------------------- | ---------------------------------------------------- |
| `data`                                               | [operations.Data6](../../models/operations/data6.md) | :heavy_check_mark:                                   | N/A                                                  |
| `provider`                                           | *"aws-kms"*                                          | :heavy_check_mark:                                   | N/A                                                  |