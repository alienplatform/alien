# ExtendReleaseInfoAzureStack

Azure-specific binding specification

## Example Usage

```typescript
import { ExtendReleaseInfoAzureStack } from "@alienplatform/platform-api/models";

let value: ExtendReleaseInfoAzureStack = {
  scope: "<value>",
};
```

## Fields

| Field                                              | Type                                               | Required                                           | Description                                        |
| -------------------------------------------------- | -------------------------------------------------- | -------------------------------------------------- | -------------------------------------------------- |
| `scope`                                            | *string*                                           | :heavy_check_mark:                                 | Scope (subscription/resource group/resource level) |