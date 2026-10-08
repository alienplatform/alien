# CurrentReleaseOverrideAzureResource

Azure-specific binding specification

## Example Usage

```typescript
import { CurrentReleaseOverrideAzureResource } from "@alienplatform/platform-api/models";

let value: CurrentReleaseOverrideAzureResource = {
  scope: "<value>",
};
```

## Fields

| Field                                              | Type                                               | Required                                           | Description                                        |
| -------------------------------------------------- | -------------------------------------------------- | -------------------------------------------------- | -------------------------------------------------- |
| `scope`                                            | *string*                                           | :heavy_check_mark:                                 | Scope (subscription/resource group/resource level) |