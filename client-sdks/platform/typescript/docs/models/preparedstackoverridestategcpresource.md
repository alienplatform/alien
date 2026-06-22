# PreparedStackOverrideStateGcpResource

GCP-specific binding specification

## Example Usage

```typescript
import { PreparedStackOverrideStateGcpResource } from "@alienplatform/platform-api/models";

let value: PreparedStackOverrideStateGcpResource = {
  scope: "<value>",
};
```

## Fields

| Field                                                     | Type                                                      | Required                                                  | Description                                               |
| --------------------------------------------------------- | --------------------------------------------------------- | --------------------------------------------------------- | --------------------------------------------------------- |
| `condition`                                               | *models.PreparedStackOverrideStateResourceConditionUnion* | :heavy_minus_sign:                                        | N/A                                                       |
| `scope`                                                   | *string*                                                  | :heavy_check_mark:                                        | Scope (project/resource level)                            |