# CurrentReleaseProfileStateAzureResource

Azure-specific binding specification

## Example Usage

```typescript
import { CurrentReleaseProfileStateAzureResource } from "@alienplatform/platform-api/models";

let value: CurrentReleaseProfileStateAzureResource = {
  scope: "<value>",
};
```

## Fields

| Field                                              | Type                                               | Required                                           | Description                                        |
| -------------------------------------------------- | -------------------------------------------------- | -------------------------------------------------- | -------------------------------------------------- |
| `scope`                                            | *string*                                           | :heavy_check_mark:                                 | Scope (subscription/resource group/resource level) |