# ProfileReleaseInfoGcpStack

GCP-specific binding specification

## Example Usage

```typescript
import { ProfileReleaseInfoGcpStack } from "@alienplatform/platform-api/models";

let value: ProfileReleaseInfoGcpStack = {
  scope: "<value>",
};
```

## Fields

| Field                                     | Type                                      | Required                                  | Description                               |
| ----------------------------------------- | ----------------------------------------- | ----------------------------------------- | ----------------------------------------- |
| `condition`                               | *models.ProfileReleaseInfoConditionUnion* | :heavy_minus_sign:                        | N/A                                       |
| `scope`                                   | *string*                                  | :heavy_check_mark:                        | Scope (project/resource level)            |