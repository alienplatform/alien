# TargetReleaseProfileStateGcpResource

GCP-specific binding specification

## Example Usage

```typescript
import { TargetReleaseProfileStateGcpResource } from "@alienplatform/platform-api/models";

let value: TargetReleaseProfileStateGcpResource = {
  scope: "<value>",
};
```

## Fields

| Field                                                    | Type                                                     | Required                                                 | Description                                              |
| -------------------------------------------------------- | -------------------------------------------------------- | -------------------------------------------------------- | -------------------------------------------------------- |
| `condition`                                              | *models.TargetReleaseProfileStateResourceConditionUnion* | :heavy_minus_sign:                                       | N/A                                                      |
| `scope`                                                  | *string*                                                 | :heavy_check_mark:                                       | Scope (project/resource level)                           |