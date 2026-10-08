# OperatorCapabilityReport

## Example Usage

```typescript
import { OperatorCapabilityReport } from "@alienplatform/platform-api/models";

let value: OperatorCapabilityReport = {
  key: "<key>",
  state: "granted",
};
```

## Fields

| Field                                                                  | Type                                                                   | Required                                                               | Description                                                            |
| ---------------------------------------------------------------------- | ---------------------------------------------------------------------- | ---------------------------------------------------------------------- | ---------------------------------------------------------------------- |
| `key`                                                                  | *string*                                                               | :heavy_check_mark:                                                     | N/A                                                                    |
| `state`                                                                | [models.OperatorCapabilityState](../models/operatorcapabilitystate.md) | :heavy_check_mark:                                                     | N/A                                                                    |
| `detail`                                                               | *string*                                                               | :heavy_minus_sign:                                                     | N/A                                                                    |