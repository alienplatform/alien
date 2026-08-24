# Affected

## Example Usage

```typescript
import { Affected } from "@alienplatform/platform-api/models/operations";

let value: Affected = {
  deploymentGroupId: "<id>",
  name: "<value>",
  externalId: null,
  provider: "<value>",
  reason: "provider-no-longer-compatible",
};
```

## Fields

| Field                                                                                                      | Type                                                                                                       | Required                                                                                                   | Description                                                                                                |
| ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- |
| `deploymentGroupId`                                                                                        | *string*                                                                                                   | :heavy_check_mark:                                                                                         | N/A                                                                                                        |
| `name`                                                                                                     | *string*                                                                                                   | :heavy_check_mark:                                                                                         | N/A                                                                                                        |
| `externalId`                                                                                               | *string*                                                                                                   | :heavy_check_mark:                                                                                         | N/A                                                                                                        |
| `provider`                                                                                                 | *string*                                                                                                   | :heavy_check_mark:                                                                                         | N/A                                                                                                        |
| `reason`                                                                                                   | [operations.PreviewProjectModelsImpactReason](../../models/operations/previewprojectmodelsimpactreason.md) | :heavy_check_mark:                                                                                         | N/A                                                                                                        |