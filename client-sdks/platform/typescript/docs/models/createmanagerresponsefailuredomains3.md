# CreateManagerResponseFailureDomains3

Failure-domain policy selected for a compute pool.

## Example Usage

```typescript
import { CreateManagerResponseFailureDomains3 } from "@alienplatform/platform-api/models";

let value: CreateManagerResponseFailureDomains3 = {
  spread: 297029,
};
```

## Fields

| Field                                                                                                                          | Type                                                                                                                           | Required                                                                                                                       | Description                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ |
| `selectedFailureDomains`                                                                                                       | *string*[]                                                                                                                     | :heavy_minus_sign:                                                                                                             | Concrete provider domains selected during setup.<br/>Empty delegates deterministic selection to the provider setup implementation. |
| `spread`                                                                                                                       | *number*                                                                                                                       | :heavy_check_mark:                                                                                                             | Number of distinct failure domains across which new stateful replicas may be spread.                                           |
