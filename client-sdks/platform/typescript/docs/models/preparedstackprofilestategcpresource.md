# PreparedStackProfileStateGcpResource

GCP-specific binding specification

## Example Usage

```typescript
import { PreparedStackProfileStateGcpResource } from "@alienplatform/platform-api/models";

let value: PreparedStackProfileStateGcpResource = {
  scope: "<value>",
};
```

## Fields

| Field                                                    | Type                                                     | Required                                                 | Description                                              |
| -------------------------------------------------------- | -------------------------------------------------------- | -------------------------------------------------------- | -------------------------------------------------------- |
| `condition`                                              | *models.PreparedStackProfileStateResourceConditionUnion* | :heavy_minus_sign:                                       | N/A                                                      |
| `scope`                                                  | *string*                                                 | :heavy_check_mark:                                       | Scope (project/resource level)                           |