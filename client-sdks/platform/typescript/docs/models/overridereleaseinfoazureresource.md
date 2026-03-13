# OverrideReleaseInfoAzureResource

Azure-specific binding specification

## Example Usage

```typescript
import { OverrideReleaseInfoAzureResource } from "@aliendotdev/platform-api/models";

let value: OverrideReleaseInfoAzureResource = {
  scope: "<value>",
};
```

## Fields

| Field                                              | Type                                               | Required                                           | Description                                        |
| -------------------------------------------------- | -------------------------------------------------- | -------------------------------------------------- | -------------------------------------------------- |
| `scope`                                            | *string*                                           | :heavy_check_mark:                                 | Scope (subscription/resource group/resource level) |