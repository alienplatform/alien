# OperationSettingValue3

An environment variable of the Operator process. Only for an Operator
installed on its own, so secrets stay in its environment.

## Example Usage

```typescript
import { OperationSettingValue3 } from "@alienplatform/manager-api/models";

let value: OperationSettingValue3 = {
  env: "<value>",
};
```

## Fields

| Field                      | Type                       | Required                   | Description                |
| -------------------------- | -------------------------- | -------------------------- | -------------------------- |
| `env`                      | *string*                   | :heavy_check_mark:         | Environment variable name. |
