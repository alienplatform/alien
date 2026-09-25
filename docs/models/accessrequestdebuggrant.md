# AccessRequestDebugGrant

## Example Usage

```typescript
import { AccessRequestDebugGrant } from "@alienplatform/platform-api/models";

let value: AccessRequestDebugGrant = {
  tool: "aws",
  namespace: "braintrust",
  cloudScope: "123456789012/prod-readonly",
};
```

## Fields

| Field                                                | Type                                                 | Required                                             | Description                                          | Example                                              |
| ---------------------------------------------------- | ---------------------------------------------------- | ---------------------------------------------------- | ---------------------------------------------------- | ---------------------------------------------------- |
| `tool`                                               | [models.DebugGrantTool](../models/debuggranttool.md) | :heavy_check_mark:                                   | N/A                                                  |                                                      |
| `namespace`                                          | *string*                                             | :heavy_minus_sign:                                   | N/A                                                  | braintrust                                           |
| `cloudScope`                                         | *string*                                             | :heavy_minus_sign:                                   | N/A                                                  | 123456789012/prod-readonly                           |