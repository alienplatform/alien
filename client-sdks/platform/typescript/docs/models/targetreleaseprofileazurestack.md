# TargetReleaseProfileAzureStack

Azure-specific binding specification

## Example Usage

```typescript
import { TargetReleaseProfileAzureStack } from "@alienplatform/platform-api/models";

let value: TargetReleaseProfileAzureStack = {
  scope: "<value>",
};
```

## Fields

| Field                                              | Type                                               | Required                                           | Description                                        |
| -------------------------------------------------- | -------------------------------------------------- | -------------------------------------------------- | -------------------------------------------------- |
| `scope`                                            | *string*                                           | :heavy_check_mark:                                 | Scope (subscription/resource group/resource level) |