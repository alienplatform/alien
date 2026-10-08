# EventDataAssumingRole

## Example Usage

```typescript
import { EventDataAssumingRole } from "@alienplatform/platform-api/models";

let value: EventDataAssumingRole = {
  roleArn: "<value>",
  type: "AssumingRole",
};
```

## Fields

| Field                     | Type                      | Required                  | Description               |
| ------------------------- | ------------------------- | ------------------------- | ------------------------- |
| `roleArn`                 | *string*                  | :heavy_check_mark:        | ARN of the role to assume |
| `type`                    | *"AssumingRole"*          | :heavy_check_mark:        | N/A                       |
