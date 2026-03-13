# ManagerDeploymentResources

## Example Usage

```typescript
import { ManagerDeploymentResources } from "@aliendotdev/platform-api/models";

let value: ManagerDeploymentResources = {
  type: "<value>",
  status: "<value>",
};
```

## Fields

| Field                 | Type                  | Required              | Description           |
| --------------------- | --------------------- | --------------------- | --------------------- |
| `type`                | *string*              | :heavy_check_mark:    | Resource type         |
| `status`              | *string*              | :heavy_check_mark:    | Resource status       |
| `outputs`             | Record<string, *any*> | :heavy_minus_sign:    | Resource outputs      |