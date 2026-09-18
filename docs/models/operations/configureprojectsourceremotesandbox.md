# ConfigureProjectSourceRemoteSandbox

## Example Usage

```typescript
import { ConfigureProjectSourceRemoteSandbox } from "@alienplatform/platform-api/models/operations";

let value: ConfigureProjectSourceRemoteSandbox = {
  enabled: true,
};
```

## Fields

| Field                                                                                            | Type                                                                                             | Required                                                                                         | Description                                                                                      |
| ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ |
| `enabled`                                                                                        | *boolean*                                                                                        | :heavy_check_mark:                                                                               | N/A                                                                                              |
| `baseImage`                                                                                      | *string*                                                                                         | :heavy_minus_sign:                                                                               | N/A                                                                                              |
| `azure`                                                                                          | [operations.ConfigureProjectSourceAzure](../../models/operations/configureprojectsourceazure.md) | :heavy_minus_sign:                                                                               | N/A                                                                                              |
| `maxLifetimeSeconds`                                                                             | *number*                                                                                         | :heavy_minus_sign:                                                                               | N/A                                                                                              |