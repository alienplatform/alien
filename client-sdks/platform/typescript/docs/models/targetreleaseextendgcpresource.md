# TargetReleaseExtendGcpResource

GCP-specific binding specification

## Example Usage

```typescript
import { TargetReleaseExtendGcpResource } from "@alienplatform/platform-api/models";

let value: TargetReleaseExtendGcpResource = {
  scope: "<value>",
};
```

## Fields

| Field                                              | Type                                               | Required                                           | Description                                        |
| -------------------------------------------------- | -------------------------------------------------- | -------------------------------------------------- | -------------------------------------------------- |
| `condition`                                        | *models.TargetReleaseExtendResourceConditionUnion* | :heavy_minus_sign:                                 | N/A                                                |
| `scope`                                            | *string*                                           | :heavy_check_mark:                                 | Scope (project/resource level)                     |