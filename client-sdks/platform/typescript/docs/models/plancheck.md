# PlanCheck

The machine's last plan check. Absent for machines that don't verify releases or haven't reported one.

## Example Usage

```typescript
import { PlanCheck } from "@alienplatform/platform-api/models";

let value: PlanCheck = {
  observedAt: "<value>",
};
```

## Fields

| Field                                                                         | Type                                                                          | Required                                                                      | Description                                                                   |
| ----------------------------------------------------------------------------- | ----------------------------------------------------------------------------- | ----------------------------------------------------------------------------- | ----------------------------------------------------------------------------- |
| `observedAt`                                                                  | *string*                                                                      | :heavy_check_mark:                                                            | N/A                                                                           |
| `refusal`                                                                     | *string*                                                                      | :heavy_minus_sign:                                                            | Why the machine refused the last plan it checked; absent when it accepted it. |