# GetProjectAiUsageRequest

## Example Usage

```typescript
import { GetProjectAiUsageRequest } from "@alienplatform/platform-api/models/operations";

let value: GetProjectAiUsageRequest = {
  idOrName: "<value>",
};
```

## Fields

| Field                                                                                                      | Type                                                                                                       | Required                                                                                                   | Description                                                                                                |
| ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- |
| `idOrName`                                                                                                 | *string*                                                                                                   | :heavy_check_mark:                                                                                         | Project ID or name.                                                                                        |
| `range`                                                                                                    | [operations.GetProjectAiUsageQueryParamRange](../../models/operations/getprojectaiusagequeryparamrange.md) | :heavy_minus_sign:                                                                                         | N/A                                                                                                        |