# TargetReleaseExtendGcpStack

GCP-specific binding specification

## Example Usage

```typescript
import { TargetReleaseExtendGcpStack } from "@alienplatform/platform-api/models";

let value: TargetReleaseExtendGcpStack = {
  scope: "<value>",
};
```

## Fields

| Field                                      | Type                                       | Required                                   | Description                                |
| ------------------------------------------ | ------------------------------------------ | ------------------------------------------ | ------------------------------------------ |
| `condition`                                | *models.TargetReleaseExtendConditionUnion* | :heavy_minus_sign:                         | N/A                                        |
| `scope`                                    | *string*                                   | :heavy_check_mark:                         | Scope (project/resource level)             |