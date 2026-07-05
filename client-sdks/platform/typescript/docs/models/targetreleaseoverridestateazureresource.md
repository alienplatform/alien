# TargetReleaseOverrideStateAzureResource

Azure-specific binding specification

## Example Usage

```typescript
import { TargetReleaseOverrideStateAzureResource } from "@alienplatform/platform-api/models";

let value: TargetReleaseOverrideStateAzureResource = {
  scope: "<value>",
};
```

## Fields

| Field                                              | Type                                               | Required                                           | Description                                        |
| -------------------------------------------------- | -------------------------------------------------- | -------------------------------------------------- | -------------------------------------------------- |
| `scope`                                            | *string*                                           | :heavy_check_mark:                                 | Scope (subscription/resource group/resource level) |