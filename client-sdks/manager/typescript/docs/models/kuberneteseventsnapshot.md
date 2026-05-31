# KubernetesEventSnapshot

## Example Usage

```typescript
import { KubernetesEventSnapshot } from "@alienplatform/manager-api/models";

let value: KubernetesEventSnapshot = {
  message: "<value>",
  reason: "<value>",
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `count`                                                                                       | *number*                                                                                      | :heavy_minus_sign:                                                                            | N/A                                                                                           |
| `eventTime`                                                                                   | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_minus_sign:                                                                            | N/A                                                                                           |
| `firstTimestamp`                                                                              | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_minus_sign:                                                                            | N/A                                                                                           |
| `involvedObject`                                                                              | [models.KubernetesEventInvolvedObject](../models/kuberneteseventinvolvedobject.md)            | :heavy_minus_sign:                                                                            | N/A                                                                                           |
| `lastTimestamp`                                                                               | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_minus_sign:                                                                            | N/A                                                                                           |
| `message`                                                                                     | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `raw`                                                                                         | *any*                                                                                         | :heavy_minus_sign:                                                                            | N/A                                                                                           |
| `reason`                                                                                      | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `source`                                                                                      | [models.KubernetesEventSource](../models/kuberneteseventsource.md)                            | :heavy_minus_sign:                                                                            | N/A                                                                                           |
| `type`                                                                                        | *string*                                                                                      | :heavy_minus_sign:                                                                            | N/A                                                                                           |