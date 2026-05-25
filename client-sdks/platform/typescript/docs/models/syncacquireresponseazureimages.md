# SyncAcquireResponseAzureImages

Azure Horizon host image entry.

## Example Usage

```typescript
import { SyncAcquireResponseAzureImages } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseAzureImages = {
  imageDefinitionId: "<id>",
};
```

## Fields

| Field                                      | Type                                       | Required                                   | Description                                |
| ------------------------------------------ | ------------------------------------------ | ------------------------------------------ | ------------------------------------------ |
| `imageDefinitionId`                        | *string*                                   | :heavy_check_mark:                         | Azure Compute Gallery image definition ID. |