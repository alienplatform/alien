# CreateProjectRemoteSandbox

## Example Usage

```typescript
import { CreateProjectRemoteSandbox } from "@alienplatform/platform-api/models/operations";

let value: CreateProjectRemoteSandbox = {
  enabled: true,
};
```

## Fields

| Field                                                                          | Type                                                                           | Required                                                                       | Description                                                                    |
| ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ |
| `enabled`                                                                      | *true*                                                                         | :heavy_check_mark:                                                             | N/A                                                                            |
| `baseImage`                                                                    | *string*                                                                       | :heavy_minus_sign:                                                             | N/A                                                                            |
| `azure`                                                                        | [operations.CreateProjectAzure](../../models/operations/createprojectazure.md) | :heavy_minus_sign:                                                             | N/A                                                                            |
| `maxLifetimeSeconds`                                                           | *number*                                                                       | :heavy_minus_sign:                                                             | N/A                                                                            |