# SyncReconcileResponseExternalBindingsSqs

AWS SQS queue parameters

## Example Usage

```typescript
import { SyncReconcileResponseExternalBindingsSqs } from "@aliendotdev/platform-api/models";

let value: SyncReconcileResponseExternalBindingsSqs = {
  service: "sqs",
  type: "queue",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `queueUrl`                                                                                                           | *any*                                                                                                                | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"sqs"*                                                                                                              | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.SyncReconcileResponseTypeQueue1](../models/syncreconcileresponsetypequeue1.md)                               | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |