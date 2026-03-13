# SyncAcquireResponseExternalBindingsSqs

AWS SQS queue parameters

## Example Usage

```typescript
import { SyncAcquireResponseExternalBindingsSqs } from "@aliendotdev/platform-api/models";

let value: SyncAcquireResponseExternalBindingsSqs = {
  service: "sqs",
  type: "queue",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `queueUrl`                                                                                                           | *any*                                                                                                                | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"sqs"*                                                                                                              | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.SyncAcquireResponseTypeQueue1](../models/syncacquireresponsetypequeue1.md)                                   | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |