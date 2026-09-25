# CreateChildDeploymentRequest

## Example Usage

```typescript
import { CreateChildDeploymentRequest } from "@alienplatform/platform-api/models";

let value: CreateChildDeploymentRequest = {
  name: "<value>",
  version: "<value>",
  stack: {
    id: "<id>",
    resources: {},
  },
};
```

## Fields

| Field                                                                                      | Type                                                                                       | Required                                                                                   | Description                                                                                |
| ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ |
| `name`                                                                                     | *string*                                                                                   | :heavy_check_mark:                                                                         | N/A                                                                                        |
| `version`                                                                                  | *string*                                                                                   | :heavy_check_mark:                                                                         | N/A                                                                                        |
| `stack`                                                                                    | [models.CreateChildDeploymentRequestStack](../models/createchilddeploymentrequeststack.md) | :heavy_check_mark:                                                                         | A bag of resources, unaware of any cloud.                                                  |