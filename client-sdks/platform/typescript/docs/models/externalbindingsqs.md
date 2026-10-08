# ExternalBindingSqs

AWS SQS queue parameters

## Example Usage

```typescript
import { ExternalBindingSqs } from "@alienplatform/platform-api/models";

let value: ExternalBindingSqs = {
  queueUrl: {
    "secretRef": {
      "key": "<key>",
      "name": "<value>",
    },
  },
  service: "sqs",
  type: "queue",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `queueUrl`                                                                                                           | *any*                                                                                                                | :heavy_check_mark:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"sqs"*                                                                                                              | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.TypeQueue1](../models/typequeue1.md)                                                                         | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
