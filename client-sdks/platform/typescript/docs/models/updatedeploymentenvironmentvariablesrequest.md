# UpdateDeploymentEnvironmentVariablesRequest

Request schema for updating agent environment variables

## Example Usage

```typescript
import { UpdateDeploymentEnvironmentVariablesRequest } from "@aliendotdev/platform-api/models";

let value: UpdateDeploymentEnvironmentVariablesRequest = {
  variables: [
    {
      name: "<value>",
      value: "<value>",
      type: "plain",
    },
  ],
};
```

## Fields

| Field                                                                                                                            | Type                                                                                                                             | Required                                                                                                                         | Description                                                                                                                      |
| -------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- |
| `variables`                                                                                                                      | [models.UpdateDeploymentEnvironmentVariablesRequestVariable](../models/updatedeploymentenvironmentvariablesrequestvariable.md)[] | :heavy_check_mark:                                                                                                               | Environment variables for the agent                                                                                              |