# SyncReconcileResponseFailureDomains2

Failure-domain policy selected for a compute pool.

## Example Usage

```typescript
import { SyncReconcileResponseFailureDomains2 } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseFailureDomains2 = {
  spread: 575572,
};
```

## Fields

| Field                                                                                                                          | Type                                                                                                                           | Required                                                                                                                       | Description                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ |
| `selectedFailureDomains`                                                                                                       | *string*[]                                                                                                                     | :heavy_minus_sign:                                                                                                             | Concrete provider domains selected during setup.<br/>Empty delegates deterministic selection to the provider setup implementation. |
| `spread`                                                                                                                       | *number*                                                                                                                       | :heavy_check_mark:                                                                                                             | Number of distinct failure domains across which new stateful replicas may be spread.                                           |
