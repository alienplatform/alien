# CurrentReleaseProfileStateGcpResource

GCP-specific binding specification

## Example Usage

```typescript
import { CurrentReleaseProfileStateGcpResource } from "@alienplatform/platform-api/models";

let value: CurrentReleaseProfileStateGcpResource = {
  scope: "<value>",
};
```

## Fields

| Field                                                     | Type                                                      | Required                                                  | Description                                               |
| --------------------------------------------------------- | --------------------------------------------------------- | --------------------------------------------------------- | --------------------------------------------------------- |
| `condition`                                               | *models.CurrentReleaseProfileStateResourceConditionUnion* | :heavy_minus_sign:                                        | N/A                                                       |
| `scope`                                                   | *string*                                                  | :heavy_check_mark:                                        | Scope (project/resource level)                            |