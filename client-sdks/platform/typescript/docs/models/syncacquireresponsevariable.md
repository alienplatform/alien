# SyncAcquireResponseVariable

Environment variable for deployment

## Example Usage

```typescript
import { SyncAcquireResponseVariable } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseVariable = {
  name: "<value>",
  type: "plain",
  value: "<value>",
};
```

## Fields

| Field                                                                                                          | Type                                                                                                           | Required                                                                                                       | Description                                                                                                    |
| -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- |
| `name`                                                                                                         | *string*                                                                                                       | :heavy_check_mark:                                                                                             | Variable name                                                                                                  |
| `targetResources`                                                                                              | *string*[]                                                                                                     | :heavy_minus_sign:                                                                                             | Target resource patterns (null = all resources, Some = wildcard patterns)                                      |
| `type`                                                                                                         | [models.SyncAcquireResponseEnvironmentVariablesType](../models/syncacquireresponseenvironmentvariablestype.md) | :heavy_check_mark:                                                                                             | Type of environment variable                                                                                   |
| `value`                                                                                                        | *string*                                                                                                       | :heavy_check_mark:                                                                                             | Variable value (decrypted - deployment has access to decryption keys)                                          |