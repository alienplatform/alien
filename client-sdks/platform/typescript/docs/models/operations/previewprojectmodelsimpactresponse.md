# PreviewProjectModelsImpactResponse

Model configuration impact.

## Example Usage

```typescript
import { PreviewProjectModelsImpactResponse } from "@alienplatform/platform-api/models/operations";

let value: PreviewProjectModelsImpactResponse = {
  connectedCustomers: 881037,
  affectedCustomers: 980474,
  verificationNeeded: 321894,
  affected: [],
  truncated: false,
};
```

## Fields

| Field                                                        | Type                                                         | Required                                                     | Description                                                  |
| ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ |
| `connectedCustomers`                                         | *number*                                                     | :heavy_check_mark:                                           | N/A                                                          |
| `affectedCustomers`                                          | *number*                                                     | :heavy_check_mark:                                           | N/A                                                          |
| `verificationNeeded`                                         | *number*                                                     | :heavy_check_mark:                                           | N/A                                                          |
| `affected`                                                   | [operations.Affected](../../models/operations/affected.md)[] | :heavy_check_mark:                                           | N/A                                                          |
| `truncated`                                                  | *boolean*                                                    | :heavy_check_mark:                                           | N/A                                                          |