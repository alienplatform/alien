# OverrideReleaseInfoGcpStack

GCP-specific binding specification

## Example Usage

```typescript
import { OverrideReleaseInfoGcpStack } from "@alienplatform/platform-api/models";

let value: OverrideReleaseInfoGcpStack = {
  scope: "<value>",
};
```

## Fields

| Field                                      | Type                                       | Required                                   | Description                                |
| ------------------------------------------ | ------------------------------------------ | ------------------------------------------ | ------------------------------------------ |
| `condition`                                | *models.OverrideReleaseInfoConditionUnion* | :heavy_minus_sign:                         | N/A                                        |
| `scope`                                    | *string*                                   | :heavy_check_mark:                         | Scope (project/resource level)             |