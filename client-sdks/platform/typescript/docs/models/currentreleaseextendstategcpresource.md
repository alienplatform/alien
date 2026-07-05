# CurrentReleaseExtendStateGcpResource

GCP-specific binding specification

## Example Usage

```typescript
import { CurrentReleaseExtendStateGcpResource } from "@alienplatform/platform-api/models";

let value: CurrentReleaseExtendStateGcpResource = {
  scope: "<value>",
};
```

## Fields

| Field                                                    | Type                                                     | Required                                                 | Description                                              |
| -------------------------------------------------------- | -------------------------------------------------------- | -------------------------------------------------------- | -------------------------------------------------------- |
| `condition`                                              | *models.CurrentReleaseExtendStateResourceConditionUnion* | :heavy_minus_sign:                                       | N/A                                                      |
| `scope`                                                  | *string*                                                 | :heavy_check_mark:                                       | Scope (project/resource level)                           |