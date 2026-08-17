# TargetReleaseProfileAzureResource

Azure-specific binding specification

## Example Usage

```typescript
import { TargetReleaseProfileAzureResource } from "@alienplatform/platform-api/models";

let value: TargetReleaseProfileAzureResource = {
  scope: "<value>",
};
```

## Fields

| Field                                              | Type                                               | Required                                           | Description                                        |
| -------------------------------------------------- | -------------------------------------------------- | -------------------------------------------------- | -------------------------------------------------- |
| `scope`                                            | *string*                                           | :heavy_check_mark:                                 | Scope (subscription/resource group/resource level) |