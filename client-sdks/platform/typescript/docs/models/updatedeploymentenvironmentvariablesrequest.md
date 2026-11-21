# UpdateDeploymentEnvironmentVariablesRequest

Request schema for updating deployment environment variables

## Example Usage

```typescript
import { UpdateDeploymentEnvironmentVariablesRequest } from "@alienplatform/platform-api/models";

let value: UpdateDeploymentEnvironmentVariablesRequest = {
  variables: [
    {
      name: "<value>",
      type: "plain",
    },
  ],
};
```

## Fields

| Field                                                                                                                                                                                                                               | Type                                                                                                                                                                                                                                | Required                                                                                                                                                                                                                            | Description                                                                                                                                                                                                                         |
| ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `variables`                                                                                                                                                                                                                         | [models.UpdateDeploymentEnvironmentVariablesRequestVariable](../models/updatedeploymentenvironmentvariablesrequestvariable.md)[]                                                                                                    | :heavy_check_mark:                                                                                                                                                                                                                  | Environment variables for the deployment. Replaces the whole list. A row carries either `value` (older clients) or `valueRef` (the client stored the value in the manager); send an unchanged row's current valueRef back as it is. |