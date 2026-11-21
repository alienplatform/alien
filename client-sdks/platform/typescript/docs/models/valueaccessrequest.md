# ValueAccessRequest

## Example Usage

```typescript
import { ValueAccessRequest } from "@alienplatform/platform-api/models";

let value: ValueAccessRequest = {
  slots: [
    "<value 1>",
    "<value 2>",
    "<value 3>",
  ],
  access: "write",
};
```

## Fields

| Field                                                  | Type                                                   | Required                                               | Description                                            |
| ------------------------------------------------------ | ------------------------------------------------------ | ------------------------------------------------------ | ------------------------------------------------------ |
| `slots`                                                | *string*[]                                             | :heavy_check_mark:                                     | N/A                                                    |
| `access`                                               | [models.ValueAccessMode](../models/valueaccessmode.md) | :heavy_check_mark:                                     | N/A                                                    |