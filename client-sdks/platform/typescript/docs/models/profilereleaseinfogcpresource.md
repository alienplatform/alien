# ProfileReleaseInfoGcpResource

GCP-specific binding specification

## Example Usage

```typescript
import { ProfileReleaseInfoGcpResource } from "@alienplatform/platform-api/models";

let value: ProfileReleaseInfoGcpResource = {
  scope: "<value>",
};
```

## Fields

| Field                                             | Type                                              | Required                                          | Description                                       |
| ------------------------------------------------- | ------------------------------------------------- | ------------------------------------------------- | ------------------------------------------------- |
| `condition`                                       | *models.ProfileReleaseInfoResourceConditionUnion* | :heavy_minus_sign:                                | N/A                                               |
| `scope`                                           | *string*                                          | :heavy_check_mark:                                | Scope (project/resource level)                    |