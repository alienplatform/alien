# OverrideReleaseInfoGcpResource

GCP-specific binding specification

## Example Usage

```typescript
import { OverrideReleaseInfoGcpResource } from "@alienplatform/platform-api/models";

let value: OverrideReleaseInfoGcpResource = {
  scope: "<value>",
};
```

## Fields

| Field                                              | Type                                               | Required                                           | Description                                        |
| -------------------------------------------------- | -------------------------------------------------- | -------------------------------------------------- | -------------------------------------------------- |
| `condition`                                        | *models.OverrideReleaseInfoResourceConditionUnion* | :heavy_minus_sign:                                 | N/A                                                |
| `scope`                                            | *string*                                           | :heavy_check_mark:                                 | Scope (project/resource level)                     |