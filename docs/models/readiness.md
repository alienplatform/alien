# Readiness

## Example Usage

```typescript
import { Readiness } from "@alienplatform/platform-api/models";

let value: Readiness = {
  status: "unknown",
  checks: [
    {
      code: "<value>",
      status: "warning",
      message: "<value>",
      checkedAt: "<value>",
    },
  ],
};
```

## Fields

| Field                                                  | Type                                                   | Required                                               | Description                                            |
| ------------------------------------------------------ | ------------------------------------------------------ | ------------------------------------------------------ | ------------------------------------------------------ |
| `status`                                               | [models.ReadinessStatus](../models/readinessstatus.md) | :heavy_check_mark:                                     | N/A                                                    |
| `checks`                                               | [models.Check](../models/check.md)[]                   | :heavy_check_mark:                                     | N/A                                                    |