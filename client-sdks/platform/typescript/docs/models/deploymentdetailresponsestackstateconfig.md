# DeploymentDetailResponseStackStateConfig

Resource that can hold any resource type in the Alien system. All resources share common 'type' and 'id' fields with additional type-specific properties.

## Example Usage

```typescript
import { DeploymentDetailResponseStackStateConfig } from "@aliendotdev/platform-api/models";

let value: DeploymentDetailResponseStackStateConfig = {
  id: "<id>",
  type: "<value>",
};
```

## Fields

| Field                                                                                                                                                                  | Type                                                                                                                                                                   | Required                                                                                                                                                               | Description                                                                                                                                                            |
| ---------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `id`                                                                                                                                                                   | *string*                                                                                                                                                               | :heavy_check_mark:                                                                                                                                                     | The unique identifier for this specific resource instance. Must contain only alphanumeric characters, hyphens, and underscores ([A-Za-z0-9-_]). Maximum 64 characters. |
| `type`                                                                                                                                                                 | *string*                                                                                                                                                               | :heavy_check_mark:                                                                                                                                                     | Resource type identifier that determines the specific kind of resource. This field is used for polymorphic deserialization and resource-specific behavior.             |
| `additionalProperties`                                                                                                                                                 | Record<string, *any*>                                                                                                                                                  | :heavy_minus_sign:                                                                                                                                                     | N/A                                                                                                                                                                    |