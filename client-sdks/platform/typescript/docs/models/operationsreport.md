# OperationsReport

Operations bundle set the Operator currently has loaded, for plugin-sync status tracking.

## Example Usage

```typescript
import { OperationsReport } from "@alienplatform/platform-api/models";

let value: OperationsReport = {};
```

## Fields

| Field                                                        | Type                                                         | Required                                                     | Description                                                  |
| ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ |
| `loadedBundleHash`                                           | *string*                                                     | :heavy_minus_sign:                                           | N/A                                                          |
| `operations`                                                 | [models.ReportedOperation](../models/reportedoperation.md)[] | :heavy_minus_sign:                                           | N/A                                                          |