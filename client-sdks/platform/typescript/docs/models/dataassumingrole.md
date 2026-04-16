# DataAssumingRole

## Example Usage

```typescript
import { DataAssumingRole } from "@alienplatform/platform-api/models";

let value: DataAssumingRole = {
  roleArn: "<value>",
  type: "AssumingRole",
};
```

## Fields

| Field                     | Type                      | Required                  | Description               |
| ------------------------- | ------------------------- | ------------------------- | ------------------------- |
| `roleArn`                 | *string*                  | :heavy_check_mark:        | ARN of the role to assume |
| `type`                    | *"AssumingRole"*          | :heavy_check_mark:        | N/A                       |