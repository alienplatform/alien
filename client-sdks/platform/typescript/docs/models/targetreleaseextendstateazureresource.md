# TargetReleaseExtendStateAzureResource

Azure-specific binding specification

## Example Usage

```typescript
import { TargetReleaseExtendStateAzureResource } from "@alienplatform/platform-api/models";

let value: TargetReleaseExtendStateAzureResource = {
  scope: "<value>",
};
```

## Fields

| Field                                              | Type                                               | Required                                           | Description                                        |
| -------------------------------------------------- | -------------------------------------------------- | -------------------------------------------------- | -------------------------------------------------- |
| `scope`                                            | *string*                                           | :heavy_check_mark:                                 | Scope (subscription/resource group/resource level) |