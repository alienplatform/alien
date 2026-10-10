# CpuChoices

Allowed deployment-time resource quantities. These are choices, not autoscaling targets.

## Example Usage

```typescript
import { CpuChoices } from "@alienplatform/platform-api/models";

let value: CpuChoices = {
  default: "<value>",
  max: "<value>",
  min: "<value>",
};
```

## Fields

| Field                                                      | Type                                                       | Required                                                   | Description                                                |
| ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- |
| `default`                                                  | *string*                                                   | :heavy_check_mark:                                         | Allocation used when deployment settings omit a selection. |
| `max`                                                      | *string*                                                   | :heavy_check_mark:                                         | Largest permitted allocation.                              |
| `min`                                                      | *string*                                                   | :heavy_check_mark:                                         | Smallest permitted allocation.                             |
