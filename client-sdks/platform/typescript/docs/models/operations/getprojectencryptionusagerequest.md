# GetProjectEncryptionUsageRequest

## Example Usage

```typescript
import { GetProjectEncryptionUsageRequest } from "@alienplatform/platform-api/models/operations";

let value: GetProjectEncryptionUsageRequest = {
  idOrName: "<value>",
};
```

## Fields

| Field                                                                                                                      | Type                                                                                                                       | Required                                                                                                                   | Description                                                                                                                |
| -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- |
| `idOrName`                                                                                                                 | *string*                                                                                                                   | :heavy_check_mark:                                                                                                         | Project ID or name.                                                                                                        |
| `range`                                                                                                                    | [operations.GetProjectEncryptionUsageQueryParamRange](../../models/operations/getprojectencryptionusagequeryparamrange.md) | :heavy_minus_sign:                                                                                                         | N/A                                                                                                                        |