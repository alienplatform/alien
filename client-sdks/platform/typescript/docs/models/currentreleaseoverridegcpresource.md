# CurrentReleaseOverrideGcpResource

GCP-specific binding specification

## Example Usage

```typescript
import { CurrentReleaseOverrideGcpResource } from "@alienplatform/platform-api/models";

let value: CurrentReleaseOverrideGcpResource = {
  scope: "<value>",
};
```

## Fields

| Field                                                    | Type                                                     | Required                                                 | Description                                              |
| -------------------------------------------------------- | -------------------------------------------------------- | -------------------------------------------------------- | -------------------------------------------------------- |
| `condition`                                              | *models.CurrentReleaseOverrideGcpResourceConditionUnion* | :heavy_minus_sign:                                       | N/A                                                      |
| `scope`                                                  | *string*                                                 | :heavy_check_mark:                                       | Scope (project/resource level)                           |