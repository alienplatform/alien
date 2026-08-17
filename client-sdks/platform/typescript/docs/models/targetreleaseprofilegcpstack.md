# TargetReleaseProfileGcpStack

GCP-specific binding specification

## Example Usage

```typescript
import { TargetReleaseProfileGcpStack } from "@alienplatform/platform-api/models";

let value: TargetReleaseProfileGcpStack = {
  scope: "<value>",
};
```

## Fields

| Field                                       | Type                                        | Required                                    | Description                                 |
| ------------------------------------------- | ------------------------------------------- | ------------------------------------------- | ------------------------------------------- |
| `condition`                                 | *models.TargetReleaseProfileConditionUnion* | :heavy_minus_sign:                          | N/A                                         |
| `scope`                                     | *string*                                    | :heavy_check_mark:                          | Scope (project/resource level)              |