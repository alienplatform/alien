# ProfileReleaseInfoAzureResource

Azure-specific binding specification

## Example Usage

```typescript
import { ProfileReleaseInfoAzureResource } from "@aliendotdev/platform-api/models";

let value: ProfileReleaseInfoAzureResource = {
  scope: "<value>",
};
```

## Fields

| Field                                              | Type                                               | Required                                           | Description                                        |
| -------------------------------------------------- | -------------------------------------------------- | -------------------------------------------------- | -------------------------------------------------- |
| `scope`                                            | *string*                                           | :heavy_check_mark:                                 | Scope (subscription/resource group/resource level) |