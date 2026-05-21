# PreparedDeploymentStackProfileAzure

Azure-specific platform permission configuration

## Example Usage

```typescript
import { PreparedDeploymentStackProfileAzure } from "@alienplatform/platform-api/models";

let value: PreparedDeploymentStackProfileAzure = {
  binding: {},
  grant: {},
};
```

## Fields

| Field                                                                                                        | Type                                                                                                         | Required                                                                                                     | Description                                                                                                  |
| ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ |
| `binding`                                                                                                    | [models.PreparedDeploymentStackProfileAzureBinding](../models/prepareddeploymentstackprofileazurebinding.md) | :heavy_check_mark:                                                                                           | Generic binding configuration for permissions                                                                |
| `grant`                                                                                                      | [models.PreparedDeploymentStackProfileAzureGrant](../models/prepareddeploymentstackprofileazuregrant.md)     | :heavy_check_mark:                                                                                           | Grant permissions for a specific cloud platform                                                              |