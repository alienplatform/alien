# PublishChildDeploymentRequest

## Example Usage

```typescript
import { PublishChildDeploymentRequest } from "@alienplatform/platform-api/models";

let value: PublishChildDeploymentRequest = {
  version: "<value>",
  stack: {
    id: "<id>",
    resources: {
      "key": {
        config: {
          id: "<id>",
          type: "<value>",
        },
        dependencies: [],
        lifecycle: "frozen",
      },
    },
  },
};
```

## Fields

| Field                                                                                        | Type                                                                                         | Required                                                                                     | Description                                                                                  |
| -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `version`                                                                                    | *string*                                                                                     | :heavy_check_mark:                                                                           | N/A                                                                                          |
| `stack`                                                                                      | [models.PublishChildDeploymentRequestStack](../models/publishchilddeploymentrequeststack.md) | :heavy_check_mark:                                                                           | A bag of resources, unaware of any cloud.                                                    |